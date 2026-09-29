use super::*;
use crate::application::workflow::{activation::*, lease::*};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::time::Instant;

#[derive(sqlx::FromRow)]
pub(super) struct LeaseRun {
    pub binding_id: Uuid,
    pub max_context_bytes: i32,
}
#[derive(sqlx::FromRow)]
pub(super) struct LeaseJob {
    pub status: String,
    pub retry_count: i32,
    pub max_retries: i32,
    pub worker_id: Option<Uuid>,
    pub execution_generation: Option<Uuid>,
    pub lock_expires_at: Option<DateTime<Utc>>,
    pub payload: Value,
    pub step_id: String,
}

#[async_trait]
impl WorkflowLeases for PostgresPersistence {
    async fn claim_io(
        &self,
        scope: ActivationRequest,
        worker: WorkflowWorkerId,
        policy: LeasePolicy,
    ) -> AppResult<Option<ClaimedWorkflow>> {
        let started = Instant::now();
        tokio::time::timeout(policy.persistence_timeout(), async {
            let mut tx = self.pool.begin().await?;
            let result =
                super::lease_claim::claim_on(&mut tx, scope, worker, policy, started).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
        .map_err(|_| timed_out())?
    }
    async fn renew_io(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
    ) -> AppResult<Option<LeaseWindow>> {
        let started = Instant::now();
        tokio::time::timeout(policy.persistence_timeout(), async {
            let mut tx = self.pool.begin().await?;
            if !lock_fence(&mut tx, fence, policy).await? { return Ok(None); }
            // A renewal must not remain uncommitted across its old expiry while
            // another claimant counts that old version as free capacity.
            capacity::lock(&mut tx).await?;
            if live_window(&mut tx, fence, started).await?.is_none() { return Ok(None); }
            sqlx::query("UPDATE background_tasks SET lock_expires_at = LEAST(clock_timestamp() + make_interval(secs => $2), (SELECT deadline FROM workflow_runs WHERE company_id = $3 AND id = $4)) WHERE id = $1 AND lock_expires_at > clock_timestamp() AND EXISTS (SELECT 1 FROM workflow_runs AS run WHERE run.company_id=$3 AND run.id=$4 AND run.deadline > clock_timestamp())")
                .bind(fence.scope.job.0).bind(policy.duration().as_secs_f64()).bind(fence.scope.company.as_uuid()).bind(fence.scope.run.as_uuid()).execute(&mut *tx).await?;
            let window = live_window(&mut tx, fence, started).await?;
            tx.commit().await?;
            Ok(window)
        }).await.map_err(|_| timed_out())?
    }
    async fn validate_io(&self, fence: WorkflowFence, policy: LeasePolicy) -> AppResult<bool> {
        tokio::time::timeout(policy.persistence_timeout(), async {
            let mut tx = self.pool.begin().await?;
            if !lock_fence(&mut tx, fence, policy).await? {
                return Ok(false);
            }
            let valid = live_window(&mut tx, fence, Instant::now()).await?.is_some();
            tx.commit().await?;
            Ok(valid)
        })
        .await
        .map_err(|_| timed_out())?
    }
    async fn release_io(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
        cause: LeaseReleaseCause,
    ) -> AppResult<bool> {
        tokio::time::timeout(policy.persistence_timeout(), async {
            let mut tx = self.pool.begin().await?;
            if !lock_fence(&mut tx, fence, policy).await? {
                return Ok(false);
            }
            if live_window(&mut tx, fence, Instant::now()).await?.is_none() {
                return Ok(false);
            }
            retire(&mut tx, fence, policy, Retirement::Interrupted(cause)).await?;
            tx.commit().await?;
            Ok(true)
        })
        .await
        .map_err(|_| timed_out())?
    }
}

pub(super) fn timed_out() -> AppError {
    AppError::Conflict("Workflow lease transaction timed out".into())
}

pub(super) async fn lock_scope(
    db: &mut PgConnection,
    scope: ActivationRequest,
    policy: LeasePolicy,
) -> AppResult<Option<(LeaseRun, LeaseJob)>> {
    let timeout = format!("{}ms", policy.persistence_timeout().as_millis());
    sqlx::query(
        "SELECT set_config('lock_timeout',$1,true), set_config('statement_timeout',$1,true)",
    )
    .bind(timeout)
    .execute(&mut *db)
    .await?;
    // Run-first ordering serializes claims, renewals, cancellation and progression.
    let run = sqlx::query_as::<_, LeaseRun>("SELECT binding_id, max_context_bytes FROM workflow_runs WHERE company_id=$1 AND id=$2 AND state IN ('queued','running') FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(run) = run else {
        return Ok(None);
    };
    let step: Option<String> = sqlx::query_scalar("SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 AND completed_at IS NULL FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(step_id) = step else {
        return Ok(None);
    };
    let job = sqlx::query_as::<_, LeaseJob>("SELECT status,retry_count,max_retries,worker_id,execution_generation,lock_expires_at,payload,$4::text AS step_id FROM background_tasks WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).bind(step_id).fetch_optional(&mut *db).await?;
    let Some(job) = job else {
        return Ok(None);
    };
    if decode_job_payload(job.payload.clone())? != scope.execution {
        return Ok(None);
    }
    let active: bool = sqlx::query_scalar("SELECT state IN ('queued','running') AND deadline > clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    Ok(active.then_some((run, job)))
}

pub(super) async fn lock_fence(
    db: &mut PgConnection,
    fence: WorkflowFence,
    policy: LeasePolicy,
) -> AppResult<bool> {
    let Some((_, job)) = lock_scope(db, fence.scope, policy).await? else {
        return Ok(false);
    };
    Ok(job.status == "processing"
        && job.worker_id == Some(fence.worker.0)
        && job.execution_generation == Some(fence.generation.0)
        && job.retry_count.checked_add(1) == Some(fence.attempt.0))
}

#[derive(sqlx::FromRow)]
struct Remaining {
    lease_seconds: f64,
    run_seconds: f64,
}
pub(super) async fn live_window(
    db: &mut PgConnection,
    fence: WorkflowFence,
    started: Instant,
) -> AppResult<Option<LeaseWindow>> {
    let row = sqlx::query_as::<_, Remaining>("SELECT extract(epoch FROM (job.lock_expires_at-clock_timestamp()))::float8 AS lease_seconds, extract(epoch FROM (run.deadline-clock_timestamp()))::float8 AS run_seconds FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id JOIN task_attempts AS attempt ON attempt.task_id=job.id AND attempt.attempt_number=$7 AND attempt.execution_generation=$6 AND attempt.worker_id=$5 AND attempt.status='processing' WHERE job.company_id=$1 AND run.id=$2 AND execution.id=$3 AND job.id=$4 AND job.queue_kind='workflow' AND job.status='processing' AND job.worker_id=$5 AND job.execution_generation=$6 AND job.retry_count+1=$7 AND job.lock_expires_at > clock_timestamp() AND execution.completed_at IS NULL AND run.state IN ('queued','running') AND run.deadline > clock_timestamp()")
        .bind(fence.scope.company.as_uuid()).bind(fence.scope.run.as_uuid()).bind(fence.scope.execution.as_uuid()).bind(fence.scope.job.0).bind(fence.worker.0).bind(fence.generation.0).bind(fence.attempt.0).fetch_optional(db).await?;
    Ok(row
        .filter(|row| row.lease_seconds > 0.0 && row.run_seconds > 0.0)
        .map(|row| LeaseWindow {
            expires: started + std::time::Duration::from_secs_f64(row.lease_seconds),
            run_deadline: started + std::time::Duration::from_secs_f64(row.run_seconds),
        }))
}

pub(super) async fn retire(
    db: &mut PgConnection,
    fence: WorkflowFence,
    _policy: LeasePolicy,
    reason: Retirement,
) -> AppResult<()> {
    // Failure settlement is shared with expired ownership; the caller already holds
    // the run/execution/job locks. Box the restore/progression boundary.
    Box::pin(super::recovery::retire_on(db, fence, reason)).await
}

#[derive(Clone)]
pub(super) enum Retirement {
    BudgetExhausted,
    Expired,
    Interrupted(LeaseReleaseCause),
}

#[async_trait]
impl WorkflowLeaseRecovery for PostgresPersistence {
    async fn retire_exhausted_work(
        &self,
        scope: ActivationRequest,
        policy: LeasePolicy,
    ) -> AppResult<bool> {
        tokio::time::timeout(policy.persistence_timeout(), async {
            let mut tx = self.pool.begin().await?;
            let Some((_, job)) = lock_scope(&mut tx, scope, policy).await? else {
                return Ok(false);
            };
            if job.status != "pending" || job.retry_count < job.max_retries {
                return Ok(false);
            }
            pending_recovery::settle(
                &mut tx,
                scope,
                pending_recovery::PendingFailure::AttemptsExhausted,
            )
            .await?;
            tx.commit().await?;
            Ok(true)
        })
        .await
        .map_err(|_| timed_out())?
    }

    async fn retire_expired_io(
        &self,
        scope: ActivationRequest,
        policy: LeasePolicy,
    ) -> AppResult<bool> {
        tokio::time::timeout(policy.persistence_timeout(), async {
            let mut tx = self.pool.begin().await?;
            let Some((_, job)) = lock_scope(&mut tx, scope, policy).await? else {
                return Ok(false);
            };
            if job.status != "processing" {
                return Ok(false);
            }
            let retired = super::lease_claim::recover_expired(&mut tx, scope, &job, policy).await?;
            tx.commit().await?;
            Ok(retired)
        })
        .await
        .map_err(|_| timed_out())?
    }
}
