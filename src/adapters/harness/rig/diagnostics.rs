//! A closed projection of execution state. Provider responses and replay blocks never enter it.
use crate::{
    entities::value_objects::ToolId,
    services::harness::{AgentExecutionDisposition, AgentExecutionOutput, AgentRun, runs::*},
};
use serde_json::json;

#[derive(Default, Clone, Copy)]
pub(super) struct InputCounts {
    prompt_characters: usize,
    history_message_count: usize,
}
impl From<&AgentRun<'_>> for InputCounts {
    fn from(run: &AgentRun<'_>) -> Self {
        Self {
            prompt_characters: run.full_prompt.chars().count(),
            history_message_count: run.history_message_count,
        }
    }
}

/// Used by the final-candidate gate as well as publication, so diagnostics count toward the same
/// serialized output ceiling before a candidate is committed.
pub(super) struct OutputContext {
    pub inputs: InputCounts,
    pub supported_tools: Vec<ToolId>,
}

pub(super) fn output(
    checkpoint: &RunCheckpoint,
    inputs: InputCounts,
    supported_tools: Vec<ToolId>,
    content: String,
    disposition: AgentExecutionDisposition,
) -> AgentExecutionOutput {
    let usage = RunUsage::from(checkpoint);
    let validation = if checkpoint.identity.response_contract.is_none() {
        "not_configured"
    } else if checkpoint.state == RunState::InvalidOutput {
        "invalid"
    } else if disposition == AgentExecutionDisposition::Completed {
        "validated"
    } else {
        "pending"
    };
    // Only identities from the compiled catalogue are allowed. Model-supplied names are content.
    let tool_names: std::collections::BTreeSet<_> = checkpoint
        .turns
        .iter()
        .flat_map(|turn| &turn.calls)
        .map(|call| {
            if supported_tools.contains(&call.tool_id) {
                call.tool_id.as_str()
            } else {
                "unknown"
            }
        })
        .collect();
    let metadata = json!({
        "harness_run_id": checkpoint.run_id,
        "execution_diagnostics": {
            "harness": checkpoint.identity.harness,
            "provider": checkpoint.identity.provider,
            "model": checkpoint.identity.model,
            "prompt_characters": inputs.prompt_characters,
            "response_characters": content.chars().count(),
            "history_message_count": inputs.history_message_count,
            "token_usage_source": usage.source,
            "unreported_usage_calls": usage.unreported_calls,
            "unresolved_model_calls": usage.unresolved_calls,
            "model_calls": checkpoint.reservations.len(),
            "tool_call_count": checkpoint.invocations.len(),
            "tool_names": tool_names,
            "supported_tool_ids": supported_tools,
            "structured_validation": validation,
            "repair_call_count": checkpoint.repair_count(),
            "terminal_invalid_output_reason": if checkpoint.state == RunState::InvalidOutput {
                checkpoint.turns.last().and_then(|turn| turn.invalid_response).map(|reason| reason.code())
            } else { None },
        }
    });
    AgentExecutionOutput {
        structured: None,
        content,
        disposition,
        token_usage: usage.tokens,
        metadata: Some(metadata),
    }
}
