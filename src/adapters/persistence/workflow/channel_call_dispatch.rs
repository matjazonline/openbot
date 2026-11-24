//! The `channel.call` dispatch port (agent-workflow-call gate AWC-P03-D1 r2): one transaction
//! starts a call's child and parks the agent attempt, or answers a refused call, or consumes a
//! finished call from its immutable record and the child's terminal fact.
use super::*;
use crate::application::transport::CanonicalContent;
use crate::application::workflow::channel_call::{
    CallReply, ChannelCallDispatch, ChannelCallError, ChannelCallOutcome, ChannelCallToolCall,
    DeniedCode, RESPONSE_INVALID, REVIEW_EXPIRED, REVIEW_REJECTED, REVOKED,
};
use crate::application::workflow::{
    actions::ActionScope, agent::AgentToolObservation, lease::WorkflowFence, tools::*,
};
use crate::entities::access_level::AccessLevel;
use crate::entities::channel::{ChannelAccessMode, participant_authorized};
use channel_call_target::{CallDispatch, CallStart};
use serde_json::Value;
use std::time::Duration;

/// The whole dispatch, retries included (AWC-C003: the port's outer bound).
const DISPATCH_BOUND: Duration = Duration::from_secs(10);
/// Settings that changed between the account hint and the desk read are re-read this often
/// before the attempt fails as a transient error (D1 "Conflict"). Nothing else is retried: a
/// refused park or a lost fence fails the attempt at once.
const CONFLICT_ATTEMPTS: usize = 3;

#[async_trait]
impl ChannelCallDispatch for PostgresPersistence {
    async fn call_channel(
        &self,
        fence: WorkflowFence,
        call: ChannelCallToolCall,
    ) -> AppResult<AgentToolObservation> {
        tokio::time::timeout(DISPATCH_BOUND, async {
            for _ in 0..CONFLICT_ATTEMPTS {
                let mut tx = self.pool.begin().await?;
                // Box the transition: it may write a whole child run.
                match Box::pin(dispatch_on(&mut tx, fence, &call)).await? {
                    Some(observation) => {
                        tx.commit().await?;
                        return Ok(observation);
                    }
                    None => tx.rollback().await?,
                }
            }
            Err(AppError::ServiceUnavailable(
                "The desk's configuration kept changing; try the call again".into(),
            ))
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
}

/// One dispatch attempt; `None` when the desk's settings changed under it and nothing was
/// written, so the caller rolls back and tries again.
pub(super) async fn dispatch_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: WorkflowFence,
    call: &ChannelCallToolCall,
) -> AppResult<Option<AgentToolObservation>> {
    let dispatch = CallDispatch {
        fence,
        call: call.call,
        arguments: &call.arguments,
        origin: call.origin,
    };
    let observation = match channel_call_target::start_on(tx, &dispatch).await? {
        CallStart::Stale => return Ok(None),
        CallStart::Answered(value) => AgentToolObservation::Completed(value),
        CallStart::Started { child, parked } => {
            workflow_tool_calls::park_on(tx, fence, parked.wait.as_uuid()).await?;
            tracing::info!(
                run_id = %fence.scope.run.as_uuid(),
                child_run_id = %child.as_uuid(),
                wait_id = %parked.wait.as_uuid(),
                "channel call started"
            );
            AgentToolObservation::Parked(parked.wait)
        }
        CallStart::Replayed { child } => {
            AgentToolObservation::Completed(consume_on(tx, &dispatch, child).await?)
        }
    };
    Ok(Some(observation))
}

#[derive(sqlx::FromRow)]
struct RecordRow {
    channel_id: Uuid,
    binding_id: Uuid,
    wait_state: String,
}

#[derive(sqlx::FromRow)]
struct ChildTerminal {
    content_level: i16,
    content_exact: bool,
    terminal_state: Option<String>,
    terminal_execution_id: Option<Uuid>,
}

/// The child's settled final response: its decision and, unless suppressed, the proposal review
/// approved (AWC-P04-D1 D3).
#[derive(sqlx::FromRow)]
struct SettledReply {
    disposition: String,
    closure: Option<String>,
    content: Option<Value>,
    fields: Option<Value>,
    handed_off_run_id: Option<Uuid>,
}

/// D4 and AWC-P04-D1 r2 D3/D7: a recorded call's result, once its child has finished and the
/// wakeup has continued the caller. Authority is re-read first, on every consumption (F8); a lost
/// right discloses nothing and stores nothing. The first disclosure is stored, and replays return
/// it.
async fn consume_on(
    tx: &mut Transaction<'_, Postgres>,
    dispatch: &CallDispatch<'_>,
    child: RunId,
) -> AppResult<Value> {
    let scope = dispatch.fence.scope;
    let record = sqlx::query_as::<_, RecordRow>(
        "SELECT call.channel_id, call.binding_id, wait.state AS wait_state \
         FROM workflow_channel_calls AS call JOIN workflow_waits AS wait ON wait.company_id=call.company_id \
           AND wait.run_id=call.run_id AND wait.execution_id=call.execution_id \
           AND wait.child_tool_call_id=call.call_id AND wait.child_run_id=call.child_run_id \
         WHERE call.company_id=$1 AND call.run_id=$2 AND call.execution_id=$3 AND call.call_id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(dispatch.call.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    // The park closed the job, so a waiting call cannot coexist with a live fence.
    if record.wait_state != "completed" {
        return Err(invalid());
    }
    if !still_authorized_on(tx, dispatch, &record).await? {
        return Ok(
            ChannelCallError::new(REVOKED, "This call's desk is no longer available").to_value(),
        );
    }
    if let Some(stored) = channel_call_results::stored_on(tx, scope, dispatch.call).await? {
        absorb_on(tx, scope, &stored).await?;
        return Ok(stored.result);
    }
    let terminal = sqlx::query_as::<_, ChildTerminal>(
        "SELECT child.content_level::smallint AS content_level, child.content_exact, event.terminal_state, event.terminal_execution_id \
         FROM workflow_runs AS child \
         LEFT JOIN workflow_run_events AS event ON event.company_id=child.company_id \
           AND event.run_id=child.id AND event.sequence=child.terminal_wakeup_sequence \
           AND event.event_kind='parent_wakeup' \
         WHERE child.company_id=$1 AND child.id=$2 AND child.parent_run_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(child.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    let level = AccessLevel::try_from(terminal.content_level).map_err(|_| invalid())?;
    let (closure, result, disclosed) = match terminal.terminal_state.as_deref() {
        Some("succeeded") => match settled_outcome_on(tx, scope, child).await? {
            Some(outcome) => match outcome.to_value() {
                Ok(value) => (
                    outcome.closure(),
                    value,
                    !matches!(outcome, ChannelCallOutcome::NoReply { .. }),
                ),
                Err(_) => (
                    "error",
                    tool_error(
                        TOOL_RESULT_TOO_LARGE,
                        "The desk's answer is larger than a tool result may be",
                    ),
                    false,
                ),
            },
            None => (
                "error",
                ChannelCallError::new(RESPONSE_INVALID, "The desk's answer could not be read")
                    .to_value(),
                false,
            ),
        },
        Some("failed") => {
            let code = review_failure_on(tx, scope, child, terminal.terminal_execution_id)
                .await?
                .unwrap_or(CHILD_FAILED);
            (
                "error",
                tool_error(code, &format!("The desk's run {child} failed")),
                false,
            )
        }
        Some("cancelled") => (
            "error",
            tool_error(
                CHILD_CANCELLED,
                &format!("The desk's run {child} was cancelled"),
            ),
            false,
        ),
        _ => return Err(invalid()),
    };
    // Only a disclosed answer carries the child's level into the caller (D1.4, D3 "Absorption").
    let stored = channel_call_results::StoredResult {
        result,
        level: if disclosed { level } else { AccessLevel::L0 },
        exact: !disclosed || terminal.content_exact,
    };
    absorb_on(tx, scope, &stored).await?;
    if channel_call_results::insert_on(tx, scope, dispatch.call, closure, &stored).await? {
        let code = stored.result["error"]["code"].as_str();
        super::channel_call_note::file_on(tx, scope, dispatch.call.as_uuid(), closure, code)
            .await?;
    }
    Ok(stored.result)
}

/// The caller absorbs a stored answer's level before the model sees it; repeating it on a
/// replay is a no-op, since the run's level only rises.
async fn absorb_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: crate::application::workflow::activation::ActivationRequest,
    stored: &channel_call_results::StoredResult,
) -> AppResult<()> {
    super::knowledge::absorb_content_on(tx, scope.company, scope.run, stored.level, stored.exact)
        .await
}

/// A succeeded child's answer from its final-response decision and the approved proposal.
/// `None` when the stored content does not decode: a defect guard (AWC-C004 r2).
async fn settled_outcome_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: crate::application::workflow::activation::ActivationRequest,
    child: RunId,
) -> AppResult<Option<ChannelCallOutcome>> {
    let settled: SettledReply = sqlx::query_as(
        "SELECT decision.disposition, proposal.closure, proposal.content, proposal.fields, proposal.handed_off_run_id \
         FROM channel_reply_decisions AS decision \
         LEFT JOIN channel_reply_proposals AS proposal ON proposal.company_id=decision.company_id AND proposal.id=decision.proposal_id \
         WHERE decision.company_id=$1 AND decision.run_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(child.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    // A succeeded call child always settles a decision with its terminal (D3).
    .ok_or_else(invalid)?;
    if settled.disposition == "suppressed" {
        return Ok(Some(ChannelCallOutcome::NoReply { child }));
    }
    let Some(reply) = settled.content.as_ref().and_then(|content| {
        let text = |key: &str| content.get(key).and_then(Value::as_str);
        let content = CanonicalContent::parse(text("subject")?, text("body")?).ok()?;
        let fields = match &settled.fields {
            None => None,
            Some(Value::Object(fields)) => Some(fields.clone()),
            Some(_) => return None,
        };
        Some(CallReply { content, fields })
    }) else {
        return Ok(None);
    };
    Ok(Some(match settled.closure.as_deref() {
        Some("done") => ChannelCallOutcome::Done { child, reply },
        Some("denied") => ChannelCallOutcome::Denied {
            code: DeniedCode::Procedure,
            reason: reply.content.body_text().to_owned(),
            launched: Some((child, reply)),
        },
        Some("handed_off") => ChannelCallOutcome::HandedOff {
            child,
            to: RunId::new(settled.handed_off_run_id.ok_or_else(invalid)?),
            reply,
        },
        _ => return Err(invalid()),
    }))
}

/// A failed child whose terminal execution holds its final proposal, refused or expired in
/// review: that proposal's approval decision recorded the outcome (AWC-P04-D1 r2 D3 "Failed").
/// A rejection routed elsewhere leaves no proposal at the terminal, so it reads `child_failed`.
async fn review_failure_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: crate::application::workflow::activation::ActivationRequest,
    child: RunId,
    terminal: Option<Uuid>,
) -> AppResult<Option<&'static str>> {
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT event.event_kind FROM channel_reply_proposals AS proposal \
         JOIN workflow_run_events AS event ON event.company_id=proposal.company_id AND event.run_id=proposal.run_id \
           AND event.execution_id=proposal.execution_id AND event.decision_id=proposal.approval_decision_id \
         WHERE proposal.company_id=$1 AND proposal.run_id=$2 AND proposal.execution_id=$3 \
           AND event.event_kind IN ('message.approval_rejected','message.approval_expired') \
         ORDER BY event.sequence LIMIT 1",
    )
    .bind(scope.company.as_uuid())
    .bind(child.as_uuid())
    .bind(terminal.ok_or_else(invalid)?)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(kind.map(|kind| {
        if kind == "message.approval_rejected" {
            REVIEW_REJECTED
        } else {
            REVIEW_EXPIRED
        }
    }))
}

#[derive(sqlx::FromRow)]
struct DeskNow {
    access_mode: String,
    enabled: bool,
}

/// p01 D1.5, re-read under the account and company locks this transaction holds: the calling
/// agent's origin and execution authority, its write right on the desk, the desk enabled, and
/// the destination binding still active. Writers of those rows conflict with these share locks.
async fn still_authorized_on(
    tx: &mut Transaction<'_, Postgres>,
    dispatch: &CallDispatch<'_>,
    record: &RecordRow,
) -> AppResult<bool> {
    let scope = dispatch.fence.scope;
    let action = ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    };
    let proven = async {
        let actor = channel_call_target::caller_actor_on(tx, scope).await?;
        let caller = channel_call_target::caller_binding_on(tx, scope, actor).await?;
        let selection = agent_origin::channel_call_on(
            tx,
            action,
            actor,
            caller.bundle(),
            dispatch.origin,
            dispatch.call,
            dispatch.arguments,
        )
        .await?;
        agent_authority::check_execution_on(tx, action, selection.agent(), AgentUseMode::Tools)
            .await
    }
    .await;
    match proven {
        Ok(()) => {}
        Err(error) if lost_authority(&error) => return Ok(false),
        Err(error) => return Err(error),
    }
    let desk: Option<DeskNow> = sqlx::query_as(
        "SELECT desk.access_mode, desk.enabled FROM channels AS desk \
         JOIN departments AS unit ON unit.company_id=desk.company_id AND unit.id=desk.department_id \
         WHERE desk.company_id=$1 AND desk.id=$2 FOR SHARE OF desk FOR KEY SHARE OF unit",
    )
    .bind(scope.company.as_uuid())
    .bind(record.channel_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(desk) = desk else { return Ok(false) };
    let mode: ChannelAccessMode = desk.access_mode.parse().map_err(|_| invalid())?;
    let listed = channel_call_target::agent_participates_on(
        tx,
        scope.company,
        dispatch.origin.agent,
        record.channel_id,
    )
    .await?;
    if !desk.enabled || !participant_authorized(mode, listed, false) {
        return Ok(false);
    }
    let active: Option<bool> = sqlx::query_scalar(
        "SELECT active FROM workflow_bindings WHERE company_id=$1 AND id=$2 FOR SHARE",
    )
    .bind(scope.company.as_uuid())
    .bind(record.binding_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(active == Some(true))
}

/// A refusal of the agent's origin or execution authority that a later revocation causes, as
/// opposed to an infrastructure error, which propagates.
fn lost_authority(error: &AppError) -> bool {
    matches!(
        error,
        AppError::NotFound(_) | AppError::Forbidden(_) | AppError::BadRequest(_)
    )
}
