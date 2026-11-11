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
    pub model: Option<ModelSettings>,
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
    pub fn hosted_model(&self) -> AppResult<&ModelSettings> {
        self.model.as_ref().ok_or_else(|| invalid(
            "Hosted agent execution requires an explicit provider and model; native profiles choose their own model",
        ))
    }

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
            || self.model.as_ref().is_some_and(|model| {
                model.model.is_empty()
                    || model.model.len() > 128
                    || !(1..=131_072).contains(&model.max_output_tokens)
            })
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
            crate::entities::skill_bundle::validate_skill_files(&skill.files)
                .map_err(AppError::BadRequest)?;
            crate::entities::skill_bundle::native_skill_markdown(
                skill.name.as_str(),
                &skill.description,
                &skill.instructions,
            )
            .map_err(AppError::BadRequest)?;
            if skill.company_id != self.company_id
                || !skills.insert(skill.skill_id)
                || !assignments.insert(assigned.assignment_id)
                || skill.instructions.len() > MAX_AGENT_TEXT_BYTES
                || skill.required_tools.len() > 32
            {
                return Err(invalid("Invalid skill assignment"));
            }
            if let Some(required) = skill.required_tools.iter().find(|required| {
                !self
                    .tools
                    .iter()
                    .any(|tool| tool.binding.alias.as_str() == required.as_str())
            }) {
                return Err(invalid(&format!(
                    "Skill {} requires tool alias {}. Add that tool to the agent draft before publishing.",
                    skill.name.as_str(),
                    required.as_str()
                )));
            }
        }
        let mut declarations = BTreeSet::new();
        for tool in &self.tools {
            if tool.policy.policy_revision == 0
                || serde_json::to_vec(tool)
                    .map_err(|_| invalid("Invalid tool contract"))?
                    .len()
                    > crate::domain::entities::agent_capability::MAX_AGENT_BINDING_BYTES
            {
                return Err(invalid("Invalid or oversized agent tool contract"));
            }
            validate_tool_schema(tool)?;
            if !declarations.insert(super::AgentModelToolName::for_alias(&tool.binding.alias)?) {
                return Err(invalid(
                    "Agent aliases produce an ambiguous model function name",
                ));
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

fn validate_tool_schema(tool: &ResolvedAgentTool) -> AppResult<()> {
    use crate::application::workflow::compiler::{Schema, SourceSpan};
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    for schema in [&tool.input_schema, &tool.output_schema] {
        Schema::compile(schema.clone(), "/agent-tool", span).map_err(|_| {
            invalid(&format!(
                "Agent tool {} has an invalid schema",
                tool.binding.alias.as_str()
            ))
        })?;
    }
    Ok(())
}
