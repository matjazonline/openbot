use super::*;
use crate::application::workflow::templates::{MAX_DESCRIPTION_BYTES, MAX_TITLE_BYTES};

pub type LifecycleResult<T> = Result<T, LifecycleError>;

#[derive(Debug)]
pub enum LifecycleError {
    Application(AppError),
    Validation(Box<compiler::Diagnostic>),
}
impl From<AppError> for LifecycleError {
    fn from(error: AppError) -> Self {
        Self::Application(error)
    }
}
impl From<compiler::Diagnostic> for LifecycleError {
    fn from(error: compiler::Diagnostic) -> Self {
        Self::Validation(Box::new(error))
    }
}

pub struct DraftContent {
    pub title: String,
    pub description: String,
    pub source: String,
}
impl DraftContent {
    pub(super) fn check(&self) -> AppResult<()> {
        if self.title.trim().is_empty()
            || self.title.len() > MAX_TITLE_BYTES
            || self.description.len() > MAX_DESCRIPTION_BYTES
            || self.source.len() > compiler::MAX_SOURCE_BYTES
        {
            return Err(AppError::BadRequest(
                "Workflow draft content exceeds bounds or has an empty title".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct DraftState {
    pub draft: CompanyWorkflowDraft,
    pub archived: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefinitionTarget {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub workflow: WorkflowId,
}

pub struct SaveDraft {
    pub target: DefinitionTarget,
    /// None means insert only. Existing drafts require their exact revision.
    pub expected: Option<DraftRevision>,
    pub content: DraftContent,
}

/// Publication command equivalence excludes the current dependency catalogue:
/// successful retries return the original captured content after reauthorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishDraft {
    pub target: DefinitionTarget,
    pub expected: DraftRevision,
    pub version: VersionId,
    pub key: IdempotencyKey,
}
impl PublishDraft {
    pub fn equivalent(&self, other: &Self) -> bool {
        self.target.company == other.target.company
            && self.target.workflow == other.target.workflow
            && self.expected == other.expected
            && self.version == other.version
            && self.key == other.key
    }
}

#[derive(Clone)]
pub struct PublishedCommand {
    pub request: PublishDraft,
    /// Exact source and authoring metadata captured at publication, independent
    /// of subsequent draft edits and archive state.
    pub draft: CompanyWorkflowDraft,
    pub bundle: Arc<PublishedBundle>,
}

pub struct PublicationDependencies {
    pub snapshots: publication::DependencySnapshots,
    pub children: Vec<Arc<PublishedBundle>>,
}

pub struct PreparedDraft {
    pub(super) actor: WorkflowActor,
    pub(super) expected: Option<DraftRevision>,
    pub(super) state: DraftState,
}
impl PreparedDraft {
    pub fn actor(&self) -> WorkflowActor {
        self.actor
    }
    pub fn expected(&self) -> Option<DraftRevision> {
        self.expected
    }
    pub fn state(&self) -> &DraftState {
        &self.state
    }
}

pub struct PreparedPublication {
    pub(super) command: PublishedCommand,
}
impl PreparedPublication {
    pub fn command(&self) -> &PublishedCommand {
        &self.command
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindingTarget {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub binding: WorkflowBindingId,
}

#[derive(Clone)]
pub struct BindingState {
    pub configuration: Arc<ConfiguredBinding>,
    /// Fixed on creation: changing the associated resource requires a new binding.
    pub association: RelatedAssociation,
    pub revision: BindingStateRevision,
    pub active: bool,
}

pub struct ConfigureBinding {
    pub target: BindingTarget,
    pub expected: Option<BindingStateRevision>,
    pub association: RelatedAssociation,
    pub version: VersionId,
    pub params: serde_json::Value,
    pub resources: std::collections::BTreeMap<ResourceName, RuntimeResourceId>,
}

#[derive(Debug, Clone, Copy)]
pub struct SetBindingActivity {
    pub target: BindingTarget,
    pub expected: BindingStateRevision,
}

pub struct PreparedBinding {
    pub(super) actor: WorkflowActor,
    pub(super) expected: Option<BindingStateRevision>,
    pub(super) state: BindingState,
}
impl PreparedBinding {
    pub fn actor(&self) -> WorkflowActor {
        self.actor
    }
    pub fn expected(&self) -> Option<BindingStateRevision> {
        self.expected
    }
    pub fn state(&self) -> &BindingState {
        &self.state
    }
}

pub(super) fn conflict() -> AppError {
    AppError::Conflict("Workflow lifecycle revision or command conflict".into())
}
pub(super) fn missing() -> AppError {
    AppError::NotFound("Workflow lifecycle resource".into())
}
pub(super) fn next(value: u64) -> AppResult<u64> {
    value.checked_add(1).ok_or_else(conflict)
}
