//! One-shot classification against a company's own model credential.
//!
//! Deliberately narrower than [`AgentHarness`]: `src/application/AGENTS.md` says to split a broad
//! trait rather than add optional methods with safe-looking defaults, and neither caller of this
//! one wants an agent. The spam guardrail wants a verdict string before a run starts, and the
//! system-prompt generator wants prose -- no tools, no memory, no approvals, no trace, and
//! nothing durable behind either call.
//!
//! [`AgentHarness`]: super::ports::AgentHarness

use async_trait::async_trait;

use crate::app_error::AppResult;
use crate::entities::value_objects::{ModelName, ModelProvider};

/// One completion to run, and the credential to run it with.
///
/// A struct rather than five positional arguments: `system_prompt` and `user_prompt` are two
/// `&str` of different meaning sitting next to each other, which is the argument-swap case
/// `src/AGENTS.md` names -- and swapping these two would send the caller's untrusted input in as
/// the instruction.
pub struct ClassificationRequest<'a> {
    /// What this completion is for, in one identifier.
    ///
    /// It names the completion in whatever the harness records about it and is never shown to the
    /// model. A caller-supplied label rather than a constant because two very different questions
    /// are asked through this port, and a trace that calls both "classifier" cannot tell a spam
    /// verdict from a generated prompt.
    pub purpose: &'static str,
    /// What the model is being asked to be. Written by this codebase, never by a sender.
    pub system_prompt: &'a str,
    /// What it is being asked about. Fenced by the caller when it carries untrusted text.
    pub user_prompt: &'a str,
    pub provider: &'a ModelProvider,
    pub model: &'a ModelName,
    pub api_key: &'a str,
}

/// A single classified answer from a model.
///
/// No method has a default body, for the reason `src/application/AGENTS.md` gives: the guardrail
/// treats an unreadable answer as a rejection, so a default that returned an empty string would
/// look like a working implementation and fail closed only by accident.
#[async_trait]
pub trait TextClassifier: Send + Sync {
    /// The model's reply, verbatim and untrimmed. Parsing it is the caller's business -- what
    /// counts as a usable answer differs between a JSON verdict and a generated prompt.
    async fn complete(&self, request: ClassificationRequest<'_>) -> AppResult<String>;
}
