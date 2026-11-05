//! Agent-owned identities and concrete capability bindings. These are configuration,
//! never authorization to make an effect or to open a credential.
use super::mcp::McpToolName;
use crate::domain::workflow::{TypeName, VersionId, WorkflowId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

pub const MAX_AGENT_BINDINGS: usize = 32;
pub const MAX_AGENT_BINDING_BYTES: usize = 65_536;

macro_rules! identity {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            pub const fn new(id: Uuid) -> Self { Self(id) }
            pub const fn as_uuid(self) -> Uuid { self.0 }
        }
    )+};
}
identity!(
    AgentId,
    AgentRevisionId,
    AgentBindingId,
    AgentSkillAssignmentId,
    DependencyManifestId
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct AgentDraftRevision(u64);
impl AgentDraftRevision {
    pub fn new(value: u64) -> Result<Self, String> {
        if value == 0 || value > i64::MAX as u64 {
            return Err("Agent draft revision is outside its positive bigint range".into());
        }
        Ok(Self(value))
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl TryFrom<u64> for AgentDraftRevision {
    type Error = String;
    fn try_from(value: u64) -> Result<Self, String> {
        Self::new(value)
    }
}
impl From<AgentDraftRevision> for u64 {
    fn from(value: AgentDraftRevision) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentToolAlias(TypeName);
impl AgentToolAlias {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, String> {
        TypeName::parse(value)
            .map(Self)
            .map_err(|error| error.to_string())
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn as_type_name(&self) -> &TypeName {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentBuiltinTool {
    MessageSend,
    MessageReply,
    ListCompanyAgents,
    CreateAgentChannel,
    SearchKnowledge,
}
impl AgentBuiltinTool {
    pub const fn name(self) -> &'static str {
        match self {
            Self::MessageSend => "message.send",
            Self::MessageReply => "message.reply",
            Self::ListCompanyAgents => "list_company_agents",
            Self::CreateAgentChannel => "create_agent_channel",
            Self::SearchKnowledge => "search_knowledge",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "message.send" => Some(Self::MessageSend),
            "message.reply" => Some(Self::MessageReply),
            "list_company_agents" => Some(Self::ListCompanyAgents),
            "create_agent_channel" => Some(Self::CreateAgentChannel),
            "search_knowledge" => Some(Self::SearchKnowledge),
            _ => None,
        }
    }
    pub const fn is_provisioning(self) -> bool {
        matches!(self, Self::ListCompanyAgents | Self::CreateAgentChannel)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentBindingSource {
    Builtin {
        tool: AgentBuiltinTool,
    },
    Http {
        connection: Uuid,
        operation: TypeName,
    },
    Mcp {
        connection: Uuid,
        operation: TypeName,
        reviewed_version: Uuid,
        remote_name: McpToolName,
    },
    Workflow {
        registration: TypeName,
        workflow: WorkflowId,
        version: VersionId,
        params: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolBinding {
    pub id: AgentBindingId,
    pub alias: AgentToolAlias,
    pub source: AgentBindingSource,
}
impl AgentToolBinding {
    pub fn builtin(tool: AgentBuiltinTool) -> Self {
        Self {
            id: AgentBindingId::new(Uuid::new_v4()),
            alias: AgentToolAlias::parse(tool.name()).expect("closed built-in alias"),
            source: AgentBindingSource::Builtin { tool },
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let reserved = self.alias.as_str().starts_with("message.")
            || AgentBuiltinTool::parse(self.alias.as_str()).is_some();
        if reserved
            && !matches!(&self.source,
            AgentBindingSource::Builtin { tool } if tool.name() == self.alias.as_str())
        {
            return Err("Reserved agent alias must name its exact trusted built-in".into());
        }
        if let AgentBindingSource::Workflow { params, .. } = &self.source
            && !params.is_object()
        {
            return Err("Workflow tool parameters must be an object".into());
        }
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_AGENT_BINDING_BYTES
        {
            return Err("Agent tool binding exceeds 64 KiB".into());
        }
        Ok(())
    }
}

pub fn validate_agent_bindings(bindings: &[AgentToolBinding]) -> Result<(), String> {
    if bindings.len() > MAX_AGENT_BINDINGS {
        return Err("An agent may have at most 32 tool bindings".into());
    }
    let mut aliases = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for binding in bindings {
        binding.validate()?;
        if !aliases.insert(&binding.alias) || !ids.insert(binding.id) {
            return Err("Agent tool aliases and binding identities must be distinct".into());
        }
    }
    Ok(())
}
