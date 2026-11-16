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
// Reserve work for both payloads and unused scopes so either protected prefix cannot starve the other.
pub const HISTORY_EXPIRY_SWEEP_LIMIT: u16 = STORAGE_SWEEP_LIMIT / 2;
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

/// Discovery cursor, never authority to expire a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryCandidate {
    pub company: CompanyId,
    pub finished_at: DateTime<Utc>,
    pub exchange: crate::application::workflow::agent_history::AgentExchangeId,
}

#[derive(Debug)]
pub struct HistoryPage {
    pub candidates: Vec<HistoryCandidate>,
}

/// Discovery only; deletion must recheck every live canonical owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationOriginCursor {
    pub company: CompanyId,
    pub origin: uuid::Uuid,
}

#[derive(Debug)]
pub struct NotificationOriginSweep {
    pub deleted: u64,
    pub next: Option<NotificationOriginCursor>,
}

#[derive(Default)]
struct SweepCursor {
    root: Option<RetentionRoot>,
    history: Option<HistoryCandidate>,
    history_scope: Option<CompanyId>,
    notification_origin: Option<NotificationOriginCursor>,
}

#[async_trait]
pub trait WorkflowStorageMaintenance: Send + Sync {
    async fn sweep_obsolete_witnesses(&self, limit: u16) -> AppResult<u64>;
    async fn sweep_notification_origins(
        &self,
        after: Option<NotificationOriginCursor>,
        limit: u16,
    ) -> AppResult<NotificationOriginSweep>;
    async fn history_page(&self, after: Option<HistoryCandidate>) -> AppResult<HistoryPage>;
    /// Lock current source ownership and recheck every pin before clearing history copies.
    async fn expire_history(&self, candidate: HistoryCandidate) -> AppResult<bool>;
    async fn history_scope_page(&self, after: Option<CompanyId>) -> AppResult<Vec<CompanyId>>;
    async fn prune_history(&self, company: CompanyId, limit: u16) -> AppResult<u64>;
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
    cursor: &mut SweepCursor,
) {
    if let Ok(count) = bounded(
        port.sweep_obsolete_witnesses(STORAGE_SWEEP_LIMIT),
        monitoring,
    )
    .await
    {
        monitoring.increment_counter("workflow_storage_witnesses_deleted_total", count, &[]);
    }
    if let Ok(swept) = bounded(
        port.sweep_notification_origins(cursor.notification_origin, STORAGE_SWEEP_LIMIT),
        monitoring,
    )
    .await
    {
        cursor.notification_origin = swept.next;
        monitoring.increment_counter(
            "workflow_storage_notification_origins_deleted_total",
            swept.deleted,
            &[],
        );
    }
    sweep_history(port, monitoring, cursor).await;
    if let Ok(page) = bounded(port.retention_page(cursor.root), monitoring).await {
        // Advance past every candidate, including protected or failed trees. Wrap on exhaustion.
        cursor.root = page.next;
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

async fn sweep_history(
    port: &dyn WorkflowStorageMaintenance,
    monitoring: &dyn MonitoringService,
    cursor: &mut SweepCursor,
) {
    if let Ok(page) = bounded(port.history_page(cursor.history), monitoring).await {
        if page.candidates.is_empty() {
            cursor.history = None;
        }
        for candidate in page.candidates {
            // Advance before mutation, including a busy/protected/error candidate.
            cursor.history = Some(candidate);
            match bounded(port.expire_history(candidate), monitoring).await {
                Ok(true) => {
                    monitoring.increment_counter("agent_history_payloads_expired_total", 1, &[])
                }
                Ok(false) => {
                    monitoring.increment_counter("agent_history_payloads_skipped_total", 1, &[])
                }
                Err(_) => {}
            }
        }
    }
    if let Ok(companies) = bounded(port.history_scope_page(cursor.history_scope), monitoring).await
    {
        if companies.is_empty() {
            cursor.history_scope = None;
        }
        for company in companies {
            cursor.history_scope = Some(company);
            let limit =
                (STORAGE_SWEEP_LIMIT - HISTORY_EXPIRY_SWEEP_LIMIT) / RETENTION_PAGE_SIZE as u16;
            if let Ok(count) = bounded(port.prune_history(company, limit), monitoring).await {
                monitoring.increment_counter("agent_history_scopes_pruned_total", count, &[]);
            }
        }
    }
}

/// The host owns and joins this future. Cancellation drops the actual in-flight transaction.
pub async fn run(
    port: &dyn WorkflowStorageMaintenance,
    monitoring: &dyn MonitoringService,
    shutdown: &CancellationToken,
) {
    let mut cursor = SweepCursor::default();
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            _ = sweep(port, monitoring, &mut cursor) => {}
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
