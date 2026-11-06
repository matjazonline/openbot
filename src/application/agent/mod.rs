//! Agent-owned immutable capability definitions and publication contracts.
use crate::domain::workflow::{CompanyId, TypeName};
use serde::{Deserialize, Serialize};

mod authoring;
mod authoring_reads;
mod definition;
pub(crate) mod dependency;
mod manifest;
mod publication;
mod resolution;
mod tool_name;
mod tool_origin;
pub use authoring::*;
pub use authoring_reads::*;
pub use definition::*;
pub use manifest::*;
pub use publication::*;
pub use resolution::*;
pub use tool_name::*;
pub use tool_origin::*;

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

/// Explicit settings only; no open provider configuration map that could capture
/// credentials. `(ResolvedAgentDefinition.company_id, provider)` is the existing company model
/// connection key. Resolve its current secret at runtime without following agent defaults.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSettings {
    pub provider: TypeName,
    pub model: String,
    pub max_output_tokens: u32,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSnapshot {
    pub company_id: CompanyId,
    /// Trusted library identity for revocation checks; never model input.
    pub skill_id: uuid::Uuid,
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
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedActionPolicy {
    pub capability: TypeName,
    pub policy_revision: u64,
    pub effect: ActionEffect,
    pub recovery: ActionRecovery,
}

#[cfg(test)]
mod contract_tests;

#[cfg(test)]
mod resolution_tests;
