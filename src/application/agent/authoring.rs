//! Company draft commands and explicit, credential-free capability choices.
use super::AgentPublicationScope;
use crate::app_error::{AppError, AppResult};
use crate::application::use_cases::agent::AgentWrite;
use crate::domain::workflow::{TypeName, VersionId, WorkflowId};
use crate::entities::{
    agent::Agent,
    agent_capability::{
        AgentBindingId, AgentBindingSource, AgentBuiltinTool, AgentDraftRevision, AgentId,
        AgentToolAlias,
    },
    mcp::McpToolName,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AgentParameterDigest(String);
impl TryFrom<String> for AgentParameterDigest {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(
                "A workflow parameter digest must contain 64 hexadecimal characters".into(),
            );
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
}
impl From<AgentParameterDigest> for String {
    fn from(value: AgentParameterDigest) -> Self {
        value.0
    }
}
impl AgentParameterDigest {
    pub fn of(params: &serde_json::Value) -> AppResult<Self> {
        let bytes = serde_json::to_vec(params)
            .map_err(|_| AppError::BadRequest("Invalid workflow parameters".into()))?;
        Ok(Self(format!("{:x}", Sha256::digest(bytes))))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentToolChoice {
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
        params_digest: AgentParameterDigest,
    },
}
impl AgentToolChoice {
    pub fn from_source(source: &AgentBindingSource) -> AppResult<Self> {
        Ok(match source {
            AgentBindingSource::Builtin { tool } => Self::Builtin { tool: *tool },
            AgentBindingSource::Http {
                connection,
                operation,
            } => Self::Http {
                connection: *connection,
                operation: operation.clone(),
            },
            AgentBindingSource::Mcp {
                connection,
                operation,
                reviewed_version,
                remote_name,
            } => Self::Mcp {
                connection: *connection,
                operation: operation.clone(),
                reviewed_version: *reviewed_version,
                remote_name: remote_name.clone(),
            },
            AgentBindingSource::Workflow {
                registration,
                workflow,
                version,
                params,
            } => Self::Workflow {
                registration: registration.clone(),
                workflow: *workflow,
                version: *version,
                params_digest: AgentParameterDigest::of(params)?,
            },
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolAssignment {
    pub binding: Option<AgentBindingId>,
    pub alias: AgentToolAlias,
    pub choice: AgentToolChoice,
}

pub struct CompanyAgentUpdate {
    pub agent: AgentId,
    pub expected_revision: AgentDraftRevision,
    pub write: AgentWrite,
    /// None is a metadata-only edit; an empty list deliberately clears the draft's tools.
    pub tools: Option<Vec<AgentToolAssignment>>,
}
pub struct SaveCompanyAgent {
    pub scope: AgentPublicationScope,
    pub expected_revision: AgentDraftRevision,
    pub write: AgentWrite,
    pub tools: Option<Vec<AgentToolAssignment>>,
}

#[async_trait]
pub trait AgentDraftPersistence: Send + Sync {
    /// Locks current authority and draft, compares the expected revision and commits all fields together.
    async fn save_company_agent(&self, request: SaveCompanyAgent) -> AppResult<Agent>;
}
