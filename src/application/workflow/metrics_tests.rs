use super::*;
use crate::adapters::monitoring::InMemoryMonitor;
use std::sync::atomic::{AtomicBool, Ordering};

struct SnapshotPort(Option<f64>);

#[async_trait]
impl WorkflowMetricPersistence for SnapshotPort {
    async fn workflow_metrics(&self) -> AppResult<WorkflowMetrics> {
        match self.0 {
            Some(ready_jobs) => Ok(WorkflowMetrics {
                ready_jobs,
                sampled_at: 100.0 + ready_jobs,
                ..Default::default()
            }),
            None => Err(crate::app_error::AppError::Timeout("test".into())),
        }
    }
}

#[tokio::test]
async fn workflow_metrics_failure_retains_last_success_and_empty_success_clears() {
    let monitor = InMemoryMonitor::new();
    sample(&SnapshotPort(Some(3.0)), &monitor).await;
    sample(&SnapshotPort(None), &monitor).await;
    let gauges = monitor.get_stats_json()["gauges"].clone();
    assert_eq!(gauges["workflow_ready_jobs"], 3.0);
    assert_eq!(
        gauges["workflow_metrics_last_success_timestamp_seconds"],
        103.0
    );
    assert_eq!(gauges["workflow_metrics_collection_ok"], 0.0);
    sample(&SnapshotPort(Some(0.0)), &monitor).await;
    let gauges = monitor.get_stats_json()["gauges"].clone();
    assert_eq!(gauges["workflow_ready_jobs"], 0.0);
    assert_eq!(
        gauges["workflow_metrics_last_success_timestamp_seconds"],
        100.0
    );
    assert_eq!(gauges["workflow_metrics_collection_ok"], 1.0);
    assert_eq!(gauges.as_object().unwrap().len(), 24);
    assert!(
        gauges
            .as_object()
            .unwrap()
            .keys()
            .all(|key| key.starts_with("workflow_") && !key.contains('{'))
    );
}

struct PendingPort {
    entered: tokio::sync::Notify,
    dropped: AtomicBool,
}

struct PendingRead<'a>(&'a AtomicBool);
impl Drop for PendingRead<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl WorkflowMetricPersistence for PendingPort {
    async fn workflow_metrics(&self) -> AppResult<WorkflowMetrics> {
        let _read = PendingRead(&self.dropped);
        self.entered.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn workflow_metrics_shutdown_drops_pending_read_and_joins() {
    let port = PendingPort {
        entered: Default::default(),
        dropped: AtomicBool::new(false),
    };
    let monitor = InMemoryMonitor::new();
    let shutdown = CancellationToken::new();
    let stop = async {
        port.entered.notified().await;
        shutdown.cancel();
    };
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(run(&port, &monitor, &shutdown), stop);
    })
    .await
    .unwrap();
    assert!(port.dropped.load(Ordering::SeqCst));
    assert_eq!(monitor.get_stats_json()["gauges"], serde_json::json!({}));
}
