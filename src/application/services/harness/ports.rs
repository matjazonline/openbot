//! The ports one agent run is expressed in: the harness itself, and the three things a harness
//! must reach back into the application for.
//!
//! Nothing here names a runtime. [`AgentHarness`] takes an [`AgentRun`] carrying a
//! harness-neutral [`AgentCapabilitySpec`] and returns an [`AgentExecutionOutput`]; the adapter
//! that implements it is the only code that knows which dialect the spec was compiled into.
//!
//! The three sub-ports exist so the traffic in the other direction stays neutral too. A harness
//! needs to ask a human for approval, to call our own tools, and to say what it did -- and all
//! three answers live in the application layer. Passing them as traits is what lets the adapter
//! answer them without reaching back into a use case, and what keeps
//! `ai_agents::hitl::ApprovalHandler` out of every file above the adapter.

use std::sync::{Arc, atomic::AtomicBool};

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

/// Everything one harness run needs, and nothing more.
///
/// A struct rather than a nine-argument call: `src/AGENTS.md` -- any tuple with three-plus
/// elements, or two same-typed elements, becomes a struct with named fields. Two `&str` prompt
/// fields and three `Option<&dyn ...>` ports sit here side by side, and positional arguments
/// would let any two of them swap silently.
pub struct AgentRun<'a> {
    /// Boxed: it carries every skill body, so it dominates any future or enum it lands in
    /// (`clippy::large_enum_variant`).
    pub spec: Box<AgentCapabilitySpec>,
    /// Resolved from the company's encrypted model connection by the caller.
    ///
    /// A `&str` and deliberately not a newtype: it is the one value `sanitize_text` matches
    /// literally, and wrapping it invites a `Display` impl that puts it in a log line.
    pub api_key: &'a str,
    /// Already composed, fenced and guardrailed by the caller.
    pub full_prompt: &'a str,
    /// How many history messages the prompt carries, for the run's diagnostics.
    pub history_message_count: usize,
    /// Whether the agent was addressed directly or copied, for the runtime context block.
    pub recipient_role: Option<RecipientRole>,
    /// `None` when nothing about this run can be approved, which is also when nothing may be.
    pub approvals: Option<&'a dyn HarnessApprovals>,
    /// `None` when this run has no native tools to offer -- see [`HarnessToolHost::available`].
    pub tool_host: Option<&'a dyn HarnessToolHost>,
    pub trace: Option<&'a dyn HarnessTrace>,
    /// Set by the approval handler or the outreach tool when the run parks awaiting a human or
    /// another agent. The caller reads it to decide `Completed` vs `Suspended`.
    pub suspended: Arc<AtomicBool>,
}

/// One way of running an agent.
///
/// The implementation owns everything between a composed prompt and a response: building its
/// runtime, granting tools, executing skills, and counting tokens. It does not own prompt
/// composition, the spam guardrail, the wall-clock deadline, or the lease -- those stay with the
/// caller, which is why they are absent from [`AgentRun`].
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

// ---------------------------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------------------------

/// Deciding whether an agent may do something a human should see first.
///
/// Tool approval is the security boundary (`src/application/AGENTS.md`), so this port carries an
/// authorization decision and every implementation states its own: there is no default body to
/// fall through to an accidental approval.
#[async_trait]
pub trait HarnessApprovals: Send + Sync {
    /// Decide one approval trigger, parking the run if a human must answer.
    ///
    /// Returning `Err` is not "undecided": the caller treats it as a rejection, which is the
    /// fail-closed direction and the one the current handler already takes.
    async fn decide(&self, ask: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict>;
}

/// One approval request, restated in terms no harness owns.
///
/// Wider than the trigger alone because both extra fields are load-bearing: [`Self::message`] is
/// the summary a human reads in the approval mail, and [`Self::context`] is stored verbatim on the
/// approval row so the decision can be audited against what the agent actually saw.
pub struct ApprovalAsk<'a> {
    pub trigger: ApprovalTrigger<'a>,
    /// The runtime's own wording for what it is about to do. Empty when it offered none, in which
    /// case the handler falls back to describing the trigger.
    pub message: &'a str,
    /// Whatever the runtime attached to the request. An object, or `Null` when there is none.
    pub context: &'a serde_json::Value,
}

/// What an agent is asking permission for.
///
/// The serialized shape is part of the contract, not an implementation detail: it is stored in
/// `human_approvals.payload`, so it must keep matching the rows already written -- an
/// externally-tagged `type` discriminator with the runtime's own field names.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApprovalTrigger<'a> {
    Tool {
        name: &'a str,
        args: &'a serde_json::Value,
    },
    /// `matched` is the matched *expression*, not a boolean: the runtime reports which branch
    /// fired, and a `bool` here would throw that away.
    Condition {
        name: &'a str,
        matched: &'a str,
    },
    State {
        from: Option<&'a str>,
        to: &'a str,
    },
}

impl ApprovalTrigger<'_> {
    /// The `action_type` stored on the approval row.
    ///
    /// These three strings are already in the database, so they are fixed: they must keep matching
    /// what the runtime's own `trigger_type()` produced.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Tool { .. } => "tool",
            Self::Condition { .. } => "condition",
            Self::State { .. } => "state",
        }
    }
}

/// The answer to one [`ApprovalAsk`].
///
/// `Rejected` covers both "a human said no" and "a human has not answered yet" -- the run stops
/// either way, and the difference is carried by the run's `suspended` flag rather than here,
/// because that is what decides whether the durable task stays open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalVerdict {
    Approved,
    Rejected { reason: String },
}

impl ApprovalVerdict {
    /// Reject with `reason`, saving the `.to_string()` at each call site.
    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected {
            reason: reason.into(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Native tools
// ---------------------------------------------------------------------------------------------

/// Our own tools, offered to whichever harness is running.
///
/// [`Self::available`] replaces today's "context present implies tool registered" coupling: the
/// host reports which of our tools this particular run can actually serve, and the harness grants
/// the intersection of that with the spec's grant list. Registration was never a grant, and this
/// keeps it that way -- see `docs/custom_tools.md`.
#[async_trait]
pub trait HarnessToolHost: Send + Sync {
    /// The native tools this run may use, in a stable order.
    fn available(&self) -> &[ToolId];

    /// Run one native tool.
    ///
    /// A dispatcher, not a schema registry: each tool's JSON schema stays with the tool, and the
    /// adapter reads it from there when it declares the tool to its runtime.
    async fn invoke(&self, id: &ToolId, args: serde_json::Value) -> AppResult<ToolInvocation>;
}

/// What one native tool call returned.
///
/// `success: false` is a tool that ran and reported a problem the model should see, not a
/// transport fault -- those come back as `Err` from [`HarnessToolHost::invoke`].
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInvocation {
    pub success: bool,
    pub output: serde_json::Value,
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
/// call, not its content. [`HarnessTrace::tool_started`] is the one exception and takes the
/// arguments only so the implementation can record their *names*.
#[async_trait]
pub trait HarnessTrace: Send + Sync {
    async fn tool_started(&self, tool: &ToolId, args: &serde_json::Value);

    /// One finished call, retries folded in. The authoritative callback: a harness may skip
    /// [`Self::tool_started`] entirely, so nothing may depend on having seen it.
    async fn tool_finished(&self, record: ToolTraceRecord<'_>);

    /// The run parked awaiting a human decision, identified by the harness's own request id.
    async fn approval_requested(&self, request_id: &str);

    /// Control passed to another agent.
    async fn handoff(&self, from: &str, to: &str, reason: &str);

    /// A delegated step finished, in the state it left behind.
    async fn delegate_finished(&self, agent: &str, state: &str, duration_ms: u64);

    /// The run reported an error. `error` is already rendered, and already sanitized.
    async fn run_failed(&self, error: &str);
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
    pub cancellation_reason: Option<&'a str>,
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
    TimedOut,
    Cancelled,
}

impl ToolTraceOutcome {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::NotExecuted => "not_executed",
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
