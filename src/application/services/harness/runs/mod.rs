//! Durable, runtime-neutral continuation protocol. Task leases authorize worker writes;
//! approval and ownership transactions must use their own authority after parking.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::AgentExecutionOutput;
use crate::{
    app_error::{AppError, AppResult},
    entities::{
        harness::HarnessKind,
        task::TaskLeaseRef,
        value_objects::{ModelName, ModelProvider, ToolId},
    },
};

mod diagnostics;
mod usage;
pub use usage::RunUsage;
mod state;
pub use diagnostics::RunDiagnostics;
pub use state::*;

pub use crate::entities::harness_run::{
    CheckpointRevision, InvocationId, InvocationRef, ModelRequestId, RunId,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunIdentity {
    pub company_id: Uuid,
    pub task_id: Uuid,
    pub agent_id: Uuid,
    pub harness: HarnessKind,
    pub provider: ModelProvider,
    pub model: ModelName,
    /// Hash of execution-affecting configuration, selected schemas, and ordered skills.
    #[serde(default)]
    pub response_contract: Option<crate::entities::response_contract::ResponseContract>,
    pub capability_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConversationMessage {
    User {
        text: String,
    },
    Assistant {
        text: String,
        calls: Vec<SavedToolCall>,
        continuation: Vec<ContinuationBlock>,
    },
    Tool {
        invocation_id: InvocationId,
        call_id: String,
        item_id: Option<String>,
        result: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairReservation {
    pub candidate: ModelRequestId,
    pub reason: crate::services::response_contract::InvalidResponse,
}

/// Explicit version and provider discriminator; never a serialized runtime struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuationBlock {
    pub schema_version: u16,
    pub provider: ModelProvider,
    pub kind: String,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedToolCall {
    pub invocation_id: InvocationId,
    pub call_id: String,
    pub item_id: Option<String>,
    pub tool_id: ToolId,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelReservation {
    #[serde(default)]
    pub repair: Option<RepairReservation>,
    pub request_id: ModelRequestId,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedModelTurn {
    pub token_usage_source: TokenUsageSource,
    #[serde(default)]
    pub invalid_response: Option<crate::services::response_contract::InvalidResponse>,
    pub request_id: ModelRequestId,
    pub text: String,
    pub calls: Vec<SavedToolCall>,
    pub continuation: Vec<ContinuationBlock>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Provenance survives checkpoint replay; a provider's absent counters are never free usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenUsageSource {
    Reported,
    Estimated,
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationState {
    Prepared,
    Ready,
    Waiting,
    Completed,
    Failed,
    Indeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedInvocation {
    pub mcp: Option<SavedMcpInvocation>,
    pub call: SavedToolCall,
    pub turn: u16,
    pub ordinal: u16,
    pub state: InvocationState,
    pub result: Option<Value>,
}

pub struct OpenRun<'a> {
    pub lease: TaskLeaseRef,
    pub identity: &'a RunIdentity,
    pub initial_prompt: &'a str,
    pub policy: RigExecutionPolicy,
}

#[derive(Debug)]
pub enum WriteOutcome<T> {
    Applied(T),
    AlreadyApplied(T),
    OwnershipLost,
    RevisionConflict,
}

pub struct RunWrite {
    pub company_id: Uuid,
    pub run_id: RunId,
    pub lease: TaskLeaseRef,
    pub expected_revision: CheckpointRevision,
}

/// All operations are mandatory; no success-shaped fallback can drop a checkpoint.
#[async_trait]
pub trait HarnessRunStore: Send + Sync {
    fn mcp_journal(
        &self,
        lease: TaskLeaseRef,
    ) -> std::sync::Arc<dyn crate::services::mcp_runtime::McpInvocationJournal>;
    async fn reserve_execution(
        &self,
        write: &RunWrite,
        allowance_ms: u64,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;
    async fn bind_tool_catalogue(
        &self,
        write: &RunWrite,
        fingerprint: String,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;
    async fn open_run(&self, request: OpenRun<'_>) -> AppResult<WriteOutcome<RunCheckpoint>>;
    async fn diagnostics(
        &self,
        company_id: Uuid,
        task_id: Uuid,
    ) -> AppResult<Option<RunDiagnostics>>;
    async fn load_run(
        &self,
        company_id: Uuid,
        task_id: Uuid,
        run_id: RunId,
    ) -> AppResult<Option<RunCheckpoint>>;
    async fn reserve_model(
        &self,
        write: &RunWrite,
        reservation: ModelReservation,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;
    async fn commit_model_turn(
        &self,
        write: &RunWrite,
        turn: SavedModelTurn,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;
    async fn prepare_invocation(
        &self,
        write: &RunWrite,
        invocation_id: InvocationId,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;
    async fn record_result(
        &self,
        write: &RunWrite,
        invocation_id: InvocationId,
        result: Value,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;
    async fn fail_invalid_output(&self, write: &RunWrite)
    -> AppResult<WriteOutcome<RunCheckpoint>>;
    async fn save_final_output(
        &self,
        write: &RunWrite,
        output: AgentExecutionOutput,
    ) -> AppResult<WriteOutcome<RunCheckpoint>>;
}

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct RunExecution {
    pub company_id: Uuid,
    pub lease: TaskLeaseRef,
    pub store: std::sync::Arc<dyn HarnessRunStore>,
}

/// Provider replay data uses our own versioned vocabulary, preserving the original item order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantReplay {
    pub message_id: Option<String>,
    pub response_id: Option<String>,
    pub content: Vec<SavedAssistantContent>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SavedAssistantContent {
    Text {
        text: String,
        parameters: Option<Value>,
    },
    Call {
        ordinal: u16,
        wire_id: Option<String>,
        signature: Option<String>,
        parameters: Option<Value>,
    },
    Reasoning {
        id: Option<String>,
        content: Vec<SavedReasoning>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SavedReasoning {
    Text {
        text: String,
        signature: Option<String>,
    },
    Encrypted {
        data: String,
    },
    Redacted {
        data: String,
    },
    Summary {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedMcpInvocation {
    pub connection_id: Uuid,
    pub tool_name: crate::entities::mcp::McpToolName,
    pub definition_revision: i64,
    pub credential_revision: i64,
    pub selection_revision: i64,
    pub schema_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReservation {
    pub generation: Uuid,
    pub allowance_ms: u64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub replayed_invocations: u16,
}
