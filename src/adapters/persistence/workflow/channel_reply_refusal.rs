use super::*;
use crate::application::workflow::{activation::ActivationRequest, lease::LeaseReleaseCause};
use crate::domain::workflow::KnowledgeRefusal;

/// Preserve the release cause through the original step owner. Knowledge failures never route.
pub(super) async fn settle_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    owner: completion_job::CompletionOwner,
    refusal: Option<KnowledgeRefusal>,
) -> AppResult<()> {
    match owner {
        completion_job::CompletionOwner::Pure => {
            let failure = refusal.map_or(pending_recovery::PendingFailure::InvalidOutput, |_| {
                pending_recovery::PendingFailure::KnowledgeRefused
            });
            pending_recovery::settle(db, scope, failure).await?
        }
        completion_job::CompletionOwner::Fenced(fence) => {
            let cause = match refusal {
                Some(refusal) => LeaseReleaseCause::Classified(
                    recovery::classify_final_refusal_on(db, scope, refusal).await?,
                ),
                None => LeaseReleaseCause::InvalidResult,
            };
            recovery::retire_on(db, fence, lease::Retirement::Interrupted(cause)).await?
        }
        completion_job::CompletionOwner::Parked(_)
        | completion_job::CompletionOwner::Approved { .. } => {
            if matches!(owner, completion_job::CompletionOwner::Approved { .. }) {
                completion_job::finish(db, scope, owner).await?;
            }
            let failure = match refusal {
                Some(refusal) => StepFailure::from_knowledge_refusal(refusal),
                None => StepFailure::new(
                    FailureClass::Terminal,
                    FailureCode::parse("message.final_response_refused").map_err(|_| invalid())?,
                    None,
                )
                .map_err(|_| invalid())?,
            };
            recovery::route_parked_failure(db, scope, &failure).await?;
            if refusal.is_some() {
                waits::audit(db, scope, failure.code().as_str()).await?;
            }
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
            error @ (AppError::BadRequest(_)
            | AppError::NotFound(_)
            | AppError::KnowledgeRelease { .. }),
        ) => {
            sqlx::query("ROLLBACK TO SAVEPOINT channel_final_stage")
                .execute(&mut *db)
                .await?;
            Box::pin(settle_on(db, scope, owner, knowledge_refusal(&error))).await?;
            None
        }
        Err(error) => return Err(error),
    };
    sqlx::query("RELEASE SAVEPOINT channel_final_stage")
        .execute(db)
        .await?;
    Ok(prepared)
}

pub(super) fn knowledge_refusal(error: &AppError) -> Option<KnowledgeRefusal> {
    match error {
        AppError::KnowledgeRelease {
            content,
            destination,
        } => Some(KnowledgeRefusal {
            content: *content,
            destination: *destination,
        }),
        _ => None,
    }
}
