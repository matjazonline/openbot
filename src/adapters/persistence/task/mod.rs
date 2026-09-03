//! The task queue persistence port and its Postgres implementation.
//!
//! This module owns the [`TaskPersistence`] trait and the lease types the worker drives it with.
//! The implementation is split by topic alongside it: [`queue`] for the task lifecycle, [`board`]
//! for the correlation-chain read model, [`outbox`] and [`outreach`] for the delivery and
//! third-party paths, [`rows`] for the stored shapes, and [`operations`] for the trait impl that
//! ties them together.
//!
//! The tests are one sibling `tests.rs`, following `use_cases/thread`: they share fixtures across
//! every topic here, so splitting them per module would mean a shared test-support module before
//! it would mean smaller files.

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
        outbox::{OutboxEntry, OutboxStatus},
        outreach::{CreateOutreachRequest, DueOutreach, OutreachProgress, OutreachReplyMatch},
        stuck_work::{StuckWorkCensus, StuckWorkThresholds},
        task::{
            BackgroundTask, NewTask, ResumeActor, StopActor, TaskAttemptOutcome, TaskAttemptRecord,
            TaskAttemptRef, TaskBoardFilter, TaskChainBoard, TaskChainDetail, TaskFailure,
            TaskLeaseRef, TaskStatus, TaskStatusEvent, TaskStatusEventCursor, ThreadActivity,
        },
        value_objects::MessageId,
    },
    use_cases::thread::MessageWrite,
};

mod board;
mod operations;
mod outbox;
mod outreach;
mod queue;
mod rows;

pub(crate) use board::*;
pub(crate) use outbox::*;
pub(crate) use outreach::*;
pub(crate) use queue::*;
pub(crate) use rows::*;

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
/// One email handed to the transport for delivery.
///
/// A struct rather than four positional parameters, two of which are `Uuid`.
pub struct OutboundSend {
    pub company_id: Uuid,
    /// The channel this email goes out as. Unlike `task_id` it is not a lifecycle join — it is the
    /// dimension the outbox is filtered by.
    pub channel_id: Uuid,
    /// The task whose work produced this email. Carries no lifecycle meaning — it is the join the
    /// task view uses to show delivery state, nothing writes back through it.
    pub task_id: Option<Uuid>,
    /// The chain this email belongs to, inherited from the task that produced it. Unlike
    /// `task_id` it is never cleared, so a delivered email stays attached to its trail even after
    /// the task row is gone.
    pub correlation_id: CorrelationId,
    /// Stable across every retry of the same logical send — that is what makes it a lock, and what
    /// the delivered Message-ID is derived from.
    pub idempotency_key: String,
    /// The `OutboundEmail` for the poller to deliver.
    pub payload: serde_json::Value,
}

/// Everything one agent dispatch makes durable, so it can land as a single transaction.
///
/// The reply message in each answered thread, the outbox row that delivers it, and the audit
/// payload on the task are one result. Committed separately, a crash or a lost lease between
/// them leaves the thread showing an answer that was never sent, or an email going out for a
/// task whose payload says it never ran -- and the retry then has to reconcile the difference.
pub struct AgentDispatchCommit<'a> {
    /// Proof this run still owns the task. The whole transaction is fenced on it.
    pub lease: TaskLeaseRef,
    /// The reply, stored once per thread it answered.
    pub messages: &'a [MessageWrite],
    /// The email to hand to the outbox, or `None` for a simulated run that sends nothing.
    pub outbound: Option<OutboundSend>,
    /// The run's audit payload, written back onto the task.
    pub payload: Value,
    /// Whether this dispatch also closes the task's outreach.
    pub complete_outreach: bool,
}

/// What [`TaskPersistence::commit_agent_dispatch`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchCommit {
    /// Everything landed. `outbox_id` is `None` when an equivalent send was already queued, which
    /// is the idempotency key doing its job rather than a failure.
    Committed { outbox_id: Option<Uuid> },
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
        _response_message_id: Uuid,
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

    async fn claim_outbox_emails(
        &self,
        _worker_id: Uuid,
        _lock_expires_at: DateTime<Utc>,
        _limit: i64,
    ) -> AppResult<Vec<OutboxEmail>> {
        Ok(Vec::new())
    }

    async fn mark_outbox_email_sent(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _provider_message_id: &str,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn mark_outbox_email_failed(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _error: &str,
    ) -> AppResult<bool> {
        Ok(true)
    }

    /// Dead-letter one claimed delivery outright, without spending its remaining attempts.
    ///
    /// For a failure that cannot come out differently next time — a payload that will not
    /// deserialize, say. Retrying those only delays the same verdict by five backoffs.
    async fn mark_outbox_email_dead(
        &self,
        _id: Uuid,
        _worker_id: Uuid,
        _error: &str,
    ) -> AppResult<bool> {
        Ok(true)
    }

    /// End every delivery whose lease has run out, counting each as a spent attempt.
    ///
    /// A claimed row whose worker died — or whose status write never landed — is otherwise stuck
    /// in `sending` with its attempt count untouched, so it is redelivered every lease period and
    /// never reaches the dead-letter cap. Returns how many rows were reaped.
    async fn reap_expired_outbox_leases(&self) -> AppResult<u64> {
        Ok(0)
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

    /// Every delivery this task handed to the transport, oldest first.
    ///
    /// Exists so the task view can show delivery state without the transport writing back into the
    /// task. The default returns nothing, which renders as "no deliveries".
    /// What the transport did with the emails one task produced.
    ///
    /// Read-only, and deliberately so — the task and the transport are separate processes, and
    /// this is the join the UI makes at render time, not a channel either one writes through.
    async fn list_task_deliveries(&self, _task_id: Uuid) -> AppResult<Vec<OutboxEntry>> {
        Ok(Vec::new())
    }

    /// One filtered page of the company's outbox, newest first unless `sort_asc`.
    ///
    /// `limit` is the caller's probe size, so whether a further page exists comes back with the
    /// page itself; see [`crate::entities::outbox::OutboxFilter::probe_limit`].
    async fn list_company_outbox_page(
        &self,
        _company_id: Uuid,
        _channel_id: Option<Uuid>,
        _status: Option<OutboxStatus>,
        _sort_asc: bool,
        _offset: i64,
        _limit: i64,
    ) -> AppResult<Vec<OutboxEntry>> {
        Ok(Vec::new())
    }

    /// One outbox row by id. The caller checks its `company_id` before showing it — the id comes
    /// from a URL.
    async fn get_outbox_entry(&self, _outbox_id: Uuid) -> AppResult<Option<OutboxEntry>> {
        Ok(None)
    }

    /// Hand one email to the transport, exactly once per `idempotency_key`.
    ///
    /// `Some(outbox_id)` means this caller queued it; `None` means an equivalent send is already
    /// queued or delivered and this caller must not queue a second one. Delivery itself happens
    /// later, in the outbox poller, on whichever instance claims the row.
    ///
    /// The default accepts without recording, so test doubles are unaffected.
    async fn enqueue_outbound_send(&self, _send: OutboundSend) -> AppResult<Option<Uuid>> {
        Ok(Some(Uuid::new_v4()))
    }

    async fn get_outreach_thread_for_outbox(&self, _outbox_id: Uuid) -> AppResult<Option<Uuid>> {
        Ok(None)
    }

    async fn is_outbox_delivery_active(&self, _outbox_id: Uuid) -> AppResult<bool> {
        Ok(true)
    }

    async fn cancel_claimed_outbox(&self, _outbox_id: Uuid, _worker_id: Uuid) -> AppResult<bool> {
        Ok(false)
    }

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

    /// The task an email already caused, found through the canonical message it was stored as.
    ///
    /// The lookup goes through `email_message_metadata` rather than a text column on the task, so
    /// "have we run this message" and "have we stored this message" are the same fact.
    async fn find_task_for_email_message(
        &self,
        _company_id: Uuid,
        _rfc_message_id: &MessageId,
    ) -> AppResult<Option<BackgroundTask>> {
        Ok(None)
    }

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
    async fn begin_task_attempt(&self, attempt: TaskAttemptRef) -> AppResult<()> {
        let _ = attempt;
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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
