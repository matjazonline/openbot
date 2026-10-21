//! One-shot classification against a company's own model credential.
//!
//! Deliberately narrower than [`AgentHarness`]: `src/application/AGENTS.md` says to split a broad
//! trait rather than add optional methods with safe-looking defaults, and its one caller does not
//! want an agent. The system-prompt generator wants prose -- no tools, no memory, no approvals, no
//! trace, and nothing durable behind the call.
//!
//! The call is not metered: no `model_usage_calls` row is written for it, because that table
//! attributes usage to a task and an agent and an operator generating a prompt has neither. The
//! user accepted that gap on 2026-10-21 (06-R024, 06-F030).
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
    /// What the model is being asked to be. Written by this codebase, never by a sender.
    pub system_prompt: &'a str,
    /// What it is being asked about. Fenced by the caller when it carries untrusted text.
    pub user_prompt: &'a str,
    pub provider: &'a ModelProvider,
    pub model: &'a ModelName,
    pub api_key: &'a str,
}

/// A single answer from a model.
///
/// No method has a default body, for the reason `src/application/AGENTS.md` gives: a default that
/// returned an empty string would look like a working implementation and hand the operator an
/// empty prompt.
#[async_trait]
pub trait TextClassifier: Send + Sync {
    /// The model's reply, verbatim and untrimmed. Cleaning it up is the caller's business.
    async fn complete(&self, request: ClassificationRequest<'_>) -> AppResult<String>;
}
