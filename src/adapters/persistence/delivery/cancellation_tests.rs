//! Cancellation against a real database: the intent, and every worker path that meets it.
//!
//! Each test holds the rows it claims, so each takes a database of its own: the claims are global.

use super::*;
use crate::{
    adapters::persistence::{
        delivery::cancellation::{CancelledDeliveries, cancel_task_deliveries_on},
        test_support::wait_until_a_backend_is_blocked,
    },
    entities::{task::NewTask, transport::DeliveryCancellationReason},
    task_queue::TaskPersistence,
};

/// A task to own the deliveries. Its channel has no agent, so the task itself is never claimable.
async fn task_for(persistence: &PostgresPersistence, scope: &Scope) -> Uuid {
    persistence
        .enqueue_task(NewTask::starting_new_chain(
            scope.company.id,
            scope.channel.id,
            None,
            "cancellation_test",
            serde_json::json!({}),
        ))
        .await
        .unwrap()
        .id
}

async fn queue_for_task(
    persistence: &PostgresPersistence,
    scope: &Scope,
    task_id: Uuid,
    key: &str,
    parts: u16,
) -> DeliveryFixture {
    queue(
        persistence,
        scope,
        DeliveryFixtureRequest {
            task_id: Some(task_id),
            parts,
            ..DeliveryFixtureRequest::new(scope.company.id, scope.channel.id, scope.thread_id, key)
        },
    )
    .await
}

async fn claim(persistence: &PostgresPersistence, delivery_id: DeliveryId) -> ClaimedDelivery {
    sort_first(persistence, delivery_id).await;
    claim_mine(persistence, WorkerId::random(), delivery_id)
        .await
        .expect("the backdated row is claimed first")
}

async fn cancel(
    persistence: &PostgresPersistence,
    scope: &Scope,
    task_id: Uuid,
) -> CancelledDeliveries {
    let mut tx = persistence.pool.begin().await.unwrap();
    let cancelled = cancel_task_deliveries_on(
        &mut tx,
        scope.company.id,
        &[task_id],
        DeliveryCancellationReason::ChannelAgentRemoved,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    cancelled
}

async fn reason_of(persistence: &PostgresPersistence, delivery_id: DeliveryId) -> Option<String> {
    sqlx::query_scalar("SELECT cancellation_reason FROM message_deliveries WHERE id = $1")
        .bind(delivery_id.as_uuid())
        .fetch_one(&persistence.pool)
        .await
        .unwrap()
}

async fn class_of(persistence: &PostgresPersistence, delivery_id: DeliveryId) -> Option<String> {
    sqlx::query_scalar("SELECT last_error_class FROM message_deliveries WHERE id = $1")
        .bind(delivery_id.as_uuid())
        .fetch_one(&persistence.pool)
        .await
        .unwrap()
}

fn delivered() -> ProviderSendOutcome {
    ProviderSendOutcome::Delivered { provider_key: None }
}

fn retryable() -> ProviderSendOutcome {
    ProviderSendOutcome::Retryable {
        class: FailureClass::Network,
        detail: detail("the connection was refused"),
    }
}

async fn send(
    persistence: &PostgresPersistence,
    claimed: &ClaimedDelivery,
    part: usize,
    outcome: &ProviderSendOutcome,
) -> DeliveryOutcome {
    assert_eq!(
        persistence
            .begin_part(&claimed.lease, claimed.parts[part].id)
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Sending)
    );
    persistence
        .complete_part(PartResult {
            fence: &claimed.lease,
            part_id: claimed.parts[part].id,
            outcome,
        })
        .await
        .unwrap()
}

/// A queued delivery nobody holds is settled on the spot, keeping every fact about what it sent,
/// and nothing can put it back in the queue afterwards.
#[tokio::test]
async fn a_queued_delivery_is_superseded_and_keeps_its_partial_send() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let task_id = task_for(&persistence, &scope).await;
    let queued = queue_for_task(&persistence, &scope, task_id, "partial", 2).await;
    let id = queued.delivery.id;

    let claimed = claim(&persistence, id).await;
    assert_eq!(
        send(&persistence, &claimed, 0, &delivered()).await,
        DeliveryOutcome::Applied(DeliveryStatus::Sending)
    );
    assert_eq!(
        send(&persistence, &claimed, 1, &retryable()).await,
        DeliveryOutcome::Applied(DeliveryStatus::Retryable)
    );
    let attempts = attempts_of(&persistence, id).await;

    let cancelled = cancel(&persistence, &scope, task_id).await;
    assert_eq!(cancelled.superseded, 1);
    assert!(cancelled.potentially_sent.is_empty());
    assert_eq!(
        status_of(&persistence, id).await,
        DeliveryStatus::DeadLetter
    );
    assert_eq!(
        class_of(&persistence, id).await.as_deref(),
        Some("superseded")
    );
    assert_eq!(
        reason_of(&persistence, id).await.as_deref(),
        Some("channel_agent_removed")
    );
    assert_eq!(
        attempts_of(&persistence, id).await,
        attempts,
        "a cancellation costs no provider call, so it charges no attempt"
    );
    assert_eq!(
        part_statuses(&persistence, id).await,
        vec![DeliveryPartStatus::Delivered, DeliveryPartStatus::Dead],
        "the part that landed keeps its result; only the unsent part is buried"
    );

    assert_eq!(
        cancel(&persistence, &scope, task_id).await,
        CancelledDeliveries::default(),
        "a repeated cancellation finds nothing left to cancel"
    );
    let requeued = sqlx::query("UPDATE message_deliveries SET status = 'retryable' WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&persistence.pool)
        .await;
    assert!(
        requeued.is_err(),
        "a cancelled delivery can never be claimable"
    );
    let rewritten = sqlx::query(
        "UPDATE message_deliveries SET cancellation_reason = 'task_stopped' WHERE id = $1",
    )
    .bind(id.as_uuid())
    .execute(&persistence.pool)
    .await;
    assert!(rewritten.is_err(), "the first cancellation intent stands");
}

/// A worker mid-way through a multi-part send starts nothing further once the intent lands, and
/// the row it releases says exactly what was and was not sent.
#[tokio::test]
async fn a_held_delivery_starts_no_further_part_once_cancelled() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let task_id = task_for(&persistence, &scope).await;
    let queued = queue_for_task(&persistence, &scope, task_id, "held", 2).await;
    let id = queued.delivery.id;
    let claimed = claim(&persistence, id).await;
    send(&persistence, &claimed, 0, &delivered()).await;

    let cancelled = cancel(&persistence, &scope, task_id).await;
    assert_eq!(cancelled.superseded, 0);
    assert_eq!(cancelled.potentially_sent, vec![id]);
    assert_eq!(
        status_of(&persistence, id).await,
        DeliveryStatus::Sending,
        "the intent does not reach past the worker's lease"
    );

    assert_eq!(
        persistence
            .begin_part(&claimed.lease, claimed.parts[1].id)
            .await
            .unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
    assert_eq!(
        part_statuses(&persistence, id).await,
        vec![DeliveryPartStatus::Delivered, DeliveryPartStatus::Dead]
    );
    let started: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT request_started_at FROM message_delivery_parts WHERE id = $1")
            .bind(claimed.parts[1].id.as_uuid())
            .fetch_one(&persistence.pool)
            .await
            .unwrap();
    assert!(
        started.is_none(),
        "no request was started for the buried part"
    );
    assert!(
        !persistence
            .renew_delivery_lease(&claimed.lease, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap(),
        "the settle released the lease rather than stranding it"
    );
}

/// A request already made when the cancellation lands is recorded as the provider answered it:
/// delivered stays delivered, ambiguous stays ambiguous, and a definite failure is not retried.
#[tokio::test]
async fn a_request_already_made_keeps_its_real_outcome() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let task_id = task_for(&persistence, &scope).await;
    let landed = queue_for_task(&persistence, &scope, task_id, "landed", 1).await;
    let refused = queue_for_task(&persistence, &scope, task_id, "refused", 1).await;
    let unclear = queue_for_task(&persistence, &scope, task_id, "unclear", 1).await;
    let mut claims = Vec::new();
    for queued in [&landed, &refused, &unclear] {
        let claimed = claim(&persistence, queued.delivery.id).await;
        persistence
            .begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap();
        claims.push(claimed);
    }

    let cancelled = cancel(&persistence, &scope, task_id).await;
    assert_eq!(cancelled.potentially_sent.len(), 3);

    let complete = |claimed: &ClaimedDelivery, outcome: ProviderSendOutcome| {
        let persistence = &persistence;
        let fence = claimed.lease;
        let part_id = claimed.parts[0].id;
        async move {
            persistence
                .complete_part(PartResult {
                    fence: &fence,
                    part_id,
                    outcome: &outcome,
                })
                .await
                .unwrap()
        }
    };
    assert_eq!(
        complete(&claims[0], delivered()).await,
        DeliveryOutcome::Cancelled(DeliveryStatus::Delivered)
    );
    assert_eq!(
        complete(&claims[1], retryable()).await,
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter),
        "a cancelled delivery is never handed back for a retry"
    );
    assert_eq!(
        complete(
            &claims[2],
            ProviderSendOutcome::OutcomeUnknown {
                class: FailureClass::Timeout,
                detail: detail("the connection dropped after the request went out"),
            },
        )
        .await,
        DeliveryOutcome::Cancelled(DeliveryStatus::OutcomeUnknown)
    );
    assert_eq!(
        class_of(&persistence, unclear.delivery.id).await.as_deref(),
        Some("timeout"),
        "the ambiguity keeps its own classification rather than being filed as a cancellation"
    );
    for queued in [&landed, &refused, &unclear] {
        assert_eq!(
            reason_of(&persistence, queued.delivery.id).await.as_deref(),
            Some("channel_agent_removed")
        );
    }
}

/// Shutdown release, a failure before the provider, and the lease sweep all settle a cancelled
/// delivery instead of returning it to the queue.
#[tokio::test]
async fn release_failure_and_the_lease_sweep_settle_rather_than_requeue() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let task_id = task_for(&persistence, &scope).await;
    let released = queue_for_task(&persistence, &scope, task_id, "released", 1).await;
    let failed = queue_for_task(&persistence, &scope, task_id, "failed", 1).await;
    let started = queue_for_task(&persistence, &scope, task_id, "started", 1).await;
    let stranded = queue_for_task(&persistence, &scope, task_id, "stranded", 1).await;
    let released_claim = claim(&persistence, released.delivery.id).await;
    let failed_claim = claim(&persistence, failed.delivery.id).await;
    let started_claim = claim(&persistence, started.delivery.id).await;
    let stranded_claim = claim(&persistence, stranded.delivery.id).await;
    persistence
        .begin_part(&started_claim.lease, started_claim.parts[0].id)
        .await
        .unwrap();

    cancel(&persistence, &scope, task_id).await;

    assert_eq!(
        persistence
            .release_delivery(&released_claim.lease)
            .await
            .unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
    assert_eq!(
        persistence
            .fail_delivery(DeliveryFailure {
                fence: &failed_claim.lease,
                class: FailureClass::Network,
                detail: detail("the relay was unreachable"),
                disposition: Disposition::Retry,
            })
            .await
            .unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );

    expire(&persistence, started_claim.lease.row).await;
    expire(&persistence, stranded_claim.lease.row).await;
    let reaping = persistence.reap_expired_deliveries().await.unwrap();
    assert_eq!(reaping.leases_expired, 2);
    assert_eq!(
        status_of(&persistence, started.delivery.id).await,
        DeliveryStatus::OutcomeUnknown,
        "a request that went out stays ambiguous, cancelled or not"
    );
    assert_eq!(
        reason_of(&persistence, started.delivery.id)
            .await
            .as_deref(),
        Some("channel_agent_removed")
    );
    assert_eq!(
        status_of(&persistence, stranded.delivery.id).await,
        DeliveryStatus::DeadLetter
    );
    assert_eq!(
        part_statuses(&persistence, stranded.delivery.id).await,
        vec![DeliveryPartStatus::Dead]
    );
    for queued in [&released, &failed, &stranded] {
        assert_eq!(
            class_of(&persistence, queued.delivery.id).await.as_deref(),
            Some("superseded")
        );
    }
}

/// When the cancellation holds the parent row first, a `begin_part` racing it waits, then finds
/// the intent and starts no request -- the ordering the plan requires, proven with a real waiter.
#[tokio::test]
async fn a_begin_part_that_loses_the_parent_lock_starts_no_request() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let persistence = PostgresPersistence::new(pool.clone());
    let scope = scope(&persistence).await;
    let task_id = task_for(&persistence, &scope).await;
    let queued = queue_for_task(&persistence, &scope, task_id, "race", 1).await;
    let claimed = claim(&persistence, queued.delivery.id).await;

    let mut tx = pool.begin().await.unwrap();
    let cancelled = cancel_task_deliveries_on(
        &mut tx,
        scope.company.id,
        &[task_id],
        DeliveryCancellationReason::ChannelAgentRemoved,
    )
    .await
    .unwrap();
    assert_eq!(cancelled.potentially_sent, vec![queued.delivery.id]);

    let racer = {
        let persistence = PostgresPersistence::new(pool.clone());
        let lease = claimed.lease;
        let part_id = claimed.parts[0].id;
        tokio::spawn(async move { persistence.begin_part(&lease, part_id).await.unwrap() })
    };
    wait_until_a_backend_is_blocked(&pool).await;
    tx.commit().await.unwrap();

    assert_eq!(
        racer.await.unwrap(),
        DeliveryOutcome::Cancelled(DeliveryStatus::DeadLetter)
    );
    assert_eq!(
        part_statuses(&persistence, queued.delivery.id).await,
        vec![DeliveryPartStatus::Dead]
    );
}

/// Every cancellation reason Rust can write is one the database accepts.
#[tokio::test]
async fn the_cancellation_reasons_match_their_database_constraint() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let definition: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint
          WHERE conname = 'message_deliveries_cancellation_check'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    for reason in DeliveryCancellationReason::ALL {
        assert!(
            definition.contains(&format!("'{}'", reason.as_str())),
            "SQL rejects the '{reason}' cancellation reason"
        );
    }
}
