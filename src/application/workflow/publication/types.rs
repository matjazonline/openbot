use super::registry::ToolContract;
use crate::application::agent::{AgentKey, ApprovedActionPolicy};
use crate::domain::workflow::{CompanyId, ResourceName, StepId, VersionId, WorkflowId};
use crate::entities::agent_capability::AgentId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ContentHash(pub(super) String);
impl ContentHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentReference {
    pub company_id: CompanyId,
    pub agent_id: AgentId,
    /// Authored name remains a source lookup key even after the agent is renamed.
    pub key: AgentKey,
    pub uses: Vec<AgentReferenceUse>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentReferenceUse {
    pub step: StepId,
    pub mode: AgentUseMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentUseMode {
    ModelOnly,
    Tools,
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
    /// A trusted local-agent workflow step; it is not a selectable generic tool.
    LocalAgent,
    /// A trusted built-in provisioning tool (p08 D2.1). Only a name-only snapshot carries a
    /// provisioning name this way; an MCP tool of the same name stays a slot tool.
    Provisioning,
    /// A name-only, exactly verified local knowledge search built-in.
    Knowledge,
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
        if self.contract.connection.is_none()
            && self.contract.name.as_str() == crate::application::local_agents::workflow::KIND
        {
            return ToolKind::LocalAgent;
        }
        if crate::application::workflow::message::is_builtin_message_tool(
            self.contract.name.as_str(),
        ) {
            return ToolKind::Message;
        }
        if self.contract.connection.is_none()
            && crate::application::workflow::provisioning::is_builtin_provisioning_tool(
                self.contract.name.as_str(),
            )
        {
            return ToolKind::Provisioning;
        }
        if self.contract.connection.is_none()
            && crate::application::workflow::knowledge::is_builtin_knowledge_tool(
                self.contract.name.as_str(),
            )
        {
            return ToolKind::Knowledge;
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
#[serde(deny_unknown_fields)]
pub struct DependencySnapshots {
    pub agents: Vec<AgentReference>,
    pub tools: Vec<ToolSnapshot>,
}

impl ToolSnapshot {
    pub(super) fn schemas(&self) -> [&Value; 2] {
        [&self.contract.input_schema, &self.contract.output_schema]
    }
}
