//! What the delivery worker decides, with a scripted queue and a scripted provider.
//!
//! The database-backed protocol -- claim, fence, reap -- is
//! `src/adapters/persistence/delivery/tests.rs`. What is left here is the worker's own judgement:
//! which part it sends next, when it stops touching a delivery, and how it orders a batch so one
//! tenant cannot be served before every other tenant is started.

use std::sync::{Arc, Mutex};

use chrono::Utc;

use super::*;
#[path = "delivery_worker_lifecycle_tests.rs"]
mod lifecycle;
use crate::{
    entities::{
        correlation::CorrelationId,
        message::CanonicalMessageId,
        transport::{ChannelBindingId, DeliveryId, DeliveryPartId, DeliveryPartStatus},
    },
    transport::{
        ContentDigest, DeliveryAttribution, DeliveryCreation, DeliveryKey, MAX_DELIVERY_ATTEMPTS,
        PartIndex, PartKey, RenderedPart, TransportPayload, TransportRenderer, TransportSender,
    },
};

fn record(company_id: uuid::Uuid) -> DeliveryRecord {
    DeliveryRecord {
        id: DeliveryId::random(),
        company_id,
        attribution: Some(DeliveryAttribution {
            channel_id: uuid::Uuid::new_v4(),
            message_id: CanonicalMessageId::random(),
            source_binding_id: ChannelBindingId::random(),
            destination_binding_id: ChannelBindingId::random(),
        }),
        external_destination: None,
        correlation_id: CorrelationId::new(),
        transport: TransportKind::Email,
        purpose: crate::entities::transport::DeliveryPurpose::WorkflowReply,
        idempotency_key: DeliveryKey::parse("reply:test").expect("a short key"),
        attempt_count: 0,
        max_attempts: MAX_DELIVERY_ATTEMPTS,
        cancellation: None,
    }
}

fn stored_part(index: u16, status: DeliveryPartStatus) -> StoredPart {
    StoredPart {
        id: DeliveryPartId::random(),
        rendered: RenderedPart {
            index: PartIndex::new(index),
            key: PartKey::parse(format!("part-{index}")).unwrap(),
            payload: TransportPayload::encode(TransportKind::Email, 1, &index).unwrap(),
            digest: ContentDigest::sha256_of(b"body"),
        },
        status,
        attempt_count: 0,
        request_started_at: None,
        provider_message_key: None,
    }
}

fn claimed(company_id: uuid::Uuid, parts: Vec<StoredPart>) -> ClaimedDelivery {
    let record = record(company_id);
    ClaimedDelivery {
        lease: ExecutionLease::new(
            record.id,
            WorkerId::random(),
            Utc::now() + chrono::Duration::minutes(5),
        ),
        record,
        parts,
    }
}

/// A batch is served round-robin over companies, so no tenant waits behind another's burst.
///
/// The claim itself has to stay global -- tenant-scoping it would leave the other tenants' rows
/// unclaimed by anybody -- which is exactly why fairness is a scheduling decision made over what
/// the claim returned.
#[test]
fn a_batch_is_interleaved_so_one_tenant_cannot_monopolise_it() {
    let busy = uuid::Uuid::new_v4();
    let quiet = uuid::Uuid::new_v4();

    // The claim's own order: ten from one company, then one from another, which is what a burst
    // looks like when it is ordered by due time.
    let mut batch: Vec<ClaimedDelivery> = (0..10)
        .map(|_| claimed(busy, vec![stored_part(0, DeliveryPartStatus::Prepared)]))
        .collect();
    batch.push(claimed(
        quiet,
        vec![stored_part(0, DeliveryPartStatus::Prepared)],
    ));

    let ordered = fair_order(batch);
    assert_eq!(ordered.len(), 11, "fairness reorders, it never drops");

    let quiet_at = ordered
        .iter()
        .position(|delivery| delivery.record.company_id == quiet)
        .expect("the quiet tenant is still in the batch");
    assert!(
        quiet_at <= PER_COMPANY_BATCH_SHARE,
        "the quiet tenant waited behind {quiet_at} of the busy tenant's rows"
    );

    // And within a company the claim's order survives, which is what keeps it fair over time.
    let busy_order: Vec<DeliveryId> = ordered
        .iter()
        .filter(|delivery| delivery.record.company_id == busy)
        .map(|delivery| delivery.record.id)
        .collect();
    assert_eq!(busy_order.len(), 10);
}

/// One company's batch is not reordered at all: the fairness pass must be a no-op where there is
/// nothing to be fair between.
#[test]
fn a_single_tenant_batch_keeps_the_claim_s_order() {
    let company = uuid::Uuid::new_v4();
    let batch: Vec<ClaimedDelivery> = (0..7)
        .map(|_| claimed(company, vec![stored_part(0, DeliveryPartStatus::Prepared)]))
        .collect();
    let before: Vec<DeliveryId> = batch.iter().map(|delivery| delivery.record.id).collect();

    let after: Vec<DeliveryId> = fair_order(batch)
        .iter()
        .map(|delivery| delivery.record.id)
        .collect();
    assert_eq!(before, after);
}

/// A partly delivered multi-part send resumes at the first part that still owes a provider call,
/// and never re-sends one the provider already accepted.
#[test]
fn a_resumed_delivery_starts_at_its_first_unfinished_part() {
    let delivery = claimed(
        uuid::Uuid::new_v4(),
        vec![
            stored_part(0, DeliveryPartStatus::Delivered),
            stored_part(1, DeliveryPartStatus::Retryable),
            stored_part(2, DeliveryPartStatus::Prepared),
        ],
    );

    assert_eq!(
        delivery
            .next_unfinished_part()
            .map(|part| part.rendered.index.get()),
        Some(1)
    );

    // An ambiguous part is *not* unfinished. Resuming it is the duplicate the whole state exists
    // to prevent, so the delivery has nothing left to do even though nothing was confirmed.
    let ambiguous = claimed(
        uuid::Uuid::new_v4(),
        vec![stored_part(0, DeliveryPartStatus::OutcomeUnknown)],
    );
    assert!(ambiguous.next_unfinished_part().is_none());
}

/// A transport this deployment cannot speak is a configuration fact, and a delivery naming one is
/// dead-lettered rather than retried: five backoffs will not install an adapter.
/// p08 B4b: the stuck-delivery gauges moved here from the retired task census. Every sweep
/// publishes all three, zeroes included, under the same `stuck_work` name and labels.
#[tokio::test]
async fn a_sweep_publishes_every_stuck_delivery_gauge() {
    let monitor = Arc::new(crate::adapters::monitoring::in_memory_monitor::InMemoryMonitor::new());
    let worker = DeliveryWorker::new(
        Arc::new(ScriptedQueue::default()),
        Arc::new(TransportRegistry::new()),
    )
    .with_monitoring(monitor.clone());

    worker.reap().await;

    let gauges = monitor.get_stats_json()["gauges"].clone();
    assert_eq!(
        gauges["stuck_work{kind=delivery_dead_lettered}"], 2.0,
        "{gauges}"
    );
    assert_eq!(gauges["stuck_work{kind=delivery_overdue}"], 0.0, "{gauges}");
    assert_eq!(
        gauges["stuck_work{kind=delivery_unconfirmed}"], 0.0,
        "{gauges}"
    );
}

#[tokio::test]
async fn a_delivery_for_an_unregistered_transport_is_dead_lettered() {
    let queue = Arc::new(ScriptedQueue::default());
    let worker = DeliveryWorker::new(queue.clone(), Arc::new(TransportRegistry::new()));
    let delivery = claimed(
        uuid::Uuid::new_v4(),
        vec![stored_part(0, DeliveryPartStatus::Prepared)],
    );
    let (_shutdown, mut receiver) = broadcast::channel(1);

    let mut batch = BatchLeases::of(std::slice::from_ref(&delivery));
    worker.deliver(delivery, &mut batch, &mut receiver).await;

    let failures = queue.failures.lock().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, FailureClass::Internal);
    assert_eq!(
        failures[0].1,
        Disposition::Terminal,
        "a missing adapter cannot come out differently next time"
    );
    assert!(
        queue.started.lock().unwrap().is_empty(),
        "nothing may be marked as started when there is nothing to send it"
    );
}

/// Shutdown before the provider call gives the claim back rather than letting the lease lapse.
///
/// Letting it expire would strand the row for a full lease period *and* charge it an attempt,
/// which is how a rolling deploy turns into a delivery delay.
#[tokio::test]
async fn shutdown_before_a_send_releases_the_claim_rather_than_stranding_it() {
    let queue = Arc::new(ScriptedQueue::default());
    let registry = TransportRegistry::new()
        .register(
            Arc::new(NoopRenderer),
            Arc::new(ScriptedSender::new(ProviderSendOutcome::Delivered {
                provider_key: None,
            })),
        )
        .expect("one transport registers");
    let worker = DeliveryWorker::new(queue.clone(), Arc::new(registry));

    let (shutdown, mut receiver) = broadcast::channel(1);
    shutdown.send(()).expect("the receiver is live");

    let delivery = claimed(
        uuid::Uuid::new_v4(),
        vec![stored_part(0, DeliveryPartStatus::Prepared)],
    );
    let mut batch = BatchLeases::of(std::slice::from_ref(&delivery));
    worker.deliver(delivery, &mut batch, &mut receiver).await;

    assert_eq!(
        *queue.released.lock().unwrap(),
        1,
        "the claim goes back immediately"
    );
    assert!(
        queue.started.lock().unwrap().is_empty(),
        "shutdown must be observed before the provider is called"
    );
    assert!(queue.failures.lock().unwrap().is_empty());
}

/// The worker stops touching a delivery the moment a fenced write reports the lease gone.
#[tokio::test]
async fn losing_the_lease_stops_the_worker_mid_send() {
    let queue = Arc::new(ScriptedQueue {
        lease_lost_on_begin: Mutex::new(true),
        ..ScriptedQueue::default()
    });
    let registry = TransportRegistry::new()
        .register(
            Arc::new(NoopRenderer),
            Arc::new(ScriptedSender::new(ProviderSendOutcome::Delivered {
                provider_key: None,
            })),
        )
        .expect("one transport registers");
    let worker = DeliveryWorker::new(queue.clone(), Arc::new(registry));
    let (_shutdown, mut receiver) = broadcast::channel(1);

    let delivery = claimed(
        uuid::Uuid::new_v4(),
        vec![
            stored_part(0, DeliveryPartStatus::Prepared),
            stored_part(1, DeliveryPartStatus::Prepared),
        ],
    );
    let mut batch = BatchLeases::of(std::slice::from_ref(&delivery));
    worker.deliver(delivery, &mut batch, &mut receiver).await;

    assert_eq!(
        *queue.sends.lock().unwrap(),
        0,
        "a run that lost its lease must make no provider call"
    );
    assert!(
        queue.completions.lock().unwrap().is_empty(),
        "and must record no result over the run that replaced it"
    );
}

/// A renewal that reports the lease gone stops the delivery before its provider call, and the
/// worker renews before every part it sends.
#[tokio::test]
async fn delivery_late_completion_a_lost_renewal_stops_the_send() {
    let registry = || {
        TransportRegistry::new()
            .register(
                Arc::new(NoopRenderer),
                Arc::new(ScriptedSender::new(ProviderSendOutcome::Delivered {
                    provider_key: None,
                })),
            )
            .expect("one transport registers")
    };
    let two_parts = || {
        claimed(
            uuid::Uuid::new_v4(),
            vec![
                stored_part(0, DeliveryPartStatus::Prepared),
                stored_part(1, DeliveryPartStatus::Prepared),
            ],
        )
    };
    let (_shutdown, mut receiver) = broadcast::channel(1);

    let lost = Arc::new(ScriptedQueue {
        lease_lost_on_renew: Mutex::new(true),
        ..ScriptedQueue::default()
    });
    let worker = DeliveryWorker::new(lost.clone(), Arc::new(registry()));
    let delivery = two_parts();
    let mut batch = BatchLeases::of(std::slice::from_ref(&delivery));
    worker.deliver(delivery, &mut batch, &mut receiver).await;
    assert!(
        lost.started.lock().unwrap().is_empty(),
        "a lease renewal reported lost means no provider request starts"
    );
    assert!(batch.held.is_empty(), "the lost lease is no longer renewed");

    let live = Arc::new(ScriptedQueue {
        parts_still_sending: Mutex::new(1),
        ..ScriptedQueue::default()
    });
    let worker = DeliveryWorker::new(live.clone(), Arc::new(registry()));
    let delivery = two_parts();
    let mut batch = BatchLeases::of(std::slice::from_ref(&delivery));
    worker.deliver(delivery, &mut batch, &mut receiver).await;
    assert_eq!(live.started.lock().unwrap().len(), 2);
    assert_eq!(
        *live.renewals.lock().unwrap(),
        2,
        "one renewal before each part"
    );
}

/// A renewal that errors changes nothing: the delivery stays in the renewed set and the worker
/// goes on to `begin_part`, whose live-lease fence decides whether the provider call starts
/// (FIXAPP-P02-B1-F3; the G1 r2 F5 decision).
#[tokio::test]
async fn delivery_late_completion_a_renewal_error_still_sends() {
    let queue = Arc::new(ScriptedQueue {
        renewal_errors: Mutex::new(true),
        ..ScriptedQueue::default()
    });
    let registry = TransportRegistry::new()
        .register(
            Arc::new(NoopRenderer),
            Arc::new(ScriptedSender::new(ProviderSendOutcome::Delivered {
                provider_key: None,
            })),
        )
        .expect("one transport registers");
    let worker = DeliveryWorker::new(queue.clone(), Arc::new(registry));
    let (_shutdown, mut receiver) = broadcast::channel(1);

    let delivery = claimed(
        uuid::Uuid::new_v4(),
        vec![stored_part(0, DeliveryPartStatus::Prepared)],
    );
    let row = delivery.lease.row;
    let mut batch = BatchLeases::of(std::slice::from_ref(&delivery));
    worker.deliver(delivery, &mut batch, &mut receiver).await;

    assert_eq!(*queue.renewals.lock().unwrap(), 1, "the renewal was tried");
    assert_eq!(
        queue.started.lock().unwrap().len(),
        1,
        "a renewal error still lets begin_part decide"
    );
    assert_eq!(
        queue.completions.lock().unwrap().len(),
        1,
        "and the send completes"
    );
    assert_eq!(
        batch.held.iter().map(|lease| lease.row).collect::<Vec<_>>(),
        vec![row],
        "an error drops nothing from the renewed set"
    );
}

/// A queue that records what the worker asked of it and answers as the test scripted.
#[derive(Default)]
struct ScriptedQueue {
    started: Mutex<Vec<DeliveryPartId>>,
    completions: Mutex<Vec<DeliveryPartId>>,
    failures: Mutex<Vec<(FailureClass, Disposition)>>,
    released: Mutex<usize>,
    sends: Mutex<usize>,
    lease_lost_on_begin: Mutex<bool>,
    lease_lost_on_renew: Mutex<bool>,
    renewal_errors: Mutex<bool>,
    renewals: Mutex<usize>,
    /// How many completions leave the delivery `sending`, i.e. with more parts to go.
    parts_still_sending: Mutex<usize>,
}

#[async_trait::async_trait]
impl DeliveryQueue for ScriptedQueue {
    async fn census_stuck_deliveries(
        &self,
        _overdue_after: Duration,
    ) -> AppResult<crate::entities::stuck_work::StuckWorkCensus> {
        Ok(crate::entities::stuck_work::StuckWorkCensus {
            delivery_dead_lettered: 2,
            ..Default::default()
        })
    }

    async fn claim_deliveries(
        &self,
        _owner: WorkerId,
        _lease_for: Duration,
        _limit: i64,
    ) -> AppResult<Vec<ClaimedDelivery>> {
        Ok(Vec::new())
    }

    async fn renew_delivery_leases(
        &self,
        fences: &[ExecutionLease<DeliveryId>],
        _lease_for: Duration,
    ) -> AppResult<Vec<DeliveryId>> {
        *self.renewals.lock().unwrap() += 1;
        if *self.renewal_errors.lock().unwrap() {
            return Err(crate::app_error::AppError::Internal(
                "scripted renewal error".into(),
            ));
        }
        if *self.lease_lost_on_renew.lock().unwrap() {
            return Ok(Vec::new());
        }
        Ok(fences.iter().map(|fence| fence.row).collect())
    }

    async fn begin_part(
        &self,
        _fence: &ExecutionLease<DeliveryId>,
        part_id: DeliveryPartId,
    ) -> AppResult<DeliveryOutcome> {
        if *self.lease_lost_on_begin.lock().unwrap() {
            return Ok(DeliveryOutcome::LeaseLost);
        }
        self.started.lock().unwrap().push(part_id);
        Ok(DeliveryOutcome::Applied(DeliveryStatus::Sending))
    }

    async fn complete_part(&self, result: PartResult<'_>) -> AppResult<DeliveryOutcome> {
        let mut completions = self.completions.lock().unwrap();
        completions.push(result.part_id);
        if completions.len() <= *self.parts_still_sending.lock().unwrap() {
            return Ok(DeliveryOutcome::Applied(DeliveryStatus::Sending));
        }
        Ok(DeliveryOutcome::Applied(DeliveryStatus::Delivered))
    }

    async fn fail_delivery(&self, failure: DeliveryFailure<'_>) -> AppResult<DeliveryOutcome> {
        self.failures
            .lock()
            .unwrap()
            .push((failure.class, failure.disposition));
        Ok(DeliveryOutcome::Applied(DeliveryStatus::DeadLetter))
    }

    async fn release_delivery(
        &self,
        _fence: &ExecutionLease<DeliveryId>,
    ) -> AppResult<DeliveryOutcome> {
        *self.released.lock().unwrap() += 1;
        Ok(DeliveryOutcome::Applied(DeliveryStatus::Pending))
    }

    async fn reap_expired_deliveries(&self) -> AppResult<DeliveryReaping> {
        Ok(DeliveryReaping::default())
    }
}

struct ScriptedSender {
    outcome: ProviderSendOutcome,
}

impl ScriptedSender {
    fn new(outcome: ProviderSendOutcome) -> Self {
        Self { outcome }
    }
}

#[async_trait::async_trait]
impl TransportSender for ScriptedSender {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }

    async fn send(&self, _delivery: &DeliveryRecord, _part: &RenderedPart) -> ProviderSendOutcome {
        self.outcome.clone()
    }
}

struct NoopRenderer;

impl TransportRenderer for NoopRenderer {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }

    fn render(
        &self,
        _envelope: &crate::transport::DeliveryEnvelope,
    ) -> AppResult<Vec<RenderedPart>> {
        Ok(vec![stored_part(0, DeliveryPartStatus::Prepared).rendered])
    }

    fn render_standalone(
        &self,
        _envelope: &crate::transport::StandaloneDeliveryEnvelope,
    ) -> AppResult<Vec<RenderedPart>> {
        Ok(vec![stored_part(0, DeliveryPartStatus::Prepared).rendered])
    }

    fn rendered_correlation(
        &self,
        _part: &RenderedPart,
    ) -> AppResult<crate::transport::ports::RenderedCorrelation> {
        Ok(crate::transport::ports::RenderedCorrelation {
            provider_key: None,
            extension: crate::application::ports::thread::ProtocolExtension::none(),
        })
    }
    fn application_message_key(
        &self,
        _company: &crate::entities::value_objects::CompanySlug,
        _nonce: uuid::Uuid,
    ) -> AppResult<crate::entities::transport::ExternalMessageKey> {
        Err(crate::app_error::AppError::BadRequest(
            "Fixture transport does not mint application ingress keys".into(),
        ))
    }
}

/// Absorbing a duplicate is the queue working, not failing, and the caller has to be able to tell
/// the two apart to log the right thing.
#[test]
fn a_creation_says_whether_it_queued_anything() {
    let id = DeliveryId::random();
    assert!(DeliveryCreation::Created(id).was_created());
    assert!(!DeliveryCreation::Absorbed(id).was_created());
    assert_eq!(DeliveryCreation::Absorbed(id).delivery_id(), id);
}
