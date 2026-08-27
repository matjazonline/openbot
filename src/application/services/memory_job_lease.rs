use std::{future::Future, time::Duration};

use chrono::{DateTime, Utc};
use tokio::time::{Instant, sleep_until};
use uuid::Uuid;

/// The immutable identity of one claim, plus the expiry most recently accepted by persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryJobLease {
    pub job_id: Uuid,
    pub operation_generation: i64,
    pub token: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MemoryLeaseOutcome<T> {
    Completed(T),
    DeadlineExceeded,
    Shutdown,
    Lost,
    RenewalFailed(String),
}

/// Own a provider future until it completes or must be cancelled.
///
/// The provider future is deliberately not spawned. Returning on lease loss, shutdown, or the
/// deadline drops it in this scope, so no detached external work can survive the claim that owned
/// it. Renewal itself is supervised by the same deadline and shutdown signal.
pub async fn supervise_memory_job_lease<T, Provider, Renew, Renewal, Shutdown>(
    lease: &mut MemoryJobLease,
    lease_duration: Duration,
    operation_deadline: Instant,
    mut renew: Renew,
    provider: Provider,
    shutdown: Shutdown,
) -> MemoryLeaseOutcome<T>
where
    Provider: Future<Output = T>,
    Renew: FnMut(DateTime<Utc>) -> Renewal,
    Renewal: Future<Output = Result<bool, String>>,
    Shutdown: Future<Output = ()>,
{
    debug_assert!(lease_duration >= Duration::from_secs(3));
    // Renew comfortably before one third of the term, leaving room for persistence latency.
    let heartbeat = (lease_duration / 3) * 4 / 5;
    let mut next_heartbeat = Instant::now() + heartbeat;
    tokio::pin!(provider);
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => return MemoryLeaseOutcome::Shutdown,
            _ = tokio::time::sleep_until(operation_deadline) => {
                return MemoryLeaseOutcome::DeadlineExceeded;
            }
            result = &mut provider => return MemoryLeaseOutcome::Completed(result),
            _ = sleep_until(next_heartbeat) => {}
        }

        let renewed_until = Utc::now()
            + chrono::Duration::from_std(lease_duration)
                .expect("memory lease duration fits chrono");
        let renewal = renew(renewed_until);
        tokio::pin!(renewal);
        let renewed = tokio::select! {
            biased;
            _ = &mut shutdown => return MemoryLeaseOutcome::Shutdown,
            _ = tokio::time::sleep_until(operation_deadline) => {
                return MemoryLeaseOutcome::DeadlineExceeded;
            }
            result = &mut provider => return MemoryLeaseOutcome::Completed(result),
            result = &mut renewal => result,
        };
        match renewed {
            Ok(true) => lease.expires_at = renewed_until,
            Ok(false) => return MemoryLeaseOutcome::Lost,
            Err(error) => return MemoryLeaseOutcome::RenewalFailed(error),
        }
        next_heartbeat = Instant::now() + heartbeat;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use super::*;
    use tokio::sync::Mutex;

    struct CancellationProof(Arc<AtomicBool>);

    impl Drop for CancellationProof {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn lease() -> MemoryJobLease {
        MemoryJobLease {
            job_id: Uuid::new_v4(),
            operation_generation: 7,
            token: Uuid::new_v4(),
            expires_at: Utc::now() + chrono::Duration::seconds(30),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeats_hold_a_slow_operation_for_its_entire_lifetime() {
        let renewals = Arc::new(AtomicUsize::new(0));
        let observed = renewals.clone();
        let outcome = supervise_memory_job_lease(
            &mut lease(),
            Duration::from_secs(30),
            Instant::now() + Duration::from_secs(100),
            move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                async { Ok(true) }
            },
            async {
                tokio::time::sleep(Duration::from_secs(35)).await;
                "done"
            },
            std::future::pending(),
        )
        .await;

        assert_eq!(outcome, MemoryLeaseOutcome::Completed("done"));
        assert!(renewals.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test(start_paused = true)]
    async fn competing_claimants_cannot_take_a_heartbeating_lease() {
        let owned_until = Arc::new(Mutex::new(Instant::now() + Duration::from_secs(30)));
        let renewal_state = owned_until.clone();
        let competitor_state = owned_until.clone();
        let mut active_lease = lease();
        let supervisor = supervise_memory_job_lease(
            &mut active_lease,
            Duration::from_secs(30),
            Instant::now() + Duration::from_secs(100),
            move |_| {
                let renewal_state = renewal_state.clone();
                async move {
                    *renewal_state.lock().await = Instant::now() + Duration::from_secs(30);
                    Ok(true)
                }
            },
            async {
                tokio::time::sleep(Duration::from_secs(65)).await;
            },
            std::future::pending(),
        );
        let competitor = async move {
            tokio::time::sleep(Duration::from_secs(31)).await;
            let first_claim = Instant::now() >= *competitor_state.lock().await;
            tokio::time::sleep(Duration::from_secs(30)).await;
            let second_claim = Instant::now() >= *competitor_state.lock().await;
            (first_claim, second_claim)
        };

        let (outcome, claims) = tokio::join!(supervisor, competitor);
        assert_eq!(outcome, MemoryLeaseOutcome::Completed(()));
        assert_eq!(claims, (false, false));
    }

    #[tokio::test(start_paused = true)]
    async fn lease_loss_drops_the_real_provider_future() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let proof = CancellationProof(cancelled.clone());
        let outcome = supervise_memory_job_lease(
            &mut lease(),
            Duration::from_secs(30),
            Instant::now() + Duration::from_secs(100),
            |_| async { Ok(false) },
            async move {
                let _proof = proof;
                std::future::pending::<()>().await
            },
            std::future::pending(),
        )
        .await;

        assert_eq!(outcome, MemoryLeaseOutcome::Lost);
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn an_immortal_provider_is_cancelled_at_the_operation_deadline() {
        let outcome = supervise_memory_job_lease(
            &mut lease(),
            Duration::from_secs(30),
            Instant::now() + Duration::from_secs(25),
            |_| async { Ok(true) },
            std::future::pending::<()>(),
            std::future::pending(),
        )
        .await;

        assert_eq!(outcome, MemoryLeaseOutcome::DeadlineExceeded);
    }

    #[tokio::test]
    async fn shutdown_drops_active_provider_work_immediately() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let proof = CancellationProof(cancelled.clone());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let mut active_lease = lease();
        let operation = supervise_memory_job_lease(
            &mut active_lease,
            Duration::from_secs(30),
            Instant::now() + Duration::from_secs(100),
            |_| async { Ok(true) },
            async move {
                let _proof = proof;
                std::future::pending::<()>().await
            },
            async {
                let _ = shutdown_rx.await;
            },
        );
        tokio::pin!(operation);
        tokio::task::yield_now().await;
        shutdown_tx.send(()).expect("send shutdown");

        assert_eq!(operation.await, MemoryLeaseOutcome::Shutdown);
        assert!(cancelled.load(Ordering::SeqCst));
    }
}
