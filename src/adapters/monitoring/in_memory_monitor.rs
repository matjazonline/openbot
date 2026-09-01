use crate::domain::monitoring::{
    AiExecutionMetrics, MonitoringService, SmtpConnectionMetrics, SmtpStatus, TaskExecutionMetrics,
    TaskStatusMetric,
};
use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// A gauge's identity is its name *and* its labels. Folding the labels into the key keeps
/// `stuck_work{kind="dead_lettered"}` from overwriting `stuck_work{kind="outbox_failed"}`.
fn gauge_key(name: &str, labels: &[(&str, &str)]) -> String {
    if labels.is_empty() {
        return name.to_string();
    }
    let rendered = labels
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{name}{{{rendered}}}")
}

fn counter_key(name: &str, labels: &[(&str, &str)]) -> String {
    gauge_key(name, labels)
}

pub struct InMemoryMonitor {
    // AI Metrics
    ai_total_executions: AtomicU64,
    ai_successful_executions: AtomicU64,
    ai_failed_executions: AtomicU64,
    ai_total_prompt_tokens: AtomicU64,
    ai_total_completion_tokens: AtomicU64,
    ai_total_tokens: AtomicU64,
    ai_total_duration_ms: AtomicU64,

    // SMTP Metrics
    smtp_total_connections: AtomicU64,
    smtp_accepted: AtomicU64,
    smtp_blocked_dnsbl: AtomicU64,
    smtp_blocked_rate_limit: AtomicU64,
    smtp_rejected_spf: AtomicU64,
    smtp_rejected_dkim: AtomicU64,
    smtp_rejected_dmarc: AtomicU64,
    smtp_rejected_spam_score: AtomicU64,
    smtp_rejected_helo: AtomicU64,
    smtp_rejected_ptr: AtomicU64,
    smtp_errors: AtomicU64,

    // Task Worker Metrics
    task_total_executions: AtomicU64,
    task_completed: AtomicU64,
    task_failed: AtomicU64,
    task_retried: AtomicU64,
    task_total_duration_ms: AtomicU64,

    // Custom Counters
    custom_counters: RwLock<HashMap<String, u64>>,
    /// Latest value per labelled gauge. Overwritten rather than accumulated -- see
    /// [`MonitoringService::record_gauge`].
    gauges: RwLock<HashMap<String, f64>>,
}

impl InMemoryMonitor {
    pub fn new() -> Self {
        Self {
            ai_total_executions: AtomicU64::new(0),
            ai_successful_executions: AtomicU64::new(0),
            ai_failed_executions: AtomicU64::new(0),
            ai_total_prompt_tokens: AtomicU64::new(0),
            ai_total_completion_tokens: AtomicU64::new(0),
            ai_total_tokens: AtomicU64::new(0),
            ai_total_duration_ms: AtomicU64::new(0),

            smtp_total_connections: AtomicU64::new(0),
            smtp_accepted: AtomicU64::new(0),
            smtp_blocked_dnsbl: AtomicU64::new(0),
            smtp_blocked_rate_limit: AtomicU64::new(0),
            smtp_rejected_spf: AtomicU64::new(0),
            smtp_rejected_dkim: AtomicU64::new(0),
            smtp_rejected_dmarc: AtomicU64::new(0),
            smtp_rejected_spam_score: AtomicU64::new(0),
            smtp_rejected_helo: AtomicU64::new(0),
            smtp_rejected_ptr: AtomicU64::new(0),
            smtp_errors: AtomicU64::new(0),

            task_total_executions: AtomicU64::new(0),
            task_completed: AtomicU64::new(0),
            task_failed: AtomicU64::new(0),
            task_retried: AtomicU64::new(0),
            task_total_duration_ms: AtomicU64::new(0),

            custom_counters: RwLock::new(HashMap::new()),
            gauges: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl MonitoringService for InMemoryMonitor {
    fn record_ai_execution(&self, metrics: &AiExecutionMetrics) {
        self.ai_total_executions.fetch_add(1, Ordering::Relaxed);
        if metrics.success {
            self.ai_successful_executions
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.ai_failed_executions.fetch_add(1, Ordering::Relaxed);
        }
        self.ai_total_prompt_tokens
            .fetch_add(metrics.prompt_tokens as u64, Ordering::Relaxed);
        self.ai_total_completion_tokens
            .fetch_add(metrics.completion_tokens as u64, Ordering::Relaxed);
        self.ai_total_tokens
            .fetch_add(metrics.total_tokens as u64, Ordering::Relaxed);
        self.ai_total_duration_ms
            .fetch_add(metrics.duration_ms, Ordering::Relaxed);
    }

    fn record_smtp_connection(&self, metrics: &SmtpConnectionMetrics) {
        self.smtp_total_connections.fetch_add(1, Ordering::Relaxed);
        match metrics.status {
            SmtpStatus::Accepted => self.smtp_accepted.fetch_add(1, Ordering::Relaxed),
            SmtpStatus::BlockedDnsbl => self.smtp_blocked_dnsbl.fetch_add(1, Ordering::Relaxed),
            SmtpStatus::BlockedRateLimit => {
                self.smtp_blocked_rate_limit.fetch_add(1, Ordering::Relaxed)
            }
            SmtpStatus::RejectedSpf => self.smtp_rejected_spf.fetch_add(1, Ordering::Relaxed),
            SmtpStatus::RejectedDkim => self.smtp_rejected_dkim.fetch_add(1, Ordering::Relaxed),
            SmtpStatus::RejectedDmarc => self.smtp_rejected_dmarc.fetch_add(1, Ordering::Relaxed),
            SmtpStatus::RejectedSpamScore => self
                .smtp_rejected_spam_score
                .fetch_add(1, Ordering::Relaxed),
            SmtpStatus::RejectedHelo => self.smtp_rejected_helo.fetch_add(1, Ordering::Relaxed),
            SmtpStatus::RejectedPtr => self.smtp_rejected_ptr.fetch_add(1, Ordering::Relaxed),
            SmtpStatus::Error => self.smtp_errors.fetch_add(1, Ordering::Relaxed),
        };
    }

    fn record_task_execution(&self, metrics: &TaskExecutionMetrics) {
        self.task_total_executions.fetch_add(1, Ordering::Relaxed);
        match metrics.status {
            TaskStatusMetric::Completed => self.task_completed.fetch_add(1, Ordering::Relaxed),
            TaskStatusMetric::Failed => self.task_failed.fetch_add(1, Ordering::Relaxed),
            TaskStatusMetric::Retried => self.task_retried.fetch_add(1, Ordering::Relaxed),
        };
        self.task_total_duration_ms
            .fetch_add(metrics.duration_ms, Ordering::Relaxed);
    }

    fn increment_counter(&self, name: &str, value: u64, labels: &[(&str, &str)]) {
        if let Ok(mut lock) = self.custom_counters.write() {
            *lock.entry(counter_key(name, labels)).or_insert(0) += value;
        }
    }

    fn record_gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        if let Ok(mut lock) = self.gauges.write() {
            lock.insert(gauge_key(name, labels), value);
        }
    }

    fn record_histogram(&self, name: &str, duration_ms: f64, _labels: &[(&str, &str)]) {
        self.increment_counter(&format!("{}_ms", name), duration_ms as u64, &[]);
    }

    fn get_stats_json(&self) -> serde_json::Value {
        let ai_total = self.ai_total_executions.load(Ordering::Relaxed);
        let ai_duration = self.ai_total_duration_ms.load(Ordering::Relaxed);
        let ai_avg_latency_ms = if ai_total > 0 {
            ai_duration as f64 / ai_total as f64
        } else {
            0.0
        };

        let task_total = self.task_total_executions.load(Ordering::Relaxed);
        let task_duration = self.task_total_duration_ms.load(Ordering::Relaxed);
        let task_avg_latency_ms = if task_total > 0 {
            task_duration as f64 / task_total as f64
        } else {
            0.0
        };

        let custom = self
            .custom_counters
            .read()
            .ok()
            .map(|c| c.clone())
            .unwrap_or_default();

        let gauges: HashMap<String, f64> = self
            .gauges
            .read()
            .ok()
            .map(|gauges| gauges.clone())
            .unwrap_or_default();

        serde_json::json!({
            "ai_executions": {
                "total": ai_total,
                "successful": self.ai_successful_executions.load(Ordering::Relaxed),
                "failed": self.ai_failed_executions.load(Ordering::Relaxed),
                "total_prompt_tokens": self.ai_total_prompt_tokens.load(Ordering::Relaxed),
                "total_completion_tokens": self.ai_total_completion_tokens.load(Ordering::Relaxed),
                "total_tokens": self.ai_total_tokens.load(Ordering::Relaxed),
                "avg_latency_ms": ai_avg_latency_ms
            },
            "smtp_connections": {
                "total": self.smtp_total_connections.load(Ordering::Relaxed),
                "accepted": self.smtp_accepted.load(Ordering::Relaxed),
                "blocked_dnsbl": self.smtp_blocked_dnsbl.load(Ordering::Relaxed),
                "blocked_rate_limit": self.smtp_blocked_rate_limit.load(Ordering::Relaxed),
                "rejected_spf": self.smtp_rejected_spf.load(Ordering::Relaxed),
                "rejected_dkim": self.smtp_rejected_dkim.load(Ordering::Relaxed),
                "rejected_dmarc": self.smtp_rejected_dmarc.load(Ordering::Relaxed),
                "rejected_spam_score": self.smtp_rejected_spam_score.load(Ordering::Relaxed),
                "rejected_helo": self.smtp_rejected_helo.load(Ordering::Relaxed),
                "rejected_ptr": self.smtp_rejected_ptr.load(Ordering::Relaxed),
                "errors": self.smtp_errors.load(Ordering::Relaxed)
            },
            "tasks": {
                "total": task_total,
                "completed": self.task_completed.load(Ordering::Relaxed),
                "failed": self.task_failed.load(Ordering::Relaxed),
                "retried": self.task_retried.load(Ordering::Relaxed),
                "avg_latency_ms": task_avg_latency_ms
            },
            "custom_counters": custom,
            "gauges": gauges
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_counters_retain_fixed_labels_without_merging_buckets() {
        let monitor = InMemoryMonitor::new();
        monitor.increment_counter(
            "pagination_observed",
            1,
            &[("endpoint", "tasks"), ("offset_bucket", "0-99")],
        );
        monitor.increment_counter(
            "pagination_observed",
            2,
            &[("endpoint", "tasks"), ("offset_bucket", "1000+")],
        );
        let stats = monitor.get_stats_json();
        let counters = stats["custom_counters"].as_object().unwrap();
        assert_eq!(
            counters["pagination_observed{endpoint=tasks,offset_bucket=0-99}"],
            1
        );
        assert_eq!(
            counters["pagination_observed{endpoint=tasks,offset_bucket=1000+}"],
            2
        );
        assert_eq!(counters.len(), 2);
    }
}
