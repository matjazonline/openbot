//! Plan `fix-03-run-board-finished-columns` step 1: `workflow_runs.finished_at` is set exactly
//! while a run is terminal. The `workflow_run_finished` trigger stamps it on entering a terminal
//! state, keeps the first stamp while the run stays terminal and clears it on retry, so every
//! writer of `state` is covered without naming the column; `workflow_run_finished_shape` refuses
//! any row where the two disagree.
use super::run_lineage_tests::admitted;
use super::*;
use crate::application::workflow::reads::{RunStateName, WorkflowReads};
use chrono::{DateTime, Utc};

/// `run` moved to `state` by a plain write of the column, as every production writer does; a
/// waiting run waits on a timer.
pub(super) async fn drive(f: &AdmissionFixture, run: RunId, state: RunStateName) {
    sqlx::query(
        "UPDATE workflow_runs SET state = $3, \
         waiting_reason = CASE WHEN $3 = 'waiting' THEN 'timer' END \
         WHERE company_id = $1 AND id = $2",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(run.as_uuid())
    .bind(state.as_str())
    .execute(f.persistence().pool())
    .await
    .unwrap();
}

/// `run`'s `finished_at` and `revision` as stored.
async fn stamp(f: &AdmissionFixture, run: RunId) -> (Option<DateTime<Utc>>, i64) {
    sqlx::query_as("SELECT finished_at, revision FROM workflow_runs WHERE id = $1")
        .bind(run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

/// A write to `run` that must be refused by `workflow_run_finished_shape`.
async fn refused(f: &AdmissionFixture, run: RunId, sql: &str) {
    let error = sqlx::query(sql)
        .bind(run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap_err();
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("23514"), "{sql}");
    assert_eq!(
        error.constraint(),
        Some("workflow_run_finished_shape"),
        "{sql}"
    );
}

/// Success, failure and cancellation each stamp the run once it gets there, after it was created,
/// and the read side carries the stamp.
#[tokio::test]
async fn run_finished_at_is_stamped_on_success_failure_and_cancel() {
    let f = AdmissionFixture::new().await;
    for (key, state) in [
        ("succeeds", RunStateName::Succeeded),
        ("fails", RunStateName::Failed),
        ("is-cancelled", RunStateName::Cancelled),
    ] {
        let run = admitted(&f, key).await.run;
        drive(&f, run, RunStateName::Running).await;
        assert_eq!(stamp(&f, run).await.0, None, "{key}");
        drive(&f, run, state).await;
        let finished = stamp(&f, run).await.0.unwrap_or_else(|| panic!("{key}"));
        let detail = f
            .persistence()
            .run_detail(f.binding.target.company, run)
            .await
            .unwrap()
            .unwrap();
        assert!(finished >= detail.summary.created_at, "{key}");
        assert_eq!(detail.summary.finished_at, Some(finished), "{key}");
    }
}

/// A retry reopens the run and clears the stamp; finishing again stamps it later.
#[tokio::test]
async fn run_finished_at_clears_on_retry_and_restamps_later() {
    let f = AdmissionFixture::new().await;
    let run = admitted(&f, "retried").await.run;
    drive(&f, run, RunStateName::Failed).await;
    let first = stamp(&f, run).await.0.unwrap();
    drive(&f, run, RunStateName::Queued).await;
    assert_eq!(stamp(&f, run).await.0, None);
    drive(&f, run, RunStateName::Running).await;
    drive(&f, run, RunStateName::Failed).await;
    assert!(stamp(&f, run).await.0.unwrap() > first);
}

/// A writer that names the column cannot leave a finishing run unstamped.
#[tokio::test]
async fn run_finished_at_is_stamped_when_a_terminal_write_clears_it() {
    let f = AdmissionFixture::new().await;
    let run = admitted(&f, "cleared").await.run;
    drive(&f, run, RunStateName::Running).await;
    sqlx::query("UPDATE workflow_runs SET state = 'succeeded', finished_at = NULL WHERE id = $1")
        .bind(run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(stamp(&f, run).await.0.is_some());
}

/// While a run stays terminal its first stamp holds, and a write the stamp undoes changes nothing:
/// `workflow_run_finished` fires before `workflow_run_revision`, which therefore sees the row
/// unchanged and leaves the revision alone.
#[tokio::test]
async fn run_finished_at_keeps_the_revision_when_the_stamp_undoes_a_write() {
    let f = AdmissionFixture::new().await;
    let run = admitted(&f, "kept").await.run;
    drive(&f, run, RunStateName::Succeeded).await;
    let finished = stamp(&f, run).await;
    for sql in [
        "UPDATE workflow_runs SET state = 'succeeded', finished_at = NULL WHERE id = $1",
        "UPDATE workflow_runs SET state = 'succeeded', \
         finished_at = finished_at - interval '1 hour' WHERE id = $1",
        "UPDATE workflow_runs SET state = state WHERE id = $1",
    ] {
        sqlx::query(sql)
            .bind(run.as_uuid())
            .execute(f.persistence().pool())
            .await
            .unwrap();
        assert_eq!(stamp(&f, run).await, finished, "{sql}");
    }
}

/// The shape holds whoever writes: an open run cannot carry a stamp, and a finished one cannot
/// drop it, when the write leaves `state` alone and the trigger does not fire.
#[tokio::test]
async fn run_finished_at_shape_rejects_a_stamped_open_run_and_an_unstamped_finished_one() {
    let f = AdmissionFixture::new().await;
    let open = admitted(&f, "open").await.run;
    refused(
        &f,
        open,
        "UPDATE workflow_runs SET finished_at = clock_timestamp() WHERE id = $1",
    )
    .await;
    let finished = admitted(&f, "finished").await.run;
    drive(&f, finished, RunStateName::Failed).await;
    refused(
        &f,
        finished,
        "UPDATE workflow_runs SET finished_at = NULL WHERE id = $1",
    )
    .await;
}

/// `finished_at` is lifecycle state beside the frozen snapshot: with the snapshot guard on, a
/// finishing write that also names a snapshot column unchanged is stamped, and moving the stamp
/// alone is allowed.
#[tokio::test]
async fn run_finished_at_stamps_beside_a_snapshot_column() {
    let f = AdmissionFixture::new().await;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let run = admitted(&f, "guarded").await.run;
    drive(&f, run, RunStateName::Running).await;
    sqlx::query("UPDATE workflow_runs SET state = 'succeeded', deadline = deadline WHERE id = $1")
        .bind(run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let finished = stamp(&f, run).await.0.unwrap();
    sqlx::query(
        "UPDATE workflow_runs SET finished_at = finished_at - interval '1 day' WHERE id = $1",
    )
    .bind(run.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(
        stamp(&f, run).await.0,
        Some(finished - chrono::Duration::days(1))
    );
}

/// The SQL terminal set and `RunStateName::is_open` agree on every state.
#[tokio::test]
async fn run_finished_at_is_set_exactly_in_the_states_that_are_not_open() {
    let f = AdmissionFixture::new().await;
    for state in RunStateName::ALL {
        let run = admitted(&f, state.as_str()).await.run;
        drive(&f, run, state).await;
        assert_eq!(
            stamp(&f, run).await.0.is_none(),
            state.is_open(),
            "{}",
            state.as_str()
        );
    }
}
