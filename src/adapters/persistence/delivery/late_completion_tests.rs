//! fix-app 02 §3: a provider outcome is recorded by the execution that holds the delivery even
//! after its lease lapsed, unless the reaper took the row first; and a worker keeps a slow batch's
//! leases live so its last delivery is still sent.
//!
//! Recording without a live lease is a deliberate exception to "completion stays conditional on a
//! live lease" (`src/adapters/persistence/AGENTS.md`), so the races it opens are tested with
//! competing claimants, not only sequentially.

use std::sync::Arc;

use tokio::sync::Barrier;

use super::*;
use crate::{
    adapters::protocols::email::EmailRenderer,
    application::services::delivery_worker::DeliveryWorker,
    transport::{DeliveryRecord, TransportRegistry, TransportSender},
};

/// Rounds per race: enough for both orders to occur in practice; each round asserts the
/// invariant whichever side won.
const RACE_ROUNDS: usize = 12;

fn delivered(key: &str) -> ProviderSendOutcome {
    ProviderSendOutcome::Delivered {
        provider_key: Some(ExternalMessageKey::parse(format!("<{key}@example.com>")).unwrap()),
    }
}

async fn provider_key_of(
    persistence: &PostgresPersistence,
    delivery: DeliveryId,
) -> Option<String> {
    sqlx::query_scalar(
        "SELECT provider_message_key FROM message_delivery_parts WHERE delivery_id = $1",
    )
    .bind(delivery.as_uuid())
    .fetch_one(&persistence.pool)
    .await
    .unwrap()
}

/// Queue one delivery, claim it and start its only part: a provider request is in flight.
async fn in_flight(
    persistence: &PostgresPersistence,
    scope: &Scope,
    source_key: &str,
) -> ClaimedDelivery {
    let queued = queue(
        persistence,
        scope,
        DeliveryFixtureRequest::new(
            scope.company.id,
            scope.channel.id,
            scope.thread_id,
            source_key,
        ),
    )
    .await;
    sort_first(persistence, queued.delivery.id).await;
    let claimed = claim_mine(persistence, WorkerId::random(), queued.delivery.id)
        .await
        .expect("the backdated row is claimed first");
    assert_eq!(
        persistence
            .begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Sending)
    );
    claimed
}

/// A slow provider answered after the lease ran out, and nobody reaped the row in between: what
/// it said is recorded, provider key included, instead of being thrown away as `LeaseLost`.
#[tokio::test]
async fn delivery_late_completion_after_the_lease_expired_records_the_outcome() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let claimed = in_flight(&persistence, &scope, "late").await;
    expire(&persistence, claimed.lease.row).await;

    let outcome = persistence
        .complete_part(PartResult {
            fence: &claimed.lease,
            part_id: claimed.parts[0].id,
            outcome: &delivered("late"),
        })
        .await
        .unwrap();

    assert_eq!(outcome, DeliveryOutcome::Applied(DeliveryStatus::Delivered));
    assert_eq!(
        status_of(&persistence, claimed.lease.row).await,
        DeliveryStatus::Delivered
    );
    assert_eq!(
        part_statuses(&persistence, claimed.lease.row).await,
        vec![DeliveryPartStatus::Delivered]
    );
    assert_eq!(
        provider_key_of(&persistence, claimed.lease.row)
            .await
            .as_deref(),
        Some("<late@example.com>")
    );
}

/// The reaper got there first: the row is `outcome_unknown` with no execution on it, so the late
/// answer finds nothing to write and the row stays for reconciliation.
#[tokio::test]
async fn delivery_late_completion_after_the_reaper_took_the_row_is_lease_lost() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let claimed = in_flight(&persistence, &scope, "reaped").await;
    expire(&persistence, claimed.lease.row).await;
    persistence.reap_expired_deliveries().await.unwrap();
    assert_eq!(
        status_of(&persistence, claimed.lease.row).await,
        DeliveryStatus::OutcomeUnknown
    );

    let outcome = persistence
        .complete_part(PartResult {
            fence: &claimed.lease,
            part_id: claimed.parts[0].id,
            outcome: &delivered("reaped"),
        })
        .await
        .unwrap();

    assert_eq!(outcome, DeliveryOutcome::LeaseLost);
    assert_eq!(
        status_of(&persistence, claimed.lease.row).await,
        DeliveryStatus::OutcomeUnknown
    );
    assert_eq!(
        part_statuses(&persistence, claimed.lease.row).await,
        vec![DeliveryPartStatus::OutcomeUnknown]
    );
    assert_eq!(provider_key_of(&persistence, claimed.lease.row).await, None);
}

/// Competing claimants: a late completion and the reaper start together on a lapsed in-flight
/// delivery. Exactly one wins, and the row says which: delivered with the provider key, or
/// `outcome_unknown` with the completion told `LeaseLost`. Never both, never neither.
#[tokio::test]
async fn delivery_late_completion_races_the_reaper_and_exactly_one_wins() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let (mut completed, mut reaped) = (0, 0);
    for round in 0..RACE_ROUNDS {
        let key = format!("race-{round}");
        let claimed = in_flight(&persistence, &scope, &key).await;
        expire(&persistence, claimed.lease.row).await;
        let barrier = Barrier::new(2);
        let outcome = delivered(&key);
        let (completion, reaping) = tokio::join!(
            async {
                barrier.wait().await;
                persistence
                    .complete_part(PartResult {
                        fence: &claimed.lease,
                        part_id: claimed.parts[0].id,
                        outcome: &outcome,
                    })
                    .await
                    .unwrap()
            },
            async {
                barrier.wait().await;
                persistence.reap_expired_deliveries().await.unwrap()
            }
        );
        let row = claimed.lease.row;
        match completion {
            DeliveryOutcome::Applied(DeliveryStatus::Delivered) => {
                completed += 1;
                assert_eq!(
                    status_of(&persistence, row).await,
                    DeliveryStatus::Delivered
                );
                assert_eq!(
                    part_statuses(&persistence, row).await,
                    vec![DeliveryPartStatus::Delivered]
                );
                assert_eq!(
                    provider_key_of(&persistence, row).await,
                    Some(format!("<{key}@example.com>"))
                );
                assert_eq!(
                    reaping.leases_expired, 0,
                    "round {round}: the reaper took nothing"
                );
            }
            DeliveryOutcome::LeaseLost => {
                reaped += 1;
                assert_eq!(
                    status_of(&persistence, row).await,
                    DeliveryStatus::OutcomeUnknown
                );
                assert_eq!(
                    part_statuses(&persistence, row).await,
                    vec![DeliveryPartStatus::OutcomeUnknown]
                );
                assert_eq!(provider_key_of(&persistence, row).await, None);
                assert_eq!(
                    reaping.leases_expired, 1,
                    "round {round}: the reaper took it"
                );
            }
            other => panic!("round {round}: unexpected completion {other:?}"),
        }
    }
    assert_eq!(completed + reaped, RACE_ROUNDS);
}

/// Competing claimants: a renewal and the reaper start together on a lease at its expiry
/// boundary. Either the renewal extended it and the row is still `sending` under this execution,
/// or the reaper took it and the renewal returned nothing -- never both. (Neither is possible too:
/// the renewal can arrive just after the expiry and the reaper just before it; the row is then
/// still lapsed under this execution, for the next sweep.)
#[tokio::test]
async fn delivery_late_completion_renewal_races_the_reaper_and_exactly_one_wins() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let (mut renewed_count, mut reaped_count) = (0, 0);
    for round in 0..RACE_ROUNDS {
        let queued = queue(
            &persistence,
            &scope,
            DeliveryFixtureRequest::new(
                scope.company.id,
                scope.channel.id,
                scope.thread_id,
                &format!("renew-race-{round}"),
            ),
        )
        .await;
        sort_first(&persistence, queued.delivery.id).await;
        let claimed = claim_mine(&persistence, WorkerId::random(), queued.delivery.id)
            .await
            .expect("the backdated row is claimed first");
        // Lapse within a few milliseconds, so the two arrive on either side of the boundary.
        sqlx::query(
            "UPDATE message_deliveries SET locked_at = clock_timestamp() - interval '1 minute', \
               lock_expires_at = clock_timestamp() + interval '15 milliseconds' WHERE id = $1",
        )
        .bind(claimed.lease.row.as_uuid())
        .execute(&persistence.pool)
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let barrier = Barrier::new(2);
        let fences = [claimed.lease];
        let (renewal, reaping) = tokio::join!(
            async {
                barrier.wait().await;
                persistence
                    .renew_delivery_leases(&fences, Duration::from_secs(120))
                    .await
                    .unwrap()
            },
            async {
                barrier.wait().await;
                persistence.reap_expired_deliveries().await.unwrap()
            }
        );
        let row = claimed.lease.row;
        let renewed = renewal.contains(&row);
        let reaped = reaping.leases_expired == 1;
        assert!(!(renewed && reaped), "round {round}: both claimed the row");
        let execution: Option<Uuid> =
            sqlx::query_scalar("SELECT execution_id FROM message_deliveries WHERE id = $1")
                .bind(row.as_uuid())
                .fetch_one(&persistence.pool)
                .await
                .unwrap();
        if reaped {
            reaped_count += 1;
            assert_eq!(
                status_of(&persistence, row).await,
                DeliveryStatus::Retryable
            );
            assert_eq!(execution, None, "round {round}");
        } else {
            renewed_count += usize::from(renewed);
            assert_eq!(status_of(&persistence, row).await, DeliveryStatus::Sending);
            assert_eq!(
                execution,
                Some(claimed.lease.execution.as_uuid()),
                "round {round}"
            );
            // Leave nothing in flight for the next round's reaper.
            persistence.release_delivery(&claimed.lease).await.unwrap();
        }
        park(&persistence, row).await;
    }
    assert!(renewed_count + reaped_count <= RACE_ROUNDS);
}

/// The renewal locks its rows in id order whatever order the worker lists them in, the order the
/// reaper, a cancellation and the receipt consumer take, so it cannot deadlock with them. Another
/// transaction holds the lower id; a renewal listing the higher id first waits on the lower one
/// without having locked the higher one, so the holder can still take the higher id.
#[tokio::test]
async fn delivery_late_completion_renewal_locks_in_id_order() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    for key in ["order-a", "order-b"] {
        let queued = queue(
            &persistence,
            &scope,
            DeliveryFixtureRequest::new(scope.company.id, scope.channel.id, scope.thread_id, key),
        )
        .await;
        sort_first(&persistence, queued.delivery.id).await;
    }
    let mut leases: Vec<_> = persistence
        .claim_deliveries(WorkerId::random(), Duration::from_secs(120), 2)
        .await
        .unwrap()
        .into_iter()
        .map(|claimed| claimed.lease)
        .collect();
    assert_eq!(leases.len(), 2);
    leases.sort_by_key(|lease| lease.row.as_uuid());
    let (lower, higher) = (leases[0], leases[1]);

    let mut holder = persistence.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM message_deliveries WHERE id = $1 FOR UPDATE")
        .bind(lower.row.as_uuid())
        .execute(&mut *holder)
        .await
        .unwrap();
    let renewing = {
        let persistence = persistence.clone();
        tokio::spawn(async move {
            persistence
                .renew_delivery_leases(&[higher, lower], Duration::from_secs(120))
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !renewing.is_finished(),
        "the renewal waits on the held lower id"
    );
    sqlx::query("SET LOCAL lock_timeout = '2s'")
        .execute(&mut *holder)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM message_deliveries WHERE id = $1 FOR UPDATE")
        .bind(higher.row.as_uuid())
        .execute(&mut *holder)
        .await
        .expect("the renewal has not locked the higher id ahead of the lower one");
    holder.commit().await.unwrap();

    let mut renewed = renewing.await.unwrap().unwrap();
    renewed.sort_by_key(|row| row.as_uuid());
    assert_eq!(renewed, vec![lower.row, higher.row]);
}

/// A provider that takes `delay` per call and accepts everything.
struct SlowProvider {
    delay: Duration,
}

#[async_trait::async_trait]
impl TransportSender for SlowProvider {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }

    async fn send(&self, delivery: &DeliveryRecord, _part: &RenderedPart) -> ProviderSendOutcome {
        tokio::time::sleep(self.delay).await;
        delivered(&delivery.id.as_uuid().simple().to_string())
    }
}

/// One claim leases a whole batch, which is then sent in sequence. With a lease shorter than the
/// batch takes, the worker's renewals keep the waiting deliveries live, so the last one is still
/// sent: the third call starts after the original lease would have run out.
#[tokio::test]
async fn delivery_late_completion_worker_renews_so_a_slow_batch_completes() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&persistence).await;
    let mut deliveries = Vec::new();
    for index in 0..3 {
        let queued = queue(
            &persistence,
            &scope,
            DeliveryFixtureRequest::new(
                scope.company.id,
                scope.channel.id,
                scope.thread_id,
                &format!("slow-{index}"),
            ),
        )
        .await;
        deliveries.push(queued.delivery.id);
    }
    let transports = TransportRegistry::new()
        .register(
            Arc::new(EmailRenderer::new("example.test")),
            Arc::new(SlowProvider {
                delay: Duration::from_millis(1200),
            }),
        )
        .unwrap();
    let worker = DeliveryWorker::new(Arc::new(persistence.clone()), Arc::new(transports))
        .with_lease(Duration::from_secs(2));
    let (_shutdown, mut receiver) = tokio::sync::broadcast::channel(1);

    let claimed = worker.drain_once(&mut receiver).await.unwrap();

    assert_eq!(claimed, 3);
    for delivery in deliveries {
        assert_eq!(
            status_of(&persistence, delivery).await,
            DeliveryStatus::Delivered,
            "every delivery of the batch was sent, the last one included"
        );
    }
}
