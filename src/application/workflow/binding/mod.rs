//! Immutable binding configuration and current resource readiness checks.
//! Lifecycle authorization and atomic revision selection belong to the command/store
//! layer. Readiness is a current observation, never a durable authorization grant.

mod resources;
pub(crate) use resources::validate as validate_resource;
pub use resources::{ResourceDirectory, ResourceReadiness, ResourceStatus};

use super::publication::PublishedBundle;
use super::triggers::MessageTriggerInput;
use super::{RelatedAssociation, WorkflowActor, compiler};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    BindingRevision, CompanyId, MessageTrigger, ResourceName, RuntimeResourceId, WorkflowBindingId,
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
    /// Whose authority automatic (message/schedule) runs of this revision act with (p08 D1.1).
    pub run_as: WorkflowActor,
    /// Which accepted messages admit a run; `None` admits none (p08 D1.2).
    pub message_trigger: Option<MessageTrigger>,
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
    pub fn run_as(&self) -> WorkflowActor {
        self.configuration.run_as
    }
    pub fn message_trigger(&self) -> Option<MessageTrigger> {
        self.configuration.message_trigger
    }
    pub fn bundle(&self) -> &Arc<PublishedBundle> {
        &self.bundle
    }

    /// A message trigger needs a channel or thread association, and the pinned input schema
    /// must accept the standard message input, so a bad pairing fails at configuration rather
    /// than refusing every message (p08 D1.2).
    pub fn check_message_trigger(
        &self,
        association: RelatedAssociation,
    ) -> Result<(), compiler::Diagnostic> {
        if self.configuration.message_trigger.is_none() {
            return Ok(());
        }
        if matches!(association, RelatedAssociation::Company) {
            return Err(configuration_error(
                &self.bundle,
                "binding.message_trigger",
                "/message_trigger",
                "A message trigger needs a channel or thread association",
            ));
        }
        self.bundle
            .compiled()
            .validate_input(&MessageTriggerInput::representative())
            .map_err(|_| {
                configuration_error(
                    &self.bundle,
                    "binding.message_trigger",
                    "/input_schema",
                    "The workflow input schema must accept the standard message input \
                     (source_message, message, subject, sender)",
                )
            })
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
