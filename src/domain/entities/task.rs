use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Processing,
    PendingApproval,
    WaitingForThirdPartyReply,
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
            TaskStatus::WaitingForThirdPartyReply => "waiting_for_third_party_reply",
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
            "waiting_for_third_party_reply" | "waitingforthirdpartyreply" => {
                Ok(TaskStatus::WaitingForThirdPartyReply)
            }
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
    pub worker_id: Option<Uuid>,
    pub locked_at: Option<chrono::NaiveDateTime>,
    pub lock_expires_at: Option<chrono::NaiveDateTime>,
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

/// One page of a company's background tasks: which ones, in what order, and how far in.
///
/// Both the classic tasks page and the `/ui` Tasks workspace page the same list, so the clamping
/// and the offset arithmetic live here rather than being re-derived in either adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskFilter {
    pub channel_id: Option<Uuid>,
    pub status: Option<TaskStatus>,
    /// Oldest first when set; the list is newest first otherwise.
    pub sort_asc: bool,
    page: usize,
    limit: usize,
}

impl TaskFilter {
    pub const DEFAULT_PAGE_SIZE: usize = 50;
    pub const MAX_PAGE_SIZE: usize = 100;

    /// Builds a filter from what a request asked for, clamping the paging to what the list will
    /// actually serve.
    pub fn new(
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
        page: Option<usize>,
        limit: Option<usize>,
    ) -> Self {
        Self {
            channel_id,
            status,
            sort_asc,
            page: page.unwrap_or(1).max(1),
            limit: limit
                .unwrap_or(Self::DEFAULT_PAGE_SIZE)
                .clamp(1, Self::MAX_PAGE_SIZE),
        }
    }

    pub fn page(&self) -> usize {
        self.page
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// How many tasks this page skips, saturating rather than wrapping on an absurd `?page=`.
    pub fn offset(&self) -> i64 {
        self.page
            .saturating_sub(1)
            .saturating_mul(self.limit)
            .min(i64::MAX as usize) as i64
    }

    /// One row more than the page needs, so whether a next page exists comes out of the same
    /// query instead of a second count.
    pub fn probe_limit(&self) -> i64 {
        self.limit.saturating_add(1) as i64
    }

    /// Splits a [`Self::probe_limit`]-sized read into the page itself and whether one follows it.
    pub fn split_probe(&self, mut tasks: Vec<BackgroundTask>) -> (Vec<BackgroundTask>, bool) {
        let has_next = tasks.len() > self.limit;
        tasks.truncate(self.limit);
        (tasks, has_next)
    }

    pub fn on_page(self, page: usize) -> Self {
        Self {
            page: page.max(1),
            ..self
        }
    }
}
