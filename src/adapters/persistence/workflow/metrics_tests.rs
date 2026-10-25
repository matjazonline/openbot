use super::*;
use crate::application::workflow::metrics::WorkflowMetricPersistence;

#[tokio::test]
async fn workflow_metrics_real_owners_and_bounded_lock_failure() {
    // Normal publication/admission creates a live HTTP execution, never fabricated history.
    let setup = Box::pin(setup("http://127.0.0.1:1", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let metrics = p.workflow_metrics().await.unwrap();
    assert_eq!(metrics.active_runs, 1.0);
    assert_eq!(metrics.waiting_runs, 0.0);
    assert_eq!(metrics.unknown_actions, 0.0);
    assert_eq!(metrics.reserved_tokens, 0.0);
    assert!(metrics.sampled_at > 0.0);

    let mut lock = p.pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE workflow_runs IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    assert!(p.workflow_metrics().await.is_err());
    assert!(started.elapsed() < Duration::from_secs(4));
    lock.rollback().await.unwrap();
    let recovered = p.workflow_metrics().await.unwrap();
    assert_eq!(recovered.active_runs, 1.0);
    assert!(recovered.sampled_at >= metrics.sampled_at);
}
