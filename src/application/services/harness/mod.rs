//! The seam between what an agent may do and what runs it.
//!
//! An agent is described once, by an [`AgentCapabilitySpec`], in terms no runtime owns. An
//! [`AgentHarness`] compiles that description into its own dialect and executes it. Everything
//! above this module -- dispatch, the task worker, the settings pages -- speaks only the spec, and
//! [`HarnessRegistry`] is where a deployment says which harnesses it can actually run.
//!
//! Adding a second harness means implementing [`AgentHarness`] under `src/adapters/harness/` and
//! registering it. It does not mean editing the runner, the capability spec, or this module.
//!
//! [`TextClassifier`] sits beside it and deliberately does not extend it: the spam guardrail and
//! the system-prompt generator want one classified answer against a company credential, not an
//! agent, and `src/application/AGENTS.md` says to split a broad trait rather than add optional
//! methods to one.
//!
//! # Why the ports live here
//!
//! `src/application/AGENTS.md`: ports are defined where they are consumed. Dispatch is what asks
//! for a harness, so the trait and its registry are here, and the adapter is what implements them
//! -- the same shape, and the same reasoning, as [`crate::transport::TransportRegistry`].
//!
//! [`AgentCapabilitySpec`]: crate::entities::harness::AgentCapabilitySpec

pub mod approvals;
pub mod classifier;
pub mod context;
pub mod mcp;
pub mod ports;
pub mod redaction;
pub mod registry;
pub mod runs;

pub use approvals::{AgentApprovalHandler, InternalDelegationPolicy};
pub use classifier::{ClassificationRequest, TextClassifier};
pub use ports::{
    AgentExecutionDisposition, AgentExecutionOutput, AgentHarness, AgentRun, ApprovalAsk,
    ApprovalTrigger, ApprovalVerdict, EXECUTION_DIAGNOSTICS_KEY, HarnessApprovals, HarnessToolHost,
    HarnessTrace, NativeToolDeclaration, NativeToolSafety, ToolInvocation,
    ToolInvocationDisposition, ToolTraceOutcome, ToolTraceRecord, ToolTraceSource,
};
pub use redaction::sanitize_text;
pub use registry::{HarnessRegistrationError, HarnessRegistry, UnsupportedHarness};

/// Harness vocabulary that is domain-level, re-exported so this module reads as one place.
pub use crate::entities::harness::{HarnessKind, SubAgentScope};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
