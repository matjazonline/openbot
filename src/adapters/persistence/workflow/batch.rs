use super::*;
use crate::application::workflow::{activation::*, batch::*};
use std::time::Instant;

#[derive(sqlx::FromRow)]
struct BatchRun {
    binding_id: Uuid,
    state: String,
    max_steps: i32,
    expired: bool,
}
#[derive(sqlx::FromRow)]
struct BatchExecution {
    step_id: String,
    response_disposition: String,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    committed_route: Option<String>,
    route_target: Option<String>,
    successor_execution_id: Option<Uuid>,
}
#[derive(sqlx::FromRow)]
struct BatchJob {
    status: String,
    eligible: bool,
}

#[async_trait]
impl WorkflowBatch for PostgresPersistence {
    async fn advance_pure(
        &self,
        request: ActivationRequest,
        budget: BatchBudget,
    ) -> AppResult<BatchResult> {
        // Cancel the transaction future itself, so an expired budget cannot
        // continue progressing in detached work. SQLx rolls back dropped txs.
        tokio::time::timeout(budget.transaction_timeout(), async {
            let mut tx = self.pool.begin().await?;
            match advance_on(&mut tx, request, budget, self.channel_reply_notices()).await {
                Ok(result) => {
                    tx.commit().await?;
                    Ok(result)
                }
                Err(error) => {
                    tx.rollback().await?;
                    Err(error)
                }
            }
        })
        .await
        .map_err(|_| AppError::Conflict("Workflow batch transaction timed out".into()))?
    }
}

pub(super) async fn advance_on(
    db: &mut PgConnection,
    request: ActivationRequest,
    budget: BatchBudget,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<BatchResult> {
    let started = Instant::now();
    let timeout = format!("{}ms", budget.transaction_timeout().as_millis());
    sqlx::query(
        "SELECT set_config('lock_timeout', $1, true), set_config('statement_timeout', $1, true)",
    )
    .bind(timeout)
    .execute(&mut *db)
    .await?;
    let progress = Box::pin(channel_reply_resume::resume_on(db, request, notices)).await?;
    if let Some(saved) = finalization_result(request, progress) {
        return Ok(saved);
    }
    // Preparation may discover a final response after several pure steps. Roll
    // that entire preparation back before acquiring any newly required account.
    sqlx::query("SAVEPOINT workflow_batch_accounts")
        .execute(&mut *db)
        .await?;
    let prepared = Box::pin(run_attempt(
        db,
        request,
        budget,
        started,
        BatchAccounts::Company,
        notices,
    ))
    .await?;
    let result = match prepared {
        BatchProgress::Finished(result) => *result,
        BatchProgress::ReplyRequired => {
            sqlx::query("ROLLBACK TO SAVEPOINT workflow_batch_accounts")
                .execute(&mut *db)
                .await?;
            // The account prefix and the second actual attempt stay boxed on stock stacks.
            let unavailable = Box::pin(reply_accounts_on(db, request)).await?;
            let accounts = if unavailable.is_some() {
                BatchAccounts::Company
            } else {
                BatchAccounts::Reply
            };
            match Box::pin(run_attempt(db, request, budget, started, accounts, notices)).await? {
                BatchProgress::Finished(result) => *result,
                BatchProgress::ReplyRequired => return Err(unavailable.unwrap_or_else(invalid)),
            }
        }
    };
    sqlx::query("RELEASE SAVEPOINT workflow_batch_accounts")
        .execute(db)
        .await?;
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BatchAccounts {
    Company,
    Reply,
}

enum BatchProgress {
    Finished(Box<BatchResult>),
    ReplyRequired,
}

impl BatchProgress {
    fn finished(result: BatchResult) -> Self {
        Self::Finished(Box::new(result))
    }
}

async fn reply_accounts_on(
    db: &mut PgConnection,
    request: ActivationRequest,
) -> AppResult<Option<AppError>> {
    let result = action_authority::prelock_reply_on(
        db,
        crate::application::workflow::actions::ActionScope {
            company: request.company,
            run: request.run,
            execution: request.execution,
        },
        None,
    )
    .await;
    match result {
        Ok(_) => Ok(None),
        // Under company, the actual second pass may now yield or replay instead
        // of publishing. Only a still-required final effect returns this refusal.
        Err(error @ AppError::NotFound(_)) => Ok(Some(error)),
        Err(error) => Err(error),
    }
}

async fn run_attempt(
    db: &mut PgConnection,
    request: ActivationRequest,
    budget: BatchBudget,
    started: Instant,
    accounts: BatchAccounts,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<BatchProgress> {
    lease::lock_lease_company_on(db, request.company).await?;
    let run = sqlx::query_as::<_, BatchRun>(
        "SELECT binding_id, state, max_steps, deadline <= clock_timestamp() AS expired \
         FROM workflow_runs WHERE company_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(request.company.as_uuid())
    .bind(request.run.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    let first = lock_execution(db, request).await?;
    if first.completed_at.is_some() {
        return replay(db, request, &first)
            .await
            .map(BatchProgress::finished);
    }
    if run.expired || !matches!(run.state.as_str(), "queued" | "running") {
        return Err(ineligible());
    }
    let binding = admission_binding::read_saved_binding(
        db,
        request.company,
        WorkflowBindingId::new(run.binding_id),
        request.run.as_uuid(),
    )
    .await?;
    // Keep the transaction/activation chain shallow in stock-stack debug tests.
    Box::pin(run_steps(
        db,
        BatchContext {
            request,
            budget,
            started,
            max_steps: run.max_steps,
            accounts,
            notices,
        },
        binding.bundle(),
        first,
    ))
    .await
}

fn finalization_result(
    request: ActivationRequest,
    progress: crate::application::workflow::channel_reply::FinalizationProgress,
) -> Option<BatchResult> {
    match progress {
        crate::application::workflow::channel_reply::FinalizationProgress::Unstaged => None,
        crate::application::workflow::channel_reply::FinalizationProgress::Completed(step) => {
            Some(BatchResult {
                disposition: if step.disposition
                    == crate::application::workflow::completion::CommitDisposition::Replayed
                {
                    BatchDisposition::Replay
                } else {
                    BatchDisposition::Completed
                },
                response_disposition: Some(step.response_disposition),
                completed: u8::from(
                    step.disposition
                        == crate::application::workflow::completion::CommitDisposition::Committed,
                ),
                last: request,
                continuation: None,
            })
        }
        _ => Some(BatchResult {
            disposition: BatchDisposition::Deferred,
            response_disposition: None,
            completed: 0,
            last: request,
            continuation: None,
        }),
    }
}

struct BatchContext<'a> {
    notices: Option<&'a crate::application::workflow::decisions::DecisionNotices>,
    request: ActivationRequest,
    budget: BatchBudget,
    started: Instant,
    max_steps: i32,
    accounts: BatchAccounts,
}

async fn run_steps(
    db: &mut PgConnection,
    context: BatchContext<'_>,
    bundle: &PublishedBundle,
    first: BatchExecution,
) -> AppResult<BatchProgress> {
    let mut current = context.request;
    let mut row = first;
    let mut completed = 0;
    loop {
        let step = StepId::parse(row.step_id.clone()).map_err(|_| invalid())?;
        if !is_pure(bundle, &step)? {
            batch_commit::check_deadline(db, context.request).await?;
            return Ok(BatchProgress::finished(BatchResult {
                disposition: BatchDisposition::Boundary,
                response_disposition: None,
                completed,
                last: current,
                continuation: Some(current),
            }));
        }
        // Box the actual preparation/commit phase; the account guard runs before final effects.
        let (response, next) = match Box::pin(advance_step(db, current, bundle, &context)).await? {
            StepProgress::Advanced { response, next } => (response, next),
            StepProgress::Failed => return Ok(BatchProgress::finished(failed(current, completed))),
            StepProgress::Deferred => {
                return Ok(BatchProgress::finished(deferred(current, completed)));
            }
            StepProgress::ReplyRequired => return Ok(BatchProgress::ReplyRequired),
        };
        completed += 1;
        if let Some(result) = progression_result(&context, current, completed, response, next) {
            batch_commit::check_deadline(db, context.request).await?;
            return Ok(BatchProgress::finished(result));
        }
        current = next.ok_or_else(invalid)?;
        row = lock_execution(db, current).await?;
    }
}

enum StepProgress {
    Advanced {
        response: crate::domain::workflow::ResponseDisposition,
        next: Option<ActivationRequest>,
    },
    Failed,
    Deferred,
    ReplyRequired,
}

async fn advance_step(
    db: &mut PgConnection,
    current: ActivationRequest,
    bundle: &PublishedBundle,
    context: &BatchContext<'_>,
) -> AppResult<StepProgress> {
    let (activated, completion) =
        match pending_recovery::prepare_pure(db, current, bundle, context.max_steps).await? {
            pending_recovery::Pending::Ready(prepared) => prepared,
            pending_recovery::Pending::Settled => return Ok(StepProgress::Failed),
            pending_recovery::Pending::Deferred => return Ok(StepProgress::Deferred),
        };
    if context.accounts == BatchAccounts::Company
        && channel_reply_completion::final_response_disposition(&completion)
            == Some(crate::domain::workflow::ResponseDisposition::Automatic)
        && channel_reply_completion::pending_reply_on(db, current).await?
    {
        return Ok(StepProgress::ReplyRequired);
    }
    let progress = batch_commit::complete(
        db,
        current,
        batch_commit::CompletedStep {
            activated: &activated,
            graph: bundle.compiled().graph(),
        },
        &completion,
        context.max_steps,
        completion_job::CompletionOwner::Pure,
        context.notices,
    )
    .await?;
    Ok(match progress {
        batch_commit::CompletionProgress::Refused => StepProgress::Failed,
        batch_commit::CompletionProgress::Parked => StepProgress::Deferred,
        batch_commit::CompletionProgress::Advanced(next) => StepProgress::Advanced {
            response: completion.response_disposition,
            next,
        },
    })
}

fn progression_result(
    context: &BatchContext<'_>,
    current: ActivationRequest,
    completed: u8,
    response: crate::domain::workflow::ResponseDisposition,
    next: Option<ActivationRequest>,
) -> Option<BatchResult> {
    let disposition = if next.is_none() {
        BatchDisposition::Completed
    } else if completed >= context.budget.steps()
        || context.started.elapsed() >= context.budget.work()
    {
        BatchDisposition::Yielded
    } else {
        return None;
    };
    Some(BatchResult {
        disposition,
        response_disposition: next.is_none().then_some(response),
        completed,
        last: current,
        continuation: next,
    })
}

async fn lock_execution(
    db: &mut PgConnection,
    request: ActivationRequest,
) -> AppResult<BatchExecution> {
    let row = sqlx::query_as::<_, BatchExecution>(
        "SELECT step_id, response_disposition, completed_at, committed_route, route_target, successor_execution_id \
         FROM workflow_executions WHERE company_id = $1 AND run_id = $2 AND id = $3 FOR UPDATE",
    )
    .bind(request.company.as_uuid())
    .bind(request.run.as_uuid())
    .bind(request.execution.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    let job = sqlx::query_as::<_, BatchJob>(
        "SELECT status, (status = 'pending' AND retry_count < max_retries AND run_at <= clock_timestamp() \
         AND worker_id IS NULL AND execution_generation IS NULL AND locked_at IS NULL AND lock_expires_at IS NULL) AS eligible \
         FROM background_tasks WHERE company_id = $1 AND id = $2 AND workflow_execution_id = $3 \
          FOR UPDATE",
    ).bind(request.company.as_uuid()).bind(request.job.0).bind(request.execution.as_uuid())
        .fetch_optional(db).await?.ok_or_else(missing)?;
    if row.completed_at.is_some() {
        if job.status != "completed"
            && !(job.status == "failed" && row.committed_route.as_deref() == Some("final_error"))
        {
            return Err(ineligible());
        }
    } else if !job.eligible {
        return Err(ineligible());
    }
    Ok(row)
}

async fn replay(
    db: &mut PgConnection,
    request: ActivationRequest,
    row: &BatchExecution,
) -> AppResult<BatchResult> {
    if row.committed_route.is_none() || row.route_target.is_none() {
        return Err(invalid());
    }
    let continuation = if let Some(execution) = row.successor_execution_id {
        let job = sqlx::query_scalar(
            "SELECT id FROM background_tasks WHERE company_id = $1 AND workflow_execution_id = $2 ",
        )
        .bind(request.company.as_uuid())
        .bind(execution)
        .fetch_one(db)
        .await?;
        Some(ActivationRequest {
            execution: ExecutionId::new(execution),
            job: WorkflowJobId(job),
            ..request
        })
    } else {
        None
    };
    Ok(BatchResult {
        disposition: BatchDisposition::Replay,
        response_disposition: if row.route_target.as_deref() == Some("$end")
            && row.committed_route.as_deref() != Some("final_error")
        {
            Some(row.response_disposition.parse().map_err(|_| invalid())?)
        } else {
            None
        },
        completed: 0,
        last: request,
        continuation,
    })
}
fn ineligible() -> AppError {
    AppError::Conflict("Workflow batch is not runnable".into())
}

fn failed(last: ActivationRequest, completed: u8) -> BatchResult {
    BatchResult {
        disposition: BatchDisposition::Failed,
        response_disposition: None,
        completed,
        last,
        continuation: None,
    }
}

fn deferred(last: ActivationRequest, completed: u8) -> BatchResult {
    BatchResult {
        disposition: BatchDisposition::Deferred,
        response_disposition: None,
        completed,
        last,
        continuation: None,
    }
}
