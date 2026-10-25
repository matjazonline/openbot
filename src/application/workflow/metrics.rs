//! Durable deployment-wide workflow observations. Never workflow authority or payload labels.
use crate::{app_error::AppResult, domain::monitoring::MonitoringService};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Current state and explicitly named rolling24h measures, rebuilt from durable owners.
/// Zero latency with zero completed steps means no observations, not instantaneous execution.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct WorkflowMetrics {
    pub sampled_at: f64,
    pub ready_jobs: f64,
    pub oldest_ready_seconds: f64,
    pub active_runs: f64,
    pub waiting_runs: f64,
    pub expired_leases: f64,
    pub expired_run_deadlines: f64,
    pub expired_wait_deadlines: f64,
    pub retries_last24h: f64,
    pub completed_steps_last24h: f64,
    pub step_duration_p95_seconds_last24h: f64,
    pub open_decisions: f64,
    pub oldest_decision_seconds: f64,
    pub reconciliation_runs: f64,
    pub unknown_actions: f64,
    pub unknown_deliveries: f64,
    pub delivered_last24h: f64,
    pub dead_deliveries_last24h: f64,
    pub reserved_tokens: f64,
    pub charged_tokens_last24h: f64,
    pub reserved_estimated_usd: f64,
    pub charged_estimated_usd_last24h: f64,
    pub unpriced_model_calls: f64,
}

impl WorkflowMetrics {
    fn publish(&self, monitoring: &dyn MonitoringService) {
        for (name, value) in [
            ("workflow_ready_jobs", self.ready_jobs),
            ("workflow_oldest_ready_seconds", self.oldest_ready_seconds),
            ("workflow_active_runs", self.active_runs),
            ("workflow_waiting_runs", self.waiting_runs),
            ("workflow_expired_leases", self.expired_leases),
            ("workflow_expired_run_deadlines", self.expired_run_deadlines),
            (
                "workflow_expired_wait_deadlines",
                self.expired_wait_deadlines,
            ),
            ("workflow_retries_last24h", self.retries_last24h),
            (
                "workflow_completed_steps_last24h",
                self.completed_steps_last24h,
            ),
            (
                "workflow_step_duration_p95_seconds_last24h",
                self.step_duration_p95_seconds_last24h,
            ),
            ("workflow_open_decisions", self.open_decisions),
            (
                "workflow_oldest_decision_seconds",
                self.oldest_decision_seconds,
            ),
            ("workflow_reconciliation_runs", self.reconciliation_runs),
            ("workflow_unknown_actions", self.unknown_actions),
            ("workflow_unknown_deliveries", self.unknown_deliveries),
            ("workflow_delivered_last24h", self.delivered_last24h),
            (
                "workflow_dead_deliveries_last24h",
                self.dead_deliveries_last24h,
            ),
            ("workflow_model_reserved_tokens", self.reserved_tokens),
            (
                "workflow_model_charged_tokens_last24h",
                self.charged_tokens_last24h,
            ),
            (
                "workflow_model_reserved_estimated_usd",
                self.reserved_estimated_usd,
            ),
            (
                "workflow_model_charged_estimated_usd_last24h",
                self.charged_estimated_usd_last24h,
            ),
            ("workflow_unpriced_model_calls", self.unpriced_model_calls),
            (
                "workflow_metrics_last_success_timestamp_seconds",
                self.sampled_at,
            ),
        ] {
            monitoring.record_gauge(name, value, &[]);
        }
    }
}

#[async_trait]
pub trait WorkflowMetricPersistence: Send + Sync {
    async fn workflow_metrics(&self) -> AppResult<WorkflowMetrics>;
}

const SAMPLE_INTERVAL: Duration = Duration::from_secs(30);
const SAMPLE_BUDGET: Duration = Duration::from_secs(4);

async fn sample(persistence: &dyn WorkflowMetricPersistence, monitoring: &dyn MonitoringService) {
    match tokio::time::timeout(SAMPLE_BUDGET, persistence.workflow_metrics()).await {
        Ok(Ok(snapshot)) => {
            snapshot.publish(monitoring);
            monitoring.record_gauge("workflow_metrics_collection_ok", 1.0, &[]);
        }
        _ => {
            // Retain the previous successful values and timestamp. Failure must not look idle.
            monitoring.record_gauge("workflow_metrics_collection_ok", 0.0, &[]);
            tracing::warn!("Workflow metrics collection failed or exceeded its budget");
        }
    }
}

/// The host retains/joins this future. Cancellation drops the pending bounded read, never a write.
pub async fn run(
    persistence: &dyn WorkflowMetricPersistence,
    monitoring: &dyn MonitoringService,
    shutdown: &CancellationToken,
) {
    let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            _ = interval.tick() => {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => return,
                    _ = sample(persistence, monitoring) => {}
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;
