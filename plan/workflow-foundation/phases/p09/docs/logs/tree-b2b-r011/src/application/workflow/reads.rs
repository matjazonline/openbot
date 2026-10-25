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

/// One immutable published version's source, for the editor's compare (p09 B1a).
#[derive(Debug, Clone, PartialEq)]
pub struct VersionSource {
    pub version: VersionId,
    pub draft_revision: u64,
    pub title: String,
    pub source: String,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleUse {
    pub schedule: Uuid,
    pub name: String,
    pub enabled: bool,
}

/// A binding of the workflow with what it reaches: its channel's name and the schedules that
/// start runs through it.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingUse {
    pub binding: BindingSummary,
    pub workflow_title: String,
    pub channel_name: Option<String>,
    /// The current configuration's resource per slot, as stored.
    pub resources: std::collections::BTreeMap<String, Uuid>,
    /// At most [`MAX_SCHEDULES_PER_BINDING`], by name.
    pub schedules: Vec<ScheduleUse>,
    pub schedules_truncated: bool,
}

pub const MAX_SCHEDULES_PER_BINDING: i64 = 20;

/// Every binding of one workflow, at most [`MAX_LISTED`], oldest first. The route still drops
/// the ones on channels the caller cannot see.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkflowUsage {
    pub bindings: Vec<BindingUse>,
    pub truncated: bool,
}

/// An approved operation a publication selection may name: the frozen snapshot publication
/// would capture, from a connection the company owns. Connection labels are the MCP name or
/// the HTTP origin; never an endpoint path, header or credential.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationOption {
    pub connection: Uuid,
    pub connection_label: String,
    pub connection_enabled: bool,
    pub name: String,
    /// The stored `ToolSnapshot` (contract and policy).
    pub snapshot: Value,
    pub approval_required: bool,
}

/// The most connections listed for one needed tool name.
pub const MAX_CONNECTIONS_PER_TOOL: i64 = 20;
/// The most tool names one options read considers (a draft's literal tool names, deduplicated).
pub const MAX_OPTION_NAMES: usize = 256;

/// The connections offering each tool a draft needs, at most [`MAX_CONNECTIONS_PER_TOOL`] per
/// name. Reading by the needed names first keeps the bound from hiding a needed tool.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PublicationOptions {
    pub mcp: Vec<OperationOption>,
    pub http: Vec<OperationOption>,
    /// The MCP tool names whose connection list was cut at the bound.
    pub mcp_cut: std::collections::BTreeSet<String>,
    /// The HTTP operation names whose connection list was cut at the bound.
    pub http_cut: std::collections::BTreeSet<String>,
}

/// The most bindings the agent pane lists as using an agent.
pub const MAX_AGENT_USES: i64 = 50;

/// The most of each kind of choice a binding form offers (channels, connections, people).
pub const MAX_BINDING_OPTIONS: i64 = 200;

/// A channel, connection or person a binding form may offer. Connection labels are the MCP
/// name or the HTTP origin; never an endpoint path, header or credential.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceOption {
    pub id: Uuid,
    pub label: String,
    /// A connection's `enabled`; a person's role is in `detail`.
    pub enabled: bool,
    pub detail: String,
}

/// What a binding form chooses from, each list at most [`MAX_BINDING_OPTIONS`] by label. The
/// route still drops the channels the caller cannot see; the command checks every choice.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BindingOptions {
    pub channels: Vec<ChoiceOption>,
    pub http: Vec<ChoiceOption>,
    pub mcp: Vec<ChoiceOption>,
    /// Owners and admins (the run-as candidates) first, then members, by name.
    pub people: Vec<ChoiceOption>,
    /// Which lists were cut at the bound.
    pub truncated: bool,
}

/// The version a workflow tool registration pins, as the tools page names it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolTarget {
    pub version: VersionId,
    pub workflow: WorkflowId,
    pub workflow_title: String,
    pub draft_revision: u64,
    pub archived: bool,
}

/// The authoring pages' reads (p09 B1a, B1b); company-scoped, lock-free, granting nothing.
#[async_trait]
pub trait AuthoringReads: Send + Sync {
    async fn version_source(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
        version: VersionId,
    ) -> AppResult<Option<VersionSource>>;
    async fn workflow_usage(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
    ) -> AppResult<WorkflowUsage>;
    /// The approved MCP operations named `mcp_tools` and HTTP operations named `http_tools`;
    /// more than [`MAX_OPTION_NAMES`] names in either list is refused.
    async fn publication_options(
        &self,
        company: CompanyId,
        mcp_tools: &[String],
        http_tools: &[String],
    ) -> AppResult<PublicationOptions>;
    /// One binding of the company with what it reaches, whatever the listings' bounds.
    async fn binding_use(&self, company: CompanyId, binding: Uuid)
    -> AppResult<Option<BindingUse>>;
    /// Every binding on `channel` (and on its threads), at most [`MAX_LISTED`], oldest first.
    async fn channel_usage(&self, company: CompanyId, channel: Uuid) -> AppResult<WorkflowUsage>;
    /// The bindings whose bound version froze `agent`, directly or through a pinned child, at
    /// most [`MAX_AGENT_USES`] by binding id (`truncated` says more exist). Reads the version's
    /// `agent_ids` display index (p09 gate P09-B1b-D1).
    async fn agent_usage(&self, company: CompanyId, agent: Uuid) -> AppResult<WorkflowUsage>;
    async fn binding_options(&self, company: CompanyId) -> AppResult<BindingOptions>;
    /// The agent's ordered workflow tool grants; `None` when the company has no such agent.
    async fn agent_tool_grants(
        &self,
        company: CompanyId,
        agent: Uuid,
    ) -> AppResult<Option<Vec<String>>>;
    /// The pinned versions among `versions` that this company holds.
    async fn tool_targets(
        &self,
        company: CompanyId,
        versions: &[VersionId],
    ) -> AppResult<Vec<ToolTarget>>;
}
