use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use tokio::{sync::broadcast, time::sleep};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    domain::monitoring::MonitoringService,
    entities::memory::{LeasedMemoryJob, MemoryProviderError},
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
                if !self
                    .persistence
                    .renew_provisioning_job(job.id, token, lease_expiry())
                    .await
                    .map_err(|error| error.to_string())?
                {
                    return Ok(true);
                }
                match provider.provision(&job.remote_database_id).await {
                    Ok(()) => match provider.is_ready(&job.remote_database_id).await {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(MemoryProviderError::NotReady),
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                }
            }
            None => Err(MemoryProviderError::Authentication),
        };
        match outcome {
            Ok(()) => {
                self.record_job("provision", "completed");
                self.persistence
                    .complete_provisioning(job.id, token)
                    .await
                    .map_err(|error| error.to_string())?;
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
                if !self
                    .persistence
                    .renew_cleanup_job(job.id, token, lease_expiry())
                    .await
                    .map_err(|error| error.to_string())?
                {
                    return Ok(true);
                }
                provider.delete(&job.remote_database_id).await
            }
            None => Err(MemoryProviderError::Authentication),
        };
        match outcome {
            Ok(()) | Err(MemoryProviderError::NotFound) => {
                self.record_job("cleanup", "completed");
                self.persistence
                    .complete_cleanup(job.id, token)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Err(error) => {
                self.record_job(
                    "cleanup",
                    if error.retryable() { "retry" } else { "failed" },
                );
                let terminal = !error.retryable() || job.attempts >= MAX_ATTEMPTS;
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
    use super::*;

    #[test]
    fn retry_backoff_is_bounded_and_not_ready_polls_quickly() {
        let now = Utc::now();
        assert!(
            retry_at(99, &MemoryProviderError::Unavailable) <= now + chrono::Duration::seconds(301)
        );
        assert!(retry_at(1, &MemoryProviderError::NotReady) <= now + chrono::Duration::seconds(3));
    }
}
