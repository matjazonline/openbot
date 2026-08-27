use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::{
    sync::broadcast,
    time::{Instant, sleep, timeout_at},
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    domain::monitoring::MonitoringService,
    entities::memory::{
        LeasedMemoryJob, MAX_MEMORY_PROVIDER_OPERATION_SECONDS, MemoryProviderError,
    },
    services::memory_provider::MemoryProviderRegistry,
    use_cases::memory::MemoryConnectionPersistence,
};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const ERROR_BACKOFF: Duration = Duration::from_secs(5);
const LEASE_SECONDS: i64 = 180;
const MAX_ATTEMPTS: i32 = 8;

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
            let pause = match self.process_one_provisioning().await {
                Ok(true) => Duration::ZERO,
                Ok(false) => POLL_INTERVAL,
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
            let pause = match self.process_one_cleanup().await {
                Ok(true) => Duration::ZERO,
                Ok(false) => POLL_INTERVAL,
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

    async fn process_one_provisioning(&self) -> Result<bool, String> {
        let token = Uuid::new_v4();
        let Some(job) = self
            .persistence
            .claim_provisioning_job(token, lease_expiry())
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(false);
        };
        if !self
            .persistence
            .mark_provisioning(job.id, token)
            .await
            .map_err(|error| error.to_string())?
        {
            return Ok(true);
        }
        let outcome = match self.providers.get(job.provider) {
            Some(provider) => {
                let operation_deadline = provider_operation_deadline();
                if !self
                    .persistence
                    .renew_provisioning_job(job.id, token, lease_expiry())
                    .await
                    .map_err(|error| error.to_string())?
                {
                    return Ok(true);
                }
                timeout_at(operation_deadline, async {
                    match provider.provision(&job.remote_database_id).await {
                        Ok(()) => match provider.is_ready(&job.remote_database_id).await {
                            Ok(true) => Ok(()),
                            Ok(false) => Err(MemoryProviderError::NotReady),
                            Err(error) => Err(error),
                        },
                        Err(error) => Err(error),
                    }
                })
                .await
                .unwrap_or(Err(MemoryProviderError::Timeout))
            }
            None => Err(MemoryProviderError::Authentication),
        };
        match outcome {
            Ok(()) => {
                let completed = self
                    .persistence
                    .complete_provisioning(job.id, token)
                    .await
                    .map_err(|error| error.to_string())?;
                self.record_job(
                    "provision",
                    if completed {
                        "completed"
                    } else {
                        "stale_suppressed"
                    },
                );
            }
            Err(error) => {
                self.record_job(
                    "provision",
                    if error.retryable() { "retry" } else { "failed" },
                );
                self.fail_provisioning(&job, error).await?;
            }
        }
        Ok(true)
    }

    async fn fail_provisioning(
        &self,
        job: &LeasedMemoryJob,
        error: MemoryProviderError,
    ) -> Result<(), String> {
        let terminal = !error.retryable() || job.attempts >= MAX_ATTEMPTS;
        self.persistence
            .retry_provisioning_job(
                job.id,
                job.lease_token,
                retry_at(job.attempts, &error),
                &error.to_string(),
                terminal,
            )
            .await
            .map_err(|failure| failure.to_string())?;
        Ok(())
    }

    async fn process_one_cleanup(&self) -> Result<bool, String> {
        let token = Uuid::new_v4();
        let Some(job) = self
            .persistence
            .claim_cleanup_job(token, lease_expiry())
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(false);
        };
        let outcome = match self.providers.get(job.provider) {
            Some(provider) => {
                let operation_deadline = provider_operation_deadline();
                if !self
                    .persistence
                    .renew_cleanup_job(job.id, token, lease_expiry())
                    .await
                    .map_err(|error| error.to_string())?
                {
                    return Ok(true);
                }
                timeout_at(operation_deadline, provider.delete(&job.remote_database_id))
                    .await
                    .unwrap_or(Err(MemoryProviderError::Timeout))
            }
            None => Err(MemoryProviderError::Authentication),
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
                        "confirmed"
                    } else {
                        "stale_suppressed"
                    },
                );
            }
            Err(error) => {
                self.record_job(
                    "cleanup",
                    if error.retryable() { "retry" } else { "failed" },
                );
                let terminal = !error.retryable() || job.attempts >= MAX_ATTEMPTS;
                if terminal {
                    self.record_job("cleanup", "exhausted");
                }
                self.persistence
                    .retry_cleanup_job(
                        job.id,
                        token,
                        retry_at(job.attempts, &error),
                        &error.to_string(),
                        terminal,
                    )
                    .await
                    .map_err(|failure| failure.to_string())?;
            }
        }
        Ok(true)
    }

    fn record_job(&self, operation: &str, outcome: &str) {
        self.monitoring.increment_counter(
            "memory_worker_jobs_total",
            1,
            &[("operation", operation), ("outcome", outcome)],
        );
    }
}

fn lease_expiry() -> DateTime<Utc> {
    Utc::now() + chrono::Duration::seconds(LEASE_SECONDS)
}

fn provider_operation_deadline() -> Instant {
    Instant::now() + Duration::from_secs(MAX_MEMORY_PROVIDER_OPERATION_SECONDS)
}

fn retry_at(attempts: i32, error: &MemoryProviderError) -> DateTime<Utc> {
    let seconds = if *error == MemoryProviderError::NotReady {
        2
    } else {
        (5_i64 * 2_i64.pow(attempts.clamp(0, 6) as u32)).min(300)
    };
    Utc::now() + chrono::Duration::seconds(seconds)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

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
        services::memory_provider::{MemoryConversation, MemoryProvider},
        use_cases::{
            company::{CompanyPersistence, CompanyWrite},
            memory::MemoryConnectionPersistence,
            user::UserPersistence,
        },
    };

    struct PausingProvider {
        provision_started: Semaphore,
        release_provision: Semaphore,
        present: AtomicBool,
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
            _query: &str,
            _scopes: &[ResolvedMemoryScope],
            _mode: MemoryRecallMode,
            _max_results: u8,
            _additional_context: Option<&str>,
        ) -> Result<Vec<MemoryChunk>, MemoryProviderError> {
            Ok(Vec::new())
        }

        async fn persist(
            &self,
            _database_id: &str,
            _collections: &[String],
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

    #[test]
    fn retry_backoff_is_bounded_and_not_ready_polls_quickly() {
        let now = Utc::now();
        assert!(
            retry_at(99, &MemoryProviderError::Unavailable) <= now + chrono::Duration::seconds(301)
        );
        assert!(retry_at(1, &MemoryProviderError::NotReady) <= now + chrono::Duration::seconds(3));
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
