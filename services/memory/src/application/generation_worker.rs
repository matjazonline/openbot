//! The supervised generation workers and the maintenance task (04B, D9, D13).
//!
//! A worker claims one operation (fairly, at most one in flight per tenant, spend reserved),
//! commits `dispatched` before anything is sent, and races the single provider call against a
//! local deadline (the last successful renewal's send instant + lease − margin, always before
//! the database's `lease_until`), a failed heartbeat (lease lost or cancel requested) and
//! shutdown. Losing that race drops the call, which aborts the connection, and settles the
//! operation as `unknown` (or `cancelled`), charged and never re-sent. A completed call is
//! settled in one fenced transaction; a settlement that cannot be written is retried from the
//! result in memory, never by calling the model again. The maintenance task reaps expired
//! leases and pending requests and sweeps retained inputs, with bounded batches and backoff.
use super::{
    generation::{GenerationOutcome, SkipReason, generate},
    ports::{
        Aged, Dispatch, GenerationJob, GenerationProvider, ProviderUsage, Reaped, Removed, Settled,
        Settlement, Store, Swept,
    },
    provider_breaker::{AUTH_KINDS, BREAKER_BASE, BREAKER_MAX, Permit, ProviderBreaker},
};
use crate::{application::contract::GenerationState, domain::error::ServiceError};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GenerationSettings {
    /// Concurrent workers: the global capacity (at most 4, user decision).
    pub workers: usize,
    pub lease: Duration,
    pub heartbeat: Duration,
    /// The local deadline is the last successful renewal's send instant + `lease - margin`.
    pub margin: Duration,
    /// The reaper frees a dispatched slot only after `lease_until + grace`.
    pub reaper_grace: Duration,
    /// Sleep when nothing is due.
    pub idle: Duration,
    /// Maintenance (reaper and sweeper) interval when there is no backlog.
    pub maintenance_tick: Duration,
    /// Rows per maintenance category per tick.
    pub batch: i64,
    /// `MEMORY_SHUTDOWN_GRACE_MS`: a call already dispatched when shutdown starts may finish
    /// within the first 80 % of it; the rest is left to settle it as `unknown(shutdown)`
    /// before the worker is joined (and aborted after the grace).
    pub shutdown_grace: Duration,
    /// The provider authentication breaker's first window and its bound (A4-1: 5 min to 1 h;
    /// tests shorten them).
    pub breaker_base: Duration,
    pub breaker_max: Duration,
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            workers: 4,
            lease: Duration::from_secs(90),
            heartbeat: Duration::from_secs(30),
            margin: Duration::from_secs(15),
            reaper_grace: Duration::from_secs(30),
            idle: Duration::from_secs(1),
            maintenance_tick: Duration::from_secs(5),
            batch: 500,
            shutdown_grace: Duration::from_secs(10),
            breaker_base: BREAKER_BASE,
            breaker_max: BREAKER_MAX,
        }
    }
}

/// What one worker pass did, for tests and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pass {
    /// Nothing could be claimed.
    Idle,
    /// The dispatch compare-and-swap lost (another worker, the reaper or a cancel).
    Stale,
    /// A re-check before dispatch finished the operation; nothing was sent.
    Stopped(&'static str),
    /// The provider call finished and the settlement committed.
    Settled(GenerationState),
    /// The call was dropped (`lease_lost`, `deadline` or `shutdown`) and then settled.
    Abandoned(&'static str, Settled),
    /// The settlement found the fence or state moved: this worker was stale.
    StaleSettle,
    /// The provider authentication breaker is open: nothing was claimed.
    Paused,
}

struct Lease {
    deadline: Instant,
    lost: bool,
}

struct AbortOnDrop(Option<tokio::task::JoinHandle<()>>);

impl AbortOnDrop {
    async fn join(mut self) {
        if let Some(handle) = self.0.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
}

#[derive(Clone)]
pub struct GenerationWorker {
    store: Arc<dyn Store>,
    provider: Arc<dyn GenerationProvider>,
    settings: GenerationSettings,
    owner: Uuid,
    breaker: Arc<ProviderBreaker>,
}

impl GenerationWorker {
    /// A worker with its own breaker; workers of one process share one with
    /// [`Self::with_breaker`].
    pub fn new(
        store: Arc<dyn Store>,
        provider: Arc<dyn GenerationProvider>,
        settings: GenerationSettings,
    ) -> Self {
        let breaker = Arc::new(ProviderBreaker::new(
            settings.breaker_base,
            settings.breaker_max,
        ));
        Self {
            store,
            provider,
            settings,
            owner: Uuid::now_v7(),
            breaker,
        }
    }

    pub fn with_breaker(mut self, breaker: Arc<ProviderBreaker>) -> Self {
        self.breaker = breaker;
        self
    }

    pub fn breaker(&self) -> &Arc<ProviderBreaker> {
        &self.breaker
    }

    pub fn owner(&self) -> Uuid {
        self.owner
    }

    /// Run until `shutdown`; database errors back off (1 s doubling to 30 s) and never stop it.
    pub async fn run(&self, shutdown: CancellationToken) {
        let mut backoff = Duration::from_secs(1);
        loop {
            if shutdown.is_cancelled() {
                return;
            }
            let wait = match self.pass(&shutdown).await {
                Ok(Pass::Idle | Pass::Paused) => self.settings.idle,
                Ok(pass) => {
                    backoff = Duration::from_secs(1);
                    tracing::info!(?pass, "generation pass");
                    Duration::ZERO
                }
                Err(error) => {
                    tracing::warn!(code = ?error.code, "generation pass failed; backing off");
                    let wait = backoff;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    wait
                }
            };
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    /// Claim, dispatch, call and settle at most one operation, unless the provider
    /// authentication breaker is open.
    pub async fn pass(&self, shutdown: &CancellationToken) -> Result<Pass, ServiceError> {
        let permit = self.breaker.permit();
        if permit == Permit::Open {
            return Ok(Pass::Paused);
        }
        let pass = self.pass_with(permit, shutdown).await;
        if permit == Permit::Probe {
            // A no-op once the probe's call tripped or closed the breaker.
            self.breaker.released();
        }
        pass
    }

    async fn pass_with(
        &self,
        permit: Permit,
        shutdown: &CancellationToken,
    ) -> Result<Pass, ServiceError> {
        let lease = self.settings.lease;
        let local = lease.saturating_sub(self.settings.margin);
        let sent = Instant::now();
        let Some(job) = self.store.claim_generation(self.owner, lease).await? else {
            return Ok(Pass::Idle);
        };
        if shutdown.is_cancelled() {
            // Nothing was sent: give the claim back.
            self.store
                .settle_generation(&job, &Settlement::Release)
                .await?;
            return Ok(Pass::Abandoned(
                "shutdown",
                Settled::Done(GenerationState::Accepted),
            ));
        }
        if Instant::now() >= sent + local {
            // The claim's lease may be gone; the reaper returns it.
            return Ok(Pass::Stale);
        }
        let sent = Instant::now();
        let prepared = match self.store.dispatch_generation(&job, lease).await? {
            Dispatch::Send(prepared) => prepared,
            Dispatch::Stale => return Ok(Pass::Stale),
            Dispatch::Stopped(code) => return Ok(Pass::Stopped(code)),
        };
        let held = Arc::new(Mutex::new(Lease {
            deadline: sent + local,
            lost: false,
        }));
        let stop = CancellationToken::new();
        // Aborted with this pass if the pass itself is dropped (an aborted worker), so a dead
        // worker never keeps renewing a lease.
        let heartbeat = AbortOnDrop(Some(tokio::spawn(heartbeat(
            self.store.clone(),
            job.clone(),
            held.clone(),
            self.settings.clone(),
            stop.clone(),
        ))));
        let call = generate(self.provider.as_ref(), &prepared);
        // A graceful shutdown lets a dispatched call finish within most of the grace (no new
        // claim starts meanwhile), so a deploy does not turn every call into an unknown.
        let drained = async {
            shutdown.cancelled().await;
            tokio::time::sleep(self.settings.shutdown_grace.mul_f64(0.8)).await;
        };
        let result = tokio::select! {
            result = call => Ok(result),
            reason = lost(&held) => Err(reason),
            () = drained => Err("shutdown"),
        };
        let pass = match result {
            Err(reason) => {
                // The call future is dropped here, which aborts its connection.
                let settled = self.settle(&job, &Settlement::Unknown(reason), &held).await;
                stop.cancel();
                heartbeat.join().await;
                return settled.map(|settled| Pass::Abandoned(reason, settled));
            }
            Ok(result) => {
                let settlement = match settlement(result) {
                    Settlement::Failed(kind, _) if AUTH_KINDS.contains(&kind.as_str()) => {
                        super::metrics::METRICS.inc(
                            "memory_provider_auth_failures_total",
                            &[("kind", &kind)],
                            1,
                        );
                        let delay = self.breaker.tripped(permit, &kind);
                        Settlement::Paused(kind, delay)
                    }
                    settlement => {
                        if provider_answered(&settlement) {
                            self.breaker.answered();
                        }
                        settlement
                    }
                };
                self.settle(&job, &settlement, &held).await
            }
        };
        stop.cancel();
        heartbeat.join().await;
        Ok(match pass? {
            Settled::Done(state) => Pass::Settled(state),
            Settled::Stale => Pass::StaleSettle,
        })
    }

    /// Write the settlement, retrying a failed write (never the call) while the lease is held.
    async fn settle(
        &self,
        job: &GenerationJob,
        settlement: &Settlement,
        held: &Mutex<Lease>,
    ) -> Result<Settled, ServiceError> {
        let mut wait = Duration::from_millis(200);
        loop {
            match self.store.settle_generation(job, settlement).await {
                Ok(settled) => return Ok(settled),
                Err(error) => {
                    let live = held
                        .lock()
                        .map(|lease| !lease.lost && Instant::now() + wait < lease.deadline)
                        .unwrap_or(false);
                    if !live {
                        // The output is lost: the operation stays dispatched until the reaper
                        // records it as unknown (D9: surfaced, never re-sent).
                        tracing::error!(
                            operation = %job.operation_id,
                            code = ?error.code,
                            "a generation result could not be saved before the lease ended"
                        );
                        return Err(error);
                    }
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(5));
                }
            }
        }
    }
}

fn settlement(result: Result<super::generation::Generated, ServiceError>) -> Settlement {
    let generated = match result {
        Ok(generated) => generated,
        // The destination is not authorized for the input level: refused before any call.
        Err(_) => {
            return Settlement::Failed(
                "destination_not_authorized".into(),
                ProviderUsage::default(),
            );
        }
    };
    let usage = generated.usage;
    match generated.outcome {
        GenerationOutcome::Saved(items) => Settlement::Saved(items, usage),
        GenerationOutcome::Skipped(SkipReason::NothingToSave) => {
            Settlement::Skipped("nothing_to_save", usage)
        }
        GenerationOutcome::Skipped(SkipReason::NothingToProcess) => {
            Settlement::Skipped("nothing_to_process", usage)
        }
        GenerationOutcome::Failed(code) => Settlement::Failed(code, usage),
        GenerationOutcome::Retryable(code) => Settlement::Retry(code),
        GenerationOutcome::Unknown => Settlement::Unknown("provider_unknown"),
    }
}

/// Whether the provider itself answered the call (so its credentials were accepted): not for a
/// call that was not sent, never made (privacy left nothing, the destination is not
/// authorized) or whose outcome is unknown.
fn provider_answered(settlement: &Settlement) -> bool {
    match settlement {
        Settlement::Saved(..) => true,
        Settlement::Skipped(code, _) => *code == "nothing_to_save",
        Settlement::Failed(code, _) => code != "destination_not_authorized",
        Settlement::Retry(code) => code != "not_sent",
        Settlement::Unknown(_) | Settlement::Release | Settlement::Paused(..) => false,
    }
}

/// Resolves with the reason once the lease is lost or the local deadline passes.
async fn lost(held: &Mutex<Lease>) -> &'static str {
    loop {
        let deadline = match held.lock() {
            Ok(lease) if !lease.lost => lease.deadline,
            _ => return "lease_lost",
        };
        let now = Instant::now();
        if now >= deadline {
            return "deadline";
        }
        tokio::time::sleep((deadline - now).min(Duration::from_millis(50))).await;
    }
}

async fn heartbeat(
    store: Arc<dyn Store>,
    job: GenerationJob,
    held: Arc<Mutex<Lease>>,
    settings: GenerationSettings,
    stop: CancellationToken,
) {
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            () = tokio::time::sleep(settings.heartbeat) => {}
        }
        let sent = Instant::now();
        match store.renew_generation(&job, settings.lease).await {
            Ok(true) => {
                if let Ok(mut lease) = held.lock() {
                    lease.deadline = sent + settings.lease.saturating_sub(settings.margin);
                }
            }
            Ok(false) => {
                if let Ok(mut lease) = held.lock() {
                    lease.lost = true;
                }
                return;
            }
            // A failed renewal leaves the deadline where it was, so the call stops in time.
            Err(_) => {}
        }
    }
}

/// One maintenance pass: the reaper, the input/evidence sweep, content removal of deleted
/// namespaces and tenants, and age-based retention. Every count is bounded by the batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MaintenancePass {
    pub reaped: Reaped,
    pub swept: Swept,
    pub removed: Removed,
    pub aged: Aged,
}

impl MaintenancePass {
    /// Count this pass (06A A2-1) and stamp the maintenance freshness gauge.
    pub fn record(&self) {
        let metrics = &super::metrics::METRICS;
        for (kind, count) in [
            ("released", self.reaped.released),
            ("abandoned", self.reaped.abandoned),
            ("expired", self.reaped.expired),
        ] {
            metrics.inc("memory_generation_reaped_total", &[("kind", kind)], count);
        }
        for (category, count) in [
            ("generation_requests", self.swept.requests),
            ("source_contents", self.swept.sources),
            ("removed_observations", self.removed.observations),
            ("removed_operations", self.removed.operations),
            ("removed_sources", self.removed.sources),
            ("removed_sessions", self.removed.sessions),
            ("removed_namespaces", self.removed.namespaces_marked),
            ("credential_labels", self.removed.labels),
            ("removed_tenant_rows", self.removed.tenant_rows),
            ("removed_tenants", self.removed.tenants_marked),
            ("expired_observations", self.aged.observations),
            ("idempotency_records", self.aged.idempotency_records),
            ("audit_events", self.aged.audit_events),
            ("spend_days", self.aged.spend_days),
            ("unreferenced_operations", self.aged.operations),
            ("unreferenced_sources", self.aged.sources),
        ] {
            metrics.inc(
                "memory_retention_rows_total",
                &[("category", category)],
                count,
            );
        }
        super::metrics::LAST_MAINTENANCE_UNIX
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
    }

    /// Every category's count; one at the batch size means more may be waiting.
    pub fn counts(&self) -> Vec<u64> {
        let mut counts = vec![
            self.reaped.released,
            self.reaped.abandoned,
            self.reaped.expired,
            self.swept.requests,
            self.swept.sources,
        ];
        counts.extend(self.removed.counts());
        counts.extend(self.aged.counts());
        counts
    }
}

/// The reaper and the retention sweeper on one task. A category that processed a full batch
/// runs again after 100 ms; otherwise the next tick waits `maintenance_tick`. An error backs
/// off from 1 s doubling to 5 min with jitter, so failing rows never make it hot-spin.
#[derive(Clone)]
pub struct Maintenance {
    store: Arc<dyn Store>,
    settings: GenerationSettings,
    ticks: Arc<AtomicU64>,
}

impl Maintenance {
    pub fn new(store: Arc<dyn Store>, settings: GenerationSettings) -> Self {
        Self {
            store,
            settings,
            ticks: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Ticks run so far (tests observe the backoff with it).
    pub fn ticks(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }

    pub async fn tick(&self) -> Result<MaintenancePass, ServiceError> {
        self.ticks.fetch_add(1, Ordering::Relaxed);
        let batch = self.settings.batch;
        let reaped = self
            .store
            .reap_generations(self.settings.reaper_grace, batch)
            .await?;
        let swept = self.store.sweep_retention(batch).await?;
        // 06A: content of deleted namespaces and tenants, then age-based retention.
        let removed = self.store.remove_deleted(batch).await?;
        let aged = self.store.sweep_aged(batch).await?;
        Ok(MaintenancePass {
            reaped,
            swept,
            removed,
            aged,
        })
    }

    pub async fn run(&self, shutdown: CancellationToken) {
        let mut backoff = Duration::from_secs(1);
        loop {
            if shutdown.is_cancelled() {
                return;
            }
            let wait = match self.tick().await {
                Ok(pass) => {
                    backoff = Duration::from_secs(1);
                    pass.record();
                    let full = pass
                        .counts()
                        .into_iter()
                        .any(|count| count as i64 >= self.settings.batch);
                    if pass != MaintenancePass::default() {
                        let MaintenancePass {
                            reaped,
                            swept,
                            removed,
                            aged,
                        } = pass;
                        tracing::info!(?reaped, ?swept, ?removed, ?aged, "maintenance");
                    }
                    if full {
                        Duration::from_millis(100)
                    } else {
                        self.settings.maintenance_tick
                    }
                }
                Err(error) => {
                    super::metrics::METRICS.inc("memory_maintenance_errors_total", &[], 1);
                    tracing::warn!(code = ?error.code, "generation maintenance failed; backing off");
                    let jitter = (Uuid::new_v4().as_u128() % 250) as u32;
                    let wait = backoff + backoff * jitter / 1000;
                    backoff = (backoff * 2).min(Duration::from_secs(300));
                    wait
                }
            };
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = tokio::time::sleep(wait) => {}
            }
        }
    }
}
