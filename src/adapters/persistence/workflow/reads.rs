//! The workflow routes' read models (p08 B3b-1): plain company-scoped reads with no locks. A
//! route authorizes before it shows any of them.
use super::binding_rows::association_from_columns;
use super::board_reads;
use super::*;
use crate::application::workflow::board::{BoardCount, RunBoardScope, VisibleChannels};
use crate::application::workflow::reads::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::QueryBuilder;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// One parent's children in one state, as `child_rollup` groups them.
type ChildStateCount = (Uuid, String, i64);

pub(super) fn revision_of(value: i64) -> AppResult<u64> {
    u64::try_from(value).map_err(|_| invalid())
}

/// Rows fetched one past the page size become the page and its `more` flag.
fn paged<T>(mut rows: Vec<T>, size: i64) -> Page<T> {
    let size = usize::try_from(size).unwrap_or(0);
    let more = rows.len() > size;
    rows.truncate(size);
    Page { items: rows, more }
}

/// A detail list fetched one past [`MAX_LISTED`]: the kept rows and whether any were cut.
pub(super) fn bounded<T>(mut rows: Vec<T>) -> (Vec<T>, bool) {
    let cut = rows.len() > MAX_LISTED as usize;
    rows.truncate(MAX_LISTED as usize);
    (rows, cut)
}

fn stale_cursor() -> AppError {
    AppError::BadRequest("The page cursor no longer names a row; start the listing again".into())
}

type DefinitionRow = (Uuid, i64, String, bool, Option<Value>);

fn definition_summary(
    (workflow, revision, title, archived, origin): DefinitionRow,
) -> AppResult<DefinitionSummary> {
    Ok(DefinitionSummary {
        workflow: WorkflowId::new(workflow),
        revision: revision_of(revision)?,
        title,
        archived,
        template_origin: origin,
    })
}

pub(super) type BindingRow = (
    Uuid,
    i64,
    i64,
    bool,
    Option<Uuid>,
    Option<Uuid>,
    Uuid,
    Uuid,
    Value,
    Uuid,
    Option<String>,
);

pub(super) const BINDING_COLUMNS: &str = "binding.id, binding.state_revision, binding.configuration_revision, \
    binding.active, binding.channel_id, binding.thread_id, version.workflow_id, revision.version_id, \
    revision.params, revision.run_as_user_id, revision.message_trigger";
pub(super) const BINDING_FROM: &str = "FROM workflow_bindings AS binding \
    JOIN workflow_binding_revisions AS revision ON revision.company_id = binding.company_id \
      AND revision.binding_id = binding.id AND revision.revision = binding.configuration_revision \
    JOIN workflow_versions AS version ON version.company_id = revision.company_id \
      AND version.id = revision.version_id";

pub(super) fn binding_summary(row: BindingRow) -> AppResult<BindingSummary> {
    let (
        id,
        state,
        configuration,
        active,
        channel,
        thread,
        workflow,
        version,
        params,
        run_as,
        trigger,
    ) = row;
    Ok(BindingSummary {
        binding: WorkflowBindingId::new(id),
        state_revision: revision_of(state)?,
        configuration_revision: revision_of(configuration)?,
        active,
        association: association_from_columns(channel, thread)?,
        workflow: WorkflowId::new(workflow),
        version: VersionId::new(version),
        params,
        run_as,
        message_trigger: trigger
            .map(|trigger| trigger.parse().map_err(|_| invalid()))
            .transpose()?,
    })
}

/// A `workflow_runs` row as the run reads select it ([`RUN_COLUMNS`]), matched by column name.
#[derive(sqlx::FromRow)]
pub(super) struct RunRow {
    id: Uuid,
    binding_id: Uuid,
    workflow_id: Uuid,
    version_id: Uuid,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    state: String,
    waiting_reason: Option<String>,
    revision: i64,
    actor_id: Uuid,
    parent_run_id: Option<Uuid>,
    rerun_of_run_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

pub(super) const RUN_COLUMNS: &str = "id, binding_id, workflow_id, version_id, channel_id, thread_id, state, \
    waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline, \
    finished_at";

pub(super) fn run_summary(row: RunRow) -> AppResult<RunSummary> {
    Ok(RunSummary {
        run: RunId::new(row.id),
        binding: WorkflowBindingId::new(row.binding_id),
        workflow: WorkflowId::new(row.workflow_id),
        version: VersionId::new(row.version_id),
        association: association_from_columns(row.channel_id, row.thread_id)?,
        state: row.state,
        waiting_reason: row.waiting_reason,
        revision: revision_of(row.revision)?,
        actor: row.actor_id,
        parent: row.parent_run_id.map(RunId::new),
        rerun_of: row.rerun_of_run_id.map(RunId::new),
        created_at: row.created_at,
        deadline: row.deadline,
        finished_at: row.finished_at,
    })
}

/// The run list's statement: newest first, `limit` rows, after `cursor` when given. Synchronous so
/// `dump_run_list_query_shapes` can hand `scripts/tests/run-list-query-plan.py` the shipped text.
///
/// With an agent, the versions that use it (their `agent_ids` display index, gate P09-B1b-D1) are
/// read in the same statement, and each version's runs are one ordered range of
/// `workflow_runs_version_newest` cut at `limit` before the ranges are merged. A page therefore
/// reads at most `limit` runs per such version — never more than the agent's history, but all of
/// it once its versions each hold no more runs than a page. A plain `version_id IN (...)` read and
/// sorted the whole history on every page. A top-level read takes its ranges from
/// `workflow_runs_version_top_level`, which leaves child runs out, so a version whose runs are all
/// children is empty rather than read to its end. Measurements, and the deferred run-to-agent link
/// table that would read one page, are in `docs/performance/run-list.md`.
fn run_list_query(
    company: CompanyId,
    filter: &RunFilter,
    cursor: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> QueryBuilder<'static, Postgres> {
    let mut query = QueryBuilder::new("");
    let Some(agent) = filter.agent else {
        push_run_page(&mut query, company, None, filter, cursor, limit);
        return query;
    };
    query
        .push(format!(
            "SELECT {RUN_COLUMNS} FROM (SELECT version.id AS used_version \
             FROM workflow_versions AS version WHERE version.company_id = "
        ))
        .push_bind(company.as_uuid())
        .push(" AND ")
        .push_bind(agent)
        .push(" = ANY(version.agent_ids)) AS agent_versions CROSS JOIN LATERAL (");
    push_run_page(
        &mut query,
        company,
        Some("agent_versions.used_version"),
        filter,
        cursor,
        limit,
    );
    query
        .push(") AS run ORDER BY created_at DESC, id DESC LIMIT ")
        .push_bind(limit);
    query
}

/// One newest-first page of the company's runs with the filters present — only those, because a
/// generic plan over `$n IS NULL OR ...` would not use the `workflow_runs_newest` /
/// `workflow_runs_binding_newest` keyset indexes. `version`, an outer column, narrows it to one
/// version's range.
fn push_run_page(
    query: &mut QueryBuilder<'static, Postgres>,
    company: CompanyId,
    version: Option<&'static str>,
    filter: &RunFilter,
    cursor: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) {
    query
        .push(format!(
            "SELECT {RUN_COLUMNS} FROM workflow_runs WHERE company_id = "
        ))
        .push_bind(company.as_uuid());
    if let Some(version) = version {
        query.push(format!(" AND version_id = {version}"));
    }
    if let Some(binding) = filter.binding {
        query
            .push(" AND binding_id = ")
            .push_bind(binding.as_uuid());
    }
    if let Some(state) = &filter.state {
        query.push(" AND state = ").push_bind(state.clone());
    }
    if filter.lineage == RunLineage::TopLevel {
        query.push(" AND parent_run_id IS NULL");
    }
    if let Some((created, id)) = cursor {
        query
            .push(" AND (created_at, id) < (")
            .push_bind(created)
            .push(", ")
            .push_bind(id)
            .push(")");
    }
    query
        .push(" ORDER BY created_at DESC, id DESC LIMIT ")
        .push_bind(limit);
}

/// How long one run-list read may take before it is logged. The deleted task board's threshold
/// (`BOARD_QUERY_WARN_THRESHOLD`); `docs/performance/run-list.md` says how to read the line.
pub(crate) const RUN_LIST_QUERY_WARN_THRESHOLD: Duration = Duration::from_millis(500);

/// Whether a read that took `elapsed` is past `threshold`.
fn is_slow(elapsed: Duration, threshold: Duration) -> bool {
    elapsed > threshold
}

/// The state filter as the slow-read log names it: one of the run states, `none`, or `other`. The
/// JSON route passes the caller's text through unchecked, so it is never logged as given.
fn logged_state(state: Option<&str>) -> &'static str {
    match state {
        None => "none",
        Some(state) => RunStateName::parse(state).map_or("other", RunStateName::as_str),
    }
}

/// The lineage filter as the slow-read log names it: `any` or `top_level`.
fn logged_lineage(lineage: RunLineage) -> &'static str {
    match lineage {
        RunLineage::Any => "any",
        RunLineage::TopLevel => "top_level",
    }
}

/// Log a run-list read past [`RUN_LIST_QUERY_WARN_THRESHOLD`] with the shape that ran — which
/// binding and agent filters were present (not their ids), the state asked for and the lineage —
/// and `page_rows`, the rows the statement returned: `page_size` plus one look-ahead row when the
/// page filled, fewer when it ran out of matches. That is the evidence for revisiting the index
/// decisions in `docs/performance/run-list.md`.
fn warn_if_run_list_is_slow(
    company: CompanyId,
    filter: &RunFilter,
    page_size: i64,
    page_rows: usize,
    elapsed: Duration,
) {
    if !is_slow(elapsed, RUN_LIST_QUERY_WARN_THRESHOLD) {
        return;
    }
    tracing::warn!(
        elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        company_id = %company.as_uuid(),
        binding = filter.binding.is_some(),
        state = logged_state(filter.state.as_deref()),
        agent = filter.agent.is_some(),
        lineage = logged_lineage(filter.lineage),
        page_size,
        page_rows,
        "Workflow run list is slow"
    );
}

#[async_trait]
impl WorkflowReads for PostgresPersistence {
    async fn list_definitions(
        &self,
        company: CompanyId,
        page: PageRequest,
    ) -> AppResult<Page<DefinitionSummary>> {
        let cursor: Option<(String, Uuid)> = match page.after {
            None => None,
            Some(after) => Some(
                sqlx::query_as(
                    "SELECT title, id FROM workflow_definitions WHERE company_id = $1 AND id = $2",
                )
                .bind(company.as_uuid())
                .bind(after)
                .fetch_optional(self.pool())
                .await?
                .ok_or_else(stale_cursor)?,
            ),
        };
        let rows = sqlx::query_as::<_, DefinitionRow>(
            "SELECT id, revision, title, archived, template_origin FROM workflow_definitions \
             WHERE company_id = $1 AND ($2::text IS NULL OR (title, id) > ($2, $3)) \
             ORDER BY title, id LIMIT $4",
        )
        .bind(company.as_uuid())
        .bind(cursor.as_ref().map(|(title, _)| title.as_str()))
        .bind(cursor.as_ref().map(|(_, id)| *id))
        .bind(page.size + 1)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .map(definition_summary)
        .collect::<AppResult<Vec<_>>>()?;
        Ok(paged(rows, page.size))
    }

    async fn definition(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
    ) -> AppResult<Option<DefinitionDetail>> {
        let row: Option<(Uuid, i64, String, bool, Option<Value>, String, String)> = sqlx::query_as(
            "SELECT id, revision, title, archived, template_origin, description, source \
                 FROM workflow_definitions WHERE company_id = $1 AND id = $2",
        )
        .bind(company.as_uuid())
        .bind(workflow.as_uuid())
        .fetch_optional(self.pool())
        .await?;
        let Some((id, revision, title, archived, origin, description, source)) = row else {
            return Ok(None);
        };
        let versions: Vec<(Uuid, i64, String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, draft_revision, content_hash, created_at FROM workflow_versions \
             WHERE company_id = $1 AND workflow_id = $2 ORDER BY created_at DESC, id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(workflow.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (versions, versions_truncated) = bounded(versions);
        Ok(Some(DefinitionDetail {
            summary: definition_summary((id, revision, title, archived, origin))?,
            description,
            source,
            versions: versions
                .into_iter()
                .map(|(version, draft, hash, created)| {
                    Ok(VersionSummary {
                        version: VersionId::new(version),
                        draft_revision: revision_of(draft)?,
                        content_hash: hash,
                        created_at: created,
                    })
                })
                .collect::<AppResult<_>>()?,
            versions_truncated,
        }))
    }

    async fn list_bindings(
        &self,
        company: CompanyId,
        page: PageRequest,
    ) -> AppResult<Page<BindingSummary>> {
        let rows = sqlx::query_as::<_, BindingRow>(&format!(
            "SELECT {BINDING_COLUMNS} {BINDING_FROM} WHERE binding.company_id = $1 \
             AND ($2::uuid IS NULL OR binding.id > $2) ORDER BY binding.id LIMIT $3"
        ))
        .bind(company.as_uuid())
        .bind(page.after)
        .bind(page.size + 1)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .map(binding_summary)
        .collect::<AppResult<Vec<_>>>()?;
        Ok(paged(rows, page.size))
    }

    async fn binding_summary(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<BindingSummary>> {
        sqlx::query_as::<_, BindingRow>(&format!(
            "SELECT {BINDING_COLUMNS} {BINDING_FROM} WHERE binding.company_id = $1 AND binding.id = $2"
        ))
        .bind(company.as_uuid())
        .bind(binding.as_uuid())
        .fetch_optional(self.pool())
        .await?
        .map(binding_summary)
        .transpose()
    }

    async fn list_runs(
        &self,
        company: CompanyId,
        filter: &RunFilter,
        page: PageRequest,
    ) -> AppResult<Page<RunSummary>> {
        let cursor: Option<(DateTime<Utc>, Uuid)> = match page.after {
            None => None,
            Some(after) => Some(
                sqlx::query_as(
                    "SELECT created_at, id FROM workflow_runs WHERE company_id = $1 AND id = $2",
                )
                .bind(company.as_uuid())
                .bind(after)
                .fetch_optional(self.pool())
                .await?
                .ok_or_else(stale_cursor)?,
            ),
        };
        let started = Instant::now();
        let rows = run_list_query(company, filter, cursor, page.size + 1)
            .build_query_as::<RunRow>()
            .fetch_all(self.pool())
            .await?;
        warn_if_run_list_is_slow(company, filter, page.size, rows.len(), started.elapsed());
        let rows = rows
            .into_iter()
            .map(run_summary)
            .collect::<AppResult<Vec<_>>>()?;
        Ok(paged(rows, page.size))
    }

    async fn run_summary(&self, company: CompanyId, run: RunId) -> AppResult<Option<RunSummary>> {
        sqlx::query_as::<_, RunRow>(&format!(
            "SELECT {RUN_COLUMNS} FROM workflow_runs WHERE company_id = $1 AND id = $2"
        ))
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .fetch_optional(self.pool())
        .await?
        .map(run_summary)
        .transpose()
    }

    async fn run_board_counts(
        &self,
        company: CompanyId,
        scope: &RunBoardScope,
        cutoff: DateTime<Utc>,
    ) -> AppResult<Vec<BoardCount>> {
        board_reads::board_counts_query(company, scope, cutoff)
            .build_query_as::<board_reads::BoardCountRow>()
            .fetch_all(self.pool())
            .await?
            .into_iter()
            .map(|row| {
                Ok(BoardCount {
                    state: RunStateName::parse(&row.state).ok_or_else(invalid)?,
                    channel: row.channel_id,
                    runs: u64::try_from(row.runs).map_err(|_| invalid())?,
                })
            })
            .collect()
    }

    async fn run_board_cards(
        &self,
        company: CompanyId,
        scope: &RunBoardScope,
        cutoff: DateTime<Utc>,
        visible: &VisibleChannels,
    ) -> AppResult<Vec<RunSummary>> {
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        board_reads::board_cards_query(company, scope, cutoff, visible)
            .build_query_as::<RunRow>()
            .fetch_all(self.pool())
            .await?
            .into_iter()
            .map(run_summary)
            .collect()
    }

    async fn child_rollup(
        &self,
        company: CompanyId,
        parents: &[RunId],
    ) -> AppResult<HashMap<RunId, ChildCounts>> {
        if parents.len() > MAX_ROLLUP_PARENTS {
            return Err(AppError::BadRequest(format!(
                "A child rollup counts at most {MAX_ROLLUP_PARENTS} parent runs"
            )));
        }
        let parents: Vec<Uuid> = parents.iter().map(|run| run.as_uuid()).collect();
        // One grouped read over the parent index (`workflow_run_parents_by_parent`), never one per
        // card. `workflow_run_parents` holds direct links only, so grandchildren stay with their
        // own parent.
        let rows: Vec<ChildStateCount> = sqlx::query_as(
            "SELECT link.parent_run_id, child.state, COUNT(*)::bigint \
               FROM workflow_run_parents AS link \
               JOIN workflow_runs AS child ON child.company_id = link.company_id \
                AND child.id = link.child_run_id \
              WHERE link.company_id = $1 AND link.parent_run_id = ANY($2) \
              GROUP BY link.parent_run_id, child.state",
        )
        .bind(company.as_uuid())
        .bind(&parents)
        .fetch_all(self.pool())
        .await?;
        let mut rollup: HashMap<RunId, ChildCounts> = HashMap::new();
        for (parent, state, count) in rows {
            let count = u64::try_from(count).map_err(|_| invalid())?;
            let counts = rollup.entry(RunId::new(parent)).or_default();
            counts.total += count;
            match RunStateName::parse(&state) {
                Some(state) if state.is_open() => counts.open += count,
                Some(RunStateName::Failed) => counts.failed += count,
                Some(_) => {}
                None => return Err(invalid()),
            }
        }
        Ok(rollup)
    }

    async fn run_detail(&self, company: CompanyId, run: RunId) -> AppResult<Option<RunDetail>> {
        let Some(summary) = self.run_summary(company, run).await? else {
            return Ok(None);
        };
        let executions: Vec<(
            Uuid,
            String,
            i64,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT id, step_id, activation, activated_at, completed_at, committed_route \
                 FROM workflow_executions WHERE company_id = $1 AND run_id = $2 \
                 ORDER BY activation DESC, id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (executions, executions_truncated) = bounded(executions);
        let waits: Vec<(
            Uuid,
            Uuid,
            String,
            String,
            DateTime<Utc>,
            Option<Uuid>,
            Option<Uuid>,
        )> = sqlx::query_as(
            "SELECT wait.id, wait.execution_id, wait.reason, wait.state, wait.deadline, \
                   decision.id, wait.child_run_id \
                 FROM workflow_waits AS wait \
                 LEFT JOIN workflow_decisions AS decision ON decision.company_id = wait.company_id \
                   AND decision.wait_id = wait.id \
                 WHERE wait.company_id = $1 AND wait.run_id = $2 \
                 ORDER BY wait.created_at DESC, wait.id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (waits, waits_truncated) = bounded(waits);
        let (actions, actions_truncated) = self.actions(company, run).await?;
        Ok(Some(RunDetail {
            summary,
            executions: executions
                .into_iter()
                .map(
                    |(execution, step, activation, activated, completed, route)| ExecutionSummary {
                        execution,
                        step,
                        activation,
                        activated_at: activated,
                        completed_at: completed,
                        route,
                    },
                )
                .collect(),
            executions_truncated,
            waits: waits
                .into_iter()
                .map(
                    |(wait, execution, reason, state, deadline, decision, child)| WaitSummary {
                        wait,
                        execution,
                        reason,
                        state,
                        deadline,
                        decision,
                        child_run: child.map(RunId::new),
                    },
                )
                .collect(),
            waits_truncated,
            actions,
            actions_truncated,
        }))
    }

    async fn decision_company(&self, decision: Uuid) -> AppResult<Option<CompanyId>> {
        Ok(
            sqlx::query_scalar::<_, Uuid>(
                "SELECT company_id FROM workflow_decisions WHERE id = $1",
            )
            .bind(decision)
            .fetch_optional(self.pool())
            .await?
            .map(CompanyId::new),
        )
    }
}

type ActionRow = (Uuid, Uuid, Uuid, String, String, i32, DateTime<Utc>, bool);

impl PostgresPersistence {
    /// The run's newest remote and message dispatches (P08-D3 §D3.4): the evidence-conflict
    /// check once per distinct execution, and message destinations in one read.
    async fn actions(
        &self,
        company: CompanyId,
        run: RunId,
    ) -> AppResult<(Vec<ActionSummary>, bool)> {
        let rows: Vec<ActionRow> = sqlx::query_as(
            "SELECT dispatch.id, dispatch.execution_id, dispatch.invocation_id, \
                    dispatch.argument_digest, dispatch.effect_kind, dispatch.attempt_number, \
                    dispatch.created_at, \
                    EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt \
                           WHERE receipt.company_id = dispatch.company_id \
                             AND receipt.invocation_id = dispatch.invocation_id) \
               FROM workflow_action_dispatches AS dispatch \
              WHERE dispatch.company_id = $1 AND dispatch.run_id = $2 \
                AND dispatch.effect_kind IN ('remote', 'message') \
              ORDER BY dispatch.created_at DESC, dispatch.id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (rows, truncated) = bounded(rows);
        let executions = rows
            .iter()
            .map(|row| row.1)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let conflicted: std::collections::HashMap<Uuid, bool> = sqlx::query_as(
            "SELECT execution, workflow_action_has_evidence_conflict($1, execution) \
               FROM unnest($2::uuid[]) AS execution",
        )
        .bind(company.as_uuid())
        .bind(&executions)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .collect();
        let messages = rows
            .iter()
            .filter(|row| row.4 == "message")
            .map(|row| row.2)
            .collect::<Vec<_>>();
        let mut destinations: std::collections::HashMap<Uuid, Vec<DestinationSummary>> =
            std::collections::HashMap::new();
        if !messages.is_empty() {
            let rows: Vec<(Uuid, i16, Uuid, String, i64)> = sqlx::query_as(
                "SELECT target.invocation_id, target.ordinal, delivery.id, delivery.status, \
                        (SELECT count(*) FROM message_delivery_parts AS part \
                          WHERE part.delivery_id = delivery.id) \
                   FROM workflow_message_destinations AS target \
                   JOIN message_deliveries AS delivery \
                     ON delivery.company_id = target.company_id AND delivery.id = target.delivery_id \
                  WHERE target.company_id = $1 AND target.run_id = $2 \
                    AND target.invocation_id = ANY($3) \
                  ORDER BY target.invocation_id, target.ordinal",
            )
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(&messages)
            .fetch_all(self.pool())
            .await?;
            for (invocation, ordinal, delivery, status, parts) in rows {
                destinations
                    .entry(invocation)
                    .or_default()
                    .push(DestinationSummary {
                        ordinal,
                        delivery,
                        status,
                        parts,
                    });
            }
        }
        let actions = rows
            .into_iter()
            .map(
                |(marker, execution, invocation, digest, kind, attempt, created, receipt)| {
                    ActionSummary {
                        marker,
                        execution,
                        invocation,
                        argument_digest: digest,
                        destinations: if kind == "message" {
                            destinations.get(&invocation).cloned().unwrap_or_default()
                        } else {
                            Vec::new()
                        },
                        effect_kind: kind,
                        attempt,
                        created_at: created,
                        has_receipt: receipt,
                        evidence_conflict: conflicted.get(&execution).copied().unwrap_or(false),
                    }
                },
            )
            .collect();
        Ok((actions, truncated))
    }
}

#[cfg(test)]
#[path = "reads_tests.rs"]
mod tests;
