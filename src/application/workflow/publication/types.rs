use super::registry::{CapabilityProfile, ToolContract};
use crate::domain::workflow::{CompanyId, TypeName};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct AgentKey(String);
impl<'de> Deserialize<'de> for AgentKey {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let value = String::deserialize(decoder)?;
        Self::parse(value).ok_or_else(|| serde::de::Error::custom("invalid agent key"))
    }
}
impl AgentKey {
    /// Stable company-local agent identifier (UUIDs and explicit catalogue aliases).
    pub fn parse(value: impl AsRef<str>) -> Option<Self> {
        let value = value.as_ref();
        (!value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')))
        .then(|| Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ContentHash(pub(super) String);
impl ContentHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Explicit settings only; no open provider configuration map that could capture
/// credentials. Provider connections are runtime resources, not snapshot values.
#[derive(Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    pub provider: TypeName,
    pub model: String,
    pub max_output_tokens: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub company_id: CompanyId,
    pub key: AgentKey,
    pub instructions: String,
    pub model: ModelSettings,
    pub tools: Vec<TypeName>,
    pub skills: Vec<TypeName>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SkillSnapshot {
    pub company_id: CompanyId,
    pub name: TypeName,
    pub instructions: String,
    pub required_tools: Vec<TypeName>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionEffect {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRecovery {
    /// Safe repeat was explicitly approved; never inferred from MCP annotations.
    SafeRepeat,
    ProviderIdempotency,
    Reconcile,
}

/// Supplied by the trusted policy loader, not copied from remote MCP annotations.
#[derive(Clone, Serialize, Deserialize)]
pub struct ApprovedActionPolicy {
    pub capability: TypeName,
    pub policy_revision: u64,
    pub effect: ActionEffect,
    pub recovery: ActionRecovery,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolSnapshot {
    pub company_id: CompanyId,
    pub contract: ToolContract,
    pub policy: ApprovedActionPolicy,
}

/// The complete finite selection universe for this version. Agent/profile names
/// from runtime bindings can only choose captured entries; no live fallback.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct DependencySnapshots {
    pub agents: Vec<AgentSnapshot>,
    pub skills: Vec<SkillSnapshot>,
    pub profiles: Vec<CapabilityProfile>,
    pub tools: Vec<ToolSnapshot>,
}

impl ToolSnapshot {
    pub(super) fn schemas(&self) -> [&Value; 2] {
        [&self.contract.input_schema, &self.contract.output_schema]
    }
}
