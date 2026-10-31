//! Read models behind the workflow HTTP routes (p08 B3b-1). Every read is company-scoped, and a
//! read grants nothing: the route authorizes the company, and the record's own association,
//! before it shows a record.
use super::RelatedAssociation;
use super::board::{BoardCount, RunBoardScope, VisibleChannels};
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
    /// When the run reached its terminal state; `None` while it is open. Stamped by the database
    /// (`stamp_workflow_run_finished`), kept while terminal, cleared by a retry.
    pub finished_at: Option<DateTime<Utc>>,
}

/// The six values `workflow_runs.state` stores, in the run board's column order. One list for the
/// board, the child rollup, the slow-read logs and the list's state filter; the database's own
/// terminal set (`workflow_run_finished_shape`, `stamp_workflow_run_finished`) is pinned to
/// [`RunStateName::is_open`] by a database test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunStateName {
    Queued,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl RunStateName {
    pub const ALL: [Self; 6] = [
        Self::Queued,
        Self::Running,
        Self::Waiting,
        Self::Succeeded,
        Self::Failed,
        Self::Cancelled,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// The state a stored or requested name names; `None` for anything else.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == name)
    }

    /// Queued, running or waiting: not yet finished, so it has no `finished_at`.
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Waiting)
    }
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

/// Which runs of a lineage a read includes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunLineage {
    /// Every run, children included: the run list, the forensic feed, which labels each child
    /// with its parent.
    #[default]
    Any,
    /// Runs no other run started (`parent_run_id IS NULL`); a rerun is one. The run board reads
    /// only these, so a parent and the child it started are one piece of work, and the parent's
    /// card carries its children's counts instead ([`WorkflowReads::child_rollup`]).
    TopLevel,
}

#[derive(Debug, Clone, Default)]
pub struct RunFilter {
    pub binding: Option<WorkflowBindingId>,
    /// Runs of one workflow, any of its versions. The route resolves the id company-scoped (a
    /// definition of the company) before it gets here.
    pub workflow: Option<WorkflowId>,
    pub state: Option<String>,
    /// Runs of versions that *use* this agent: its id is in the version's `agent_ids` display
    /// index, directly or through a pinned child, including branches a run never took. Not the
    /// agents a run actually called: `model_usage_calls.agent_id` exists only once a model call
    /// has happened, so a queued run would never match. The route resolves the id
    /// company-scoped before it gets here.
    pub agent: Option<Uuid>,
    pub lineage: RunLineage,
}

/// The most parents one [`WorkflowReads::child_rollup`] takes: six board columns of 50 cards
/// (`fix-03-run-board-finished-columns` defines the column cap and keeps the two in step).
pub const MAX_ROLLUP_PARENTS: usize = 300;

/// A parent run's direct children, counted by state. Grandchildren are not counted: they belong
/// to their own parent, and the child's pane lists them. A child carries its parent's channel
/// association (`children::create_on` copies it, and associations never change), so whoever may
/// see the parent may see these counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChildCounts {
    pub total: u64,
    /// Queued, running or waiting.
    pub open: u64,
    pub failed: u64,
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
    /// The board's working set — `scope`'s top-level runs that are open, or finished at or after
    /// `cutoff` — counted by state and channel, hidden channels included: the caller decides
    /// which it may show before it reads any card.
    async fn run_board_counts(
        &self,
        company: CompanyId,
        scope: &RunBoardScope,
        cutoff: DateTime<Utc>,
    ) -> AppResult<Vec<BoardCount>>;
    /// Up to `BOARD_COLUMN_CAP + 1` cards of each state, of `scope`'s top-level runs on `visible`
    /// channels only: open states oldest first, finished states finished at or after `cutoff`,
    /// most recently finished first.
    async fn run_board_cards(
        &self,
        company: CompanyId,
        scope: &RunBoardScope,
        cutoff: DateTime<Utc>,
        visible: &VisibleChannels,
    ) -> AppResult<Vec<RunSummary>>;
    /// The direct children of each of `parents` that has any, counted by state, in one read.
    /// More than [`MAX_ROLLUP_PARENTS`] parents is refused rather than cut.
    async fn child_rollup(
        &self,
        company: CompanyId,
        parents: &[RunId],
    ) -> AppResult<std::collections::HashMap<RunId, ChildCounts>>;
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
    /// `agent_ids` display index (p09 gate P09-B1b-D1), as the run list's agent filter
    /// ([`RunFilter::agent`]) does; nothing decides or authorizes on it.
    async fn agent_usage(&self, company: CompanyId, agent: Uuid) -> AppResult<WorkflowUsage>;
    async fn binding_options(&self, company: CompanyId) -> AppResult<BindingOptions>;
    /// The company's agents by name, at most [`MAX_BINDING_OPTIONS`], and whether there were more:
    /// what the runs workspace's agent filter offers.
    async fn agent_options(&self, company: CompanyId) -> AppResult<(Vec<ChoiceOption>, bool)>;
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
