use super::contracts::{SourceIdentity, TemplateError};
use crate::application::workflow::{
    compiler::{MAX_SOURCE_BYTES, SourceDecoder},
    registry::{self, CatalogueFacts},
};
use crate::domain::workflow::{TemplateId, TemplateRevision, VersionId, WorkflowId};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_TEMPLATES: usize = 64;
pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_DESCRIPTION_BYTES: usize = 4096;
pub const MAX_CATALOGUE_BYTES: usize = 2 * 1024 * 1024;

/// Borrowed trusted assembly input. The complete list is preflighted before any
/// decoder/compiler work or owned entry allocation. Facts are illustrative only.
pub struct TemplateOffer<'a> {
    pub id: TemplateId,
    pub revision: TemplateRevision,
    pub title: &'a str,
    pub description: &'a str,
    pub source: &'a str,
    pub facts: &'a CatalogueFacts,
    pub dependencies: &'a BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
}

pub struct WorkflowTemplate {
    id: TemplateId,
    revision: TemplateRevision,
    title: String,
    description: String,
    source: String,
    source_identity: SourceIdentity,
    validation_identity: TemplateValidationIdentity,
}
/// Compiler-produced identity of the offered source plus its illustrative facts.
/// This is never the compiled identity of a rebased company draft.
pub struct TemplateValidationIdentity(String);
impl TemplateValidationIdentity {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl WorkflowTemplate {
    pub fn id(&self) -> &TemplateId {
        &self.id
    }
    pub fn revision(&self) -> TemplateRevision {
        self.revision
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn source_identity(&self) -> &SourceIdentity {
        &self.source_identity
    }
    pub fn validation_identity(&self) -> &TemplateValidationIdentity {
        &self.validation_identity
    }
}

/// Composition-time immutable snapshot. Future operator ingress must enforce
/// account/config operator policy before building it. Distribution is not wired.
/// Replacing this catalogue never changes previously copied company drafts.
pub struct TemplateCatalogue {
    entries: Vec<WorkflowTemplate>,
}
impl TemplateCatalogue {
    pub fn build(
        offers: &[TemplateOffer<'_>],
        decoder: &impl SourceDecoder,
    ) -> Result<Self, TemplateError> {
        preflight(offers)?;
        let mut entries = Vec::with_capacity(offers.len());
        for offer in offers {
            let compiled = registry::compile(
                decoder.decode(offer.source)?,
                VersionId::new(uuid::Uuid::nil()),
                offer.facts,
                offer.dependencies,
            )?;
            if compiled.graph().definition().workflow_id.as_uuid().is_nil() {
                return Err(crate::application::workflow::compiler::Diagnostic::at(
                    "template.identity",
                    "Template root workflow identity must be non-nil",
                    "/workflow_id",
                    compiled.source_map()["/workflow_id"],
                )
                .into());
            }
            entries.push(WorkflowTemplate {
                id: offer.id.clone(),
                revision: offer.revision,
                title: offer.title.into(),
                description: offer.description.into(),
                source: offer.source.into(),
                source_identity: SourceIdentity::of(offer.source),
                validation_identity: TemplateValidationIdentity(compiled.content_hash().into()),
            });
        }
        Ok(Self { entries })
    }
    pub fn entries(&self) -> &[WorkflowTemplate] {
        &self.entries
    }
    pub(super) fn selected(
        &self,
        id: &TemplateId,
        revision: TemplateRevision,
    ) -> Result<&WorkflowTemplate, TemplateError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.id == *id)
            .ok_or(TemplateError::Missing)?;
        if entry.revision != revision {
            return Err(TemplateError::StaleRevision);
        }
        Ok(entry)
    }
}
fn preflight(offers: &[TemplateOffer<'_>]) -> Result<(), TemplateError> {
    if offers.len() > MAX_TEMPLATES {
        return Err(TemplateError::Limit("template count"));
    }
    let mut remaining = MAX_CATALOGUE_BYTES;
    let mut ids = BTreeSet::new();
    for offer in offers {
        if offer.title.is_empty() || offer.title.len() > MAX_TITLE_BYTES {
            return Err(TemplateError::Limit("template title bytes"));
        }
        if offer.description.len() > MAX_DESCRIPTION_BYTES {
            return Err(TemplateError::Limit("template description bytes"));
        }
        if offer.source.len() > MAX_SOURCE_BYTES {
            return Err(TemplateError::Limit("template source bytes"));
        }
        for size in [
            offer.id.as_str().len(),
            std::mem::size_of::<u64>(),
            offer.title.len(),
            offer.description.len(),
            offer.source.len(),
        ] {
            remaining = remaining
                .checked_sub(size)
                .ok_or(TemplateError::Limit("aggregate catalogue bytes"))?;
        }
        if !ids.insert(&offer.id) {
            return Err(TemplateError::Duplicate(offer.id.clone()));
        }
    }
    Ok(())
}
