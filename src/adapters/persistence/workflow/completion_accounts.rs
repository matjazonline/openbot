//! Read the actual completion purpose before owning one sorted account/company prefix.
use super::*;
use crate::application::workflow::actions::ActionScope;
use crate::domain::workflow::ResponseDisposition;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    Company,
    Reply,
}

async fn purpose_on(db: &mut PgConnection, result: &FencedWorkflowResult) -> AppResult<Purpose> {
    let scope = result.fence.scope;
    if !channel_reply_completion::pending_reply_on(db, scope).await? {
        return Ok(Purpose::Company);
    }
    let Some(run) = Box::pin(run_on(db, scope, CompletionRead::Observe)).await? else {
        return Ok(Purpose::Company);
    };
    let Some(row) = Box::pin(execution_on(db, scope, CompletionRead::Observe)).await? else {
        return Ok(Purpose::Company);
    };
    if row.completed_at.is_some()
        || !job_on(db, result.fence, &row, CompletionRead::Observe).await?
        || lease::live_window(db, result.fence, tokio::time::Instant::now())
            .await?
            .is_none()
        || token_budget::observe_scope(db, scope)
            .await?
            .progression()?
            != token_budget::Progression::Open
        || action_uncertainty::conflicted_on(
            db,
            ActionScope {
                company: scope.company,
                run: scope.run,
                execution: scope.execution,
            },
        )
        .await?
    {
        return Ok(Purpose::Company);
    }
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let Some(activation) = Box::pin(activation::preview_on(db, scope)).await? else {
        return Ok(Purpose::Company);
    };
    let completion = match prepare_result(
        binding.bundle(),
        &activation,
        result.output.clone(),
        run.max_steps,
    ) {
        Ok(completion) => completion,
        // The actual owner retires this invalid result without producing a final reply.
        Err(_) => return Ok(Purpose::Company),
    };
    Ok(
        if channel_reply_completion::final_response_disposition(&completion)
            == Some(ResponseDisposition::Automatic)
        {
            Purpose::Reply
        } else {
            Purpose::Company
        },
    )
}

pub(super) async fn prelock_on(
    db: &mut PgConnection,
    result: &FencedWorkflowResult,
) -> AppResult<Purpose> {
    let scope = result.fence.scope;
    let discovered = Box::pin(purpose_on(db, result)).await?;
    let owned = match discovered {
        Purpose::Company => Purpose::Company,
        Purpose::Reply => {
            let prelocked = action_authority::prelock_reply_on(
                db,
                ActionScope {
                    company: scope.company,
                    run: scope.run,
                    execution: scope.execution,
                },
                None,
            )
            .await;
            match prelocked {
                Ok(_) => Purpose::Reply,
                Err(error) => {
                    // Another owner may have completed/retired this while a live mapping
                    // disappeared. A no-effect downgrade uses only already held accounts.
                    if matches!(&error, AppError::NotFound(_))
                        && Box::pin(purpose_on(db, result)).await? == Purpose::Company
                    {
                        Purpose::Company
                    } else {
                        return Err(error);
                    }
                }
            }
        }
    };
    lease::lock_lease_company_on(db, scope.company).await?;
    if owned == Purpose::Company && Box::pin(purpose_on(db, result)).await? == Purpose::Reply {
        return Err(changed());
    }
    Ok(owned)
}

pub(super) async fn ensure_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    owned: Purpose,
    completion: &crate::application::workflow::batch::PureCompletion,
) -> AppResult<()> {
    if owned == Purpose::Company
        && channel_reply_completion::final_response_disposition(completion)
            == Some(ResponseDisposition::Automatic)
        && channel_reply_completion::root_on(db, scope)
            .await?
            .is_some()
    {
        return Err(changed());
    }
    Ok(())
}

fn changed() -> AppError {
    AppError::Conflict("Workflow completion purpose changed; retry".into())
}
