//! Authenticated native MCP ingress. Model arguments never select an effect owner.
use super::{CredentialVerifier, protocol::ConnectionIdentity};
use crate::app_error::AppResult;
use crate::domain::local_agents::{
    CloudroomClientRequestId, CloudroomModelCallId, CloudroomThreadId, CloudroomTurnId,
    NativeMcpCallId, OperationId,
};
use crate::domain::workflow::RunId;
use crate::entities::agent_capability::AgentToolAlias;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_ARGUMENT_BYTES: usize = 65_536;
pub const MAX_REQUEST_BYTES: usize = 131_072;
pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
// Reserve room for the bounded immutable origin, receipt IDs and outcome envelope.
pub const MAX_RESULT_BYTES: usize = MAX_RESPONSE_BYTES - 4096;
pub const MAX_OPERATION_CALLS: usize = 64;
pub const MAX_MACHINE_UNFINISHED: usize = 8;
pub const MAX_CALL_SECONDS: i64 = 900;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeMcpInvocation {
    pub thread_id: CloudroomThreadId,
    pub turn_id: CloudroomTurnId,
    pub client_request_id: CloudroomClientRequestId,
    pub model_call_id: CloudroomModelCallId,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeMcpAction {
    Call {
        alias: AgentToolAlias,
        arguments: Value,
    },
    Status {
        call_id: NativeMcpCallId,
    },
    Cancel {
        call_id: NativeMcpCallId,
    },
    CancelInvocation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeMcpBody {
    pub invocation: NativeMcpInvocation,
    pub action: NativeMcpAction,
}

pub struct NativeMcpRequest {
    pub verifier: CredentialVerifier,
    pub identity: ConnectionIdentity,
    pub operation: OperationId,
    pub body: NativeMcpBody,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativeMcpState {
    Pending,
    Completed { result: Value },
    ToolError { result: Value },
    Failed { code: &'static str },
    Cancelled,
    ReconciliationRequired,
    Collected,
}

#[derive(Serialize)]
pub struct NativeMcpOrigin {
    pub operation_id: OperationId,
    pub invocation: NativeMcpInvocation,
}

#[derive(Serialize)]
pub struct NativeMcpResponse {
    pub call_id: NativeMcpCallId,
    pub run_id: RunId,
    pub deadline: DateTime<Utc>,
    pub origin: NativeMcpOrigin,
    #[serde(flatten)]
    pub state: NativeMcpState,
}

#[async_trait]
pub trait NativeMcpBroker: Send + Sync {
    /// Authenticate current native source authority before decoding model-controlled input.
    async fn authorize_native_mcp(
        &self,
        request: &super::harness::NativeBundleRequest,
    ) -> AppResult<()>;
    async fn native_mcp(&self, request: &NativeMcpRequest) -> AppResult<NativeMcpResponse>;
    async fn sweep_native_mcp(&self, limit: u16) -> AppResult<u16>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_mcp_maximum_result_fits_the_complete_response_with_maximum_origin_ids() {
        let mut result = serde_json::json!({"content":[{"type":"text","text":""}]});
        let overhead = serde_json::to_vec(&result).unwrap().len();
        result["content"][0]["text"] = Value::String("x".repeat(MAX_RESULT_BYTES - overhead));
        assert_eq!(serde_json::to_vec(&result).unwrap().len(), MAX_RESULT_BYTES);
        let response = NativeMcpResponse {
            call_id: NativeMcpCallId::new(uuid::Uuid::new_v4()).unwrap(),
            run_id: RunId::new(uuid::Uuid::new_v4()),
            deadline: Utc::now(),
            origin: NativeMcpOrigin {
                operation_id: OperationId::new(uuid::Uuid::new_v4()).unwrap(),
                invocation: NativeMcpInvocation {
                    thread_id: "t".repeat(128).try_into().unwrap(),
                    turn_id: "u".repeat(128).try_into().unwrap(),
                    client_request_id: "r".repeat(128).try_into().unwrap(),
                    model_call_id: "c".repeat(128).try_into().unwrap(),
                },
            },
            state: NativeMcpState::Completed { result },
        };
        assert!(serde_json::to_vec(&response).unwrap().len() <= MAX_RESPONSE_BYTES);
    }
}
