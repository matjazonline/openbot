//! Classified retirement uses the existing job/attempt owners and progression writer.
use super::{
    lease::{LeaseJob, Retirement},
    *,
};
use crate::application::workflow::{activation::*, batch::PureCompletion, lease::*};
use crate::domain::workflow::{
    AttemptBudget, EngineDisposition, FailureClass, FailureCode, RecoveryDecision, RecoveryPolicy,
    RecoverySnapshot, RetryEligibility, RetrySafety, StepFailure, StepOutcome, resolve_outcome,
};
use chrono::{DateTime, Utc};

#[derive(sqlx::FromRow)]
struct Snapshot {
    binding_id: Uuid,
    max_steps: i32,
    deadline: DateTime<Utc>,
    now: DateTime<Utc>,
}

pub(super) async fn retire_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    reason: Retirement,
) -> AppResult<()> {
    let scope = fence.scope;
    let run = sqlx::query_as::<_, Snapshot>("SELECT binding_id,max_steps,deadline,clock_timestamp() AS now FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    let job = sqlx::query_as::<_, LeaseJob>("SELECT status,retry_count,max_retries,worker_id,execution_generation,lock_expires_at,payload,execution.step_id FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE job.company_id=$1 AND job.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.job.0).fetch_one(&mut *db).await?;
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let graph = binding.bundle().compiled().graph();
    let step = StepId::parse(job.step_id).map_err(|_| invalid())?;
    let definition = graph.definition().steps.get(&step).ok_or_else(invalid)?;
    let report = report(&reason, definition.step_type.as_str())?;
    let attempts = AttemptBudget::new(
        u32::try_from(fence.attempt.0).map_err(|_| invalid())?,
        u32::try_from(job.max_retries).map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    let decision = RecoveryPolicy::default().decide(
        &report.failure,
        report.safety,
        RecoverySnapshot {
            attempts,
            work_budget: RetryEligibility::Available,
            now: run.now,
            deadline: run.deadline,
        },
    );
    close_attempt(db, fence, &report, &reason).await?;
    let routed = if decision == RecoveryDecision::FinalError {
        route_failure(db, scope, &run, graph, &step, &report.failure).await?
    } else {
        false
    };
    if !routed {
        settle_run(db, scope, decision).await?;
    }
    finish_job(db, fence, decision, &reason).await?;
    Ok(())
}

fn report(reason: &Retirement, kind: &str) -> AppResult<WorkflowFailure> {
    if let Retirement::Interrupted(LeaseReleaseCause::Classified(report)) = reason {
        return Ok(report.clone());
    }
    // These engine-owned read handlers cannot dispatch an effect. Every other
    // interrupted handler is ambiguous until its owning subsystem reconciles it.
    let safety = if matches!(kind, "context.load" | "memory.load") {
        RetrySafety::SafeToRetry
    } else {
        RetrySafety::EffectOutcomeUnknown
    };
    let (class, code) = match reason {
        Retirement::Expired => (FailureClass::Retryable, "workflow.lease_expired"),
        Retirement::Interrupted(LeaseReleaseCause::ActivationLimit) => {
            (FailureClass::Terminal, "workflow.activation_limit")
        }
        Retirement::Interrupted(LeaseReleaseCause::InvalidResult) => {
            (FailureClass::Terminal, "workflow.invalid_result")
        }
        Retirement::Interrupted(LeaseReleaseCause::Deadline) => {
            (FailureClass::Retryable, "workflow.operation_deadline")
        }
        Retirement::Interrupted(_) => (FailureClass::Retryable, "workflow.interrupted"),
    };
    Ok(WorkflowFailure {
        failure: StepFailure::new(
            class,
            FailureCode::parse(code).map_err(|_| invalid())?,
            None,
        )
        .map_err(|_| invalid())?,
        safety,
    })
}

async fn close_attempt(
    db: &mut PgConnection,
    fence: WorkflowFence,
    report: &WorkflowFailure,
    reason: &Retirement,
) -> AppResult<()> {
    let closed = sqlx::query("UPDATE task_attempts SET status='failed',finished_at=clock_timestamp(),workflow_failure_class=$5,workflow_failure_code=$6,workflow_retry_safety=$7,workflow_retirement=$8,stop_reason=$9 WHERE task_id=$1 AND attempt_number=$2 AND execution_generation=$3 AND worker_id=$4 AND status='processing'")
        .bind(fence.scope.job.0).bind(fence.attempt.0).bind(fence.generation.0).bind(fence.worker.0)
        .bind(match report.failure.class() { FailureClass::Retryable => "retryable", FailureClass::Terminal => "terminal" })
        .bind(report.failure.code().as_str()).bind(match report.safety { RetrySafety::SafeToRetry => "safe", RetrySafety::EffectOutcomeUnknown => "unknown" })
        .bind(if matches!(reason,Retirement::Expired) { "expired" } else { "live" }).bind(match reason { Retirement::Expired | Retirement::Interrupted(LeaseReleaseCause::LeaseLost) => Some("lease_lost"), Retirement::Interrupted(LeaseReleaseCause::Deadline) => Some("timed_out"), _ => None }).execute(db).await?;
    if closed.rows_affected() != 1 {
        return Err(invalid());
    }
    Ok(())
}

async fn settle_run(
    db: &mut PgConnection,
    scope: ActivationRequest,
    decision: RecoveryDecision,
) -> AppResult<()> {
    let (state, wait) = match decision {
        RecoveryDecision::RetryAt(_) => ("running", None),
        RecoveryDecision::Reconcile => ("waiting", Some("reconciliation")),
        _ => ("failed", None),
    };
    sqlx::query("UPDATE workflow_runs SET state=$3,waiting_reason=$4,terminal_execution_id=CASE WHEN $3='failed' THEN $5 ELSE NULL END WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(state).bind(wait).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    // Attempt identity makes each debit independently observable without duplicating the ledger.
    sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT company_id,id,(SELECT COALESCE(MAX(sequence),0)+1 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2), 'attempt_failed:' || (SELECT retry_count+1 FROM background_tasks WHERE id=$4),actor_id,$3 FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).execute(db).await?;
    Ok(())
}

async fn finish_job(
    db: &mut PgConnection,
    fence: WorkflowFence,
    decision: RecoveryDecision,
    reason: &Retirement,
) -> AppResult<()> {
    let retry_at = match decision {
        RecoveryDecision::RetryAt(at) => Some(at),
        _ => None,
    };
    let changed = sqlx::query("UPDATE background_tasks SET status=$8,retry_count=retry_count+1,run_at=COALESCE($9,run_at),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL,updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='processing' AND execution_generation=$4 AND worker_id=$5 AND retry_count+1=$6 AND ($7 OR lock_expires_at>clock_timestamp())")
        .bind(fence.scope.company.as_uuid()).bind(fence.scope.job.0).bind(fence.scope.execution.as_uuid()).bind(fence.generation.0).bind(fence.worker.0).bind(fence.attempt.0)
        .bind(matches!(reason,Retirement::Expired)).bind(if retry_at.is_some(){"pending"}else{"failed"}).bind(retry_at).execute(db).await?;
    if changed.rows_affected() != 1 {
        return Err(lease::timed_out());
    }
    Ok(())
}

async fn route_failure(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &Snapshot,
    graph: &crate::domain::workflow::ValidatedWorkflow,
    step: &StepId,
    failure: &StepFailure,
) -> AppResult<bool> {
    let outcome = StepOutcome::Failed(failure.clone());
    let target = match resolve_outcome(graph, step, &outcome, RetryEligibility::Exhausted, run.now)
        .map_err(|_| invalid())?
    {
        EngineDisposition::Terminal { target, .. } => target.cloned(),
        _ => return Err(invalid()),
    };
    Ok(if let Some(target) = target {
        let activation = activation::activate_on(db, scope).await?;
        // A final-error branch cannot manufacture an activation beyond the run allowance.
        if target == TransitionTarget::End
            || activation.ordinal < u64::try_from(run.max_steps).map_err(|_| invalid())?
        {
            let completion = PureCompletion {
                output: serde_json::Value::Null,
                route: RouteSelection::FinalError,
                state: if target == TransitionTarget::End {
                    RunState::Failed
                } else {
                    RunState::Running
                },
                target,
            };
            batch_commit::complete(
                db,
                scope,
                &activation,
                &completion,
                run.max_steps,
                completion_job::CompletionOwner::Recovered,
            )
            .await?;
            true
        } else {
            false
        }
    } else {
        false
    })
}

/// Pending deterministic failures reuse the exact same final-error decision and writer.
pub(super) async fn route_pending_failure(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<bool> {
    let run=sqlx::query_as::<_,Snapshot>("SELECT binding_id,max_steps,deadline,clock_timestamp() AS now FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    if run.deadline <= run.now {
        return Ok(false);
    }
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let step: String = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let step = StepId::parse(step).map_err(|_| invalid())?;
    let failure = StepFailure::new(
        FailureClass::Terminal,
        FailureCode::parse("workflow.pending_failure").map_err(|_| invalid())?,
        None,
    )
    .map_err(|_| invalid())?;
    route_failure(
        db,
        scope,
        &run,
        binding.bundle().compiled().graph(),
        &step,
        &failure,
    )
    .await
}
