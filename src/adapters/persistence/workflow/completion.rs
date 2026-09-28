use super::*;
use crate::application::workflow::{activation::*, completion::*, lease::*};
use crate::domain::workflow::{ChoiceName, RouteSelection, TransitionTarget};
use serde_json::Value;
use std::time::Duration;

#[derive(sqlx::FromRow)]
struct CompletionRun {
    binding_id: Uuid,
    max_steps: i32,
}
#[derive(sqlx::FromRow)]
struct CompletionExecution {
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    committed_output: Option<Value>,
    committed_route: Option<String>,
    route_target: Option<String>,
    successor_execution_id: Option<Uuid>,
}

#[async_trait]
impl WorkflowCompletion for PostgresPersistence {
    async fn complete_io(
        &self,
        result: FencedWorkflowResult,
    ) -> AppResult<Option<CommittedWorkflowStep>> {
        tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = self.pool.begin().await?;
            // Box at the activation/restore boundary to preserve stock-stack tests.
            let saved = Box::pin(complete_on(&mut tx, result)).await?;
            tx.commit().await?;
            Ok(saved)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
}

pub(super) async fn complete_on(
    db: &mut PgConnection,
    result: FencedWorkflowResult,
) -> AppResult<Option<CommittedWorkflowStep>> {
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
        .execute(&mut *db).await?;
    let scope = result.fence.scope;
    let run = sqlx::query_as::<_, CompletionRun>(
        "SELECT binding_id,max_steps FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    let Some(run) = run else {
        return Ok(None);
    };
    let row = sqlx::query_as::<_, CompletionExecution>("SELECT completed_at,committed_output,committed_route,route_target,successor_execution_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if !lock_job(db, result.fence, &row).await? {
        return Ok(None);
    }
    if row.completed_at.is_some() {
        return replay(db, result, row).await;
    }
    if lease::live_window(db, result.fence, tokio::time::Instant::now())
        .await?
        .is_none()
    {
        return Ok(None);
    }
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let activation = activation::activate_on(db, scope).await?;
    if activation.ordinal > u64::try_from(run.max_steps).map_err(|_| invalid())? {
        return Err(lease::timed_out());
    }
    let completion = prepare_io(binding.bundle(), &activation, result.output)?;
    let successor = batch_commit::complete(
        db,
        scope,
        &activation,
        &completion,
        run.max_steps,
        completion_job::CompletionOwner::Fenced(result.fence),
    )
    .await?;
    Ok(Some(CommittedWorkflowStep {
        disposition: CommitDisposition::Committed,
        output: completion.output,
        route: completion.route,
        target: completion.target,
        successor,
    }))
}

async fn lock_job(
    db: &mut PgConnection,
    fence: WorkflowFence,
    row: &CompletionExecution,
) -> AppResult<bool> {
    let scope = fence.scope;
    let payload: Option<Value> = sqlx::query_scalar("SELECT payload FROM background_tasks WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status=$4 FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid())
        .bind(if row.completed_at.is_some() { "completed" } else { "processing" }).fetch_optional(&mut *db).await?;
    let Some(payload) = payload else {
        return Ok(false);
    };
    if decode_job_payload(payload)? != scope.execution {
        return Ok(false);
    }
    let attempt: Option<Uuid> = sqlx::query_scalar("SELECT id FROM task_attempts WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status=$5 FOR UPDATE")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0)
        .bind(if row.completed_at.is_some() { "completed" } else { "processing" }).fetch_optional(db).await?;
    Ok(attempt.is_some())
}

async fn replay(
    db: &mut PgConnection,
    result: FencedWorkflowResult,
    row: CompletionExecution,
) -> AppResult<Option<CommittedWorkflowStep>> {
    if row.committed_output.as_ref() != Some(&result.output) {
        return Ok(None);
    }
    let route = match row.committed_route.as_deref() {
        Some("success") => RouteSelection::Success,
        Some(value) if value.starts_with("choice:") => {
            RouteSelection::Choice(ChoiceName::parse(&value[7..]).map_err(|_| invalid())?)
        }
        _ => return Err(invalid()),
    };
    let target = match row.route_target.as_deref() {
        Some("$end") => TransitionTarget::End,
        Some(step) => TransitionTarget::Step(StepId::parse(step).map_err(|_| invalid())?),
        None => return Err(invalid()),
    };
    let scope = result.fence.scope;
    let successor = if let Some(execution) = row.successor_execution_id {
        let job = sqlx::query_scalar("SELECT job.id FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE job.company_id=$1 AND execution.run_id=$2 AND execution.id=$3 AND job.queue_kind='workflow'")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(execution).fetch_one(db).await?;
        Some(ActivationRequest {
            execution: ExecutionId::new(execution),
            job: WorkflowJobId(job),
            ..scope
        })
    } else {
        None
    };
    Ok(Some(CommittedWorkflowStep {
        disposition: CommitDisposition::Replayed,
        output: result.output,
        route,
        target,
        successor,
    }))
}
