//! Workflow tools (07-D2): a company registration names a published workflow version that agents
//! granted that name can call as a tool. Registering is an admin act; publication freezes the
//! pin, and revoking the name stops new runs and new calls (P07-Q2). There is deliberately no
//! approval flag: the pinned workflow's own steps govern what it does.
use super::{
    agent::AgentToolObservation, authorization::WorkflowActor, lease::WorkflowFence,
    publication::WorkflowToolPin,
};
use crate::{
    app_error::AppResult,
    domain::workflow::{CompanyId, ToolCallId, TypeName, VersionId, WorkflowId},
};
use async_trait::async_trait;
use serde_json::{Value, json};
use uuid::Uuid;

/// The most workflow tools one company registers (bounded like HTTP operations).
pub const MAX_WORKFLOW_TOOLS: usize = 256;
/// A tool result the agent receives is at most what an action receipt may hold.
pub const MAX_TOOL_RESULT_BYTES: usize = 131_072;
/// Static child parameters stored with a registration.
pub const MAX_TOOL_PARAMS_BYTES: usize = 65_536;
/// The most tool names one agent grants (`valid_workflow_type_names`).
pub const MAX_AGENT_TOOL_GRANTS: usize = 32;

pub const TOOL_REVOKED: &str = "workflow.tool_revoked";
pub const TOOL_LIMIT: &str = "workflow.tool_limit";
pub const TOOL_RESULT_TOO_LARGE: &str = "workflow.tool_result_too_large";
/// A child's outcome, the same code for a step child's unrouted failure and for a tool call's
/// typed error.
pub const CHILD_FAILED: &str = "workflow.child_failed";
pub const CHILD_CANCELLED: &str = "workflow.child_cancelled";
/// Input a child refuses: a tool call's arguments or a `flow.repeat` round's next input.
pub const CHILD_INVALID_INPUT: &str = "workflow.invalid_input";

/// Create (`expected: None`) or replace (`expected: Some(revision)`) the registration of `name`.
#[derive(Debug, Clone)]
pub struct WorkflowToolWrite {
    pub name: TypeName,
    pub workflow: WorkflowId,
    pub version: VersionId,
    pub params: Value,
    pub expected: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowToolRegistration {
    pub name: TypeName,
    pub workflow: WorkflowId,
    pub version: VersionId,
    pub params: Value,
    pub revision: i64,
}

/// Company-admin commands over workflow tool registrations. HTTP and UI entry points belong to
/// phases 08 and 09 (notice WF-C003).
#[async_trait]
pub trait WorkflowTools: Send + Sync {
    async fn register_workflow_tool(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        write: WorkflowToolWrite,
    ) -> AppResult<WorkflowToolRegistration>;
    async fn revoke_workflow_tool(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        name: TypeName,
        revision: i64,
    ) -> AppResult<()>;
    async fn workflow_tools(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
    ) -> AppResult<Vec<WorkflowToolRegistration>>;
    /// Replace the company agent's ordered tool grants, only while they are still `expected`
    /// (the list the caller saw). Grants are names; publication resolves them (07-D2 §D2.2).
    async fn grant_agent_tools(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        agent: Uuid,
        expected: &[TypeName],
        tools: &[TypeName],
    ) -> AppResult<Vec<TypeName>>;
}

/// One saved model call of a workflow tool: the frozen pin and the call's identity.
#[derive(Debug, Clone)]
pub struct WorkflowToolCall {
    pub name: TypeName,
    pub pin: WorkflowToolPin,
    pub call: ToolCallId,
    pub arguments: Value,
}

/// Starts the call's child and parks the agent job, or answers a call whose child already
/// finished (07-D2 §D2.3). Never `Uncertain`: the child, its wait and the park commit together.
#[async_trait]
pub trait WorkflowToolDispatch: Send + Sync {
    async fn call_workflow_tool(
        &self,
        fence: WorkflowFence,
        call: WorkflowToolCall,
    ) -> AppResult<AgentToolObservation>;
}

/// The typed error the model receives instead of a result, in the same shape as an unknown
/// tool's refusal.
pub fn tool_error(code: &str, message: &str) -> Value {
    json!({"error": {"code": code, "message": message}})
}
