//! The run board's read model (plan `fix-03-run-board-finished-columns`): one column per run state,
//! top-level runs only, finished columns limited to a window. Persistence counts the working set
//! by state and channel and reads the cards ([`super::reads::WorkflowReads::run_board_counts`],
//! [`super::reads::WorkflowReads::run_board_cards`]); the route decides which channels the viewer
//! may see between the two reads and assembles the columns here.
use super::reads::{MAX_ROLLUP_PARENTS, RunStateName, RunSummary};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{WorkflowBindingId, WorkflowId};
use std::collections::HashMap;
use uuid::Uuid;

/// The most cards one board column shows. Each column reads one more, so the board knows when it
/// cut one. Whoever raises this must leave `RunBoardColumn::cut`/`total` and the
/// `Workflow run board is slow` warning working, and keep the child rollup's bound in step.
pub const BOARD_COLUMN_CAP: usize = 50;
const _: () = assert!(MAX_ROLLUP_PARENTS >= RunStateName::ALL.len() * BOARD_COLUMN_CAP);

/// How far back the board's finished columns reach. Open columns have no window: an open run is
/// work in progress however old it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardWindow {
    Day,
    /// The same seven days as the Work page's failed-run rule.
    #[default]
    Week,
    Month,
}

impl BoardWindow {
    pub fn duration(self) -> chrono::Duration {
        match self {
            Self::Day => chrono::Duration::days(1),
            Self::Week => chrono::Duration::days(7),
            Self::Month => chrono::Duration::days(30),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }
}

/// What the board shows runs of: the run list's scope filters. A board is always top-level (a
/// parent card counts its children instead) and never filtered by state: the states are its
/// columns.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunBoardScope {
    pub binding: Option<WorkflowBindingId>,
    /// A workflow of the company, already resolved company-scoped: runs of any of its versions.
    pub workflow: Option<WorkflowId>,
    /// An agent of the company, already resolved company-scoped: runs of versions that use it.
    pub agent: Option<Uuid>,
}

/// Top-level runs of one state on one channel (`None`: runs without a channel) inside the board's
/// working set — every open run, and runs finished at or after the window's cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardCount {
    pub state: RunStateName,
    pub channel: Option<Uuid>,
    pub runs: u64,
}

/// The channels whose runs the viewer may see, and whether runs without a channel are among them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibleChannels {
    pub channels: Vec<Uuid>,
    pub company: bool,
}

impl VisibleChannels {
    pub fn includes(&self, channel: Option<Uuid>) -> bool {
        match channel {
            Some(channel) => self.channels.contains(&channel),
            None => self.company,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.channels.is_empty() && !self.company
    }
}

/// One state's column as the viewer sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunBoardColumn {
    pub state: RunStateName,
    /// At most [`BOARD_COLUMN_CAP`], in the column's order: open columns oldest first, finished
    /// columns most recently finished first.
    pub cards: Vec<RunSummary>,
    /// The column's runs on channels the viewer may see; at least the cards read, so a cut column
    /// always reads more than it shows.
    pub total: u64,
    /// More runs than [`BOARD_COLUMN_CAP`]: the column read its look-ahead card.
    pub cut: bool,
}

/// The six columns, in [`RunStateName::ALL`] order, from the counts and the cards read (at most
/// `BOARD_COLUMN_CAP + 1` per state, already restricted to `visible`). Counts on hidden channels
/// are left out. A run that arrived between the two reads can make a column read more cards than
/// it counted; the total then follows the cards, and the next wake-up reconciles.
pub fn assemble_columns(
    counts: &[BoardCount],
    visible: &VisibleChannels,
    cards: Vec<RunSummary>,
) -> AppResult<Vec<RunBoardColumn>> {
    let mut by_state: HashMap<RunStateName, Vec<RunSummary>> = HashMap::new();
    for card in cards {
        let state = RunStateName::parse(&card.state).ok_or_else(|| {
            AppError::Internal(format!(
                "A board card has an unknown run state: {}",
                card.state
            ))
        })?;
        by_state.entry(state).or_default().push(card);
    }
    Ok(RunStateName::ALL
        .into_iter()
        .map(|state| {
            let mut cards = by_state.remove(&state).unwrap_or_default();
            let read = cards.len();
            let counted: u64 = counts
                .iter()
                .filter(|count| count.state == state && visible.includes(count.channel))
                .map(|count| count.runs)
                .sum();
            cards.truncate(BOARD_COLUMN_CAP);
            RunBoardColumn {
                state,
                cards,
                total: counted.max(read as u64),
                cut: read > BOARD_COLUMN_CAP,
            }
        })
        .collect())
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
