use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiExecutionMetrics {
    pub company_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub provider: String,
    pub model: String,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub duration_ms: u64,
    pub success: bool,
    pub error_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SmtpStatus {
    Accepted,
    BlockedDnsbl,
    BlockedRateLimit,
    RejectedSpf,
    RejectedDkim,
    RejectedDmarc,
    RejectedSpamScore,
    RejectedHelo,
    RejectedPtr,
    Error,
}

impl std::fmt::Display for SmtpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SmtpStatus::Accepted => write!(f, "accepted"),
            SmtpStatus::BlockedDnsbl => write!(f, "blocked_dnsbl"),
            SmtpStatus::BlockedRateLimit => write!(f, "blocked_rate_limit"),
            SmtpStatus::RejectedSpf => write!(f, "rejected_spf"),
            SmtpStatus::RejectedDkim => write!(f, "rejected_dkim"),
            SmtpStatus::RejectedDmarc => write!(f, "rejected_dmarc"),
            SmtpStatus::RejectedSpamScore => write!(f, "rejected_spam_score"),
            SmtpStatus::RejectedHelo => write!(f, "rejected_helo"),
            SmtpStatus::RejectedPtr => write!(f, "rejected_ptr"),
            SmtpStatus::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmtpConnectionMetrics {
    pub client_ip: IpAddr,
    pub status: SmtpStatus,
    pub duration_ms: u64,
    pub mail_from: Option<String>,
    pub rcpt_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TaskStatusMetric {
    Completed,
    Failed,
    Retried,
}

impl std::fmt::Display for TaskStatusMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatusMetric::Completed => write!(f, "completed"),
            TaskStatusMetric::Failed => write!(f, "failed"),
            TaskStatusMetric::Retried => write!(f, "retried"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskExecutionMetrics {
    pub company_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub task_type: String,
    pub duration_ms: u64,
    pub status: TaskStatusMetric,
    pub retry_count: u32,
}

#[async_trait::async_trait]
pub trait MonitoringService: Send + Sync {
    fn record_ai_execution(&self, metrics: &AiExecutionMetrics);
    fn record_smtp_connection(&self, metrics: &SmtpConnectionMetrics);
    fn record_task_execution(&self, metrics: &TaskExecutionMetrics);
    fn increment_counter(&self, name: &str, value: u64, labels: &[(&str, &str)]);
    /// Report the *current* value of something that goes up and down.
    ///
    /// Distinct from [`Self::increment_counter`] because a standing condition is not an
    /// occurrence: four dead-lettered tasks are four dead-lettered tasks however many times the
    /// sweep looks at them, and adding four to a counter every thirty seconds would describe a
    /// runaway failure that is not happening. A gauge also *clears*, which is what lets an alert
    /// on it stop firing once the work is dealt with.
    fn record_gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]);
    fn record_histogram(&self, name: &str, duration_ms: f64, labels: &[(&str, &str)]);
    fn get_stats_json(&self) -> serde_json::Value;
}
