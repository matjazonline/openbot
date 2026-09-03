use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr};
use uuid::Uuid;

use crate::entities::correlation::CorrelationId;
use crate::entities::message::CanonicalMessageId;
use crate::entities::transport::RecipientRole;

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
    /// Is the task parked waiting on someone outside the queue?
    ///
    /// A suspended task is neither finished nor failed: it set its own status during the run and
    /// keeps it, so the worker must not overwrite it on the way out.
    pub fn is_suspended(self) -> bool {
        matches!(
            self,
            TaskStatus::PendingApproval | TaskStatus::WaitingForThirdPartyReply
        )
    }

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

    /// Whether a run is under way right now, as opposed to parked or finished.
    ///
    /// The column animates only this one: it is the single state that is expected to resolve on its
    /// own, without anybody doing anything.
    pub fn is_running(self) -> bool {
        matches!(self, ThreadActivity::Working)
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

/// Proof that the caller still owns a task's lease, for the exact run it was granted to.
///
/// `worker_id` alone is not proof. A worker whose lease lapsed, whose task was reaped and which
/// then re-claimed the same task, would satisfy a `worker_id = $me` guard from *either* run --
/// so a write from the abandoned run could still land on top of the replacement's work. The
/// generation is minted afresh at each claim, so only the current run matches.
///
/// Required by every write that changes a leased task's state or commits its effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskLeaseRef {
    pub task_id: Uuid,
    pub worker_id: Uuid,
    pub execution_generation: Uuid,
}

impl TaskLeaseRef {
    /// The lease a claim just granted, or `None` if the row is not actually held -- which is the
    /// only honest answer for a row that is not `processing`.
    pub fn of(task: &BackgroundTask) -> Option<Self> {
        Some(Self {
            task_id: task.id,
            worker_id: task.worker_id?,
            execution_generation: task.execution_generation?,
        })
    }
}

/// Which task an approval parks, and on whose authority.
///
/// Two callers park a task and they are not equivalent: the run that owns it parks itself, and a
/// maintenance sweep parks one that is *already* parked. Modelling that as a bare `Option<Uuid>`
/// let the second case's lack of a lease silently license the first, so a superseded run could
/// park work the current run was actively doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskSuspension {
    /// The leased run is parking itself. Fenced on its lease.
    Leased(TaskLeaseRef),
    /// A sweep parking a task that is already suspended and so, by
    /// `background_tasks_lease_check`, holds no lease. It may never park a running task.
    AlreadySuspended { task_id: Uuid },
}

impl TaskSuspension {
    pub fn task_id(self) -> Uuid {
        match self {
            TaskSuspension::Leased(lease) => lease.task_id,
            TaskSuspension::AlreadySuspended { task_id } => task_id,
        }
    }

    /// The lease to fence the write on, or `None` when the caller holds none and may therefore
    /// only act on a task that is already suspended.
    pub fn lease(self) -> Option<TaskLeaseRef> {
        match self {
            TaskSuspension::Leased(lease) => Some(lease),
            TaskSuspension::AlreadySuspended { .. } => None,
        }
    }
}

/// Which numbered run at a task a ledger write is about.
///
/// The number is derived from `retry_count`, so a task re-claimed after its lease lapsed reuses the
/// number of the run that never reported — which is the point: that run produced no result, so its
/// record should be replaced rather than sat beside a duplicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskAttemptRef {
    pub task_id: Uuid,
    pub attempt_number: i32,
    pub execution_generation: Uuid,
}

impl TaskAttemptRef {
    /// The attempt this leased run is about to make, given how many times the task has failed.
    ///
    /// The generation comes from the lease rather than being minted here, so the ledger row and
    /// the fence on every write of this run name the same execution.
    pub fn of(task: &BackgroundTask, lease: TaskLeaseRef) -> Self {
        Self {
            task_id: task.id,
            attempt_number: task.retry_count + 1,
            execution_generation: lease.execution_generation,
        }
    }
}

/// What one run of a task actually did.
///
/// The worker needs these apart to close the row out correctly, and returning them explicitly
/// keeps a provider failure from being mistaken for an answer. Previously a run reported
/// `Result<(), String>` and suspension had to be inferred by re-reading the row afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskExecutionOutcome {
    /// The run finished its work and the task is done. Named for the common case -- an agent
    /// replied -- but it also covers a run that correctly had nothing to do.
    Replied,
    /// An agent parked the task awaiting an approval or an outreach reply. The row keeps the
    /// status the agent gave it and its attempt stays open, because the resume continues the
    /// same numbered run rather than starting another.
    Suspended,
    /// Something transient failed. The attempt is consumed and the task is retried with backoff
    /// until `max_retries`, at which point it dead-letters.
    RetryableFailure(String),
    /// The configured per-agent wall-clock deadline elapsed.
    TimedOut(String),
    /// Process shutdown cancelled an active durable execution.
    Interrupted(String),
    /// The task stopped because this worker no longer owned its lease.
    LeaseLost(String),
    /// The task cannot succeed however often it is retried -- an unparseable payload, a missing
    /// field a retry cannot conjure. It dead-letters now instead of burning every attempt first.
    TerminalFailure(String),
}

/// Bounded operational reason for an execution ending. Detailed provider/database text belongs
/// in the durable attempt error, not in metric labels where it would create unbounded cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStopReason {
    Completed,
    RetryableFailure,
    TerminalFailure,
    TimedOut,
    Shutdown,
    LeaseLost,
}

impl TaskStopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::RetryableFailure => "retryable_failure",
            Self::TerminalFailure => "terminal_failure",
            Self::TimedOut => "timed_out",
            Self::Shutdown => "shutdown",
            Self::LeaseLost => "lease_lost",
        }
    }
}

impl std::fmt::Display for TaskStopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskStopReason {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "completed" => Ok(Self::Completed),
            "retryable_failure" => Ok(Self::RetryableFailure),
            "terminal_failure" => Ok(Self::TerminalFailure),
            "timed_out" => Ok(Self::TimedOut),
            "shutdown" => Ok(Self::Shutdown),
            "lease_lost" => Ok(Self::LeaseLost),
            other => Err(format!("Unknown task stop reason: {other}")),
        }
    }
}

impl TaskExecutionOutcome {
    /// The message to record against the attempt and the task row, if this run failed.
    pub fn failure_message(&self) -> Option<&str> {
        match self {
            TaskExecutionOutcome::RetryableFailure(message)
            | TaskExecutionOutcome::TimedOut(message)
            | TaskExecutionOutcome::Interrupted(message)
            | TaskExecutionOutcome::LeaseLost(message)
            | TaskExecutionOutcome::TerminalFailure(message) => Some(message),
            TaskExecutionOutcome::Replied | TaskExecutionOutcome::Suspended => None,
        }
    }
}

/// How an attempt ended. Only terminal states — an attempt still running is written as
/// `processing` when it begins and is never described by this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAttemptStatus {
    Completed,
    Failed,
}

/// The state of an attempt as read from the durable execution ledger.
///
/// Unlike [`TaskAttemptStatus`], this includes the open `processing` state because the Tasks UI
/// reads an attempt while it may still be running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAttemptRecordStatus {
    Processing,
    Completed,
    Failed,
}

impl FromStr for TaskAttemptRecordStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "processing" => Ok(Self::Processing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            other => Err(format!("Unknown task attempt status: {other}")),
        }
    }
}

/// One execution recorded for a task, including failed runs that the task payload no longer
/// represents after a retry succeeds.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskAttemptRecord {
    pub attempt_number: i32,
    pub status: TaskAttemptRecordStatus,
    pub error: Option<String>,
    pub stop_reason: Option<TaskStopReason>,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub result: Option<serde_json::Value>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub execution_generation: Uuid,
}

impl TaskAttemptRecord {
    pub fn total_tokens(&self) -> Option<i64> {
        match (self.prompt_tokens, self.completion_tokens) {
            (None, None) => None,
            (prompt, completion) => {
                Some(i64::from(prompt.unwrap_or(0)) + i64::from(completion.unwrap_or(0)))
            }
        }
    }

    pub fn duration_ms(&self) -> Option<i64> {
        self.finished_at
            .map(|finished_at| (finished_at - self.started_at).num_milliseconds().max(0))
    }
}

impl TaskAttemptStatus {
    /// The value stored in `task_attempts.status`, which a CHECK constraint restricts.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskAttemptStatus::Completed => "completed",
            TaskAttemptStatus::Failed => "failed",
        }
    }
}

/// What one finished attempt cost and how it ended.
///
/// One value rather than five parameters: `error` and the two token counts are all optional, and
/// positionally they would be trivially swappable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAttemptOutcome {
    pub attempt: TaskAttemptRef,
    pub status: TaskAttemptStatus,
    pub stop_reason: TaskStopReason,
    pub error: Option<String>,
    /// `None` when the run never reached a model — a guard rejected it, or it failed first.
    pub tokens: Option<TokenUsage>,
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

/// Everything needed to enqueue one task.
///
/// One value rather than six positional parameters: `company_id`, `channel_id` and the optional
/// `thread_id` are all bare `Uuid`s that the compiler would happily let a caller swap, and
/// `correlation_id` joining them made that worse rather than better.
/// What caused a task, when the cause is something that can be delivered twice.
///
/// This is the task queue's idempotency key. A provider redelivering a message, or a second
/// scheduler waking for the same slot, must find the task the first delivery created rather than
/// start a second run of the same work -- so the cause is stated as a typed value the queue can put
/// a unique constraint on, not fished out of the payload JSON by a string pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskSource {
    /// Nothing repeatable caused it: an approval resuming, an outreach starting, a test.
    #[default]
    Unattributed,
    /// A canonical message. Every delivery of that message resolves to this one task.
    Message(CanonicalMessageId),
    /// A schedule slot coming due.
    ScheduleRun(Uuid),
}

impl TaskSource {
    pub fn message_id(self) -> Option<CanonicalMessageId> {
        match self {
            Self::Message(id) => Some(id),
            Self::Unattributed | Self::ScheduleRun(_) => None,
        }
    }

    pub fn schedule_run_id(self) -> Option<Uuid> {
        match self {
            Self::ScheduleRun(id) => Some(id),
            Self::Unattributed | Self::Message(_) => None,
        }
    }
}

/// One channel-and-thread pair a task's run drives, in pipeline order.
///
/// Stated by whoever enqueues the task. The previous shape re-read them out of the payload JSON,
/// which meant the queue understood the *shape* of one producer's payload and quietly enqueued a
/// single-channel run for anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskTarget {
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub recipient_role: RecipientRole,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewTask {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub task_type: String,
    pub payload: serde_json::Value,
    /// Every channel this run answers on. Empty means the task's own channel and thread, which is
    /// what a schedule or an approval resume drives.
    pub targets: Vec<TaskTarget>,
    /// What this task is the work for, when redelivering that thing must not run it twice.
    pub source: TaskSource,
    /// The chain this task belongs to.
    ///
    /// A task enqueued *by* a running task must pass that task's id through, so an outreach into
    /// another channel, an approval resume, and a schedule's next occurrence all stay one trail.
    /// [`CorrelationId::new`] belongs at ingress, not here.
    pub correlation_id: CorrelationId,
}

impl NewTask {
    /// A task that begins a chain of its own, for the few callers that legitimately start one: a
    /// schedule firing, and tests.
    ///
    /// Anything reacting to work already under way must use [`NewTask::caused_by`] or set
    /// `correlation_id` explicitly instead -- minting here would silently split one trail in two,
    /// which is the failure the whole mechanism exists to prevent.
    pub fn starting_new_chain(
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        task_type: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            company_id,
            channel_id,
            thread_id,
            task_type: task_type.into(),
            payload,
            targets: Vec::new(),
            source: TaskSource::Unattributed,
            correlation_id: CorrelationId::new(),
        }
    }

    /// A task caused by work already under way, inheriting its chain.
    pub fn caused_by(
        parent: &BackgroundTask,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        task_type: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            company_id: parent.company_id,
            channel_id,
            thread_id,
            task_type: task_type.into(),
            payload,
            targets: Vec::new(),
            source: TaskSource::Unattributed,
            correlation_id: parent.correlation_id,
        }
    }

    /// Name what this task is the work for, so a redelivery of that cause finds this task.
    pub fn caused_by_source(mut self, source: TaskSource) -> Self {
        self.source = source;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackgroundTask {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    /// The inbound event this task descends from. Inherited, never minted here -- see
    /// [`CorrelationId`] and [`NewTask::correlation_id`].
    pub correlation_id: CorrelationId,
    pub task_type: String,
    pub status: TaskStatus,
    pub payload: serde_json::Value,
    pub retry_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub worker_id: Option<Uuid>,
    /// Set only while the row is `processing`. Minted at claim time, it is what a write from a
    /// superseded run fails to match -- see [`TaskLeaseRef`].
    pub execution_generation: Option<Uuid>,
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

/// Why a task moved between two durable queue states.
///
/// Detailed provider and message text deliberately does not belong here; the event ledger is an
/// operational index into the existing attempt/approval/outreach records, not a second payload
/// store.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskTransitionReason {
    Enqueued,
    Claimed,
    Completed,
    RetryableFailure,
    TerminalFailure,
    TimedOut,
    Shutdown,
    LeaseLost,
    ApprovalRequested,
    ApprovalAccepted,
    ApprovalRejected,
    OutreachStarted,
    OutreachReplyReceived,
    OutreachTimedOut,
    OutreachExtended,
    OperatorStopped,
    OperatorResumed,
    /// The transition happened, but nothing on the write said why.
    ///
    /// Every caller that knows its cause states it, so this reason means a status changed through
    /// a path that does not -- which is a gap in the write contract, not a kind of failure. It is
    /// recorded rather than guessed so the gap can be found:
    ///
    /// ```sql
    /// SELECT from_status, to_status, count(*) FROM task_status_events
    ///  WHERE reason = 'unknown' GROUP BY 1, 2;
    /// ```
    Unknown,
}

impl TaskTransitionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enqueued => "enqueued",
            Self::Claimed => "claimed",
            Self::Completed => "completed",
            Self::RetryableFailure => "retryable_failure",
            Self::TerminalFailure => "terminal_failure",
            Self::TimedOut => "timed_out",
            Self::Shutdown => "shutdown",
            Self::LeaseLost => "lease_lost",
            Self::ApprovalRequested => "approval_requested",
            Self::ApprovalAccepted => "approval_accepted",
            Self::ApprovalRejected => "approval_rejected",
            Self::OutreachStarted => "outreach_started",
            Self::OutreachReplyReceived => "outreach_reply_received",
            Self::OutreachTimedOut => "outreach_timed_out",
            Self::OutreachExtended => "outreach_extended",
            Self::OperatorStopped => "operator_stopped",
            Self::OperatorResumed => "operator_resumed",
            Self::Unknown => "unknown",
        }
    }
}

impl FromStr for TaskTransitionReason {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "enqueued" => Ok(Self::Enqueued),
            "claimed" => Ok(Self::Claimed),
            "completed" => Ok(Self::Completed),
            "retryable_failure" => Ok(Self::RetryableFailure),
            "terminal_failure" => Ok(Self::TerminalFailure),
            "timed_out" => Ok(Self::TimedOut),
            "shutdown" => Ok(Self::Shutdown),
            "lease_lost" => Ok(Self::LeaseLost),
            "approval_requested" => Ok(Self::ApprovalRequested),
            "approval_accepted" => Ok(Self::ApprovalAccepted),
            "approval_rejected" => Ok(Self::ApprovalRejected),
            "outreach_started" => Ok(Self::OutreachStarted),
            "outreach_reply_received" => Ok(Self::OutreachReplyReceived),
            "outreach_timed_out" => Ok(Self::OutreachTimedOut),
            "outreach_extended" => Ok(Self::OutreachExtended),
            "operator_stopped" => Ok(Self::OperatorStopped),
            "operator_resumed" => Ok(Self::OperatorResumed),
            "unknown" => Ok(Self::Unknown),
            other => Err(format!("Unknown task transition reason: {other}")),
        }
    }
}

impl std::fmt::Display for TaskTransitionReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskTransitionActorKind {
    System,
    Worker,
    Operator,
    Approval,
    Outreach,
}

impl TaskTransitionActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Worker => "worker",
            Self::Operator => "operator",
            Self::Approval => "approval",
            Self::Outreach => "outreach",
        }
    }
}

impl FromStr for TaskTransitionActorKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "system" => Ok(Self::System),
            "worker" => Ok(Self::Worker),
            "operator" => Ok(Self::Operator),
            "approval" => Ok(Self::Approval),
            "outreach" => Ok(Self::Outreach),
            other => Err(format!("Unknown task transition actor kind: {other}")),
        }
    }
}

/// Who is stopping a task, and therefore why. A stop is never anonymous: the two callers that can
/// order one are an operator pressing the button and a rejected approval, and the ledger has to
/// tell them apart afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopActor {
    Operator(Uuid),
    Approval(Uuid),
}

impl StopActor {
    pub fn reason(self) -> TaskTransitionReason {
        match self {
            Self::Operator(_) => TaskTransitionReason::OperatorStopped,
            Self::Approval(_) => TaskTransitionReason::ApprovalRejected,
        }
    }

    pub fn transition_actor(self) -> TransitionActor {
        match self {
            Self::Operator(id) => TransitionActor::Operator(id),
            Self::Approval(id) => TransitionActor::Approval(id),
        }
    }
}

/// Who is resuming a task. An operator retrying failed work and an approval releasing a parked
/// task reach the same status, so only the actor separates "someone decided to try again" from
/// "the thing it was waiting for arrived".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeActor {
    Operator(Uuid),
    Approval(Uuid),
}

impl ResumeActor {
    pub fn reason(self) -> TaskTransitionReason {
        match self {
            Self::Operator(_) => TaskTransitionReason::OperatorResumed,
            Self::Approval(_) => TaskTransitionReason::ApprovalAccepted,
        }
    }

    pub fn transition_actor(self) -> TransitionActor {
        match self {
            Self::Operator(id) => TransitionActor::Operator(id),
            Self::Approval(id) => TransitionActor::Approval(id),
        }
    }
}

/// Every actor and related-source shape a status transition may carry, as one value.
///
/// The database stores this split across an actor kind, an actor id, and two nullable source ids,
/// with a CHECK constraint tying them together. Keeping the valid combinations in a single enum is
/// what makes that constraint unreachable from Rust: there is no way to name an actor kind without
/// also naming the id it requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionActor {
    System,
    Worker(Uuid),
    Operator(Uuid),
    Approval(Uuid),
    Outreach(Uuid),
}

impl TransitionActor {
    pub fn kind(self) -> TaskTransitionActorKind {
        match self {
            Self::System => TaskTransitionActorKind::System,
            Self::Worker(_) => TaskTransitionActorKind::Worker,
            Self::Operator(_) => TaskTransitionActorKind::Operator,
            Self::Approval(_) => TaskTransitionActorKind::Approval,
            Self::Outreach(_) => TaskTransitionActorKind::Outreach,
        }
    }

    /// The id of the actor itself, which only a worker or an operator has. An approval or an
    /// outreach is a *source*: it is identified by its row, reported below.
    pub fn actor_id(self) -> Option<Uuid> {
        match self {
            Self::Worker(id) | Self::Operator(id) => Some(id),
            Self::System | Self::Approval(_) | Self::Outreach(_) => None,
        }
    }

    pub fn approval_id(self) -> Option<Uuid> {
        match self {
            Self::Approval(id) => Some(id),
            Self::System | Self::Worker(_) | Self::Operator(_) | Self::Outreach(_) => None,
        }
    }

    pub fn outreach_id(self) -> Option<Uuid> {
        match self {
            Self::Outreach(id) => Some(id),
            Self::System | Self::Worker(_) | Self::Operator(_) | Self::Approval(_) => None,
        }
    }
}

/// Which side of the retry budget a failure lands on. A bool here reads as `is_dead_letter: false`
/// at the call site and says nothing about why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFailureOutcome {
    Retry,
    DeadLetter,
}

impl TaskFailureOutcome {
    pub fn status(self) -> TaskStatus {
        match self {
            Self::Retry => TaskStatus::Pending,
            Self::DeadLetter => TaskStatus::DeadLetter,
        }
    }
}

/// One fenced failure write: which run failed, what it said, when it may run again, and why it
/// stopped. The lease carries the worker, so a failure cannot be attributed to anyone else.
#[derive(Debug, Clone, Copy)]
pub struct TaskFailure<'a> {
    pub lease: TaskLeaseRef,
    pub error: &'a str,
    pub next_run_at: DateTime<Utc>,
    pub outcome: TaskFailureOutcome,
    pub reason: TaskStopReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatusEvent {
    pub id: Uuid,
    pub company_id: Uuid,
    pub task_id: Uuid,
    pub correlation_id: CorrelationId,
    pub sequence: i32,
    pub from_status: Option<TaskStatus>,
    pub to_status: TaskStatus,
    pub reason: TaskTransitionReason,
    pub actor_kind: TaskTransitionActorKind,
    pub actor_id: Option<Uuid>,
    pub related_approval_id: Option<Uuid>,
    pub related_outreach_id: Option<Uuid>,
    pub retry_count: i32,
    pub run_at: chrono::DateTime<chrono::Utc>,
    pub execution_generation: Option<Uuid>,
    pub transitioned_at: chrono::DateTime<chrono::Utc>,
}

/// A stable cursor for the globally chronological chain timeline. Task id and sequence preserve
/// task-local ordering when two transitions share one database timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskStatusEventCursor {
    pub transitioned_at: chrono::DateTime<chrono::Utc>,
    pub task_id: Uuid,
    pub sequence: i32,
    pub id: Uuid,
}

impl From<&TaskStatusEvent> for TaskStatusEventCursor {
    fn from(event: &TaskStatusEvent) -> Self {
        Self {
            transitioned_at: event.transitioned_at,
            task_id: event.task_id,
            sequence: event.sequence,
            id: event.id,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ChainStage {
    Queued,
    Running,
    WaitingApproval,
    WaitingReply,
    Completed,
    NeedsAttention,
}

impl ChainStage {
    pub const ALL: [Self; 6] = [
        Self::Queued,
        Self::Running,
        Self::WaitingApproval,
        Self::WaitingReply,
        Self::Completed,
        Self::NeedsAttention,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::WaitingReply => "waiting_reply",
            Self::Completed => "completed",
            Self::NeedsAttention => "needs_attention",
        }
    }

    /// The Rust representation of the board's stage precedence.
    ///
    /// The board query derives the same rule in SQL, because `stage` has to be a column there
    /// for the window functions to partition on. Neither copy is generated from the other;
    /// `chain_stage_sql_matches_rust_derivation` is the equivalence test that keeps them
    /// identical.
    pub fn derive(counts: &TaskChainCounts) -> Self {
        if counts.failed > 0
            || counts.dead_letter > 0
            || counts.stopped > 0
            || counts.expired_processing > 0
            || counts.delivery_failed > 0
        {
            Self::NeedsAttention
        } else if counts.pending_approval > 0 {
            Self::WaitingApproval
        } else if counts.processing > 0 || counts.delivery_sending > 0 {
            Self::Running
        } else if counts.waiting_reply > 0 {
            Self::WaitingReply
        } else if counts.pending > 0 || counts.delivery_pending > 0 {
            Self::Queued
        } else if counts.total_tasks > 0
            && counts.completed == counts.total_tasks
            && counts.delivery_sent == counts.total_deliveries
        {
            Self::Completed
        } else {
            Self::NeedsAttention
        }
    }
}

impl FromStr for ChainStage {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "waiting_approval" => Ok(Self::WaitingApproval),
            "waiting_reply" => Ok(Self::WaitingReply),
            "completed" => Ok(Self::Completed),
            "needs_attention" => Ok(Self::NeedsAttention),
            other => Err(format!("Unknown task chain stage: {other}")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskChainCounts {
    pub total_tasks: i64,
    pub pending: i64,
    pub processing: i64,
    pub expired_processing: i64,
    pub pending_approval: i64,
    pub waiting_reply: i64,
    pub completed: i64,
    pub failed: i64,
    pub dead_letter: i64,
    pub stopped: i64,
    pub total_deliveries: i64,
    pub delivery_pending: i64,
    pub delivery_sending: i64,
    pub delivery_sent: i64,
    pub delivery_failed: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskChainCard {
    pub correlation_id: CorrelationId,
    pub stage: ChainStage,
    pub title: String,
    pub channel_names: Vec<String>,
    pub agent_names: Vec<String>,
    pub counts: TaskChainCounts,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activity_at: chrono::DateTime<chrono::Utc>,
    pub next_action_at: Option<chrono::DateTime<chrono::Utc>>,
    pub retry_count: i64,
    pub failure_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskChainBoard {
    pub cards: HashMap<ChainStage, Vec<TaskChainCard>>,
    pub totals: HashMap<ChainStage, i64>,
    pub per_column_limit: usize,
}

impl TaskChainBoard {
    pub fn cards(&self, stage: ChainStage) -> &[TaskChainCard] {
        self.cards.get(&stage).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn total(&self, stage: ChainStage) -> i64 {
        self.totals.get(&stage).copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskBoardFilter {
    pub channel_id: Option<Uuid>,
    pub terminal_since: chrono::DateTime<chrono::Utc>,
    pub per_column_limit: usize,
}

impl TaskBoardFilter {
    pub const DEFAULT_PER_COLUMN: usize = 50;

    pub fn new(channel_id: Option<Uuid>, now: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            channel_id,
            terminal_since: now - chrono::Duration::days(7),
            per_column_limit: Self::DEFAULT_PER_COLUMN,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskChainTaskDetail {
    pub task: BackgroundTask,
    pub attempts: Vec<TaskAttemptRecord>,
    pub deliveries: Vec<crate::entities::outbox::OutboxEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskApprovalContext {
    pub id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub action_title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskOutreachContext {
    pub id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub required_threshold_percent: f64,
    pub target_count: i64,
    pub response_count: i64,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskChainDetail {
    pub company_id: Uuid,
    pub correlation_id: CorrelationId,
    pub title: String,
    pub channel_names: Vec<String>,
    pub agent_names: Vec<String>,
    pub tasks: Vec<TaskChainTaskDetail>,
    pub events: Vec<TaskStatusEvent>,
    pub approvals: Vec<TaskApprovalContext>,
    pub outreaches: Vec<TaskOutreachContext>,
    /// Set when any of the pane's bounded reads had more rows to give.
    ///
    /// The pane says so rather than drawing a partial timeline that reads as a complete one — a
    /// chain that silently stops halfway is worse than one that admits where it was cut.
    pub truncated: bool,
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
            // Every state a thread can be in says something in the column.
            assert!(!activity.label().is_empty());
        }
    }

    #[test]
    fn every_individual_chain_state_maps_to_its_operational_stage() {
        let cases = [
            (
                TaskChainCounts {
                    total_tasks: 1,
                    pending: 1,
                    ..Default::default()
                },
                ChainStage::Queued,
            ),
            (
                TaskChainCounts {
                    total_tasks: 1,
                    processing: 1,
                    ..Default::default()
                },
                ChainStage::Running,
            ),
            (
                TaskChainCounts {
                    total_tasks: 1,
                    pending_approval: 1,
                    ..Default::default()
                },
                ChainStage::WaitingApproval,
            ),
            (
                TaskChainCounts {
                    total_tasks: 1,
                    waiting_reply: 1,
                    ..Default::default()
                },
                ChainStage::WaitingReply,
            ),
            (
                TaskChainCounts {
                    total_tasks: 1,
                    completed: 1,
                    total_deliveries: 1,
                    delivery_sent: 1,
                    ..Default::default()
                },
                ChainStage::Completed,
            ),
        ];
        for (counts, expected) in cases {
            assert_eq!(ChainStage::derive(&counts), expected);
        }
    }

    #[test]
    fn chain_stage_precedence_surfaces_mixed_work_and_delivery_failures() {
        let counts = TaskChainCounts {
            total_tasks: 4,
            pending: 1,
            processing: 1,
            pending_approval: 1,
            waiting_reply: 1,
            ..Default::default()
        };
        assert_eq!(ChainStage::derive(&counts), ChainStage::WaitingApproval);

        let attention = TaskChainCounts {
            delivery_failed: 1,
            ..counts.clone()
        };
        assert_eq!(ChainStage::derive(&attention), ChainStage::NeedsAttention);

        let expired = TaskChainCounts {
            expired_processing: 1,
            ..counts
        };
        assert_eq!(ChainStage::derive(&expired), ChainStage::NeedsAttention);
    }

    #[test]
    fn completed_chains_require_every_task_and_delivery_to_succeed() {
        let incomplete_delivery = TaskChainCounts {
            total_tasks: 2,
            completed: 2,
            total_deliveries: 1,
            delivery_pending: 1,
            ..Default::default()
        };
        assert_eq!(ChainStage::derive(&incomplete_delivery), ChainStage::Queued);

        let stopped = TaskChainCounts {
            total_tasks: 1,
            stopped: 1,
            ..Default::default()
        };
        assert_eq!(ChainStage::derive(&stopped), ChainStage::NeedsAttention);
    }

    #[test]
    fn a_transition_actor_carries_exactly_the_one_id_its_kind_requires() {
        let worker = Uuid::new_v4();
        let operator = Uuid::new_v4();
        let approval = Uuid::new_v4();
        let outreach = Uuid::new_v4();

        let cases = [
            (
                TransitionActor::System,
                TaskTransitionActorKind::System,
                None,
                None,
                None,
            ),
            (
                TransitionActor::Worker(worker),
                TaskTransitionActorKind::Worker,
                Some(worker),
                None,
                None,
            ),
            (
                TransitionActor::Operator(operator),
                TaskTransitionActorKind::Operator,
                Some(operator),
                None,
                None,
            ),
            (
                TransitionActor::Approval(approval),
                TaskTransitionActorKind::Approval,
                None,
                Some(approval),
                None,
            ),
            (
                TransitionActor::Outreach(outreach),
                TaskTransitionActorKind::Outreach,
                None,
                None,
                Some(outreach),
            ),
        ];

        for (actor, kind, actor_id, approval_id, outreach_id) in cases {
            assert_eq!(actor.kind(), kind, "{actor:?}");
            assert_eq!(actor.actor_id(), actor_id, "{actor:?}");
            assert_eq!(actor.approval_id(), approval_id, "{actor:?}");
            assert_eq!(actor.outreach_id(), outreach_id, "{actor:?}");
        }
    }

    #[test]
    fn stop_and_resume_causes_name_their_own_reason_and_actor() {
        let operator = Uuid::new_v4();
        let approval = Uuid::new_v4();

        assert_eq!(
            StopActor::Operator(operator).reason(),
            TaskTransitionReason::OperatorStopped
        );
        assert_eq!(
            StopActor::Operator(operator).transition_actor(),
            TransitionActor::Operator(operator)
        );
        // A rejection is an approval acting, not an operator: the ledger's actor kind has to agree
        // with its reason, and `operator_stopped` here would contradict it.
        assert_eq!(
            StopActor::Approval(approval).reason(),
            TaskTransitionReason::ApprovalRejected
        );
        assert_eq!(
            StopActor::Approval(approval).transition_actor(),
            TransitionActor::Approval(approval)
        );

        assert_eq!(
            ResumeActor::Operator(operator).reason(),
            TaskTransitionReason::OperatorResumed
        );
        assert_eq!(
            ResumeActor::Operator(operator).transition_actor(),
            TransitionActor::Operator(operator)
        );
        assert_eq!(
            ResumeActor::Approval(approval).reason(),
            TaskTransitionReason::ApprovalAccepted
        );
        assert_eq!(
            ResumeActor::Approval(approval).transition_actor(),
            TransitionActor::Approval(approval)
        );
    }

    #[test]
    fn a_failure_outcome_names_the_status_it_lands_on() {
        assert_eq!(TaskFailureOutcome::Retry.status(), TaskStatus::Pending);
        assert_eq!(
            TaskFailureOutcome::DeadLetter.status(),
            TaskStatus::DeadLetter
        );
    }
}
