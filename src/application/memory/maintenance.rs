//! Retention of settled memory rows and their gauges (p06 gate A3-1–A3-3, A2-4).
//!
//! One supervised task, run when the memory service is configured. Every 5 minutes it removes
//! settled delivery intents and settled absent lifecycle rows in bounded batches, then records
//! the open-row gauges and the retention backlog (rows already eligible but not yet removed).
//! A backlog above [`BACKLOG_WARN_ROWS`] warns once per run: the early signal that the batch
//! bound is being approached.
use crate::app_error::AppResult;
use crate::domain::monitoring::MonitoringService;
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

pub const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(300);
pub const RETENTION_BATCH: u16 = 500;
/// Full batches repeated per table per run before the next interval.
pub const MAX_BATCHES_PER_RUN: usize = 8;
pub const BACKLOG_WARN_ROWS: i64 = 10_000;
/// A settled intent is kept this long: past the service's 7-day idempotency window.
pub const SETTLED_INTENT_RETENTION_DAYS: i32 = 7;
/// A settled absent lifecycle row whose owner is gone is kept this long. It is also the window
/// within which a service restore can be reconciled from these rows (p06 A1 F3).
pub const SETTLED_LIFECYCLE_RETENTION_DAYS: i32 = 30;

/// Open and backlog row counts, from partial indexes only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryRowCounts {
    pub deliveries_pending: i64,
    pub deliveries_claimed: i64,
    pub lifecycles_pending: i64,
    pub lifecycles_claimed: i64,
    pub lifecycles_failed: i64,
    /// Pending or claimed rows retrying an outage 12 or more times (the stuck threshold).
    pub lifecycles_stuck: i64,
    /// Rows postponed because the configured provisioner does not (yet) manage their tenant.
    pub lifecycles_provisioner_mismatch: i64,
    /// Absent steps waiting for a disabled tenant to be enabled or deleted.
    pub lifecycles_tenant_disabled: i64,
    /// Steps waiting to re-check a tenant the probe just confirmed (review B1 F2a).
    pub lifecycles_tenant_recheck: i64,
    pub delivery_backlog: i64,
    pub lifecycle_backlog: i64,
}

#[async_trait]
pub trait MemoryMaintenanceStore: Send + Sync {
    /// Remove at most `limit` settled intents that can no longer be replayed or read: terminal,
    /// created more than 7 days ago, and either a `generation` intent or a save whose run is
    /// retired, or terminal with its deadline passed (a control retry needs a future deadline).
    async fn sweep_settled_deliveries(&self, limit: u16) -> AppResult<u64>;
    /// Remove at most `limit` settled `absent` lifecycle rows older than 30 days whose owner
    /// row is gone.
    async fn sweep_settled_lifecycles(&self, limit: u16) -> AppResult<u64>;
    async fn memory_row_counts(&self) -> AppResult<MemoryRowCounts>;
}

/// What one run removed and saw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MaintenanceRun {
    pub deliveries_removed: u64,
    pub lifecycles_removed: u64,
    pub counts: MemoryRowCounts,
}

pub struct MemoryMaintenance {
    store: Arc<dyn MemoryMaintenanceStore>,
    monitoring: Arc<dyn MonitoringService>,
}

impl MemoryMaintenance {
    pub fn new(
        store: Arc<dyn MemoryMaintenanceStore>,
        monitoring: Arc<dyn MonitoringService>,
    ) -> Self {
        Self { store, monitoring }
    }

    pub async fn run(&self, stop: &CancellationToken) {
        loop {
            if let Err(error) = self.pass().await {
                self.monitoring
                    .increment_counter("memory_retention_errors_total", 1, &[]);
                tracing::warn!(%error, "Memory maintenance failed; retrying at the next interval");
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = tokio::time::sleep(MAINTENANCE_INTERVAL) => {}
            }
        }
    }

    /// One run: both sweeps (a full batch repeats, at most [`MAX_BATCHES_PER_RUN`] times),
    /// then the gauges.
    pub async fn pass(&self) -> AppResult<MaintenanceRun> {
        let mut run = MaintenanceRun::default();
        for _ in 0..MAX_BATCHES_PER_RUN {
            let removed = self.store.sweep_settled_deliveries(RETENTION_BATCH).await?;
            run.deliveries_removed += removed;
            if removed < u64::from(RETENTION_BATCH) {
                break;
            }
        }
        for _ in 0..MAX_BATCHES_PER_RUN {
            let removed = self.store.sweep_settled_lifecycles(RETENTION_BATCH).await?;
            run.lifecycles_removed += removed;
            if removed < u64::from(RETENTION_BATCH) {
                break;
            }
        }
        let monitoring = self.monitoring.as_ref();
        for (table, count) in [
            ("memory_deliveries", run.deliveries_removed),
            ("memory_lifecycles", run.lifecycles_removed),
        ] {
            monitoring.increment_counter(
                "memory_retention_deleted_total",
                count,
                &[("table", table)],
            );
        }
        let counts = self.store.memory_row_counts().await?;
        run.counts = counts;
        let gauge = |name: &str, value: i64, labels: &[(&str, &str)]| {
            monitoring.record_gauge(name, value as f64, labels);
        };
        gauge(
            "memory_deliveries_rows",
            counts.deliveries_pending,
            &[("state", "pending")],
        );
        gauge(
            "memory_deliveries_rows",
            counts.deliveries_claimed,
            &[("state", "claimed")],
        );
        gauge(
            "memory_lifecycles_rows",
            counts.lifecycles_pending,
            &[("state", "pending")],
        );
        gauge(
            "memory_lifecycles_rows",
            counts.lifecycles_claimed,
            &[("state", "claimed")],
        );
        gauge(
            "memory_lifecycle_failed_rows",
            counts.lifecycles_failed,
            &[],
        );
        gauge("memory_lifecycle_stuck_rows", counts.lifecycles_stuck, &[]);
        for (outcome, rows) in [
            (
                "provisioner_mismatch",
                counts.lifecycles_provisioner_mismatch,
            ),
            ("tenant_disabled", counts.lifecycles_tenant_disabled),
            ("tenant_recheck", counts.lifecycles_tenant_recheck),
        ] {
            gauge(
                "memory_lifecycle_postponed_rows",
                rows,
                &[("outcome", outcome)],
            );
        }
        gauge(
            "memory_retention_backlog_rows",
            counts.delivery_backlog,
            &[("table", "memory_deliveries")],
        );
        gauge(
            "memory_retention_backlog_rows",
            counts.lifecycle_backlog,
            &[("table", "memory_lifecycles")],
        );
        for (table, backlog) in [
            ("memory_deliveries", counts.delivery_backlog),
            ("memory_lifecycles", counts.lifecycle_backlog),
        ] {
            if backlog > BACKLOG_WARN_ROWS {
                tracing::warn!(
                    table,
                    backlog,
                    threshold = BACKLOG_WARN_ROWS,
                    "Memory retention backlog above its threshold; the sweep is not keeping up"
                );
            }
        }
        Ok(run)
    }
}
