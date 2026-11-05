//! What the run pages read (p09 B2a): one run with its lineage, children, controls and
//! history, and one step of it with its frozen inputs, output, model calls, receipts and worker
//! attempts. Routes authorize company/channel access before exposing a view. Bounded payload
//! reads finish before a fresh joint viewer/run-stamp statement and application projection.
use super::publication::PublishedBundle;
use super::reads::RunDetail;
use crate::app_error::AppResult;
use crate::domain::workflow::{CompanyId, RunId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

/// The most of each kind of record one step lists.
pub const MAX_STEP_ITEMS: i64 = 50;

/// Where a runtime child run came from (WF-C002): its parent run and the execution that
/// started it, the `flow.repeat` round, or the saved model call of a workflow tool (WF-C003).
#[derive(Debug, Clone, PartialEq)]
pub struct Lineage {
    pub parent: RunId,
    pub parent_execution: Uuid,
    pub parent_step: Option<String>,
    pub round: Option<i32>,
    pub tool_call: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChildRun {
    pub run: RunId,
    pub state: String,
    pub waiting_reason: Option<String>,
    pub execution: Uuid,
    pub step: Option<String>,
    pub round: Option<i32>,
    pub tool_call: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// A cancel or retry as the control command recorded it.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlRecord {
    pub operation: String,
    pub actor: Uuid,
    pub result: String,
    pub expected_revision: i64,
    pub result_revision: i64,
    pub created_at: DateTime<Utc>,
}

/// An action whose outcome reconciliation recorded (an annotation; nothing resumed from it).
#[derive(Debug, Clone, PartialEq)]
pub struct ReconciliationRecord {
    pub execution: Uuid,
    pub invocation: Uuid,
    pub dispatch: Uuid,
    pub effect_kind: String,
    pub failure_code: String,
    pub observed_at: DateTime<Utc>,
}

/// One entry of the run's audit history (`workflow_run_events`).
#[derive(Debug, Clone, PartialEq)]
pub struct RunEvent {
    pub sequence: i64,
    pub kind: String,
    pub actor: Uuid,
    pub execution: Option<Uuid>,
    pub decision: Option<Uuid>,
    pub terminal_state: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunView {
    pub payload_restricted: bool,
    pub detail: RunDetail,
    pub workflow_title: String,
    pub draft_revision: u64,
    pub channel_name: Option<String>,
    /// The inbound message whose admission started the run, if a message did.
    pub trigger_message: Option<Uuid>,
    pub input: Value,
    pub params: Value,
    pub lineage: Option<Lineage>,
    /// Runs started as reruns of this one, newest first.
    pub reruns: Vec<RunId>,
    pub reruns_truncated: bool,
    /// Newest first.
    pub children: Vec<ChildRun>,
    pub children_truncated: bool,
    /// Newest first.
    pub controls: Vec<ControlRecord>,
    pub controls_truncated: bool,
    /// Newest first.
    pub reconciliations: Vec<ReconciliationRecord>,
    pub reconciliations_truncated: bool,
    /// Newest first.
    pub events: Vec<RunEvent>,
    pub events_truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCall {
    pub provider: String,
    pub model: String,
    pub requested_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub usage_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    pub dispatch: Uuid,
    pub invocation: Uuid,
    pub effect_kind: String,
    pub result: Value,
    pub created_at: DateTime<Utc>,
}

/// One worker attempt at the step (diagnostics).
#[derive(Debug, Clone, PartialEq)]
pub struct Attempt {
    pub number: i32,
    pub status: String,
    pub stop_reason: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub failure_class: Option<String>,
    pub failure_code: Option<String>,
    pub retry_safety: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepView {
    pub payload_restricted: bool,
    pub execution: Uuid,
    pub step: String,
    pub activation: i64,
    pub created_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub frozen_inputs: Option<Value>,
    pub committed_output: Option<Value>,
    pub frozen_choice: Option<String>,
    pub committed_route: Option<String>,
    pub route_target: Option<String>,
    /// Oldest first, at most [`MAX_STEP_ITEMS`] each.
    pub model_calls: Vec<ModelCall>,
    pub receipts: Vec<Receipt>,
    pub attempts: Vec<Attempt>,
    pub children: Vec<ChildRun>,
    /// Any of the lists was cut at [`MAX_STEP_ITEMS`].
    pub truncated: bool,
}

#[async_trait]
pub trait RunViews: Send + Sync {
    /// Fresh joint viewer/stamp facts, after payload reads; never authorizes a later fetch.
    async fn run_visibility(
        &self,
        scope: RunReadScope,
    ) -> AppResult<Option<crate::application::knowledge::PayloadVisibility>>;
    async fn run_view(&self, scope: RunReadScope) -> AppResult<Option<RunView>>;
    async fn step_view(&self, scope: RunReadScope, execution: Uuid) -> AppResult<Option<StepView>>;
    /// The titles of these workflows of the company, by id (the run list names each run's).
    async fn workflow_titles(
        &self,
        company: CompanyId,
        workflows: &[Uuid],
    ) -> AppResult<std::collections::HashMap<Uuid, String>>;
    /// The bundle the run was admitted with: its graph, and each agent's frozen tools and skills.
    async fn run_bundle(
        &self,
        company: CompanyId,
        run: RunId,
    ) -> AppResult<Option<Arc<PublishedBundle>>>;
}

/// Resource identity and authenticated actor, never a caller-supplied level.
#[derive(Debug, Clone, Copy)]
pub struct RunReadScope {
    pub company: CompanyId,
    pub run: RunId,
    pub viewer: super::WorkflowActor,
}
impl RunView {
    pub fn project(&mut self, visibility: crate::application::knowledge::PayloadVisibility) {
        if visibility.is_visible() {
            return;
        }
        self.payload_restricted = true;
        self.input = Value::Null;
        self.params = Value::Null;
        self.detail.project(visibility);
    }
}
impl StepView {
    pub fn project(&mut self, visibility: crate::application::knowledge::PayloadVisibility) {
        if visibility.is_visible() {
            return;
        }
        self.payload_restricted = true;
        self.frozen_inputs = None;
        self.committed_output = None;
        self.frozen_choice = None;
        self.committed_route = None;
        self.route_target = None;
        self.model_calls.clear();
        self.receipts.clear();
        self.attempts.clear();
        self.truncated = false;
    }
}
