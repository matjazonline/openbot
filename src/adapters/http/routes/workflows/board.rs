//! The run board (plan `fix-03-run-board-finished-columns`): one column per run state over the
//! top-level runs of a company, a binding or an agent — every open run, and the runs finished
//! inside a window. Counts first, then which of the counted channels the caller may see, then only
//! those channels' cards, each column cut at
//! [`BOARD_COLUMN_CAP`](crate::application::workflow::board::BOARD_COLUMN_CAP); each shown card
//! counts its children. No transaction spans the reads: a run that moves between them shows up on
//! the next read.
use super::bindings::{Visibility, company_agent, company_workflow, run_json, visible_binding};
use super::*;
use crate::application::workflow::RelatedChannelId;
use crate::application::workflow::board::{
    BoardCount, BoardWindow, RunBoardColumn, RunBoardScope, VisibleChannels, assemble_columns,
};
use crate::application::workflow::reads::ChildCounts;
use crate::domain::workflow::RunId;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

/// How long one board read — its four phases together — may take before it is logged; the run
/// list's threshold. `docs/performance/run-list.md` says how to read the line.
pub(super) const RUN_BOARD_QUERY_WARN_THRESHOLD: Duration = Duration::from_millis(500);

/// The board as the caller may see it.
pub(super) struct RunBoard {
    pub(super) window: BoardWindow,
    /// `as_of` less the window: the finished columns hold runs finished at or after it.
    pub(super) cutoff: DateTime<Utc>,
    /// In [`crate::application::workflow::reads::RunStateName::ALL`] order.
    pub(super) columns: Vec<RunBoardColumn>,
    /// Child counts of the shown cards that have children.
    pub(super) children: HashMap<RunId, ChildCounts>,
}

/// What one board read did, phase by phase: the slow-read log's fields.
#[derive(Debug, Default)]
struct BoardReadTrace {
    counts: Duration,
    visibility: Duration,
    cards: Duration,
    rollup: Duration,
    /// Every counted run, hidden channels included: the working set the counts read.
    eligible_runs: u64,
    /// Distinct associations authorized, the company's included.
    channels_checked: usize,
    /// Cards the statement returned, each column's look-ahead card included.
    cards_read: usize,
    cards_shown: usize,
}

impl BoardReadTrace {
    fn elapsed(&self) -> Duration {
        self.counts + self.visibility + self.cards + self.rollup
    }

    fn is_slow(&self) -> bool {
        self.elapsed() > RUN_BOARD_QUERY_WARN_THRESHOLD
    }
}

/// The board of `scope` as `actor` sees it at `as_of`. The caller has authorized the company and
/// resolved the scope company-scoped (a binding the caller may see, an agent of the company).
pub(super) async fn visible_board(
    engine: &WorkflowEngine,
    company: CompanyId,
    actor: WorkflowActor,
    scope: &RunBoardScope,
    window: BoardWindow,
    as_of: DateTime<Utc>,
) -> AppResult<RunBoard> {
    let cutoff = as_of - window.duration();
    let mut trace = BoardReadTrace::default();

    let phase = Instant::now();
    let counts = engine
        .reads()
        .run_board_counts(company, scope, cutoff)
        .await?;
    trace.counts = phase.elapsed();
    trace.eligible_runs = counts.iter().map(|count| count.runs).sum();

    let phase = Instant::now();
    let visible = visible_channels(engine, company, actor, &counts, &mut trace).await?;
    trace.visibility = phase.elapsed();

    let phase = Instant::now();
    let cards = engine
        .reads()
        .run_board_cards(company, scope, cutoff, &visible)
        .await?;
    trace.cards = phase.elapsed();
    trace.cards_read = cards.len();
    let columns = assemble_columns(&counts, &visible, cards)?;

    let phase = Instant::now();
    let shown: Vec<RunId> = columns
        .iter()
        .flat_map(|column| column.cards.iter().map(|card| card.run))
        .collect();
    trace.cards_shown = shown.len();
    let children = if shown.is_empty() {
        HashMap::new()
    } else {
        engine.reads().child_rollup(company, &shown).await?
    };
    trace.rollup = phase.elapsed();

    warn_if_run_board_is_slow(company, scope, window, &trace);
    Ok(RunBoard {
        window,
        cutoff,
        columns,
        children,
    })
}

/// The counted channels `actor` may see, each authorized once; runs without a channel are the
/// company's.
async fn visible_channels(
    engine: &WorkflowEngine,
    company: CompanyId,
    actor: WorkflowActor,
    counts: &[BoardCount],
    trace: &mut BoardReadTrace,
) -> AppResult<VisibleChannels> {
    let channels: BTreeSet<Option<Uuid>> = counts.iter().map(|count| count.channel).collect();
    trace.channels_checked = channels.len();
    let mut visibility = Visibility::new(engine, company, actor);
    let mut visible = VisibleChannels::default();
    for channel in channels {
        let association = channel.map_or(RelatedAssociation::Company, |channel| {
            RelatedAssociation::Channel(RelatedChannelId::new(channel))
        });
        if !visibility.allows(association).await? {
            continue;
        }
        match channel {
            Some(channel) => visible.channels.push(channel),
            None => visible.company = true,
        }
    }
    visibility.report("workflow-run-board");
    Ok(visible)
}

/// Log a board read past [`RUN_BOARD_QUERY_WARN_THRESHOLD`] with each phase's share, the scope
/// that ran (whether a binding, a workflow and an agent were given, not their ids), the window and the working
/// set: the evidence for revisiting the board's statements and index in
/// `docs/performance/run-list.md`.
fn warn_if_run_board_is_slow(
    company: CompanyId,
    scope: &RunBoardScope,
    window: BoardWindow,
    trace: &BoardReadTrace,
) {
    if !trace.is_slow() {
        return;
    }
    let ms = |duration: Duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    tracing::warn!(
        elapsed_ms = ms(trace.elapsed()),
        counts_ms = ms(trace.counts),
        visibility_ms = ms(trace.visibility),
        cards_ms = ms(trace.cards),
        rollup_ms = ms(trace.rollup),
        company_id = %company.as_uuid(),
        binding = scope.binding.is_some(),
        workflow = scope.workflow.is_some(),
        agent = scope.agent.is_some(),
        window = window.as_str(),
        eligible_runs = trace.eligible_runs,
        channels_checked = trace.channels_checked,
        cards_read = trace.cards_read,
        cards_shown = trace.cards_shown,
        "Workflow run board is slow"
    );
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardQuery {
    /// A binding the caller may see; another one is not found.
    pub binding_id: Option<Uuid>,
    /// A workflow of the company; another company's is not found.
    pub workflow_id: Option<Uuid>,
    /// Runs of versions that use this agent of the company, as the run list's `agent_id`.
    pub agent_id: Option<Uuid>,
    /// `day`, `week` (the default) or `month`: how far back the finished columns reach.
    #[serde(default)]
    pub window: BoardWindow,
}

/// `{window, cutoff, columns: [{state, total, cut, items}]}`: each column's `total` counts the
/// runs the caller may see, `cut` says it shows only the first
/// [`BOARD_COLUMN_CAP`](crate::application::workflow::board::BOARD_COLUMN_CAP), and each item is a
/// run as the run list shows it plus its `children` counts.
pub async fn board(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<BoardQuery>,
) -> WorkflowResult<Json<Value>> {
    let (company, actor) = (company(company_id), actor(&user)?);
    authorize(&engine, company, actor, RelatedAssociation::Company).await?;
    let binding = match query.binding_id {
        Some(binding) => Some(
            visible_binding(&engine, company, actor, binding)
                .await?
                .binding,
        ),
        None => None,
    };
    let agent = match query.agent_id {
        Some(agent) => Some(company_agent(&engine, company, agent).await?),
        None => None,
    };
    let workflow = match query.workflow_id {
        Some(workflow) => Some(company_workflow(&engine, company, workflow).await?),
        None => None,
    };
    let scope = RunBoardScope {
        binding,
        workflow,
        agent,
    };
    let board = visible_board(&engine, company, actor, &scope, query.window, Utc::now()).await?;
    Ok(ok(board_json(&board)))
}

fn board_json(board: &RunBoard) -> Value {
    let columns: Vec<Value> = board
        .columns
        .iter()
        .map(|column| {
            let items: Vec<Value> = column
                .cards
                .iter()
                .map(|card| {
                    let children = board.children.get(&card.run).copied().unwrap_or_default();
                    let mut item = run_json(card);
                    item["children"] = json!({
                        "total": children.total,
                        "open": children.open,
                        "failed": children.failed,
                    });
                    item
                })
                .collect();
            json!({
                "state": column.state.as_str(),
                "total": column.total,
                "cut": column.cut,
                "items": items,
            })
        })
        .collect();
    json!({
        "window": board.window.as_str(),
        "cutoff": board.cutoff,
        "columns": columns,
    })
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
