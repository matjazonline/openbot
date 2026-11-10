//! Which step outputs and tool results keep a run's `content_level` exact (phase 05 E3).
//!
//! A run's level is exact only while every input it consumed was level-accounted. The two
//! persistence funnels, the step-output writer and the tool-result checkpoint update, ask
//! this module and lower `workflow_runs.content_exact` for anything not on the allowlist.
//! Anything unknown, including a step kind or tool target added later, lowers it until it is
//! classified here.
use super::agent::AgentToolTarget;
use crate::entities::access_level::{ExactnessTarget, HumanInputClass, human_input_exactness};
use serde_json::Value;

/// How one step kind's committed output is accounted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAccounting {
    /// Derived only from already accounted inputs and authored literals.
    Pure,
    /// A read that raises `content_level` itself before its output commits.
    RaisingRead,
    /// A model step: its output derives from inputs accounted elsewhere.
    ModelStep,
    /// A receipt for an effect, carrying no external content.
    Receipt,
    /// A child run; its exactness reaches the parent when the child result is absorbed.
    ChildControl,
    /// `decision.human`: the declared choice is an authored enum, free text is conversational.
    HumanDecision,
    /// `message.ask`: the output is a person's reply.
    HumanReply,
    /// External or unclassified content: the output lowers exactness.
    Unaccounted,
}

pub fn step_accounting(kind: &str) -> StepAccounting {
    match kind {
        // `data.url` only constructs a URL from inputs (no request), like `data.map`.
        "data.map" | "data.url" | "decision.rule" | "channel.no_reply" | "wait.timer" => {
            StepAccounting::Pure
        }
        "context.history" | "memory.load" => StepAccounting::RaisingRead,
        "agent.run" | "ai.classify" | "decision.agent" => StepAccounting::ModelStep,
        "message.send" | "message.reply" | "memory.save" => StepAccounting::Receipt,
        "workflow.call" | "flow.repeat" => StepAccounting::ChildControl,
        "decision.human" => StepAccounting::HumanDecision,
        "message.ask" => StepAccounting::HumanReply,
        // A local agent's output can carry content of any level from the machine it ran on.
        "local_agent.run" => StepAccounting::Unaccounted,
        _ => StepAccounting::Unaccounted,
    }
}

/// Whether committing `output` for a step of `kind` keeps the run exact.
pub fn step_output_exact(kind: &str, output: &Value) -> bool {
    match step_accounting(kind) {
        StepAccounting::Pure
        | StepAccounting::RaisingRead
        | StepAccounting::ModelStep
        | StepAccounting::Receipt
        | StepAccounting::ChildControl => true,
        StepAccounting::HumanDecision => !decision_carries_free_text(output),
        StepAccounting::HumanReply => {
            human_input_exactness(HumanInputClass::ConversationalInput, ExactnessTarget::Run)
        }
        StepAccounting::Unaccounted => false,
    }
}

/// A submission's `feedback` and `data` are typed by a person; only the choice is authored.
fn decision_carries_free_text(output: &Value) -> bool {
    let feedback = match output.get("feedback") {
        None | Some(Value::Null) => false,
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(_) => true,
    };
    (feedback || carries_data(output.get("data")))
        && !human_input_exactness(HumanInputClass::ConversationalInput, ExactnessTarget::Run)
}

fn carries_data(data: Option<&Value>) -> bool {
    match data {
        None | Some(Value::Null) => false,
        Some(Value::Object(fields)) => !fields.is_empty(),
        Some(_) => true,
    }
}

/// A reviewed channel reply commits the reviewer's submitted `data` (an edited body) as the
/// step's output, which is then published: a person typed it. Review feedback is stored for
/// people only and never reaches a model or the output.
pub fn reviewed_output_exact(submitted_data: Option<&Value>) -> bool {
    !carries_data(submitted_data)
        || human_input_exactness(HumanInputClass::ConversationalInput, ExactnessTarget::Run)
}

/// How one tool target's results are accounted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAccounting {
    /// Knowledge and memory reads raise the level in the observing transaction.
    RaisingRead,
    /// Message tools return delivery receipts.
    Receipt,
    /// Company configuration, an exact 0 under U2.
    Configuration,
    /// A pinned child workflow; its exactness is absorbed with its result.
    ChildControl,
    /// External content: the result lowers exactness.
    Unaccounted,
}

pub fn tool_accounting(target: &AgentToolTarget) -> ToolAccounting {
    match target {
        AgentToolTarget::Knowledge => ToolAccounting::RaisingRead,
        // Reads raise in their observing transaction; a save returns only its intent receipt.
        AgentToolTarget::Memory { tool } => match tool {
            crate::application::workflow::memory_tools::MemoryTool::Save => ToolAccounting::Receipt,
            _ => ToolAccounting::RaisingRead,
        },
        AgentToolTarget::Message { .. } => ToolAccounting::Receipt,
        AgentToolTarget::Provisioning { .. } => ToolAccounting::Configuration,
        AgentToolTarget::Workflow { .. } => ToolAccounting::ChildControl,
        AgentToolTarget::Http { .. } | AgentToolTarget::Mcp { .. } => ToolAccounting::Unaccounted,
    }
}

/// Whether a tool result keeps the run exact. A result for no prepared tool (the host's
/// unknown-tool refusal) is not trusted to be accounted either.
pub fn tool_result_exact(target: Option<&AgentToolTarget>) -> bool {
    match target.map(tool_accounting) {
        Some(
            ToolAccounting::RaisingRead
            | ToolAccounting::Receipt
            | ToolAccounting::Configuration
            | ToolAccounting::ChildControl,
        ) => true,
        Some(ToolAccounting::Unaccounted) | None => false,
    }
}

#[cfg(test)]
#[path = "exactness_tests.rs"]
mod tests;
