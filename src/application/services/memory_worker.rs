use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::{
    sync::broadcast,
    time::{Instant, sleep},
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    domain::monitoring::MonitoringService,
    entities::memory::{
        LeasedProvisioningJob, MAX_MEMORY_PROVIDER_OPERATION_SECONDS,
        MEMORY_DELETION_QUIESCENCE_SECONDS, MEMORY_READINESS_TIMEOUT_ERROR,
        MEMORY_READINESS_WINDOW_SECONDS, MemoryProviderError, MemoryProvisioningPhase,
    },
    services::{
        memory_job_lease::{MemoryJobLease, MemoryLeaseOutcome, supervise_memory_job_lease},
        memory_job_schedule::{next_poll_at, retry_at},
        memory_provider::MemoryProviderRegistry,
    },
    use_cases::memory::MemoryConnectionPersistence,
};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const ERROR_BACKOFF: Duration = Duration::from_secs(5);
const LEASE_SECONDS: i64 = MEMORY_DELETION_QUIESCENCE_SECONDS;
const LEASE_DURATION: Duration = Duration::from_secs(LEASE_SECONDS as u64);
const _: () = assert!(MAX_MEMORY_PROVIDER_OPERATION_SECONDS < LEASE_SECONDS as u64);
const MAX_PROVIDER_FAILURE_ATTEMPTS: i32 = 8;
const FAILURE_BUDGET_ERROR: &str = "memory provider failure budget was exhausted";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkCycle {
    Processed,
    Idle,
    Shutdown,
}

enum SupervisedProvider<T> {
    Outcome(T),
    Interrupted(WorkCycle),
}

pub struct MemoryWorker {
    persistence: Arc<dyn MemoryConnectionPersistence>,
    providers: Arc<MemoryProviderRegistry>,
    monitoring: Arc<dyn MonitoringService>,
}

impl MemoryWorker {
    pub fn new(
        persistence: Arc<dyn MemoryConnectionPersistence>,
        providers: Arc<MemoryProviderRegistry>,
        monitoring: Arc<dyn MonitoringService>,
    ) -> Self {
        Self {
            persistence,
            providers,
            monitoring,
        }
    }

    pub async fn run(self: Arc<Self>, shutdown: broadcast::Receiver<()>) {
        info!("Starting memory provisioning and cleanup workers");
        let provision = Arc::clone(&self).run_provisioning(shutdown.resubscribe());
        let cleanup = self.run_cleanup(shutdown);
        tokio::join!(provision, cleanup);
    }

    async fn run_provisioning(self: Arc<Self>, mut shutdown: broadcast::Receiver<()>) {
        loop {
            let pause = match self
                .process_one_provisioning_until(async {
                    let _ = shutdown.recv().await;
                })
                .await
            {
                Ok(WorkCycle::Processed) => Duration::ZERO,
                Ok(WorkCycle::Idle) => POLL_INTERVAL,
                Ok(WorkCycle::Shutdown) => return,
                Err(error) => {
                    warn!(error = %error, "Memory provisioning poll failed");
                    ERROR_BACKOFF
                }
            };
            tokio::select! {
                _ = shutdown.recv() => return,
                _ = sleep(pause) => {}
            }
        }
    }

    async fn run_cleanup(self: Arc<Self>, mut shutdown: broadcast::Receiver<()>) {
        loop {
            let pause = match self
                .process_one_cleanup_until(async {
                    let _ = shutdown.recv().await;
                })
                .await
            {
                Ok(WorkCycle::Processed) => Duration::ZERO,
                Ok(WorkCycle::Idle) => POLL_INTERVAL,
                Ok(WorkCycle::Shutdown) => return,
                Err(error) => {
                    warn!(error = %error, "Memory cleanup poll failed");
                    ERROR_BACKOFF
                }
            };
            tokio::select! {
                _ = shutdown.recv() => return,
                _ = sleep(pause) => {}
            }
        }
    }

    #[cfg(test)]
    async fn process_one_provisioning(&self) -> Result<bool, String> {
        Ok(matches!(
            self.process_one_provisioning_until(std::future::pending())
                .await?,
            WorkCycle::Processed
        ))
    }

    async fn process_one_provisioning_until<Shutdown>(
        &self,
        shutdown: Shutdown,
    ) -> Result<WorkCycle, String>
    where
        Shutdown: std::future::Future<Output = ()>,
    {
        let token = Uuid::new_v4();
        let initial_expiry = lease_expiry();
        let Some(job) = self
            .persistence
            .claim_provisioning_job(token, initial_expiry)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(WorkCycle::Idle);
        };
        if !self
            .persistence
            .mark_provisioning(job.id, token)
            .await
            .map_err(|error| error.to_string())?
        {
            self.record_job("provision", "stale_transition");
            return Ok(WorkCycle::Processed);
        }
        if job.failure_attempts >= MAX_PROVIDER_FAILURE_ATTEMPTS {
            self.fail_provisioning_terminal(&job, FAILURE_BUDGET_ERROR, "terminal")
                .await?;
            return Ok(WorkCycle::Processed);
        }
        let cycle = match job.phase {
            MemoryProvisioningPhase::CreatePending => {
                self.create_database(&job, initial_expiry, shutdown).await?
            }
            MemoryProvisioningPhase::WaitingReady => {
                self.poll_readiness(&job, initial_expiry, shutdown).await?
            }
            MemoryProvisioningPhase::Ready | MemoryProvisioningPhase::Failed => {
                return Err("A terminal memory provisioning job was claimed.".into());
            }
        };
        Ok(cycle)
    }

    async fn create_database<Shutdown>(
        &self,
        job: &LeasedProvisioningJob,
        initial_expiry: DateTime<Utc>,
        shutdown: Shutdown,
    ) -> Result<WorkCycle, String>
    where
        Shutdown: std::future::Future<Output = ()>,
    {
        let outcome = match self.providers.get(job.provider) {
            Some(provider) => {
                let mut lease = memory_job_lease(job, initial_expiry);
                supervise_memory_job_lease(
                    &mut lease,
                    LEASE_DURATION,
                    provider_operation_deadline(),
                    |expires_at| async move {
                        self.persistence
                            .renew_provisioning_job(job.id, job.lease_token, expires_at)
                            .await
                            .map_err(|error| error.to_string())
                    },
                    provider.provision(&job.remote_database_id),
                    shutdown,
                )
                .await
            }
            None => MemoryLeaseOutcome::Completed(Err(MemoryProviderError::Authentication)),
        };
        let outcome = match self.provider_outcome("provision", outcome)? {
            SupervisedProvider::Outcome(outcome) => outcome,
            SupervisedProvider::Interrupted(cycle) => return Ok(cycle),
        };
        match outcome {
            Ok(()) => {
                let accepted = self
                    .persistence
                    .begin_readiness_polling(
                        job.id,
                        job.lease_token,
                        Utc::now() + chrono::Duration::seconds(MEMORY_READINESS_WINDOW_SECONDS),
                        next_poll_at(),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                self.record_job(
                    "provision",
                    if accepted {
                        "completed"
                    } else {
                        "stale_transition"
                    },
                );
            }
            Err(error) => {
                let outcome = provisioning_failure_outcome(job, &error);
                let accepted = self.fail_provisioning(job, error).await?;
                self.record_job(
                    "provision",
                    if accepted {
                        outcome
                    } else {
                        "stale_transition"
                    },
                );
            }
        }
        Ok(WorkCycle::Processed)
    }

    async fn poll_readiness<Shutdown>(
        &self,
        job: &LeasedProvisioningJob,
        initial_expiry: DateTime<Utc>,
        shutdown: Shutdown,
    ) -> Result<WorkCycle, String>
    where
        Shutdown: std::future::Future<Output = ()>,
    {
        let deadline = job
            .readiness_deadline
            .ok_or_else(|| "A readiness polling job has no deadline.".to_string())?;
        if Utc::now() >= deadline {
            self.timeout_provisioning(job).await?;
            return Ok(WorkCycle::Processed);
        }
        let outcome = match self.providers.get(job.provider) {
            Some(provider) => {
                let mut lease = memory_job_lease(job, initial_expiry);
                supervise_memory_job_lease(
                    &mut lease,
                    LEASE_DURATION,
                    provider_operation_deadline(),
                    |expires_at| async move {
                        self.persistence
                            .renew_provisioning_job(job.id, job.lease_token, expires_at)
                            .await
                            .map_err(|error| error.to_string())
                    },
                    provider.is_ready(&job.remote_database_id),
                    shutdown,
                )
                .await
            }
            None => MemoryLeaseOutcome::Completed(Err(MemoryProviderError::Authentication)),
        };
        let outcome = match self.provider_outcome("provision", outcome)? {
            SupervisedProvider::Outcome(outcome) => outcome,
            SupervisedProvider::Interrupted(cycle) => return Ok(cycle),
        };
        match outcome {
            Ok(true) => {
                let completed = self
                    .persistence
                    .complete_provisioning(job.id, job.lease_token)
                    .await
                    .map_err(|error| error.to_string())?;
                self.record_job(
                    "provision",
                    if completed {
                        "completed"
                    } else {
                        "stale_transition"
                    },
                );
            }
            Ok(false) if Utc::now() >= deadline => self.timeout_provisioning(job).await?,
            Ok(false) => {
                let scheduled = self
                    .persistence
                    .schedule_readiness_poll(job.id, job.lease_token, next_poll_at().min(deadline))
                    .await
                    .map_err(|error| error.to_string())?;
                self.record_job(
                    "provision",
                    if scheduled {
                        "waiting_ready"
                    } else {
                        "stale_transition"
                    },
                );
            }
            Err(error) => {
                let outcome = provisioning_failure_outcome(job, &error);
                let accepted = self.fail_provisioning(job, error).await?;
                self.record_job(
                    "provision",
                    if accepted {
                        outcome
                    } else {
                        "stale_transition"
                    },
                );
            }
        }
        Ok(WorkCycle::Processed)
    }

    async fn timeout_provisioning(&self, job: &LeasedProvisioningJob) -> Result<(), String> {
        self.record_job("provision", "timeout");
        self.fail_provisioning_terminal(job, MEMORY_READINESS_TIMEOUT_ERROR, "terminal")
            .await
    }

    async fn fail_provisioning_terminal(
        &self,
        job: &LeasedProvisioningJob,
        safe_error: &str,
        outcome: &str,
    ) -> Result<(), String> {
        let failed = self
            .persistence
            .fail_provisioning_job(job.id, job.lease_token, safe_error)
            .await
            .map_err(|error| error.to_string())?;
        self.record_job(
            "provision",
            if failed { outcome } else { "stale_transition" },
        );
        Ok(())
    }

    async fn fail_provisioning(
        &self,
        job: &LeasedProvisioningJob,
        error: MemoryProviderError,
    ) -> Result<bool, String> {
        let terminal =
            !error.retryable() || job.failure_attempts + 1 >= MAX_PROVIDER_FAILURE_ATTEMPTS;
        let mut retry_at = retry_at(job.failure_attempts + 1);
        if job.phase == MemoryProvisioningPhase::WaitingReady
            && let Some(deadline) = job.readiness_deadline
        {
            retry_at = retry_at.min(deadline);
        }
        let accepted = self
            .persistence
            .retry_provisioning_job(
                job.id,
                job.lease_token,
                retry_at,
                &error.to_string(),
                terminal,
            )
            .await
            .map_err(|failure| failure.to_string())?;
        Ok(accepted)
    }

    #[cfg(test)]
    async fn process_one_cleanup(&self) -> Result<bool, String> {
        Ok(matches!(
            self.process_one_cleanup_until(std::future::pending())
                .await?,
            WorkCycle::Processed
        ))
    }

    async fn process_one_cleanup_until<Shutdown>(
        &self,
        shutdown: Shutdown,
    ) -> Result<WorkCycle, String>
    where
        Shutdown: std::future::Future<Output = ()>,
    {
        let token = Uuid::new_v4();
        let initial_expiry = lease_expiry();
        let Some(job) = self
            .persistence
            .claim_cleanup_job(token, initial_expiry)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(WorkCycle::Idle);
        };
        let outcome = match self.providers.get(job.provider) {
            Some(provider) => {
                let mut lease = MemoryJobLease {
                    job_id: job.id,
                    operation_generation: job.operation_generation,
                    token: job.lease_token,
                    expires_at: initial_expiry,
                };
                supervise_memory_job_lease(
                    &mut lease,
                    LEASE_DURATION,
                    provider_operation_deadline(),
                    |expires_at| async move {
                        self.persistence
                            .renew_cleanup_job(job.id, job.lease_token, expires_at)
                            .await
                            .map_err(|error| error.to_string())
                    },
                    provider.delete(&job.remote_database_id),
                    shutdown,
                )
                .await
            }
            None => MemoryLeaseOutcome::Completed(Err(MemoryProviderError::Authentication)),
        };
        let outcome = match self.provider_outcome("cleanup", outcome)? {
            SupervisedProvider::Outcome(outcome) => outcome,
            SupervisedProvider::Interrupted(cycle) => return Ok(cycle),
        };
        match outcome {
            Ok(()) | Err(MemoryProviderError::NotFound) => {
                let completed = self
                    .persistence
                    .complete_cleanup(job.id, token)
                    .await
                    .map_err(|error| error.to_string())?;
                self.record_job(
                    "cleanup",
                    if completed {
                        "completed"
                    } else {
                        "stale_transition"
                    },
                );
            }
            Err(error) => {
                let terminal =
                    !error.retryable() || job.failure_attempts >= MAX_PROVIDER_FAILURE_ATTEMPTS;
                let accepted = self
                    .persistence
                    .retry_cleanup_job(
                        job.id,
                        token,
                        retry_at(job.failure_attempts),
                        &error.to_string(),
                        terminal,
                    )
                    .await
                    .map_err(|failure| failure.to_string())?;
                self.record_job(
                    "cleanup",
                    if !accepted {
                        "stale_transition"
                    } else if terminal {
                        "terminal"
                    } else {
                        "retry"
                    },
                );
            }
        }
        Ok(WorkCycle::Processed)
    }

    fn provider_outcome<T>(
        &self,
        operation: &str,
        outcome: MemoryLeaseOutcome<Result<T, MemoryProviderError>>,
    ) -> Result<SupervisedProvider<Result<T, MemoryProviderError>>, String> {
        match outcome {
            MemoryLeaseOutcome::Completed(result) => {
                if let Err(error) = &result
                    && let Some(boundary) = error.bound_label()
                {
                    self.monitoring.increment_counter(
                        "memory_bound_rejections_total",
                        1,
                        &[("operation", operation), ("boundary", boundary)],
                    );
                }
                Ok(SupervisedProvider::Outcome(result))
            }
            MemoryLeaseOutcome::DeadlineExceeded => {
                self.record_job(operation, "timeout");
                Ok(SupervisedProvider::Outcome(Err(
                    MemoryProviderError::Timeout,
                )))
            }
            MemoryLeaseOutcome::Shutdown => {
                self.record_job(operation, "shutdown_interruption");
                Ok(SupervisedProvider::Interrupted(WorkCycle::Shutdown))
            }
            MemoryLeaseOutcome::Lost => {
                self.record_job(operation, "lease_loss");
                Ok(SupervisedProvider::Interrupted(WorkCycle::Processed))
            }
            MemoryLeaseOutcome::RenewalFailed(error) => {
                self.record_job(operation, "lease_loss");
                Err(format!("Memory job lease renewal failed: {error}"))
            }
        }
    }

    fn record_job(&self, operation: &str, outcome: &str) {
        self.monitoring.increment_counter(
            "memory_worker_jobs_total",
            1,
            &[("operation", operation), ("outcome", outcome)],
        );
    }
}

fn memory_job_lease(job: &LeasedProvisioningJob, initial_expiry: DateTime<Utc>) -> MemoryJobLease {
    MemoryJobLease {
        job_id: job.id,
        operation_generation: job.operation_generation,
        token: job.lease_token,
        expires_at: initial_expiry,
    }
}

fn lease_expiry() -> DateTime<Utc> {
    Utc::now() + chrono::Duration::seconds(LEASE_SECONDS)
}

fn provider_operation_deadline() -> Instant {
    Instant::now() + Duration::from_secs(MAX_MEMORY_PROVIDER_OPERATION_SECONDS)
}

fn provisioning_failure_outcome(
    job: &LeasedProvisioningJob,
    error: &MemoryProviderError,
) -> &'static str {
    if !error.retryable() || job.failure_attempts + 1 >= MAX_PROVIDER_FAILURE_ATTEMPTS {
        "terminal"
    } else {
        "retry"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use async_trait::async_trait;
    use tokio::sync::Semaphore;

    use super::*;
    use crate::{
        adapters::{
            monitoring::InMemoryMonitor,
            persistence::{
                PostgresPersistence,
                test_support::{UNSCOPED_CLAIM, test_pool},
            },
        },
        entities::memory::{
            MemoryChunk, MemoryProviderKind, MemoryRecallMode, ResolvedMemoryScope,
        },
        services::memory_provider::{MemoryConversation, MemoryPersistenceTarget, MemoryProvider},
        use_cases::{
            company::{CompanyPersistence, CompanyWrite},
            memory::{MemoryBindingPersistence, MemoryConnectionPersistence},
            user::UserPersistence,
        },
    };

    struct PausingProvider {
        provision_started: Semaphore,
        release_provision: Semaphore,
        present: AtomicBool,
    }

    struct CountingReadinessProvider {
        provision_calls: AtomicUsize,
        readiness_calls: AtomicUsize,
        status_failures: usize,
        non_ready_polls: usize,
    }

    impl CountingReadinessProvider {
        fn new(status_failures: usize, non_ready_polls: usize) -> Self {
            Self {
                provision_calls: AtomicUsize::new(0),
                readiness_calls: AtomicUsize::new(0),
                status_failures,
                non_ready_polls,
            }
        }
    }

    #[async_trait]
    impl MemoryProvider for CountingReadinessProvider {
        async fn provision(&self, _database_id: &str) -> Result<(), MemoryProviderError> {
            self.provision_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn is_ready(&self, _database_id: &str) -> Result<bool, MemoryProviderError> {
            let call = self.readiness_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call <= self.status_failures {
                Err(MemoryProviderError::Unavailable)
            } else {
                Ok(call > self.status_failures + self.non_ready_polls)
            }
        }

        async fn recall(
            &self,
            _database_id: &str,
            _query: &crate::services::memory_provider::MemoryRecallQuery,
            _scopes: &[ResolvedMemoryScope],
            _mode: MemoryRecallMode,
            _max_results: u8,
            _additional_context: Option<&crate::services::memory_provider::MemoryAdditionalContext>,
        ) -> Result<Vec<MemoryChunk>, MemoryProviderError> {
            Ok(Vec::new())
        }

        async fn persist(
            &self,
            _database_id: &str,
            _targets: &[MemoryPersistenceTarget],
            _conversation: &MemoryConversation,
        ) -> Vec<Result<(), MemoryProviderError>> {
            Vec::new()
        }

        async fn delete(&self, _database_id: &str) -> Result<(), MemoryProviderError> {
            Ok(())
        }
    }

    impl PausingProvider {
        fn new() -> Self {
            Self {
                provision_started: Semaphore::new(0),
                release_provision: Semaphore::new(0),
                present: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl MemoryProvider for PausingProvider {
        async fn provision(&self, _database_id: &str) -> Result<(), MemoryProviderError> {
            self.provision_started.add_permits(1);
            self.release_provision
                .acquire()
                .await
                .expect("provision release semaphore")
                .forget();
            self.present.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn is_ready(&self, _database_id: &str) -> Result<bool, MemoryProviderError> {
            Ok(self.present.load(Ordering::SeqCst))
        }

        async fn recall(
            &self,
            _database_id: &str,
            _query: &crate::services::memory_provider::MemoryRecallQuery,
            _scopes: &[ResolvedMemoryScope],
            _mode: MemoryRecallMode,
            _max_results: u8,
            _additional_context: Option<&crate::services::memory_provider::MemoryAdditionalContext>,
        ) -> Result<Vec<MemoryChunk>, MemoryProviderError> {
            Ok(Vec::new())
        }

        async fn persist(
            &self,
            _database_id: &str,
            _targets: &[MemoryPersistenceTarget],
            _conversation: &MemoryConversation,
        ) -> Vec<Result<(), MemoryProviderError>> {
            Vec::new()
        }

        async fn delete(&self, _database_id: &str) -> Result<(), MemoryProviderError> {
            if self.present.swap(false, Ordering::SeqCst) {
                Ok(())
            } else {
                Err(MemoryProviderError::NotFound)
            }
        }
    }

    #[tokio::test]
    async fn healthy_slow_readiness_does_not_retry_create_or_spend_failure_budget() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
        let suffix = Uuid::new_v4().simple().to_string();
        let user = persistence
            .create_user(
                &format!("memory-slow-{suffix}"),
                &format!("memory-slow-{suffix}@example.com"),
                "hash",
            )
            .await
            .expect("slow readiness user");
        let company = persistence
            .create(
                user.id,
                CompanyWrite {
                    name: "Slow memory readiness".into(),
                    slug: format!("memory-slow-{suffix}"),
                    ..CompanyWrite::default()
                },
            )
            .await
            .expect("slow readiness company");
        let connection = persistence
            .select_provider(company.id, MemoryProviderKind::Hydradb)
            .await
            .expect("select provider");
        let provider = Arc::new(CountingReadinessProvider::new(0, 10));
        let providers = Arc::new(
            MemoryProviderRegistry::default()
                .register(MemoryProviderKind::Hydradb, provider.clone()),
        );
        let monitor = Arc::new(InMemoryMonitor::new());

        MemoryWorker::new(persistence.clone(), providers.clone(), monitor.clone())
            .process_one_provisioning()
            .await
            .expect("create phase");
        let original_deadline: DateTime<Utc> = sqlx::query_scalar(
            "SELECT readiness_deadline FROM memory_provisioning_jobs WHERE company_id = $1",
        )
        .bind(company.id)
        .fetch_one(&pool)
        .await
        .expect("readiness deadline");

        for _ in 0..=10 {
            sqlx::query(
                "UPDATE memory_provisioning_jobs SET next_poll_at = CURRENT_TIMESTAMP \
                 WHERE company_id = $1 AND status = 'pending'",
            )
            .bind(company.id)
            .execute(&pool)
            .await
            .expect("make readiness poll due");
            // Constructing a fresh worker exercises restart behavior: the deadline comes from the row.
            MemoryWorker::new(persistence.clone(), providers.clone(), monitor.clone())
                .process_one_provisioning()
                .await
                .expect("readiness poll");
        }

        let job: (String, String, i32, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT status, phase, failure_attempts, readiness_deadline \
             FROM memory_provisioning_jobs WHERE company_id = $1",
        )
        .bind(company.id)
        .fetch_one(&pool)
        .await
        .expect("completed provisioning job");
        assert_eq!(job.0, "completed");
        assert_eq!(job.1, "ready");
        assert_eq!(job.2, 0);
        assert_eq!(job.3, Some(original_deadline));
        assert_eq!(provider.provision_calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.readiness_calls.load(Ordering::SeqCst), 11);
        assert_eq!(
            persistence
                .active_binding(company.id)
                .await
                .expect("load binding")
                .connection()
                .expect("connection")
                .remote_database_id,
            connection.remote_database_id
        );
    }

    #[tokio::test]
    async fn transient_status_failure_spends_failure_budget_and_preserves_create_phase() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
        let suffix = Uuid::new_v4().simple().to_string();
        let user = persistence
            .create_user(
                &format!("memory-transient-{suffix}"),
                &format!("memory-transient-{suffix}@example.com"),
                "hash",
            )
            .await
            .expect("transient status user");
        let company = persistence
            .create(
                user.id,
                CompanyWrite {
                    name: "Transient memory status".into(),
                    slug: format!("memory-transient-{suffix}"),
                    ..CompanyWrite::default()
                },
            )
            .await
            .expect("transient status company");
        persistence
            .select_provider(company.id, MemoryProviderKind::Hydradb)
            .await
            .expect("select provider");
        let provider = Arc::new(CountingReadinessProvider::new(1, 0));
        let providers = Arc::new(
            MemoryProviderRegistry::default()
                .register(MemoryProviderKind::Hydradb, provider.clone()),
        );
        let worker = MemoryWorker::new(
            persistence.clone(),
            providers,
            Arc::new(InMemoryMonitor::new()),
        );
        worker
            .process_one_provisioning()
            .await
            .expect("create phase");
        sqlx::query(
            "UPDATE memory_provisioning_jobs SET next_poll_at = CURRENT_TIMESTAMP \
             WHERE company_id = $1",
        )
        .bind(company.id)
        .execute(&pool)
        .await
        .expect("make failing poll due");
        let before_failure = Utc::now();
        worker
            .process_one_provisioning()
            .await
            .expect("transient failing poll");

        let retry: (String, i32, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            "SELECT phase, failure_attempts, next_poll_at, readiness_deadline \
             FROM memory_provisioning_jobs WHERE company_id = $1",
        )
        .bind(company.id)
        .fetch_one(&pool)
        .await
        .expect("retry state");
        assert_eq!(retry.0, "waiting_ready");
        assert_eq!(retry.1, 1);
        assert!(retry.2 >= before_failure + chrono::Duration::seconds(7));
        assert!(retry.2 <= before_failure + chrono::Duration::seconds(13));
        assert!(retry.3 > retry.2);
        assert_eq!(provider.provision_calls.load(Ordering::SeqCst), 1);

        sqlx::query(
            "UPDATE memory_provisioning_jobs SET next_poll_at = CURRENT_TIMESTAMP \
             WHERE company_id = $1",
        )
        .bind(company.id)
        .execute(&pool)
        .await
        .expect("make successful poll due");
        worker
            .process_one_provisioning()
            .await
            .expect("successful readiness poll");
        assert_eq!(provider.provision_calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.readiness_calls.load(Ordering::SeqCst), 2);
        let completed: (String, i32) = sqlx::query_as(
            "SELECT status, failure_attempts FROM memory_provisioning_jobs WHERE company_id = $1",
        )
        .bind(company.id)
        .fetch_one(&pool)
        .await
        .expect("completed retry state");
        assert_eq!(completed, ("completed".into(), 1));
    }

    #[tokio::test]
    async fn deletion_during_provisioning_is_reconciled_to_confirmed_absence() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
        let suffix = Uuid::new_v4().simple().to_string();
        let user = persistence
            .create_user(
                &format!("memory-worker-{suffix}"),
                &format!("memory-worker-{suffix}@example.com"),
                "hash",
            )
            .await
            .expect("worker test user");
        let company = persistence
            .create(
                user.id,
                CompanyWrite {
                    name: "Memory worker race".into(),
                    slug: format!("memory-worker-{suffix}"),
                    ..CompanyWrite::default()
                },
            )
            .await
            .expect("worker test company");
        let connection = persistence
            .select_provider(company.id, MemoryProviderKind::Hydradb)
            .await
            .expect("provider selection");

        let provider = Arc::new(PausingProvider::new());
        let providers = Arc::new(
            MemoryProviderRegistry::default()
                .register(MemoryProviderKind::Hydradb, provider.clone()),
        );
        let worker = Arc::new(MemoryWorker::new(
            persistence.clone(),
            providers,
            Arc::new(InMemoryMonitor::new()),
        ));
        let provisioning = tokio::spawn({
            let worker = worker.clone();
            async move { worker.process_one_provisioning().await }
        });
        provider
            .provision_started
            .acquire()
            .await
            .expect("provision started")
            .forget();

        persistence
            .delete(company.id)
            .await
            .expect("delete while provider call is in flight");
        assert!(
            !worker
                .process_one_cleanup()
                .await
                .expect("cleanup poll during quiescence"),
            "an early provider 404 cannot complete cleanup"
        );

        provider.release_provision.add_permits(1);
        assert!(
            provisioning
                .await
                .expect("provision task joins")
                .expect("provision worker result")
        );
        assert!(provider.present.load(Ordering::SeqCst));

        sqlx::query(
            r#"UPDATE memory_remote_resource_lifecycles
               SET quiesce_until = CURRENT_TIMESTAMP,
                   operation_lease_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(&connection.remote_database_id)
        .execute(&pool)
        .await
        .expect("advance beyond quiescence");
        sqlx::query(
            r#"UPDATE memory_cleanup_jobs SET available_at = CURRENT_TIMESTAMP
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(&connection.remote_database_id)
        .execute(&pool)
        .await
        .expect("make cleanup due");

        assert!(
            worker
                .process_one_cleanup()
                .await
                .expect("post-quiescence cleanup")
        );
        assert!(!provider.present.load(Ordering::SeqCst));
        let cleanup_status: String = sqlx::query_scalar(
            r#"SELECT status FROM memory_cleanup_jobs
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(connection.remote_database_id)
        .fetch_one(&pool)
        .await
        .expect("cleanup status");
        assert_eq!(cleanup_status, "completed");
    }
}
