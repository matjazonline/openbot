//! Provider promises come from trusted registration, never transport metadata.
use super::*;
use crate::app_error::AppResult;
use crate::application::workflow::publication::{ActionRecovery, ToolSnapshot};
use crate::domain::workflow::TypeName;
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Copy, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ProviderReplayMode {
    /// Repetition, including concurrent delivery, cannot add a logical effect.
    SafeRepeat,
    /// Provider deduplicates the supplied key for this duration from first acceptance,
    /// including concurrent requests and lost responses. The key namespace includes
    /// the approved company/target/operation; transport must actually transmit it.
    ProviderIdempotency { retention: Duration },
}

/// Created by trusted adapter registration after checking the provider's actual contract.
/// No Deserialize: HTTP/MCP metadata or stored JSON cannot manufacture this approval.
#[derive(Clone, Serialize)]
pub struct ProviderReplayContract {
    version: u8,
    registration: TypeName,
    operation: ArgumentDigest,
    guarantee: ProviderReplayMode,
}
impl ProviderReplayContract {
    pub fn approve(
        registration: TypeName,
        tool: &ToolSnapshot,
        target: &ActionTarget,
        guarantee: ProviderReplayMode,
    ) -> AppResult<Self> {
        if tool.policy.policy_revision == 0 {
            return Err(super::contracts::invalid());
        }
        match (tool.policy.recovery, guarantee) {
            (ActionRecovery::SafeRepeat, ProviderReplayMode::SafeRepeat) => {}
            (
                ActionRecovery::ProviderIdempotency,
                ProviderReplayMode::ProviderIdempotency { retention },
            ) if !retention.is_zero() && retention <= Duration::from_secs(365 * 24 * 60 * 60) => {}
            _ => return Err(super::contracts::invalid()),
        }
        Ok(Self {
            version: 1,
            registration,
            operation: signature(tool, target)?,
            guarantee,
        })
    }
}

/// Service-owned transport arguments. They cannot be constructed from a caller's key.
pub struct ProviderInvocation {
    contract: Option<ProviderReplayContract>,
    key: Option<ActionIdempotencyKey>,
}
impl ProviderInvocation {
    pub fn idempotency_key(&self) -> Option<&ActionIdempotencyKey> {
        self.key.as_ref()
    }
    pub(crate) fn proof(&self) -> AppResult<Value> {
        serde_json::to_value(&self.contract).map_err(|_| super::contracts::invalid())
    }
}

pub(crate) fn prepare_provider(
    action: &FrozenAction,
    registered: Option<&ProviderReplayContract>,
    required_retention: Duration,
) -> AppResult<ProviderInvocation> {
    let recovery = action.request().contract.policy.recovery;
    if recovery == ActionRecovery::Reconcile && registered.is_none() {
        return Ok(ProviderInvocation {
            contract: None,
            key: None,
        });
    }
    let contract = registered.ok_or_else(super::contracts::invalid)?;
    if contract.operation != signature(&action.request().contract, &action.request().target)? {
        return Err(super::contracts::invalid());
    }
    let key = match (recovery, contract.guarantee) {
        (ActionRecovery::SafeRepeat, ProviderReplayMode::SafeRepeat) => None,
        (
            ActionRecovery::ProviderIdempotency,
            ProviderReplayMode::ProviderIdempotency { retention },
        ) if required_retention <= retention => Some(action.idempotency_key()),
        _ => return Err(super::contracts::invalid()),
    };
    Ok(ProviderInvocation {
        contract: Some(contract.clone()),
        key,
    })
}

fn signature(tool: &ToolSnapshot, target: &ActionTarget) -> AppResult<ArgumentDigest> {
    use sha2::{Digest, Sha256};
    Ok(ArgumentDigest(format!(
        "{:x}",
        Sha256::digest(replay_subject(tool, target)?)
    )))
}

/// Preserve the exact canonical bytes used by the approved descriptor. JSONB's
/// serializer is intentionally not part of this hash contract.
pub(crate) fn replay_subject(tool: &ToolSnapshot, target: &ActionTarget) -> AppResult<Vec<u8>> {
    let value = super::freeze::canonical(
        &json!({"tool":tool,"target":target}),
        super::freeze::MAX_OPERATION_BYTES,
    )?;
    serde_json::to_vec(&value).map_err(|_| super::contracts::invalid())
}
