//! Database-backed inbox protocol tests.
//!
//! The queue is intentionally global, so every test holds `UNSCOPED_CLAIM` from before it stores
//! a row through cleanup. Competing claims are the behavior under test, not interference between
//! otherwise unrelated test cases.

use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::persistence::{PostgresPersistence, test_support::UNSCOPED_CLAIM},
    entities::{
        correlation::CorrelationId,
        transport::{ExternalEventKey, InboundEventErrorClass, InboundEventIgnoreReason},
    },
    transport::{
        AuthenticatedInboundEvent, InboundContentType, InboundEventFailure, InboundEventInbox,
        InboundEventPayload, InboundEventQueue, InboundEventTransition, InboundFailureDetail,
        SafeHeaderFacts, WorkerId,
    },
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

struct Scope {
    company_id: Uuid,
    owner_id: Uuid,
}

async fn scope(persistence: &PostgresPersistence) -> Scope {
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("inbound_owner_{suffix}");
    let email = format!("{username}@example.test");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Inbound Inbox Test".into(),
            slug: format!("inbound-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    Scope {
        company_id: company.id,
        owner_id: owner.id,
    }
}

fn event(company_id: Uuid, key: &str) -> AuthenticatedInboundEvent {
    AuthenticatedInboundEvent {
        transport: TransportKind::Email,
        company_id,
        installation_id: None,
        external_event_key: ExternalEventKey::parse(key).unwrap(),
        correlation_id: CorrelationId::new(),
        payload: InboundEventPayload::parse(br#"{"type":"message"}"#.to_vec()).unwrap(),
        content_type: Some(InboundContentType::parse("application/json").unwrap()),
        safe_header_facts: SafeHeaderFacts::default(),
        received_at: Utc::now(),
    }
}

async fn cleanup(persistence: &PostgresPersistence, scope: Scope) {
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(scope.company_id)
        .execute(persistence.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(scope.owner_id)
        .execute(persistence.pool())
        .await
        .unwrap();
}

async fn claim_one(
    persistence: &PostgresPersistence,
    event_id: InboundEventId,
) -> ClaimedInboundEvent {
    sqlx::query(
        "UPDATE inbound_events SET available_at = CURRENT_TIMESTAMP - INTERVAL '10 years' \
         WHERE id = $1",
    )
    .bind(event_id.as_uuid())
    .execute(persistence.pool())
    .await
    .unwrap();
    persistence
        .claim_inbound_events(WorkerId::random(), Duration::from_secs(120), 1)
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("the event is ready")
}

#[tokio::test]
async fn simultaneous_authenticated_stores_create_one_row_and_both_succeed() {
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let key = format!("event-{}", Uuid::new_v4());
    let first = event(scope.company_id, &key);
    let second = first.clone();

    let (left, right) = tokio::join!(
        persistence.store_authenticated(first),
        persistence.store_authenticated(second)
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_eq!(left.event_id(), right.event_id());
    assert_ne!(left.was_stored(), right.was_stored());

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inbound_events WHERE transport = 'email' \
         AND external_event_key = $1",
    )
    .bind(&key)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
    cleanup(&persistence, scope).await;
}

#[tokio::test]
async fn simultaneous_claimants_receive_disjoint_events() {
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let first = persistence
        .store_authenticated(event(
            scope.company_id,
            &format!("event-{}", Uuid::new_v4()),
        ))
        .await
        .unwrap()
        .event_id();
    let second = persistence
        .store_authenticated(event(
            scope.company_id,
            &format!("event-{}", Uuid::new_v4()),
        ))
        .await
        .unwrap()
        .event_id();
    sqlx::query(
        "UPDATE inbound_events SET available_at = CURRENT_TIMESTAMP - INTERVAL '10 years' \
         WHERE id = ANY($1)",
    )
    .bind(&[first.as_uuid(), second.as_uuid()][..])
    .execute(persistence.pool())
    .await
    .unwrap();

    let (left, right) = tokio::join!(
        persistence.claim_inbound_events(WorkerId::random(), Duration::from_secs(120), 1),
        persistence.claim_inbound_events(WorkerId::random(), Duration::from_secs(120), 1)
    );
    let claimed = left
        .unwrap()
        .into_iter()
        .chain(right.unwrap())
        .collect::<Vec<_>>();
    assert_eq!(claimed.len(), 2);
    assert_ne!(claimed[0].record.id, claimed[1].record.id);
    assert_eq!(
        claimed
            .iter()
            .map(|event| event.record.id)
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from([first, second])
    );
    cleanup(&persistence, scope).await;
}

#[tokio::test]
async fn every_transition_rejects_a_stale_execution_fence() {
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let stored = persistence
        .store_authenticated(event(
            scope.company_id,
            &format!("event-{}", Uuid::new_v4()),
        ))
        .await
        .unwrap()
        .event_id();
    let claimed = claim_one(&persistence, stored).await;
    let stale = ExecutionLease::new(
        claimed.record.id,
        claimed.lease.owner,
        Utc::now() + chrono::Duration::minutes(5),
    );
    let detail = || InboundFailureDetail::parse("stale transition").unwrap();

    assert!(
        !persistence
            .renew_inbound_event_lease(&stale, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    assert_eq!(
        persistence.complete_inbound_event(&stale).await.unwrap(),
        InboundEventTransition::LeaseLost
    );
    assert_eq!(
        persistence
            .ignore_inbound_event(&stale, InboundEventIgnoreReason::NotMessage)
            .await
            .unwrap(),
        InboundEventTransition::LeaseLost
    );
    assert_eq!(
        persistence
            .retry_inbound_event(InboundEventFailure {
                fence: &stale,
                class: InboundEventErrorClass::Internal,
                detail: detail(),
            })
            .await
            .unwrap(),
        InboundEventTransition::LeaseLost
    );
    assert_eq!(
        persistence
            .dead_letter_inbound_event(InboundEventFailure {
                fence: &stale,
                class: InboundEventErrorClass::InvalidPayload,
                detail: detail(),
            })
            .await
            .unwrap(),
        InboundEventTransition::LeaseLost
    );
    assert_eq!(
        persistence
            .complete_inbound_event(&claimed.lease)
            .await
            .unwrap(),
        InboundEventTransition::Applied(InboundEventStatus::Completed)
    );
    cleanup(&persistence, scope).await;
}

#[tokio::test]
async fn retry_backoff_prevents_hot_reclaim_and_poison_becomes_a_dead_letter() {
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let stored = persistence
        .store_authenticated(event(
            scope.company_id,
            &format!("event-{}", Uuid::new_v4()),
        ))
        .await
        .unwrap()
        .event_id();
    // The retry budget is the column default, and `MAX_INBOUND_EVENT_ATTEMPTS` is how the rest of
    // the application states the same bound. Asserting them equal here is what stops the two
    // drifting apart silently, since nothing else reads the constant at runtime.
    let max_attempts: i32 =
        sqlx::query_scalar("SELECT max_attempts FROM inbound_events WHERE id = $1")
            .bind(stored.as_uuid())
            .fetch_one(persistence.pool())
            .await
            .unwrap();
    assert_eq!(max_attempts, crate::transport::MAX_INBOUND_EVENT_ATTEMPTS);

    let claimed = claim_one(&persistence, stored).await;
    let outcome = persistence
        .retry_inbound_event(InboundEventFailure {
            fence: &claimed.lease,
            class: InboundEventErrorClass::ProviderFault,
            detail: InboundFailureDetail::parse("temporary provider fault").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(
        outcome,
        InboundEventTransition::Applied(InboundEventStatus::Retryable)
    );
    let immediate = persistence
        .claim_inbound_events(WorkerId::random(), Duration::from_secs(120), 8)
        .await
        .unwrap();
    assert!(immediate.iter().all(|event| event.record.id != stored));

    // Put the poison event on its final attempt and simulate a worker disappearing. The reaper
    // must charge that lost lease and make it operator-visible instead of recycling it forever.
    sqlx::query(
        r#"UPDATE inbound_events
              SET status = 'processing', attempt_count = max_attempts - 1,
                  available_at = CURRENT_TIMESTAMP,
                  execution_id = gen_random_uuid(), owner_worker_id = gen_random_uuid(),
                  locked_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes',
                  lock_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 minute',
                  last_error_class = NULL, last_error_detail = NULL
            WHERE id = $1"#,
    )
    .bind(stored.as_uuid())
    .execute(persistence.pool())
    .await
    .unwrap();
    let reaped = persistence.reap_expired_inbound_events().await.unwrap();
    assert_eq!(reaped.leases_expired, 1);
    let status: String = sqlx::query_scalar("SELECT status FROM inbound_events WHERE id = $1")
        .bind(stored.as_uuid())
        .fetch_one(persistence.pool())
        .await
        .unwrap();
    assert_eq!(status, "dead_letter");
    cleanup(&persistence, scope).await;
}

#[tokio::test]
async fn installed_transports_require_the_matching_company_installation() {
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let result = sqlx::query(
        r#"INSERT INTO inbound_events
               (id, company_id, transport, external_event_key, correlation_id, raw_payload,
                content_hash, received_at)
           VALUES ($1, $2, 'slack', $3, $4, 'x'::BYTEA, $5, CURRENT_TIMESTAMP)"#,
    )
    .bind(Uuid::new_v4())
    .bind(scope.company_id)
    .bind(format!("event-{}", Uuid::new_v4()))
    .bind(Uuid::new_v4())
    .bind(vec![0_u8; 32])
    .execute(persistence.pool())
    .await;
    assert!(result.is_err());
    cleanup(&persistence, scope).await;
}

/// The trigger, its channel name, and the listener that consumes it have to agree, and nothing
/// else in the code path fails loudly when they drift: the worker just falls back to polling and
/// nobody notices the wake-up went missing.
#[tokio::test]
async fn storing_an_event_announces_it_on_the_channel_the_listener_subscribes_to() {
    let _claim_guard = UNSCOPED_CLAIM.lock().await;
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool.clone());
    let scope = scope(&persistence).await;

    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap();
    listener.listen("inbound_event_ready").await.unwrap();
    let key = format!("event-{}", Uuid::new_v4());
    let stored = persistence
        .store_authenticated(event(scope.company_id, &key))
        .await
        .unwrap()
        .event_id();

    // `LISTEN` is database-wide, so filter to this test's row exactly as a shared listener must.
    let announced = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let notification = listener.recv().await.unwrap();
            if notification.payload() == stored.as_uuid().to_string() {
                return;
            }
        }
    })
    .await;
    assert!(announced.is_ok(), "the stored event was never announced");

    cleanup(&persistence, scope).await;
}
