//! Plan `fix-03-run-board-finished-columns` step 2 at the read port: the board counts and reads
//! only top-level runs of the company that are open or finished inside the window, each column cut
//! one card past the cap, cards only from the channels the caller may see.
use super::run_finished_at_tests::drive;
use super::run_lineage_tests::admitted;
use super::*;
use crate::application::workflow::board::{
    BOARD_COLUMN_CAP, BoardCount, RunBoardColumn, RunBoardScope, VisibleChannels, assemble_columns,
};
use crate::application::workflow::reads::{RunStateName, RunSummary, WorkflowReads};
use chrono::{DateTime, Duration, SubsecRound, Utc};

fn data_map() -> Value {
    serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap()
}

/// Runs without a channel are visible; no channel is.
fn company_only() -> VisibleChannels {
    VisibleChannels {
        channels: Vec::new(),
        company: true,
    }
}

/// The board's two reads of `f`'s company at `cutoff`.
async fn read(
    f: &AdmissionFixture,
    scope: &RunBoardScope,
    cutoff: DateTime<Utc>,
    visible: &VisibleChannels,
) -> (Vec<BoardCount>, Vec<RunSummary>) {
    let (company, p) = (f.binding.target.company, f.persistence());
    (
        p.run_board_counts(company, scope, cutoff).await.unwrap(),
        p.run_board_cards(company, scope, cutoff, visible)
            .await
            .unwrap(),
    )
}

/// The columns of `f`'s company board at `cutoff`, every run visible.
async fn columns(f: &AdmissionFixture, cutoff: DateTime<Utc>) -> Vec<RunBoardColumn> {
    let visible = company_only();
    let (counts, cards) = read(f, &RunBoardScope::default(), cutoff, &visible).await;
    assemble_columns(&counts, &visible, cards).unwrap()
}

fn column(columns: &[RunBoardColumn], state: RunStateName) -> &RunBoardColumn {
    columns.iter().find(|column| column.state == state).unwrap()
}

fn ids(column: &RunBoardColumn) -> Vec<RunId> {
    column.cards.iter().map(|card| card.run).collect()
}

async fn finished_at(f: &AdmissionFixture, run: RunId, at: DateTime<Utc>) {
    sqlx::query("UPDATE workflow_runs SET finished_at = $2 WHERE id = $1")
        .bind(run.as_uuid())
        .bind(at)
        .execute(f.persistence().pool())
        .await
        .unwrap();
}

/// A column with more runs than the cap reads one more card than it shows, oldest first, and
/// counts them all.
#[tokio::test]
async fn run_board_read_cuts_each_column_one_past_the_cap() {
    let f = AdmissionFixture::new().await;
    let mut queued = Vec::new();
    for index in 0..BOARD_COLUMN_CAP + 2 {
        queued.push(admitted(&f, &format!("run-{index}")).await.run);
    }
    let visible = company_only();
    let (counts, cards) = read(&f, &RunBoardScope::default(), Utc::now(), &visible).await;
    assert_eq!(
        cards.len(),
        BOARD_COLUMN_CAP + 1,
        "the cap plus the look-ahead"
    );
    let columns = assemble_columns(&counts, &visible, cards).unwrap();
    let column = column(&columns, RunStateName::Queued);
    assert_eq!(ids(column), queued[..BOARD_COLUMN_CAP]);
    assert!(column.cut);
    assert_eq!(column.total, (BOARD_COLUMN_CAP + 2) as u64);
    assert!(
        columns
            .iter()
            .filter(|column| column.state != RunStateName::Queued)
            .all(|column| column.total == 0 && column.cards.is_empty())
    );
}

/// A finished column holds runs finished at or after the cutoff, newest finished first; an open
/// column ignores the window, however far ahead the cutoff is.
#[tokio::test]
async fn run_board_read_windows_finished_columns_by_finished_at() {
    let f = AdmissionFixture::new().await;
    let as_of = Utc::now().trunc_subsecs(6);
    let cutoff = as_of - Duration::days(7);
    let mut runs = Vec::new();
    for (key, at) in [
        ("on-the-cutoff", cutoff),
        ("before-it", cutoff - Duration::microseconds(1)),
        ("an-hour-ago", as_of - Duration::hours(1)),
    ] {
        let run = admitted(&f, key).await.run;
        drive(&f, run, RunStateName::Succeeded).await;
        finished_at(&f, run, at).await;
        runs.push(run);
    }
    let open = admitted(&f, "open").await.run;

    let board = columns(&f, cutoff).await;
    let succeeded = column(&board, RunStateName::Succeeded);
    assert_eq!(ids(succeeded), [runs[2], runs[0]]);
    assert_eq!((succeeded.total, succeeded.cut), (2, false));
    assert_eq!(ids(column(&board, RunStateName::Queued)), [open]);

    let board = columns(&f, as_of + Duration::days(30)).await;
    assert_eq!(column(&board, RunStateName::Succeeded).total, 0);
    assert_eq!(ids(column(&board, RunStateName::Queued)), [open]);
}

/// A retried run leaves its finished column for its open one.
#[tokio::test]
async fn run_board_read_moves_a_retried_run_back_to_its_open_column() {
    let f = AdmissionFixture::new().await;
    let run = admitted(&f, "retried").await.run;
    drive(&f, run, RunStateName::Failed).await;
    let cutoff = Utc::now() - Duration::days(1);
    assert_eq!(
        ids(column(&columns(&f, cutoff).await, RunStateName::Failed)),
        [run]
    );
    drive(&f, run, RunStateName::Queued).await;
    let board = columns(&f, cutoff).await;
    assert_eq!(column(&board, RunStateName::Failed).total, 0);
    assert_eq!(ids(column(&board, RunStateName::Queued)), [run]);
}

/// A child is neither counted nor read, nor is another company's run; a binding scope keeps its
/// own binding's runs.
#[tokio::test]
async fn run_board_read_leaves_out_children_other_bindings_and_other_tenants() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let (root, child) = (scopes[0].run, scopes[1].run);
    let sibling = f.sibling(WorkflowId::new(Uuid::new_v4())).await;
    let other = admitted(&sibling, "other-binding").await.run;
    let foreign = f.foreign().await;
    admitted(&foreign, "theirs").await;

    let visible = company_only();
    let cutoff = Utc::now() - Duration::days(1);
    let (counts, cards) = read(&f, &RunBoardScope::default(), cutoff, &visible).await;
    assert_eq!(counts.iter().map(|count| count.runs).sum::<u64>(), 2);
    let board = assemble_columns(&counts, &visible, cards).unwrap();
    assert_eq!(ids(column(&board, RunStateName::Waiting)), [root]);
    assert_eq!(ids(column(&board, RunStateName::Queued)), [other]);
    assert!(board.iter().all(|column| !ids(column).contains(&child)));

    let scope = RunBoardScope {
        binding: Some(f.binding.target.binding),
        ..RunBoardScope::default()
    };
    let (counts, cards) = read(&f, &scope, cutoff, &visible).await;
    assert_eq!(counts.len(), 1);
    assert_eq!(
        cards.iter().map(|card| card.run).collect::<Vec<_>>(),
        [root]
    );
}

/// Cards come only from the visible channels; the counts still hold every channel, for the route
/// to filter after it decides which the caller may see.
#[tokio::test]
async fn run_board_read_reads_cards_only_from_the_visible_channels() {
    let f = AdmissionFixture::new().await;
    admitted(&f, "company-run").await;
    let cutoff = Utc::now() - Duration::days(1);
    let elsewhere = VisibleChannels {
        channels: vec![Uuid::new_v4()],
        company: false,
    };
    let (counts, cards) = read(&f, &RunBoardScope::default(), cutoff, &elsewhere).await;
    assert_eq!(counts.len(), 1);
    assert!(cards.is_empty());
    let (_, cards) = read(
        &f,
        &RunBoardScope::default(),
        cutoff,
        &VisibleChannels::default(),
    )
    .await;
    assert!(cards.is_empty());
    let (_, cards) = read(&f, &RunBoardScope::default(), cutoff, &company_only()).await;
    assert_eq!(cards.len(), 1);
}

/// A run admitted between the two reads: its column reads one card more than it counted, the total
/// follows the cards (never "1 of 2" for a column that was not cut), and the next read counts it.
#[tokio::test]
async fn run_board_read_counts_a_run_that_arrives_between_its_reads_on_the_next_read() {
    let f = AdmissionFixture::new().await;
    let (company, p) = (f.binding.target.company, f.persistence());
    let (scope, visible) = (RunBoardScope::default(), company_only());
    let cutoff = Utc::now() - Duration::days(1);
    let first = admitted(&f, "first").await.run;
    let counts = p.run_board_counts(company, &scope, cutoff).await.unwrap();
    let late = admitted(&f, "late").await.run;
    let cards = p
        .run_board_cards(company, &scope, cutoff, &visible)
        .await
        .unwrap();
    let board = assemble_columns(&counts, &visible, cards).unwrap();
    let queued = column(&board, RunStateName::Queued);
    assert_eq!(ids(queued), [first, late]);
    assert_eq!((queued.total, queued.cut), (2, false));

    let (counts, _) = read(&f, &scope, cutoff, &visible).await;
    let queued: u64 = counts
        .iter()
        .filter(|count| count.state == RunStateName::Queued)
        .map(|count| count.runs)
        .sum();
    assert_eq!(queued, 2);
}
