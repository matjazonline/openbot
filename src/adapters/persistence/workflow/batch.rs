use super::*;
use crate::application::workflow::{activation::*, batch::*};
use serde_json::Value;
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
    activation: i64,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    committed_route: Option<String>,
    route_target: Option<String>,
    successor_execution_id: Option<Uuid>,
}
#[derive(sqlx::FromRow)]
struct BatchJob {
    payload: Value,
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
            let result = advance_on(&mut tx, request, budget).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
        .map_err(|_| AppError::Conflict("Workflow batch transaction timed out".into()))?
    }
}

pub(super) async fn advance_on(
    db: &mut PgConnection,
    request: ActivationRequest,
    budget: BatchBudget,
) -> AppResult<BatchResult> {
    let started = Instant::now();
    let timeout = format!("{}ms", budget.transaction_timeout().as_millis());
    sqlx::query(
        "SELECT set_config('lock_timeout', $1, true), set_config('statement_timeout', $1, true)",
    )
    .bind(timeout)
    .execute(&mut *db)
    .await?;
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
        return replay(db, request, &first).await;
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
        },
        binding.bundle(),
        first,
    ))
    .await
}

struct BatchContext {
    request: ActivationRequest,
    budget: BatchBudget,
    started: Instant,
    max_steps: i32,
}

async fn run_steps(
    db: &mut PgConnection,
    context: BatchContext,
    bundle: &PublishedBundle,
    first: BatchExecution,
) -> AppResult<BatchResult> {
    let BatchContext {
        request,
        budget,
        started,
        max_steps,
    } = context;
    let mut current = request;
    let mut row = first;
    let mut completed = 0;
    loop {
        let step = StepId::parse(row.step_id.clone()).map_err(|_| invalid())?;
        if !is_pure(bundle, &step)? {
            batch_commit::check_deadline(db, request).await?;
            return Ok(BatchResult {
                disposition: BatchDisposition::Boundary,
                completed,
                last: current,
                continuation: Some(current),
            });
        }
        if row.activation > i64::from(max_steps) {
            return Err(ineligible());
        }
        let activated = activation::activate_on(db, current).await?;
        let completion = execute_pure(bundle, &activated)?.ok_or_else(invalid)?;
        let next = batch_commit::complete(db, current, &activated, &completion, max_steps).await?;
        completed += 1;
        let Some(next) = next else {
            batch_commit::check_deadline(db, request).await?;
            return Ok(BatchResult {
                disposition: BatchDisposition::Completed,
                completed,
                last: current,
                continuation: None,
            });
        };
        if completed >= budget.steps() || started.elapsed() >= budget.work() {
            batch_commit::check_deadline(db, request).await?;
            return Ok(BatchResult {
                disposition: BatchDisposition::Yielded,
                completed,
                last: current,
                continuation: Some(next),
            });
        }
        current = next;
        row = lock_execution(db, current).await?;
    }
}

async fn lock_execution(
    db: &mut PgConnection,
    request: ActivationRequest,
) -> AppResult<BatchExecution> {
    let row = sqlx::query_as::<_, BatchExecution>(
        "SELECT step_id, activation, completed_at, committed_route, route_target, successor_execution_id \
         FROM workflow_executions WHERE company_id = $1 AND run_id = $2 AND id = $3 FOR UPDATE",
    ).bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(request.execution.as_uuid())
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let job = sqlx::query_as::<_, BatchJob>(
        "SELECT payload, status, (status = 'pending' AND run_at <= clock_timestamp() \
         AND worker_id IS NULL AND execution_generation IS NULL AND locked_at IS NULL AND lock_expires_at IS NULL) AS eligible \
         FROM background_tasks WHERE company_id = $1 AND id = $2 AND workflow_execution_id = $3 \
         AND queue_kind = 'workflow' FOR UPDATE",
    ).bind(request.company.as_uuid()).bind(request.job.0).bind(request.execution.as_uuid())
        .fetch_optional(db).await?.ok_or_else(missing)?;
    if decode_job_payload(job.payload)? != request.execution {
        return Err(invalid());
    }
    if row.completed_at.is_some() {
        if job.status != "completed" {
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
        let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id = $1 AND workflow_execution_id = $2 AND queue_kind = 'workflow'")
            .bind(request.company.as_uuid()).bind(execution).fetch_one(db).await?;
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
        completed: 0,
        last: request,
        continuation,
    })
}
fn ineligible() -> AppError {
    AppError::Conflict("Workflow batch is not runnable".into())
}
