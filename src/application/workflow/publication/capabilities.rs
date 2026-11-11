//! Workflows retain identities; execution resolves one immutable revision from its run manifest.
use super::*;
use crate::app_error::{AppError, AppResult};
use crate::application::agent::{
    AgentKey, PublishedAgentRevision, ResolvedAgentDefinition, ResolvedAgentSkill,
    ResolvedAgentTool,
};
use crate::entities::agent_capability::DependencyManifestId;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct ResolvedAgent {
    manifest: DependencyManifestId,
    revision: PublishedAgentRevision,
    mode: AgentUseMode,
}

impl ResolvedAgent {
    pub fn from_revision(
        manifest: DependencyManifestId,
        revision: PublishedAgentRevision,
    ) -> AppResult<Self> {
        if manifest.as_uuid().is_nil()
            || revision.id.as_uuid().is_nil()
            || revision.definition.content_hash()? != revision.digest
        {
            return Err(AppError::Internal("Invalid selected agent revision".into()));
        }
        revision.definition.hosted_model()?;
        Ok(Self {
            manifest,
            revision,
            mode: AgentUseMode::Tools,
        })
    }
    pub fn company_id(&self) -> CompanyId {
        self.revision.definition.company_id
    }
    pub fn agent(&self) -> &ResolvedAgentDefinition {
        &self.revision.definition
    }
    pub fn revision(&self) -> &PublishedAgentRevision {
        &self.revision
    }
    pub fn manifest_id(&self) -> DependencyManifestId {
        self.manifest
    }
    pub fn mode(&self) -> AgentUseMode {
        self.mode
    }
    pub fn tools(&self) -> &[ResolvedAgentTool] {
        if self.mode == AgentUseMode::ModelOnly {
            &[]
        } else {
            &self.revision.definition.tools
        }
    }
    pub fn skills(&self) -> &[ResolvedAgentSkill] {
        if self.mode == AgentUseMode::ModelOnly {
            &[]
        } else {
            &self.revision.definition.skills
        }
    }
    pub fn model_only(mut self) -> Self {
        self.mode = AgentUseMode::ModelOnly;
        self
    }
}

impl PublishedBundle {
    pub fn agent_reference(
        &self,
        company: CompanyId,
        key: &AgentKey,
    ) -> AppResult<&AgentReference> {
        if company != self.company_id {
            return Err(AppError::NotFound("Agent reference".into()));
        }
        self.snapshots
            .agents
            .iter()
            .find(|agent| &agent.key == key)
            .ok_or_else(|| {
                AppError::BadRequest("Agent is outside the published reference universe".into())
            })
    }
}

/// Every declared candidate and its actual source uses, independently of the first branch run.
pub fn agent_reference_uses(
    decoded: &compiler::DecodedSource,
) -> Result<BTreeMap<AgentKey, Vec<AgentReferenceUse>>, String> {
    let parsed = compiler::parse_workflow(decoded, VersionId::new(uuid::Uuid::nil()))
        .map_err(|diagnostic| diagnostic.message.into_string())?;
    reference_uses(&parsed.definition)
}

pub(super) fn reference_uses(
    definition: &crate::domain::workflow::WorkflowDefinition,
) -> Result<BTreeMap<AgentKey, Vec<AgentReferenceUse>>, String> {
    let mut references: BTreeMap<AgentKey, Vec<AgentReferenceUse>> = BTreeMap::new();
    for (id, step) in &definition.steps {
        let Some(kind) =
            crate::application::workflow::registry::ModelStep::of(step.step_type.as_str())
        else {
            continue;
        };
        let mode = if kind == crate::application::workflow::registry::ModelStep::AgentRun {
            AgentUseMode::Tools
        } else {
            AgentUseMode::ModelOnly
        };
        let refused =
            || format!("Step {id}: dynamic agent selection requires a finite set of agent keys");
        let options = match step.inputs.get("agent") {
            Some(crate::domain::workflow::Binding::Literal(Value::String(key))) => {
                vec![key.clone()]
            }
            Some(crate::domain::workflow::Binding::Reference(reference))
                if reference.root_name() == "params" && reference.tail().len() == 1 =>
            {
                definition
                    .parameter_schema
                    .as_ref()
                    .and_then(|schema| {
                        schema["properties"][&reference.tail()[0]]["enum"].as_array()
                    })
                    .filter(|options| !options.is_empty() && options.len() <= MAX_DEPENDENCIES)
                    .ok_or_else(refused)?
                    .iter()
                    .map(|option| option.as_str().map(str::to_owned).ok_or_else(refused))
                    .collect::<Result<Vec<_>, _>>()?
            }
            _ => return Err(refused()),
        };
        for option in options {
            let key =
                AgentKey::parse(&option).ok_or_else(|| format!("Invalid agent key {option}"))?;
            let uses = references.entry(key).or_default();
            let source = AgentReferenceUse {
                step: id.clone(),
                mode,
            };
            if !uses.contains(&source) {
                uses.push(source);
            }
            if references.len() > MAX_DEPENDENCIES {
                return Err("Agent reference universe exceeds 256 selections".into());
            }
        }
    }
    Ok(references)
}

pub fn agent_universe(decoded: &compiler::DecodedSource) -> Result<Vec<AgentKey>, String> {
    Ok(agent_reference_uses(decoded)?.into_keys().collect())
}
