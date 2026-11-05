//! The same bounded validation is used by previews, publication and fixtures.
use super::definition::invalid;
use super::{AgentDraft, ResolvedAgentDefinition, ResolvedAgentSkill, ResolvedAgentTool};
use crate::app_error::AppResult;
use crate::entities::agent_capability::validate_agent_bindings;

pub struct AgentResolutionFacts {
    pub draft: AgentDraft,
    /// Ordered exactly like the draft; a name match cannot substitute another identity.
    pub tools: Vec<ResolvedAgentTool>,
    pub skills: Vec<ResolvedAgentSkill>,
    pub model_available: bool,
}

pub fn resolve_agent(facts: AgentResolutionFacts) -> AppResult<ResolvedAgentDefinition> {
    let AgentResolutionFacts {
        draft,
        tools,
        skills,
        model_available,
    } = facts;
    validate_agent_bindings(&draft.bindings).map_err(crate::app_error::AppError::BadRequest)?;
    if !model_available
        || tools.len() != draft.bindings.len()
        || skills.len() != draft.skills.len()
        || draft.skills.len() > crate::entities::agent::MAX_AGENT_SKILLS
        || draft
            .bindings
            .iter()
            .zip(&tools)
            .any(|(wanted, actual)| wanted != &actual.binding)
        || draft.skills.iter().zip(&skills).any(|(wanted, actual)| {
            wanted.id != actual.assignment_id || wanted.skill_id != actual.skill.skill_id
        })
    {
        return Err(invalid(
            "Agent sources changed or the selected model is unavailable",
        ));
    }
    let definition = ResolvedAgentDefinition {
        company_id: draft.company_id,
        agent_id: draft.agent_id,
        key: draft.key,
        instructions: draft.instructions,
        model: draft
            .model
            .ok_or_else(|| invalid("Select an explicit agent provider and model"))?,
        tools,
        skills,
    };
    definition.validate()?;
    Ok(definition)
}
