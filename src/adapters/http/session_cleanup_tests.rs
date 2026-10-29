use super::*;
use crate::{adapters::monitoring::InMemoryMonitor, application::session::SessionCleanup};
use std::sync::atomic::{AtomicBool, Ordering};

struct HeldCleanup {
    entered: tokio::sync::Notify,
    dropped: Arc<AtomicBool>,
}
struct InFlight(Arc<AtomicBool>);
impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
#[async_trait::async_trait]
impl SessionPersistence for HeldCleanup {
    async fn create_session(&self, _: &SessionRecord) -> AppResult<()> {
        unreachable!()
    }
    async fn session_is_live(&self, _: &SessionRecord) -> AppResult<bool> {
        unreachable!()
    }
    async fn revoke_session(&self, _: VerifiedSession) -> AppResult<()> {
        unreachable!()
    }
    async fn cleanup_sessions(&self) -> AppResult<SessionCleanup> {
        let _active = InFlight(self.dropped.clone());
        self.entered.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn security_session_cleanup_shutdown_cancels_actual_work() {
    let dropped = Arc::new(AtomicBool::new(false));
    let store = Arc::new(HeldCleanup {
        entered: tokio::sync::Notify::new(),
        dropped: dropped.clone(),
    });
    let sessions = Arc::new(SessionAuthority::new(&AppConfig::for_test(), store.clone()));
    let (stop, receiver) = tokio::sync::broadcast::channel(1);
    let worker = tokio::spawn(sessions.run_cleanup(receiver));
    tokio::time::timeout(std::time::Duration::from_secs(1), store.entered.notified())
        .await
        .unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), worker)
        .await
        .unwrap()
        .unwrap();
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn security_session_validation_and_cleanup_metrics_use_fixed_labels() {
    let Some((database, persistence, user)) = super::super::concurrency_tests::fixture().await
    else {
        return;
    };
    let monitoring = Arc::new(InMemoryMonitor::new());
    let sessions = Arc::new(
        SessionAuthority::new(&AppConfig::for_test(), persistence)
            .with_monitoring(monitoring.clone()),
    );
    let cookie = sessions.cookie(&user).await.unwrap();
    assert!(
        sessions
            .verify("invalid-token-marker")
            .await
            .unwrap()
            .is_none()
    );
    let verified = sessions.verify(cookie.value()).await.unwrap().unwrap();
    sessions.revoke(verified).await.unwrap();
    assert!(sessions.verify(cookie.value()).await.unwrap().is_none());
    let (stop, receiver) = tokio::sync::broadcast::channel(1);
    let worker = tokio::spawn(sessions.clone().run_cleanup(receiver));
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while monitoring.get_stats_json()["gauges"]["session_cleanup_backlog"].is_null() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    worker.await.unwrap();
    database.pool.close().await;
    assert!(sessions.verify(cookie.value()).await.is_err());
    let stats = monitoring.get_stats_json();
    assert_eq!(
        stats["custom_counters"]["session_validation_failures{reason=invalid_token}"],
        1
    );
    assert_eq!(
        stats["custom_counters"]["session_validation_failures{reason=inactive_session}"],
        1
    );
    assert_eq!(
        stats["custom_counters"]["session_validation_failures{reason=store_unavailable}"],
        1
    );
    assert_eq!(stats["custom_counters"]["session_cleanup_removed"], 1);
    assert_eq!(stats["gauges"]["session_cleanup_backlog"], 0.0);
    assert_eq!(stats["gauges"]["session_cleanup_backlog_capped"], 0.0);
    let rendered = stats.to_string();
    assert!(!rendered.contains(cookie.value()));
    assert!(!rendered.contains(&user.id.to_string()));
    assert!(!rendered.contains("invalid-token-marker"));
}

#[tokio::test]
async fn security_session_cleanup_failure_is_observable() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@localhost/unused")
        .unwrap();
    pool.close().await;
    let monitoring = Arc::new(InMemoryMonitor::new());
    let store = Arc::new(crate::adapters::persistence::PostgresPersistence::new(pool));
    let sessions = Arc::new(
        SessionAuthority::new(&AppConfig::for_test(), store).with_monitoring(monitoring.clone()),
    );
    let (stop, receiver) = tokio::sync::broadcast::channel(1);
    let worker = tokio::spawn(sessions.run_cleanup(receiver));
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while monitoring.get_stats_json()["custom_counters"]["session_cleanup_failures"].is_null() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    worker.await.unwrap();
    assert_eq!(
        monitoring.get_stats_json()["custom_counters"]["session_cleanup_failures"],
        1
    );
    assert!(
        monitoring.get_stats_json()["gauges"]["session_cleanup_last_success_unix_seconds"]
            .is_null()
    );
}
