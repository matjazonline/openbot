//! 04.11-D3 §K (R006 F016.5): settlement competing with the consumer, `begin_part`, another
//! settlement and the workflow cancellation. Real competing transactions: a separate connection
//! holds the run row until every competitor is queued behind it (observed in `pg_locks`, never
//! slept for), then releases it so they race. Marked `Injected`, outside the lease filters.
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    connect, waiters,
};
use sqlx::PgConnection;

/// The run row, held FOR UPDATE by a connection outside every competitor.
struct RunHold {
    conn: PgConnection,
    pid: i32,
    observer: PgConnection,
}
impl RunHold {
    async fn take(setup: &Setup) -> Self {
        let (mut conn, pid) = connect(setup).await;
        let (observer, _) = connect(setup).await;
        sqlx::raw_sql("BEGIN").execute(&mut conn).await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
            .bind(setup.scope().run.as_uuid())
            .execute(&mut conn)
            .await
            .unwrap();
        Self {
            conn,
            pid,
            observer,
        }
    }
    /// Releases the run once `n` competitors wait behind it, directly or transitively.
    async fn release_behind(mut self, n: usize, what: &str) {
        waiters(&mut self.observer, self.pid, n, what, &[]).await;
        sqlx::raw_sql("ROLLBACK")
            .execute(&mut self.conn)
            .await
            .unwrap();
    }
}

async fn consume_failures(setup: &Setup) -> i16 {
    sqlx::query_scalar("SELECT consume_failures FROM workflow_message_actions WHERE run_id=$1")
        .bind(setup.scope().run.as_uuid())
        .fetch_one(setup.pool())
        .await
        .unwrap()
}

async fn reopens(setup: &Setup) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_message_delivery_reopens WHERE run_id=$1")
        .bind(setup.scope().run.as_uuid())
        .fetch_one(setup.pool())
        .await
        .unwrap()
}

/// Applied truth for an unknown delivery and the consumer of the same action's pending events.
/// Either order converges: one receipt, accepted, and the step resumes.
#[tokio::test]
async fn workflow_message_race_settle_and_consumer_compete_for_one_action() {
    let _injected = Injected::mark();
    let setup = Box::pin(send(&[To::Owner, To::User])).await;
    parked(&setup).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    let (accepted, unknown) = (
        delivery(setup.pool(), run, 0).await,
        delivery(setup.pool(), run, 1).await,
    );
    deliver(setup.pool(), accepted, "delivered").await;
    send_unknown(&setup, unknown).await;
    ledger(
        &setup,
        unknown,
        "applied",
        Some(Utc::now() - chrono::Duration::milliseconds(1)),
        60.0,
        None,
    )
    .await;
    let command = command(&setup, unknown, "race-applied", verified()).await;
    let settlement = service(&setup, true);
    let hold = RunHold::take(&setup).await;
    let (settled, consumed, ()) = tokio::join!(
        reconcile(&settlement, &command),
        p.consume_message_receipts(scope, lease()),
        hold.release_behind(2, "settlement and consumer"),
    );
    assert_eq!(
        settled.unwrap().outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: false }
    );
    assert_eq!(consumed.unwrap(), 1);
    assert_eq!(consume_failures(&setup).await, 0, "no competitor failed");
    // Whatever the order, one more visit sees the newest event.
    p.consume_message_receipts(scope, lease()).await.unwrap();
    assert_eq!(action_state(setup.pool(), run).await, "accepted");
    assert_eq!(rows(setup.pool(), run).await.receipts, 1);
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Completed(_)
    ));
}

/// A reopening settlement of one delivery, the consumer of its events and `begin_part` of an
/// unsent sibling, all queued on the run at once: no deadlock, each applies exactly once.
#[tokio::test]
async fn workflow_message_race_settle_consumer_and_begin_part_compete() {
    let _injected = Injected::mark();
    let setup = Box::pin(send(&[To::Owner, To::User])).await;
    parked(&setup).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    let (sibling, unknown) = (
        delivery(setup.pool(), run, 0).await,
        delivery(setup.pool(), run, 1).await,
    );
    send_unknown(&setup, unknown).await;
    ledger(&setup, unknown, "not_applied", None, 60.0, None).await;
    let command = command(&setup, unknown, "race-reopen", verified()).await;
    let settlement = service(&setup, true);
    let claimed = claim(p, sibling).await;
    let hold = RunHold::take(&setup).await;
    let (settled, consumed, began, ()) = tokio::join!(
        reconcile(&settlement, &command),
        p.consume_message_receipts(scope, lease()),
        p.begin_part(&claimed.lease, claimed.parts[0].id),
        hold.release_behind(3, "settlement, consumer and begin_part"),
    );
    assert_eq!(
        settled.unwrap().outcome,
        ReconciliationOutcome::DeliveryReopened
    );
    assert_eq!(consumed.unwrap(), 1);
    assert_eq!(
        began.unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Sending)
    );
    assert_eq!(consume_failures(&setup).await, 0);
    assert_eq!(reopens(&setup).await, 1);
    let reopened = delivery_row(setup.pool(), unknown).await;
    assert_eq!(
        (
            reopened.status.as_str(),
            reopened.part.as_str(),
            reopened.attempt_count
        ),
        ("retryable", "retryable", 1)
    );
    let sending = delivery_row(setup.pool(), sibling).await;
    assert_eq!(
        (sending.status.as_str(), sending.part.as_str()),
        ("sending", "sending")
    );
    p.consume_message_receipts(scope, lease()).await.unwrap();
    assert_eq!(action_state(setup.pool(), run).await, "pending");
    assert_eq!(
        state(setup.pool(), scope).await.wait.as_deref(),
        Some("waiting")
    );
}

/// Two settlements of the same unknown delivery from one snapshot revision: exactly one
/// reopens it; the other is refused without a second grant.
#[tokio::test]
async fn workflow_message_race_competing_settlements_reopen_once() {
    let _injected = Injected::mark();
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    ledger(&setup, unknown, "not_applied", None, 60.0, None).await;
    let first = command(&setup, unknown, "race-first", verified()).await;
    let second = command(&setup, unknown, "race-second", verified()).await;
    let (a, b) = (service(&setup, true), service(&setup, true));
    let hold = RunHold::take(&setup).await;
    let (one, two, ()) = tokio::join!(
        reconcile(&a, &first),
        reconcile(&b, &second),
        hold.release_behind(2, "two settlements"),
    );
    let outcomes = [one, two].map(|result| result.map(|result| result.outcome));
    let reopened = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok(ReconciliationOutcome::DeliveryReopened)))
        .count();
    assert_eq!(reopened, 1, "{outcomes:?}");
    assert!(
        outcomes.iter().all(|outcome| matches!(
            outcome,
            Ok(ReconciliationOutcome::DeliveryReopened
                | ReconciliationOutcome::RevisionConflict
                | ReconciliationOutcome::Blocked {
                    reason: ReconciliationBlockedReason::StaleSnapshot
                })
                | Err(_)
        )),
        "{outcomes:?}"
    );
    assert_eq!(reopens(&setup).await, 1);
    let row = delivery_row(setup.pool(), unknown).await;
    assert_eq!((row.status.as_str(), row.attempt_count), ("retryable", 1));
}

/// A not-applied settlement racing the run's cancellation. The loser of the revision refuses
/// cleanly and its retry converges: the delivery is superseded with `workflow_cancelled`, the run
/// is cancelled, and no provider request starts.
#[tokio::test]
async fn workflow_message_race_settle_racing_workflow_cancellation_supersedes() {
    let _injected = Injected::mark();
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let (p, scope) = (&setup.fixture.persistence, setup.scope());
    let started: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT request_started_at FROM message_delivery_parts WHERE delivery_id=$1",
    )
    .bind(unknown)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    ledger(&setup, unknown, "not_applied", None, 60.0, None).await;
    let command = command(&setup, unknown, "race-settle", verified()).await;
    let settlement = service(&setup, true);
    let cancel = |key: &'static str, revision| CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: setup.fixture.target.actor,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: revision,
    };
    let head = p.head(scope.company, scope.run).await.unwrap().unwrap();
    let hold = RunHold::take(&setup).await;
    let (settled, cancelled, ()) = tokio::join!(
        reconcile(&settlement, &command),
        p.cancel(cancel("race-cancel", head.revision)),
        hold.release_behind(2, "settlement and cancellation"),
    );
    let settled = settled.unwrap().outcome;
    // The revision loser retries once against the current revision.
    if matches!(cancelled.unwrap(), CancelResult::RevisionConflict { .. }) {
        let head = p.head(scope.company, scope.run).await.unwrap().unwrap();
        assert!(matches!(
            p.cancel(cancel("race-cancel-retry", head.revision))
                .await
                .unwrap(),
            CancelResult::Applied { .. }
        ));
    }
    if settled == ReconciliationOutcome::RevisionConflict {
        let retry = super::command(&setup, unknown, "race-settle-retry", verified()).await;
        assert_eq!(
            reconcile(&service(&setup, true), &retry)
                .await
                .unwrap()
                .outcome,
            ReconciliationOutcome::NotAppliedRecorded
        );
    } else {
        assert!(
            matches!(
                settled,
                ReconciliationOutcome::DeliveryReopened | ReconciliationOutcome::NotAppliedRecorded
            ),
            "{settled:?}"
        );
    }
    assert_eq!(state(setup.pool(), scope).await.run, "cancelled");
    let row = delivery_row(setup.pool(), unknown).await;
    assert_eq!(
        (
            row.status.as_str(),
            row.part.as_str(),
            row.cancellation_reason.as_deref(),
            row.attempt_count
        ),
        ("dead_letter", "dead", Some("workflow_cancelled"), 1)
    );
    let after: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT request_started_at FROM message_delivery_parts WHERE delivery_id=$1",
    )
    .bind(unknown)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!(after, started, "no provider request started");
    assert!(reopens(&setup).await <= 1);
}
