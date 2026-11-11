//! Bounded authoring metadata. No current credential or immutable prompt/argument payload is projected.
use super::*;
use crate::app_error::AppResult;
use crate::application::workflow::WorkflowActor;
use crate::entities::{agent::Agent, agent_capability::*};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const AGENT_HISTORY_PAGE: usize = 20;
pub const AGENT_CATALOG_PAGE: usize = 50;

#[derive(Clone, Copy)]
pub struct AgentCompanyScope {
    pub company: CompanyId,
    pub actor: WorkflowActor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolKind {
    Builtin,
    Http,
    Mcp,
    Workflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentGrantState {
    Unpublished,
    Live,
    Revoked,
}

/// Configuration hints only. Publication and execution still perform the complete current policy checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolAvailability {
    Configured,
    ConnectionUnavailable,
    GrantRequired,
    ReviewChanged,
    OperationUnavailable,
    HistoricalPin,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentToolState {
    pub assignment: AgentToolAssignment,
    pub access: AgentGrantState,
    pub availability: AgentToolAvailability,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentAuthoringSkill {
    pub id: Uuid,
    pub assignment: AgentSkillAssignmentId,
    pub name: String,
    pub description: String,
    pub required_tools: Vec<TypeName>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentActiveSkill {
    pub assignment: AgentSkillAssignmentId,
    pub skill: Uuid,
    pub name: TypeName,
    pub access: AgentGrantState,
}

#[derive(Clone, Serialize)]
pub struct AgentActiveState {
    pub id: AgentRevisionId,
    pub draft_revision: AgentDraftRevision,
    pub key: AgentKey,
    pub model: Option<ModelSettings>,
    pub tools: Vec<AgentToolState>,
    pub skills: Vec<AgentActiveSkill>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentHistoryCursor {
    pub created_at: DateTime<Utc>,
    pub id: AgentRevisionId,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentHistoryEntry {
    pub id: AgentRevisionId,
    pub draft_revision: AgentDraftRevision,
    pub created_at: DateTime<Utc>,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentHistoryPage {
    pub items: Vec<AgentHistoryEntry>,
    pub next: Option<AgentHistoryCursor>,
}

#[derive(Clone, Serialize)]
pub struct AgentAuthoringState {
    pub agent: Agent,
    pub skills: Vec<AgentAuthoringSkill>,
    pub tools: Vec<AgentToolState>,
    pub active: Option<AgentActiveState>,
    pub history: AgentHistoryPage,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentConnectionOption {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentConnectionPage {
    pub items: Vec<AgentConnectionOption>,
    pub next: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentToolOption {
    pub name: TypeName,
    pub choice: AgentToolChoice,
    pub availability: AgentToolAvailability,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentToolPage {
    pub items: Vec<AgentToolOption>,
    pub next: Option<TypeName>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentDirectoryEntry {
    pub id: AgentId,
    pub key: AgentKey,
    pub name: String,
    pub active_revision: Option<AgentRevisionId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentDirectoryPage {
    pub items: Vec<AgentDirectoryEntry>,
    pub next: Option<AgentKey>,
}

pub struct AgentToolPageRequest {
    pub scope: AgentPublicationScope,
    pub kind: AgentToolKind,
    pub connection: Option<Uuid>,
    pub after: Option<TypeName>,
}

#[async_trait]
pub trait AgentAuthoringReads: Send + Sync {
    async fn authoring_state(&self, scope: AgentPublicationScope)
    -> AppResult<AgentAuthoringState>;
    async fn revision_history(
        &self,
        scope: AgentPublicationScope,
        before: Option<AgentHistoryCursor>,
    ) -> AppResult<AgentHistoryPage>;
    async fn tool_connections(
        &self,
        scope: AgentPublicationScope,
        kind: AgentToolKind,
        after: Option<Uuid>,
    ) -> AppResult<AgentConnectionPage>;
    async fn tool_options(&self, request: AgentToolPageRequest) -> AppResult<AgentToolPage>;
    async fn agent_directory(
        &self,
        scope: AgentCompanyScope,
        after: Option<AgentKey>,
    ) -> AppResult<AgentDirectoryPage>;
    /// Retain an exact referenced selection even when it is outside the current directory page.
    async fn agent_directory_entry(
        &self,
        scope: AgentCompanyScope,
        key: AgentKey,
    ) -> AppResult<Option<AgentDirectoryEntry>>;
    async fn authorize_company_authoring(&self, scope: AgentCompanyScope) -> AppResult<()>;
    /// Fresh current authority immediately before a prepared privileged authoring response is released.
    async fn authorize_authoring(&self, scope: AgentPublicationScope) -> AppResult<()>;
}

impl AgentToolChoice {
    pub fn target_label(&self) -> String {
        match self {
            Self::Builtin { tool } => format!("Built-in {}", tool.name()),
            Self::Http {
                connection,
                operation,
            } => format!("HTTP {operation} · connection {connection}"),
            Self::Mcp {
                connection,
                remote_name,
                reviewed_version,
                operation,
            } => format!(
                "MCP {operation} → {} · connection {connection} · review {reviewed_version}",
                remote_name.as_str()
            ),
            Self::Workflow {
                registration,
                workflow,
                version,
                ..
            } => format!("Workflow {registration} · {workflow} · version {version}"),
        }
    }
}
