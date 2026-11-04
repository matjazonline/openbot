//! Application-wide storage maintenance. Discovery never grants deletion authority.
use crate::{
    app_error::{AppError, AppResult},
    domain::{
        monitoring::MonitoringService,
        workflow::{CompanyId, RunId},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const RETENTION_PAGE_SIZE: usize = 4;
pub const STORAGE_SWEEP_LIMIT: u16 = 256;
const OPERATION_BUDGET: Duration = Duration::from_secs(6);
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionRoot {
    pub finished_at: DateTime<Utc>,
    pub company: CompanyId,
    pub run: RunId,
}
#[derive(Debug)]
pub struct RetentionPage {
    pub roots: Vec<RetentionRoot>,
    pub next: Option<RetentionRoot>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetiredTree {
    pub runs: u64,
    pub jobs: u64,
    pub calls: u64,
}

#[async_trait]
pub trait WorkflowStorageMaintenance: Send + Sync {
    async fn sweep_obsolete_witnesses(&self, limit: u16) -> AppResult<u64>;
    async fn retention_page(&self, after: Option<RetentionRoot>) -> AppResult<RetentionPage>;
    /// Recheck and lock the complete tree; archive usage and remove runs/jobs atomically.
    async fn retire_tree(&self, root: RetentionRoot) -> AppResult<Option<RetiredTree>>;
    async fn sweep_unused_bundles(&self, limit: u16) -> AppResult<u64>;
    async fn oldest_terminal_root(&self) -> AppResult<Option<DateTime<Utc>>>;
}

async fn bounded<T>(
    future: impl std::future::Future<Output = AppResult<T>>,
    monitoring: &dyn MonitoringService,
) -> AppResult<T> {
    let result = tokio::time::timeout(OPERATION_BUDGET, future)
        .await
        .unwrap_or_else(|_| {
            Err(AppError::Timeout(
                "Workflow storage operation timed out".into(),
            ))
        });
    if let Err(error) = &result {
        monitoring.increment_counter("workflow_storage_errors_total", 1, &[]);
        tracing::warn!(%error, "Workflow storage operation failed");
    }
    result
}

async fn sweep(
    port: &dyn WorkflowStorageMaintenance,
    monitoring: &dyn MonitoringService,
    after: &mut Option<RetentionRoot>,
) {
    if let Ok(count) = bounded(
        port.sweep_obsolete_witnesses(STORAGE_SWEEP_LIMIT),
        monitoring,
    )
    .await
    {
        monitoring.increment_counter("workflow_storage_witnesses_deleted_total", count, &[]);
    }
    if let Ok(page) = bounded(port.retention_page(*after), monitoring).await {
        // Advance past every candidate, including protected or failed trees. Wrap on exhaustion.
        *after = page.next;
        for root in page.roots {
            match bounded(port.retire_tree(root), monitoring).await {
                Ok(Some(deleted)) => {
                    for (name, count) in [
                        ("workflow_storage_trees_deleted_total", 1),
                        ("workflow_storage_runs_deleted_total", deleted.runs),
                        ("workflow_storage_jobs_deleted_total", deleted.jobs),
                        ("workflow_storage_usage_archived_total", deleted.calls),
                    ] {
                        monitoring.increment_counter(name, count, &[]);
                    }
                }
                Ok(None) => {
                    monitoring.increment_counter("workflow_storage_trees_skipped_total", 1, &[])
                }
                Err(_) => {}
            }
        }
    }
    if let Ok(count) = bounded(port.sweep_unused_bundles(STORAGE_SWEEP_LIMIT), monitoring).await {
        monitoring.increment_counter("workflow_storage_bundles_deleted_total", count, &[]);
    }
    if let Ok(oldest) = bounded(port.oldest_terminal_root(), monitoring).await {
        monitoring.record_gauge(
            "workflow_storage_oldest_terminal_root_finished_timestamp_seconds",
            oldest.map_or(0.0, |time| time.timestamp() as f64),
            &[],
        );
        monitoring.record_gauge(
            "workflow_storage_last_observation_timestamp_seconds",
            Utc::now().timestamp() as f64,
            &[],
        );
    }
}

/// The host owns and joins this future. Cancellation drops the actual in-flight transaction.
pub async fn run(
    port: &dyn WorkflowStorageMaintenance,
    monitoring: &dyn MonitoringService,
    shutdown: &CancellationToken,
) {
    let mut after = None;
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            _ = sweep(port, monitoring, &mut after) => {}
        }
        // A full page or an error never turns cleanup into a busy polling loop.
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(SWEEP_INTERVAL) => {}
        }
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
