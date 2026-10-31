//! Assembling the board's columns from the counts and the cards read.
use super::*;
use crate::application::workflow::RelatedAssociation;
use crate::domain::workflow::{RunId, VersionId, WorkflowId};

fn card(state: RunStateName) -> RunSummary {
    let now = chrono::Utc::now();
    RunSummary {
        run: RunId::new(Uuid::new_v4()),
        binding: WorkflowBindingId::new(Uuid::new_v4()),
        workflow: WorkflowId::new(Uuid::new_v4()),
        version: VersionId::new(Uuid::new_v4()),
        association: RelatedAssociation::Company,
        state: state.as_str().into(),
        waiting_reason: None,
        revision: 1,
        actor: Uuid::new_v4(),
        parent: None,
        rerun_of: None,
        created_at: now,
        deadline: now,
        finished_at: (!state.is_open()).then_some(now),
    }
}

fn cards(state: RunStateName, count: usize) -> Vec<RunSummary> {
    (0..count).map(|_| card(state)).collect()
}

fn count(state: RunStateName, channel: Option<Uuid>, runs: u64) -> BoardCount {
    BoardCount {
        state,
        channel,
        runs,
    }
}

#[test]
fn the_board_has_one_column_per_state_in_order_and_empty_ones_count_nothing() {
    let columns = assemble_columns(&[], &VisibleChannels::default(), Vec::new()).unwrap();
    assert_eq!(
        columns
            .iter()
            .map(|column| column.state)
            .collect::<Vec<_>>(),
        RunStateName::ALL
    );
    assert!(
        columns
            .iter()
            .all(|column| column.cards.is_empty() && column.total == 0 && !column.cut)
    );
}

#[test]
fn a_column_counts_only_the_channels_the_viewer_may_see() {
    let (seen, hidden) = (Uuid::new_v4(), Uuid::new_v4());
    let visible = VisibleChannels {
        channels: vec![seen],
        company: true,
    };
    let counts = [
        count(RunStateName::Failed, Some(seen), 3),
        count(RunStateName::Failed, Some(hidden), 40),
        count(RunStateName::Failed, None, 2),
        count(RunStateName::Queued, Some(hidden), 7),
    ];
    let columns = assemble_columns(&counts, &visible, cards(RunStateName::Failed, 5)).unwrap();
    let failed = &columns[4];
    assert_eq!(
        (failed.state, failed.total, failed.cut),
        (RunStateName::Failed, 5, false)
    );
    assert_eq!(failed.cards.len(), 5);
    assert_eq!(columns[0].total, 0, "the queued runs are all hidden");

    let company_hidden = VisibleChannels {
        channels: vec![seen],
        company: false,
    };
    let columns = assemble_columns(&counts, &company_hidden, Vec::new()).unwrap();
    assert_eq!(columns[4].total, 3);
}

#[test]
fn a_column_past_the_cap_is_cut_and_reads_more_than_it_shows() {
    let read = cards(RunStateName::Running, BOARD_COLUMN_CAP + 1);
    let first = read[0].run;
    let counts = [count(RunStateName::Running, None, 312)];
    let visible = VisibleChannels {
        channels: Vec::new(),
        company: true,
    };
    let columns = assemble_columns(&counts, &visible, read).unwrap();
    let running = &columns[1];
    assert_eq!(running.cards.len(), BOARD_COLUMN_CAP);
    assert_eq!(running.cards[0].run, first, "the read order is kept");
    assert!(running.cut);
    assert_eq!(running.total, 312);

    // A run that landed between the two reads: counted 50, read 51. The column still says it was
    // cut and never "50 of 50".
    let columns = assemble_columns(
        &[count(RunStateName::Running, None, BOARD_COLUMN_CAP as u64)],
        &visible,
        cards(RunStateName::Running, BOARD_COLUMN_CAP + 1),
    )
    .unwrap();
    assert!(columns[1].cut);
    assert_eq!(columns[1].total, BOARD_COLUMN_CAP as u64 + 1);
}

#[test]
fn a_card_in_an_unknown_state_is_an_error_not_a_dropped_card() {
    let mut odd = card(RunStateName::Queued);
    odd.state = "paused".into();
    assert!(assemble_columns(&[], &VisibleChannels::default(), vec![odd]).is_err());
}

#[test]
fn the_windows_reach_back_a_day_a_week_and_thirty_days() {
    assert_eq!(BoardWindow::default(), BoardWindow::Week);
    assert_eq!(BoardWindow::Day.duration(), chrono::Duration::days(1));
    assert_eq!(BoardWindow::Week.duration(), chrono::Duration::days(7));
    assert_eq!(BoardWindow::Month.duration(), chrono::Duration::days(30));
}
