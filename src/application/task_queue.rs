//! The task queue contract, owned by the application worker that consumes it.
//!
//! PostgreSQL implements this port in the outer persistence layer; in-memory doubles implement
//! it beside their consumers. Keeping the trait here prevents queue users from depending on the
//! database adapter merely to name the operation they need.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        message::CanonicalMessageId,
        outreach::{DueOutreach, OutreachProgress, OutreachReplyMatch},
        runtime_metrics::MachineIdentity,
        stuck_work::{StuckWorkCensus, StuckWorkThresholds},
        task::{
            BackgroundTask, NewTask, ResumeActor, StopActor, TaskAttemptOutcome, TaskAttemptRecord,
            TaskAttemptRef, TaskBoardFilter, TaskChainBoard, TaskChainDetail, TaskFailure,
            TaskLeaseRef, TaskStatus, TaskStatusEvent, TaskStatusEventCursor, ThreadActivity,
        },
        transport::DeliveryId,
        value_objects::{EmailAddress, MessageId},
    },
    transport::{DeliveryCreation, NewDelivery},
    use_cases::thread::{AgentReply, MessageWrite, TaskChannelTarget},
};

pub const TASK_LEASE_SECONDS: i64 = 15 * 60;
/// Log when a lease-guarded state change was ignored because the lease or status moved on.
///
/// Both queues need this, and so does the inline dispatch path: a status write that quietly does
/// nothing leaves its row claimed, and a claimed row that never reports a result is redelivered —
/// or re-executed — on every lease expiry after that.
pub fn report_outcome(subject: &str, id: Uuid, change: &str, outcome: AppResult<bool>) {
    match outcome {
        Ok(true) => {}
        Ok(false) => {
            warn!("{subject} {id} {change} ignored because its lease or status changed")
        }
        Err(err) => error!("Failed to record {subject} {id} {change}: {err}"),
    }
}

/// A claim on a task: who holds it, and for how long at a time. Holding the *duration* rather than
/// a computed deadline is what lets a lease extend itself — a precomputed `expires_at` is only ever
/// right at the instant it was made, and every renewal after that has to guess the interval again.
#[derive(Clone, Copy, Debug)]
pub struct TaskLease {
    /// Which task, held by which worker, for which run.
    pub reference: TaskLeaseRef,
    ttl: chrono::Duration,
}

impl TaskLease {
    /// A worker's lease on a queued task: long, because the worker heartbeats it.
    pub fn worker(reference: TaskLeaseRef) -> Self {
        Self::new(reference, TASK_LEASE_SECONDS)
    }

    fn new(reference: TaskLeaseRef, ttl_seconds: i64) -> Self {
        Self {
            reference,
            ttl: chrono::Duration::seconds(ttl_seconds),
        }
    }

    /// The deadline a claim or renewal made *now* should carry.
    pub fn expires_at(&self) -> DateTime<Utc> {
        Utc::now() + self.ttl
    }

    /// How often to renew: a third of the term, so two beats can be missed before it lapses.
    pub fn heartbeat(&self) -> Duration {
        Duration::from_secs((self.ttl.num_seconds() / 3).max(1) as u64)
    }
}

/// What became of work run under a lease.
pub enum Leased<T> {
    Finished(T),
    /// The lease could not be renewed — the task was stopped, another worker took it over, or the
    /// database is unreachable. Whatever this run was doing, it is no longer the run of record and
    /// must not report a result.
    Lost,
}

/// Run `work` while keeping `lease` alive, renewing on every [`TaskLease::heartbeat`].
///
/// The worker holds a lease across a task it polled for, and cannot assume that task finishes
/// inside a single lease term — a lapsed lease means a second run of the same agent.
pub async fn while_leased<F: Future>(
    persistence: &dyn TaskPersistence,
    lease: &TaskLease,
    work: F,
) -> Leased<F::Output> {
    let task_id = lease.reference.task_id;
    tokio::pin!(work);
    let mut heartbeat = tokio::time::interval(lease.heartbeat());
    // The first tick completes immediately; spend it here rather than renewing a lease just taken.
    heartbeat.tick().await;

    loop {
        tokio::select! {
            output = &mut work => return Leased::Finished(output),
            _ = heartbeat.tick() => {
                match persistence
                    .renew_task_lease(lease.reference, lease.expires_at())
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => return Leased::Lost,
                    Err(error) => {
                        error!("Failed to renew the lease on task {task_id}: {error}");
                        return Leased::Lost;
                    }
                }
            }
        }
    }
}
/// One recipient of an outreach: the question it was asked, and the mail that carries it.
///
/// Both are built by the caller and written by the same transaction that creates the outreach, so
/// a target row can never exist without the message it was asked with or the delivery that sends
/// it. They live here rather than in `entities::outreach` because a `MessageWrite` and a
/// `NewDelivery` are application vocabulary, and the domain may not reach upward for them.
#[derive(Debug, Clone)]
pub struct OutreachTargetRequest {
    pub email: EmailAddress,
    /// The question, as a canonical message in the task's thread. `request_message_id` on the
    /// target row points at it, which is how the reply guard tells the agent asking a third party
    /// something from the agent answering this turn.
    pub request: MessageWrite,
    pub delivery: NewDelivery,
}

/// One outreach to create, with every target already composed.
#[derive(Debug, Clone)]
pub struct CreateOutreachRequest {
    pub id: Uuid,
    pub task_id: Uuid,
    pub company_id: Uuid,
    /// The channel every target is asked as.
    pub channel_id: Uuid,
    /// The chain the outreaching task belongs to, so the mail this sends and the replies it
    /// provokes stay on the same trail as the run that asked for them.
    pub correlation_id: CorrelationId,
    pub worker_id: Uuid,
    pub outreach_key: String,
    pub required_threshold_percent: f64,
    pub expires_at: DateTime<Utc>,
    pub subject: String,
    pub body: String,
    pub targets: Vec<OutreachTargetRequest>,
}

/// Everything one agent dispatch makes durable, so it can land as a single transaction.
///
/// The reply message in each answered thread, the delivery that carries it, and the audit payload
/// on the task are one result. Committed separately, a crash or a lost lease between them leaves
/// the thread showing an answer that was never sent, or a delivery going out for a task whose
/// payload says it never ran -- and the retry then has to reconcile the difference.
pub struct AgentDispatchCommit<'a> {
    /// Proof this run still owns the task. The whole transaction is fenced on it.
    pub lease: TaskLeaseRef,
    /// The reply: one canonical message, plus the further threads it also answered.
    pub reply: &'a AgentReply,
    /// The deliveries the reply is owed, already rendered. Empty for a simulated run, which
    /// stores its answer and sends nothing.
    pub deliveries: Vec<NewDelivery>,
    /// The run's audit payload, written back onto the task.
    pub payload: Value,
    /// Whether this dispatch also closes the task's outreach.
    pub complete_outreach: bool,
}

/// What [`TaskPersistence::commit_agent_dispatch`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchCommit {
    /// Everything landed, and these are the deliveries the reply will go out through -- including
    /// any that were absorbed onto a delivery an earlier run of this task had already queued,
    /// which is the idempotency key doing its job rather than a failure.
    Committed { deliveries: Vec<DeliveryCreation> },
    /// This run no longer owns the task, so nothing was written at all.
    LeaseLost,
}
#[async_trait]
pub trait TaskPersistence: Send + Sync {
    /// What each of these threads is currently doing, for the mailbox's activity indicators.
    ///
    /// Batched deliberately: the thread column renders up to a full page of rows at once, and one
    /// query per row would be a page-load's worth of round trips. Threads with nothing in flight
    /// are simply absent from the map.
    async fn list_thread_activity(
        &self,
        _thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadActivity>> {
        Ok(HashMap::new())
    }

    async fn create_outreach_and_pause(
        &self,
        _request: CreateOutreachRequest,
    ) -> AppResult<OutreachProgress> {
        Err(AppError::Internal(
            "Outreach persistence is not configured".into(),
        ))
    }

    async fn find_correlated_outreach_reply(
        &self,
        _company_id: Uuid,
        _channel_id: Uuid,
        _thread_id: Uuid,
        _sender: &str,
        _references: &[MessageId],
    ) -> AppResult<Option<OutreachReplyMatch>> {
        Ok(None)
    }

    async fn record_outreach_reply(
        &self,
        _matched: &OutreachReplyMatch,
        _response_association_id: Uuid,
    ) -> AppResult<OutreachProgress> {
        Err(AppError::Internal(
            "Outreach persistence is not configured".into(),
        ))
    }

    async fn list_due_outreaches(
        &self,
        _due_at: DateTime<Utc>,
        _limit: i64,
    ) -> AppResult<Vec<DueOutreach>> {
        Ok(Vec::new())
    }

    async fn mark_outreach_timeout_pending(&self, _outreach_id: Uuid) -> AppResult<bool> {
        Ok(false)
    }

    async fn restore_outreach_waiting(&self, _outreach_id: Uuid) -> AppResult<()> {
        Ok(())
    }

    async fn get_outreach_context(&self, _task_id: Uuid) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn complete_outreach(&self, _task_id: Uuid) -> AppResult<()> {
        Ok(())
    }

    /// Give back every task whose lease lapsed without the run reporting anything, charging each
    /// one an attempt.
    ///
    /// Claims used to steal an expired `processing` row directly, which re-ran it with
    /// `retry_count` untouched, left its open attempt sitting in the ledger and applied no
    /// backoff -- so a task that reliably outlived its lease was retried for ever and never
    /// dead-lettered. Reaping makes lease expiry cost exactly what a reported failure costs.
    ///
    /// Returns how many rows were reaped. Defaulted to a no-op for doubles that never lease.
    async fn reap_expired_task_leases(&self) -> AppResult<u64> {
        Ok(0)
    }

    /// How much work is stuck right now, by kind.
    ///
    /// One query rather than seven: the maintenance sweep runs on a timer against the same pool
    /// the workers claim from, and seven round trips every thirty seconds to answer one question
    /// is a poor trade for a sweep that usually finds nothing.
    ///
    /// The default reports a quiet system, which is the honest answer for a double that stores no
    /// queue at all.
    async fn census_stuck_work(
        &self,
        _thresholds: StuckWorkThresholds,
    ) -> AppResult<StuckWorkCensus> {
        Ok(StuckWorkCensus::default())
    }

    /// Queue one delivery that no other durable write has to land with.
    ///
    /// The narrow case: a scheduled run's mail, whose canonical answer is written by the same
    /// call. Everything else -- an agent reply, an approval, an outreach -- has state that must
    /// become visible with its delivery, and so goes through the purpose-specific method that
    /// commits both.
    ///
    /// The default accepts without recording, so test doubles are unaffected.
    async fn enqueue_delivery(&self, _delivery: NewDelivery) -> AppResult<DeliveryCreation> {
        Err(AppError::Internal(
            "Delivery persistence is not configured".into(),
        ))
    }

    /// The thread an outreach's question was asked in, for the delivery that carried it.
    async fn get_outreach_thread_for_delivery(
        &self,
        _delivery_id: DeliveryId,
    ) -> AppResult<Option<Uuid>> {
        Ok(None)
    }

    /// Store the message an outreach asked with, and mark the target row as having asked it.
    ///
    /// One transaction, because the mark is what tells the reply guard that this outbound message
    /// is the agent asking rather than the agent answering. Written separately, a failure between
    /// them leaves an unmarked outreach mail in the thread -- which the guard would then read as
    /// the answer this turn owed, and complete the task without one.
    ///
    /// Deliberately not defaulted away: a double that accepted it silently would let a test pass
    /// while every outreach request looked like an answer.
    async fn record_outreach_request_message(
        &self,
        delivery_id: DeliveryId,
        write: &MessageWrite,
    ) -> AppResult<CanonicalMessageId>;

    async fn enqueue_task(&self, new_task: NewTask) -> AppResult<BackgroundTask>;

    async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>>;

    /// Every execution attempt for one company-owned task, oldest first.
    ///
    /// The company predicate is part of the query because the task id originates in a browser
    /// route; authorization must scope the object being listed, not merely a preceding lookup.
    async fn list_task_attempts(
        &self,
        _company_id: Uuid,
        _task_id: Uuid,
    ) -> AppResult<Vec<TaskAttemptRecord>> {
        Ok(Vec::new())
    }

    /// The channels one queued run drives, in the order its ingest resolved them.
    ///
    /// No default: a double that answered "no targets" would let a multi-channel dispatch quietly
    /// answer on one channel and never on the others, which reads as a routing bug rather than as
    /// a missing test double.
    async fn list_task_channel_targets(
        &self,
        company_id: Uuid,
        task_id: Uuid,
    ) -> AppResult<Vec<TaskChannelTarget>>;

    async fn update_task_payload(&self, id: Uuid, payload: Value) -> AppResult<()>;

    /// Commit one dispatch's entire visible effect, or none of it.
    ///
    /// No default: a double that silently reported success would let the dispatch believe it had
    /// delivered a reply it never queued.
    async fn commit_agent_dispatch(
        &self,
        commit: AgentDispatchCommit<'_>,
    ) -> AppResult<DispatchCommit>;

    /// Extend this run's lease. `false` means the run no longer owns the task and must stop.
    ///
    /// No default, for the same reason: a double that always renews cannot exercise lease loss,
    /// which is the behaviour every caller of this branches on.
    async fn renew_task_lease(
        &self,
        lease: TaskLeaseRef,
        lock_expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;

    /// Open a `task_attempts` row for a run that is starting, so its duration and token cost have
    /// somewhere to land.
    ///
    /// Defaulted to a no-op for the same reason as [`Self::renew_task_lease`]: the hand-written
    /// mocks across the suite assert on queue transitions, and this ledger is not one.
    async fn begin_task_attempt(
        &self,
        attempt: TaskAttemptRef,
        machine: &MachineIdentity,
    ) -> AppResult<()> {
        let _ = (attempt, machine);
        Ok(())
    }

    /// Close the row opened by [`Self::begin_task_attempt`]. `false` means the row was not still
    /// open — another worker took the task over — and the caller's numbers are not the ones of
    /// record.
    async fn finish_task_attempt(&self, outcome: &TaskAttemptOutcome) -> AppResult<bool> {
        let _ = outcome;
        Ok(true)
    }

    async fn claim_pending_tasks(
        &self,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>>;

    async fn claim_task(
        &self,
        id: Uuid,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;

    async fn mark_task_completed(&self, lease: TaskLeaseRef) -> AppResult<bool>;

    /// The fenced failure transition, carrying why the run stopped and which side of the retry
    /// budget it lands on.
    ///
    /// No default: a double that silently forwards to some other write records a failure the
    /// ledger cannot attribute, and attribution is the only thing this write adds over a status
    /// change anyone could make.
    async fn mark_task_failed(&self, failure: TaskFailure<'_>) -> AppResult<bool>;

    /// Stop a task on someone's authority. No default, and no anonymous variant: an operator and a
    /// rejected approval reach the same status for different reasons, and only the caller knows
    /// which.
    async fn stop_task(&self, id: Uuid, actor: StopActor) -> AppResult<BackgroundTask>;

    /// Resume a task on someone's authority. See [`Self::stop_task`] for why there is no default.
    async fn resume_task(&self, id: Uuid, actor: ResumeActor) -> AppResult<BackgroundTask>;

    /// The six-column correlation-chain read model. Defaults keep narrow test doubles small.
    async fn list_task_chain_board(
        &self,
        _company_id: Uuid,
        filter: TaskBoardFilter,
    ) -> AppResult<TaskChainBoard> {
        Ok(TaskChainBoard {
            cards: HashMap::new(),
            totals: HashMap::new(),
            per_column_limit: filter.per_column_limit,
        })
    }

    async fn get_task_chain_detail(
        &self,
        _company_id: Uuid,
        _correlation_id: CorrelationId,
    ) -> AppResult<Option<TaskChainDetail>> {
        Ok(None)
    }

    async fn list_task_status_events(
        &self,
        _company_id: Uuid,
        _correlation_id: CorrelationId,
        _cursor: Option<TaskStatusEventCursor>,
        _limit: usize,
    ) -> AppResult<Vec<TaskStatusEvent>> {
        Ok(Vec::new())
    }

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>>;

    async fn list_company_tasks_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let tasks = self
            .list_company_tasks(company_id, channel_id, status, sort_asc)
            .await?;
        Ok(tasks
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
            .collect())
    }
}
