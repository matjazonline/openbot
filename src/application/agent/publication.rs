//! Company-authorized, independent agent publication; no workflow or resource slot is required.
use super::ResolvedAgentDefinition;
use crate::app_error::AppResult;
use crate::application::workflow::WorkflowActor;
use crate::domain::workflow::CompanyId;
use crate::entities::agent_capability::{
    AgentBindingId, AgentDraftRevision, AgentId, AgentRevisionId, AgentSkillAssignmentId,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AgentPublicationKey(String);

impl AgentPublicationKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err("Agent publication key must contain 1–128 visible ASCII characters".into());
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AgentPublicationKey {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(decoder)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy)]
pub struct AgentPublicationScope {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub agent: AgentId,
}

#[derive(Clone)]
pub struct PublishAgent {
    pub scope: AgentPublicationScope,
    pub expected_draft_revision: AgentDraftRevision,
    pub expected_active_revision: Option<AgentRevisionId>,
    pub key: AgentPublicationKey,
}

#[derive(Clone, Serialize)]
pub struct AgentPreview {
    pub draft_revision: AgentDraftRevision,
    pub active_revision: Option<AgentRevisionId>,
    pub definition: ResolvedAgentDefinition,
}

#[derive(Clone, Serialize)]
pub struct PublishedAgentRevision {
    pub id: AgentRevisionId,
    pub draft_revision: AgentDraftRevision,
    pub definition: ResolvedAgentDefinition,
    pub digest: [u8; 32],
}

#[derive(Clone, Copy)]
pub enum AgentGrantIdentity {
    Binding(AgentBindingId),
    Skill(AgentSkillAssignmentId),
}

#[async_trait]
pub trait AgentPublications: Send + Sync {
    async fn preview_agent(&self, scope: AgentPublicationScope) -> AppResult<AgentPreview>;
    async fn publish_agent(&self, request: PublishAgent) -> AppResult<PublishedAgentRevision>;
    async fn active_agent_revision(
        &self,
        scope: AgentPublicationScope,
    ) -> AppResult<Option<PublishedAgentRevision>>;
    async fn published_agent_revision(
        &self,
        scope: AgentPublicationScope,
        revision: AgentRevisionId,
    ) -> AppResult<Option<PublishedAgentRevision>>;
    async fn deactivate_agent(
        &self,
        scope: AgentPublicationScope,
        expected: Option<AgentRevisionId>,
    ) -> AppResult<()>;
    async fn revoke_agent_grant(
        &self,
        scope: AgentPublicationScope,
        identity: AgentGrantIdentity,
    ) -> AppResult<()>;
}
