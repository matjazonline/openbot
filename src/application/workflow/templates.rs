//! Immutable operator starter catalogue and authorized independent draft copies.
//! No production distribution, draft storage, publication or resource grants.
mod catalogue;
mod contracts;
pub use catalogue::*;
pub use contracts::*;

use super::compiler::{SourceDecoder, rebase_workflow_id};
use super::{RelatedAssociation, WorkflowAuthorization, WorkflowOperation};
use crate::application::app_error::AppError;
use async_trait::async_trait;

/// Atomically insert the entire draft, never upsert. Implementations must check
/// current owner/admin membership for the included actor and company in the same
/// durable transaction, returning NotFound on revocation and Conflict on any
/// destination identity collision. No partial source/metadata/origin writes.
/// Phase 03 supplies production storage; there is no default implementation.
#[async_trait]
pub trait WorkflowDraftCopies: Send + Sync {
    async fn insert_copy(&self, copy: PreparedTemplateCopy) -> Result<(), AppError>;
}

pub struct TemplateCopyService<A, P, D> {
    authorization: A,
    persistence: P,
    decoder: D,
}
impl<A, P, D> TemplateCopyService<A, P, D>
where
    A: WorkflowAuthorization,
    P: WorkflowDraftCopies,
    D: SourceDecoder,
{
    pub fn new(authorization: A, persistence: P, decoder: D) -> Self {
        Self {
            authorization,
            persistence,
            decoder,
        }
    }

    pub async fn copy(
        &self,
        catalogue: &TemplateCatalogue,
        request: CopyTemplateRequest,
    ) -> Result<CompanyWorkflowDraft, TemplateError> {
        self.authorization
            .authorize(
                request.company,
                request.actor,
                RelatedAssociation::Company,
                WorkflowOperation::CopyTemplate,
            )
            .await?;
        let template = catalogue.selected(&request.template, request.revision)?;
        let source = rebase_workflow_id(&self.decoder, template.source(), request.workflow)?;
        let draft = CompanyWorkflowDraft::copied(&request, template, source);
        self.persistence
            .insert_copy(PreparedTemplateCopy::new(request.actor, draft.clone()))
            .await?;
        Ok(draft)
    }
}

#[cfg(test)]
#[path = "templates/tests.rs"]
mod tests;
