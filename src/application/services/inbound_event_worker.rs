//! Supervised worker for authenticated provider events.
//!
//! Claims are global, execution is bounded, and scheduling caps both company and installation
//! concurrency. Decode and canonical commit run inside the lease supervisor as the real future,
//! so lease loss or shutdown drops that future before this task returns.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    sync::Arc,
    time::{Duration, Instant as StdInstant},
};

use chrono::Utc;
use tokio::{
    sync::broadcast,
    task::JoinSet,
    time::{Instant, sleep_until},
};
use tracing::{info, warn};

use crate::{
    app_error::AppError,
    domain::monitoring::MonitoringService,
    entities::transport::{
        InboundEventErrorClass, InboundEventId, InboundEventIgnoreReason, InboundEventStatus,
        InstallationId,
    },
    transport::{
        ClaimedInboundEvent, ExecutionLease, INBOUND_EVENT_CLAIM_BATCH,
        INBOUND_EVENT_LEASE_SECONDS, InboundEventDecodeOutcome, InboundEventDecoderRegistry,
        InboundEventFailure, InboundEventQueue, InboundEventRecord, InboundEventTransition,
        InboundFailureDetail, InboundMessageCommitter, InboundRetentionPolicy, WorkerId,
    },
};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const ERROR_BACKOFF: Duration = Duration::from_secs(5);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
const RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);
const WORKER_CONCURRENCY: usize = 4;
const PER_COMPANY_CONCURRENCY: usize = 2;
const EVENT_LEASE: Duration = Duration::from_secs(INBOUND_EVENT_LEASE_SECONDS as u64);
const EVENT_DEADLINE: Duration = Duration::from_secs(90);
const _: () = assert!(EVENT_DEADLINE.as_secs() < EVENT_LEASE.as_secs());

/// A local latency hint for a producer. Correctness never depends on it: every worker polls and
/// reconciles durable rows at startup and on a timer.
#[derive(Clone)]
pub struct InboundEventWakeups {
    sender: broadcast::Sender<()>,
}

impl InboundEventWakeups {
    pub fn new() -> Self {
        Self {
            sender: broadcast::channel(64).0,
        }
    }

    pub fn notify(&self) {
        let _ = self.sender.send(());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.sender.subscribe()
    }
}

impl Default for InboundEventWakeups {
    fn default() -> Self {
        Self::new()
    }
}

pub struct InboundEventWorker {
    queue: Arc<dyn InboundEventQueue>,
    committer: Arc<dyn InboundMessageCommitter>,
    decoders: Arc<InboundEventDecoderRegistry>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    wakeups: InboundEventWakeups,
    worker_id: WorkerId,
}

impl InboundEventWorker {
    pub fn new(
        queue: Arc<dyn InboundEventQueue>,
        committer: Arc<dyn InboundMessageCommitter>,
        decoders: Arc<InboundEventDecoderRegistry>,
        wakeups: InboundEventWakeups,
    ) -> Self {
        Self {
            queue,
            committer,
            decoders,
            monitoring: None,
            wakeups,
            worker_id: WorkerId::random(),
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    pub const fn worker_id(&self) -> WorkerId {
        self.worker_id
    }

    pub async fn run(self: Arc<Self>, mut shutdown: broadcast::Receiver<()>) {
        info!(
            worker_id = %self.worker_id,
            transports = ?self.decoders.registered().collect::<Vec<_>>(),
            "Starting the inbound event worker"
        );
        let mut wakeups = self.wakeups.subscribe();
        let mut next_reconcile = Instant::now();
        let mut next_retention = Instant::now();

        loop {
            if Instant::now() >= next_reconcile {
                self.reconcile().await;
                next_reconcile = Instant::now() + RECONCILE_INTERVAL;
            }
            if Instant::now() >= next_retention {
                self.retain().await;
                next_retention = Instant::now() + RETENTION_INTERVAL;
            }

            let claimed = match self.claim().await {
                Ok(claimed) => claimed,
                Err(error) => {
                    warn!(%error, "The inbound event loop could not claim work");
                    if wait_for_next(ERROR_BACKOFF, &mut wakeups, &mut shutdown).await {
                        return;
                    }
                    continue;
                }
            };
            if claimed.is_empty() {
                if wait_for_next(POLL_INTERVAL, &mut wakeups, &mut shutdown).await {
                    return;
                }
                continue;
            }
            if self.process_batch(claimed, shutdown.resubscribe()).await {
                return;
            }
        }
    }

    async fn claim(&self) -> Result<Vec<ClaimedInboundEvent>, AppError> {
        let claimed = self
            .queue
            .claim_inbound_events(self.worker_id, EVENT_LEASE, INBOUND_EVENT_CLAIM_BATCH)
            .await?;
        if !claimed.is_empty()
            && let Some(monitoring) = self.monitoring.as_ref()
        {
            monitoring.increment_counter("inbound_events_claimed_total", claimed.len() as u64, &[]);
        }
        Ok(fair_order(claimed))
    }

    /// Run a batch with one active event per installation and at most two per company.
    async fn process_batch(
        self: &Arc<Self>,
        claimed: Vec<ClaimedInboundEvent>,
        mut shutdown: broadcast::Receiver<()>,
    ) -> bool {
        let mut waiting: VecDeque<_> = claimed.into();
        let mut running = JoinSet::new();
        let mut active_scopes = HashSet::new();
        let mut active_companies = HashMap::<uuid::Uuid, usize>::new();

        loop {
            while running.len() < WORKER_CONCURRENCY {
                let Some(event) = next_schedulable(&mut waiting, &active_scopes, &active_companies)
                else {
                    break;
                };
                let scope = event_scope(&event);
                active_scopes.insert(scope);
                *active_companies.entry(scope.0).or_default() += 1;
                let worker = Arc::clone(self);
                let event_shutdown = shutdown.resubscribe();
                running.spawn(async move {
                    worker.process_claimed(event, event_shutdown).await;
                    scope
                });
            }

            if running.is_empty() {
                return false;
            }
            tokio::select! {
                _ = shutdown.recv() => {
                    // Every child also observes this broadcast. `shutdown` is the bounded join:
                    // it aborts any child that has not yet returned and awaits its cancellation.
                    running.shutdown().await;
                    return true;
                }
                joined = running.join_next() => {
                    if let Some(Ok(scope)) = joined {
                        active_scopes.remove(&scope);
                        if let Some(count) = active_companies.get_mut(&scope.0) {
                            *count -= 1;
                            if *count == 0 {
                                active_companies.remove(&scope.0);
                            }
                        }
                    }
                }
            }
        }
    }

    async fn process_claimed(
        &self,
        claimed: ClaimedInboundEvent,
        mut shutdown: broadcast::Receiver<()>,
    ) {
        let started = StdInstant::now();
        let mut lease = claimed.lease;
        let work = self.decode_and_commit(&claimed.record, lease);
        let outcome = supervise_event_lease(&mut lease, self.queue.as_ref(), work, async move {
            let _ = shutdown.recv().await;
        })
        .await;

        match outcome {
            SupervisedEvent::Completed(result) => self.settle(&claimed, result).await,
            SupervisedEvent::Deadline => {
                self.fail_retryable(
                    &claimed,
                    lease,
                    InboundEventErrorClass::Deadline,
                    "Inbound event processing exceeded its deadline",
                )
                .await;
            }
            SupervisedEvent::LeaseLost => self.record_lease_lost(&claimed, "renewal_refused"),
            SupervisedEvent::RenewalFailed(error) => warn!(
                event_id = %claimed.record.id,
                installation_id = ?claimed.record.installation_id,
                correlation_id = %claimed.record.correlation_id,
                %error,
                "Could not renew an inbound event lease; its reaper owns recovery"
            ),
            SupervisedEvent::Shutdown => info!(
                event_id = %claimed.record.id,
                "Cancelled inbound event work during shutdown; its short lease will recover it"
            ),
        }
        if let Some(monitoring) = self.monitoring.as_ref() {
            monitoring.record_histogram(
                "inbound_event_processing_duration_ms",
                started.elapsed().as_secs_f64() * 1_000.0,
                &[("transport", claimed.record.transport.as_str())],
            );
        }
    }

    async fn decode_and_commit(
        &self,
        event: &InboundEventRecord,
        fence: ExecutionLease<InboundEventId>,
    ) -> WorkResult {
        let Some(decoder) = self.decoders.get(event.transport) else {
            return WorkResult::Terminal(
                InboundEventErrorClass::UnsupportedTransport,
                "No decoder is registered for this transport".into(),
            );
        };
        match decoder.decode(event).await {
            InboundEventDecodeOutcome::Ignore(reason) => WorkResult::Ignore(reason),
            InboundEventDecodeOutcome::Retry { class, detail } => {
                WorkResult::Retry(class, detail.into_string())
            }
            InboundEventDecodeOutcome::Terminal { class, detail } => {
                WorkResult::Terminal(class, detail.into_string())
            }
            InboundEventDecodeOutcome::Message(request) => {
                let mut request = *request;
                if let Err(detail) = validate_commit_request(event, &request) {
                    return WorkResult::Terminal(InboundEventErrorClass::InvalidPayload, detail);
                }
                request.claimed_event = Some(fence);
                match self.committer.commit_inbound(request).await {
                    Ok(_) => WorkResult::Committed,
                    Err(error) => {
                        let (class, terminal) = classify_commit_error(&error);
                        if terminal {
                            WorkResult::Terminal(class, error.to_string())
                        } else {
                            WorkResult::Retry(class, error.to_string())
                        }
                    }
                }
            }
        }
    }

    async fn settle(&self, claimed: &ClaimedInboundEvent, result: WorkResult) {
        let transition = match result {
            // Canonical commit completed the event inside its own transaction. A second queue
            // completion here would create the crash window this inbox exists to remove.
            WorkResult::Committed => {
                self.record_transition(claimed, InboundEventStatus::Completed, None);
                return;
            }
            WorkResult::Ignore(reason) => {
                let outcome = self
                    .queue
                    .ignore_inbound_event(&claimed.lease, reason)
                    .await;
                if let Some(monitoring) = self.monitoring.as_ref() {
                    monitoring.increment_counter(
                        "inbound_events_ignored_total",
                        1,
                        &[
                            ("transport", claimed.record.transport.as_str()),
                            ("reason", reason.as_str()),
                        ],
                    );
                }
                outcome
            }
            WorkResult::Retry(class, detail) => {
                self.failure_transition(claimed, class, &detail, false)
                    .await
            }
            WorkResult::Terminal(class, detail) => {
                self.failure_transition(claimed, class, &detail, true).await
            }
        };
        match transition {
            Ok(InboundEventTransition::Applied(status)) => {
                self.record_transition(claimed, status, None)
            }
            Ok(InboundEventTransition::LeaseLost) => {
                self.record_lease_lost(claimed, "settlement_refused")
            }
            Err(error) => warn!(
                event_id = %claimed.record.id,
                correlation_id = %claimed.record.correlation_id,
                %error,
                "Could not settle an inbound event; its lease reaper owns recovery"
            ),
        }
    }

    async fn fail_retryable(
        &self,
        claimed: &ClaimedInboundEvent,
        fence: ExecutionLease<InboundEventId>,
        class: InboundEventErrorClass,
        detail: &str,
    ) {
        let bounded = bounded_detail(detail);
        let transition = self
            .queue
            .retry_inbound_event(InboundEventFailure {
                fence: &fence,
                class,
                detail: bounded,
            })
            .await;
        match transition {
            Ok(InboundEventTransition::Applied(status)) => {
                self.record_transition(claimed, status, Some(class))
            }
            Ok(InboundEventTransition::LeaseLost) => {
                self.record_lease_lost(claimed, "deadline_settlement_refused")
            }
            Err(error) => {
                warn!(event_id = %claimed.record.id, %error, "Could not retry an inbound event")
            }
        }
    }

    async fn failure_transition(
        &self,
        claimed: &ClaimedInboundEvent,
        class: InboundEventErrorClass,
        detail: &str,
        terminal: bool,
    ) -> Result<InboundEventTransition, AppError> {
        let failure = InboundEventFailure {
            fence: &claimed.lease,
            class,
            detail: bounded_detail(detail),
        };
        if terminal {
            self.queue.dead_letter_inbound_event(failure).await
        } else {
            self.queue.retry_inbound_event(failure).await
        }
    }

    fn record_transition(
        &self,
        claimed: &ClaimedInboundEvent,
        status: InboundEventStatus,
        class: Option<InboundEventErrorClass>,
    ) {
        let level_needs_attention = status == InboundEventStatus::DeadLetter;
        if level_needs_attention {
            warn!(
                event_id = %claimed.record.id,
                installation_id = ?claimed.record.installation_id,
                correlation_id = %claimed.record.correlation_id,
                transport = %claimed.record.transport,
                status = %status,
                error_class = ?class,
                "An inbound event needs operator attention"
            );
        }
        if let Some(monitoring) = self.monitoring.as_ref() {
            monitoring.increment_counter(
                "inbound_event_transitions_total",
                1,
                &[
                    ("transport", claimed.record.transport.as_str()),
                    ("status", status.as_str()),
                ],
            );
        }
    }

    fn record_lease_lost(&self, claimed: &ClaimedInboundEvent, phase: &'static str) {
        warn!(
            event_id = %claimed.record.id,
            installation_id = ?claimed.record.installation_id,
            correlation_id = %claimed.record.correlation_id,
            execution_id = %claimed.lease.execution,
            phase,
            "Lost an inbound event lease; active work was cancelled"
        );
        if let Some(monitoring) = self.monitoring.as_ref() {
            monitoring.increment_counter(
                "inbound_event_lease_lost_total",
                1,
                &[
                    ("transport", claimed.record.transport.as_str()),
                    ("phase", phase),
                ],
            );
        }
    }

    async fn reconcile(&self) {
        match self.queue.reap_expired_inbound_events().await {
            Ok(reaping) if reaping.leases_expired > 0 => warn!(
                leases_expired = reaping.leases_expired,
                "Recovered expired inbound event leases"
            ),
            Ok(_) => {}
            Err(error) => warn!(%error, "Could not reap expired inbound event leases"),
        }
        match self.queue.inbound_event_census().await {
            Ok(census) => {
                if let Some(monitoring) = self.monitoring.as_ref() {
                    monitoring.record_gauge(
                        "inbound_events_dead_letter",
                        census.dead_letter as f64,
                        &[],
                    );
                    monitoring.record_gauge(
                        "inbound_events_oldest_ready_age_seconds",
                        census.oldest_ready_age.map_or(0.0, |age| age.as_secs_f64()),
                        &[],
                    );
                }
            }
            Err(error) => warn!(%error, "Could not census the inbound event queue"),
        }
    }

    async fn retain(&self) {
        match self
            .queue
            .purge_inbound_events(InboundRetentionPolicy::default())
            .await
        {
            Ok(deleted)
                if deleted.completed_deleted > 0
                    || deleted.ignored_deleted > 0
                    || deleted.dead_letters_deleted > 0 =>
            {
                info!(
                    completed = deleted.completed_deleted,
                    ignored = deleted.ignored_deleted,
                    dead_letters = deleted.dead_letters_deleted,
                    "Purged retained inbound event payloads"
                );
            }
            Ok(_) => {}
            Err(error) => warn!(%error, "Could not purge retained inbound event payloads"),
        }
    }
}

#[derive(Debug)]
enum WorkResult {
    Committed,
    Ignore(InboundEventIgnoreReason),
    Retry(InboundEventErrorClass, String),
    Terminal(InboundEventErrorClass, String),
}

enum SupervisedEvent<T> {
    Completed(T),
    Deadline,
    Shutdown,
    LeaseLost,
    RenewalFailed(AppError),
}

async fn supervise_event_lease<T, Work, Shutdown>(
    lease: &mut ExecutionLease<InboundEventId>,
    queue: &dyn InboundEventQueue,
    work: Work,
    shutdown: Shutdown,
) -> SupervisedEvent<T>
where
    Work: Future<Output = T>,
    Shutdown: Future<Output = ()>,
{
    let heartbeat = (EVENT_LEASE / 3) * 4 / 5;
    let deadline = Instant::now() + EVENT_DEADLINE;
    let mut next_heartbeat = Instant::now() + heartbeat;
    tokio::pin!(work);
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => return SupervisedEvent::Shutdown,
            _ = sleep_until(deadline) => return SupervisedEvent::Deadline,
            result = &mut work => return SupervisedEvent::Completed(result),
            _ = sleep_until(next_heartbeat) => {}
        }

        let renewed_until =
            Utc::now() + chrono::Duration::from_std(EVENT_LEASE).expect("event lease fits chrono");
        let renewed = {
            let renewal = queue.renew_inbound_event_lease(lease, renewed_until);
            tokio::pin!(renewal);
            tokio::select! {
                biased;
                _ = &mut shutdown => return SupervisedEvent::Shutdown,
                _ = sleep_until(deadline) => return SupervisedEvent::Deadline,
                result = &mut work => return SupervisedEvent::Completed(result),
                result = &mut renewal => result,
            }
        };
        match renewed {
            Ok(true) => *lease = (*lease).renewed_until(renewed_until),
            Ok(false) => return SupervisedEvent::LeaseLost,
            Err(error) => return SupervisedEvent::RenewalFailed(error),
        }
        next_heartbeat = Instant::now() + heartbeat;
    }
}

async fn wait_for_next(
    pause: Duration,
    wakeups: &mut broadcast::Receiver<()>,
    shutdown: &mut broadcast::Receiver<()>,
) -> bool {
    tokio::select! {
        _ = shutdown.recv() => true,
        _ = wakeups.recv() => false,
        _ = tokio::time::sleep(pause) => false,
    }
}

type EventScope = (uuid::Uuid, Option<InstallationId>);

fn event_scope(event: &ClaimedInboundEvent) -> EventScope {
    (event.record.company_id, event.record.installation_id)
}

fn fair_order(claimed: Vec<ClaimedInboundEvent>) -> Vec<ClaimedInboundEvent> {
    let mut groups: Vec<(EventScope, VecDeque<ClaimedInboundEvent>)> = Vec::new();
    let mut positions: HashMap<EventScope, usize> = HashMap::new();
    for event in claimed {
        let scope = event_scope(&event);
        if let Some(position) = positions.get(&scope).copied() {
            groups[position].1.push_back(event);
        } else {
            positions.insert(scope, groups.len());
            groups.push((scope, VecDeque::from([event])));
        }
    }
    let mut ordered = Vec::new();
    loop {
        let before = ordered.len();
        for (_, group) in &mut groups {
            if let Some(event) = group.pop_front() {
                ordered.push(event);
            }
        }
        if ordered.len() == before {
            return ordered;
        }
    }
}

fn next_schedulable(
    waiting: &mut VecDeque<ClaimedInboundEvent>,
    active_scopes: &HashSet<EventScope>,
    active_companies: &HashMap<uuid::Uuid, usize>,
) -> Option<ClaimedInboundEvent> {
    let position = waiting.iter().position(|event| {
        let scope = event_scope(event);
        !active_scopes.contains(&scope)
            && active_companies.get(&scope.0).copied().unwrap_or(0) < PER_COMPANY_CONCURRENCY
    })?;
    waiting.remove(position)
}

fn validate_commit_request(
    event: &InboundEventRecord,
    request: &crate::transport::InboundCommitRequest,
) -> Result<(), String> {
    if request.claimed_event.is_some() {
        return Err("A decoder attempted to choose its own inbound event fence".into());
    }
    if request.company_id != event.company_id {
        return Err("The decoded message belongs to a different company than its event".into());
    }
    if request.envelope.correlation_id != event.correlation_id {
        return Err("The decoded message replaced its event correlation id".into());
    }
    if request.envelope.source.event_key.as_ref() != Some(&event.external_event_key) {
        return Err("The decoded message does not carry its event delivery key".into());
    }
    Ok(())
}

fn classify_commit_error(error: &AppError) -> (InboundEventErrorClass, bool) {
    match error {
        AppError::BadRequest(_) | AppError::NotFound(_) | AppError::Conflict(_) => {
            (InboundEventErrorClass::Routing, true)
        }
        AppError::Timeout(_) => (InboundEventErrorClass::Deadline, false),
        AppError::Database(_) | AppError::Internal(_) | AppError::InvalidCredentials => {
            (InboundEventErrorClass::Internal, false)
        }
    }
}

fn bounded_detail(message: &str) -> InboundFailureDetail {
    let mut detail = message.to_string();
    while InboundFailureDetail::parse(detail.clone()).is_err() {
        let mut end = detail
            .len()
            .min(crate::transport::inbox::MAX_INBOUND_ERROR_DETAIL_BYTES);
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        detail.truncate(end);
        if detail.is_empty() {
            detail.push_str("Unclassified inbound event failure");
        }
    }
    InboundFailureDetail::parse(detail).expect("detail was bounded above")
}

#[cfg(test)]
#[path = "inbound_event_worker_tests.rs"]
mod tests;
