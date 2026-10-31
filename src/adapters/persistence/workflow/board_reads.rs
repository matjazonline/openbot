//! The run board's two statements (plan `fix-03-run-board-finished-columns`): the working set
//! counted by state and channel, then the cards of the channels the viewer may see. Both read only
//! top-level runs that are open or finished inside the window, never a version's or the company's
//! whole history. State names are written into the text from [`RunStateName`], never input, so
//! the partial indexes whose predicates name them apply to generic plans too. Synchronous so
//! `dump_run_list_query_shapes` hands `scripts/tests/run-list-query-plan.py` the shipped text.
//!
//! A binding, workflow or agent scope filters a `MATERIALIZED` working set, which Postgres runs at most once.
//! With the filter beside the working set, the planner drove the read from
//! `workflow_runs_binding_newest` or from each of the agent's versions and read that scope's whole
//! history: 230,378 buffers over a dense agent's 400 versions where the working set took 12,652.
//! Behind a mere `OFFSET 0` fence, it re-read the working set once per version of the agent
//! (`docs/performance/run-list.md`, "Run board"). A scope with little history pays the working set
//! instead; the read stays bounded by the window.
use super::reads::RUN_COLUMNS;
use super::*;
use crate::application::workflow::board::{BOARD_COLUMN_CAP, RunBoardScope, VisibleChannels};
use crate::application::workflow::reads::RunStateName;
use chrono::{DateTime, Utc};
use sqlx::QueryBuilder;

/// The open states as a SQL list: `'queued', 'running', 'waiting'`, the predicate of
/// `workflow_runs_open_by_channel`.
fn open_states() -> String {
    RunStateName::ALL
        .into_iter()
        .filter(|state| state.is_open())
        .map(|state| format!("'{}'", state.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn scoped(scope: &RunBoardScope) -> bool {
    scope.binding.is_some() || scope.workflow.is_some() || scope.agent.is_some()
}

/// The scope's filters on the materialized working set, the first after `WHERE`. The agent is
/// matched through the versions that use it.
fn push_scope(
    query: &mut QueryBuilder<'static, Postgres>,
    company: CompanyId,
    scope: &RunBoardScope,
) {
    let mut keyword = " WHERE ";
    if let Some(binding) = scope.binding {
        query
            .push(keyword)
            .push("binding_id = ")
            .push_bind(binding.as_uuid());
        keyword = " AND ";
    }
    if let Some(workflow) = scope.workflow {
        query
            .push(keyword)
            .push("workflow_id = ")
            .push_bind(workflow.as_uuid());
        keyword = " AND ";
    }
    if let Some(agent) = scope.agent {
        query
            .push(keyword)
            .push(
                "version_id IN (SELECT version.id FROM workflow_versions AS version \
                 WHERE version.company_id = ",
            )
            .push_bind(company.as_uuid())
            .push(" AND ")
            .push_bind(agent)
            .push(" = ANY(version.agent_ids))");
    }
}

/// Every open run, and runs finished at or after `cutoff`, counted by state and channel.
pub(super) fn board_counts_query(
    company: CompanyId,
    scope: &RunBoardScope,
    cutoff: DateTime<Utc>,
) -> QueryBuilder<'static, Postgres> {
    let working =
        "SELECT state, channel_id, binding_id, workflow_id, version_id FROM workflow_runs";
    let mut query = QueryBuilder::new(format!(
        "WITH working AS MATERIALIZED ({working} WHERE company_id = "
    ));
    query.push_bind(company.as_uuid()).push(format!(
        " AND state IN ({}) AND parent_run_id IS NULL UNION ALL {working} WHERE company_id = ",
        open_states()
    ));
    query
        .push_bind(company.as_uuid())
        .push(" AND finished_at >= ")
        .push_bind(cutoff)
        .push(
            " AND parent_run_id IS NULL) \
             SELECT state, channel_id, COUNT(*)::bigint AS runs FROM working",
        );
    push_scope(&mut query, company, scope);
    query.push(" GROUP BY state, channel_id");
    query
}

/// One subquery per state, each cut one past [`BOARD_COLUMN_CAP`] so the board knows when it cut a
/// column: open states oldest first, finished states most recently finished first. Unscoped, a
/// finished column stops after its first matching rows of the finished-run index; scoped, each
/// column's working set is materialized, filtered and sorted.
pub(super) fn board_cards_query(
    company: CompanyId,
    scope: &RunBoardScope,
    cutoff: DateTime<Utc>,
    visible: &VisibleChannels,
) -> QueryBuilder<'static, Postgres> {
    let mut query = QueryBuilder::new("");
    if scoped(scope) {
        for (index, state) in RunStateName::ALL.into_iter().enumerate() {
            query.push(if index == 0 { "WITH " } else { ", " });
            query.push(format!(
                "{} AS MATERIALIZED (SELECT {RUN_COLUMNS} FROM ",
                state.as_str()
            ));
            push_column(&mut query, company, state, cutoff, visible);
            query.push(")");
        }
        query.push(" ");
    }
    for (index, state) in RunStateName::ALL.into_iter().enumerate() {
        if index > 0 {
            query.push(" UNION ALL ");
        }
        query.push(format!("(SELECT {RUN_COLUMNS} FROM "));
        if scoped(scope) {
            query.push(state.as_str());
            push_scope(&mut query, company, scope);
        } else {
            push_column(&mut query, company, state, cutoff, visible);
        }
        let order = if state.is_open() {
            "created_at, id"
        } else {
            "finished_at DESC, id DESC"
        };
        query.push(format!(" ORDER BY {order} LIMIT {})", BOARD_COLUMN_CAP + 1));
    }
    query
}

/// One column's working set: its top-level runs on the visible channels, finished ones inside the
/// window.
fn push_column(
    query: &mut QueryBuilder<'static, Postgres>,
    company: CompanyId,
    state: RunStateName,
    cutoff: DateTime<Utc>,
    visible: &VisibleChannels,
) {
    query
        .push("workflow_runs WHERE company_id = ")
        .push_bind(company.as_uuid())
        .push(format!(
            " AND state = '{}' AND parent_run_id IS NULL",
            state.as_str()
        ))
        .push(" AND (channel_id = ANY(")
        .push_bind(visible.channels.clone())
        .push(") OR (channel_id IS NULL AND ")
        .push_bind(visible.company)
        .push("))");
    if !state.is_open() {
        query.push(" AND finished_at >= ").push_bind(cutoff);
    }
}

/// One `(state, channel)` group of [`board_counts_query`].
#[derive(sqlx::FromRow)]
pub(super) struct BoardCountRow {
    pub(super) state: String,
    pub(super) channel_id: Option<Uuid>,
    pub(super) runs: i64,
}
