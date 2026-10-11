use super::persistence_bound::{Attempt, End, Evidence, WriteBound, set_server_timeouts};
use super::*;
use crate::application::workflow::{activation::*, lease::*};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
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
        let bound = WriteBound::start("lease.claim", policy);
        // Minted before the timer, so a resolution recognises exactly this call's claim.
        let generation = WorkflowGeneration(Uuid::new_v4());
        let claim = ClaimIdentity {
            scope,
            worker,
            generation,
        };
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let claimed = super::lease_claim::claim_on(&mut tx, claim, &bound, policy).await;
                bound.end(tx, claimed.map(End::Commit)).await
            }))
            .await;
        match attempt {
            Attempt::Done(claimed) => Ok(claimed),
            Attempt::Failed(error) => Err(error),
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(async {
                        let started = Instant::now();
                        let mut conn = bound.acquire(&self.pool).await?;
                        let mut tx = bound.begin(&mut conn).await?;
                        let evidence =
                            super::lease_claim::claimed_on(&mut tx, claim, &bound, started).await;
                        bound.roll_back(tx, evidence).await
                    }))
                    .await
            }
        }
    }
    async fn renew_io(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
    ) -> AppResult<Option<LeaseWindow>> {
        let bound = WriteBound::start("lease.renew", policy);
        let started = bound.started();
        let attempt = bound.attempt(Box::pin(async {
            let mut conn = bound.acquire(&self.pool).await?;
            let mut tx = bound.begin(&mut conn).await?;
            let renewed: AppResult<End<_>> = async {
                if !lock_fence(&mut tx, fence, bound.server_timeout()).await? { return Ok(End::Rollback(None)); }
                // A renewal must not remain uncommitted across its old expiry while
                // another claimant counts that old version as free capacity.
                capacity::lock(&mut tx).await?;
                super::persistence_bound::locks_held();
                if live_window(&mut tx, fence, started).await?.is_none() { return Ok(End::Rollback(None)); }
                sqlx::query("UPDATE background_tasks SET lock_expires_at = LEAST(clock_timestamp() + make_interval(secs => $2), (SELECT deadline FROM workflow_runs WHERE company_id = $3 AND id = $4)) WHERE id = $1 AND lock_expires_at > clock_timestamp() AND EXISTS (SELECT 1 FROM workflow_runs AS run WHERE run.company_id=$3 AND run.id=$4 AND run.deadline > clock_timestamp())")
                    .bind(fence.scope.job.0).bind(policy.duration().as_secs_f64()).bind(fence.scope.company.as_uuid()).bind(fence.scope.run.as_uuid()).execute(&mut *tx).await?;
                Ok(End::Commit(live_window(&mut tx, fence, started).await?))
            }
            .await;
            bound.end(tx, renewed).await
        })).await;
        match attempt {
            Attempt::Done(window) => Ok(window),
            Attempt::Failed(error) => Err(error),
            // The server expiry is authoritative whoever wrote it, measured from before
            // this read: a renewal that did not apply returns the older, shorter window.
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(async {
                        let started = Instant::now();
                        let mut conn = bound.acquire(&self.pool).await?;
                        let mut tx = bound.begin(&mut conn).await?;
                        let window = async {
                            Ok(
                                if lock_fence(&mut tx, fence, bound.server_timeout()).await? {
                                    live_window(&mut tx, fence, started).await?
                                } else {
                                    None
                                },
                            )
                        }
                        .await;
                        bound.roll_back(tx, window.map(Evidence::Applied)).await
                    }))
                    .await
            }
        }
    }
    async fn validate_io(&self, fence: WorkflowFence, policy: LeasePolicy) -> AppResult<bool> {
        let bound = WriteBound::read_only("lease.validate", policy);
        // Read-only: ROLLBACK needs no synchronous flush and leaves nothing ambiguous.
        bound
            .read(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let valid = async {
                    Ok(lock_fence(&mut tx, fence, bound.server_timeout()).await?
                        && live_window(&mut tx, fence, Instant::now()).await?.is_some())
                }
                .await;
                bound.roll_back(tx, valid).await
            }))
            .await
    }
    async fn release_io(
        &self,
        fence: WorkflowFence,
        policy: LeasePolicy,
        cause: LeaseReleaseCause,
    ) -> AppResult<bool> {
        let first = WriteBound::start("lease.release", policy);
        let deadline = first.deadline();
        match Box::pin(release_once(
            &self.pool,
            fence,
            policy,
            cause.clone(),
            first,
        ))
        .await?
        {
            Some(released) => Ok(released),
            // The ambiguous release did not apply and the fence is still live: one more try
            // within what remains of the same deadline T, so the whole call stays inside the
            // supervision guard. Expiry retirement remains the backstop after that.
            None => {
                let retry = WriteBound::within("lease.release", policy, deadline);
                Box::pin(release_once(&self.pool, fence, policy, cause, retry))
                    .await?
                    .ok_or_else(|| {
                        AppError::Conflict("Workflow lease.release commit outcome unknown".into())
                    })
            }
        }
    }
}

/// `Some(released)` once settled; `None` when an ambiguous release proved not applied.
async fn release_once(
    pool: &PgPool,
    fence: WorkflowFence,
    policy: LeasePolicy,
    cause: LeaseReleaseCause,
    bound: WriteBound,
) -> AppResult<Option<bool>> {
    let attempt = bound
        .attempt(Box::pin(async {
            let mut conn = bound.acquire(pool).await?;
            let mut tx = bound.begin(&mut conn).await?;
            let released = async {
                if !lock_fence(&mut tx, fence, bound.server_timeout()).await? {
                    return Ok(End::Rollback(false));
                }
                if live_window(&mut tx, fence, Instant::now()).await?.is_none() {
                    return Ok(End::Rollback(false));
                }
                retire(&mut tx, fence, policy, Retirement::Interrupted(cause)).await?;
                Ok(End::Commit(true))
            }
            .await;
            bound.end(tx, released).await
        }))
        .await;
    match attempt {
        Attempt::Done(released) => Ok(Some(released)),
        Attempt::Failed(error) => Err(error),
        Attempt::Ambiguous => {
            bound
                .resolve_retryable(Box::pin(async {
                    let mut conn = bound.acquire(pool).await?;
                    let mut tx = bound.begin(&mut conn).await?;
                    let evidence = released_on(&mut tx, fence, bound.server_timeout()).await;
                    bound.roll_back(tx, evidence).await
                }))
                .await
        }
    }
}

#[derive(sqlx::FromRow)]
struct AttemptState {
    status: String,
    workflow_retirement: Option<String>,
}

/// Classifies on our own attempt row, never on the job's current owner (a third party
/// may serialize between the original COMMIT and this read). Still processing means the
/// release did not apply and the fence is still ours to release.
async fn released_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    server: std::time::Duration,
) -> AppResult<Evidence<bool>> {
    // Waits on the original's run, execution and job locks until it commits or aborts.
    lock_scope(db, fence.scope, server).await?;
    let state = sqlx::query_as::<_, AttemptState>("SELECT status,workflow_retirement FROM task_attempts WHERE task_id=$1 AND attempt_number=$2 AND execution_generation=$3 AND worker_id=$4")
        .bind(fence.scope.job.0).bind(fence.attempt.0).bind(fence.generation.0).bind(fence.worker.0)
        .fetch_optional(&mut *db).await?;
    Ok(match state {
        Some(state) if state.status == "processing" => Evidence::NotApplied,
        Some(state) => Evidence::Applied(
            state.status == "failed" && state.workflow_retirement.as_deref() == Some("live"),
        ),
        None => Evidence::Applied(false),
    })
}

/// The exact claim one `claim_io` call tries to install.
#[derive(Clone, Copy)]
pub(super) struct ClaimIdentity {
    pub scope: ActivationRequest,
    pub worker: WorkflowWorkerId,
    pub generation: WorkflowGeneration,
}

/// Kept for the persistence sites outside the lease bound (capacity, polling, waits and
/// in-transaction refusals that predate it); the bounded sites report their own outcome.
pub(super) fn timed_out() -> AppError {
    AppError::Conflict("Workflow lease transaction timed out".into())
}

/// `timeout` is the server lock and statement bound: the caller passes what remains of
/// its own client bound, so a late statement cannot outlive it.
pub(super) async fn lock_scope(
    db: &mut PgConnection,
    scope: ActivationRequest,
    timeout: std::time::Duration,
) -> AppResult<Option<(LeaseRun, LeaseJob)>> {
    set_server_timeouts(db, timeout).await?;
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
    super::persistence_bound::locks_held();
    let active: bool = sqlx::query_scalar("SELECT state IN ('queued','running') AND deadline > clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    Ok(active.then_some((run, job)))
}

pub(super) async fn lock_fence(
    db: &mut PgConnection,
    fence: WorkflowFence,
    timeout: std::time::Duration,
) -> AppResult<bool> {
    let Some((_, job)) = lock_scope(db, fence.scope, timeout).await? else {
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
        let bound = WriteBound::start("lease.retire_exhausted", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let retired = async {
                    let Some((_, job)) = lock_scope(&mut tx, scope, bound.server_timeout()).await?
                    else {
                        return Ok(End::Rollback(false));
                    };
                    if !exhausted(&job) {
                        return Ok(End::Rollback(false));
                    }
                    pending_recovery::settle(
                        &mut tx,
                        scope,
                        pending_recovery::PendingFailure::AttemptsExhausted,
                    )
                    .await?;
                    Ok(End::Commit(true))
                }
                .await;
                bound.end(tx, retired).await
            }))
            .await;
        settle_retirement(&self.pool, &bound, scope, attempt, exhausted).await
    }

    async fn retire_expired_io(
        &self,
        scope: ActivationRequest,
        policy: LeasePolicy,
    ) -> AppResult<bool> {
        let bound = WriteBound::start("lease.retire_expired", policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let retired = async {
                    let Some((_, job)) = lock_scope(&mut tx, scope, bound.server_timeout()).await?
                    else {
                        return Ok(End::Rollback(false));
                    };
                    if job.status != "processing" {
                        return Ok(End::Rollback(false));
                    }
                    Ok(End::Commit(
                        super::lease_claim::recover_expired(&mut tx, scope, &job, policy).await?,
                    ))
                }
                .await;
                bound.end(tx, retired).await
            }))
            .await;
        settle_retirement(&self.pool, &bound, scope, attempt, expired).await
    }
}

fn exhausted(job: &LeaseJob) -> bool {
    job.status == "pending" && job.retry_count >= job.max_retries
}
/// Only expiry retirement moves a job out of 'processing' without its owner; a claim needs
/// 'pending'. So a job still processing after an ambiguous sweep was not retired by it.
fn expired(job: &LeaseJob) -> bool {
    job.status == "processing"
}

/// An ambiguous sweep applied exactly when its own predicate no longer holds; otherwise it
/// did not, and the next poll retries the idempotent sweep.
async fn settle_retirement(
    pool: &PgPool,
    bound: &WriteBound,
    scope: ActivationRequest,
    attempt: Attempt<bool>,
    pending: fn(&LeaseJob) -> bool,
) -> AppResult<bool> {
    match attempt {
        Attempt::Done(retired) => Ok(retired),
        Attempt::Failed(error) => Err(error),
        Attempt::Ambiguous => {
            bound
                .resolve(Box::pin(async {
                    let mut conn = bound.acquire(pool).await?;
                    let mut tx = bound.begin(&mut conn).await?;
                    let job = lock_scope(&mut tx, scope, bound.server_timeout()).await;
                    let job = bound.roll_back(tx, job).await?;
                    Ok(match job {
                        Some((_, job)) if pending(&job) => Evidence::NotApplied,
                        _ => Evidence::Applied(true),
                    })
                }))
                .await
        }
    }
}
