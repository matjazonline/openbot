use super::{lease::*, *};
use crate::application::workflow::{activation::*, lease::*};
use tokio::time::Instant;

pub(super) async fn claim_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    worker: WorkflowWorkerId,
    policy: LeasePolicy,
    started: Instant,
) -> AppResult<Option<ClaimedWorkflow>> {
    let Some((run, job)) = lock_scope(db, scope, policy).await? else {
        return Ok(None);
    };
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let step = StepId::parse(job.step_id.clone()).map_err(|_| invalid())?;
    let definition = binding
        .bundle()
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .ok_or_else(invalid)?;
    if !crate::application::workflow::completion::is_io_kind(definition.step_type.as_str()) {
        return Ok(None);
    }
    if job.status == "processing" {
        recover_expired(db, scope, &job, policy).await?;
        return Ok(None);
    }
    if job.status != "pending" || job.retry_count >= job.max_retries {
        return Ok(None);
    }
    let due: bool =
        sqlx::query_scalar("SELECT run_at <= clock_timestamp() FROM background_tasks WHERE id=$1")
            .bind(scope.job.0)
            .fetch_one(&mut *db)
            .await?;
    if !due {
        return Ok(None);
    }
    if !reconciliation_budget_eligible(db, scope).await? {
        return Ok(None);
    }
    // Box the activation boundary to keep debug/test stacks at stock 2 MiB.
    let Some(activation) = Box::pin(pending_recovery::activate(db, scope)).await? else {
        return Ok(None);
    };
    if !fairness::available(db, scope, worker).await? {
        return Ok(None);
    }
    let fence = WorkflowFence {
        scope,
        worker,
        generation: WorkflowGeneration(Uuid::new_v4()),
        attempt: WorkflowAttempt(job.retry_count.checked_add(1).ok_or_else(invalid)?),
    };
    install(db, fence, policy).await?;
    let window = live_window(db, fence, started)
        .await?
        .ok_or_else(timed_out)?;
    Ok(Some(ClaimedWorkflow {
        fence,
        activation,
        window,
        max_result_bytes: usize::try_from(run.max_context_bytes).map_err(|_| invalid())?,
    }))
}

pub(super) async fn recover_expired(
    db: &mut PgConnection,
    scope: ActivationRequest,
    job: &LeaseJob,
    policy: LeasePolicy,
) -> AppResult<bool> {
    let expired: bool = sqlx::query_scalar("SELECT $1::timestamptz <= clock_timestamp()")
        .bind(job.lock_expires_at.ok_or_else(invalid)?)
        .fetch_one(&mut *db)
        .await?;
    if !expired {
        return Ok(false);
    }
    let fence = WorkflowFence {
        scope,
        worker: WorkflowWorkerId(job.worker_id.ok_or_else(invalid)?),
        generation: WorkflowGeneration(job.execution_generation.ok_or_else(invalid)?),
        attempt: WorkflowAttempt(job.retry_count.checked_add(1).ok_or_else(invalid)?),
    };
    retire(db, fence, policy, Retirement::Expired).await?;
    // Retirement establishes positive engine-owned delay. Even an ancient expired
    // lease cannot be reclaimed repeatedly by successive polls without time moving.
    Ok(true)
}

async fn install(
    db: &mut PgConnection,
    fence: WorkflowFence,
    policy: LeasePolicy,
) -> AppResult<()> {
    let scope = fence.scope;
    let changed = sqlx::query("UPDATE background_tasks SET status='processing', worker_id=$4, execution_generation=$5, locked_at=clock_timestamp(), lock_expires_at=LEAST(clock_timestamp()+make_interval(secs => $6), (SELECT deadline FROM workflow_runs WHERE company_id=$1 AND id=$7)) WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='pending' AND retry_count < max_retries AND run_at <= clock_timestamp() AND EXISTS (SELECT 1 FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$7 AND run.state IN ('queued','running') AND run.deadline > clock_timestamp())")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).bind(fence.worker.0).bind(fence.generation.0).bind(policy.duration().as_secs_f64()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    if changed.rows_affected() != 1 {
        return Err(timed_out());
    }
    sqlx::query("INSERT INTO task_attempts (id,task_id,attempt_number,execution_generation,status,worker_id,machine_id,started_at) VALUES (gen_random_uuid(),$1,$2,$3,'processing',$4,'workflow',clock_timestamp())")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.generation.0).bind(fence.worker.0).execute(&mut *db).await?;
    sqlx::query(
        "UPDATE workflow_runs SET state='running' WHERE company_id=$1 AND id=$2 AND state='queued'",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .execute(db)
    .await?;
    Ok(())
}

// Only the first claim of the exact scheduled episode gets this gate. A later
// ordinary retry keeps its saved logical reservations and existing policy.
async fn reconciliation_budget_eligible(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<bool> {
    let episode: Option<String> =
        sqlx::query_scalar("SELECT workflow_action_pending_claim_episode($1,$2,$3,$4)")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .bind(scope.execution.as_uuid())
            .bind(scope.job.0)
            .fetch_one(&mut *db)
            .await?;
    let Some(command_key) = episode else {
        return Ok(true);
    };
    // Requesting run, execution and job are already locked. Never lock the root
    // run: only its shared usage serializes descendant reservations and claims.
    sqlx::query("SELECT usage.root_run_id FROM workflow_root_budget_usage AS usage JOIN workflow_run_budgets AS link ON link.company_id=usage.company_id AND link.root_run_id=usage.root_run_id WHERE link.company_id=$1 AND link.run_id=$2 FOR UPDATE OF usage")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    let active: bool = sqlx::query_scalar("SELECT state IN ('queued','running') AND deadline>clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    if !active {
        return Err(timed_out());
    }
    // A fresh statement after the usage wait observes the committed competitor.
    let eligible: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_budget_eligible($1,$2)")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(&mut *db)
            .await?;
    if eligible {
        return Ok(true);
    }
    // This is no accounting request: preserve all prior debit/attempt facts.
    Box::pin(pending_recovery::settle(
        db,
        scope,
        pending_recovery::PendingFailure::RootBudgetExhausted,
    ))
    .await?;
    let refusal = sqlx::query("INSERT INTO workflow_action_claim_budget_refusals(company_id,run_id,execution_id,job_id,command_key,retired_attempt,audit_sequence) SELECT company_id,run_id,execution_id,job_id,command_key,retired_attempt,audit_sequence FROM workflow_action_claim_retirement_witnesses WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND job_id=$4 AND command_key=$5 AND transaction_id=pg_current_xact_id() AND retirement_confirmed AND audit_sequence IS NOT NULL")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).bind(command_key).execute(&mut *db).await?;
    if refusal.rows_affected() != 1 {
        return Err(invalid());
    }
    Ok(false)
}
