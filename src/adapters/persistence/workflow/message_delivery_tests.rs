//! 04.11 delivery-engine effects: workflow-linked `begin_part` rechecks, duplicate callbacks,
//! the reaper's parent locks and the receipt events every producer writes (D2 §4.1, §6; D3 §D).
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::entities::transport::DeliveryStatus;
use crate::transport::{DeliveryOutcome, DeliveryQueue, PartResult, ProviderSendOutcome};
use std::time::Duration;

async fn parked_user(to: &[To]) -> (Setup, Uuid) {
    let setup = Box::pin(send(to)).await;
    parked(&setup).await;
    let delivery = delivery(setup.pool(), setup.scope().run, 0).await;
    (setup, delivery)
}

async fn begin(setup: &Setup, delivery: Uuid) -> DeliveryOutcome {
    let p = &setup.fixture.persistence;
    let claimed = claim(p, delivery).await;
    p.begin_part(&claimed.lease, claimed.parts[0].id)
        .await
        .unwrap()
}

/// No provider entry happened: unsent, no request started, one terminal event.
async fn assert_refused(setup: &Setup, delivery: Uuid, outcome: DeliveryOutcome, reason: &str) {
    assert_eq!(
        outcome,
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
    let row = delivery_row(setup.pool(), delivery).await;
    assert_eq!(
        (
            row.status.as_str(),
            row.attempt_count,
            row.cancellation_reason.as_deref(),
            row.part.as_str()
        ),
        ("dead_letter", 0, Some(reason), "dead")
    );
    let started: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT request_started_at FROM message_delivery_parts WHERE delivery_id=$1",
    )
    .bind(delivery)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert!(
        started.is_none(),
        "a refused part never reached the provider"
    );
    assert_eq!(
        events(setup.pool(), setup.scope().run).await,
        vec![(0, "sending".into(), "dead_letter".into())]
    );
    let p = &setup.fixture.persistence;
    assert_eq!(
        p.consume_message_receipts(setup.scope(), lease())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        action_state(setup.pool(), setup.scope().run).await,
        "failed"
    );
}

#[tokio::test]
async fn workflow_message_delivery_begin_part_refuses_a_recipient_no_longer_a_member() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    sqlx::query("UPDATE users SET email='moved@example.test' WHERE id=$1")
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    let outcome = begin(&setup, delivery).await;
    assert_refused(&setup, delivery, outcome, "recipient_revoked").await;
}

#[tokio::test]
async fn workflow_message_delivery_begin_part_refuses_a_revoked_actor() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    // Ownership moves to the recipient; the run actor is left a plain member.
    sqlx::query("UPDATE companies SET user_id=$2 WHERE id=$1")
        .bind(setup.fixture.target.company.as_uuid())
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(setup.fixture.target.company.as_uuid())
        .bind(setup.owner)
        .execute(setup.pool())
        .await
        .unwrap();
    let outcome = begin(&setup, delivery).await;
    assert_refused(&setup, delivery, outcome, "workflow_revoked").await;
}

#[tokio::test]
async fn workflow_message_delivery_begin_part_refuses_a_run_past_its_deadline() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp() WHERE id=$1")
        .bind(setup.scope().run.as_uuid())
        .execute(setup.pool())
        .await
        .unwrap();
    let outcome = begin(&setup, delivery).await;
    assert_refused(&setup, delivery, outcome, "workflow_expired").await;
}

/// The accepted path, then a duplicate unchanged callback: no status change, no second event.
#[tokio::test]
async fn workflow_message_delivery_duplicate_callback_adds_no_event() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    let p = &setup.fixture.persistence;
    let claimed = claim(p, delivery).await;
    let part = claimed.parts[0].id;
    assert!(
        p.begin_part(&claimed.lease, part)
            .await
            .unwrap()
            .lease_held()
    );
    let accepted = ProviderSendOutcome::Delivered { provider_key: None };
    let result = |outcome| PartResult {
        fence: &claimed.lease,
        part_id: part,
        outcome,
    };
    assert_eq!(
        p.complete_part(result(&accepted)).await.unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Delivered)
    );
    assert_eq!(
        p.complete_part(result(&accepted)).await.unwrap(),
        DeliveryOutcome::LeaseLost
    );
    let rejected = ProviderSendOutcome::Terminal {
        class: crate::entities::transport::FailureClass::InvalidPayload,
        detail: crate::transport::FailureDetail::parse("late duplicate").unwrap(),
    };
    assert_eq!(
        p.complete_part(result(&rejected)).await.unwrap(),
        DeliveryOutcome::LeaseLost
    );
    assert_eq!(
        delivery_row(setup.pool(), delivery).await.status,
        "delivered"
    );
    assert_eq!(
        events(setup.pool(), setup.scope().run).await,
        vec![(0, "sending".into(), "delivered".into())]
    );
    // The SQL guard itself: a part that already took its outcome cannot take another.
    sqlx::query(
        "UPDATE message_delivery_parts SET status='dead', delivered_at=NULL WHERE delivery_id=$1",
    )
    .bind(delivery)
    .execute(setup.pool())
    .await
    .unwrap_err();
}

/// The reaper locks stranded parents first (SKIP LOCKED); a request that started becomes
/// unknown with one event, and the consumer makes the action uncertain.
#[tokio::test]
async fn workflow_message_delivery_reaper_locks_parents_and_marks_started_requests_unknown() {
    let (setup, delivery) = Box::pin(parked_user(&[To::User])).await;
    let p = &setup.fixture.persistence;
    let claimed = claim(p, delivery).await;
    assert!(
        p.begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap()
            .lease_held()
    );
    sqlx::query("UPDATE message_deliveries SET locked_at=clock_timestamp()-interval '2 minutes', lock_expires_at=clock_timestamp()-interval '1 minute' WHERE id=$1")
        .bind(delivery).execute(setup.pool()).await.unwrap();
    // A producer holding the parent: the sweep skips it rather than waiting.
    let mut holder = setup.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM message_deliveries WHERE id=$1 FOR UPDATE")
        .bind(delivery)
        .execute(&mut *holder)
        .await
        .unwrap();
    let reaped = tokio::time::timeout(Duration::from_secs(5), p.reap_expired_deliveries())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reaped.leases_expired, 0);
    holder.rollback().await.unwrap();
    let reaped = p.reap_expired_deliveries().await.unwrap();
    assert_eq!(reaped.leases_expired, 1);
    let row = delivery_row(setup.pool(), delivery).await;
    assert_eq!(
        (row.status.as_str(), row.part.as_str()),
        ("outcome_unknown", "outcome_unknown")
    );
    assert_eq!(
        events(setup.pool(), setup.scope().run).await,
        vec![(0, "sending".into(), "outcome_unknown".into())]
    );
    // Reaper and consumer concurrently: no deadlock, and the truth is consumed once.
    let (reaped, consumed) = tokio::join!(
        p.reap_expired_deliveries(),
        p.consume_message_receipts(setup.scope(), lease())
    );
    reaped.unwrap();
    assert_eq!(consumed.unwrap(), 1);
    assert_eq!(
        action_state(setup.pool(), setup.scope().run).await,
        "uncertain"
    );
}

/// F004: the lower-sequence producer is paused holding its parent while a higher one commits;
/// the consumer blocks on the parent and then observes both.
#[tokio::test]
async fn workflow_message_race_paused_producer_then_consumer_sees_both_events() {
    let _injected = Injected::mark();
    let (setup, first) = Box::pin(parked_user(&[To::User, To::Owner])).await;
    let second = delivery(setup.pool(), setup.scope().run, 1).await;
    let p = setup.fixture.persistence.clone();
    let scope = setup.scope();
    let mut paused = setup.pool().begin().await.unwrap();
    // The paused producer: its parent UPDATE (and so its event) is written, not committed.
    for (status, part) in [("sending", "sending")] {
        sqlx::query("UPDATE message_deliveries SET status=$2, execution_id=gen_random_uuid(), owner_worker_id=gen_random_uuid(), locked_at=clock_timestamp(), lock_expires_at=clock_timestamp()+interval '1 minute' WHERE id=$1")
            .bind(first).bind(status).execute(&mut *paused).await.unwrap();
        sqlx::query("UPDATE message_delivery_parts SET status=$2, request_started_at=clock_timestamp(), attempt_count=1 WHERE delivery_id=$1")
            .bind(first).bind(part).execute(&mut *paused).await.unwrap();
    }
    sqlx::query("UPDATE message_delivery_parts SET status='delivered', delivered_at=clock_timestamp() WHERE delivery_id=$1")
        .bind(first).execute(&mut *paused).await.unwrap();
    sqlx::query("UPDATE message_deliveries SET status='delivered', delivered_at=clock_timestamp(), attempt_count=1, execution_id=NULL, owner_worker_id=NULL, locked_at=NULL, lock_expires_at=NULL WHERE id=$1")
        .bind(first).execute(&mut *paused).await.unwrap();
    deliver(setup.pool(), second, "delivered").await;
    let consumer = tokio::spawn(async move { p.consume_message_receipts(scope, lease()).await });
    // The consumer is blocked behind the paused producer's parent lock, not finished early.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !consumer.is_finished(),
        "the consumer waits on the paused producer's parent"
    );
    paused.commit().await.unwrap();
    assert_eq!(consumer.await.unwrap().unwrap(), 1);
    assert_eq!(
        action_state(setup.pool(), setup.scope().run).await,
        "accepted"
    );
    assert_eq!(events(setup.pool(), setup.scope().run).await.len(), 2);
    assert_eq!(rows(setup.pool(), setup.scope().run).await.receipts, 1);
}
