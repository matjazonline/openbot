//! Workflow tool calls (07-D2 §D2.3–§D2.4). One saved model call of a workflow tool starts its
//! pinned child and parks the fenced agent attempt in a single commit, so the call is either
//! fully started or not at all: it is never uncertain and needs no action marker (§D2.3a). The
//! child's terminal fact continues the agent job; the replayed call reads its result from
//! immutable facts.
use super::*;
use crate::app_error::ExecutionFailure;
use crate::application::workflow::{
    activation::ActivationRequest,
    agent::AgentToolObservation,
    lease::WorkflowFence,
    tools::*,
    waits::{WaitFailureReason, WaitProgress},
};
use children::{ChildOrigin, ChildStart};
use serde_json::Value;
use std::time::Duration;

#[async_trait]
impl WorkflowToolDispatch for PostgresPersistence {
    async fn call_workflow_tool(
        &self,
        fence: WorkflowFence,
        call: WorkflowToolCall,
    ) -> AppResult<AgentToolObservation> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            // Box the transition: it may write a whole child run.
            let observation = Box::pin(call_on(&mut tx, fence, &call)).await?;
            tx.commit().await?;
            Ok(observation)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
}

#[derive(sqlx::FromRow)]
struct CallWait {
    state: String,
    child_run_id: Uuid,
}

pub(super) async fn call_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    call: &WorkflowToolCall,
) -> AppResult<AgentToolObservation> {
    let scope = fence.scope;
    // 0. Authority before run (the existing lock order): a revocation or re-point waits for us.
    let registration = workflow_tools::registration_on(db, scope.company, &call.name).await?;
    // 1. The run, execution and job under the live fence.
    if !lease::lock_fence(db, fence, Duration::from_secs(1)).await? {
        return Err(AppError::Execution(ExecutionFailure::OwnershipLost));
    }
    // 2. The replay after the continuation: the call's child already finished.
    let existing = sqlx::query_as::<_, CallWait>(
        "SELECT state, child_run_id FROM workflow_waits \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND child_tool_call_id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(call.call.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    if let Some(wait) = existing {
        // The park closed the job, so a waiting call cannot coexist with a live fence.
        if wait.state != "completed" {
            return Err(invalid());
        }
        return match call_result(db, scope, wait.child_run_id).await? {
            CallResult::Deliver(result) => Ok(AgentToolObservation::Completed(result)),
            CallResult::InvalidOutput => Err(invalid()),
        };
    }
    // 3. The run rule (P07-Q2): the name is still registered for the same workflow.
    if registration.is_none_or(|current| current.workflow != call.pin.workflow_id) {
        return Ok(AgentToolObservation::Completed(tool_error(
            TOOL_REVOKED,
            "This workflow tool is no longer available",
        )));
    }
    let run: (Uuid, chrono::DateTime<chrono::Utc>, String, i16) = sqlx::query_as(
        "SELECT run.binding_id, run.deadline, execution.step_id, job.continuation_count \
         FROM workflow_runs AS run \
         JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3 AND job.id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .fetch_one(&mut *db)
    .await?;
    let (binding_id, deadline, step, continuations) = run;
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let child = binding
        .bundle()
        .children()
        .get(&call.pin.version_id)
        .ok_or_else(invalid)?;
    // 4. Arguments the child refuses go back to the model; nothing is created.
    if let Err(diagnostic) = child.compiled().validate_input(&call.arguments) {
        return Ok(AgentToolObservation::Completed(tool_error(
            CHILD_INVALID_INPUT,
            &diagnostic.message,
        )));
    }
    // 5. A parked call must be able to continue its job.
    if continuations >= 63 {
        return Ok(AgentToolObservation::Completed(tool_error(
            TOOL_LIMIT,
            "This agent step cannot start another workflow tool call",
        )));
    }
    // 6. The child's own configuration: frozen params, the parent's resources for its slots.
    let configured =
        children::child_binding(&binding, call.pin.version_id, call.pin.params.clone())?
            .ok_or_else(invalid)?;
    // 7. The child and the parent's wait, keyed by the saved call.
    let step = StepId::parse(step).map_err(|_| invalid())?;
    let parked = children::create_on(
        db,
        scope,
        &ChildStart {
            step: &step,
            origin: ChildOrigin::Call {
                call: call.call,
                attempt: fence.attempt.0,
            },
            binding: &configured,
            input: &call.arguments,
            deadline,
        },
    )
    .await?;
    // 8. Close the fenced attempt and job; the run waits on the child.
    park_on(db, fence, parked.wait.as_uuid()).await?;
    Ok(AgentToolObservation::Parked(parked.wait))
}

/// The fenced attempt and job close with `child_parked`; the SQL completion guard requires this
/// execution's waiting call wait for exactly this attempt (07-D2 §D2.5).
async fn park_on(db: &mut PgConnection, fence: WorkflowFence, wait: Uuid) -> AppResult<()> {
    let scope = fence.scope;
    let closed = sqlx::query("UPDATE task_attempts SET status='completed',stop_reason='child_parked',finished_at=clock_timestamp() WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status='processing'")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0)
        .execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='child_run' WHERE company_id=$1 AND id=$2 AND state IN ('queued','running')")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    let job = sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='processing' AND worker_id=$4 AND execution_generation=$5 AND attempt_ordinal=$6 AND lock_expires_at>clock_timestamp()")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid())
        .bind(fence.worker.0).bind(fence.generation.0).bind(fence.attempt.0)
        .execute(&mut *db).await?;
    if closed.rows_affected() != 1 || job.rows_affected() != 1 {
        return Err(persistence_bound::refused("tool park"));
    }
    waits::audit_wait(db, scope, wait, "wait_parked").await?;
    Ok(())
}

/// What a finished call answers (07-D2 §D2.4).
pub(super) enum CallResult {
    /// The child's output, or a typed error the model receives instead of a result.
    Deliver(Value),
    /// The child's stored output breaks its pinned schema: a defect, never delivered.
    InvalidOutput,
}

#[derive(sqlx::FromRow)]
struct ChildOutcome {
    version_id: Uuid,
    terminal_state: Option<String>,
    terminal_execution_id: Option<Uuid>,
}

/// The call's result from immutable facts: the child's terminal `parent_wakeup` fact, its
/// committed output and the parent's frozen copy of the pinned bundle.
pub(super) async fn call_result(
    db: &mut PgConnection,
    scope: ActivationRequest,
    child: Uuid,
) -> AppResult<CallResult> {
    let outcome = sqlx::query_as::<_, ChildOutcome>(
        "SELECT child.version_id, event.terminal_state, event.terminal_execution_id \
         FROM workflow_runs AS child \
         LEFT JOIN workflow_run_events AS event ON event.company_id = child.company_id \
           AND event.run_id = child.id AND event.sequence = child.terminal_wakeup_sequence \
           AND event.event_kind = 'parent_wakeup' \
         WHERE child.company_id = $1 AND child.id = $2 AND child.parent_run_id = $3",
    )
    .bind(scope.company.as_uuid())
    .bind(child)
    .bind(scope.run.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let message = |state: &str| format!("Workflow tool child run {child} {state}");
    let result = match outcome.terminal_state.as_deref() {
        Some("succeeded") => {
            let output: Value = sqlx::query_scalar(
                "SELECT committed_output FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 AND completed_at IS NOT NULL",
            )
            .bind(scope.company.as_uuid())
            .bind(child)
            .bind(outcome.terminal_execution_id.ok_or_else(invalid)?)
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(invalid)?;
            let binding_id: Uuid = sqlx::query_scalar(
                "SELECT binding_id FROM workflow_runs WHERE company_id=$1 AND id=$2",
            )
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(&mut *db)
            .await?;
            let binding = admission_binding::read_saved_binding(
                db,
                scope.company,
                WorkflowBindingId::new(binding_id),
                scope.run.as_uuid(),
            )
            .await?;
            let pinned = binding
                .bundle()
                .children()
                .get(&VersionId::new(outcome.version_id))
                .ok_or_else(invalid)?;
            if pinned.compiled().validate_output(&output).is_err() {
                return Ok(CallResult::InvalidOutput);
            }
            if serde_json::to_vec(&output).map_err(|_| invalid())?.len() > MAX_TOOL_RESULT_BYTES {
                tool_error(
                    TOOL_RESULT_TOO_LARGE,
                    "The workflow tool's result is larger than a tool result may be",
                )
            } else {
                output
            }
        }
        Some("failed") => tool_error(CHILD_FAILED, &message("failed")),
        Some("cancelled") => tool_error(CHILD_CANCELLED, &message("was cancelled")),
        _ => return Err(invalid()),
    };
    Ok(CallResult::Deliver(result))
}

/// The child's terminal fact continues the agent job: complete the call's wait, return the run
/// to running and reopen the parked job (07-D2 §D2.4). The replayed call reads the result.
pub(super) async fn continue_call(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
    child: Uuid,
) -> AppResult<WaitProgress> {
    match call_result(db, scope, child).await? {
        CallResult::InvalidOutput => {
            wait_commit::fail(db, scope, WaitFailureReason::InvalidOutput).await
        }
        CallResult::Deliver(_) => {
            let settled = sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND id=$4 AND reason='child_run' AND child_tool_call_id IS NOT NULL AND state='waiting'")
                .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).execute(&mut *db).await?;
            if settled.rows_affected() != 1 {
                return Err(invalid());
            }
            sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2 AND state='waiting' AND waiting_reason='child_run'")
                .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
            decisions::approvals::reopen_job(db, scope).await?;
            waits::audit_wait(db, scope, wait, "wait_completed").await?;
            Ok(WaitProgress::Continued)
        }
    }
}

/// Supervisor proof that this exact fence was closed by the call's park and owns `wait`.
pub(super) async fn confirm_suspension(
    persistence: &PostgresPersistence,
    fence: WorkflowFence,
    wait: Uuid,
) -> AppResult<bool> {
    tokio::time::timeout(
        Duration::from_secs(1),
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM task_attempts AS attempt \
               JOIN workflow_waits AS wait ON wait.company_id=$1 AND wait.execution_id=$2 AND wait.id=$3 \
              WHERE attempt.task_id=$4 AND attempt.attempt_number=$5 AND attempt.worker_id=$6 AND attempt.execution_generation=$7 \
                AND attempt.status='completed' AND attempt.stop_reason='child_parked' \
                AND wait.reason='child_run' AND wait.child_tool_call_id IS NOT NULL \
                AND wait.parked_attempt_number=attempt.attempt_number)",
        )
        .bind(fence.scope.company.as_uuid())
        .bind(fence.scope.execution.as_uuid())
        .bind(wait)
        .bind(fence.scope.job.0)
        .bind(fence.attempt.0)
        .bind(fence.worker.0)
        .bind(fence.generation.0)
        .fetch_one(persistence.pool()),
    )
    .await
    .map_err(|_| lease::timed_out())?
    .map_err(Into::into)
}
