//! Worker scheduling and cancellation tests.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;

use super::*;
use crate::{
    app_error::AppResult,
    entities::{
        correlation::CorrelationId,
        transport::{ExternalEventKey, InboundEventId, TransportKind},
    },
    transport::{
        AuthenticatedInboundEvent, InboundCommitOutcome, InboundCommitRequest, InboundEventCensus,
        InboundEventDecoder, InboundEventInbox, InboundEventPayload, InboundEventReaping,
        InboundEventRetention, InboundEventStoreOutcome, InboundEventTransition,
        InboundMessageCommitter, InboundPayloadDigest, InboundRetentionPolicy, SafeHeaderFacts,
    },
};

struct ScriptedQueue {
    renew: bool,
    renewals: AtomicUsize,
}

impl ScriptedQueue {
    fn new(renew: bool) -> Self {
        Self {
            renew,
            renewals: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl InboundEventInbox for ScriptedQueue {
    async fn store_authenticated(
        &self,
        _event: AuthenticatedInboundEvent,
    ) -> AppResult<InboundEventStoreOutcome> {
        Err(AppError::Internal("unused store".into()))
    }
}

#[async_trait]
impl InboundEventQueue for ScriptedQueue {
    async fn claim_inbound_events(
        &self,
        _owner: WorkerId,
        _lease_for: Duration,
        _limit: i64,
    ) -> AppResult<Vec<ClaimedInboundEvent>> {
        Ok(Vec::new())
    }

    async fn renew_inbound_event_lease(
        &self,
        _fence: &ExecutionLease<InboundEventId>,
        _until: chrono::DateTime<Utc>,
    ) -> AppResult<bool> {
        self.renewals.fetch_add(1, Ordering::SeqCst);
        Ok(self.renew)
    }

    async fn complete_inbound_event(
        &self,
        _fence: &ExecutionLease<InboundEventId>,
    ) -> AppResult<InboundEventTransition> {
        Ok(InboundEventTransition::Applied(
            InboundEventStatus::Completed,
        ))
    }

    async fn ignore_inbound_event(
        &self,
        _fence: &ExecutionLease<InboundEventId>,
        _reason: InboundEventIgnoreReason,
    ) -> AppResult<InboundEventTransition> {
        Ok(InboundEventTransition::Applied(InboundEventStatus::Ignored))
    }

    async fn retry_inbound_event(
        &self,
        _failure: InboundEventFailure<'_>,
    ) -> AppResult<InboundEventTransition> {
        Ok(InboundEventTransition::Applied(
            InboundEventStatus::Retryable,
        ))
    }

    async fn dead_letter_inbound_event(
        &self,
        _failure: InboundEventFailure<'_>,
    ) -> AppResult<InboundEventTransition> {
        Ok(InboundEventTransition::Applied(
            InboundEventStatus::DeadLetter,
        ))
    }

    async fn reap_expired_inbound_events(&self) -> AppResult<InboundEventReaping> {
        Ok(InboundEventReaping::default())
    }

    async fn inbound_event_census(&self) -> AppResult<InboundEventCensus> {
        Ok(InboundEventCensus::default())
    }

    async fn purge_inbound_events(
        &self,
        _policy: InboundRetentionPolicy,
    ) -> AppResult<InboundEventRetention> {
        Ok(InboundEventRetention::default())
    }
}

fn claimed(company_id: uuid::Uuid, installation_id: Option<InstallationId>) -> ClaimedInboundEvent {
    let payload = InboundEventPayload::parse(br#"{"event":"message"}"#.to_vec()).unwrap();
    let id = InboundEventId::random();
    ClaimedInboundEvent {
        lease: ExecutionLease::new(
            id,
            WorkerId::random(),
            Utc::now() + chrono::Duration::seconds(INBOUND_EVENT_LEASE_SECONDS),
        ),
        record: InboundEventRecord {
            id,
            company_id,
            installation_id,
            transport: TransportKind::Slack,
            external_event_key: ExternalEventKey::parse(format!("Ev{id}")).unwrap(),
            correlation_id: CorrelationId::new(),
            payload_digest: InboundPayloadDigest::sha256(&payload),
            payload,
            content_type: None,
            safe_header_facts: SafeHeaderFacts::default(),
            attempt_count: 0,
            max_attempts: 5,
            received_at: Utc::now(),
        },
    }
}

#[test]
fn claimed_batches_are_round_robin_by_company_and_installation() {
    let busy_company = uuid::Uuid::new_v4();
    let quiet_company = uuid::Uuid::new_v4();
    let busy_installation = InstallationId::random();
    let quiet_installation = InstallationId::random();
    let mut batch = (0..6)
        .map(|_| claimed(busy_company, Some(busy_installation)))
        .collect::<Vec<_>>();
    batch.push(claimed(quiet_company, Some(quiet_installation)));

    let ordered = fair_order(batch);
    assert_eq!(ordered.len(), 7);
    assert_eq!(ordered[0].record.company_id, busy_company);
    assert_eq!(ordered[1].record.company_id, quiet_company);
}

struct CancellationProof(Arc<AtomicBool>);

impl Drop for CancellationProof {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn lease() -> ExecutionLease<InboundEventId> {
    ExecutionLease::new(
        InboundEventId::random(),
        WorkerId::random(),
        Utc::now() + chrono::Duration::from_std(EVENT_LEASE).unwrap(),
    )
}

#[tokio::test(start_paused = true)]
async fn lease_loss_cancels_the_real_decode_commit_future() {
    let queue = ScriptedQueue::new(false);
    let cancelled = Arc::new(AtomicBool::new(false));
    let proof = CancellationProof(cancelled.clone());
    let mut lease = lease();
    let supervision = supervise_event_lease(
        &mut lease,
        &queue,
        async move {
            let _proof = proof;
            std::future::pending::<()>().await
        },
        std::future::pending(),
    );
    tokio::pin!(supervision);
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(33)).await;

    assert!(matches!(supervision.await, SupervisedEvent::LeaseLost));
    assert_eq!(queue.renewals.load(Ordering::SeqCst), 1);
    assert!(cancelled.load(Ordering::SeqCst));
}

#[tokio::test]
async fn shutdown_cancels_and_awaits_active_work() {
    let queue = ScriptedQueue::new(true);
    let cancelled = Arc::new(AtomicBool::new(false));
    let proof = CancellationProof(cancelled.clone());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let mut lease = lease();
    let supervision = supervise_event_lease(
        &mut lease,
        &queue,
        async move {
            let _proof = proof;
            std::future::pending::<()>().await
        },
        async {
            let _ = shutdown_rx.await;
        },
    );
    tokio::pin!(supervision);
    tokio::task::yield_now().await;
    shutdown_tx.send(()).unwrap();

    assert!(matches!(supervision.await, SupervisedEvent::Shutdown));
    assert!(cancelled.load(Ordering::SeqCst));
}

/// A queue that yields one batch and is empty thereafter, so the real `run` loop can be driven
/// through both of the states shutdown has to be correct in.
struct LoopQueue {
    batches: Mutex<Vec<Vec<ClaimedInboundEvent>>>,
}

impl LoopQueue {
    fn new(batches: Vec<Vec<ClaimedInboundEvent>>) -> Self {
        Self {
            batches: Mutex::new(batches.into_iter().rev().collect()),
        }
    }
}

#[async_trait]
impl InboundEventInbox for LoopQueue {
    async fn store_authenticated(
        &self,
        _event: AuthenticatedInboundEvent,
    ) -> AppResult<InboundEventStoreOutcome> {
        Err(AppError::Internal("unused store".into()))
    }
}

#[async_trait]
impl InboundEventQueue for LoopQueue {
    async fn claim_inbound_events(
        &self,
        _owner: WorkerId,
        _lease_for: Duration,
        _limit: i64,
    ) -> AppResult<Vec<ClaimedInboundEvent>> {
        Ok(self.batches.lock().await.pop().unwrap_or_default())
    }

    async fn renew_inbound_event_lease(
        &self,
        _fence: &ExecutionLease<InboundEventId>,
        _until: chrono::DateTime<Utc>,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn complete_inbound_event(
        &self,
        _fence: &ExecutionLease<InboundEventId>,
    ) -> AppResult<InboundEventTransition> {
        Ok(InboundEventTransition::Applied(
            InboundEventStatus::Completed,
        ))
    }

    async fn ignore_inbound_event(
        &self,
        _fence: &ExecutionLease<InboundEventId>,
        _reason: InboundEventIgnoreReason,
    ) -> AppResult<InboundEventTransition> {
        Ok(InboundEventTransition::Applied(InboundEventStatus::Ignored))
    }

    async fn retry_inbound_event(
        &self,
        _failure: InboundEventFailure<'_>,
    ) -> AppResult<InboundEventTransition> {
        Ok(InboundEventTransition::Applied(
            InboundEventStatus::Retryable,
        ))
    }

    async fn dead_letter_inbound_event(
        &self,
        _failure: InboundEventFailure<'_>,
    ) -> AppResult<InboundEventTransition> {
        Ok(InboundEventTransition::Applied(
            InboundEventStatus::DeadLetter,
        ))
    }

    async fn reap_expired_inbound_events(&self) -> AppResult<InboundEventReaping> {
        Ok(InboundEventReaping::default())
    }

    async fn inbound_event_census(&self) -> AppResult<InboundEventCensus> {
        Ok(InboundEventCensus::default())
    }

    async fn purge_inbound_events(
        &self,
        _policy: InboundRetentionPolicy,
    ) -> AppResult<InboundEventRetention> {
        Ok(InboundEventRetention::default())
    }
}

struct RefusingCommitter;

#[async_trait]
impl InboundMessageCommitter for RefusingCommitter {
    async fn commit_inbound(
        &self,
        _request: InboundCommitRequest,
    ) -> AppResult<InboundCommitOutcome> {
        Err(AppError::Internal("no commit is expected here".into()))
    }
}

/// A decoder that never returns, holding a drop guard that proves whether its future was
/// cancelled rather than merely abandoned.
struct HangingDecoder {
    entered: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}

#[async_trait]
impl InboundEventDecoder for HangingDecoder {
    fn transport(&self) -> TransportKind {
        TransportKind::Slack
    }

    async fn decode(&self, _event: &InboundEventRecord) -> InboundEventDecodeOutcome {
        let _proof = CancellationProof(self.cancelled.clone());
        self.entered.store(true, Ordering::SeqCst);
        std::future::pending().await
    }
}

fn worker(
    queue: Arc<LoopQueue>,
    decoders: InboundEventDecoderRegistry,
) -> (Arc<InboundEventWorker>, broadcast::Sender<()>) {
    let (shutdown_tx, _) = broadcast::channel(1);
    let worker = Arc::new(InboundEventWorker::new(
        queue,
        Arc::new(RefusingCommitter),
        Arc::new(decoders),
        InboundEventWakeups::new(),
    ));
    (worker, shutdown_tx)
}

#[tokio::test]
async fn shutdown_between_iterations_returns_without_an_orphaned_loop() {
    let queue = Arc::new(LoopQueue::new(Vec::new()));
    let (worker, shutdown_tx) = worker(queue, InboundEventDecoderRegistry::new());
    let handle = tokio::spawn(Arc::clone(&worker).run(shutdown_tx.subscribe()));

    // The loop is idle in `wait_for_next` between claims; shutdown must win that select rather
    // than leaving the task parked until its poll interval elapses.
    tokio::task::yield_now().await;
    shutdown_tx.send(()).unwrap();

    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("the idle loop observed shutdown")
        .expect("the loop returned rather than panicking");
}

#[tokio::test]
async fn shutdown_during_active_work_cancels_the_decode_future_before_returning() {
    let entered = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let decoders = InboundEventDecoderRegistry::new()
        .register(Arc::new(HangingDecoder {
            entered: entered.clone(),
            cancelled: cancelled.clone(),
        }))
        .unwrap();
    let queue = Arc::new(LoopQueue::new(vec![vec![claimed(
        uuid::Uuid::new_v4(),
        Some(InstallationId::random()),
    )]]));
    let (worker, shutdown_tx) = worker(queue, decoders);
    let handle = tokio::spawn(Arc::clone(&worker).run(shutdown_tx.subscribe()));

    while !entered.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    shutdown_tx.send(()).unwrap();

    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("the busy loop observed shutdown")
        .expect("the loop returned rather than panicking");
    // Two independent paths reach the hung decode -- the batch loop's `JoinSet::shutdown`, and
    // the child's own subscription to the same broadcast -- so removing either one alone still
    // terminates. What is asserted here is the property that matters and that removing *both*
    // breaks: once `run` has returned, no decode future is still alive behind the process.
    assert!(cancelled.load(Ordering::SeqCst));
}
