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

/// Bounded operational reason for an execution ending. Detailed provider/database text belongs
/// in the durable attempt error, not in metric labels where it would create unbounded cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStopReason {
    Completed,
    RetryableFailure,
    TerminalFailure,
    TimedOut,
    Shutdown,
    LeaseLost,
}

impl std::fmt::Display for TaskStopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Completed => "completed",
            Self::RetryableFailure => "retryable_failure",
            Self::TerminalFailure => "terminal_failure",
            Self::TimedOut => "timed_out",
            Self::Shutdown => "shutdown",
            Self::LeaseLost => "lease_lost",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskExecutionMetrics {
    pub company_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub task_type: String,
    pub duration_ms: u64,
    pub status: TaskStatusMetric,
    pub stop_reason: TaskStopReason,
    pub retry_count: u32,
}

#[async_trait::async_trait]
pub trait MonitoringService: Send + Sync {
    fn record_ai_execution(&self, metrics: &AiExecutionMetrics);
    fn record_smtp_connection(&self, metrics: &SmtpConnectionMetrics);
    fn record_task_execution(&self, metrics: &TaskExecutionMetrics);
    fn increment_counter(&self, name: &str, value: u64, labels: &[(&str, &str)]);
    fn record_histogram(&self, name: &str, duration_ms: f64, labels: &[(&str, &str)]);
    fn get_stats_json(&self) -> serde_json::Value;
}
