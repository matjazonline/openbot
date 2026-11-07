use super::*;
use crate::application::workflow::{activation::ActivationRequest, lease::LeaseReleaseCause};

/// A refused final projection fails through the original step's existing owner and error route.
pub(super) async fn settle_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    owner: completion_job::CompletionOwner,
) -> AppResult<()> {
    match owner {
        completion_job::CompletionOwner::Pure => {
            pending_recovery::settle(db, scope, pending_recovery::PendingFailure::InvalidOutput)
                .await?
        }
        completion_job::CompletionOwner::Fenced(fence) => {
            recovery::retire_on(
                db,
                fence,
                lease::Retirement::Interrupted(LeaseReleaseCause::InvalidResult),
            )
            .await?
        }
        completion_job::CompletionOwner::Parked(_)
        | completion_job::CompletionOwner::Approved { .. } => {
            if matches!(owner, completion_job::CompletionOwner::Approved { .. }) {
                completion_job::finish(db, scope, owner).await?;
            }
            let failure = StepFailure::new(
                FailureClass::Terminal,
                FailureCode::parse("message.final_response_refused").map_err(|_| invalid())?,
                None,
            )
            .map_err(|_| invalid())?;
            recovery::route_parked_failure(db, scope, &failure).await?;
        }
        completion_job::CompletionOwner::Recovered => return Err(invalid()),
    }
    Ok(())
}

pub(super) async fn prepare_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    completion: &crate::application::workflow::batch::PureCompletion,
    owner: completion_job::CompletionOwner,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<Option<channel_reply_completion::PreparedFinal>> {
    if completion.state != RunState::Succeeded || completion.target != TransitionTarget::End {
        return Ok(Some(channel_reply_completion::PreparedFinal::Unowned));
    }
    sqlx::query("SAVEPOINT channel_final_stage")
        .execute(&mut *db)
        .await?;
    let staged = Box::pin(channel_reply_completion::prepare_on(
        db, scope, completion, owner, notices,
    ))
    .await;
    let prepared = match staged {
        Ok(prepared) => Some(prepared),
        Err(
            AppError::BadRequest(_) | AppError::NotFound(_) | AppError::KnowledgeRelease { .. },
        ) => {
            sqlx::query("ROLLBACK TO SAVEPOINT channel_final_stage")
                .execute(&mut *db)
                .await?;
            Box::pin(settle_on(db, scope, owner)).await?;
            None
        }
        Err(error) => return Err(error),
    };
    sqlx::query("RELEASE SAVEPOINT channel_final_stage")
        .execute(db)
        .await?;
    Ok(prepared)
}
