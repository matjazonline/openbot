use super::contracts::*;
use super::freeze;
use crate::application::app_error::AppResult;
use crate::application::workflow::publication::ToolSnapshot;
use crate::domain::workflow::TypeName;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    pub scope: ActionScope,
    pub operation_key: TypeName,
    pub target: ActionTarget,
    pub contract: ToolSnapshot,
    pub arguments: Value,
    pub context: ActionPolicyContext,
}

/// Constructed only after bounded canonicalization and schema validation.
pub struct FrozenAction {
    pub(super) request: ActionRequest,
    pub(super) saved: Value,
    pub(super) argument_digest: ArgumentDigest,
    pub(super) operation_digest: ArgumentDigest,
    pub(super) decision: ActionPolicyDecision,
}
impl FrozenAction {
    pub fn request(&self) -> &ActionRequest {
        &self.request
    }
    pub fn scope(&self) -> ActionScope {
        self.request.scope
    }
    pub fn operation_key(&self) -> &TypeName {
        &self.request.operation_key
    }
    pub fn saved_operation(&self) -> &Value {
        &self.saved
    }
    pub fn argument_digest(&self) -> &ArgumentDigest {
        &self.argument_digest
    }
    pub fn operation_digest(&self) -> &ArgumentDigest {
        &self.operation_digest
    }
    pub fn decision(&self) -> ActionPolicyDecision {
        self.decision
    }
    pub fn idempotency_key(&self) -> ActionIdempotencyKey {
        ActionIdempotencyKey(format!(
            "workflow-action:v1:{}",
            self.operation_digest.as_str()
        ))
    }
    /// Stored content is untrusted; restoring revalidates its schema and bounds.
    pub fn restore(saved: Value) -> AppResult<Self> {
        freeze::restore(saved)
    }
}

#[async_trait]
pub trait ActionIntents: Send + Sync {
    /// Atomically persist/replay immutable intent and optional model-call mapping.
    /// A reused model call with different content must fail without writing a new invocation.
    async fn prepare(
        &self,
        action: &FrozenAction,
        model_call: Option<&ModelToolCallId>,
    ) -> AppResult<ActionIntent>;
}

pub struct ActionService<P> {
    pub(super) persistence: P,
}
impl<P> ActionService<P> {
    pub fn new(persistence: P) -> Self {
        Self { persistence }
    }
}
impl<P: ActionIntents> ActionService<P> {
    pub async fn prepare_step(&self, request: ActionRequest) -> AppResult<ActionIntent> {
        let frozen = freeze::freeze(request)?;
        self.persistence.prepare(&frozen, None).await
    }
    pub async fn prepare_tool(
        &self,
        request: ActionRequest,
        call: ModelToolCallId,
    ) -> AppResult<ActionIntent> {
        let frozen = freeze::freeze(request)?;
        self.persistence.prepare(&frozen, Some(&call)).await
    }
}
