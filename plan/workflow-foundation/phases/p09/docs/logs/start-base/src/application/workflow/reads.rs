//! Read models behind the workflow HTTP routes (p08 B3b-1). Every read is company-scoped, and a
//! read grants nothing: the route authorizes the company, and the record's own association,
//! before it shows a record.
use super::RelatedAssociation;
use crate::app_error::AppResult;
use crate::domain::workflow::{
    CompanyId, MessageTrigger, RunId, VersionId, WorkflowBindingId, WorkflowId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

/// The most rows one list read returns, and the default page size.
pub const MAX_LISTED: i64 = 200;

/// One page of a listing: at most the requested size, newest or first-ordered first. `more` says
/// the store holds rows past this page, so a caller never mistakes a page for the whole listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub more: bool,
}

/// Where a page starts: after the row with this id (the last one of the previous page), and how
/// many rows it holds (1..=[`MAX_LISTED`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    pub after: Option<Uuid>,
    pub size: i64,
}
impl PageRequest {
    pub fn new(after: Option<Uuid>, size: Option<i64>) -> AppResult<Self> {
        let size = size.unwrap_or(MAX_LISTED);
        if !(1..=MAX_LISTED).contains(&size) {
            return Err(crate::app_error::AppError::BadRequest(format!(
                "A page holds 1 to {MAX_LISTED} rows"
            )));
        }
        Ok(Self { after, size })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionSummary {
    pub version: VersionId,
    pub draft_revision: u64,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionSummary {
    pub workflow: WorkflowId,
    pub revision: u64,
    pub title: String,
    pub archived: bool,
    /// The template a copy (or a provisioned assistant) started from, as stored.
    pub template_origin: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DefinitionDetail {
    pub summary: DefinitionSummary,
    pub description: String,
    pub source: String,
    /// Newest first, at most [`MAX_LISTED`].
    pub versions: Vec<VersionSummary>,
    pub versions_truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BindingSummary {
    pub binding: WorkflowBindingId,
    pub state_revision: u64,
    pub configuration_revision: u64,
    pub active: bool,
    pub association: RelatedAssociation,
    pub workflow: WorkflowId,
    pub version: VersionId,
    pub params: Value,
    pub run_as: Uuid,
    pub message_trigger: Option<MessageTrigger>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunSummary {
    pub run: RunId,
    pub binding: WorkflowBindingId,
    pub workflow: WorkflowId,
    pub version: VersionId,
    pub association: RelatedAssociation,
    pub state: String,
    pub waiting_reason: Option<String>,
    pub revision: u64,
    pub actor: Uuid,
    pub parent: Option<RunId>,
    pub rerun_of: Option<RunId>,
    pub created_at: DateTime<Utc>,
    pub deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionSummary {
    pub execution: Uuid,
    pub step: String,
    pub activation: i64,
    pub activated_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub route: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaitSummary {
    pub wait: Uuid,
    pub execution: Uuid,
    pub reason: String,
    pub state: String,
    pub deadline: DateTime<Utc>,
    /// The decision a `decision` wait parks on.
    pub decision: Option<Uuid>,
    /// The child run a `child_run` wait parks on.
    pub child_run: Option<RunId>,
}

/// One delivery-backed destination of a message action (P08-D3 §D3.4).
#[derive(Debug, Clone, PartialEq)]
pub struct DestinationSummary {
    pub ordinal: i16,
    pub delivery: Uuid,
    pub status: String,
    pub parts: i64,
}
impl DestinationSummary {
    /// A hint, not a grant: the command's own subject conditions (an `outcome_unknown`
    /// single-part delivery) decide.
    pub fn reconcilable(&self) -> bool {
        self.status == "outcome_unknown" && self.parts == 1
    }
}

/// A remote or message dispatch of the run, as a reconciliation command names it. Local
/// dispatches are left out: no reconciliation target names them.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionSummary {
    /// The dispatch id, the command's marker.
    pub marker: Uuid,
    pub execution: Uuid,
    pub invocation: Uuid,
    pub argument_digest: String,
    /// `remote` or `message`.
    pub effect_kind: String,
    pub attempt: i32,
    pub created_at: DateTime<Utc>,
    /// The invocation has its receipt: its outcome is settled.
    pub has_receipt: bool,
    /// The execution's evidence conflicts (`workflow_action_has_evidence_conflict`).
    pub evidence_conflict: bool,
    /// For a message action, its delivery-backed destinations in ordinal order.
    pub destinations: Vec<DestinationSummary>,
}
impl ActionSummary {
    /// A hint for a remote action: no receipt while the run waits on reconciliation. The
    /// command's snapshot and settlement checks decide.
    pub fn reconcilable(&self, run: &RunSummary) -> bool {
        self.effect_kind == "remote"
            && !self.has_receipt
            && run.state == "waiting"
            && run.waiting_reason.as_deref() == Some("reconciliation")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunDetail {
    pub summary: RunSummary,
    /// The newest [`MAX_LISTED`] activations, newest first.
    pub executions: Vec<ExecutionSummary>,
    pub executions_truncated: bool,
    /// The newest [`MAX_LISTED`] waits, newest first.
    pub waits: Vec<WaitSummary>,
    pub waits_truncated: bool,
    /// The newest [`MAX_LISTED`] remote and message dispatches, newest first.
    pub actions: Vec<ActionSummary>,
    pub actions_truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RunFilter {
    pub binding: Option<WorkflowBindingId>,
    pub state: Option<String>,
}

#[async_trait]
pub trait WorkflowReads: Send + Sync {
    /// By title, then id.
    async fn list_definitions(
        &self,
        company: CompanyId,
        page: PageRequest,
    ) -> AppResult<Page<DefinitionSummary>>;
    async fn definition(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
    ) -> AppResult<Option<DefinitionDetail>>;
    /// By id.
    async fn list_bindings(
        &self,
        company: CompanyId,
        page: PageRequest,
    ) -> AppResult<Page<BindingSummary>>;
    async fn binding_summary(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<BindingSummary>>;
    /// Newest first (creation time, then id).
    async fn list_runs(
        &self,
        company: CompanyId,
        filter: &RunFilter,
        page: PageRequest,
    ) -> AppResult<Page<RunSummary>>;
    async fn run_summary(&self, company: CompanyId, run: RunId) -> AppResult<Option<RunSummary>>;
    async fn run_detail(&self, company: CompanyId, run: RunId) -> AppResult<Option<RunDetail>>;
    /// The company a decision belongs to, for the notice link that names only the decision.
    async fn decision_company(&self, decision: Uuid) -> AppResult<Option<CompanyId>>;
}
