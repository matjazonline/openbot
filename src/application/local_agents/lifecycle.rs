//! Fixed, bounded retention under the process supervisor. Physical settlement is
//! owned by persistence; a workflow timeout is never cleanup authority.
use crate::app_error::AppResult;
use async_trait::async_trait;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const COMPANY_PAGE: usize = 4;
pub const RETENTION_PAGE: i64 = 128;

#[derive(Default)]
pub struct RetentionPage {
    pub next_company: Option<Uuid>,
    pub compacted_observations: u64,
    pub archived_calls: u64,
    pub removed_operations: u64,
    pub removed_sessions: u64,
}

#[async_trait]
pub trait LocalAgentMaintenance: Send + Sync {
    async fn retain_local_agents(&self, after_company: Option<Uuid>) -> AppResult<RetentionPage>;
}

pub async fn run(port: &dyn LocalAgentMaintenance, shutdown: &CancellationToken) {
    let mut cursor = None;
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => return,
            _ = interval.tick() => {}
        }
        let checked = tokio::select! {
            biased;
            () = shutdown.cancelled() => return,
            result = tokio::time::timeout(Duration::from_secs(6),port.retain_local_agents(cursor)) => result,
        };
        match checked {
            Ok(Ok(page)) => cursor = page.next_company,
            // Database errors can contain row values. Retention logs only safe state.
            _ => tracing::warn!(?cursor, "local-agent retention page failed or timed out"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::Notify;

    struct Blocked {
        entered: Notify,
        cancelled: Arc<AtomicBool>,
    }
    struct Pending(Arc<AtomicBool>);
    impl Drop for Pending {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl LocalAgentMaintenance for Blocked {
        async fn retain_local_agents(&self, _: Option<Uuid>) -> AppResult<RetentionPage> {
            let _pending = Pending(self.cancelled.clone());
            self.entered.notify_one();
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn cloudroom_lifecycle_retention_shutdown_cancels_the_actual_pending_port_future() {
        let port = Arc::new(Blocked {
            entered: Notify::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        let shutdown = CancellationToken::new();
        let owned = port.clone();
        let stop = shutdown.clone();
        let task = tokio::spawn(async move { run(owned.as_ref(), &stop).await });
        port.entered.notified().await;
        shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert!(port.cancelled.load(Ordering::SeqCst));
    }
}
