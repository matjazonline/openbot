use crate::domain::monitoring::{
    AiExecutionMetrics, MonitoringService, SmtpConnectionMetrics, TaskExecutionMetrics,
};
use std::sync::Arc;

pub struct CompositeMonitor {
    monitors: Vec<Arc<dyn MonitoringService>>,
}

impl CompositeMonitor {
    pub fn new(monitors: Vec<Arc<dyn MonitoringService>>) -> Self {
        Self { monitors }
    }
}

#[async_trait::async_trait]
impl MonitoringService for CompositeMonitor {
    fn record_ai_execution(&self, metrics: &AiExecutionMetrics) {
        for monitor in &self.monitors {
            monitor.record_ai_execution(metrics);
        }
    }

    fn record_smtp_connection(&self, metrics: &SmtpConnectionMetrics) {
        for monitor in &self.monitors {
            monitor.record_smtp_connection(metrics);
        }
    }

    fn record_task_execution(&self, metrics: &TaskExecutionMetrics) {
        for monitor in &self.monitors {
            monitor.record_task_execution(metrics);
        }
    }

    fn increment_counter(&self, name: &str, value: u64, labels: &[(&str, &str)]) {
        for monitor in &self.monitors {
            monitor.increment_counter(name, value, labels);
        }
    }

    fn record_gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        for monitor in &self.monitors {
            monitor.record_gauge(name, value, labels);
        }
    }

    fn record_histogram(&self, name: &str, duration_ms: f64, labels: &[(&str, &str)]) {
        for monitor in &self.monitors {
            monitor.record_histogram(name, duration_ms, labels);
        }
    }

    fn get_stats_json(&self) -> serde_json::Value {
        let mut combined = serde_json::Map::new();
        for (idx, monitor) in self.monitors.iter().enumerate() {
            let key = format!("monitor_{}", idx);
            combined.insert(key, monitor.get_stats_json());
        }
        serde_json::Value::Object(combined)
    }
}
