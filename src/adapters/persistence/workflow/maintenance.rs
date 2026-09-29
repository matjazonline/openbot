//! Run-deadline maintenance shares existing execution, job, attempt and wait owners.
use super::*;
use crate::application::workflow::activation::ActivationRequest;

pub(super) async fn expire_on(db: &mut PgConnection, scope: ActivationRequest) -> AppResult<bool> {
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
        .execute(&mut *db).await?;
    // Lock first, then consult the clock: waiting for another transition can cross expiry.
    let run: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_optional(&mut *db)
            .await?;
    if run.is_none() {
        return Ok(false);
    }
    let eligible: bool = sqlx::query_scalar("SELECT state IN ('queued','running','waiting') AND deadline<=clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    if !eligible {
        return Ok(false);
    }
    let associated: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.id=$3 AND job.id=$4 AND job.queue_kind='workflow' AND execution.completed_at IS NULL)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).fetch_one(&mut *db).await?;
    if !associated {
        return Ok(false);
    }
    // Runs have a frozen activation bound. These statements do not load the history into memory.
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 ORDER BY id FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    sqlx::query("SELECT job.id FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE execution.company_id=$1 AND execution.run_id=$2 AND job.queue_kind='workflow' ORDER BY job.id FOR UPDATE OF job")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    retire_jobs(db, scope.company, scope.run, RetirementCause::Deadline).await?;
    sqlx::query("UPDATE workflow_waits SET state='expired',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='failed',waiting_reason=NULL,terminal_execution_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    waits::audit(db, scope, "run_deadline_expired").await?;
    Ok(true)
}

pub(super) enum RetirementCause {
    Deadline,
    Cancel,
}

pub(super) async fn retire_jobs(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    cause: RetirementCause,
) -> AppResult<()> {
    let (code, retirement, stop) = match cause {
        RetirementCause::Deadline => ("workflow.run_deadline", "deadline", Some("timed_out")),
        RetirementCause::Cancel => ("workflow.cancelled", "cancel", None),
    };
    // Unknown is conservative: maintenance must not infer effect safety from a lost worker.
    let attempts = sqlx::query("UPDATE task_attempts AS attempt SET status='failed',finished_at=clock_timestamp(),workflow_failure_class='terminal',workflow_failure_code=$3,workflow_retry_safety='unknown',workflow_retirement=$4,stop_reason=$5 FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE execution.company_id=$1 AND execution.run_id=$2 AND job.queue_kind='workflow' AND job.status='processing' AND attempt.task_id=job.id AND attempt.attempt_number=job.retry_count+1 AND attempt.execution_generation=job.execution_generation AND attempt.worker_id=job.worker_id AND attempt.status='processing'")
        .bind(company.as_uuid()).bind(run.as_uuid()).bind(code).bind(retirement).bind(stop).execute(&mut *db).await?;
    let jobs = sqlx::query("UPDATE background_tasks AS job SET status='failed',retry_count=retry_count+1,worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL,updated_at=clock_timestamp() FROM workflow_executions AS execution WHERE execution.company_id=$1 AND execution.run_id=$2 AND job.company_id=execution.company_id AND job.workflow_execution_id=execution.id AND job.queue_kind='workflow' AND job.status='processing'")
        .bind(company.as_uuid()).bind(run.as_uuid()).execute(&mut *db).await?;
    if attempts.rows_affected() != jobs.rows_affected() {
        return Err(invalid());
    }
    sqlx::query("UPDATE background_tasks AS job SET status='failed',updated_at=clock_timestamp() FROM workflow_executions AS execution WHERE execution.company_id=$1 AND execution.run_id=$2 AND job.company_id=execution.company_id AND job.workflow_execution_id=execution.id AND job.queue_kind='workflow' AND job.status='pending'")
        .bind(company.as_uuid()).bind(run.as_uuid()).execute(db).await?;
    Ok(())
}
