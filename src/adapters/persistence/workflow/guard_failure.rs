//! Fails a run whose worker write a workflow guard refused twice (fix-app 03 batch 1, G1 r3).
//! The run lock comes first, as for every run transition; every later read therefore sees what
//! the previous lock holder committed, so a competing settlement is observed, never overwritten.
use super::*;
use crate::application::workflow::guard::{
    GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
};

/// The failure code of every attempt a guard failure closes; the guard's own name is carried by
/// the run event `guard_violation:<guard>` (the attempt code is fixed so SQL can verify it).
pub(super) const GUARD_FAILURE_CODE: &str = "workflow.guard_violation";

#[async_trait]
impl WorkflowGuardFailure for PostgresPersistence {
    async fn fail_run_on_guard(&self, failure: GuardFailure) -> AppResult<GuardFailureOutcome> {
        let mut tx = self.pool.begin().await?;
        match fail_on(&mut tx, &failure).await {
            Ok(GuardFailureOutcome::Failed) => {
                tx.commit().await?;
                Ok(GuardFailureOutcome::Failed)
            }
            Ok(outcome) => {
                tx.rollback().await?;
                Ok(outcome)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }
}

#[derive(sqlx::FromRow)]
struct RunRow {
    state: String,
    expired: bool,
    actor_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct JobRow {
    status: String,
    worker_id: Option<Uuid>,
    execution_generation: Option<Uuid>,
    attempt_ordinal: i32,
    live: Option<bool>,
}

async fn fail_on(db: &mut PgConnection, failure: &GuardFailure) -> AppResult<GuardFailureOutcome> {
    let scope = failure.scope;
    let (company, run) = (scope.company.as_uuid(), scope.run.as_uuid());
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
        .execute(&mut *db).await?;
    let owner = sqlx::query_as::<_, RunRow>("SELECT state, deadline <= clock_timestamp() AS expired, actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(company).bind(run).fetch_optional(&mut *db).await?;
    let Some(owner) = owner else {
        return Ok(GuardFailureOutcome::Inactive);
    };
    if !matches!(owner.state.as_str(), "queued" | "running" | "waiting") {
        return Ok(GuardFailureOutcome::Inactive);
    }
    if !deferral_free(db, failure, &owner).await? {
        return Ok(GuardFailureOutcome::Deferred);
    }
    // Live children are reached only through the waits closed below, so collect them first.
    let children: Vec<Uuid> = sqlx::query_scalar("SELECT child_run_id FROM workflow_waits WHERE company_id=$1 AND run_id=$2 AND state='waiting' AND reason='child_run' ORDER BY child_run_id")
        .bind(company).bind(run).fetch_all(&mut *db).await?;
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 ORDER BY id FOR UPDATE")
        .bind(company).bind(run).execute(&mut *db).await?;
    sqlx::query("SELECT job.id FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE execution.company_id=$1 AND execution.run_id=$2  ORDER BY job.id FOR UPDATE OF job")
        .bind(company).bind(run).execute(&mut *db).await?;
    maintenance::retire_jobs(
        db,
        scope.company,
        scope.run,
        maintenance::RetirementCause::Guard,
    )
    .await?;
    sqlx::query("UPDATE workflow_waits SET state='cancelled',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND state='waiting'")
        .bind(company).bind(run).execute(&mut *db).await?;
    message::receipts::cancel_message_deliveries_on(
        db,
        scope.company,
        scope.run,
        None,
        crate::entities::transport::DeliveryCancellationReason::WorkflowExpired,
    )
    .await?;
    sqlx::query("UPDATE workflow_runs SET state='failed',waiting_reason=NULL,terminal_execution_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(company).bind(run).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    waits::audit(
        db,
        scope,
        &format!("guard_violation:{}", failure.violation.guard()),
    )
    .await?;
    let code =
        crate::domain::workflow::FailureCode::parse(GUARD_FAILURE_CODE).map_err(|_| invalid())?;
    action_uncertainty::record(db, scope.company, scope.run, None, &code).await?;
    // A guard failure ends the run early, like cancellation: its live children share its
    // deadline only at expiry, so they are cancelled now rather than left running.
    controls::cancel_children_on(db, scope.company, &children, owner.actor_id).await?;
    Ok(GuardFailureOutcome::Failed)
}

/// G1 r3 deferral rules 2, 3 and 5: the scope execution is still current, run expiry does not
/// own the deadline, and no other owner holds or settled the scope job.
async fn deferral_free(
    db: &mut PgConnection,
    failure: &GuardFailure,
    owner: &RunRow,
) -> AppResult<bool> {
    let scope = failure.scope;
    let current: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 AND completed_at IS NULL AND successor_execution_id IS NULL)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    if !current || (owner.expired && failure.site != GuardSite::RunExpiry) {
        return Ok(false);
    }
    let job = sqlx::query_as::<_, JobRow>("SELECT status, worker_id, execution_generation, attempt_ordinal, lock_expires_at > clock_timestamp() AS live FROM background_tasks WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 ")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(job) = job else {
        return Ok(false);
    };
    Ok(owns(failure, &job))
}

fn owns(failure: &GuardFailure, job: &JobRow) -> bool {
    let live = job.live == Some(true);
    match (failure.fence, failure.site) {
        (Some(fence), _) => {
            job.status == "processing"
                && job.worker_id == Some(fence.worker.0)
                && job.execution_generation == Some(fence.generation.0)
                && job.attempt_ordinal == fence.attempt.0
                && live
        }
        // Expiry settles every job state of its run; nothing else will after the deadline.
        (None, GuardSite::RunExpiry) => true,
        (None, GuardSite::LeaseExpiry) => {
            (job.status == "processing" && !live) || job.status == "pending"
        }
        (None, GuardSite::Step) => matches!(job.status.as_str(), "pending" | "completed"),
    }
}
