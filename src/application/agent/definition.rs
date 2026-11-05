use super::{AgentKey, ApprovedActionPolicy, ModelSettings, SkillSnapshot};
use crate::app_error::{AppError, AppResult};
use crate::domain::entities::agent_capability::{
    AgentDraftRevision, AgentId, AgentSkillAssignmentId, AgentToolBinding, validate_agent_bindings,
};
use crate::domain::workflow::CompanyId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_AGENT_DEFINITION_BYTES: usize = 1024 * 1024;
pub const MAX_AGENT_TEXT_BYTES: usize = crate::entities::skill::MAX_SKILL_INSTRUCTIONS_BYTES;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDraft {
    pub company_id: CompanyId,
    pub agent_id: AgentId,
    pub revision: AgentDraftRevision,
    pub key: AgentKey,
    pub instructions: String,
    pub model: Option<ModelSettings>,
    pub bindings: Vec<AgentToolBinding>,
    pub skills: Vec<AgentSkillAssignment>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSkillAssignment {
    pub id: AgentSkillAssignmentId,
    pub skill_id: uuid::Uuid,
}

/// Everything that describes execution, except deliberately live authority and secrets.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedAgentDefinition {
    pub company_id: CompanyId,
    pub agent_id: AgentId,
    pub key: AgentKey,
    pub instructions: String,
    pub model: ModelSettings,
    pub tools: Vec<ResolvedAgentTool>,
    pub skills: Vec<ResolvedAgentSkill>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedAgentTool {
    pub binding: AgentToolBinding,
    pub input_schema: Value,
    pub output_schema: Value,
    pub policy: ApprovedActionPolicy,
    pub approval_required: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedAgentSkill {
    pub assignment_id: AgentSkillAssignmentId,
    pub skill: SkillSnapshot,
}

pub(super) fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}

impl ResolvedAgentDefinition {
    pub fn decode(bytes: &[u8]) -> AppResult<Self> {
        if bytes.len() > MAX_AGENT_DEFINITION_BYTES {
            return Err(invalid("Agent definition exceeds 1 MiB"));
        }
        let definition: Self =
            serde_json::from_slice(bytes).map_err(|_| invalid("Invalid agent definition"))?;
        definition.validate()?;
        Ok(definition)
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.agent_id.as_uuid().is_nil()
            || self.instructions.len() > MAX_AGENT_TEXT_BYTES
            || self.model.model.is_empty()
            || self.model.model.len() > 128
            || !(1..=131_072).contains(&self.model.max_output_tokens)
        {
            return Err(invalid(
                "Invalid agent identity, instructions or model settings",
            ));
        }
        let bindings: Vec<_> = self.tools.iter().map(|tool| tool.binding.clone()).collect();
        validate_agent_bindings(&bindings).map_err(AppError::BadRequest)?;
        let mut skills = BTreeSet::new();
        let mut assignments = BTreeSet::new();
        if self.skills.len() > crate::entities::agent::MAX_AGENT_SKILLS {
            return Err(invalid("An agent may have at most 16 skills"));
        }
        for assigned in &self.skills {
            let skill = &assigned.skill;
            if skill.company_id != self.company_id
                || !skills.insert(skill.skill_id)
                || !assignments.insert(assigned.assignment_id)
                || skill.instructions.len() > MAX_AGENT_TEXT_BYTES
                || skill.required_tools.len() > 32
                || skill.required_tools.iter().any(|required| {
                    !self
                        .tools
                        .iter()
                        .any(|tool| tool.binding.alias.as_str() == required.as_str())
                })
            {
                return Err(invalid(
                    "Invalid skill assignment or missing required agent tool",
                ));
            }
        }
        for tool in &self.tools {
            if tool.policy.policy_revision == 0
                || serde_json::to_vec(tool)
                    .map_err(|_| invalid("Invalid tool contract"))?
                    .len()
                    > crate::domain::entities::agent_capability::MAX_AGENT_BINDING_BYTES
            {
                return Err(invalid("Invalid or oversized agent tool contract"));
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| invalid("Invalid agent definition"))?
            .len()
            > MAX_AGENT_DEFINITION_BYTES
        {
            return Err(invalid("Agent definition exceeds 1 MiB"));
        }
        Ok(())
    }

    /// Object keys are sorted by serde_json's Map; ordered bindings/skills retain their order.
    pub fn canonical_content(&self) -> AppResult<Vec<u8>> {
        self.validate()?;
        let value = serde_json::to_value(self).map_err(|_| invalid("Invalid agent definition"))?;
        serde_json::to_vec(&value).map_err(|_| invalid("Invalid agent definition"))
    }

    pub fn content_hash(&self) -> AppResult<[u8; 32]> {
        Ok(Sha256::digest(self.canonical_content()?).into())
    }
}
