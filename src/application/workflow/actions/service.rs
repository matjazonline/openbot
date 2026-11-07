use super::contracts::*;
use super::freeze;
use crate::application::app_error::{AppError, AppResult};
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
    /// Only the terminal owner persists this origin; ordinary action entry points refuse it.
    pub(crate) fn channel_final(request: ActionRequest) -> AppResult<Self> {
        if !matches!(
            request.context.origin,
            crate::application::agent::ToolCallOrigin::ChannelFinal(_)
        ) {
            return Err(AppError::BadRequest(
                "Expected a final response owner".into(),
            ));
        }
        let message = super::super::message::MessageKind::Reply;
        if request.operation_key.as_str() != message.name()
            || request.contract.contract.name.as_str() != message.name()
            || !super::super::message::is_trusted_message_snapshot(&request.contract)?
            || !matches!(&request.target,ActionTarget::Local {resource,resource_kind}
                if *resource==request.scope.company.as_uuid() && resource_kind.as_str()=="messaging")
            || request.context.capability_ceiling.len() != 1
            || request.context.capability_ceiling[0].as_str() != message.name()
        {
            return Err(AppError::BadRequest("Invalid final response action".into()));
        }
        super::super::message::MessageArguments::parse(message, &request.arguments)?;
        freeze::freeze(request)
    }
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
        reject_channel_final(&request)?;
        let frozen = freeze::freeze(request)?;
        self.persistence.prepare(&frozen, None).await
    }
    pub async fn prepare_tool(
        &self,
        request: ActionRequest,
        call: ModelToolCallId,
    ) -> AppResult<ActionIntent> {
        reject_channel_final(&request)?;
        let frozen = freeze::freeze(request)?;
        self.persistence.prepare(&frozen, Some(&call)).await
    }
}

fn reject_channel_final(request: &ActionRequest) -> AppResult<()> {
    if matches!(
        request.context.origin,
        crate::application::agent::ToolCallOrigin::ChannelFinal(_)
    ) {
        return Err(AppError::BadRequest(
            "Final responses require their terminal owner".into(),
        ));
    }
    Ok(())
}
