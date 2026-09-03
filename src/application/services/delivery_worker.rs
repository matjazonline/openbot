//! The worker that drains the generic delivery queue.
//!
//! One loop for every transport. It claims a bounded batch, orders it so no single tenant can eat
//! the batch, and for each delivery walks its frozen parts in order: mark the request started,
//! make the provider call, commit what the provider said. Every one of those writes is fenced on
//! the execution the claim minted, so a run that lost its lease stops rather than overwriting the
//! run that replaced it.
//!
//! Nothing here knows what a provider is. Which adapter speaks a transport comes from the
//! registry; what to do about an outcome comes from [`ProviderSendOutcome`]; whether an attempt was
//! spent comes from the queue. That is the point of the split: adding Slack adds a registry entry,
//! not a branch in this file.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{
    app_error::AppResult,
    domain::monitoring::MonitoringService,
    entities::transport::{DeliveryStatus, FailureClass, TransportKind},
    transport::{
        ClaimedDelivery, DELIVERY_CLAIM_BATCH, DELIVERY_LEASE_SECONDS, DeliveryFailure,
        DeliveryOutcome, DeliveryQueue, DeliveryRecord, Disposition, ExecutionLease, FailureDetail,
        PartResult, ProviderSendOutcome, StoredPart, TransportRegistry, WorkerId,
        queue::DeliveryReaping,
    },
};

/// How long the loop waits before looking for work again once the queue is empty.
///
/// Short, because this is the whole delay between an agent committing its answer and that answer
/// going out: the claim is one index scan against `message_deliveries_claimable_idx`.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long to wait after a failure of the loop itself, as distinct from a failed delivery.
const ERROR_BACKOFF: Duration = Duration::from_secs(5);

/// How often expired leases and orphaned dependencies are swept.
///
/// On the lease's own timescale rather than the poll loop's: a sweep every half second would be
/// one full-table predicate per half second to find, almost always, nothing.
const REAP_INTERVAL: Duration = Duration::from_secs(30);

/// How many deliveries one company may hold in a single batch.
///
/// The claim is global, as a queue claim must be -- tenant-scoping it would leave the other
/// tenants' rows unclaimed by anybody. Fairness is therefore a *scheduling* decision made here,
/// over what the claim returned: without it, one company that queued two hundred outreach mails
/// fills every batch and every other tenant's reply waits behind them.
const PER_COMPANY_BATCH_SHARE: usize = 4;

pub struct DeliveryWorker {
    queue: Arc<dyn DeliveryQueue>,
    transports: Arc<TransportRegistry>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    worker_id: WorkerId,
}

impl DeliveryWorker {
    pub fn new(queue: Arc<dyn DeliveryQueue>, transports: Arc<TransportRegistry>) -> Self {
        Self {
            queue,
            transports,
            monitoring: None,
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

    /// Drain the queue until shutdown, sweeping stranded rows on a slower timer.
    pub async fn run(self: Arc<Self>, mut shutdown: broadcast::Receiver<()>) {
        info!(
            worker_id = %self.worker_id,
            transports = ?self.transports.registered().collect::<Vec<_>>(),
            "Starting the delivery worker"
        );
        let mut next_reap = Instant::now();

        loop {
            if Instant::now() >= next_reap {
                self.reap().await;
                next_reap = Instant::now() + REAP_INTERVAL;
            }

            let pause = match self.drain_once(&shutdown).await {
                // A full batch probably has more behind it, so go straight back round.
                Ok(claimed) if claimed as i64 >= DELIVERY_CLAIM_BATCH => Duration::ZERO,
                Ok(_) => POLL_INTERVAL,
                Err(error) => {
                    warn!(%error, "The delivery loop could not claim work");
                    ERROR_BACKOFF
                }
            };

            tokio::select! {
                _ = shutdown.recv() => {
                    info!("Shutdown signal received. Stopping the delivery worker...");
                    return;
                }
                // Even a zero pause goes through the select, so a fast-draining loop stays
                // interruptible.
                _ = tokio::time::sleep(pause) => {}
            }
        }
    }

    /// Claim one batch and send it, returning how many rows were claimed.
    async fn drain_once(&self, shutdown: &broadcast::Receiver<()>) -> AppResult<usize> {
        let claimed = self
            .queue
            .claim_deliveries(
                self.worker_id,
                Duration::from_secs(DELIVERY_LEASE_SECONDS as u64),
                DELIVERY_CLAIM_BATCH,
            )
            .await?;
        let count = claimed.len();

        for delivery in fair_order(claimed) {
            self.deliver(delivery, &mut shutdown.resubscribe()).await;
        }
        Ok(count)
    }

    /// Send one claimed delivery's parts, in order, until it is finished or this run loses it.
    async fn deliver(&self, claimed: ClaimedDelivery, shutdown: &mut broadcast::Receiver<()>) {
        let ClaimedDelivery {
            lease,
            record,
            mut parts,
        } = claimed;

        let Ok(transport) = self.transports.require(record.transport) else {
            // This deployment cannot speak the transport the row names. Terminal rather than
            // retryable: a missing adapter is configuration, and five backoffs will not install it.
            self.settle_without_provider(
                &lease,
                FailureClass::Internal,
                &format!("No {} adapter is configured", record.transport),
                Disposition::Terminal,
            )
            .await;
            return;
        };
        let sender = Arc::clone(transport.sender());

        while let Some(index) = parts.iter().position(|part| part.status.is_unfinished()) {
            // Checked before every provider call rather than once at the top: a multi-part send
            // can outlive its lease between parts, and the reaper may already have handed the row
            // to somebody else.
            if shutdown.try_recv().is_ok() {
                self.release_on_shutdown(&lease, &record).await;
                return;
            }

            match self
                .send_part(&lease, &record, &parts[index], sender.as_ref())
                .await
            {
                PartProgress::Continue(status) => {
                    parts[index].status = status;
                }
                PartProgress::Finished(status) => {
                    self.record_settled(&record, status);
                    return;
                }
                PartProgress::LeaseLost => {
                    warn!(
                        delivery_id = %record.id,
                        "Lost the lease on a delivery mid-send; the execution that holds it now owns the outcome"
                    );
                    return;
                }
            }
        }
    }

    /// One part: fence the start, call the provider, fence the result.
    async fn send_part(
        &self,
        lease: &ExecutionLease<crate::entities::transport::DeliveryId>,
        record: &DeliveryRecord,
        part: &StoredPart,
        sender: &dyn crate::transport::TransportSender,
    ) -> PartProgress {
        // Committed *before* the call. It is what lets a reaper tell "this crashed before the
        // provider saw it" from "this may already have been accepted", and getting that backwards
        // is how one message becomes two.
        match self.queue.begin_part(lease, part.id).await {
            Ok(DeliveryOutcome::Applied(_)) => {}
            Ok(DeliveryOutcome::LeaseLost) => return PartProgress::LeaseLost,
            Err(error) => {
                warn!(delivery_id = %record.id, %error, "Could not start a delivery part");
                return PartProgress::LeaseLost;
            }
        }

        let outcome = sender.send(record, &part.rendered).await;
        self.record_outcome_metric(record.transport, &outcome);

        match self
            .queue
            .complete_part(PartResult {
                fence: lease,
                part_id: part.id,
                outcome: &outcome,
            })
            .await
        {
            Ok(DeliveryOutcome::Applied(DeliveryStatus::Sending)) => {
                PartProgress::Continue(part_status_after(&outcome))
            }
            Ok(DeliveryOutcome::Applied(status)) => PartProgress::Finished(status),
            Ok(DeliveryOutcome::LeaseLost) => PartProgress::LeaseLost,
            // The provider has already acted and this run cannot say what it did. Reported rather
            // than retried: the lease will lapse and the reaper classifies the part from its
            // `request_started_at`, which is exactly the ambiguous case.
            Err(error) => {
                warn!(
                    delivery_id = %record.id,
                    %error,
                    "Could not record what a provider said about a delivery part"
                );
                PartProgress::LeaseLost
            }
        }
    }

    /// End a delivery this worker cannot even attempt.
    async fn settle_without_provider(
        &self,
        lease: &ExecutionLease<crate::entities::transport::DeliveryId>,
        class: FailureClass,
        detail: &str,
        disposition: Disposition,
    ) {
        let Some(detail) = bounded_detail(detail) else {
            return;
        };
        if let Err(error) = self
            .queue
            .fail_delivery(DeliveryFailure {
                fence: lease,
                class,
                detail,
                disposition,
            })
            .await
        {
            warn!(delivery_id = %lease.row, %error, "Could not end a delivery attempt");
        }
    }

    /// Give the claim back rather than letting it lapse.
    ///
    /// Reached only before a provider request has started, which is what makes it safe: nothing was
    /// sent, so no attempt is spent and the row is immediately claimable again. Letting the lease
    /// expire instead would strand it for a full lease period *and* charge it an attempt, which is
    /// how a rolling deploy turns into a delivery delay.
    async fn release_on_shutdown(
        &self,
        lease: &ExecutionLease<crate::entities::transport::DeliveryId>,
        record: &DeliveryRecord,
    ) {
        match self.queue.release_delivery(lease).await {
            Ok(DeliveryOutcome::Applied(_)) => info!(
                delivery_id = %record.id,
                "Released a claimed delivery on shutdown; it is immediately claimable again"
            ),
            Ok(DeliveryOutcome::LeaseLost) => {}
            Err(error) => warn!(
                delivery_id = %record.id,
                %error,
                "Could not release a claimed delivery on shutdown; its lease will expire instead"
            ),
        }
    }

    async fn reap(&self) {
        match self.queue.reap_expired_deliveries().await {
            Ok(reaping) if reaping.is_quiet() => {}
            Ok(DeliveryReaping {
                leases_expired,
                dependencies_orphaned,
            }) => warn!(
                leases_expired,
                dependencies_orphaned, "Swept deliveries that were stranded"
            ),
            Err(error) => warn!(%error, "Could not sweep stranded deliveries"),
        }
    }

    fn record_settled(&self, record: &DeliveryRecord, status: DeliveryStatus) {
        if status.needs_attention() {
            warn!(
                delivery_id = %record.id,
                correlation_id = %record.correlation_id,
                transport = %record.transport,
                status = %status,
                "A delivery finished in a state that needs a human"
            );
        }
        if let Some(monitoring) = self.monitoring.as_ref() {
            monitoring.increment_counter(
                "delivery_settled_total",
                1,
                &[
                    ("transport", record.transport.as_str()),
                    ("status", status.as_str()),
                ],
            );
        }
    }

    fn record_outcome_metric(&self, transport: TransportKind, outcome: &ProviderSendOutcome) {
        let Some(monitoring) = self.monitoring.as_ref() else {
            return;
        };
        // Retries, terminal failures and ambiguity are counted apart, because an alert that cannot
        // tell them apart is an alert that fires on a rate limit and stays quiet on a duplicate.
        let kind = match outcome {
            ProviderSendOutcome::Delivered { .. } => "delivered",
            ProviderSendOutcome::RetryAfter { .. } => "rate_limited",
            ProviderSendOutcome::Retryable { .. } => "retryable",
            ProviderSendOutcome::OutcomeUnknown { .. } => "outcome_unknown",
            ProviderSendOutcome::Terminal { .. } => "terminal",
        };
        monitoring.increment_counter(
            "delivery_provider_outcomes_total",
            1,
            &[("transport", transport.as_str()), ("outcome", kind)],
        );
    }
}

/// Where one part left its delivery.
enum PartProgress {
    /// The delivery still holds its lease and has more parts to send.
    Continue(crate::entities::transport::DeliveryPartStatus),
    /// The delivery reached a terminal state and released its lease.
    Finished(DeliveryStatus),
    /// This run no longer owns the delivery. Stop touching it.
    LeaseLost,
}

/// Interleave a claimed batch so no one tenant is served before every other tenant is started.
///
/// Round-robin over companies, taking at most [`PER_COMPANY_BATCH_SHARE`] from each pass. The
/// claim itself stays global and ordered by due time, which is what keeps it fair *over time*;
/// this is what keeps it fair *within* a batch, where a hundred rows from one company would
/// otherwise be sent before the first row of anybody else's.
fn fair_order(claimed: Vec<ClaimedDelivery>) -> Vec<ClaimedDelivery> {
    let mut by_company: Vec<(uuid::Uuid, Vec<ClaimedDelivery>)> = Vec::new();
    let mut index: HashMap<uuid::Uuid, usize> = HashMap::new();
    for delivery in claimed {
        let company_id = delivery.record.company_id;
        match index.get(&company_id) {
            // Insertion order is due order, because the claim returned them that way.
            Some(position) => by_company[*position].1.push(delivery),
            None => {
                index.insert(company_id, by_company.len());
                by_company.push((company_id, vec![delivery]));
            }
        }
    }

    let mut ordered = Vec::new();
    loop {
        let before = ordered.len();
        for (_, rows) in by_company.iter_mut() {
            let take = rows.len().min(PER_COMPANY_BATCH_SHARE);
            ordered.extend(rows.drain(..take));
        }
        // A pass that moved nothing means every company is drained.
        if ordered.len() == before {
            return ordered;
        }
    }
}

/// The part status one provider outcome leaves behind, mirroring what the queue just wrote.
fn part_status_after(
    outcome: &ProviderSendOutcome,
) -> crate::entities::transport::DeliveryPartStatus {
    crate::transport::PartTransition::of(outcome).status
}

/// A message bounded to what a failure detail may hold, truncated rather than dropped.
fn bounded_detail(message: &str) -> Option<FailureDetail> {
    let mut bounded = message.to_string();
    while FailureDetail::parse(bounded.clone()).is_err() && !bounded.is_empty() {
        bounded.truncate(bounded.len().saturating_sub(bounded.len() / 4 + 1));
    }
    FailureDetail::parse(bounded).ok()
}

#[cfg(test)]
#[path = "delivery_worker_tests.rs"]
mod tests;
