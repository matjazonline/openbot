use std::{
    collections::VecDeque,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::{sync::broadcast, time::MissedTickBehavior};
use tracing::{info, warn};

use crate::{
    app_error::AppResult,
    entities::{
        dashboard::DashboardWindow,
        runtime_metrics::{
            MachineId, MemoryProviderInterval, RuntimeMetricObservation, RuntimeMetricSample,
            RuntimeMetricSnapshot,
        },
    },
};

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(10);
pub const RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub const RETENTION_DAYS: i64 = 7;
/// One hour of failed ten-second probes. An outage cannot grow process memory without bound.
pub const FAILED_PROBE_CAPACITY: usize = 360;

/// Shared by the task executor and sampler. The guard makes cancellation-safe accounting the
/// default: every path out of an execution drops it and decrements the gauge.
#[derive(Clone, Default)]
pub struct ActiveTaskExecutions(Arc<AtomicUsize>);

impl ActiveTaskExecutions {
    pub fn enter(&self) -> ActiveTaskExecutionGuard {
        self.0.fetch_add(1, Ordering::Relaxed);
        ActiveTaskExecutionGuard(self.clone())
    }

    pub fn current(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}

pub struct ActiveTaskExecutionGuard(ActiveTaskExecutions);

impl Drop for ActiveTaskExecutionGuard {
    fn drop(&mut self) {
        let previous = self.0.0.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(previous > 0, "active task execution gauge underflowed");
    }
}

/// Shared by every memory provider adapter and the sampler. Every completed call is recorded once, and the
/// sampler drains the tally, so each sample reports exactly the calls made since the previous one.
///
/// A lock rather than a set of atomics: the three figures are one reading, and a drain that
/// interleaved with a record could otherwise report more failures than calls.
#[derive(Clone, Default)]
pub struct MemoryProviderActivity(Arc<Mutex<MemoryProviderInterval>>);

impl MemoryProviderActivity {
    pub fn record(&self, duration: Duration, succeeded: bool) {
        let mut interval = self.lock();
        interval.calls = interval.calls.saturating_add(1);
        if !succeeded {
            interval.failures = interval.failures.saturating_add(1);
        }
        interval.total_duration_ms += duration.as_secs_f64() * 1000.0;
    }

    /// Take the interval and start the next one. Called once per sample; a sample that fails to
    /// persist carries its calls into the buffered backlog rather than losing or double-counting
    /// them.
    pub fn drain(&self) -> MemoryProviderInterval {
        std::mem::take(&mut *self.lock())
    }

    /// A poisoned tally is recoverable: counters have no invariant a panic could have broken
    /// halfway, and losing memory metrics is never a reason to fail a memory call.
    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryProviderInterval> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub trait RuntimeMetricSource: Send {
    fn observe(&mut self) -> RuntimeMetricObservation;
}

#[derive(Debug, Clone)]
pub struct RuntimeMetricProbeError {
    pub sample: RuntimeMetricSample,
    pub message: String,
}

impl fmt::Display for RuntimeMetricProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

#[async_trait]
pub trait RuntimeMetricPersistence: Send + Sync {
    /// Time pool acquisition, then use that exact connection to flush `failed` and write the new
    /// observation. An acquisition or write error returns the sample that must be buffered.
    async fn probe_and_record(
        &self,
        observation: RuntimeMetricObservation,
        failed: &[RuntimeMetricSample],
    ) -> Result<RuntimeMetricSample, RuntimeMetricProbeError>;

    async fn snapshot(
        &self,
        machine_id: &MachineId,
        window: DashboardWindow,
    ) -> AppResult<RuntimeMetricSnapshot>;

    async fn prune_before(&self, cutoff: DateTime<Utc>) -> AppResult<u64>;
}

pub struct RuntimeMetricSampler<S> {
    persistence: Arc<dyn RuntimeMetricPersistence>,
    source: S,
    failed: VecDeque<RuntimeMetricSample>,
}

impl<S: RuntimeMetricSource> RuntimeMetricSampler<S> {
    pub fn new(persistence: Arc<dyn RuntimeMetricPersistence>, source: S) -> Self {
        Self {
            persistence,
            source,
            failed: VecDeque::with_capacity(FAILED_PROBE_CAPACITY),
        }
    }

    async fn sample_once(&mut self) {
        let observation = self.source.observe();
        let backlog = self.failed.make_contiguous();
        match self
            .persistence
            .probe_and_record(observation, backlog)
            .await
        {
            Ok(_) => self.failed.clear(),
            Err(error) => {
                warn!(error = %error, "Runtime metric probe could not be persisted");
                if self.failed.len() == FAILED_PROBE_CAPACITY {
                    self.failed.pop_front();
                }
                self.failed.push_back(error.sample);
            }
        }
    }

    async fn prune(&self) {
        let cutoff = Utc::now() - chrono::Duration::days(RETENTION_DAYS);
        if let Err(error) = self.persistence.prune_before(cutoff).await {
            warn!(%error, "Runtime metric retention cleanup failed");
        }
    }

    /// Sample immediately, then every ten seconds. Shutdown can cancel a probe waiting for a pool
    /// slot; the loop owns no detached work and returns only after collection has stopped.
    pub async fn run(mut self, mut shutdown: broadcast::Receiver<()>) {
        info!(
            interval_seconds = SAMPLE_INTERVAL.as_secs(),
            retention_days = RETENTION_DAYS,
            "Starting runtime metric sampler"
        );
        tokio::select! {
            biased;
            _ = shutdown.recv() => return,
            _ = self.prune() => {}
        }

        let mut samples = tokio::time::interval(SAMPLE_INTERVAL);
        samples.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut retention = tokio::time::interval(RETENTION_INTERVAL);
        retention.set_missed_tick_behavior(MissedTickBehavior::Skip);
        retention.tick().await;

        loop {
            tokio::select! {
                biased;
                _ = shutdown.recv() => break,
                _ = samples.tick() => {
                    tokio::select! {
                        biased;
                        _ = shutdown.recv() => break,
                        _ = self.sample_once() => {}
                    }
                },
                _ = retention.tick() => {
                    tokio::select! {
                        biased;
                        _ = shutdown.recv() => break,
                        _ = self.prune() => {}
                    }
                },
            }
        }

        info!(
            buffered_samples = self.failed.len(),
            "Runtime metric sampler stopped"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use crate::entities::runtime_metrics::{MachineIdentity, MachineRegion};

    struct Source {
        identity: MachineIdentity,
        sequence: i64,
    }

    impl RuntimeMetricSource for Source {
        fn observe(&mut self) -> RuntimeMetricObservation {
            self.sequence += 1;
            RuntimeMetricObservation {
                identity: self.identity.clone(),
                sampled_at: Utc::now() + chrono::Duration::microseconds(self.sequence),
                process_rss_bytes: Some(self.sequence),
                memory_limit_bytes: None,
                cpu_utilization_percent: None,
                cpu_steal_percent: None,
                cpu_throttle_percent: None,
                active_task_executions: 0,
                task_worker_concurrency_limit: 4,
                hydradb: MemoryProviderInterval::default(),
            }
        }
    }

    #[derive(Default)]
    struct Persistence {
        failures_remaining: AtomicUsize,
        calls: AtomicUsize,
        backlog_lengths: Mutex<Vec<usize>>,
        prunes: AtomicUsize,
    }

    impl Persistence {
        fn failing(count: usize) -> Self {
            Self {
                failures_remaining: AtomicUsize::new(count),
                ..Self::default()
            }
        }
    }

    #[async_trait]
    impl RuntimeMetricPersistence for Persistence {
        async fn probe_and_record(
            &self,
            observation: RuntimeMetricObservation,
            failed: &[RuntimeMetricSample],
        ) -> Result<RuntimeMetricSample, RuntimeMetricProbeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.backlog_lengths.lock().unwrap().push(failed.len());
            let sample = RuntimeMetricSample {
                identity: observation.identity,
                sampled_at: observation.sampled_at,
                process_rss_bytes: observation.process_rss_bytes,
                memory_limit_bytes: observation.memory_limit_bytes,
                cpu_utilization_percent: observation.cpu_utilization_percent,
                cpu_steal_percent: observation.cpu_steal_percent,
                cpu_throttle_percent: observation.cpu_throttle_percent,
                active_task_executions: observation.active_task_executions,
                task_worker_concurrency_limit: observation.task_worker_concurrency_limit,
                hydradb: observation.hydradb,
                database_acquire_duration_ms: 1.0,
                database_acquire_succeeded: false,
                pool_size: 1,
                pool_idle: 1,
                pool_active: 0,
            };
            if self
                .failures_remaining
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    (remaining > 0).then(|| remaining - 1)
                })
                .is_ok()
            {
                Err(RuntimeMetricProbeError {
                    sample,
                    message: "offline".into(),
                })
            } else {
                Ok(RuntimeMetricSample {
                    database_acquire_succeeded: true,
                    ..sample
                })
            }
        }

        async fn snapshot(
            &self,
            _machine_id: &MachineId,
            _window: DashboardWindow,
        ) -> AppResult<RuntimeMetricSnapshot> {
            Ok(RuntimeMetricSnapshot::default())
        }

        async fn prune_before(&self, _cutoff: DateTime<Utc>) -> AppResult<u64> {
            self.prunes.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        }
    }

    fn source() -> Source {
        Source {
            identity: MachineIdentity {
                id: MachineId::new("test-machine"),
                region: Some(MachineRegion::new("test-region")),
            },
            sequence: 0,
        }
    }

    #[test]
    fn memory_provider_activity_tallies_calls_and_starts_a_new_interval_on_drain() {
        let activity = MemoryProviderActivity::default();
        activity.record(Duration::from_millis(20), true);
        activity.record(Duration::from_millis(40), false);

        let interval = activity.drain();
        assert_eq!(interval.calls, 2);
        assert_eq!(interval.failures, 1);
        assert_eq!(interval.mean_duration_ms(), Some(30.0));

        let idle = activity.drain();
        assert_eq!(idle.calls, 0);
        assert_eq!(idle.mean_duration_ms(), None);
    }

    #[test]
    fn active_execution_gauge_is_nested_and_cancellation_safe() {
        let gauge = ActiveTaskExecutions::default();
        assert_eq!(gauge.current(), 0);
        let first = gauge.enter();
        assert_eq!(gauge.current(), 1);
        {
            let _second = gauge.enter();
            assert_eq!(gauge.current(), 2);
        }
        assert_eq!(gauge.current(), 1);
        drop(first);
        assert_eq!(gauge.current(), 0);
    }

    #[tokio::test]
    async fn failed_probe_buffer_is_bounded_and_flushed_on_recovery() {
        let persistence = Arc::new(Persistence::failing(FAILED_PROBE_CAPACITY + 5));
        let trait_persistence: Arc<dyn RuntimeMetricPersistence> = persistence.clone();
        let mut sampler = RuntimeMetricSampler::new(trait_persistence, source());

        for _ in 0..FAILED_PROBE_CAPACITY + 5 {
            sampler.sample_once().await;
        }
        assert_eq!(sampler.failed.len(), FAILED_PROBE_CAPACITY);

        sampler.sample_once().await;
        assert!(sampler.failed.is_empty());
        assert_eq!(
            persistence.backlog_lengths.lock().unwrap().last(),
            Some(&FAILED_PROBE_CAPACITY)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sampler_uses_ten_second_cadence_and_joins_on_shutdown() {
        let persistence = Arc::new(Persistence::default());
        let trait_persistence: Arc<dyn RuntimeMetricPersistence> = persistence.clone();
        let sampler = RuntimeMetricSampler::new(trait_persistence, source());
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let driver = tokio::spawn(sampler.run(shutdown_rx));

        tokio::task::yield_now().await;
        assert_eq!(persistence.calls.load(Ordering::SeqCst), 1);
        assert_eq!(persistence.prunes.load(Ordering::SeqCst), 1);

        for expected in 2..=4 {
            tokio::time::advance(SAMPLE_INTERVAL).await;
            tokio::task::yield_now().await;
            assert_eq!(persistence.calls.load(Ordering::SeqCst), expected);
        }

        let _ = shutdown_tx.send(());
        tokio::task::yield_now().await;
        assert!(driver.await.is_ok(), "the sampler task joins cleanly");
    }

    #[tokio::test(start_paused = true)]
    async fn retention_cleanup_repeats_hourly() {
        let persistence = Arc::new(Persistence::default());
        let trait_persistence: Arc<dyn RuntimeMetricPersistence> = persistence.clone();
        let sampler = RuntimeMetricSampler::new(trait_persistence, source());
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let driver = tokio::spawn(sampler.run(shutdown_rx));
        tokio::task::yield_now().await;
        assert_eq!(persistence.prunes.load(Ordering::SeqCst), 1);

        tokio::time::advance(RETENTION_INTERVAL).await;
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        assert_eq!(persistence.prunes.load(Ordering::SeqCst), 2);

        let _ = shutdown_tx.send(());
        driver.await.unwrap();
    }

    struct BlockingPersistence {
        started: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl RuntimeMetricPersistence for BlockingPersistence {
        async fn probe_and_record(
            &self,
            _observation: RuntimeMetricObservation,
            _failed: &[RuntimeMetricSample],
        ) -> Result<RuntimeMetricSample, RuntimeMetricProbeError> {
            self.started.notify_one();
            std::future::pending().await
        }

        async fn snapshot(
            &self,
            _machine_id: &MachineId,
            _window: DashboardWindow,
        ) -> AppResult<RuntimeMetricSnapshot> {
            Ok(RuntimeMetricSnapshot::default())
        }

        async fn prune_before(&self, _cutoff: DateTime<Utc>) -> AppResult<u64> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn shutdown_cancels_an_active_acquisition_probe() {
        let started = Arc::new(tokio::sync::Notify::new());
        let persistence: Arc<dyn RuntimeMetricPersistence> = Arc::new(BlockingPersistence {
            started: started.clone(),
        });
        let sampler = RuntimeMetricSampler::new(persistence, source());
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let driver = tokio::spawn(sampler.run(shutdown_rx));

        started.notified().await;
        let _ = shutdown_tx.send(());
        tokio::time::timeout(Duration::from_secs(1), driver)
            .await
            .expect("shutdown must interrupt an active probe")
            .unwrap();
    }
}
