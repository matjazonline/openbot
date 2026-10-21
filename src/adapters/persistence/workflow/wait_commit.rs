use super::*;
use crate::application::workflow::{activation::*, completion::*, waits::*};
use serde_json::{Value, json};

pub(super) async fn resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<WaitProgress> {
    if maintenance::expire_on(db, scope).await? {
        return Ok(WaitProgress::Expired(CommitDisposition::Committed));
    }
    let Some(run) = waits::lock(db, scope).await? else {
        return Ok(WaitProgress::Refused);
    };
    let Some(wait) = waits::row(db, scope, waits::WaitSelection::Execution).await? else {
        return Ok(WaitProgress::Refused);
    };
    match wait.state.as_str() {
        "completed" => {
            return saved(
                db,
                scope,
                CommitDisposition::Replayed,
                wait.reason == "effect",
            )
            .await;
        }
        "cancelled" => return Ok(WaitProgress::Refused),
        "expired" => return Ok(WaitProgress::Expired(CommitDisposition::Replayed)),
        "failed" => {
            return Ok(WaitProgress::Failed {
                disposition: CommitDisposition::Replayed,
                reason: match wait.failure_code.as_deref() {
                    Some("activation_limit") => WaitFailureReason::ActivationLimit,
                    Some("invalid_output") => WaitFailureReason::InvalidOutput,
                    Some("evidence_conflict") => WaitFailureReason::EvidenceConflict,
                    _ => return Err(invalid()),
                },
            });
        }
        "waiting" => {}
        _ => return Err(invalid()),
    }
    let now = waits::now(db).await?;
    if run.deadline <= now && maintenance::expire_on(db, scope).await? {
        return Ok(WaitProgress::Expired(CommitDisposition::Committed));
    }
    if run.state != "waiting" || (wait.reason == "event" && wait.deadline <= now) {
        return expire(db, scope, &run.state).await;
    }
    if wait.reason == "decision" {
        // Submission settles a decision in its own command; a worker only times it out.
        if wait.deadline > now {
            return Ok(WaitProgress::Pending);
        }
        return Box::pin(decision_timed_out(db, scope, wait.id)).await;
    }
    let wake = if wait.reason == "effect" {
        // Explicit, before the generic event lookup (04.11-D2 §4.4, D4 §M).
        match message::receipts::effect_resume_on(db, scope, wait.id).await? {
            message::receipts::EffectResume::Pending => return Ok(WaitProgress::Pending),
            message::receipts::EffectResume::Conflicted => {
                return Ok(WaitProgress::Failed {
                    disposition: CommitDisposition::Committed,
                    reason: WaitFailureReason::EvidenceConflict,
                });
            }
            // A saved model tool call's settled delivery (accepted or failed) is received
            // truth for the agent, not this step's result: continue the parked agent job.
            message::receipts::EffectResume::Failed
            | message::receipts::EffectResume::Accepted(_)
                if message::receipts::model_tool_wait_on(db, scope, wait.id).await? =>
            {
                return Box::pin(continue_delivery(db, scope, wait.id)).await;
            }
            message::receipts::EffectResume::Failed => {
                return Box::pin(effect_failed(db, scope)).await;
            }
            message::receipts::EffectResume::Accepted(result) => Wake::Effect(result),
        }
    } else if wait.reason == "timer" {
        if wait.deadline > now {
            return Ok(WaitProgress::Pending);
        }
        Wake::Timer
    } else {
        // Run-scoped: the oldest unconsumed event with this name and correlation, whenever it
        // arrived (05-D2 §11). Decision waits have no name, so they never match.
        let event:Option<ReceivedEvent>=sqlx::query_as("SELECT id,payload FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND event_name=$3 AND correlation=$4 AND consumed_execution_id IS NULL ORDER BY created_at,id LIMIT 1 FOR UPDATE")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(&wait.event_name).bind(&wait.correlation).fetch_optional(&mut *db).await?;
        let Some(event) = event else {
            return Ok(WaitProgress::Pending);
        };
        Wake::Event(event)
    };
    // Box the transactional continuation to keep the debug async stack shallow.
    Box::pin(advance(db, scope, run, wake)).await
}

/// Complete the exact delivery wait and reopen the agent job it parked (continuation
/// debit only). The agent replays its saved call and reads the receipt; the step itself is
/// not completed here. The SQL reopen guard requires this action's marker attempt.
async fn continue_delivery(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
) -> AppResult<WaitProgress> {
    let settled = sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND id=$4 AND reason='effect' AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).execute(&mut *db).await?;
    if settled.rows_affected() != 1 {
        return Err(invalid());
    }
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2 AND state='waiting' AND waiting_reason='effect'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    decisions::approvals::reopen_job(db, scope).await?;
    waits::audit_wait(db, scope, wait, "wait_completed").await?;
    Ok(WaitProgress::Continued)
}

/// What woke the wait: its timer, its event, or its message action's accepted receipt.
enum Wake {
    Timer,
    Event(ReceivedEvent),
    Effect(Value),
}

/// A decision past its own deadline (before the run's): Terminal `decision.timed_out` takes the
/// declared `error` route, whose successor receives `null`, or fails the run with
/// `terminal_execution_id` set. The code stays durable as the audit event kind (05-D2 §4.3).
async fn decision_timed_out(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
) -> AppResult<WaitProgress> {
    sqlx::query("UPDATE workflow_waits SET state='expired',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND id=$4 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    let token: Option<Uuid> = sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND wait_id=$2 AND subject_kind='token_allowance'")
        .bind(scope.company.as_uuid()).bind(wait).fetch_optional(&mut *db).await?;
    if let Some(token) = token {
        decisions::tokens::fail(
            db,
            scope,
            crate::application::workflow::decisions::DecisionId(token),
            "token_allowance_expired",
        )
        .await?;
        return Ok(WaitProgress::Expired(CommitDisposition::Committed));
    }
    let approval: Option<Uuid> = sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND wait_id=$2 AND subject_kind='action_approval'")
        .bind(scope.company.as_uuid()).bind(wait).fetch_optional(&mut *db).await?;
    if let Some(approval) = approval {
        decisions::approvals::fail(
            db,
            scope,
            crate::application::workflow::decisions::DecisionId(approval),
            "approval_expired",
        )
        .await?;
        return Ok(WaitProgress::Expired(CommitDisposition::Committed));
    }
    waits::audit(db, scope, DECISION_TIMED_OUT).await?;
    let failure = crate::domain::workflow::StepFailure::new(
        crate::domain::workflow::FailureClass::Terminal,
        crate::domain::workflow::FailureCode::parse(DECISION_TIMED_OUT).map_err(|_| invalid())?,
        None,
    )
    .map_err(|_| invalid())?;
    recovery::route_parked_failure(db, scope, &failure).await?;
    Ok(WaitProgress::Expired(CommitDisposition::Committed))
}
pub(super) const DECISION_TIMED_OUT: &str = "decision.timed_out";

/// A failed message receipt: the wait completes and the failure takes the shared final-error
/// writer with Terminal `message.delivery_failed`; nothing resends (04.11-D2 §4.4).
async fn effect_failed(db: &mut PgConnection, scope: ActivationRequest) -> AppResult<WaitProgress> {
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    let failure = crate::application::workflow::message::message_delivery_failed().failure;
    recovery::route_parked_failure(db, scope, &failure).await?;
    waits::audit(db, scope, "message_delivery_failed").await?;
    saved(db, scope, CommitDisposition::Committed, true).await
}

async fn advance(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: waits::WaitRun,
    wake: Wake,
) -> AppResult<WaitProgress> {
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let activation = activation::activate_on(db, scope)
        .await?
        .ok_or_else(invalid)?;
    let (prepared, event) = match wake {
        Wake::Timer => (
            prepare(
                binding.bundle(),
                &activation,
                json!({"deadline":activation.inputs["deadline"]}),
            ),
            None,
        ),
        Wake::Event(event) => (
            prepare(binding.bundle(), &activation, event.payload.clone()),
            Some(event.id),
        ),
        // A parked message step completes as the I/O step it is, with its receipt as output.
        Wake::Effect(result) => (prepare_io(binding.bundle(), &activation, result), None),
    };
    let completion = match prepared {
        Ok(completion) => completion,
        Err(AppError::BadRequest(_)) => {
            return fail(db, scope, WaitFailureReason::InvalidOutput).await;
        }
        Err(error) => return Err(error),
    };
    let max_steps = u64::try_from(run.max_steps).map_err(|_| invalid())?;
    if activation.ordinal > max_steps
        || (activation.ordinal >= max_steps
            && matches!(completion.target, TransitionTarget::Step(_)))
    {
        return fail(db, scope, WaitFailureReason::ActivationLimit).await;
    }
    if let Some(event) = event {
        let consumed = sqlx::query("UPDATE workflow_wait_events SET consumed_execution_id=$3 WHERE company_id=$1 AND run_id=$2 AND id=$4 AND consumed_execution_id IS NULL")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(event).execute(&mut *db).await?;
        if consumed.rows_affected() != 1 {
            return Err(invalid());
        }
    }
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp(),consumed_event_id=$4 WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(event).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    let successor = batch_commit::complete(
        db,
        scope,
        &activation,
        &completion,
        run.max_steps,
        completion_job::CompletionOwner::Parked,
    )
    .await?;
    Ok(WaitProgress::Completed(Box::new(CommittedWorkflowStep {
        disposition: CommitDisposition::Committed,
        output: completion.output,
        route: completion.route,
        target: completion.target,
        successor,
    })))
}
#[derive(sqlx::FromRow)]
struct ReceivedEvent {
    id: Uuid,
    payload: Value,
}
async fn expire(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run_state: &str,
) -> AppResult<WaitProgress> {
    sqlx::query("UPDATE workflow_waits SET state='expired',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    if run_state == "waiting" {
        sqlx::query("UPDATE workflow_runs SET state='failed',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    }
    waits::audit(db, scope, "wait_expired").await?;
    Ok(WaitProgress::Expired(CommitDisposition::Committed))
}
#[derive(sqlx::FromRow)]
struct SavedResult {
    committed_output: Value,
    committed_route: String,
    route_target: String,
    successor_execution_id: Option<Uuid>,
}
/// The saved completion of a completed wait. Only a message step's failed receipt completes its
/// wait through the final-error writer; with no declared route that failed the run instead.
async fn saved(
    db: &mut PgConnection,
    scope: ActivationRequest,
    disposition: CommitDisposition,
    effect: bool,
) -> AppResult<WaitProgress> {
    let saved=sqlx::query_as::<_,SavedResult>("SELECT committed_output,committed_route,route_target,successor_execution_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 AND completed_at IS NOT NULL AND committed_route IS NOT NULL")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(saved) = saved else {
        if !effect {
            return Err(invalid());
        }
        return Ok(WaitProgress::Completed(Box::new(CommittedWorkflowStep {
            disposition,
            output: Value::Null,
            route: RouteSelection::FinalError,
            target: TransitionTarget::End,
            successor: None,
        })));
    };
    let target = if saved.route_target == "$end" {
        TransitionTarget::End
    } else {
        TransitionTarget::Step(StepId::parse(saved.route_target).map_err(|_| invalid())?)
    };
    let successor = if let Some(id) = saved.successor_execution_id {
        let job=sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2 AND queue_kind='workflow'")
            .bind(scope.company.as_uuid()).bind(id).fetch_one(db).await?;
        Some(ActivationRequest {
            execution: ExecutionId::new(id),
            job: WorkflowJobId(job),
            ..scope
        })
    } else {
        None
    };
    Ok(WaitProgress::Completed(Box::new(CommittedWorkflowStep {
        disposition,
        output: saved.committed_output,
        route: saved_route(&saved.committed_route)?,
        target,
        successor,
    })))
}

/// Replay every committed route: `success`, `final_error` and a decision's `choice:<name>`.
pub(super) fn saved_route(route: &str) -> AppResult<RouteSelection> {
    Ok(match route {
        "success" => RouteSelection::Success,
        "final_error" => RouteSelection::FinalError,
        other => RouteSelection::Choice(
            crate::domain::workflow::ChoiceName::parse(
                other.strip_prefix("choice:").ok_or_else(invalid)?,
            )
            .map_err(|_| invalid())?,
        ),
    })
}

pub(super) async fn fail(
    db: &mut PgConnection,
    scope: ActivationRequest,
    reason: WaitFailureReason,
) -> AppResult<WaitProgress> {
    let code = match reason {
        WaitFailureReason::ActivationLimit => "activation_limit",
        WaitFailureReason::InvalidOutput => "invalid_output",
        WaitFailureReason::EvidenceConflict => "evidence_conflict",
    };
    sqlx::query("UPDATE workflow_waits SET state='failed',failure_code=$4,settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(code).execute(&mut *db).await?;
    sqlx::query(
        "UPDATE workflow_runs SET state='failed',waiting_reason=NULL WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .execute(&mut *db)
    .await?;
    waits::audit(db, scope, "wait_failed").await?;
    Ok(WaitProgress::Failed {
        disposition: CommitDisposition::Committed,
        reason,
    })
}
