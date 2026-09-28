use super::catalogue::WorkflowTemplate;
use crate::application::app_error::AppError;
use crate::application::workflow::{WorkflowActor, compiler::Diagnostic};
use crate::domain::workflow::{CompanyId, DraftRevision, TemplateId, TemplateRevision, WorkflowId};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum TemplateError {
    Application(AppError),
    Validation(Box<Diagnostic>),
    Limit(&'static str),
    Duplicate(TemplateId),
    Missing,
    StaleRevision,
}
impl From<AppError> for TemplateError {
    fn from(value: AppError) -> Self {
        Self::Application(value)
    }
}
impl From<Diagnostic> for TemplateError {
    fn from(value: Diagnostic) -> Self {
        Self::Validation(Box::new(value))
    }
}

/// Identity of exact source bytes, never authority or executable compiled identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentity(String);
impl SourceIdentity {
    pub(super) fn of(source: &str) -> Self {
        Self(format!("{:x}", Sha256::digest(source.as_bytes())))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateOrigin {
    template: TemplateId,
    revision: TemplateRevision,
    source_identity: SourceIdentity,
}
impl TemplateOrigin {
    /// Restore stored provenance without treating it as template authority.
    pub(crate) fn restore(
        template: TemplateId,
        revision: TemplateRevision,
        source_identity: String,
    ) -> Result<Self, AppError> {
        if source_identity.len() != 64
            || !source_identity
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(AppError::Database(
                "Invalid workflow template provenance".into(),
            ));
        }
        Ok(Self {
            template,
            revision,
            source_identity: SourceIdentity(source_identity),
        })
    }
    pub fn template(&self) -> &TemplateId {
        &self.template
    }
    pub fn revision(&self) -> TemplateRevision {
        self.revision
    }
    pub fn source_identity(&self) -> &SourceIdentity {
        &self.source_identity
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyWorkflowDraft {
    company: CompanyId,
    workflow: WorkflowId,
    revision: DraftRevision,
    title: String,
    description: String,
    source: String,
    source_identity: SourceIdentity,
    origin: Option<TemplateOrigin>,
}
impl CompanyWorkflowDraft {
    pub(super) fn copied(
        request: &CopyTemplateRequest,
        template: &WorkflowTemplate,
        source: String,
    ) -> Self {
        Self {
            company: request.company,
            workflow: request.workflow,
            revision: DraftRevision::new(1).expect("initial draft revision"),
            title: template.title().into(),
            description: template.description().into(),
            source_identity: SourceIdentity::of(&source),
            source,
            origin: Some(TemplateOrigin {
                template: template.id().clone(),
                revision: template.revision(),
                source_identity: template.source_identity().clone(),
            }),
        }
    }
    pub fn company(&self) -> CompanyId {
        self.company
    }
    pub fn workflow(&self) -> WorkflowId {
        self.workflow
    }
    pub fn revision(&self) -> DraftRevision {
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
    pub fn origin(&self) -> Option<&TemplateOrigin> {
        self.origin.as_ref()
    }

    /// Bounded source is retained even when it is invalid YAML. Validation is a
    /// separate command; template provenance survives edits without being updated.
    pub(crate) fn saved(
        company: CompanyId,
        workflow: WorkflowId,
        revision: DraftRevision,
        content: super::super::lifecycle::DraftContent,
        origin: Option<TemplateOrigin>,
    ) -> Self {
        Self {
            company,
            workflow,
            revision,
            title: content.title,
            description: content.description,
            source_identity: SourceIdentity::of(&content.source),
            source: content.source,
            origin,
        }
    }
}

pub struct CopyTemplateRequest {
    pub(super) company: CompanyId,
    pub(super) actor: WorkflowActor,
    pub(super) template: TemplateId,
    pub(super) revision: TemplateRevision,
    pub(super) workflow: WorkflowId,
}
impl CopyTemplateRequest {
    /// The ingress allocates a fresh workflow UUID. Persistence must reject any
    /// collision; rebasing additionally rejects reuse of the template root ID.
    pub fn new(
        company: CompanyId,
        actor: WorkflowActor,
        template: TemplateId,
        revision: TemplateRevision,
        workflow: WorkflowId,
    ) -> Result<Self, AppError> {
        if company.as_uuid().is_nil() || workflow.as_uuid().is_nil() {
            return Err(AppError::BadRequest(
                "non-nil company and workflow identities required".into(),
            ));
        }
        Ok(Self {
            company,
            actor,
            template,
            revision,
            workflow,
        })
    }
}

/// Only authorized copy orchestration can prepare this complete owned insert.
pub struct PreparedTemplateCopy {
    actor: WorkflowActor,
    draft: CompanyWorkflowDraft,
}
impl PreparedTemplateCopy {
    pub(super) fn new(actor: WorkflowActor, draft: CompanyWorkflowDraft) -> Self {
        Self { actor, draft }
    }
    pub fn actor(&self) -> WorkflowActor {
        self.actor
    }
    pub fn draft(&self) -> &CompanyWorkflowDraft {
        &self.draft
    }
    pub fn into_draft(self) -> CompanyWorkflowDraft {
        self.draft
    }
}
