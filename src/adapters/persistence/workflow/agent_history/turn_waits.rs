//! A blocked logical turn uses the existing job suspension and continuation owner.
use super::*;
use crate::application::workflow::{activation::ActivationRequest, waits::WaitProgress};
use crate::domain::workflow::WaitId;

pub(in crate::adapters::persistence::workflow) async fn parked_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
) -> AppResult<Option<WaitId>> {
    // This is immutable acknowledgement evidence, also valid after a fast resume.
    let wait: Option<Uuid> = sqlx::query_scalar(
        "SELECT wait.id FROM workflow_waits AS wait \
         JOIN agent_history_exchanges AS exchange ON exchange.company_id=wait.company_id \
             AND exchange.id=wait.agent_exchange_id AND exchange.run_id=wait.run_id \
             AND exchange.execution_id=wait.execution_id \
         JOIN task_attempts AS attempt ON attempt.task_id=exchange.job_id \
             AND attempt.attempt_number=wait.parked_attempt_number \
         WHERE wait.company_id=$1 AND wait.run_id=$2 AND wait.execution_id=$3 \
             AND exchange.job_id=$4 AND wait.parked_attempt_number=$5 AND wait.reason='agent_turn' \
             AND attempt.worker_id=$6 AND attempt.execution_generation=$7 \
             AND attempt.status='completed' AND attempt.stop_reason='agent_turn_parked'",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(fence.scope.run.as_uuid())
    .bind(fence.scope.execution.as_uuid())
    .bind(fence.scope.job.0)
    .bind(fence.attempt.0)
    .bind(fence.worker.0)
    .bind(fence.generation.0)
    .fetch_optional(db)
    .await?;
    Ok(wait.map(WaitId::new))
}

/// Acknowledgement only: no history, credentials or provider permission is returned.
pub(super) async fn receipt_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
) -> AppResult<Option<WaitId>> {
    let source: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR SHARE")
            .bind(fence.scope.company.as_uuid())
            .bind(fence.scope.run.as_uuid())
            .fetch_optional(&mut *db)
            .await?;
    if source.is_none() {
        return Ok(None);
    }
    parked_on(db, fence).await
}

pub(super) async fn park_on(db: &mut PgConnection, fence: WorkflowFence) -> AppResult<WaitId> {
    let scope = fence.scope;
    let remaining: bool = sqlx::query_scalar(
        "SELECT continuation_count<workflow_job_continuation_limit() FROM background_tasks \
         WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.job.0)
    .fetch_one(&mut *db)
    .await?;
    if !remaining {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::Budget,
        ));
    }
    let wait = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,agent_exchange_id,parked_attempt_number) \
         SELECT company_id,id,$3,$4,'agent_turn',deadline,$3,$5 FROM workflow_runs WHERE company_id=$1 AND id=$2",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(wait).bind(fence.attempt.0).execute(&mut *db).await?;
    close_attempt(db, fence).await?;
    waits::audit_wait(db, scope, wait, "wait_parked").await?;
    Ok(WaitId::new(wait))
}

async fn close_attempt(db: &mut PgConnection, fence: WorkflowFence) -> AppResult<()> {
    let scope = fence.scope;
    let attempt = sqlx::query(
        "UPDATE task_attempts SET status='completed',stop_reason='agent_turn_parked',finished_at=clock_timestamp() \
         WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status='processing'",
    ).bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0)
        .execute(&mut *db).await?;
    let job = sqlx::query(
        "UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL, \
         execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 \
         AND workflow_execution_id=$3 AND status='processing' AND worker_id=$4 AND execution_generation=$5 \
         AND attempt_ordinal=$6 AND continuation_count<workflow_job_continuation_limit() AND lock_expires_at>clock_timestamp()",
    ).bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid())
        .bind(fence.worker.0).bind(fence.generation.0).bind(fence.attempt.0).execute(&mut *db).await?;
    if attempt.rows_affected() != 1 || job.rows_affected() != 1 {
        return Err(ownership_lost());
    }
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='agent_turn' WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(db).await?;
    Ok(())
}

pub(in crate::adapters::persistence::workflow) async fn resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
) -> AppResult<WaitProgress> {
    // waits::lock already owns this run, execution and waiting job; no global locks follow.
    let exchange: Option<Uuid> = sqlx::query_scalar(
        "SELECT exchange.id FROM workflow_waits AS wait JOIN agent_history_exchanges AS exchange \
         ON exchange.company_id=wait.company_id AND exchange.id=wait.agent_exchange_id \
         WHERE wait.company_id=$1 AND wait.run_id=$2 AND wait.execution_id=$3 AND wait.id=$4 \
             AND wait.reason='agent_turn' AND wait.state='waiting' AND exchange.job_id=$5 AND exchange.state='queued'",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(wait).bind(scope.job.0).fetch_optional(&mut *db).await?;
    let Some(exchange) = exchange else {
        return Ok(WaitProgress::Refused);
    };
    if !turns::acquire_on(db, scope.company, AgentExchangeId::new(exchange)).await? {
        return Ok(WaitProgress::Pending);
    }
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(wait).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2 AND state='waiting' AND waiting_reason='agent_turn'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    decisions::approvals::reopen_job(db, scope).await?;
    waits::audit_wait(db, scope, wait, "wait_completed").await?;
    Ok(WaitProgress::Continued)
}
