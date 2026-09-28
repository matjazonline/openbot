//! Immutable binding configuration and current resource readiness checks.
//! Lifecycle authorization and atomic revision selection belong to the command/store
//! layer. Readiness is a current observation, never a durable authorization grant.

mod resources;
pub(crate) use resources::validate as validate_resource;
pub use resources::{ResourceDirectory, ResourceReadiness, ResourceStatus};

use super::publication::PublishedBundle;
use super::{WorkflowActor, compiler};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    BindingRevision, CompanyId, ResourceName, RuntimeResourceId, WorkflowBindingId,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Candidate data for one immutable revision. Resource IDs are credential references,
/// not credentials. Caller-supplied ownership is checked against the frozen bundle.
pub struct BindingConfiguration {
    pub id: WorkflowBindingId,
    pub revision: BindingRevision,
    pub company_id: CompanyId,
    pub params: Value,
    pub resources: BTreeMap<ResourceName, RuntimeResourceId>,
}

/// Validated configuration. Replacing the selected revision cannot mutate this value
/// or any run retaining it. Construction does not activate a binding or check access.
pub struct ConfiguredBinding {
    configuration: BindingConfiguration,
    bundle: Arc<PublishedBundle>,
}

impl ConfiguredBinding {
    pub fn new(
        configuration: BindingConfiguration,
        bundle: Arc<PublishedBundle>,
    ) -> Result<Self, compiler::Diagnostic> {
        if configuration.company_id != bundle.company_id() {
            return Err(configuration_error(
                &bundle,
                "binding.ownership",
                "/workflow_id",
                "Published workflow belongs to another company",
            ));
        }
        bundle.compiled().validate_params(&configuration.params)?;
        validate_slots(&configuration, &bundle)?;
        Ok(Self {
            configuration,
            bundle,
        })
    }

    pub fn id(&self) -> WorkflowBindingId {
        self.configuration.id
    }
    pub fn revision(&self) -> BindingRevision {
        self.configuration.revision
    }
    pub fn company_id(&self) -> CompanyId {
        self.configuration.company_id
    }
    pub fn params(&self) -> &Value {
        &self.configuration.params
    }
    pub fn resources(&self) -> &BTreeMap<ResourceName, RuntimeResourceId> {
        &self.configuration.resources
    }
    pub fn bundle(&self) -> &Arc<PublishedBundle> {
        &self.bundle
    }

    /// Recheck every selected resource against current trusted directory facts.
    /// The caller must first authorize management of this binding. Later activation
    /// commit/admission/effect paths must recheck authority where their atomicity
    /// requires it; success here cannot bypass revocation at credential use time.
    pub async fn check_readiness(
        &self,
        actor: WorkflowActor,
        directory: &impl ResourceDirectory,
    ) -> AppResult<()> {
        for requirement in &self.bundle.compiled().graph().definition().resources {
            let id = self.configuration.resources[&requirement.slot];
            let status = directory
                .inspect(self.company_id(), actor, id)
                .await?
                .ok_or_else(|| AppError::NotFound("Workflow resource".into()))?;
            validate_resource(self.company_id(), id, requirement, &status)?;
        }
        Ok(())
    }
}

fn validate_slots(
    configuration: &BindingConfiguration,
    bundle: &PublishedBundle,
) -> Result<(), compiler::Diagnostic> {
    let declared = &bundle.compiled().graph().definition().resources;
    // Reject excess cardinality before traversing caller-controlled selections.
    if configuration.resources.len() > declared.len() {
        return Err(configuration_error(
            bundle,
            "binding.resources",
            "/resources",
            "Binding supplies more resources than the workflow declares",
        ));
    }
    for (index, requirement) in declared.iter().enumerate() {
        if !configuration.resources.contains_key(&requirement.slot) {
            return Err(configuration_error(
                bundle,
                "binding.resources",
                &format!("/resources/{index}/slot"),
                &format!("Binding must select resource slot {}", requirement.slot),
            ));
        }
    }
    Ok(())
}

fn configuration_error(
    bundle: &PublishedBundle,
    code: &'static str,
    path: &str,
    message: &str,
) -> compiler::Diagnostic {
    compiler::Diagnostic::at(
        code,
        message,
        path,
        compiler::input_span(bundle.compiled().source_map(), path),
    )
}

#[cfg(test)]
mod tests;
