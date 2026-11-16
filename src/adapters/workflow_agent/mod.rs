//! Production edges of `agent.run` saved tool calls: every effect enters ActionService
//! through the HTTP, MCP, messaging or provisioning runtime's direct-tool entry, one call at a
//! time. A workflow
//! tool starts its pinned child through the workflow tool runtime instead (07-D2 §D2.3a).
//! Knowledge search and the memory tools commit their observation and classification locally,
//! without actions.
use crate::{
    adapters::{
        workflow_escalation::EscalationRuntime, workflow_http::HttpRuntime,
        workflow_mcp::McpRuntime, workflow_message::MessageRuntime,
        workflow_provisioning::ProvisioningRuntime,
    },
    app_error::{AppError, AppResult, ExecutionFailure},
    application::workflow::{
        actions::{ModelToolCallId, RemoteDispatchObservation},
        agent::{AgentToolDispatch, AgentToolObservation, AgentToolTarget, PreparedTool},
        escalation::tools::EscalationToolCall,
        http::HttpToolCall,
        knowledge::{KnowledgeToolCall, KnowledgeToolDispatch},
        lease::WorkflowFence,
        mcp::{McpToolCall, McpToolTarget},
        memory_tools::{MemoryToolCall, MemoryToolDispatch},
        message::{MessageObservation, MessageToolCall},
        provisioning::ProvisioningToolCall,
        tools::{TOOL_LIMIT, WorkflowToolCall, WorkflowToolDispatch, tool_error},
    },
    domain::workflow::ToolCallId,
};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct AgentActionTools {
    http: Arc<HttpRuntime>,
    mcp: Arc<McpRuntime>,
    message: Arc<MessageRuntime>,
    /// Every production composition includes it (notice WF-C001 r2); without it a frozen
    /// workflow tool fails closed.
    workflow: Option<Arc<dyn WorkflowToolDispatch>>,
    /// The built-in provisioning tools (p08 D2.2a); without them a frozen provisioning tool
    /// fails closed.
    provisioning: Option<Arc<ProvisioningRuntime>>,
    escalation: Option<Arc<EscalationRuntime>>,
    knowledge: Option<Arc<dyn KnowledgeToolDispatch>>,
    /// The memory tools (phase 05 E5), composed only while the memory service is configured;
    /// without them a published memory tool fails closed.
    memory: Option<Arc<dyn MemoryToolDispatch>>,
}
impl AgentActionTools {
    pub fn new(http: Arc<HttpRuntime>, mcp: Arc<McpRuntime>, message: Arc<MessageRuntime>) -> Self {
        Self {
            http,
            mcp,
            message,
            workflow: None,
            provisioning: None,
            escalation: None,
            knowledge: None,
            memory: None,
        }
    }
    pub fn with_workflow_tools(mut self, workflow: Arc<dyn WorkflowToolDispatch>) -> Self {
        self.workflow = Some(workflow);
        self
    }
    pub fn with_provisioning(mut self, provisioning: Arc<ProvisioningRuntime>) -> Self {
        self.provisioning = Some(provisioning);
        self
    }
    pub fn with_escalations(mut self, runtime: Arc<EscalationRuntime>) -> Self {
        self.escalation = Some(runtime);
        self
    }
    fn escalation_runtime(&self) -> AppResult<&EscalationRuntime> {
        self.escalation.as_deref().ok_or_else(|| {
            AppError::ServiceUnavailable("Escalation tools are unavailable to this worker".into())
        })
    }
    pub fn with_knowledge(mut self, knowledge: Arc<dyn KnowledgeToolDispatch>) -> Self {
        self.knowledge = Some(knowledge);
        self
    }
    pub fn with_memory(mut self, memory: Arc<dyn MemoryToolDispatch>) -> Self {
        self.set_memory(memory);
        self
    }
    pub fn set_memory(&mut self, memory: Arc<dyn MemoryToolDispatch>) {
        self.memory = Some(memory);
    }
}

// Box each owned call payload so adding a tool does not grow the async dispatch frame.
enum ToolInvocation {
    Http(Box<HttpToolCall>),
    Mcp(Box<McpToolCall>),
    Workflow(Box<WorkflowToolCall>),
    Provisioning(Box<ProvisioningToolCall>),
    Escalation(Box<EscalationToolCall>),
    Message(Box<MessageToolCall>),
    Knowledge(Box<KnowledgeToolCall>),
    Memory(Box<MemoryToolCall>),
}
fn invocation(
    tool: &PreparedTool,
    call: ModelToolCallId,
    arguments: Value,
) -> AppResult<ToolInvocation> {
    Ok(match &tool.target {
        AgentToolTarget::Http { connection } => ToolInvocation::Http(Box::new(HttpToolCall {
            origin: crate::application::agent::ToolCallOrigin::Agent(tool.origin),
            connection: *connection,
            operation: tool.name.clone(),
            arguments,
            call,
        })),
        AgentToolTarget::Mcp { connection } => ToolInvocation::Mcp(Box::new(McpToolCall {
            target: McpToolTarget::Agent {
                connection: *connection,
                origin: tool.origin,
            },
            tool: tool.name.clone(),
            arguments,
            call,
        })),
        AgentToolTarget::Workflow { pin } => ToolInvocation::Workflow(Box::new(WorkflowToolCall {
            origin: crate::application::agent::ToolCallOrigin::Agent(tool.origin),
            name: tool.name.clone(),
            pin: pin.clone(),
            call: ToolCallId::new(
                uuid::Uuid::parse_str(call.as_str())
                    .map_err(|_| AppError::BadRequest("Invalid tool call".into()))?,
            ),
            arguments,
        })),
        AgentToolTarget::Provisioning { .. } => {
            ToolInvocation::Provisioning(Box::new(ProvisioningToolCall {
                origin: crate::application::agent::ToolCallOrigin::Agent(tool.origin),
                kind: tool
                    .provisioning_kind()
                    .ok_or_else(|| AppError::BadRequest("Unknown provisioning tool".into()))?,
                arguments,
                call,
            }))
        }
        AgentToolTarget::Escalation { tool: kind } => {
            ToolInvocation::Escalation(Box::new(EscalationToolCall {
                origin: crate::application::agent::ToolCallOrigin::Agent(tool.origin),
                kind: *kind,
                arguments,
                call,
            }))
        }
        AgentToolTarget::Message { .. } => ToolInvocation::Message(Box::new(MessageToolCall {
            origin: crate::application::agent::ToolCallOrigin::Agent(tool.origin),
            kind: tool
                .message_kind()
                .ok_or_else(|| AppError::BadRequest("Unknown message tool".into()))?,
            arguments,
            call,
        })),
        AgentToolTarget::Knowledge => ToolInvocation::Knowledge(Box::new(KnowledgeToolCall {
            origin: crate::application::agent::ToolCallOrigin::Agent(tool.origin),
            call,
            arguments,
        })),
        AgentToolTarget::Memory { tool: memory } => {
            ToolInvocation::Memory(Box::new(MemoryToolCall {
                origin: crate::application::agent::ToolCallOrigin::Agent(tool.origin),
                tool: *memory,
                call,
                arguments,
            }))
        }
    })
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
        // Dropping the handler future cancels the real work; external seams stay boxed.
        let cancel = CancellationToken::new();
        let outcome = async {
            match invocation(tool, call, arguments)? {
                ToolInvocation::Http(call) => {
                    remote(Box::pin(self.http.call_tool(fence, *call, &cancel)).await?)
                }
                ToolInvocation::Mcp(call) => {
                    remote(Box::pin(self.mcp.call_tool(fence, *call, &cancel)).await?)
                }
                ToolInvocation::Workflow(call) => {
                    let workflow = self.workflow.as_ref().ok_or_else(|| {
                        AppError::BadRequest(
                            "Workflow tools are not available to this worker".into(),
                        )
                    })?;
                    Box::pin(workflow.call_workflow_tool(fence, *call)).await
                }
                ToolInvocation::Provisioning(call) => {
                    let provisioning = self.provisioning.as_ref().ok_or_else(|| {
                        AppError::BadRequest(
                            "Provisioning tools are not available to this worker".into(),
                        )
                    })?;
                    Box::pin(provisioning.call_tool(fence, *call)).await
                }
                ToolInvocation::Escalation(call) => {
                    Box::pin(self.escalation_runtime()?.call_tool(fence, *call)).await
                }
                ToolInvocation::Message(call) => {
                    message(Box::pin(self.message.call_tool(fence, *call)).await?)
                }
                ToolInvocation::Knowledge(call) => {
                    let knowledge = self.knowledge.as_ref().ok_or_else(|| {
                        AppError::ServiceUnavailable(
                            "Knowledge search is unavailable to this worker".into(),
                        )
                    })?;
                    Box::pin(knowledge.call_knowledge_tool(fence, *call)).await
                }
                ToolInvocation::Memory(call) => {
                    let memory = self.memory.as_ref().ok_or_else(|| {
                        AppError::ServiceUnavailable(
                            "Memory tools are unavailable to this worker".into(),
                        )
                    })?;
                    Box::pin(memory.call_memory_tool(fence, *call)).await
                }
            }
        }
        .await;
        match outcome {
            Err(AppError::KnowledgeRelease { .. }) => {
                Ok(AgentToolObservation::Completed(tool_error(
                    crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT,
                    "The destination cannot receive this run's content",
                )))
            }
            other => other,
        }
    }
}

fn remote(observation: RemoteDispatchObservation) -> AppResult<AgentToolObservation> {
    match observation {
        RemoteDispatchObservation::PreparationFailed(_) => Err(AppError::ServiceUnavailable(
            "Integration preparation failed".into(),
        )),
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
        // The same refusal a workflow tool call gets at the limit: the model is told, and the
        // step goes on without the email.
        MessageObservation::ContinuationLimit => Ok(AgentToolObservation::Completed(tool_error(
            TOOL_LIMIT,
            "This agent step cannot send another email",
        ))),
        MessageObservation::Pending => {
            Err(AppError::Conflict("Message delivery is pending".into()))
        }
    }
}

/// A protected action without an `agent.run` approval policy fails closed.
fn approval_required() -> AppError {
    AppError::BadRequest("Protected tool requires an agent.run approval policy".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model-tool email the job has no continuation left for reaches the model as the same
    /// tool-limit result a workflow tool call gets at the limit, not as an error that fails the
    /// step (fix-app 02 §1).
    #[test]
    fn message_park_continuation_limit_is_the_tool_limit_observation() {
        let Ok(AgentToolObservation::Completed(result)) =
            message(MessageObservation::ContinuationLimit)
        else {
            panic!("the continuation limit is a completed tool result");
        };
        assert_eq!(result["error"]["code"], TOOL_LIMIT);
    }
}
