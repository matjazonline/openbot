use super::{SEARCH_KNOWLEDGE, is_trusted_knowledge_snapshot, knowledge_tool_schemas};
use crate::{
    app_error::{AppError, AppResult},
    application::{
        knowledge::search::KnowledgeObservation,
        workflow::{
            CompanyId,
            actions::ModelToolCallId,
            agent::{AgentToolObservation, AgentToolTarget, PreparedTool},
            lease::WorkflowFence,
            publication::ResolvedAgent,
        },
    },
    entities::access_level::AccessLevel,
    services::harness::runs::{InvocationId, InvocationState, RunCheckpoint, SavedInvocation},
};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

pub fn validate_knowledge_observation(
    observation: &KnowledgeObservation,
    ceiling: AccessLevel,
) -> AppResult<()> {
    use crate::application::workflow::compiler::{Schema, SourceSpan};
    let max_bytes = crate::application::workflow::tools::MAX_TOOL_RESULT_BYTES
        .min(crate::services::harness::runs::MAX_RESULT_BYTES);
    if observation.highest().is_some_and(|level| level > ceiling)
        || observation.result().to_string().len() > max_bytes
    {
        return Err(missing());
    }
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    Schema::compile(knowledge_tool_schemas().output, "/output", span)
        .and_then(|schema| schema.validate(observation.result(), "/output", span))
        .map_err(|_| AppError::Internal("Invalid knowledge observation".into()))
}

pub struct KnowledgeToolCall {
    pub call: ModelToolCallId,
    pub arguments: Value,
}

impl KnowledgeToolCall {
    pub fn invocation(&self) -> AppResult<InvocationId> {
        Uuid::parse_str(self.call.as_str())
            .map(InvocationId)
            .map_err(|_| missing())
    }
}

#[derive(Clone, Copy)]
pub struct KnowledgeSearchAccess {
    pub company: CompanyId,
    pub ceiling: AccessLevel,
}

pub enum KnowledgeSearchContext {
    Pending(KnowledgeSearchAccess),
    Recorded(Value),
}

#[async_trait]
pub trait KnowledgeSearchPersistence: Send + Sync {
    async fn knowledge_context(
        &self,
        fence: WorkflowFence,
        call: &KnowledgeToolCall,
    ) -> AppResult<KnowledgeSearchContext>;

    /// Result, monotone classification rise and live consumption proof commit together.
    async fn observe_knowledge(
        &self,
        fence: WorkflowFence,
        call: &KnowledgeToolCall,
        observation: KnowledgeObservation,
    ) -> AppResult<Value>;
}

#[async_trait]
pub trait KnowledgeToolDispatch: Send + Sync {
    async fn call_knowledge_tool(
        &self,
        fence: WorkflowFence,
        call: KnowledgeToolCall,
    ) -> AppResult<AgentToolObservation>;
}

/// `selection` must be resolved from the actual immutable execution inputs and run bundle.
/// The saved preparation is trace data and cannot establish the grant by itself.
pub fn require_knowledge_invocation<'a>(
    fence: WorkflowFence,
    selection: &ResolvedAgent,
    tools: &[PreparedTool],
    checkpoint: &'a RunCheckpoint,
    call: &KnowledgeToolCall,
) -> AppResult<&'a SavedInvocation> {
    if selection.company_id() != fence.scope.company
        || checkpoint.identity.company_id != fence.scope.company.as_uuid()
        || checkpoint.identity.task_id != fence.scope.job.0
        || checkpoint.identity.agent_id != selection.agent().agent_id
    {
        return Err(missing());
    }
    let id = call.invocation()?;
    let saved = checkpoint
        .invocations
        .iter()
        .find(|item| item.call.invocation_id == id && item.call.arguments == call.arguments)
        .ok_or_else(missing)?;
    let matching: Vec<_> = tools
        .iter()
        .filter(|tool| tool.declared == saved.call.tool_id.as_str())
        .collect();
    let [tool] = matching.as_slice() else {
        return Err(missing());
    };
    if tool.target != AgentToolTarget::Knowledge
        || tool.name.as_str() != SEARCH_KNOWLEDGE
        || tool.input_schema != knowledge_tool_schemas().input
    {
        return Err(missing());
    }
    let frozen: Vec<_> = selection
        .tools()
        .iter()
        .filter(|tool| tool.contract.name.as_str() == SEARCH_KNOWLEDGE)
        .collect();
    let [snapshot] = frozen.as_slice() else {
        return Err(missing());
    };
    if snapshot.company_id != fence.scope.company || !is_trusted_knowledge_snapshot(snapshot)? {
        return Err(missing());
    }
    match saved.state {
        InvocationState::Completed if saved.result.is_some() => Ok(saved),
        InvocationState::Prepared | InvocationState::Ready | InvocationState::Indeterminate
            if checkpoint
                .pending_call()
                .is_some_and(|pending| pending.invocation_id == id) =>
        {
            Ok(saved)
        }
        _ => Err(missing()),
    }
}

fn missing() -> AppError {
    AppError::NotFound("Knowledge tool invocation".into())
}
