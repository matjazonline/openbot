//! Trusted call provenance. A model's arguments never choose these identities.
use crate::app_error::{AppError, AppResult};
use crate::entities::agent_capability::{
    AgentBindingId, AgentId, AgentRevisionId, DependencyManifestId,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolOrigin {
    pub manifest: DependencyManifestId,
    pub agent: AgentId,
    pub revision: AgentRevisionId,
    pub binding: AgentBindingId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolCallOrigin {
    Workflow,
    Agent(AgentToolOrigin),
}

impl ToolCallOrigin {
    pub fn validate(self) -> AppResult<()> {
        if let Self::Agent(origin) = self
            && [
                origin.manifest.as_uuid(),
                origin.agent.as_uuid(),
                origin.revision.as_uuid(),
                origin.binding.as_uuid(),
            ]
            .iter()
            .any(uuid::Uuid::is_nil)
        {
            return Err(AppError::BadRequest("Invalid agent tool origin".into()));
        }
        Ok(())
    }
}
