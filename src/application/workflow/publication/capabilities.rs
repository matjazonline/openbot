//! Frozen capability selection. Current policy is a separate execution check,
//! never a source of replacement instructions or additional grants.
use super::*;
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{CapabilityCeiling, TypeName};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionSource {
    AgentDefaults,
    Profile,
}

/// Constructed only from a validated publication. Serialized content is trace
/// data, not a permission token; execution must recheck current authority.
#[derive(Clone, Serialize)]
pub struct ResolvedAgent {
    company_id: CompanyId,
    agent: AgentSnapshot,
    source: SelectionSource,
    profile: Option<TypeName>,
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
    pub fn source(&self) -> SelectionSource {
        self.source
    }
    pub fn profile(&self) -> Option<&TypeName> {
        self.profile.as_ref()
    }
    pub fn tools(&self) -> &[ToolSnapshot] {
        &self.tools
    }
    pub fn skills(&self) -> &[SkillSnapshot] {
        &self.skills
    }

    /// Refuse revoked capabilities without changing frozen content or broadening
    /// it with later grants. The caller must obtain these sets from current policy
    /// for this exact company/agent; this pure check does not authorize dispatch.
    pub fn validate_current(&self, allowed: &CapabilityCeiling) -> AppResult<()> {
        if self
            .agent
            .tools
            .iter()
            .any(|tool| !allowed.tools.contains(tool))
            || self
                .agent
                .skills
                .iter()
                .any(|skill| !allowed.skills.contains(skill))
        {
            return Err(AppError::Conflict(
                "Selected agent capability was revoked".into(),
            ));
        }
        Ok(())
    }
}

pub(super) struct SelectedProfile {
    pub name: Option<TypeName>,
    pub tools: Vec<TypeName>,
    pub skills: Vec<TypeName>,
}

pub(super) fn profile(
    value: &Value,
    snapshots: &DependencySnapshots,
) -> Result<SelectedProfile, String> {
    if let Some(name) = value.as_str() {
        let name =
            TypeName::parse(name).map_err(|_| "Invalid capability profile name".to_owned())?;
        let selected = snapshots
            .profiles
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("Unknown capability profile {name}"))?;
        return Ok(SelectedProfile {
            name: Some(selected.name.clone()),
            tools: selected.tools.clone(),
            skills: selected.skills.clone(),
        });
    }
    let object = value
        .as_object()
        .ok_or_else(|| "Capability profile must be a name or object".to_owned())?;
    if object.len() != 2 || !object.contains_key("tools") || !object.contains_key("skills") {
        return Err("Capability profile must declare exactly tools and skills".into());
    }
    Ok(SelectedProfile {
        name: None,
        tools: names(&object["tools"], "tools")?,
        skills: names(&object["skills"], "skills")?,
    })
}

fn names(value: &Value, field: &str) -> Result<Vec<TypeName>, String> {
    let values = value
        .as_array()
        .ok_or_else(|| format!("Profile {field} must be an array"))?;
    if values.len() > registry::MAX_SELECTIONS {
        return Err(format!("Profile {field} exceeds the selection limit"));
    }
    let mut seen = BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let name = value
                .as_str()
                .and_then(|s| TypeName::parse(s).ok())
                .ok_or_else(|| format!("Profile {field} contains an invalid dependency name"))?;
            if !seen.insert(name.clone()) {
                return Err(format!("Profile {field} contains duplicate {name}"));
            }
            Ok(name)
        })
        .collect()
}

impl PublishedBundle {
    /// Absent profile inherits saved defaults. A present value, including null,
    /// is always treated as an explicit selection and never falls back.
    pub fn resolve_agent(
        &self,
        company: CompanyId,
        key: &AgentKey,
        selected_profile: Option<&Value>,
    ) -> AppResult<ResolvedAgent> {
        if company != self.company_id {
            return Err(AppError::NotFound("Agent bundle".into()));
        }
        let saved = self
            .snapshots
            .agents
            .iter()
            .find(|agent| &agent.key == key)
            .ok_or_else(|| AppError::BadRequest("Agent is absent from the frozen bundle".into()))?;
        let (source, mut selected) = match selected_profile {
            None => (
                SelectionSource::AgentDefaults,
                SelectedProfile {
                    name: None,
                    tools: saved.tools.clone(),
                    skills: saved.skills.clone(),
                },
            ),
            Some(value) => (
                SelectionSource::Profile,
                profile(value, &self.snapshots).map_err(AppError::BadRequest)?,
            ),
        };
        // Validate references before filtering so unknown names never silently disappear.
        validation::check_selection(&selected.tools, &selected.skills, &self.snapshots)
            .map_err(AppError::BadRequest)?;
        let ceiling = self
            .compiled
            .graph()
            .definition()
            .capability_ceiling
            .as_ref();
        selected.tools.retain(|name| {
            saved.tools.contains(name) && ceiling.is_none_or(|c| c.tools.contains(name))
        });
        selected.skills.retain(|name| {
            saved.skills.contains(name) && ceiling.is_none_or(|c| c.skills.contains(name))
        });
        validation::check_selection(&selected.tools, &selected.skills, &self.snapshots)
            .map_err(AppError::BadRequest)?;
        Ok(capture(company, saved, source, selected, &self.snapshots))
    }
}

fn capture(
    company: CompanyId,
    saved: &AgentSnapshot,
    source: SelectionSource,
    selected: SelectedProfile,
    snapshots: &DependencySnapshots,
) -> ResolvedAgent {
    let tools = selected
        .tools
        .iter()
        .map(|name| {
            snapshots
                .tools
                .iter()
                .find(|t| &t.contract.name == name)
                .expect("validated unique tool")
                .clone()
        })
        .collect();
    let skills = selected
        .skills
        .iter()
        .map(|name| {
            snapshots
                .skills
                .iter()
                .find(|s| &s.name == name)
                .expect("validated skill")
                .clone()
        })
        .collect();
    let mut agent = saved.clone();
    agent.tools = selected.tools;
    agent.skills = selected.skills;
    ResolvedAgent {
        company_id: company,
        agent,
        source,
        profile: selected.name,
        tools,
        skills,
    }
}
