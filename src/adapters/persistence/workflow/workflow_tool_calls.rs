//! Workflow tool calls (07-D2 §D2.3–§D2.4). One saved model call of a workflow tool starts its
//! pinned child and parks the fenced agent attempt in a single commit, so the call is either
//! fully started or not at all: it is never uncertain and needs no action marker (§D2.3a). The
//! child's terminal fact continues the agent job; the replayed call reads its result from
//! immutable facts.
use super::*;
use crate::app_error::ExecutionFailure;
use crate::application::workflow::{
    actions::{ActionOriginAuthority, ActionScope},
    activation::ActivationRequest,
    agent::AgentToolObservation,
    binding::ConfiguredBinding,
    lease::WorkflowFence,
    tools::*,
    waits::{WaitFailureReason, WaitProgress},
};
use children::{ChildOrigin, ChildStart};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

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

struct AuthorizedCall {
    action_scope: ActionScope,
    binding: Arc<ConfiguredBinding>,
    origin: ActionOriginAuthority,
    registration: Option<WorkflowToolRegistration>,
}

// Preserve account/company/source authority locks before locking the live execution fence.
async fn authorize_call_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    call: &WorkflowToolCall,
) -> AppResult<AuthorizedCall> {
    let action_scope = ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    };
    let actor = action_authority::prelock_on(db, action_scope)
        .await?
        .ok_or_else(missing)?;
    let binding_id: Uuid = sqlx::query_scalar(
        "SELECT binding_id FROM workflow_runs WHERE company_id=$1 AND id=$2 AND actor_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(actor.user_id())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let origin =
        super::agent_origin::workflow_tool_on(db, action_scope, call, actor, binding.bundle())
            .await?;
    let registration = workflow_tools::registration_on(db, scope.company, &call.name).await?;
    Ok(AuthorizedCall {
        action_scope,
        binding,
        origin,
        registration,
    })
}

#[derive(sqlx::FromRow)]
struct ParentContinuation {
    deadline: chrono::DateTime<chrono::Utc>,
    step_id: String,
    continuation_count: i16,
}

async fn parent_continuation_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<ParentContinuation> {
    sqlx::query_as(
        "SELECT run.deadline, execution.step_id, job.continuation_count \
         FROM workflow_runs AS run \
         JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3 AND job.id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .fetch_one(db)
    .await
    .map_err(Into::into)
}

async fn replayed_call_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    call: &WorkflowToolCall,
) -> AppResult<Option<AgentToolObservation>> {
    let wait = sqlx::query_as::<_, CallWait>(
        "SELECT state, child_run_id FROM workflow_waits \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND child_tool_call_id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(call.call.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    let Some(wait) = wait else { return Ok(None) };
    // The park closed the job, so a waiting call cannot coexist with a live fence.
    if wait.state != "completed" {
        return Err(invalid());
    }
    match call_result(db, scope, wait.child_run_id).await? {
        CallResult::Deliver(result) => Ok(Some(AgentToolObservation::Completed(result))),
        CallResult::InvalidOutput => Err(invalid()),
    }
}

pub(super) async fn call_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    call: &WorkflowToolCall,
) -> AppResult<AgentToolObservation> {
    let scope = fence.scope;
    let authorized = authorize_call_on(db, scope, call).await?;
    if !lease::lock_fence(db, fence, Duration::from_secs(1)).await? {
        return Err(AppError::Execution(ExecutionFailure::OwnershipLost));
    }
    if let Some(observation) = replayed_call_on(db, scope, call).await? {
        return Ok(observation);
    }
    if authorized
        .registration
        .is_none_or(|current| current.workflow != call.pin.workflow_id)
    {
        return Ok(AgentToolObservation::Completed(tool_error(
            TOOL_REVOKED,
            "This workflow tool is no longer available",
        )));
    }
    let run = parent_continuation_on(db, scope).await?;
    if let ActionOriginAuthority::Agent(selection) = &authorized.origin {
        super::agent_authority::check_execution_on(
            db,
            authorized.action_scope,
            selection.agent(),
            AgentUseMode::Tools,
        )
        .await?;
    }
    let child = super::agent_manifest::workflow_bundle_on(
        db,
        scope.company,
        scope.run,
        call.pin.workflow_id,
        call.pin.version_id,
    )
    .await?;
    if let Err(diagnostic) = child.compiled().validate_input(&call.arguments) {
        return Ok(AgentToolObservation::Completed(tool_error(
            CHILD_INVALID_INPUT,
            &diagnostic.message,
        )));
    }
    if run.continuation_count >= super::WORKFLOW_JOB_CONTINUATION_LIMIT {
        return Ok(AgentToolObservation::Completed(tool_error(
            TOOL_LIMIT,
            "This agent step cannot start another workflow tool call",
        )));
    }
    let configured =
        children::configured_child(&authorized.binding, child, call.pin.params.clone())?
            .ok_or_else(invalid)?;
    let step = StepId::parse(run.step_id).map_err(|_| invalid())?;
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
            deadline: run.deadline,
        },
    )
    .await?;
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
    let job = sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3  AND status='processing' AND worker_id=$4 AND execution_generation=$5 AND attempt_ordinal=$6 AND lock_expires_at>clock_timestamp()")
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
    content_level: i16,
    workflow_id: Uuid,
    version_id: Uuid,
    terminal_state: Option<String>,
    terminal_execution_id: Option<Uuid>,
}

/// The call's result from immutable facts: the child's terminal `parent_wakeup` fact, its
/// committed output and the parent's exact manifest member for the pinned bundle.
pub(super) async fn call_result(
    db: &mut PgConnection,
    scope: ActivationRequest,
    child: Uuid,
) -> AppResult<CallResult> {
    let outcome = sqlx::query_as::<_, ChildOutcome>(
        "SELECT child.workflow_id, child.version_id, child.content_level::smallint AS content_level, event.terminal_state, event.terminal_execution_id \
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
    if outcome.terminal_state.is_none() {
        return Err(invalid());
    }
    let level = crate::entities::access_level::AccessLevel::try_from(outcome.content_level)
        .map_err(|_| invalid())?;
    super::knowledge::raise_content_level_on(db, scope.company, scope.run, level).await?;
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
            let pinned = super::agent_manifest::workflow_bundle_on(
                db,
                scope.company,
                scope.run,
                WorkflowId::new(outcome.workflow_id),
                VersionId::new(outcome.version_id),
            )
            .await?;
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
