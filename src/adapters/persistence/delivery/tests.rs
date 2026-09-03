//! The delivery queue protocol, against a real database.
//!
//! Every test here is about a race or a crash, because those are the only failures the protocol
//! exists for. A queue that works when nothing goes wrong is a `Vec`.
//!
//! Two rules govern how these are written, both from `src/adapters/persistence/AGENTS.md`. The
//! claim is *global* -- it sweeps the whole table, because that is what a worker does -- so a test
//! that calls it backdates its own row to sort first and hands back anything else it caught.
//! And nothing asserts a total: another test's rows are always in the table.

use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        test_support::{
            DeliveryFixture, DeliveryFixtureRequest, UNSCOPED_CLAIM, delivery_fixture, test_pool,
        },
    },
    entities::{
        transport::{
            DeliveryPartStatus, DeliveryPurpose, DeliveryStatus, ExternalDestination,
            ExternalMessageKey, FailureClass, TransportKind,
        },
        value_objects::EmailAddress,
    },
    transport::{
        ClaimedDelivery, ContentDigest, DeliveryFailure, DeliveryKey, DeliveryOutcome,
        DeliveryQueue, Disposition, ExecutionLease, FailureDetail, MAX_DELIVERY_ATTEMPTS,
        NewDelivery, NewStandaloneDelivery, PartIndex, PartKey, PartResult, ProviderSendOutcome,
        RenderedPart, StandaloneDeliveryEnqueuer, TransportPayload, WorkerId,
    },
    use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::ThreadPersistence,
        user::UserPersistence,
    },
};

/// A company, a channel with its canonical email interface, and a thread to hang messages on.
struct Scope {
    company: crate::entities::company::Company,
    channel: crate::entities::channel::Channel,
    thread_id: Uuid,
}

async fn scope(persistence: &PostgresPersistence) -> Scope {
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("delivery_owner_{suffix}");
    let email = format!("{username}@example.com");
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
            name: "Delivery Test".to_string(),
            slug: format!("delivery-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Delivery".into(),
            slug: "delivery".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let thread = ThreadPersistence::create_thread(persistence, channel.id, "Delivery", &[])
        .await
        .unwrap();
    Scope {
        company,
        channel,
        thread_id: thread.id,
    }
}

/// One queued delivery, written through the same insert every producer uses.
async fn queue(
    persistence: &PostgresPersistence,
    scope: &Scope,
    request: DeliveryFixtureRequest<'_>,
) -> DeliveryFixture {
    let fixture = delivery_fixture(persistence, request).await;
    let mut tx = persistence.pool.begin().await.unwrap();
    enqueue::insert_delivery_on(&mut tx, &fixture.delivery)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let _ = scope;
    fixture
}

/// Sort one delivery to the very front of the claim's ordering.
///
/// The claim orders by `(available_at, id)`, so a row dated well into the past is taken first by a
/// `LIMIT 1` claim. That is what makes a global claim deterministic for one test without
/// serialising the whole suite.
async fn sort_first(persistence: &PostgresPersistence, delivery_id: DeliveryId) {
    sqlx::query("UPDATE message_deliveries SET available_at = $2 WHERE id = $1")
        .bind(delivery_id.as_uuid())
        .bind(Utc::now() - chrono::Duration::days(3650))
        .execute(&persistence.pool)
        .await
        .unwrap();
}

/// Put a delivery beyond every claim's horizon, so a neighbouring test cannot take it.
async fn park(persistence: &PostgresPersistence, delivery_id: DeliveryId) {
    sqlx::query("UPDATE message_deliveries SET available_at = $2 WHERE id = $1")
        .bind(delivery_id.as_uuid())
        .bind(Utc::now() + chrono::Duration::days(3650))
        .execute(&persistence.pool)
        .await
        .unwrap();
}

/// Claim exactly the row this test owns, releasing anything else the global claim caught.
///
/// Leaving a foreign row leased for two minutes makes somebody else's test fail several files
/// away, which `src/adapters/persistence/AGENTS.md` names as the classic shared-database mistake.
async fn claim_mine(
    persistence: &PostgresPersistence,
    owner: WorkerId,
    delivery_id: DeliveryId,
) -> Option<ClaimedDelivery> {
    let claimed = persistence
        .claim_deliveries(owner, Duration::from_secs(120), 1)
        .await
        .unwrap();
    let mut mine = None;
    for delivery in claimed {
        if delivery.record.id == delivery_id {
            mine = Some(delivery);
        } else {
            release_foreign(persistence, delivery.lease.row).await;
        }
    }
    mine
}

async fn release_foreign(persistence: &PostgresPersistence, delivery_id: DeliveryId) {
    sqlx::query(
        "UPDATE message_deliveries
            SET status = 'pending', execution_id = NULL, owner_worker_id = NULL,
                locked_at = NULL, lock_expires_at = NULL
          WHERE id = $1",
    )
    .bind(delivery_id.as_uuid())
    .execute(&persistence.pool)
    .await
    .unwrap();
}

async fn status_of(persistence: &PostgresPersistence, delivery_id: DeliveryId) -> DeliveryStatus {
    let stored: String = sqlx::query_scalar("SELECT status FROM message_deliveries WHERE id = $1")
        .bind(delivery_id.as_uuid())
        .fetch_one(&persistence.pool)
        .await
        .unwrap();
    DeliveryStatus::from_str(&stored).unwrap()
}

async fn attempts_of(persistence: &PostgresPersistence, delivery_id: DeliveryId) -> i32 {
    sqlx::query_scalar("SELECT attempt_count FROM message_deliveries WHERE id = $1")
        .bind(delivery_id.as_uuid())
        .fetch_one(&persistence.pool)
        .await
        .unwrap()
}

async fn part_statuses(
    persistence: &PostgresPersistence,
    delivery_id: DeliveryId,
) -> Vec<DeliveryPartStatus> {
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM message_delivery_parts WHERE delivery_id = $1 ORDER BY part_index",
    )
    .bind(delivery_id.as_uuid())
    .fetch_all(&persistence.pool)
    .await
    .unwrap();
    stored
        .iter()
        .map(|status| DeliveryPartStatus::from_str(status).unwrap())
        .collect()
}

fn detail(message: &str) -> FailureDetail {
    FailureDetail::parse(message).expect("a short test detail")
}

fn standalone(key: DeliveryKey) -> NewStandaloneDelivery {
    NewStandaloneDelivery {
        id: DeliveryId::random(),
        external_destination: ExternalDestination::Email(EmailAddress::from(
            "rejected-sender@example.com",
        )),
        correlation_id: crate::entities::correlation::CorrelationId::new(),
        transport: TransportKind::Email,
        purpose: DeliveryPurpose::Notification,
        idempotency_key: key.clone(),
        max_attempts: MAX_DELIVERY_ATTEMPTS,
        parts: NewDelivery::frozen_parts(vec![RenderedPart {
            index: PartIndex::new(0),
            key: PartKey::parse(format!("email:{key}")).unwrap(),
            payload: TransportPayload::encode(
                TransportKind::Email,
                1,
                &serde_json::json!({ "body": "bounce" }),
            )
            .unwrap(),
            digest: ContentDigest::sha256_of(b"bounce"),
        }])
        .unwrap(),
    }
}

/// Standalone notification rows use the same unique-key and claim protocol as attributed rows.
#[tokio::test]
async fn competing_standalone_notification_enqueues_create_one_claimable_delivery() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let key = DeliveryKey::parse(format!("notification:bounce:{}", Uuid::new_v4())).unwrap();
    let first = standalone(key.clone());
    let second = standalone(key);

    let (first_result, second_result) = tokio::join!(
        persistence.enqueue_standalone_delivery(first),
        persistence.enqueue_standalone_delivery(second),
    );
    let results = [first_result.unwrap(), second_result.unwrap()];
    assert_eq!(
        results.iter().filter(|result| result.was_created()).count(),
        1
    );
    assert_eq!(results[0].delivery_id(), results[1].delivery_id());
    let delivery_id = results[0].delivery_id();

    let _guard = UNSCOPED_CLAIM.lock().await;
    sort_first(&persistence, delivery_id).await;
    let claimed = claim_mine(&persistence, WorkerId::random(), delivery_id)
        .await
        .expect("the standalone notification is claimable");
    assert!(claimed.record.attribution.is_none());
    assert_eq!(
        claimed
            .record
            .external_destination
            .as_ref()
            .map(|value| value.as_str()),
        Some("rejected-sender@example.com")
    );
    assert_eq!(claimed.parts.len(), 1);

    sqlx::query("DELETE FROM message_deliveries WHERE id = $1")
        .bind(delivery_id.as_uuid())
        .execute(&persistence.pool)
        .await
        .unwrap();
    let parts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM message_delivery_parts WHERE delivery_id = $1")
            .bind(delivery_id.as_uuid())
            .fetch_one(&persistence.pool)
            .await
            .unwrap();
    assert_eq!(
        parts, 0,
        "deleting the standalone parent cascades to its parts"
    );
}

/// Two logical deliveries of the same thing collapse onto one row; two *different* recipients of
/// the same message do not.
///
/// This is the whole reason the destination is part of the key rather than only of the row: without
/// it, one outreach recipient would silently never be written.
#[tokio::test]
async fn one_key_per_destination_absorbs_a_repeat_and_keeps_two_recipients_apart() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;

    let first = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest {
            recipient: "one@example.com",
            purpose: DeliveryPurpose::Outreach,
            ..DeliveryFixtureRequest::new(
                scope.company.id,
                scope.channel.id,
                scope.thread_id,
                "ask",
            )
        },
    )
    .await;
    park(&persistence, first.delivery.id).await;

    // Same source, same message, a different recipient: a second row.
    let second = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest {
            recipient: "two@example.com",
            purpose: DeliveryPurpose::Outreach,
            ..DeliveryFixtureRequest::new(
                scope.company.id,
                scope.channel.id,
                scope.thread_id,
                "ask",
            )
        },
    )
    .await;
    park(&persistence, second.delivery.id).await;
    assert_ne!(first.delivery.id, second.delivery.id);

    // The same logical delivery again -- a retried planning step -- is absorbed onto the first.
    let mut tx = persistence.pool.begin().await.unwrap();
    let repeat = enqueue::insert_delivery_on(
        &mut tx,
        &crate::transport::NewDelivery {
            id: DeliveryId::random(),
            ..first.delivery.clone()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        repeat,
        crate::transport::DeliveryCreation::Absorbed(first.delivery.id),
        "a repeat must attach to the delivery that already exists"
    );

    let parts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM message_delivery_parts WHERE delivery_id = $1")
            .bind(first.delivery.id.as_uuid())
            .fetch_one(&persistence.pool)
            .await
            .unwrap();
    assert_eq!(parts, 1, "the absorbed insert must not re-freeze the parts");

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// Two claimants, one row. The loser must come away with nothing rather than a second lease.
#[tokio::test]
async fn two_claimants_never_own_the_same_delivery() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let _guard = UNSCOPED_CLAIM.lock().await;

    let queued = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(scope.company.id, scope.channel.id, scope.thread_id, "race"),
    )
    .await;
    sort_first(&persistence, queued.delivery.id).await;

    let first_worker = WorkerId::random();
    let second_worker = WorkerId::random();
    let (first, second) = tokio::join!(
        persistence.claim_deliveries(first_worker, Duration::from_secs(120), 1),
        persistence.claim_deliveries(second_worker, Duration::from_secs(120), 1),
    );

    let mut owners: Vec<ExecutionLease<DeliveryId>> = Vec::new();
    for claimed in [first.unwrap(), second.unwrap()].concat() {
        if claimed.record.id == queued.delivery.id {
            owners.push(claimed.lease);
        } else {
            release_foreign(&persistence, claimed.lease.row).await;
        }
    }
    assert_eq!(owners.len(), 1, "exactly one claimant may own the row");

    // And the winner's fence is what the row carries: a lease minted by anyone else renews nothing.
    let owner = owners.remove(0);
    assert!(
        persistence
            .renew_delivery_lease(&owner, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    let impostor = ExecutionLease::new(
        owner.row,
        second_worker,
        Utc::now() + chrono::Duration::minutes(5),
    );
    assert!(
        !persistence
            .renew_delivery_lease(&impostor, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap(),
        "a lease this run never held must renew nothing"
    );

    park(&persistence, queued.delivery.id).await;
    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// A superseded execution can neither start a part, report a result, nor fail the delivery.
///
/// The fence is the whole protocol: a worker that was reaped and replaced must find every write it
/// attempts affecting zero rows, so the run that owns the row now owns the outcome.
#[tokio::test]
async fn a_stale_execution_cannot_write_over_the_one_that_replaced_it() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let _guard = UNSCOPED_CLAIM.lock().await;

    let queued = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(scope.company.id, scope.channel.id, scope.thread_id, "fence"),
    )
    .await;
    sort_first(&persistence, queued.delivery.id).await;

    let first = claim_mine(&persistence, WorkerId::random(), queued.delivery.id)
        .await
        .expect("the backdated row is claimed first");
    let part_id = first.parts[0].id;

    // The lease lapses and the row is reaped, then claimed by someone else.
    expire(&persistence, queued.delivery.id).await;
    persistence.reap_expired_deliveries().await.unwrap();
    sort_first(&persistence, queued.delivery.id).await;
    let second = claim_mine(&persistence, WorkerId::random(), queued.delivery.id)
        .await
        .expect("the reaped row is claimable again");
    assert_ne!(first.lease.execution, second.lease.execution);

    // Nothing the first run does may land.
    assert!(
        !persistence
            .renew_delivery_lease(&first.lease, Utc::now() + chrono::Duration::minutes(5))
            .await
            .unwrap()
    );
    assert_eq!(
        persistence.begin_part(&first.lease, part_id).await.unwrap(),
        DeliveryOutcome::LeaseLost
    );
    assert_eq!(
        persistence
            .complete_part(PartResult {
                fence: &first.lease,
                part_id,
                outcome: &ProviderSendOutcome::Delivered {
                    provider_key: Some(ExternalMessageKey::parse("<stale@example.com>").unwrap()),
                },
            })
            .await
            .unwrap(),
        DeliveryOutcome::LeaseLost
    );
    assert_eq!(
        persistence
            .fail_delivery(DeliveryFailure {
                fence: &first.lease,
                class: FailureClass::Internal,
                detail: detail("a superseded run"),
                disposition: Disposition::Terminal,
            })
            .await
            .unwrap(),
        DeliveryOutcome::LeaseLost
    );

    // The row is still the second run's, untouched.
    assert_eq!(
        status_of(&persistence, queued.delivery.id).await,
        DeliveryStatus::Sending
    );
    let provider_key: Option<String> =
        sqlx::query_scalar("SELECT provider_message_key FROM message_delivery_parts WHERE id = $1")
            .bind(part_id.as_uuid())
            .fetch_one(&persistence.pool)
            .await
            .unwrap();
    assert_eq!(provider_key, None, "the stale run recorded no result");

    // The second run finishes it, so nothing is left claimable behind this test.
    persistence
        .begin_part(&second.lease, part_id)
        .await
        .unwrap();
    persistence
        .complete_part(PartResult {
            fence: &second.lease,
            part_id,
            outcome: &ProviderSendOutcome::Delivered {
                provider_key: Some(ExternalMessageKey::parse("<live@example.com>").unwrap()),
            },
        })
        .await
        .unwrap();
    assert_eq!(
        status_of(&persistence, queued.delivery.id).await,
        DeliveryStatus::Delivered
    );

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// A crash before the provider call costs an attempt and comes back; a crash *after* the request
/// went out does not come back at all.
///
/// The distinction is the point of `request_started_at`, and getting it backwards is how one
/// message becomes two.
#[tokio::test]
async fn a_reaped_lease_retries_only_what_never_reached_the_provider() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let _guard = UNSCOPED_CLAIM.lock().await;

    // Crash before send: the part is still `prepared`, so nothing was sent.
    let before = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(
            scope.company.id,
            scope.channel.id,
            scope.thread_id,
            "before",
        ),
    )
    .await;
    sort_first(&persistence, before.delivery.id).await;
    let claimed = claim_mine(&persistence, WorkerId::random(), before.delivery.id)
        .await
        .expect("the backdated row is claimed first");
    expire(&persistence, claimed.lease.row).await;

    // Crash after the request started: the provider may hold it.
    let after = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(scope.company.id, scope.channel.id, scope.thread_id, "after"),
    )
    .await;
    sort_first(&persistence, after.delivery.id).await;
    let claimed_after = claim_mine(&persistence, WorkerId::random(), after.delivery.id)
        .await
        .expect("the backdated row is claimed first");
    persistence
        .begin_part(&claimed_after.lease, claimed_after.parts[0].id)
        .await
        .unwrap();
    expire(&persistence, claimed_after.lease.row).await;

    let reaping = persistence.reap_expired_deliveries().await.unwrap();
    assert!(reaping.leases_expired >= 2);

    assert_eq!(
        status_of(&persistence, before.delivery.id).await,
        DeliveryStatus::Retryable,
        "a crash before the provider call is plainly retryable"
    );
    assert_eq!(
        attempts_of(&persistence, before.delivery.id).await,
        1,
        "an expired lease costs an attempt, or a row that always expires is retried for ever"
    );

    assert_eq!(
        status_of(&persistence, after.delivery.id).await,
        DeliveryStatus::OutcomeUnknown,
        "a crash after the request started must never be re-sent automatically"
    );
    assert_eq!(
        part_statuses(&persistence, after.delivery.id).await,
        vec![DeliveryPartStatus::OutcomeUnknown]
    );
    // And it is not claimable, which is the property that stops the duplicate.
    assert!(!DeliveryStatus::OutcomeUnknown.is_claimable());
    let reclaimed = persistence
        .claim_deliveries(WorkerId::random(), Duration::from_secs(120), 20)
        .await
        .unwrap();
    for delivery in &reclaimed {
        release_foreign(&persistence, delivery.lease.row).await;
    }
    assert!(
        !reclaimed
            .iter()
            .any(|delivery| delivery.record.id == after.delivery.id),
        "an unconfirmed delivery must never be handed back to a sender"
    );

    park(&persistence, before.delivery.id).await;
    park(&persistence, after.delivery.id).await;
    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// Age a live claim into an expired one.
///
/// Both timestamps move: `message_deliveries_lease_check` requires `lock_expires_at > locked_at`,
/// so pulling only the expiry back would make the row unrepresentable rather than stale.
async fn expire(persistence: &PostgresPersistence, delivery_id: DeliveryId) {
    sqlx::query("UPDATE message_deliveries SET locked_at = $2, lock_expires_at = $3 WHERE id = $1")
        .bind(delivery_id.as_uuid())
        .bind(Utc::now() - chrono::Duration::minutes(10))
        .bind(Utc::now() - chrono::Duration::seconds(1))
        .execute(&persistence.pool)
        .await
        .unwrap();
}

/// A poison delivery goes terminal on the attempt it is recognised, and does not come back to fill
/// the next batch.
///
/// The failure this prevents is a hot loop: a row that is re-claimed on every poll, fails the same
/// way, and occupies a claim slot that other tenants' work is waiting for.
#[tokio::test]
async fn a_poison_delivery_goes_terminal_without_spinning() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let _guard = UNSCOPED_CLAIM.lock().await;

    let queued = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(
            scope.company.id,
            scope.channel.id,
            scope.thread_id,
            "poison",
        ),
    )
    .await;
    sort_first(&persistence, queued.delivery.id).await;
    let claimed = claim_mine(&persistence, WorkerId::random(), queued.delivery.id)
        .await
        .expect("the backdated row is claimed first");

    assert_eq!(
        persistence
            .fail_delivery(DeliveryFailure {
                fence: &claimed.lease,
                class: FailureClass::InvalidPayload,
                detail: detail("this payload will never deserialize"),
                disposition: Disposition::Terminal,
            })
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::DeadLetter)
    );

    // One attempt spent, not five: the verdict cannot come out differently later.
    assert_eq!(attempts_of(&persistence, queued.delivery.id).await, 1);

    // And the very next claim does not see it again.
    let next = persistence
        .claim_deliveries(WorkerId::random(), Duration::from_secs(120), 20)
        .await
        .unwrap();
    for delivery in &next {
        release_foreign(&persistence, delivery.lease.row).await;
    }
    assert!(
        !next
            .iter()
            .any(|delivery| delivery.record.id == queued.delivery.id),
        "a dead-lettered row must not be reclaimed on the next poll"
    );

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// A retryable failure backs off rather than returning immediately, and the fifth one dead-letters.
#[tokio::test]
async fn a_retryable_failure_backs_off_and_the_last_attempt_dead_letters() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let _guard = UNSCOPED_CLAIM.lock().await;

    let queued = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(scope.company.id, scope.channel.id, scope.thread_id, "retry"),
    )
    .await;

    for attempt in 1..=MAX_DELIVERY_ATTEMPTS {
        sort_first(&persistence, queued.delivery.id).await;
        let claimed = claim_mine(&persistence, WorkerId::random(), queued.delivery.id)
            .await
            .expect("the backdated row is claimed first");
        persistence
            .fail_delivery(DeliveryFailure {
                fence: &claimed.lease,
                class: FailureClass::Network,
                detail: detail("the relay refused the connection"),
                disposition: Disposition::Retry,
            })
            .await
            .unwrap();
        assert_eq!(attempts_of(&persistence, queued.delivery.id).await, attempt);

        if attempt < MAX_DELIVERY_ATTEMPTS {
            assert_eq!(
                status_of(&persistence, queued.delivery.id).await,
                DeliveryStatus::Retryable
            );
            // Backed off rather than immediately due, which is what stops the hot loop.
            let due: chrono::DateTime<Utc> =
                sqlx::query_scalar("SELECT available_at FROM message_deliveries WHERE id = $1")
                    .bind(queued.delivery.id.as_uuid())
                    .fetch_one(&persistence.pool)
                    .await
                    .unwrap();
            assert!(due > Utc::now(), "attempt {attempt} came back immediately");
        }
    }

    assert_eq!(
        status_of(&persistence, queued.delivery.id).await,
        DeliveryStatus::DeadLetter,
        "the attempt budget has to end somewhere"
    );

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// A parent is delivered only when every part is, and one ambiguous part holds the whole delivery.
///
/// Email freezes one part, so this is the case the aggregation rule exists for and the one nothing
/// else exercises.
#[tokio::test]
async fn a_multi_part_delivery_aggregates_from_its_parts() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let _guard = UNSCOPED_CLAIM.lock().await;

    let queued = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest {
            parts: 3,
            ..DeliveryFixtureRequest::new(
                scope.company.id,
                scope.channel.id,
                scope.thread_id,
                "parts",
            )
        },
    )
    .await;
    sort_first(&persistence, queued.delivery.id).await;
    let claimed = claim_mine(&persistence, WorkerId::random(), queued.delivery.id)
        .await
        .expect("the backdated row is claimed first");
    assert_eq!(claimed.parts.len(), 3);

    // First part accepted: the delivery keeps its lease, because there is more to send.
    let delivered = |index: usize| ProviderSendOutcome::Delivered {
        provider_key: Some(
            ExternalMessageKey::parse(format!("<part-{index}@example.com>")).unwrap(),
        ),
    };
    persistence
        .begin_part(&claimed.lease, claimed.parts[0].id)
        .await
        .unwrap();
    assert_eq!(
        persistence
            .complete_part(PartResult {
                fence: &claimed.lease,
                part_id: claimed.parts[0].id,
                outcome: &delivered(0),
            })
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Sending),
        "a delivery with parts still to send keeps its claim rather than settling mid-send"
    );

    // Second part ambiguous: the whole delivery becomes unconfirmed, even though one part landed.
    persistence
        .begin_part(&claimed.lease, claimed.parts[1].id)
        .await
        .unwrap();
    assert_eq!(
        persistence
            .complete_part(PartResult {
                fence: &claimed.lease,
                part_id: claimed.parts[1].id,
                outcome: &ProviderSendOutcome::OutcomeUnknown {
                    class: FailureClass::Timeout,
                    detail: detail("the connection dropped after the request went out"),
                },
            })
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::OutcomeUnknown)
    );
    assert_eq!(
        part_statuses(&persistence, queued.delivery.id).await,
        vec![
            DeliveryPartStatus::Delivered,
            DeliveryPartStatus::OutcomeUnknown,
            DeliveryPartStatus::Prepared,
        ],
        "the part that landed keeps its own result"
    );

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// A dependent delivery waits for its predecessor, and is dead-lettered with a typed reason when
/// that predecessor can never land.
///
/// Without the sweep the dependant is excluded from every claim and nothing ever changes its
/// status, so it is invisible to the stuck-work census as well as to the worker.
#[tokio::test]
async fn a_dependent_delivery_waits_and_is_orphaned_when_its_root_dies() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let _guard = UNSCOPED_CLAIM.lock().await;

    let root = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(scope.company.id, scope.channel.id, scope.thread_id, "root"),
    )
    .await;
    let dependent = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest {
            depends_on: Some(root.delivery.id),
            ..DeliveryFixtureRequest::new(
                scope.company.id,
                scope.channel.id,
                scope.thread_id,
                "leaf",
            )
        },
    )
    .await;
    sort_first(&persistence, dependent.delivery.id).await;

    // The dependant sorts first and is still not claimable: its root has not landed.
    let claimed = persistence
        .claim_deliveries(WorkerId::random(), Duration::from_secs(120), 5)
        .await
        .unwrap();
    for delivery in &claimed {
        release_foreign(&persistence, delivery.lease.row).await;
    }
    assert!(
        !claimed
            .iter()
            .any(|delivery| delivery.record.id == dependent.delivery.id),
        "a delivery must not overtake the one it threads under"
    );

    // The root dies, and the sweep says so on the dependant rather than leaving it waiting for ever.
    sqlx::query(
        "UPDATE message_deliveries SET status = 'dead_letter', attempt_count = max_attempts
          WHERE id = $1",
    )
    .bind(root.delivery.id.as_uuid())
    .execute(&persistence.pool)
    .await
    .unwrap();
    let reaping = persistence.reap_expired_deliveries().await.unwrap();
    assert!(reaping.dependencies_orphaned >= 1);

    assert_eq!(
        status_of(&persistence, dependent.delivery.id).await,
        DeliveryStatus::DeadLetter
    );
    let class: Option<String> =
        sqlx::query_scalar("SELECT last_error_class FROM message_deliveries WHERE id = $1")
            .bind(dependent.delivery.id.as_uuid())
            .fetch_one(&persistence.pool)
            .await
            .unwrap();
    assert_eq!(
        class.as_deref(),
        Some(FailureClass::DependencyFailed.as_str()),
        "an orphaned dependant must say *why*, not just that it failed"
    );

    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// A shutdown before the provider call gives the claim back rather than letting it lapse.
#[tokio::test]
async fn releasing_a_claim_costs_no_attempt_and_makes_it_immediately_claimable() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let scope = scope(&persistence).await;
    let _guard = UNSCOPED_CLAIM.lock().await;

    let queued = queue(
        &persistence,
        &scope,
        DeliveryFixtureRequest::new(
            scope.company.id,
            scope.channel.id,
            scope.thread_id,
            "release",
        ),
    )
    .await;
    sort_first(&persistence, queued.delivery.id).await;
    let claimed = claim_mine(&persistence, WorkerId::random(), queued.delivery.id)
        .await
        .expect("the backdated row is claimed first");

    assert_eq!(
        persistence.release_delivery(&claimed.lease).await.unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Pending)
    );
    assert_eq!(
        attempts_of(&persistence, queued.delivery.id).await,
        0,
        "nothing was sent, so nothing was spent"
    );

    // And it is claimable now, not in a lease period's time.
    let again = claim_mine(&persistence, WorkerId::random(), queued.delivery.id).await;
    assert!(
        again.is_some(),
        "a released delivery is immediately claimable"
    );

    park(&persistence, queued.delivery.id).await;
    CompanyPersistence::delete(&persistence, scope.company.id)
        .await
        .unwrap();
}

/// Every stored vocabulary is written twice -- as a Rust enum and as a SQL `CHECK` -- and both
/// directions have to agree. A variant Rust knows and SQL rejects fails at insert time in
/// production; a value SQL allows and Rust cannot parse fails at read time, on a row that already
/// exists.
#[tokio::test]
async fn the_delivery_vocabularies_match_their_database_constraints() {
    let Some(pool) = test_pool().await else {
        return;
    };

    assert_check_variants(
        &pool,
        "message_deliveries_status_check",
        DeliveryStatus::ALL,
    )
    .await;
    assert_check_variants(
        &pool,
        "message_deliveries_purpose_check",
        DeliveryPurpose::ALL,
    )
    .await;
    assert_check_variants(
        &pool,
        "message_deliveries_transport_check",
        TransportKind::ALL,
    )
    .await;
    assert_check_variants(
        &pool,
        "message_delivery_parts_status_check",
        DeliveryPartStatus::ALL,
    )
    .await;

    // The failure classes are a function rather than an inline list, so they are asserted by asking.
    for class in FailureClass::ALL {
        let accepted: bool = sqlx::query_scalar("SELECT valid_delivery_failure_class($1)")
            .bind(class.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(accepted, "SQL rejects the '{class}' failure class");
    }
    let unknown: bool = sqlx::query_scalar("SELECT valid_delivery_failure_class($1)")
        .bind("gremlins")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!unknown);
}

/// The claim's predicate and the partial index it has to match are both derived from
/// [`DeliveryStatus::is_claimable`]. A status added to the enum and not to the index would be
/// claimable through a sequential scan, and nobody would notice until the queue was large.
#[tokio::test]
async fn the_claimable_statuses_match_the_index_the_claim_relies_on() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let definition: String = sqlx::query_scalar(
        "SELECT indexdef FROM pg_indexes WHERE indexname = 'message_deliveries_claimable_idx'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    for status in DeliveryStatus::ALL {
        let named = definition.contains(&format!("'{}'", status.as_str()));
        assert_eq!(
            named,
            status.is_claimable(),
            "'{status}' disagrees between `is_claimable` and the partial index"
        );
    }
}

/// Compare a `CHECK (column IN ('a', 'b'))` constraint's literals against an enum's inventory.
async fn assert_check_variants<T: std::fmt::Display>(
    pool: &sqlx::PgPool,
    constraint: &str,
    variants: &[T],
) {
    let definition: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = $1",
    )
    .bind(constraint)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|error| panic!("{constraint} is not a constraint on this schema: {error}"));

    let in_sql: Vec<String> = definition
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    let in_rust: Vec<String> = variants.iter().map(ToString::to_string).collect();

    let mut sorted_sql = in_sql.clone();
    sorted_sql.sort();
    let mut sorted_rust = in_rust.clone();
    sorted_rust.sort();
    assert_eq!(
        sorted_rust, sorted_sql,
        "{constraint} and its Rust enum hold different sets"
    );
}

/// A payload the current build cannot read is refused at the seam rather than misread halfway
/// through a provider call.
#[test]
fn a_stored_part_payload_is_decoded_fallibly() {
    let payload = crate::transport::TransportPayload::encode(
        TransportKind::Email,
        1,
        &serde_json::json!({ "body": "hello" }),
    )
    .unwrap();

    assert!(
        payload
            .decode::<serde_json::Value>(TransportKind::Slack, 1)
            .is_err()
    );
    assert!(
        payload
            .decode::<serde_json::Value>(TransportKind::Email, 2)
            .is_err()
    );
    assert!(
        payload
            .decode::<serde_json::Value>(TransportKind::Email, 1)
            .is_ok()
    );
}

/// The destination is read back through the transport that wrote it, never through a literal.
#[test]
fn a_stored_destination_is_read_through_its_own_transport() {
    let destination = crate::entities::transport::ExternalDestination::Email(EmailAddress::from(
        "person@example.com",
    ));
    assert_eq!(
        crate::entities::transport::ExternalDestination::parse(
            TransportKind::Email,
            destination.as_str()
        ),
        Ok(destination)
    );
    // Slack addresses conversations through its binding and has no address namespace, so a stored
    // destination there is a row nothing should have written -- said, not guessed.
    assert!(
        crate::entities::transport::ExternalDestination::parse(
            TransportKind::Slack,
            "person@example.com"
        )
        .is_err()
    );
}
