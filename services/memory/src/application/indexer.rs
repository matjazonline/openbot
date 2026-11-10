//! The supervised indexing worker (D7, D13). It claims due rows under a lease and a fence,
//! renews the lease on a heartbeat, stops work at a local deadline that always falls before the
//! database lease, embeds only the already-saved text it claimed, and commits parts only while
//! the fence and revision still match. Keyword recall never waits for it.
use super::{
    embedding::EmbeddingPool,
    ports::{IndexCommit, IndexJob, Store},
};
use crate::domain::error::ServiceError;
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct IndexerSettings {
    /// Rows per claim.
    pub batch: usize,
    pub lease: Duration,
    pub heartbeat: Duration,
    /// The local deadline is the last successful renewal's send instant + `lease - margin`.
    pub margin: Duration,
    /// Sleep when nothing is due.
    pub idle: Duration,
}

impl Default for IndexerSettings {
    fn default() -> Self {
        Self {
            batch: 8,
            lease: Duration::from_secs(60),
            heartbeat: Duration::from_secs(20),
            margin: Duration::from_secs(15),
            idle: Duration::from_secs(1),
        }
    }
}

/// What one pass did, for tests and logs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexPass {
    pub claimed: usize,
    pub indexed: usize,
    pub failed: usize,
    pub stale: usize,
    pub deferred: usize,
    /// Lease lost or local deadline passed: the work was dropped and nothing committed.
    pub abandoned: usize,
}

struct Lease {
    deadline: Instant,
    held: Vec<IndexJob>,
    lost: HashSet<Uuid>,
}

#[derive(Clone)]
pub struct Indexer {
    store: Arc<dyn Store>,
    pool: Arc<EmbeddingPool>,
    settings: IndexerSettings,
}

impl Indexer {
    pub fn new(store: Arc<dyn Store>, pool: Arc<EmbeddingPool>, settings: IndexerSettings) -> Self {
        Self {
            store,
            pool,
            settings,
        }
    }

    /// Run until `shutdown`; database errors back off (1 s doubling to 30 s) and never stop it.
    pub async fn run(&self, shutdown: CancellationToken) {
        let mut backoff = Duration::from_secs(1);
        loop {
            if shutdown.is_cancelled() {
                return;
            }
            let wait = match self.pass(&shutdown).await {
                Ok(pass) if pass.claimed > 0 => {
                    backoff = Duration::from_secs(1);
                    tracing::info!(
                        claimed = pass.claimed,
                        indexed = pass.indexed,
                        failed = pass.failed,
                        stale = pass.stale,
                        deferred = pass.deferred,
                        abandoned = pass.abandoned,
                        "indexing pass"
                    );
                    Duration::ZERO
                }
                Ok(_) => self.settings.idle,
                Err(error) => {
                    tracing::warn!(code = ?error.code, "indexing pass failed; backing off");
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

    /// Claim one batch and work through it.
    pub async fn run_once(&self) -> Result<IndexPass, ServiceError> {
        self.pass(&CancellationToken::new()).await
    }

    /// One pass that stops at `shutdown`: the job in progress is cancelled (nothing is
    /// committed), the rest are dropped, and their leases simply expire.
    pub async fn pass(&self, shutdown: &CancellationToken) -> Result<IndexPass, ServiceError> {
        let sent = Instant::now();
        let jobs = self
            .store
            .claim_index_jobs(self.settings.batch, self.settings.lease)
            .await?;
        let mut pass = IndexPass {
            claimed: jobs.len(),
            ..IndexPass::default()
        };
        if jobs.is_empty() {
            return Ok(pass);
        }
        let lease = Arc::new(Mutex::new(Lease {
            deadline: sent + self.settings.lease.saturating_sub(self.settings.margin),
            held: jobs.clone(),
            lost: HashSet::new(),
        }));
        let stop = CancellationToken::new();
        let heartbeat = tokio::spawn(heartbeat(
            self.store.clone(),
            lease.clone(),
            self.settings.clone(),
            stop.clone(),
        ));
        for job in jobs {
            if shutdown.is_cancelled() {
                pass.abandoned += 1;
                continue;
            }
            self.work(&job, &lease, shutdown, &mut pass).await;
            if let Ok(mut lease) = lease.lock() {
                lease
                    .held
                    .retain(|held| held.observation_id != job.observation_id);
            }
        }
        stop.cancel();
        let _ = heartbeat.await;
        Ok(pass)
    }

    async fn work(
        &self,
        job: &IndexJob,
        lease: &Arc<Mutex<Lease>>,
        shutdown: &CancellationToken,
        pass: &mut IndexPass,
    ) {
        if !still_held(lease, job.observation_id) {
            pass.abandoned += 1;
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let embedding = self
            .pool
            .embed_parts(job.title.clone(), job.body.clone(), cancel.clone());
        let outcome = tokio::select! {
            outcome = embedding => Some(outcome),
            () = lost(lease, job.observation_id) => None,
            () = shutdown.cancelled() => None,
        };
        let result = match outcome {
            None | Some(Ok(None)) => {
                cancel.store(true, Ordering::Release);
                pass.abandoned += 1;
                return;
            }
            Some(Ok(Some(parts))) => {
                if !still_held(lease, job.observation_id) {
                    pass.abandoned += 1;
                    return;
                }
                self.store
                    .commit_index(job, self.pool.model_id(), &parts)
                    .await
                    .map(|commit| match commit {
                        IndexCommit::Indexed => pass.indexed += 1,
                        IndexCommit::Stale => pass.stale += 1,
                        IndexCommit::Deferred => pass.deferred += 1,
                    })
            }
            Some(Err(failure)) => {
                tracing::warn!(
                    observation = %job.observation_id,
                    code = failure.as_str(),
                    "indexing failed"
                );
                self.store.fail_index(job, failure).await.map(|recorded| {
                    if recorded {
                        pass.failed += 1
                    } else {
                        pass.stale += 1
                    }
                })
            }
        };
        if let Err(error) = result {
            // The lease expires and another pass retries; nothing was committed.
            tracing::warn!(code = ?error.code, "indexing commit failed");
            pass.abandoned += 1;
        }
    }
}

/// Aborts the task when dropped, so an aborted supervisor takes its worker with it.
struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Run `worker` until `shutdown`, restarting it after a panic (or an unexpected return) with
/// backoff from `initial` doubling to 30 s, so a worker (indexing, generation, maintenance)
/// never stops silently. Returns the number of restarts.
pub async fn supervise<F, Fut>(
    name: &'static str,
    shutdown: CancellationToken,
    initial: Duration,
    mut worker: F,
) -> u32
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let mut restarts = 0;
    let mut backoff = initial;
    loop {
        let handle = tokio::spawn(worker());
        let guard = AbortOnDrop(handle.abort_handle());
        let result = handle.await;
        drop(guard);
        if shutdown.is_cancelled() {
            return restarts;
        }
        match result {
            Err(error) if error.is_panic() => {
                tracing::error!(worker = name, "a supervised worker panicked; restarting it");
            }
            _ => tracing::error!(
                worker = name,
                "a supervised worker stopped unexpectedly; restarting it"
            ),
        }
        restarts += 1;
        tokio::select! {
            () = shutdown.cancelled() => return restarts,
            () = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

fn still_held(lease: &Mutex<Lease>, id: Uuid) -> bool {
    lease
        .lock()
        .map(|lease| !lease.lost.contains(&id) && Instant::now() < lease.deadline)
        .unwrap_or(false)
}

/// Resolves when the job's lease is lost or the local deadline passes.
async fn lost(lease: &Mutex<Lease>, id: Uuid) {
    loop {
        let deadline = match lease.lock() {
            Ok(lease) if !lease.lost.contains(&id) => lease.deadline,
            _ => return,
        };
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        tokio::time::sleep((deadline - now).min(Duration::from_millis(100))).await;
    }
}

async fn heartbeat(
    store: Arc<dyn Store>,
    lease: Arc<Mutex<Lease>>,
    settings: IndexerSettings,
    stop: CancellationToken,
) {
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            () = tokio::time::sleep(settings.heartbeat) => {}
        }
        let held: Vec<IndexJob> = match lease.lock() {
            Ok(lease) => lease
                .held
                .iter()
                .filter(|job| !lease.lost.contains(&job.observation_id))
                .cloned()
                .collect(),
            Err(_) => return,
        };
        if held.is_empty() {
            continue;
        }
        let sent = Instant::now();
        // A failed renewal leaves the deadline where it was, so the work stops in time.
        if let Ok(renewed) = store.renew_index_leases(&held, settings.lease).await
            && let Ok(mut lease) = lease.lock()
        {
            lease.deadline = sent + settings.lease.saturating_sub(settings.margin);
            for job in held {
                if !renewed.contains(&job.observation_id) {
                    lease.lost.insert(job.observation_id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    /// F3: a panicking worker is restarted, not lost behind a green readiness probe, and the
    /// supervisor ends with shutdown.
    #[tokio::test]
    async fn memory_startup_indexer_supervisor_restarts_after_a_panic() {
        let shutdown = CancellationToken::new();
        let starts = Arc::new(AtomicU32::new(0));
        let supervisor = tokio::spawn(supervise(
            "test",
            shutdown.clone(),
            Duration::from_millis(10),
            {
                let starts = starts.clone();
                let shutdown = shutdown.clone();
                move || {
                    let starts = starts.clone();
                    let shutdown = shutdown.clone();
                    async move {
                        if starts.fetch_add(1, Ordering::SeqCst) == 0 {
                            panic!("injected worker panic");
                        }
                        shutdown.cancelled().await;
                    }
                }
            },
        ));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(starts.load(Ordering::SeqCst), 2, "restarted once");
        shutdown.cancel();
        let restarts = tokio::time::timeout(Duration::from_secs(1), supervisor)
            .await
            .expect("the supervisor stops with shutdown")
            .unwrap();
        assert_eq!(restarts, 1);
    }
}
