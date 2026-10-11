//! Receipt first: whether the completion's attempt bound elapsed while it queued behind the
//! receipt's COMMIT, from pg_stat_activity snapshots taken while it happened.
//!
//! In this order the completion legitimately waits on the run lock behind the receipt's whole
//! transaction. If the receipt's COMMIT stalls (a WAL flush under parallel load), the
//! completion's bound can elapse in that queue: a definite NotApplied that proves nothing
//! about the order. Only that is void, and only when the snapshots show it. A completion
//! blocked by anything else, queued behind the receipt's body, or behind a COMMIT that itself
//! waits on a lock (a possible cycle) still fails.
use super::*;
use std::{cell::Cell, time::Instant};

/// The receipt-first owners' backends.
#[derive(Clone, Copy, Debug)]
pub(super) struct QueuePids {
    pub(super) receipt: i32,
    pub(super) completion: i32,
}

/// One owner backend as pg_stat_activity showed it.
#[derive(Debug, sqlx::FromRow)]
struct Backend {
    pid: i32,
    state: Option<String>,
    query: Option<String>,
    wait_type: Option<String>,
    wait: Option<String>,
    blockers: Vec<i32>,
    /// Identifies the current statement, so two snapshots can show the same COMMIT.
    statement_start_us: Option<i64>,
    /// Age of the current statement: the completion's lock wait, the receipt's COMMIT.
    statement_ms: Option<i64>,
}

/// Both owners in one snapshot, timed from the gate's release.
#[derive(Debug)]
pub(super) struct Sample {
    sent_ms: u128,
    received_ms: u128,
    receipt: Option<Backend>,
    completion: Option<Backend>,
}

const SAMPLE_PAUSE: Duration = Duration::from_millis(5);

async fn sample(gate: &mut sqlx::PgConnection, pids: QueuePids, unlocked: Instant) -> Sample {
    let sent_ms = unlocked.elapsed().as_millis();
    // Autocommit, so every statement reads a fresh activity snapshot.
    let rows: Vec<Backend> = sqlx::query_as("SELECT activity.pid, activity.state, activity.query, activity.wait_event_type AS wait_type, activity.wait_event AS wait, pg_blocking_pids(activity.pid) AS blockers, (extract(epoch FROM activity.query_start)*1000000)::bigint AS statement_start_us, (extract(epoch FROM clock_timestamp()-activity.query_start)*1000)::bigint AS statement_ms FROM pg_stat_activity AS activity WHERE activity.pid=ANY($1)")
        .bind([pids.receipt, pids.completion].as_slice())
        .fetch_all(&mut *gate)
        .await
        .unwrap();
    let received_ms = unlocked.elapsed().as_millis();
    let (mut receipt, mut completion) = (None, None);
    for row in rows {
        if row.pid == pids.receipt {
            receipt = Some(row);
        } else if row.pid == pids.completion {
            completion = Some(row);
        }
    }
    Sample {
        sent_ms,
        received_ms,
        receipt,
        completion,
    }
}

/// Snapshots both owners until the completion has returned, then once more, on the gate's
/// connection, which the gate's release left idle.
pub(super) async fn samples_until(
    gate: &mut sqlx::PgConnection,
    pids: QueuePids,
    unlocked: Instant,
    returned: &Cell<Option<Instant>>,
) -> Vec<Sample> {
    let mut samples = Vec::new();
    loop {
        let after_return = returned.get().is_some();
        samples.push(sample(gate, pids, unlocked).await);
        if after_return {
            return samples;
        }
        tokio::time::sleep(SAMPLE_PAUSE).await;
    }
}

/// The receipt's backend runs COMMIT without waiting on a lock, so it still holds the run lock.
fn committing(receipt: &Backend) -> bool {
    receipt.state.as_deref() == Some("active")
        && receipt.query.as_deref().map(str::trim) == Some("COMMIT")
        && receipt.wait_type.as_deref() != Some("Lock")
}

/// The completion waits on a heavyweight lock the receipt's backend holds.
fn queued_behind(completion: &Backend, pids: QueuePids) -> bool {
    completion.wait_type.as_deref() == Some("Lock") && completion.blockers.contains(&pids.receipt)
}

/// The completion waits on no lock the receipt does not hold. While the receipt still runs
/// its COMMIT the completion cannot have been granted the lock, so it is either still queued
/// or its statement is being cancelled by its own server bound.
fn not_queued_elsewhere(completion: &Backend, pids: QueuePids) -> bool {
    completion.wait_type.as_deref() != Some("Lock") || completion.blockers.contains(&pids.receipt)
}

/// Both owners, when this snapshot shows the receipt running COMMIT.
fn during_commit(sample: &Sample) -> Option<(&Backend, &Backend)> {
    let (Some(receipt), Some(completion)) = (&sample.receipt, &sample.completion) else {
        return None;
    };
    committing(receipt).then_some((receipt, completion))
}

/// The recorded evidence of one void receipt-first attempt, printed on its VOID line.
pub(super) struct CommitQueue {
    error: String,
    returned_ms: u128,
    gap_ms: u128,
    queued_from_ms: u128,
    completion_state: String,
    lock_wait_ms: Option<i64>,
    commit_ms: Option<i64>,
    commit_wait: String,
    samples: usize,
}

impl std::fmt::Display for CommitQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "evidence=commit_queue returned_ms={} gap_ms={} queued_behind_commit_from_ms={} completion_at_last={} completion_statement_ms={:?} receipt_commit_ms={:?} receipt_commit_wait={} samples={} error={:?}",
            self.returned_ms,
            self.gap_ms,
            self.queued_from_ms,
            self.completion_state,
            self.lock_wait_ms,
            self.commit_ms,
            self.commit_wait,
            self.samples,
            self.error
        )
    }
}

/// The void rule's evidence for a completion `DatabaseTimeout` that returned `returned_ms`
/// after the gate's release, from the snapshots that finished before that return:
/// - one shows the completion waiting on a lock the receipt holds while the receipt runs
///   COMMIT;
/// - the last shows that same COMMIT still running, so the lock was never released, and the
///   completion waiting on nothing else;
/// - and the last started within a quarter of the attempt bound of the return.
///
/// Otherwise the timeout is unexplained, and `Err` describes what the snapshots showed.
pub(super) fn commit_queue(
    samples: &[Sample],
    pids: QueuePids,
    returned_ms: u128,
    attempt_bound: Duration,
    error: String,
) -> Result<CommitQueue, String> {
    let before: Vec<&Sample> = samples
        .iter()
        .filter(|sample| sample.received_ms <= returned_ms)
        .collect();
    let queued = before.iter().copied().find(|sample| {
        during_commit(sample).is_some_and(|(_, completion)| queued_behind(completion, pids))
    });
    let stale_ms = attempt_bound.as_millis() / 4;
    let explained = |last: &&Sample| {
        let (Some(queued), Some((receipt, completion))) = (queued, during_commit(last)) else {
            return false;
        };
        let same_commit = queued
            .receipt
            .as_ref()
            .map(|first| first.statement_start_us)
            == Some(receipt.statement_start_us);
        same_commit
            && not_queued_elsewhere(completion, pids)
            && returned_ms - last.sent_ms <= stale_ms
    };
    let Some(last) = before.last().copied().filter(explained) else {
        return Err(format!(
            "pids={pids:?} returned_ms={returned_ms} stale_ms={stale_ms} first queued behind COMMIT: {queued:?}; last before return: {:?}; after return: {:?}",
            before.last(),
            samples.last()
        ));
    };
    let (receipt, completion) = during_commit(last).unwrap();
    Ok(CommitQueue {
        error,
        returned_ms,
        gap_ms: returned_ms - last.sent_ms,
        queued_from_ms: queued.map_or(0, |first| first.sent_ms),
        completion_state: format!(
            "{}:{}",
            completion.state.as_deref().unwrap_or("none"),
            completion.wait_type.as_deref().unwrap_or("none")
        ),
        lock_wait_ms: completion.statement_ms,
        commit_ms: receipt.statement_ms,
        commit_wait: format!(
            "{}:{}",
            receipt.wait_type.as_deref().unwrap_or("none"),
            receipt.wait.as_deref().unwrap_or("none")
        ),
        samples: samples.len(),
    })
}
