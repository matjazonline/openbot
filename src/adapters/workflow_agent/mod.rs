//! Production edges of `agent.run` saved tool calls: every effect enters ActionService
//! through the HTTP, MCP or messaging runtime's direct-tool entry, one call at a time.
use crate::{
    adapters::{
        workflow_http::HttpRuntime, workflow_mcp::McpRuntime, workflow_message::MessageRuntime,
    },
    app_error::{AppError, AppResult, ExecutionFailure},
    application::workflow::{
        actions::{ModelToolCallId, RemoteDispatchObservation},
        agent::{AgentToolDispatch, AgentToolObservation, AgentToolTarget, PreparedTool},
        http::HttpToolCall,
        lease::WorkflowFence,
        mcp::McpToolCall,
        message::{MessageObservation, MessageToolCall},
    },
};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct AgentActionTools {
    http: Arc<HttpRuntime>,
    mcp: Arc<McpRuntime>,
    message: Arc<MessageRuntime>,
}
impl AgentActionTools {
    pub fn new(http: Arc<HttpRuntime>, mcp: Arc<McpRuntime>, message: Arc<MessageRuntime>) -> Self {
        Self { http, mcp, message }
    }
}

#[async_trait]
impl AgentToolDispatch for AgentActionTools {
    async fn dispatch(
        &self,
        fence: WorkflowFence,
        tool: &PreparedTool,
        call: ModelToolCallId,
        arguments: Value,
    ) -> AppResult<AgentToolObservation> {
        // Dropping the handler future is the cancellation path; no detached work.
        let cancel = CancellationToken::new();
        match &tool.target {
            AgentToolTarget::Http { connection } => remote(
                Box::pin(self.http.call_tool(
                    fence,
                    HttpToolCall {
                        connection: *connection,
                        operation: tool.name.clone(),
                        arguments,
                        call,
                    },
                    &cancel,
                ))
                .await?,
            ),
            AgentToolTarget::Mcp { slot } => remote(
                Box::pin(self.mcp.call_tool(
                    fence,
                    McpToolCall {
                        slot: slot.clone(),
                        tool: tool.name.clone(),
                        arguments,
                        call,
                    },
                    &cancel,
                ))
                .await?,
            ),
            AgentToolTarget::Message { .. } => {
                let kind = tool
                    .message_kind()
                    .ok_or_else(|| AppError::BadRequest("Unknown message tool".into()))?;
                message(
                    self.message
                        .call_tool(
                            fence,
                            MessageToolCall {
                                kind,
                                arguments,
                                call,
                            },
                        )
                        .await?,
                )
            }
        }
    }
}

fn remote(observation: RemoteDispatchObservation) -> AppResult<AgentToolObservation> {
    match observation {
        RemoteDispatchObservation::Committed(receipt) => {
            Ok(AgentToolObservation::Completed(receipt.result))
        }
        RemoteDispatchObservation::ApprovalParked(wait) => Ok(AgentToolObservation::Parked(wait)),
        RemoteDispatchObservation::PossibleDispatchExists => Ok(AgentToolObservation::Uncertain),
        RemoteDispatchObservation::ApprovalRequired => Err(approval_required()),
        RemoteDispatchObservation::Interrupted => {
            Err(AppError::Execution(ExecutionFailure::OwnershipLost))
        }
    }
}

fn message(observation: MessageObservation) -> AppResult<AgentToolObservation> {
    match observation {
        MessageObservation::Committed(receipt) => {
            Ok(AgentToolObservation::Completed(receipt.result))
        }
        MessageObservation::ApprovalParked(wait) | MessageObservation::DeliveryParked(wait) => {
            Ok(AgentToolObservation::Parked(wait))
        }
        MessageObservation::Uncertain => Ok(AgentToolObservation::Uncertain),
        MessageObservation::ApprovalRequired => Err(approval_required()),
        MessageObservation::ActionLimit => {
            Err(AppError::BadRequest("Message action limit reached".into()))
        }
        MessageObservation::Pending => {
            Err(AppError::Conflict("Message delivery is pending".into()))
        }
    }
}

/// A protected action without an `agent.run` approval policy fails closed.
fn approval_required() -> AppError {
    AppError::BadRequest("Protected tool requires an agent.run approval policy".into())
}
