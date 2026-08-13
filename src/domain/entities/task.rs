use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Processing,
    PendingApproval,
    Completed,
    Failed,
    DeadLetter,
    Stopped,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Processing => "processing",
            TaskStatus::PendingApproval => "pending_approval",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::DeadLetter => "dead_letter",
            TaskStatus::Stopped => "stopped",
        }
    }
}

impl FromStr for TaskStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(TaskStatus::Pending),
            "processing" => Ok(TaskStatus::Processing),
            "pending_approval" | "pendingapproval" => Ok(TaskStatus::PendingApproval),
            "completed" => Ok(TaskStatus::Completed),
            "failed" => Ok(TaskStatus::Failed),
            "dead_letter" => Ok(TaskStatus::DeadLetter),
            "stopped" => Ok(TaskStatus::Stopped),
            _ => Err(format!("Unknown task status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

impl TokenUsage {
    pub fn new(prompt_tokens: usize, completion_tokens: usize) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackgroundTask {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub task_type: String,
    pub status: TaskStatus,
    pub payload: serde_json::Value,
    pub retry_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub run_at: chrono::NaiveDateTime,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

impl BackgroundTask {
    pub fn token_usage(&self) -> Option<TokenUsage> {
        let usage_val = self
            .payload
            .get("execution_result")
            .and_then(|res| res.get("token_usage"))
            .or_else(|| self.payload.get("token_usage"));

        if let Some(val) = usage_val {
            serde_json::from_value(val.clone()).ok()
        } else {
            None
        }
    }
}
