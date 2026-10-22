use super::registry::ToolContract;
use crate::domain::workflow::{CompanyId, ResourceName, TypeName, VersionId, WorkflowId};
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
/// credentials. `(AgentSnapshot.company_id, provider)` is the existing company model
/// connection key. Resolve its current secret at runtime without following agent defaults.
#[derive(Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    pub provider: TypeName,
    pub model: String,
    pub max_output_tokens: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub company_id: CompanyId,
    /// Trusted identity for ownership checks and usage attribution; never model input.
    pub agent_id: uuid::Uuid,
    pub key: AgentKey,
    pub instructions: String,
    pub model: ModelSettings,
    pub tools: Vec<TypeName>,
    pub skills: Vec<TypeName>,
}

#[derive(Clone, Serialize, Deserialize)]
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
    /// Present only for a workflow tool (07-D2 §D2.2), so every other kind serializes as before
    /// and keeps comparing equal to its stored registration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<WorkflowToolPin>,
}

/// A workflow tool's frozen target: the registration's workflow version and static child
/// parameters at publication (07-D2 §D2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowToolPin {
    pub workflow_id: WorkflowId,
    pub version_id: VersionId,
    pub params: Value,
}

/// How a snapshot is dispatched (07-D2 §D2.2a). This is the only classifier: every site that
/// verifies or dispatches a tool matches it exhaustively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind<'a> {
    /// A trusted built-in `message.*` tool.
    Message,
    /// A tool reached through one of the workflow's resource slots (MCP or slot-bound HTTP).
    Slot(&'a ResourceName),
    /// A company HTTP operation registered by name.
    Http,
    /// A pinned child workflow registered by name.
    Workflow(&'a WorkflowToolPin),
}

impl ToolSnapshot {
    pub fn kind(&self) -> ToolKind<'_> {
        if let Some(pin) = &self.workflow {
            return ToolKind::Workflow(pin);
        }
        if crate::application::workflow::message::is_builtin_message_tool(
            self.contract.name.as_str(),
        ) {
            return ToolKind::Message;
        }
        match &self.contract.connection {
            Some(slot) => ToolKind::Slot(slot),
            None => ToolKind::Http,
        }
    }
}

/// The complete finite selection universe for this version. Agent/profile names
/// from runtime bindings can only choose captured entries; no live fallback.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct DependencySnapshots {
    pub agents: Vec<AgentSnapshot>,
    pub skills: Vec<SkillSnapshot>,
    pub tools: Vec<ToolSnapshot>,
}

impl ToolSnapshot {
    pub(super) fn schemas(&self) -> [&Value; 2] {
        [&self.contract.input_schema, &self.contract.output_schema]
    }
}
