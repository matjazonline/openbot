//! Frozen agent capabilities (06-D5): exactly the agent's saved tools and skills, in
//! saved order. Nothing at a step, classifier or skill can add or replace them. Current
//! revocation is a separate execution-time check, never a source of new grants.
use super::*;
use crate::app_error::{AppError, AppResult};
use serde::Serialize;

/// Constructed only from a validated publication. Serialized content is trace
/// data, not a permission token; execution must recheck current authority.
#[derive(Clone, Serialize)]
pub struct ResolvedAgent {
    company_id: CompanyId,
    agent: AgentSnapshot,
    tools: Vec<ToolSnapshot>,
    skills: Vec<SkillSnapshot>,
}

impl ResolvedAgent {
    pub fn company_id(&self) -> CompanyId {
        self.company_id
    }
    pub fn agent(&self) -> &AgentSnapshot {
        &self.agent
    }
    pub fn tools(&self) -> &[ToolSnapshot] {
        &self.tools
    }
    pub fn skills(&self) -> &[SkillSnapshot] {
        &self.skills
    }
    /// The same frozen agent with no tools or skills, for tool-free model steps
    /// (`ai.classify`, `decision.agent`; 06-D6 C/D). Narrowing only: the prepared document
    /// then records no capability, so current authority checks only agent and model.
    pub fn model_only(mut self) -> Self {
        self.agent.tools.clear();
        self.agent.skills.clear();
        self.tools.clear();
        self.skills.clear();
        self
    }
}

impl PublishedBundle {
    /// The frozen agent with exactly its saved tools and skills. Publication already
    /// validated every reference and each skill's required tools against these tools.
    pub fn resolve_agent(&self, company: CompanyId, key: &AgentKey) -> AppResult<ResolvedAgent> {
        if company != self.company_id {
            return Err(AppError::NotFound("Agent bundle".into()));
        }
        let saved = self
            .snapshots
            .agents
            .iter()
            .find(|agent| &agent.key == key)
            .ok_or_else(|| AppError::BadRequest("Agent is absent from the frozen bundle".into()))?;
        validation::check_selection(&saved.tools, &saved.skills, &self.snapshots)
            .map_err(AppError::BadRequest)?;
        let tools = saved
            .tools
            .iter()
            .map(|name| {
                self.snapshots
                    .tools
                    .iter()
                    .find(|tool| &tool.contract.name == name)
                    .cloned()
                    .ok_or_else(|| AppError::BadRequest("Agent tool is not frozen".into()))
            })
            .collect::<AppResult<_>>()?;
        let skills = saved
            .skills
            .iter()
            .map(|name| {
                self.snapshots
                    .skills
                    .iter()
                    .find(|skill| &skill.name == name)
                    .cloned()
                    .ok_or_else(|| AppError::BadRequest("Agent skill is not frozen".into()))
            })
            .collect::<AppResult<_>>()?;
        Ok(ResolvedAgent {
            company_id: company,
            agent: saved.clone(),
            tools,
            skills,
        })
    }
}

/// The finite set of agent keys a draft's `agent.run`/`decision.agent` steps can select
/// (06-D5): a literal key, or a `/params/<name>` reference whose declared parameter schema
/// is a finite string enum. Any other selector is refused, so capture never guesses.
pub fn agent_universe(decoded: &compiler::DecodedSource) -> Result<Vec<AgentKey>, String> {
    let parsed = compiler::parse_workflow(decoded, VersionId::new(uuid::Uuid::nil()))
        .map_err(|diagnostic| diagnostic.message.into_string())?;
    let definition = &parsed.definition;
    let mut keys = std::collections::BTreeSet::new();
    for (id, step) in &definition.steps {
        if crate::application::workflow::registry::ModelStep::of(step.step_type.as_str()).is_none()
        {
            continue;
        }
        let refused =
            || format!("Step {id}: dynamic agent selection requires a finite set of agent keys");
        match step.inputs.get("agent") {
            Some(crate::domain::workflow::Binding::Literal(Value::String(key))) => {
                keys.insert(key.clone());
            }
            Some(crate::domain::workflow::Binding::Reference(reference))
                if reference.root_name() == "params" && reference.tail().len() == 1 =>
            {
                let options = definition
                    .parameter_schema
                    .as_ref()
                    .and_then(|schema| {
                        schema["properties"][&reference.tail()[0]]["enum"].as_array()
                    })
                    .filter(|options| !options.is_empty())
                    .ok_or_else(refused)?;
                for option in options {
                    keys.insert(option.as_str().ok_or_else(refused)?.to_owned());
                }
            }
            _ => return Err(refused()),
        }
    }
    keys.into_iter()
        .map(|key| AgentKey::parse(&key).ok_or_else(|| format!("Invalid agent key {key}")))
        .collect()
}
