//! What run inspection shows of a `channel.call` (agent-workflow-call p05 B2, gate AWC-P05-D2 r2):
//! the call's phase, derived from durable rows only, and the bounded outcome signal.
//!
//! Nothing here reads call arguments, fields, results or desk configuration; the persistence
//! read selects ids, states and codes, and the route decides which links and names a viewer
//! may see.
use super::agent::AgentToolObservation;
use super::channel_call::{ChannelCallDispatch, ChannelCallToolCall};
use super::lease::WorkflowFence;
use crate::app_error::AppResult;
use crate::domain::monitoring::MonitoringService;
use crate::domain::workflow::RunId;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

/// A job that is `pending` and due for longer than this, or `processing` with an expired lease,
/// is stalled (gate D2 r2, D1). The same predicate judges the child's and the caller's job.
pub const STALLED_AFTER_SECONDS: i64 = 300;

/// The caller's call wait (`workflow_waits.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallWait {
    Waiting,
    Completed,
    Expired,
    Failed,
    Cancelled,
}

impl CallWait {
    pub fn parse(state: &str) -> Option<Self> {
        Some(match state {
            "waiting" => Self::Waiting,
            "completed" => Self::Completed,
            "expired" => Self::Expired,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Expired => "expired",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// A run's state (`workflow_runs.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallRunState {
    Queued,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl CallRunState {
    pub fn parse(state: &str) -> Option<Self> {
        Some(match state {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "waiting" => Self::Waiting,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// The stored result row's closure and its denied or error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCallResult {
    pub closure: String,
    pub code: Option<String>,
}

/// The durable facts one call's phase is decided from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallPhaseFacts {
    pub stored: Option<StoredCallResult>,
    pub wait: CallWait,
    pub child: CallRunState,
    pub child_waiting_reason: Option<String>,
    /// The child's open decision when it is a final-reply approval (`reply_proposal_id` set).
    pub final_review: Option<Uuid>,
    pub child_stalled: bool,
    pub caller: CallRunState,
    pub caller_execution_completed: bool,
    /// The caller's agent checkpoint holds this call's invocation as completed: the caller
    /// consumed the answer, stored or not (`revoked` stores none; gate F8).
    pub caller_consumed: bool,
    pub caller_stalled: bool,
}

/// One call's phase (gate AWC-P05-D2 r2, D1 rules 1–4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallPhase {
    Settled {
        closure: String,
        code: Option<String>,
    },
    ChildRunning {
        stalled: bool,
    },
    AwaitingFinalReview {
        decision: Uuid,
    },
    ChildWaiting {
        reason: Option<String>,
    },
    /// The child succeeded and its answer is not consumed yet.
    ResponseReady,
    ChildEnded {
        state: CallRunState,
    },
    /// The caller resumed and has not consumed the answer yet.
    AwaitingCaller {
        stalled: bool,
    },
    ClosedWithoutResult {
        wait: CallWait,
        caller: CallRunState,
    },
}

/// Rules 1–4, in order. Pure; every combination is unit-tested.
pub fn call_phase(facts: &CallPhaseFacts) -> CallPhase {
    if let Some(stored) = &facts.stored {
        return CallPhase::Settled {
            closure: stored.closure.clone(),
            code: stored.code.clone(),
        };
    }
    match facts.wait {
        CallWait::Waiting => child_phase(facts),
        CallWait::Completed
            if !facts.caller_consumed
                && !facts.caller_execution_completed
                && !facts.caller.terminal() =>
        {
            CallPhase::AwaitingCaller {
                stalled: facts.caller_stalled,
            }
        }
        wait => CallPhase::ClosedWithoutResult {
            wait,
            caller: facts.caller,
        },
    }
}

fn child_phase(facts: &CallPhaseFacts) -> CallPhase {
    match facts.child {
        CallRunState::Queued | CallRunState::Running => CallPhase::ChildRunning {
            stalled: facts.child_stalled,
        },
        CallRunState::Waiting => match facts.final_review {
            Some(decision) => CallPhase::AwaitingFinalReview { decision },
            None => CallPhase::ChildWaiting {
                reason: facts.child_waiting_reason.clone(),
            },
        },
        CallRunState::Succeeded => CallPhase::ResponseReady,
        state @ (CallRunState::Failed | CallRunState::Cancelled) => CallPhase::ChildEnded { state },
    }
}

impl CallPhase {
    /// The operator-facing text. Codes and states only; never content.
    pub fn label(&self) -> String {
        match self {
            Self::Settled {
                closure,
                code: Some(code),
            } => format!("Settled: {closure} ({code})"),
            Self::Settled { closure, .. } => format!("Settled: {closure}"),
            Self::ChildRunning { stalled: false } => "Child running".into(),
            Self::ChildRunning { stalled: true } => {
                "Child stalled: its job has not progressed".into()
            }
            Self::AwaitingFinalReview { .. } => "Awaiting final review".into(),
            Self::ChildWaiting {
                reason: Some(reason),
            } => format!("Child waiting ({reason})"),
            Self::ChildWaiting { reason: None } => "Child waiting".into(),
            Self::ResponseReady => "Response ready; the caller has not resumed".into(),
            Self::ChildEnded { state } => {
                format!("Child {}; the caller has not resumed", state.as_str())
            }
            Self::AwaitingCaller { stalled: false } => "Awaiting the caller".into(),
            Self::AwaitingCaller { stalled: true } => {
                "Caller stalled: its job has not progressed".into()
            }
            Self::ClosedWithoutResult { wait, caller } => format!(
                "Closed without a stored result (wait {}, caller {})",
                wait.as_str(),
                caller.as_str()
            ),
        }
    }
}

/// One call of a caller run, as inspection reads it. The route authorizes the run, desk and
/// responder references before rendering any of them.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelCallItem {
    pub call: Uuid,
    pub execution: Uuid,
    pub agent: Uuid,
    pub agent_revision: Uuid,
    pub desk: Uuid,
    pub desk_name: String,
    pub responder: Uuid,
    pub responder_revision: i64,
    pub child: RunId,
    /// The child's association and whether the viewer's knowledge level reads it; the route adds
    /// the association check before linking it (as `visible_run` does).
    pub child_association: super::RelatedAssociation,
    pub child_readable: bool,
    pub scope_manifest: Uuid,
    pub deadline: DateTime<Utc>,
    pub past_deadline: bool,
    pub created_at: DateTime<Utc>,
    pub phase: CallPhase,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChannelCallPage {
    pub items: Vec<ChannelCallItem>,
    pub truncated: bool,
}

impl ChannelCallPage {
    /// A restricted run keeps ids, phases and codes only (gate D1): no desk names.
    pub fn restrict(&mut self) {
        for item in &mut self.items {
            item.desk_name.clear();
        }
    }
}

/// The call that launched this run, on the child's page.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelCallOrigin {
    pub call: Uuid,
    pub caller: RunId,
    pub caller_association: super::RelatedAssociation,
    pub caller_readable: bool,
    pub caller_execution: Uuid,
    pub agent: Uuid,
    pub desk: Uuid,
    pub desk_name: String,
    pub caller_manifest: Uuid,
}

/// The bounded `workflow_channel_call_outcomes_total{outcome}` label for one dispatch
/// observation. A rate signal only: stuck calls are found from durable rows (gauges and
/// phases), and a replayed consumption may be counted again.
pub fn outcome_label(observation: &AppResult<AgentToolObservation>) -> &'static str {
    match observation {
        Ok(AgentToolObservation::Parked(_)) => "launched",
        Ok(AgentToolObservation::Completed(value)) => {
            match value.get("closure").and_then(|closure| closure.as_str()) {
                Some("done") => "done",
                Some("denied") => "denied",
                Some("handed_off") => "handed_off",
                Some("no_reply") => "no_reply",
                _ if refusal(value) => "refused",
                _ => "error",
            }
        }
        Ok(AgentToolObservation::Uncertain) | Err(_) => "error",
    }
}

/// A typed error launch answers before any row exists (p01 D1.1).
fn refusal(value: &serde_json::Value) -> bool {
    use super::channel_call::*;
    let code = value.pointer("/error/code").and_then(|code| code.as_str());
    matches!(
        code,
        Some(code) if [INVALID_INPUT, DEADLINE_INVALID, TARGET_UNAVAILABLE, CYCLE,
            INTAKE_NOT_DECLARED, INTAKE_INVALID, AUDIENCE_REFUSED].contains(&code)
    )
}

/// Counts each dispatch observation, then returns it unchanged.
pub struct MonitoredChannelCalls {
    inner: Arc<dyn ChannelCallDispatch>,
    monitoring: Arc<dyn MonitoringService>,
}

impl MonitoredChannelCalls {
    pub fn new(
        inner: Arc<dyn ChannelCallDispatch>,
        monitoring: Arc<dyn MonitoringService>,
    ) -> Self {
        Self { inner, monitoring }
    }
}

#[async_trait::async_trait]
impl ChannelCallDispatch for MonitoredChannelCalls {
    async fn call_channel(
        &self,
        fence: WorkflowFence,
        call: ChannelCallToolCall,
    ) -> AppResult<AgentToolObservation> {
        // `async_trait` boxes the inner future, so this level holds a pointer, not its state.
        let observation = self.inner.call_channel(fence, call).await;
        self.monitoring.increment_counter(
            "workflow_channel_call_outcomes_total",
            1,
            &[("outcome", outcome_label(&observation))],
        );
        observation
    }
}

#[cfg(test)]
#[path = "channel_call_views_tests.rs"]
mod tests;
