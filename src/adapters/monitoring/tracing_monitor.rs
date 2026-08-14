use crate::domain::monitoring::{
    AiExecutionMetrics, MonitoringService, SmtpConnectionMetrics, TaskExecutionMetrics,
};
use tracing::info;

pub struct TracingMonitor;

impl TracingMonitor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TracingMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl MonitoringService for TracingMonitor {
    fn record_ai_execution(&self, metrics: &AiExecutionMetrics) {
        info!(
            target: "monitoring::ai",
            company_id = ?metrics.company_id,
            channel_id = ?metrics.channel_id,
            agent_id = ?metrics.agent_id,
            provider = %metrics.provider,
            model = %metrics.model,
            prompt_tokens = metrics.prompt_tokens,
            completion_tokens = metrics.completion_tokens,
            total_tokens = metrics.total_tokens,
            duration_ms = metrics.duration_ms,
            success = metrics.success,
            error_type = ?metrics.error_type,
            "AI Execution Completed"
        );
    }

    fn record_smtp_connection(&self, metrics: &SmtpConnectionMetrics) {
        info!(
            target: "monitoring::smtp",
            client_ip = %metrics.client_ip,
            status = %metrics.status,
            duration_ms = metrics.duration_ms,
            mail_from = ?metrics.mail_from,
            rcpt_to = ?metrics.rcpt_to,
            "SMTP Connection Processed"
        );
    }

    fn record_task_execution(&self, metrics: &TaskExecutionMetrics) {
        info!(
            target: "monitoring::task",
            company_id = ?metrics.company_id,
            channel_id = ?metrics.channel_id,
            task_type = %metrics.task_type,
            duration_ms = metrics.duration_ms,
            status = %metrics.status,
            retry_count = metrics.retry_count,
            "Task Execution Processed"
        );
    }

    fn increment_counter(&self, name: &str, value: u64, labels: &[(&str, &str)]) {
        info!(
            target: "monitoring::counter",
            metric = %name,
            value = value,
            labels = ?labels,
            "Counter incremented"
        );
    }

    fn record_histogram(&self, name: &str, duration_ms: f64, labels: &[(&str, &str)]) {
        info!(
            target: "monitoring::histogram",
            metric = %name,
            duration_ms = duration_ms,
            labels = ?labels,
            "Histogram recorded"
        );
    }

    fn get_stats_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": "active",
            "type": "tracing"
        })
    }
}
