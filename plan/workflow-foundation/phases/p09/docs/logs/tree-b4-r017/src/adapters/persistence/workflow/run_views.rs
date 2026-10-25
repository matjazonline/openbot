//! The run pages' reads (p09 B2a): plain company-scoped reads with no locks, each list bounded
//! with one row past the bound to flag the cut. A route authorizes before it shows any of them.
use super::reads::{bounded, revision_of};
use super::*;
use crate::application::workflow::publication::{PublishedBundle, restore_bundle};
use crate::application::workflow::reads::{MAX_LISTED, WorkflowReads};
use crate::application::workflow::run_views::*;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// A list cut at [`MAX_STEP_ITEMS`], and whether it was.
fn step_bounded<T>(mut rows: Vec<T>) -> (Vec<T>, bool) {
    let cut = rows.len() as i64 > MAX_STEP_ITEMS;
    rows.truncate(MAX_STEP_ITEMS as usize);
    (rows, cut)
}

/// `rows` cut at the run lists' bound, converted, and whether they were cut.
fn listed<R, T: From<R>>(rows: Vec<R>) -> (Vec<T>, bool) {
    let (rows, cut) = bounded(rows);
    (rows.into_iter().map(T::from).collect(), cut)
}

/// `rows` cut at [`MAX_STEP_ITEMS`], converted, and whether they were cut.
fn step_listed<R, T: From<R>>(rows: Vec<R>) -> (Vec<T>, bool) {
    let (rows, cut) = step_bounded(rows);
    (rows.into_iter().map(T::from).collect(), cut)
}

/// What the run's own row, its version and its channel say about it.
#[derive(sqlx::FromRow)]
struct FactsRow {
    title: String,
    draft_revision: i64,
    channel_name: Option<String>,
    input: Value,
    params: Value,
}

#[derive(sqlx::FromRow)]
struct LineageRow {
    parent_execution_id: Uuid,
    step_id: Option<String>,
    child_ordinal: Option<i32>,
    child_tool_call_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
struct ChildRow {
    id: Uuid,
    state: String,
    waiting_reason: Option<String>,
    parent_execution_id: Uuid,
    step_id: Option<String>,
    child_ordinal: Option<i32>,
    child_tool_call_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl From<ChildRow> for ChildRun {
    fn from(row: ChildRow) -> Self {
        Self {
            run: RunId::new(row.id),
            state: row.state,
            waiting_reason: row.waiting_reason,
            execution: row.parent_execution_id,
            step: row.step_id,
            round: row.child_ordinal,
            tool_call: row.child_tool_call_id,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ControlRow {
    operation: String,
    actor_id: Uuid,
    result: String,
    expected_revision: i64,
    result_revision: i64,
    created_at: DateTime<Utc>,
}

impl From<ControlRow> for ControlRecord {
    fn from(row: ControlRow) -> Self {
        Self {
            operation: row.operation,
            actor: row.actor_id,
            result: row.result,
            expected_revision: row.expected_revision,
            result_revision: row.result_revision,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ReconciliationRow {
    execution_id: Uuid,
    invocation_id: Uuid,
    dispatch_id: Uuid,
    effect_kind: String,
    failure_code: String,
    observed_at: DateTime<Utc>,
}

impl From<ReconciliationRow> for ReconciliationRecord {
    fn from(row: ReconciliationRow) -> Self {
        Self {
            execution: row.execution_id,
            invocation: row.invocation_id,
            dispatch: row.dispatch_id,
            effect_kind: row.effect_kind,
            failure_code: row.failure_code,
            observed_at: row.observed_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct EventRow {
    sequence: i64,
    event_kind: String,
    actor_id: Uuid,
    execution_id: Option<Uuid>,
    decision_id: Option<Uuid>,
    terminal_state: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<EventRow> for RunEvent {
    fn from(row: EventRow) -> Self {
        Self {
            sequence: row.sequence,
            kind: row.event_kind,
            actor: row.actor_id,
            execution: row.execution_id,
            decision: row.decision_id,
            terminal_state: row.terminal_state,
            created_at: row.created_at,
        }
    }
}

/// One execution's own row, before its calls, receipts, attempts and children.
#[derive(sqlx::FromRow)]
struct StepRow {
    step_id: String,
    activation: i64,
    created_at: DateTime<Utc>,
    activated_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    frozen_inputs: Option<Value>,
    committed_output: Option<Value>,
    frozen_choice: Option<String>,
    committed_route: Option<String>,
    route_target: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ModelCallRow {
    provider: String,
    model: String,
    requested_at: DateTime<Utc>,
    settled_at: Option<DateTime<Utc>>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    usage_source: Option<String>,
}

impl From<ModelCallRow> for ModelCall {
    fn from(row: ModelCallRow) -> Self {
        Self {
            provider: row.provider,
            model: row.model,
            requested_at: row.requested_at,
            settled_at: row.settled_at,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            usage_source: row.usage_source,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ReceiptRow {
    dispatch_id: Uuid,
    invocation_id: Uuid,
    effect_kind: String,
    result: Value,
    created_at: DateTime<Utc>,
}

impl From<ReceiptRow> for Receipt {
    fn from(row: ReceiptRow) -> Self {
        Self {
            dispatch: row.dispatch_id,
            invocation: row.invocation_id,
            effect_kind: row.effect_kind,
            result: row.result,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct AttemptRow {
    attempt_number: i32,
    status: String,
    stop_reason: Option<String>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    workflow_failure_class: Option<String>,
    workflow_failure_code: Option<String>,
    workflow_retry_safety: Option<String>,
}

impl From<AttemptRow> for Attempt {
    fn from(row: AttemptRow) -> Self {
        Self {
            number: row.attempt_number,
            status: row.status,
            stop_reason: row.stop_reason,
            started_at: row.started_at,
            finished_at: row.finished_at,
            failure_class: row.workflow_failure_class,
            failure_code: row.workflow_failure_code,
            retry_safety: row.workflow_retry_safety,
        }
    }
}

/// Children of `run` (all, or those `execution` started), newest first, with the parent
/// wait's round and tool call.
const CHILDREN_SQL: &str = "SELECT child.id, child.state, child.waiting_reason, \
        link.parent_execution_id, execution.step_id, wait.child_ordinal, wait.child_tool_call_id, \
        child.created_at \
   FROM workflow_run_parents AS link \
   JOIN workflow_runs AS child ON child.company_id = link.company_id AND child.id = link.child_run_id \
   LEFT JOIN workflow_executions AS execution ON execution.company_id = link.company_id \
    AND execution.id = link.parent_execution_id \
   LEFT JOIN workflow_waits AS wait ON wait.company_id = link.company_id \
    AND wait.run_id = link.parent_run_id AND wait.child_run_id = link.child_run_id \
  WHERE link.company_id = $1 AND link.parent_run_id = $2 \
    AND ($3::uuid IS NULL OR link.parent_execution_id = $3) \
  ORDER BY child.created_at DESC, child.id DESC LIMIT $4";

/// The run's newest controls, reconciliations and history (`$1` company, `$2` run, `$3` limit).
const CONTROLS_SQL: &str = "SELECT operation, actor_id, result, expected_revision, \
        result_revision, created_at \
   FROM workflow_control_commands WHERE company_id = $1 AND run_id = $2 \
  ORDER BY created_at DESC, command_key DESC LIMIT $3";
/// Reconciliations and receipts are keyed by invocation; the run-led dispatch key reaches them
/// without walking the company's rows.
const RECONCILIATIONS_SQL: &str = "SELECT reconciliation.execution_id, \
        reconciliation.invocation_id, reconciliation.dispatch_id, reconciliation.effect_kind, \
        reconciliation.failure_code, reconciliation.observed_at \
   FROM workflow_action_dispatches AS dispatch \
   JOIN workflow_action_reconciliations AS reconciliation \
     ON reconciliation.company_id = dispatch.company_id \
    AND reconciliation.invocation_id = dispatch.invocation_id \
  WHERE dispatch.company_id = $1 AND dispatch.run_id = $2 \
  ORDER BY reconciliation.observed_at DESC, reconciliation.dispatch_id DESC LIMIT $3";
const EVENTS_SQL: &str = "SELECT sequence, event_kind, actor_id, execution_id, decision_id, \
        terminal_state, created_at \
   FROM workflow_run_events WHERE company_id = $1 AND run_id = $2 \
  ORDER BY sequence DESC LIMIT $3";

/// One execution's model calls, receipts and worker attempts, oldest first (`$1` company, `$2`
/// run, `$3` execution, `$4` limit).
const MODEL_CALLS_SQL: &str = "SELECT provider, model, requested_at, settled_at, input_tokens, \
        output_tokens, usage_source \
   FROM model_usage_calls WHERE company_id = $1 AND run_id = $2 AND execution_id = $3 \
  ORDER BY requested_at, request_id LIMIT $4";
const RECEIPTS_SQL: &str = "SELECT receipt.dispatch_id, receipt.invocation_id, \
        receipt.effect_kind, receipt.result, receipt.created_at \
   FROM workflow_action_dispatches AS dispatch \
   JOIN workflow_action_receipts AS receipt ON receipt.company_id = dispatch.company_id \
    AND receipt.invocation_id = dispatch.invocation_id \
  WHERE dispatch.company_id = $1 AND dispatch.run_id = $2 AND dispatch.execution_id = $3 \
  ORDER BY receipt.created_at, receipt.dispatch_id LIMIT $4";
/// Only a leased step has attempts: a pure step completes without a worker claiming it.
const ATTEMPTS_SQL: &str = "SELECT attempt.attempt_number, attempt.status, attempt.stop_reason, \
        attempt.started_at, attempt.finished_at, attempt.workflow_failure_class, \
        attempt.workflow_failure_code, attempt.workflow_retry_safety \
   FROM background_tasks AS task \
   JOIN workflow_executions AS execution ON execution.company_id = task.company_id \
    AND execution.id = task.workflow_execution_id \
   JOIN task_attempts AS attempt ON attempt.task_id = task.id \
  WHERE task.company_id = $1 AND execution.run_id = $2 AND task.workflow_execution_id = $3 \
  ORDER BY attempt.started_at, attempt.attempt_number LIMIT $4";

impl PostgresPersistence {
    async fn children(
        &self,
        company: CompanyId,
        run: RunId,
        execution: Option<Uuid>,
        limit: i64,
    ) -> AppResult<Vec<ChildRun>> {
        let rows: Vec<ChildRow> = sqlx::query_as(CHILDREN_SQL)
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(execution)
            .bind(limit + 1)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.into_iter().map(ChildRun::from).collect())
    }

    /// Up to [`MAX_LISTED`] + 1 rows of one of the run's lists.
    async fn run_rows<R>(&self, sql: &str, company: CompanyId, run: RunId) -> AppResult<Vec<R>>
    where
        R: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        Ok(sqlx::query_as(sql)
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(MAX_LISTED + 1)
            .fetch_all(self.pool())
            .await?)
    }

    /// Up to [`MAX_STEP_ITEMS`] + 1 rows of one of an execution's lists.
    async fn step_rows<R>(
        &self,
        sql: &str,
        company: CompanyId,
        run: RunId,
        execution: Uuid,
    ) -> AppResult<Vec<R>>
    where
        R: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
    {
        Ok(sqlx::query_as(sql)
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(execution)
            .bind(MAX_STEP_ITEMS + 1)
            .fetch_all(self.pool())
            .await?)
    }

    async fn run_facts(&self, company: CompanyId, run: RunId) -> AppResult<FactsRow> {
        Ok(sqlx::query_as(
            "SELECT definition.title, version.draft_revision, channel.name AS channel_name, \
                    run.input, run.params \
               FROM workflow_runs AS run \
               JOIN workflow_versions AS version ON version.company_id = run.company_id \
                AND version.id = run.version_id \
               JOIN workflow_definitions AS definition ON definition.company_id = run.company_id \
                AND definition.id = run.workflow_id \
               LEFT JOIN channels AS channel ON channel.company_id = run.company_id \
                AND channel.id = run.channel_id \
              WHERE run.company_id = $1 AND run.id = $2",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .fetch_one(self.pool())
        .await?)
    }

    /// The message whose intent started the run, when one did.
    async fn trigger_message(&self, company: CompanyId, run: RunId) -> AppResult<Option<Uuid>> {
        Ok(sqlx::query_scalar(
            "SELECT intent.message_id FROM workflow_message_intent_bindings AS outcome \
               JOIN workflow_message_intents AS intent ON intent.company_id = outcome.company_id \
                AND intent.id = outcome.intent_id \
              WHERE outcome.company_id = $1 AND outcome.run_id = $2 LIMIT 1",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .fetch_optional(self.pool())
        .await?)
    }

    /// The runs started as reruns of `run`, newest first, one past the bound.
    async fn reruns(&self, company: CompanyId, run: RunId) -> AppResult<Vec<RunId>> {
        let rows: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workflow_runs WHERE company_id = $1 AND rerun_of_run_id = $2 \
              ORDER BY created_at DESC, id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.into_iter().map(RunId::new).collect())
    }

    /// Where a child run came from: its parent's step, round and tool call.
    async fn lineage(&self, company: CompanyId, run: RunId, parent: RunId) -> AppResult<Lineage> {
        let row: LineageRow = sqlx::query_as(
            "SELECT link.parent_execution_id, execution.step_id, wait.child_ordinal, \
                    wait.child_tool_call_id \
               FROM workflow_run_parents AS link \
               LEFT JOIN workflow_executions AS execution \
                 ON execution.company_id = link.company_id \
                AND execution.id = link.parent_execution_id \
               LEFT JOIN workflow_waits AS wait ON wait.company_id = link.company_id \
                AND wait.run_id = link.parent_run_id AND wait.child_run_id = link.child_run_id \
              WHERE link.company_id = $1 AND link.child_run_id = $2",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .fetch_one(self.pool())
        .await?;
        Ok(Lineage {
            parent,
            parent_execution: row.parent_execution_id,
            parent_step: row.step_id,
            round: row.child_ordinal,
            tool_call: row.child_tool_call_id,
        })
    }
}

#[async_trait]
impl RunViews for PostgresPersistence {
    async fn run_view(&self, company: CompanyId, run: RunId) -> AppResult<Option<RunView>> {
        let Some(detail) = self.run_detail(company, run).await? else {
            return Ok(None);
        };
        let facts = self.run_facts(company, run).await?;
        let lineage = match detail.summary.parent {
            Some(parent) => Some(self.lineage(company, run, parent).await?),
            None => None,
        };
        let (reruns, reruns_truncated) = bounded(self.reruns(company, run).await?);
        let (children, children_truncated) =
            bounded(self.children(company, run, None, MAX_LISTED).await?);
        let (controls, controls_truncated) =
            listed::<ControlRow, _>(self.run_rows(CONTROLS_SQL, company, run).await?);
        let (reconciliations, reconciliations_truncated) =
            listed::<ReconciliationRow, _>(self.run_rows(RECONCILIATIONS_SQL, company, run).await?);
        let (events, events_truncated) =
            listed::<EventRow, _>(self.run_rows(EVENTS_SQL, company, run).await?);
        Ok(Some(RunView {
            detail,
            workflow_title: facts.title,
            draft_revision: revision_of(facts.draft_revision)?,
            channel_name: facts.channel_name,
            trigger_message: self.trigger_message(company, run).await?,
            input: facts.input,
            params: facts.params,
            lineage,
            reruns,
            reruns_truncated,
            children,
            children_truncated,
            controls,
            controls_truncated,
            reconciliations,
            reconciliations_truncated,
            events,
            events_truncated,
        }))
    }

    async fn step_view(
        &self,
        company: CompanyId,
        run: RunId,
        execution: Uuid,
    ) -> AppResult<Option<StepView>> {
        let row: Option<StepRow> = sqlx::query_as(
            "SELECT step_id, activation, created_at, activated_at, completed_at, frozen_inputs, \
                    committed_output, frozen_choice, committed_route, route_target \
               FROM workflow_executions WHERE company_id = $1 AND run_id = $2 AND id = $3",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(execution)
        .fetch_optional(self.pool())
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let (model_calls, calls_cut) = step_listed::<ModelCallRow, _>(
            self.step_rows(MODEL_CALLS_SQL, company, run, execution)
                .await?,
        );
        let (receipts, receipts_cut) = step_listed::<ReceiptRow, _>(
            self.step_rows(RECEIPTS_SQL, company, run, execution)
                .await?,
        );
        let (attempts, attempts_cut) = step_listed::<AttemptRow, _>(
            self.step_rows(ATTEMPTS_SQL, company, run, execution)
                .await?,
        );
        let (children, children_cut) = step_bounded(
            self.children(company, run, Some(execution), MAX_STEP_ITEMS)
                .await?,
        );
        Ok(Some(StepView {
            execution,
            step: row.step_id,
            activation: row.activation,
            created_at: row.created_at,
            activated_at: row.activated_at,
            completed_at: row.completed_at,
            frozen_inputs: row.frozen_inputs,
            committed_output: row.committed_output,
            frozen_choice: row.frozen_choice,
            committed_route: row.committed_route,
            route_target: row.route_target,
            model_calls,
            receipts,
            attempts,
            children,
            truncated: calls_cut || receipts_cut || attempts_cut || children_cut,
        }))
    }

    async fn workflow_titles(
        &self,
        company: CompanyId,
        workflows: &[Uuid],
    ) -> AppResult<std::collections::HashMap<Uuid, String>> {
        Ok(sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, title FROM workflow_definitions WHERE company_id = $1 AND id = ANY($2)",
        )
        .bind(company.as_uuid())
        .bind(workflows)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .collect())
    }

    async fn run_bundle(
        &self,
        company: CompanyId,
        run: RunId,
    ) -> AppResult<Option<Arc<PublishedBundle>>> {
        let bytes: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT bundle FROM workflow_runs WHERE company_id = $1 AND id = $2",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .fetch_optional(self.pool())
        .await?;
        bytes
            .map(|bytes| {
                restore_bundle(
                    &bytes,
                    &crate::adapters::workflow_source::WorkflowSourceDecoder,
                )
            })
            .transpose()
    }
}
