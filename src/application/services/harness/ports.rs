//! The ports one agent run is expressed in: the harness itself, and what a harness reports
//! back to the application.
//!
//! Nothing here names a runtime. [`AgentHarness`] takes an [`AgentRun`] carrying a
//! harness-neutral [`AgentCapabilitySpec`] and returns an [`AgentExecutionOutput`]; the adapter
//! that implements it is the only code that knows which dialect the spec was compiled into.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::app_error::AppResult;
use crate::entities::{
    harness::{AgentCapabilitySpec, HarnessKind},
    task::TokenUsage,
    transport::RecipientRole,
    value_objects::ToolId,
};

/// What one agent run produced.
///
/// Harness-neutral already, and deliberately so: `dispatch` decides whether a reply is sent from
/// [`Self::disposition`] alone, and the durable task row stores [`Self::token_usage`] whichever
/// runtime counted it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentExecutionOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<crate::services::response_contract::StructuredResponse>,
    pub content: String,
    pub token_usage: TokenUsage,
    pub disposition: AgentExecutionDisposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Whether a run finished or parked.
///
/// [`Self::Suspended`] means a human or another agent still owes an answer, and the durable task
/// stays open; it is not a failure and its `content` is not a reply.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionDisposition {
    Completed,
    Suspended,
}

impl AgentExecutionOutput {
    /// Record how long the run took.
    ///
    /// The harness cannot know: it is handed a composed prompt and returns a response, while the
    /// clock belongs to the caller that also owns the timeout and the lease. It is stamped into
    /// the diagnostics the harness already wrote rather than added beside them, so a reader has
    /// one place to look.
    pub fn stamp_duration(&mut self, duration_ms: u64) {
        let Some(diagnostics) = self
            .metadata
            .as_mut()
            .and_then(|metadata| metadata.get_mut(EXECUTION_DIAGNOSTICS_KEY))
            .and_then(serde_json::Value::as_object_mut)
        else {
            return;
        };
        diagnostics.insert("duration_ms".to_string(), serde_json::json!(duration_ms));
    }
}

/// The metadata key a harness records its own per-run diagnostics under.
///
/// Shared because both sides write to it: the harness fills it in, and the caller stamps the one
/// field it alone can measure. It is also already in stored task rows, so it is fixed.
pub const EXECUTION_DIAGNOSTICS_KEY: &str = "execution_diagnostics";

/// Everything one harness run needs, and nothing more.
///
/// A struct rather than a nine-argument call: `src/AGENTS.md` -- any tuple with three-plus
/// elements, or two same-typed elements, becomes a struct with named fields. Two `&str` prompt
/// fields and three `Option<Arc<dyn ...>>` ports sit here side by side, and positional arguments
/// would let any two of them swap silently.
pub struct AgentRun<'a> {
    pub company_id: Option<uuid::Uuid>,
    pub execution: Option<super::runs::RunExecution>,
    pub deadline: tokio::time::Instant,
    /// Boxed: it carries every skill body, so it dominates any future or enum it lands in
    /// (`clippy::large_enum_variant`).
    pub spec: Box<AgentCapabilitySpec>,
    /// Stable identity for structured warnings and authorization context.
    pub agent_id: uuid::Uuid,
    /// Resolved from the company's encrypted model connection by the caller.
    ///
    /// A `&str` and deliberately not a newtype: it is the one value `sanitize_text` matches
    /// literally, and wrapping it invites a `Display` impl that puts it in a log line.
    pub api_key: &'a str,
    /// Already composed and fenced by the caller.
    pub full_prompt: &'a str,
    /// How many history messages the prompt carries, for the run's diagnostics.
    pub history_message_count: usize,
    /// Whether the agent was addressed directly or copied, for the runtime context block.
    pub recipient_role: Option<RecipientRole>,
    pub trace: Option<Arc<dyn HarnessTrace>>,
}

/// One way of running an agent.
///
/// The implementation owns everything between a composed prompt and a response: building its
/// runtime, granting tools, executing skills, and counting tokens. It does not own prompt
/// composition, the wall-clock deadline, or the lease -- those stay with the caller, which is why
/// they are absent from [`AgentRun`].
///
/// No method has a default body. `src/application/AGENTS.md`: a silently-successful default is how
/// a broken protocol passes its tests, and [`Self::run`] *is* this trait's correctness operation.
#[async_trait]
pub trait AgentHarness: Send + Sync {
    /// Which harness this is. [`crate::services::harness::HarnessRegistry`] checks it against the
    /// slot the implementation was registered into, so a mis-wired adapter fails at boot.
    fn kind(&self) -> HarnessKind;

    /// Run one agent turn to completion, suspension, or failure.
    ///
    /// # Cancellation
    ///
    /// This future is awaited inside the task worker's lease, so losing the lease drops it
    /// mid-run. `src/application/AGENTS.md` -- *"Losing ownership cancels the real work"* -- means
    /// that drop must actually stop the work, not just stop waiting for it. In-process that is
    /// free. A harness that hands the run to something outside this process must tear it down on
    /// drop, not only on the happy path; the requirement is written here while there is one
    /// implementation and it is trivially true, because the day it is not is the day nobody
    /// remembers to check.
    async fn run(&self, run: AgentRun<'_>) -> AppResult<AgentExecutionOutput>;
}

/// What one tool call returned.
///
/// `success: false` is a tool that ran and reported a problem the model should see, not a
/// transport fault -- those come back as `Err`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInvocation {
    pub success: bool,
    pub output: serde_json::Value,
    pub disposition: ToolInvocationDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolInvocationDisposition {
    Continue,
    Suspended,
}

impl ToolInvocation {
    /// Only a validated application rejection is model-visible. Infrastructure and ownership
    /// errors retain their type and stop the runtime before another effect.
    pub fn denial_or_error(error: crate::app_error::AppError) -> AppResult<Self> {
        match error {
            crate::app_error::AppError::BadRequest(reason) => Ok(Self::failure(reason)),
            error => Err(error),
        }
    }

    /// A tool that did what it was asked. `output` is the structured answer the model reads.
    pub fn success(output: serde_json::Value) -> Self {
        Self {
            success: true,
            output,
            disposition: ToolInvocationDisposition::Continue,
        }
    }

    /// A tool that completed its durable work and parked the enclosing run.
    pub fn suspended(output: serde_json::Value) -> Self {
        Self {
            success: true,
            output,
            disposition: ToolInvocationDisposition::Suspended,
        }
    }

    /// A tool that ran and reported a problem the model should see and can act on -- a slug
    /// already taken, a recipient outside policy. Not a transport fault: those are `Err`.
    pub fn failure(reason: impl Into<String>) -> Self {
        Self {
            success: false,
            output: serde_json::Value::String(reason.into()),
            disposition: ToolInvocationDisposition::Continue,
        }
    }

    pub const fn suspends_run(&self) -> bool {
        matches!(self.disposition, ToolInvocationDisposition::Suspended)
    }

    /// The output as a model-facing string.
    ///
    /// A plain string is rendered as itself rather than as a quoted JSON scalar, so a failure
    /// reason reads as the sentence it is -- which is what [`Self::failure`]'s callers wrote and
    /// what the runtime showed before this was a port.
    pub fn render(&self) -> String {
        match &self.output {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Trace
// ---------------------------------------------------------------------------------------------

/// Per-action tracing for a run, in whichever harness produced the actions.
///
/// This is a port rather than logging inside each tool because it sees calls no tool of ours could
/// report: the runtime's own built-ins, and anything reached over MCP. One implementation covers
/// them all, and the correlation and task ids it labels them with live on this side of the
/// boundary.
///
/// Nothing here carries arguments or outputs. They are whatever the sender wrote, and
/// `src/AGENTS.md` rules out putting message bodies in spans; what crosses is the shape of the
/// call, not its content. Even argument names stay adapter-side: an open-object schema can make
/// them sender-controlled content too.
#[async_trait]
pub trait HarnessTrace: Send + Sync {
    async fn tool_started(&self, tool: &ToolId, argument_count: usize);

    /// One finished call, retries folded in. The authoritative callback: a harness may skip
    /// [`Self::tool_started`] entirely, so nothing may depend on having seen it.
    async fn tool_finished(&self, record: ToolTraceRecord<'_>);

    /// The run parked awaiting a human decision, identified by the harness's own request id.
    async fn approval_requested(&self, request_id: &str);

    /// Control passed to another agent.
    async fn handoff(&self, from: &str, to: &str);

    /// A delegated step finished, in the state it left behind.
    async fn delegate_finished(&self, agent: &str, state: &str, duration_ms: u64);

    /// The run reported an error. Details stay on the harness side, where its credential and
    /// content redaction rules are available.
    async fn run_failed(&self);
}

/// One finished tool call, in fields that are all identifiers, counts or closed-set labels -- so
/// the whole struct is safe to attach to a log line.
pub struct ToolTraceRecord<'a> {
    pub tool: &'a ToolId,
    /// The harness's stable id for this call, for tying a start to its finish.
    pub call_id: &'a str,
    pub source: ToolTraceSource,
    pub outcome: ToolTraceOutcome,
    /// Whether the tool implementation actually ran. Not implied by [`Self::outcome`]: a cancelled
    /// or timed-out call may or may not have reached the implementation first.
    pub executed: bool,
    pub duration_ms: u64,
    pub output_bytes: usize,
    pub output_truncated: bool,
    /// How the harness's tool policy decided, when it recorded a decision.
    pub policy: Option<&'a str>,
    /// How approval decided, when approval was checked at all.
    pub approval: Option<&'a str>,
}

/// Which runtime path asked for a call.
///
/// The one genuinely harness-shaped thing in tracing, restated here so the label set stays bounded
/// and stable across harnesses -- these become metric labels, and a per-run string would be a
/// cardinality explosion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTraceSource {
    /// The model decided to call it.
    Model,
    /// A skill step.
    Skill,
    /// An action attached to a state transition.
    StateAction,
    /// A step of a plan the runtime is executing.
    Plan,
    Orchestration,
    /// A spawned sub-agent.
    Spawner,
    /// A path this port does not name. Present so a harness gaining a new call path degrades to a
    /// known label instead of forcing a release here.
    Other,
}

impl ToolTraceSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Skill => "skill",
            Self::StateAction => "state_action",
            Self::Plan => "plan",
            Self::Orchestration => "orchestration",
            Self::Spawner => "spawner",
            Self::Other => "other",
        }
    }
}

/// How a finished call ended, in one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTraceOutcome {
    Success,
    /// It ran and reported a failure.
    Failed,
    /// Blocked before the implementation ran: policy refused it, or approval did.
    NotExecuted,
    /// Parked for an application-owned continuation rather than failed.
    Suspended,
    TimedOut,
    Cancelled,
}

impl ToolTraceOutcome {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::NotExecuted => "not_executed",
            Self::Suspended => "suspended",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether this outcome is the routine one. Everything else is an operational event: the agent
    /// did not do what it decided to do.
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }
}
