use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct Settlement {
    entered: Notify,
    proceed: Notify,
    block: AtomicBool,
    outcomes: Mutex<Vec<ProviderSendOutcome>>,
}

struct LifecycleQueue {
    inner: ScriptedQueue,
    batch: Mutex<Vec<ClaimedDelivery>>,
    settlement: Arc<Settlement>,
}

#[async_trait::async_trait]
impl DeliveryQueue for LifecycleQueue {
    async fn census_stuck_deliveries(
        &self,
        after: Duration,
    ) -> AppResult<crate::entities::stuck_work::StuckWorkCensus> {
        self.inner.census_stuck_deliveries(after).await
    }
    async fn claim_deliveries(
        &self,
        _: WorkerId,
        _: Duration,
        _: i64,
    ) -> AppResult<Vec<ClaimedDelivery>> {
        Ok(std::mem::take(&mut *self.batch.lock().unwrap()))
    }
    async fn renew_delivery_leases(
        &self,
        fences: &[ExecutionLease<DeliveryId>],
        lease: Duration,
    ) -> AppResult<Vec<DeliveryId>> {
        self.inner.renew_delivery_leases(fences, lease).await
    }
    async fn begin_part(
        &self,
        fence: &ExecutionLease<DeliveryId>,
        part: DeliveryPartId,
    ) -> AppResult<DeliveryOutcome> {
        self.inner.begin_part(fence, part).await
    }
    async fn complete_part(&self, result: PartResult<'_>) -> AppResult<DeliveryOutcome> {
        self.settlement.entered.notify_one();
        if self.settlement.block.load(Ordering::SeqCst) {
            self.settlement.proceed.notified().await;
        }
        self.settlement
            .outcomes
            .lock()
            .unwrap()
            .push(result.outcome.clone());
        self.inner.complete_part(result).await
    }
    async fn fail_delivery(&self, failure: DeliveryFailure<'_>) -> AppResult<DeliveryOutcome> {
        self.inner.fail_delivery(failure).await
    }
    async fn release_delivery(
        &self,
        fence: &ExecutionLease<DeliveryId>,
    ) -> AppResult<DeliveryOutcome> {
        self.inner.release_delivery(fence).await
    }
    async fn reap_expired_deliveries(&self) -> AppResult<DeliveryReaping> {
        self.inner.reap_expired_deliveries().await
    }
}

struct PendingSender {
    entered: Arc<Notify>,
    dropped: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}
struct DropSignal(Arc<AtomicBool>);
impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
#[async_trait::async_trait]
impl TransportSender for PendingSender {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }
    async fn send(&self, _: &DeliveryRecord, _: &RenderedPart) -> ProviderSendOutcome {
        let _drop = DropSignal(self.dropped.clone());
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        std::future::pending().await
    }
}

fn lifecycle_worker(
    sender: Arc<dyn TransportSender>,
    parts: usize,
) -> (Arc<DeliveryWorker>, Arc<LifecycleQueue>) {
    let queue = Arc::new(LifecycleQueue {
        inner: ScriptedQueue::default(),
        batch: Mutex::new(vec![claimed(
            uuid::Uuid::new_v4(),
            (0..parts)
                .map(|i| stored_part(i as u16, DeliveryPartStatus::Prepared))
                .collect(),
        )]),
        settlement: Arc::new(Settlement::default()),
    });
    let transports = TransportRegistry::new()
        .register(Arc::new(NoopRenderer), sender)
        .unwrap();
    (
        Arc::new(DeliveryWorker::new(queue.clone(), Arc::new(transports))),
        queue,
    )
}

#[tokio::test]
async fn channel_reply_delivery_shutdown_drops_actual_provider_and_records_unknown() {
    let entered = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (worker, queue) = lifecycle_worker(
        Arc::new(PendingSender {
            entered: entered.clone(),
            dropped: dropped.clone(),
            calls: calls.clone(),
        }),
        2,
    );
    let (shutdown, receiver) = broadcast::channel(1);
    let owner = tokio::spawn(worker.run(receiver));
    entered.notified().await;
    shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), owner)
        .await
        .unwrap()
        .unwrap();
    assert!(
        dropped.load(Ordering::SeqCst),
        "the actual provider future is cancelled"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no second part starts");
    assert!(matches!(
        queue.settlement.outcomes.lock().unwrap().as_slice(),
        [ProviderSendOutcome::OutcomeUnknown { .. }]
    ));
}

#[tokio::test]
async fn channel_reply_delivery_shutdown_preserves_known_result_during_settlement() {
    let (worker, queue) = lifecycle_worker(
        Arc::new(ScriptedSender::new(ProviderSendOutcome::Delivered {
            provider_key: None,
        })),
        2,
    );
    queue.settlement.block.store(true, Ordering::SeqCst);
    *queue.inner.parts_still_sending.lock().unwrap() = 1;
    let (shutdown, receiver) = broadcast::channel(1);
    let owner = tokio::spawn(worker.run(receiver));
    queue.settlement.entered.notified().await;
    shutdown.send(()).unwrap();
    tokio::task::yield_now().await;
    queue.settlement.proceed.notify_one();
    tokio::time::timeout(Duration::from_secs(5), owner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(queue.inner.started.lock().unwrap().len(), 1);
    assert_eq!(queue.inner.completions.lock().unwrap().len(), 1);
    assert_eq!(
        *queue.inner.released.lock().unwrap(),
        1,
        "the unstarted remainder is released"
    );
    assert!(matches!(
        queue.settlement.outcomes.lock().unwrap().as_slice(),
        [ProviderSendOutcome::Delivered { .. }]
    ));
}

#[tokio::test]
async fn channel_reply_delivery_shutdown_bounds_stuck_known_settlement() {
    let (worker, queue) = lifecycle_worker(
        Arc::new(ScriptedSender::new(ProviderSendOutcome::Delivered {
            provider_key: None,
        })),
        1,
    );
    queue.settlement.block.store(true, Ordering::SeqCst);
    let (shutdown, receiver) = broadcast::channel(1);
    let owner = tokio::spawn(worker.run(receiver));
    queue.settlement.entered.notified().await;
    shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), owner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        queue.inner.started.lock().unwrap().len(),
        1,
        "the durable request marker precedes the provider"
    );
    assert!(queue.inner.completions.lock().unwrap().is_empty());
    assert!(
        queue.settlement.outcomes.lock().unwrap().is_empty(),
        "no known result is fabricated as committed"
    );
    assert_eq!(
        *queue.inner.released.lock().unwrap(),
        0,
        "an unsettled request is left for fenced recovery"
    );
}

#[tokio::test]
async fn channel_reply_delivery_ready_result_wins_simultaneous_shutdown() {
    struct SignalSender(broadcast::Sender<()>);
    #[async_trait::async_trait]
    impl TransportSender for SignalSender {
        fn transport(&self) -> TransportKind {
            TransportKind::Email
        }
        async fn send(&self, _: &DeliveryRecord, _: &RenderedPart) -> ProviderSendOutcome {
            self.0.send(()).unwrap();
            ProviderSendOutcome::Delivered { provider_key: None }
        }
    }
    let (shutdown, mut receiver) = broadcast::channel(1);
    let result = call_provider(
        &SignalSender(shutdown),
        &record(uuid::Uuid::new_v4()),
        &stored_part(0, DeliveryPartStatus::Prepared).rendered,
        &mut receiver,
    )
    .await;
    assert!(!result.interrupted);
    assert!(matches!(
        result.outcome,
        ProviderSendOutcome::Delivered { .. }
    ));
}

#[tokio::test(start_paused = true)]
async fn channel_reply_delivery_provider_deadline_cancels_actual_call() {
    let dropped = Arc::new(AtomicBool::new(false));
    let sender = PendingSender {
        entered: Arc::new(Notify::new()),
        dropped: dropped.clone(),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let (_shutdown, mut receiver) = broadcast::channel(1);
    let result = call_provider(
        &sender,
        &record(uuid::Uuid::new_v4()),
        &stored_part(0, DeliveryPartStatus::Prepared).rendered,
        &mut receiver,
    )
    .await;
    assert!(dropped.load(Ordering::SeqCst));
    assert!(matches!(
        result.outcome,
        ProviderSendOutcome::OutcomeUnknown { .. }
    ));
    assert!(
        !result.interrupted,
        "a deadline quarantines this delivery without stopping unrelated work"
    );
}

#[tokio::test]
async fn channel_reply_delivery_owner_observes_shutdown_before_claiming() {
    let (worker, queue) = lifecycle_worker(
        Arc::new(ScriptedSender::new(ProviderSendOutcome::Delivered {
            provider_key: None,
        })),
        1,
    );
    let (shutdown, receiver) = broadcast::channel(1);
    shutdown.send(()).unwrap();
    worker.run(receiver).await;
    assert_eq!(queue.batch.lock().unwrap().len(), 1);
    assert!(queue.inner.started.lock().unwrap().is_empty());
}
