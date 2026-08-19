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

/// What a thread is currently doing, derived from its background tasks.
///
/// This is a *view* of task state, not a stored column: the mailbox cares about far fewer
/// distinctions than the worker does, and two of the worker's states do not mean what their names
/// suggest.
///
/// - The worker never writes [`TaskStatus::Failed`]. A transient failure goes back to `Pending`
///   with a backed-off `run_at`; only exhausted retries reach `DeadLetter`. So a failed run shows
///   up here as [`ThreadActivity::Failed`] derived from `DeadLetter`.
/// - [`TaskStatus::Processing`] does not by itself mean running. A row whose `lock_expires_at` has
///   passed is an abandoned lease waiting to be reclaimed, which is [`ThreadActivity::Queued`],
///   not work in progress.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadActivity {
    /// An agent is running for this thread right now.
    Working,
    /// Enqueued, or waiting to be reclaimed after a lost lease -- including retry backoff.
    Queued,
    /// Parked until a human answers an approval link.
    WaitingApproval,
    /// Parked until a third party replies to an outreach.
    WaitingReply,
    /// The run exhausted its retries and was dead-lettered.
    Failed,
}

impl ThreadActivity {
    /// Derive a thread's activity from its most recent unfinished task.
    ///
    /// `lease_expires_at` is that task's `lock_expires_at`; `now` is compared against it rather
    /// than read from the clock so the mapping stays a pure function and can be unit tested.
    pub fn from_task(
        status: TaskStatus,
        lease_expires_at: Option<chrono::DateTime<chrono::Utc>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<Self> {
        match status {
            TaskStatus::Processing => match lease_expires_at {
                Some(expires) if expires > now => Some(ThreadActivity::Working),
                // The worker holding this task died; another will pick it up.
                _ => Some(ThreadActivity::Queued),
            },
            TaskStatus::Pending => Some(ThreadActivity::Queued),
            TaskStatus::PendingApproval => Some(ThreadActivity::WaitingApproval),
            TaskStatus::WaitingForThirdPartyReply => Some(ThreadActivity::WaitingReply),
            TaskStatus::DeadLetter => Some(ThreadActivity::Failed),
            // Nothing to say about a thread whose work is over.
            TaskStatus::Completed | TaskStatus::Stopped | TaskStatus::Failed => None,
        }
    }

    /// The task status this activity stands for, so the mailbox badge takes its colour from the
    /// same table the task monitor uses instead of picking its own.
    pub fn task_status(self) -> TaskStatus {
        match self {
            ThreadActivity::Working => TaskStatus::Processing,
            ThreadActivity::Queued => TaskStatus::Pending,
            ThreadActivity::WaitingApproval => TaskStatus::PendingApproval,
            ThreadActivity::WaitingReply => TaskStatus::WaitingForThirdPartyReply,
            ThreadActivity::Failed => TaskStatus::DeadLetter,
        }
    }

    /// Whether this state is worth marking on a thread the reader does not have open.
    ///
    /// Work in progress is not. A reader scanning the column is not waiting on runs in threads
    /// they have not opened, and the reply landing is the signal that matters there -- an animated
    /// "replying" badge on those rows is noise, and because the mailbox enqueues a durable task
    /// alongside its inline agent run, it usually appears *after* the reply is already visible.
    ///
    /// What earns a mark is a thread that has stopped progressing on its own and needs a person.
    pub fn needs_attention(self) -> bool {
        match self {
            ThreadActivity::Working | ThreadActivity::Queued => false,
            ThreadActivity::WaitingApproval
            | ThreadActivity::WaitingReply
            | ThreadActivity::Failed => true,
        }
    }

    /// A single character standing in for this state in the thread column, where there is room for
    /// a mark but not for a sentence. The full wording rides along as the element's `title`.
    pub fn mark(self) -> &'static str {
        match self {
            // Never rendered in the column -- see `needs_attention` -- but a total match keeps a
            // new variant from silently defaulting to blank.
            ThreadActivity::Working | ThreadActivity::Queued => "",
            ThreadActivity::WaitingApproval => "⏳",
            ThreadActivity::WaitingReply => "✉",
            ThreadActivity::Failed => "⚠",
        }
    }

    /// Wording for the mailbox, where the reader is waiting on a conversation rather than
    /// inspecting a task queue.
    pub fn label(self) -> &'static str {
        match self {
            ThreadActivity::Working => "Agent replying…",
            ThreadActivity::Queued => "Queued",
            ThreadActivity::WaitingApproval => "Waiting for approval",
            ThreadActivity::WaitingReply => "Waiting for reply",
            ThreadActivity::Failed => "Agent run failed",
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
    pub locked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub lock_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub run_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes_from_now: i64) -> chrono::DateTime<chrono::Utc> {
        now() + chrono::Duration::minutes(minutes_from_now)
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-08-19T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    /// The distinction the spinner depends on: `Processing` is only work in progress while its
    /// lease is alive. An expired lease is an abandoned task, and showing it as "replying" would
    /// spin forever on a worker that already died.
    #[test]
    fn processing_is_only_working_while_its_lease_holds() {
        assert_eq!(
            ThreadActivity::from_task(TaskStatus::Processing, Some(at(5)), now()),
            Some(ThreadActivity::Working)
        );
        assert_eq!(
            ThreadActivity::from_task(TaskStatus::Processing, Some(at(-5)), now()),
            Some(ThreadActivity::Queued),
            "an expired lease is waiting to be reclaimed, not running"
        );
        assert_eq!(
            ThreadActivity::from_task(TaskStatus::Processing, None, now()),
            Some(ThreadActivity::Queued),
            "no lease at all is not running either"
        );
    }

    /// The worker re-queues transient failures as `Pending` with a backed-off `run_at` and only
    /// dead-letters once retries are exhausted, so `DeadLetter` -- not `Failed` -- is what a
    /// broken run actually looks like.
    #[test]
    fn a_dead_lettered_run_is_what_surfaces_as_failed() {
        assert_eq!(
            ThreadActivity::from_task(TaskStatus::DeadLetter, None, now()),
            Some(ThreadActivity::Failed)
        );
        assert_eq!(
            ThreadActivity::from_task(TaskStatus::Pending, None, now()),
            Some(ThreadActivity::Queued),
            "a retry in backoff still reads as queued work"
        );
    }

    #[test]
    fn blocked_tasks_are_distinguished_from_running_ones() {
        assert_eq!(
            ThreadActivity::from_task(TaskStatus::PendingApproval, None, now()),
            Some(ThreadActivity::WaitingApproval)
        );
        assert_eq!(
            ThreadActivity::from_task(TaskStatus::WaitingForThirdPartyReply, None, now()),
            Some(ThreadActivity::WaitingReply)
        );
    }

    #[test]
    fn a_finished_thread_shows_nothing() {
        for status in [
            TaskStatus::Completed,
            TaskStatus::Stopped,
            TaskStatus::Failed,
        ] {
            assert_eq!(ThreadActivity::from_task(status, Some(at(5)), now()), None);
        }
    }

    /// Every activity maps back to a real task status, which is where its badge colour comes from.
    #[test]
    fn every_activity_round_trips_to_a_task_status() {
        for activity in [
            ThreadActivity::Working,
            ThreadActivity::Queued,
            ThreadActivity::WaitingApproval,
            ThreadActivity::WaitingReply,
            ThreadActivity::Failed,
        ] {
            assert_eq!(
                ThreadActivity::from_task(activity.task_status(), Some(at(5)), now()),
                Some(activity)
            );
            assert!(!activity.label().is_empty());
            // Only the states that stall get a column mark, and every one of those has a glyph.
            assert_eq!(!activity.mark().is_empty(), activity.needs_attention());
        }
    }
}
