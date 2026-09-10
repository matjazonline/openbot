//! Explicit conversion between provider content and the application-owned replay schema.
use crate::{
    app_error::{AppError, AppResult},
    services::harness::runs::*,
};
use rig::completion::{
    CompletionResponse,
    message::{
        AssistantContent, Message, ProviderCallId, Reasoning, ReasoningContent, Text, ToolCall,
        ToolCallId, ToolFunction, ToolResultContent, UserContent,
    },
};

fn invalid() -> AppError {
    AppError::Execution(crate::app_error::ExecutionFailure::Protocol)
}

pub(super) fn capture(
    provider: &str,
    reservation: &ModelReservation,
    response: CompletionResponse,
) -> AppResult<SavedModelTurn> {
    let usage = captured_usage(&response.usage, reservation);
    let mut calls = Vec::new();
    let mut text = String::new();
    let mut content = Vec::new();
    for part in response.choice {
        content.push(match part {
            AssistantContent::Text(value) => {
                text.push_str(&value.text);
                SavedAssistantContent::Text {
                    text: value.text,
                    parameters: value
                        .additional_params
                        .map(|params| serde_json::Value::Object(params.as_map().clone())),
                }
            }
            AssistantContent::ToolCall(call) => {
                let ordinal = u16::try_from(calls.len()).map_err(|_| invalid())?;
                let wire_id = call.provider.as_ref().map(|wire| wire.call_id.clone());
                let item_id = call.provider.as_ref().and_then(|wire| wire.item_id.clone());
                calls.push(SavedToolCall {
                    invocation_id: InvocationId(uuid::Uuid::new_v4()),
                    call_id: call.id.to_string(),
                    item_id,
                    tool_id: call.function.name.into(),
                    arguments: call.function.arguments,
                });
                SavedAssistantContent::Call {
                    ordinal,
                    wire_id,
                    signature: call.signature,
                    parameters: call.additional_params,
                }
            }
            AssistantContent::Reasoning(reasoning) => SavedAssistantContent::Reasoning {
                id: reasoning.id,
                content: reasoning
                    .content
                    .into_iter()
                    .map(|part| match part {
                        ReasoningContent::Text { text, signature } => {
                            SavedReasoning::Text { text, signature }
                        }
                        ReasoningContent::Encrypted(data) => SavedReasoning::Encrypted { data },
                        ReasoningContent::Redacted { data } => SavedReasoning::Redacted { data },
                        ReasoningContent::Summary(text) => SavedReasoning::Summary { text },
                    })
                    .collect(),
            },
            AssistantContent::Image(_) => return Err(invalid()),
        });
    }
    if content.is_empty() {
        return Err(invalid());
    }
    let replay = AssistantReplay {
        message_id: response.message_id,
        response_id: response.response_id,
        content,
    };
    Ok(SavedModelTurn {
        token_usage_source: usage.source,
        invalid_response: None,
        request_id: reservation.request_id,
        text,
        calls,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        continuation: vec![ContinuationBlock {
            schema_version: 1,
            provider: provider.into(),
            kind: "assistant_v1".into(),
            data: serde_json::to_value(replay).map_err(|_| invalid())?,
        }],
    })
}

struct CapturedUsage {
    source: TokenUsageSource,
    input_tokens: u64,
    output_tokens: u64,
}

fn captured_usage(usage: &rig::completion::Usage, reservation: &ModelReservation) -> CapturedUsage {
    // Rig normalizes missing provider counters to zero. Treat each absent side explicitly;
    // reservations are conservative upper estimates and remain stable after candidate redaction.
    let input_reported = usage.input_tokens > 0;
    let output_reported = usage.output_tokens > 0;
    let token_usage_source = match (input_reported, output_reported) {
        (true, true) => TokenUsageSource::Reported,
        (false, false) => TokenUsageSource::Estimated,
        _ => TokenUsageSource::Mixed,
    };
    let input_tokens = if input_reported {
        usage.input_tokens
    } else {
        reservation.input_tokens
    };
    let output_tokens = if output_reported {
        usage.output_tokens
    } else {
        reservation.output_tokens
    };
    CapturedUsage {
        source: token_usage_source,
        input_tokens,
        output_tokens,
    }
}

fn replay(blocks: &[ContinuationBlock]) -> AppResult<AssistantReplay> {
    if blocks.len() != 1 || blocks[0].schema_version != 1 || blocks[0].kind != "assistant_v1" {
        return Err(invalid());
    }
    serde_json::from_value(blocks[0].data.clone()).map_err(|_| invalid())
}

fn restore_call(call: &SavedToolCall, part: &SavedAssistantContent) -> AppResult<ToolCall> {
    let SavedAssistantContent::Call {
        wire_id,
        signature,
        parameters,
        ..
    } = part
    else {
        return Err(invalid());
    };
    Ok(ToolCall {
        id: ToolCallId::new(&call.call_id).ok_or_else(invalid)?,
        provider: wire_id.as_ref().map(|id| ProviderCallId {
            call_id: id.clone(),
            item_id: call.item_id.clone(),
        }),
        function: ToolFunction {
            name: call.tool_id.to_string(),
            arguments: call.arguments.clone(),
        },
        signature: signature.clone(),
        additional_params: parameters.clone(),
    })
}

fn assistant(calls: &[SavedToolCall], blocks: &[ContinuationBlock]) -> AppResult<Message> {
    let replay = replay(blocks)?;
    let content = replay
        .content
        .iter()
        .map(|part| {
            Ok(match part {
                SavedAssistantContent::Text { text, parameters } => AssistantContent::Text(Text {
                    text: text.clone(),
                    additional_params: parameters
                        .clone()
                        .map(rig::completion::message::AdditionalParams::try_from_value)
                        .transpose()
                        .map_err(|_| invalid())?
                        .flatten(),
                }),
                SavedAssistantContent::Call { ordinal, .. } => AssistantContent::ToolCall(
                    restore_call(calls.get(usize::from(*ordinal)).ok_or_else(invalid)?, part)?,
                ),
                SavedAssistantContent::Reasoning { id, content } => {
                    AssistantContent::Reasoning(Reasoning {
                        id: id.clone(),
                        content: content
                            .iter()
                            .map(|part| match part {
                                SavedReasoning::Text { text, signature } => {
                                    ReasoningContent::Text {
                                        text: text.clone(),
                                        signature: signature.clone(),
                                    }
                                }
                                SavedReasoning::Encrypted { data } => {
                                    ReasoningContent::Encrypted(data.clone())
                                }
                                SavedReasoning::Redacted { data } => {
                                    ReasoningContent::Redacted { data: data.clone() }
                                }
                                SavedReasoning::Summary { text } => {
                                    ReasoningContent::Summary(text.clone())
                                }
                            })
                            .collect(),
                    })
                }
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(Message::Assistant {
        id: replay.message_id,
        content,
    })
}

pub(super) fn history(run: &RunCheckpoint) -> AppResult<Vec<Message>> {
    run.messages.iter().map(|message| match message {
        ConversationMessage::User { text } => Ok(Message::user(text)),
        ConversationMessage::Assistant { calls, continuation, .. } => assistant(calls, continuation),
        ConversationMessage::Tool { invocation_id, result, .. } => {
            let turn = run.turns.iter().find(|turn| turn.calls.iter().any(|call| call.invocation_id == *invocation_id)).ok_or_else(invalid)?;
            let ordinal = turn.calls.iter().position(|call| call.invocation_id == *invocation_id).ok_or_else(invalid)?;
            let replay = replay(&turn.continuation)?;
            let part = replay.content.iter().find(|part| matches!(part, SavedAssistantContent::Call { ordinal: index, .. } if usize::from(*index) == ordinal)).ok_or_else(invalid)?;
            let call = restore_call(&turn.calls[ordinal], part)?;
            Ok(Message::User { content: vec![UserContent::tool_result_for(call.id, call.provider, call.function.name,
                vec![ToolResultContent::Text(Text { text: result.to_string(), additional_params: None })])] })
        }
    }).collect()
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
