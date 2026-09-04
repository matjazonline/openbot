use std::future::Future;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::{Duration, Instant, sleep, sleep_until};
use tracing::{Instrument, info, warn};
use uuid::Uuid;

use crate::{
    domain::monitoring::{MonitoringService, TaskExecutionMetrics, TaskStatusMetric},
    entities::{
        approval::{ApprovalAction, ApprovalSubject, QUORUM_TIMEOUT_ACTION},
        channel::Channel,
        message::{MessageDirection, MessageRole},
        outreach::DueOutreach,
        stuck_work::StuckWorkThresholds,
        task::{
            BackgroundTask, ResumeActor, StopActor, TaskAttemptOutcome, TaskAttemptRef,
            TaskAttemptStatus, TaskExecutionOutcome, TaskFailure, TaskFailureOutcome, TaskLeaseRef,
            TaskStopReason, TaskSuspension,
        },
        value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    services::runtime_metrics::ActiveTaskExecutions,
    task_queue::{
        Leased, TASK_LEASE_SECONDS, TaskLease, TaskPersistence, report_outcome, while_leased,
    },
    transport::{
        CanonicalContent, DeliveryContext, DeliveryPurpose, DeliveryRequest, EmailDeliveryContext,
        EmailThreading, InboundTaskPayload,
    },
    use_cases::{
        schedule::{SCHEDULED_AGENT_RUN_TASK, ScheduleUseCases},
        thread::{DispatchOutcome, MessageAuthorWrite, MessageWrite, ThreadUseCases},
    },
};

/// How long the task loop waits before looking for work again. This is the whole delay between a
/// message being ingested and its agent starting, so it is short: the claim is one index scan
/// against `background_tasks_pending_ready_idx`, which costs nothing to run twice a second against
/// an empty queue.
const TASK_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How often the schedule loop checks for due recurring or one-off runs.
const SCHEDULE_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Schedule materialization performs only bounded database work. A short lease makes a crashed
/// worker recover promptly while leaving ample room for ordinary database latency.
const SCHEDULE_MATERIALIZATION_LEASE_SECONDS: i64 = 60;

/// How often the slow lane runs. Quorum timeouts and expired delivery leases are measured in
/// minutes; checking them at queue cadence scans a hundred outreaches to find nothing.
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);

/// How long to wait after an iteration that failed outright. Without it a Postgres outage fills the
/// log twice a second per loop rather than once every few seconds.
const ERROR_BACKOFF: Duration = Duration::from_secs(5);

/// What one poll iteration found, and therefore whether its loop should pause before the next one.
#[derive(Debug, PartialEq, Eq)]
enum Polled {
    /// The batch came back short, so the queue is empty for now.
    Idle,
    /// The batch came back full, so there is probably more behind it.
    MoreWaiting,
}

/// A full batch probably has more behind it; a short one emptied the queue.
fn polled(claimed: usize, batch_size: i64) -> Polled {
    if claimed as i64 >= batch_size {
        Polled::MoreWaiting
    } else {
        Polled::Idle
    }
}

/// Run `step` against `state` until shutdown: straight back round while it reports more waiting, on
/// `interval` once the queue is empty, and after [`ERROR_BACKOFF`] when it failed.
///
/// The work runs at the top of the loop rather than after the first sleep, so a queue that already
/// has something in it at startup is served immediately.
///
/// Shutdown is observed between iterations. Durable agent execution uses the separately bounded
/// task loop below, where cancellation reaches the provider future and the lease-fenced atomic
/// commit makes interruption safe.
async fn poll_until_shutdown<State, Work>(
    name: &'static str,
    interval: Duration,
    mut shutdown: broadcast::Receiver<()>,
    state: Arc<State>,
    step: fn(Arc<State>) -> Work,
) where
    Work: Future<Output = Result<Polled, String>>,
{
    loop {
        let pause = match step(Arc::clone(&state)).await {
            Ok(Polled::MoreWaiting) => Duration::ZERO,
            Ok(Polled::Idle) => interval,
            Err(error) => {
                warn!("Error in the {} poll loop: {}", name, error);
                ERROR_BACKOFF
            }
        };

        tokio::select! {
            _ = shutdown.recv() => {
                info!("Shutdown signal received. Stopping the {} poll loop...", name);
                return;
            }
            // Even a zero pause goes through the select, so a fast-draining loop stays
            // interruptible.
            _ = sleep(pause) => {}
        }
    }
}

/// Keep a bounded set of durable tasks busy, replenishing a slot as soon as its task finishes.
/// A short claim means the queue is empty, so the next database poll waits for `interval`; a full
/// claim fills every available slot and completion immediately triggers another claim.
async fn run_bounded_task_loop<Item, Claim, ClaimFuture, Execute, ExecuteFuture>(
    interval: Duration,
    mut shutdown: broadcast::Receiver<()>,
    concurrency: usize,
    mut claim: Claim,
    execute: Execute,
) where
    Item: Send + 'static,
    Claim: FnMut(usize) -> ClaimFuture,
    ClaimFuture: Future<Output = Result<Vec<Item>, String>>,
    Execute: Fn(Item, broadcast::Receiver<()>) -> ExecuteFuture,
    ExecuteFuture: Future<Output = ()> + Send + 'static,
{
    debug_assert!(concurrency > 0);
    let mut running = JoinSet::new();
    let mut next_poll = Instant::now();

    loop {
        let available = concurrency.saturating_sub(running.len());
        if available > 0 && Instant::now() >= next_poll {
            // Subscribe prospective execution slots before the claim. If shutdown races the
            // database response, a task claimed at that boundary still receives the already-sent
            // cancellation and releases its durable lease as retryable.
            let execution_shutdowns = (0..available)
                .map(|_| shutdown.resubscribe())
                .collect::<Vec<_>>();
            let claim_result = tokio::select! {
                biased;
                _ = shutdown.recv() => break,
                result = claim(available) => result,
            };
            match claim_result {
                Ok(items) => {
                    let claimed = items.len();
                    debug_assert!(claimed <= available, "claim exceeded requested capacity");
                    for (item, execution_shutdown) in items.into_iter().zip(execution_shutdowns) {
                        running.spawn(execute(item, execution_shutdown));
                    }
                    next_poll = if claimed < available {
                        Instant::now() + interval
                    } else {
                        Instant::now()
                    };
                }
                Err(error) => {
                    warn!("Error in the task poll loop: {}", error);
                    next_poll = Instant::now() + ERROR_BACKOFF;
                }
            }
        }

        if running.is_empty() {
            tokio::select! {
                _ = shutdown.recv() => break,
                _ = sleep_until(next_poll) => {}
            }
        } else if running.len() >= concurrency {
            tokio::select! {
                _ = shutdown.recv() => break,
                result = running.join_next() => report_task_join(result)
            }
        } else {
            tokio::select! {
                _ = shutdown.recv() => break,
                result = running.join_next() => report_task_join(result),
                _ = sleep_until(next_poll) => {}
            }
        }
    }

    info!(
        "Shutdown signal received. Stopping task claims and draining {} active task(s)...",
        running.len()
    );
    while let Some(result) = running.join_next().await {
        report_task_join(Some(result));
    }
}

fn report_task_join(result: Option<Result<(), tokio::task::JoinError>>) {
    if let Some(Err(error)) = result {
        warn!("Background task execution ended unexpectedly: {}", error);
    }
}

#[cfg(test)]
use crate::{entities::schedule::ScheduledRunPayload, transport::ConversationAnchor};

/// The conversation a scheduled run's mail threads onto: the run's own durable slot.
///
/// Nothing carried the prompt this answers -- the platform asked because a slot came due -- so
/// there is no received header to reply to. Naming the slot rather than the task is what makes a
/// recipient's client file every firing of one schedule as one conversation, and what makes a
/// retry of the same firing land in it rather than beside it.
///
/// Deliberately not an RFC `Message-ID`: what an anchor looks like on the wire is the email
/// adapter's decision, and a schedule that one day delivers over Slack hands over this same key.
#[cfg(test)]
fn scheduled_run_anchor(payload: &ScheduledRunPayload) -> ConversationAnchor {
    // A prefix and a UUID: 49 bytes, never empty and never control-bearing, so the bound cannot
    // be reached. A fallback here would be worse than a panic -- two schedules sharing one anchor
    // is two schedules sharing one conversation in every recipient's client.
    ConversationAnchor::parse(format!("schedule-run:{}", payload.run_key))
        .expect("a prefixed UUID is within the anchor bound")
}

#[cfg(test)]
fn reply_subject(subject: &str) -> String {
    if subject.trim_start().to_lowercase().starts_with("re:") {
        subject.to_string()
    } else {
        format!("Re: {subject}")
    }
}

/// Why one task run stopped, and whether running it again could ever end differently.
///
/// `From<String>` yields [`RunFailure::Retryable`], so the many `?` sites that surface a
/// stringly error keep the retry behaviour they already had; a terminal failure must be stated.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RunFailure {
    Retryable(String),
    TimedOut(String),
    Terminal(String),
}

impl From<String> for RunFailure {
    fn from(message: String) -> Self {
        RunFailure::Retryable(message)
    }
}

pub struct TaskWorker {
    task_persistence: Arc<dyn TaskPersistence>,
    thread_use_cases: Arc<ThreadUseCases>,
    schedule_use_cases: Option<Arc<ScheduleUseCases>>,
    config: Arc<AppConfig>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    worker_id: uuid::Uuid,
    task_concurrency: usize,
    agent_run_timeout: std::time::Duration,
    active_task_executions: ActiveTaskExecutions,
}

impl TaskWorker {
    pub fn new(
        task_persistence: Arc<dyn TaskPersistence>,
        thread_use_cases: Arc<ThreadUseCases>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            task_persistence,
            thread_use_cases,
            schedule_use_cases: None,
            config,
            monitoring: None,
            worker_id: uuid::Uuid::new_v4(),
            task_concurrency: 1,
            agent_run_timeout: std::time::Duration::from_secs(300),
            active_task_executions: ActiveTaskExecutions::default(),
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    pub fn with_schedules(mut self, schedule_use_cases: Arc<ScheduleUseCases>) -> Self {
        self.schedule_use_cases = Some(schedule_use_cases);
        self
    }

    pub fn with_task_concurrency(mut self, concurrency: usize) -> Self {
        assert!(concurrency > 0, "task worker concurrency must be positive");
        self.task_concurrency = concurrency;
        self
    }

    pub fn with_agent_run_timeout(mut self, timeout: std::time::Duration) -> Self {
        assert!(!timeout.is_zero(), "agent run timeout must be positive");
        self.agent_run_timeout = timeout;
        self
    }

    pub fn with_active_task_executions(mut self, gauge: ActiveTaskExecutions) -> Self {
        self.active_task_executions = gauge;
        self
    }

    /// Run the worker's poll loops until shutdown.
    ///
    /// They are separate on purpose. Agent runs occupy bounded task slots for seconds or minutes,
    /// while schedules must keep firing regardless. Maintenance is split off for the opposite
    /// reason: its deadlines are minutes away, so it must not be re-run every time the queue loops
    /// look.
    ///
    /// Delivery is not here at all: it is a queue of its own, drained by
    /// [`crate::services::delivery_worker::DeliveryWorker`], because a delivery outlives the task
    /// that produced it and plenty of deliveries have no task behind them.
    pub async fn start_worker_loop(
        self: Arc<Self>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) {
        info!(
            "Starting Background Task Worker (task concurrency {}, tasks every {:?}, schedules every {:?}, maintenance every {:?})...",
            self.task_concurrency, TASK_POLL_INTERVAL, SCHEDULE_POLL_INTERVAL, MAINTENANCE_INTERVAL
        );

        let tasks = Arc::clone(&self).run_task_loop(shutdown_rx.resubscribe());
        let schedules = if self.schedule_use_cases.is_some() {
            futures::future::Either::Left(poll_until_shutdown(
                "schedule",
                SCHEDULE_POLL_INTERVAL,
                shutdown_rx.resubscribe(),
                Arc::clone(&self),
                |worker| async move { worker.process_due_schedules().await },
            ))
        } else {
            futures::future::Either::Right(std::future::ready(()))
        };
        let maintenance = poll_until_shutdown(
            "maintenance",
            MAINTENANCE_INTERVAL,
            shutdown_rx,
            self,
            |worker| async move { worker.run_maintenance().await },
        );

        let _ = tokio::join!(tasks, schedules, maintenance);
    }

    /// Claim and advance any due recurring or one-off channel schedules.
    async fn process_due_schedules(&self) -> Result<Polled, String> {
        let Some(ref schedules) = self.schedule_use_cases else {
            return Ok(Polled::Idle);
        };
        let count = schedules
            .process_due_schedules(
                self.worker_id,
                chrono::Utc::now()
                    + chrono::Duration::seconds(SCHEDULE_MATERIALIZATION_LEASE_SECONDS),
                10,
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(polled(count, 10))
    }

    async fn run_task_loop(self: Arc<Self>, shutdown: broadcast::Receiver<()>) {
        let concurrency = self.task_concurrency;
        let claimant = Arc::clone(&self);
        let executor = self;
        run_bounded_task_loop(
            TASK_POLL_INTERVAL,
            shutdown,
            concurrency,
            move |available| {
                let worker = Arc::clone(&claimant);
                async move { worker.claim_pending_task_batch(available).await }
            },
            move |task, shutdown| {
                let worker = Arc::clone(&executor);
                async move { worker.process_claimed_task(task, Some(shutdown)).await }
            },
        )
        .await;
    }

    async fn claim_pending_task_batch(&self, limit: usize) -> Result<Vec<BackgroundTask>, String> {
        self.task_persistence
            .claim_pending_tasks(
                self.worker_id,
                chrono::Utc::now() + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                limit as i64,
            )
            .await
            .map_err(|e| e.to_string())
    }

    /// Run one claimed task to a terminal state.
    ///
    /// `shutdown` is absent only in tests, which drive a task with no shutdown to race against.
    /// This is called directly from the poll loop's closure rather than through a wrapper: in an
    /// unoptimized build every `async fn` between the spawned task and the agent costs its child
    /// future's size in stack, and the wrapper that used to sit here cost 200 KiB to pass an
    /// `Option`. See `scripts/stack-frames.sh`.
    async fn process_claimed_task(
        &self,
        task: BackgroundTask,
        mut shutdown: Option<broadcast::Receiver<()>>,
    ) {
        // A row this worker just claimed always carries its lease; the constraint on
        // `background_tasks` makes a `processing` row without one unrepresentable. Bailing rather
        // than asserting keeps a surprising row from taking the worker down with it.
        let Some(lease) = TaskLeaseRef::of(&task) else {
            warn!(
                "Claimed task {} carries no lease and cannot be executed safely",
                task.id
            );
            return;
        };
        let _active_execution = self.active_task_executions.enter();
        info!("Processing task {} (type = '{}')", task.id, task.task_type);
        let start_time = std::time::Instant::now();
        let attempt = TaskAttemptRef::of(&task, lease);
        let execution = self.execute_single_task_with_lease(&task, lease, attempt);
        tokio::pin!(execution);
        let outcome = match shutdown.as_mut() {
            Some(shutdown) => tokio::select! {
                outcome = &mut execution => outcome,
                _ = shutdown.recv() => TaskExecutionOutcome::Interrupted(
                    "Task execution interrupted by shutdown".into(),
                ),
            },
            None => execution.await,
        };
        let duration_ms = start_time.elapsed().as_millis() as u64;
        self.close_out_task(&task, lease, attempt, outcome, duration_ms)
            .await;
    }

    #[cfg(test)]
    async fn process_next_task_batch(&self) -> Result<Polled, String> {
        let tasks = self.claim_pending_task_batch(1).await?;
        let claimed = tasks.len();
        for task in tasks {
            self.process_claimed_task(task, None).await;
        }
        Ok(polled(claimed, 1))
    }

    /// The slow lane: work whose deadlines are minutes away, kept off the queue loops so that
    /// shortening their interval does not multiply it.
    async fn run_maintenance(&self) -> Result<Polled, String> {
        // Tasks whose run vanished mid-flight. Claims take pending rows only, so
        // without this an expired `processing` row is never picked up again by anyone.
        match self.task_persistence.reap_expired_task_leases().await {
            Ok(0) => {}
            Ok(reaped) => warn!("Reaped {} tasks whose lease had expired", reaped),
            Err(error) => warn!("Failed to reap expired task leases: {}", error),
        }

        self.check_quorum_timeouts().await?;
        self.report_stuck_work().await;
        Ok(Polled::Idle)
    }

    /// Publish how much work is stuck, and say so in the log when any of it is.
    ///
    /// The queue tables have always known this; nothing looked. Every figure goes out as a gauge
    /// on every sweep, zeroes included, so an alert built on one can clear itself -- a metric that
    /// simply stops being published looks identical to a healthy system, which is the failure mode
    /// worth avoiding here.
    ///
    /// Failing to take the census is not worth failing maintenance over: the reaping above is real
    /// work and this is only reporting on it.
    async fn report_stuck_work(&self) {
        let census = match self
            .task_persistence
            .census_stuck_work(StuckWorkThresholds::default())
            .await
        {
            Ok(census) => census,
            Err(error) => {
                warn!(error = %error, "Could not take the stuck-work census");
                return;
            }
        };

        if let Some(monitoring) = self.monitoring.as_ref() {
            for (kind, count) in census.gauges() {
                monitoring.record_gauge("stuck_work", count as f64, &[("kind", kind.as_str())]);
            }
        }

        // A healthy system stays silent. Every thirty seconds is far too often to say "all clear".
        for (kind, count) in census.alerts() {
            warn!(
                target: "monitoring::stuck_work",
                kind = %kind.as_str(),
                count = count,
                detail = %kind.description(),
                "Work is stuck"
            );
        }
    }

    /// Record the fate of one executed task: completed, suspended awaiting someone else, or failed
    /// and scheduled for retry.
    async fn close_out_task(
        &self,
        task: &BackgroundTask,
        lease: TaskLeaseRef,
        attempt: TaskAttemptRef,
        outcome: TaskExecutionOutcome,
        duration_ms: u64,
    ) {
        let task_id = task.id;
        // The row as it stands *now*, not as it was claimed: the run writes its token usage into
        // the payload and may have parked itself, and both of those are read below.
        let current = match self.task_persistence.get_task_by_id(task_id).await {
            Ok(current) => current,
            Err(error) => {
                warn!("Could not re-read task {task_id} to close it out: {error}");
                None
            }
        };

        let (err_msg, dead_letter_now, stop_reason) = match outcome {
            TaskExecutionOutcome::Suspended => {
                info!("Background task {} suspended by its agent", task_id);
                return;
            }
            TaskExecutionOutcome::Replied => {
                // A task that parked itself keeps its own status; it is neither done nor failed.
                // Its attempt stays open too — the resume runs under the same number.
                let suspended = current
                    .as_ref()
                    .map(|task| task.status)
                    .filter(|status| status.is_suspended());
                if let Some(status) = suspended {
                    info!(
                        "Background task {} suspended with status {}",
                        task_id,
                        status.as_str()
                    );
                    return;
                }
                info!("Successfully completed background task {}", task_id);
                self.close_out_attempt(
                    task,
                    attempt,
                    current.as_ref(),
                    TaskAttemptStatus::Completed,
                    TaskStopReason::Completed,
                    None,
                )
                .await;
                let outcome = self.task_persistence.mark_task_completed(lease).await;
                report_outcome("Task", task_id, "completion", outcome);
                self.record_task_metric(
                    task,
                    duration_ms,
                    TaskStatusMetric::Completed,
                    TaskStopReason::Completed,
                );
                return;
            }
            TaskExecutionOutcome::RetryableFailure(message) => {
                (message, false, TaskStopReason::RetryableFailure)
            }
            TaskExecutionOutcome::TimedOut(message) => (message, false, TaskStopReason::TimedOut),
            TaskExecutionOutcome::Interrupted(message) => {
                (message, false, TaskStopReason::Shutdown)
            }
            TaskExecutionOutcome::LeaseLost(message) => (message, false, TaskStopReason::LeaseLost),
            TaskExecutionOutcome::TerminalFailure(message) => {
                (message, true, TaskStopReason::TerminalFailure)
            }
        };

        warn!(
            task_id = %task_id,
            correlation_id = %task.correlation_id,
            task_type = %task.task_type,
            retry_count = task.retry_count,
            dead_letter = dead_letter_now || task.retry_count + 1 >= task.max_retries,
            error = %err_msg,
            "Background task failed"
        );
        self.close_out_attempt(
            task,
            attempt,
            current.as_ref(),
            TaskAttemptStatus::Failed,
            stop_reason,
            Some(err_msg.clone()),
        )
        .await;
        let next_retry = task.retry_count + 1;
        let outcome_side = if dead_letter_now || next_retry >= task.max_retries {
            TaskFailureOutcome::DeadLetter
        } else {
            TaskFailureOutcome::Retry
        };
        // Exponential backoff: 30s * 2^retry, capped so the shift can't overflow.
        let backoff_secs = 30 * (1 << next_retry.min(10));
        let next_run = chrono::Utc::now() + chrono::Duration::seconds(backoff_secs);

        let outcome = self
            .task_persistence
            .mark_task_failed(TaskFailure {
                lease,
                error: &err_msg,
                next_run_at: next_run,
                outcome: outcome_side,
                reason: stop_reason,
            })
            .await;
        report_outcome("Task", task_id, "failure", outcome);
        self.record_task_metric(task, duration_ms, TaskStatusMetric::Failed, stop_reason);
    }

    /// Close this run's row in the attempt ledger, which is what the dashboard reads duration and
    /// token spend from.
    ///
    /// Written before the queue transition rather than after: this records what *this run* did, and
    /// it did finish. Whether the run is still the one of record is a separate question, answered
    /// by the guard inside `finish_task_attempt` and by `report_outcome` on the transition itself.
    ///
    /// A ledger write that fails is logged and dropped — telemetry must not fail a task.
    async fn close_out_attempt(
        &self,
        task: &BackgroundTask,
        attempt: TaskAttemptRef,
        current: Option<&BackgroundTask>,
        status: TaskAttemptStatus,
        stop_reason: TaskStopReason,
        error: Option<String>,
    ) {
        let outcome = TaskAttemptOutcome {
            attempt,
            status,
            stop_reason,
            error,
            // The run writes its usage into the payload, so it is only on the re-read row; the
            // copy this worker claimed predates the run entirely.
            tokens: current.and_then(BackgroundTask::token_usage),
        };

        report_outcome(
            "Task attempt",
            task.id,
            "outcome",
            self.task_persistence.finish_task_attempt(&outcome).await,
        );
    }

    fn record_task_metric(
        &self,
        task: &BackgroundTask,
        duration_ms: u64,
        status: TaskStatusMetric,
        stop_reason: TaskStopReason,
    ) {
        if let Some(ref m) = self.monitoring {
            m.record_task_execution(&TaskExecutionMetrics {
                company_id: Some(task.company_id),
                channel_id: Some(task.channel_id),
                task_type: task.task_type.clone(),
                duration_ms,
                status,
                stop_reason,
                retry_count: task.retry_count as u32,
            });
            m.increment_counter(
                &format!("task_execution_stops_{}_total", stop_reason.as_str()),
                1,
                &[],
            );
        }
    }

    /// Ask a human what to do about outreaches that ran out of time below their response quorum.
    pub async fn check_quorum_timeouts(&self) -> Result<(), String> {
        let now = chrono::Utc::now();
        let outreaches = self
            .task_persistence
            .list_due_outreaches(now, 100)
            .await
            .unwrap_or_default();

        for outreach in outreaches {
            let current_percent = if outreach.target_count == 0 {
                0.0
            } else {
                outreach.response_count as f64 * 100.0 / outreach.target_count as f64
            };
            if current_percent >= outreach.required_threshold_percent {
                continue;
            }
            self.request_quorum_timeout_decision(&outreach, current_percent)
                .await?;
        }

        Ok(())
    }

    async fn request_quorum_timeout_decision(
        &self,
        outreach: &DueOutreach,
        current_percent: f64,
    ) -> Result<(), String> {
        let Some(approval_use_cases) = self.thread_use_cases.get_approval_use_cases() else {
            return Ok(());
        };
        let Some(task) = self
            .task_persistence
            .get_task_by_id(outreach.task_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(());
        };

        let Some((company, channel)) = self.task_scope(&task).await? else {
            return Ok(());
        };

        let approver_email = match self.approver_for(&channel, task.company_id).await {
            Some(approver_email) => approver_email,
            None => {
                warn!("No approver configured for outreach task {}", task.id);
                return Ok(());
            }
        };

        // Claim the timeout first: only the worker that flips the state may raise the approval.
        //
        // The claim propagates rather than defaulting. `unwrap_or(false)` read "on a database
        // error, someone else claimed it" -- which is the one answer that is never checkable: the
        // timeout is then silently never raised by anyone, and the sweep that would retry it has
        // already returned `Ok`. An error here is retried on the next sweep instead.
        if !self
            .task_persistence
            .mark_outreach_timeout_pending(outreach.outreach_id)
            .await
            .map_err(|error| error.to_string())?
        {
            return Ok(());
        }
        info!(
            "Task {} reached quorum timeout with {:.1}% responses (< {:.1}% required)",
            outreach.task_id, current_percent, outreach.required_threshold_percent
        );

        let Some(thread_id) = task.thread_id else {
            // The approval is written into the thread it concerns before it is mailed, so a task
            // with none has nowhere to raise it. Every production path that can reach a quorum
            // timeout runs on a thread; saying so is what keeps that assumption checkable.
            warn!(
                task_id = %task.id,
                "Cannot raise a quorum-timeout approval for a task with no thread"
            );
            return Ok(());
        };
        let subject = ApprovalSubject {
            company_id: task.company_id,
            channel_id: task.channel_id,
            channel_name: channel.name.clone(),
            channel_slug: channel.slug.clone(),
            company_slug: company.slug.clone(),
            thread_id,
            // The sweep holds no lease: this task is already parked awaiting third-party replies,
            // and the quorum timeout is what moves it on to awaiting a human.
            suspension: Some(TaskSuspension::AlreadySuspended { task_id: task.id }),
            correlation_id: task.correlation_id,
            approver_email,
        };
        let action = ApprovalAction {
            // Keyed on the deadline as well as the outreach, so a timeout that is extended and
            // then expires again asks afresh rather than reusing the answered request.
            step_key: format!(
                "quorum_timeout_{}_{}",
                outreach.outreach_id,
                outreach.expires_at.timestamp()
            ),
            action_type: QUORUM_TIMEOUT_ACTION.to_string(),
            title: "Partial Quorum Timeout: Action Required".to_string(),
            summary: format!(
                "Outreach timed out with {}/{} responses ({:.1}%). Required: {:.1}%.",
                outreach.response_count,
                outreach.target_count,
                current_percent,
                outreach.required_threshold_percent
            ),
            payload: serde_json::json!({
                "outreach_id": outreach.outreach_id,
                "current_percent": current_percent,
                "required_percent": outreach.required_threshold_percent,
                "current_count": outreach.response_count,
                "total_targets": outreach.target_count,
            }),
        };

        if let Err(error) = approval_use_cases
            .create_and_send_approval_request(&subject, action)
            .await
        {
            // Nobody was asked, so put the outreach back into waiting rather than stranding it.
            let _ = self
                .task_persistence
                .restore_outreach_waiting(outreach.outreach_id)
                .await;
            return Err(error.to_string());
        }
        Ok(())
    }

    /// The channel's own approver, falling back to any member of the owning company's team.
    ///
    /// The channel names a *principal*; only the email transport needs an address. A principal
    /// with no email identity is a routing gap, so the team fallback still applies rather than
    /// the approval going unasked.
    async fn approver_for(&self, channel: &Channel, company_id: Uuid) -> Option<EmailAddress> {
        if let Some(principal_id) = channel.preferred_approver() {
            match self
                .thread_use_cases
                .preferred_email_for_principal(company_id, principal_id)
                .await
            {
                Ok(address) => match address.filter(|email| !email.is_empty()) {
                    Some(address) => return Some(address),
                    None => warn!(
                        %principal_id,
                        "Channel approver has no email identity; asking the team instead"
                    ),
                },
                Err(error) => warn!(
                    %error, %principal_id,
                    "Approver identity lookup failed; asking the team instead"
                ),
            }
        }

        self.thread_use_cases
            .company_persistence()
            .list_company_team_emails(company_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .next()
            .map(EmailAddress::from)
            .filter(|email| !email.is_empty())
    }

    /// One task's work, as `Result` so the `?` operator still carries the failures upward.
    /// Anything that arrives as a bare `String` is treated as retryable; a failure that no retry
    /// could fix has to say so.
    async fn run_task(
        &self,
        task: &crate::entities::task::BackgroundTask,
        lease: TaskLeaseRef,
    ) -> Result<TaskExecutionOutcome, RunFailure> {
        if task.task_type == SCHEDULED_AGENT_RUN_TASK {
            let dispatch = Box::pin(
                self.thread_use_cases
                    .execute_claimed_scheduled_agent_task_and_dispatch(
                        task,
                        lease,
                    ),
            )
            .await
            .map_err(|error| match error {
                crate::app_error::AppError::Timeout(message) => RunFailure::TimedOut(message),
                other => RunFailure::Retryable(other.to_string()),
            })?;

            return Ok(match dispatch {
                DispatchOutcome::Suspended => TaskExecutionOutcome::Suspended,
                DispatchOutcome::Skipped | DispatchOutcome::Replied(_) => {
                    TaskExecutionOutcome::Replied
                }
            });
        }

        // A payload that will not decode will not decode on the next attempt either.
        let payload = InboundTaskPayload::decode(&task.payload)
            .map_err(|error| RunFailure::Terminal(format!("Invalid task payload: {error}")))?;
        // Everything the run needs is reloaded here rather than replayed from the payload, so a
        // task queued an hour ago answers against the channel's current configuration.
        let ingest = self
            .thread_use_cases
            .load_inbound_task(task.id, payload.identifiers())
            .await
            .map_err(|error| RunFailure::Terminal(error.to_string()))?;

        let inbound_msg = ingest.inbound_message.as_ref().ok_or_else(|| {
            RunFailure::Terminal("The message this task answers no longer exists".to_string())
        })?;

        // Idempotency Guard: Check if an outbound email for this triggering message was already sent
        let target_thread_ids: Vec<_> = ingest
            .channel_matches
            .iter()
            .map(|channel_match| channel_match.thread.id)
            .collect();
        let mut outbound_reply = None;
        let mut missing_threads = Vec::new();
        for thread_id in target_thread_ids {
            match self
                .thread_use_cases
                .find_outbound_reply_after(thread_id, inbound_msg.canonical_id)
                .await
                .map_err(|e| e.to_string())?
            {
                Some(message) => outbound_reply = Some(message),
                None => missing_threads.push(thread_id),
            }
        }

        if let Some(outbound) = outbound_reply {
            // The reply is one canonical message; a thread that is missing it needs the
            // association, not a second copy of the body.
            for thread_id in missing_threads {
                self.thread_use_cases
                    .associate_message(thread_id, outbound.canonical_id)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            // A stored outbound reply is proof the work was done. It used to be necessary to
            // sniff the body for an error prefix here, because a failed run committed its error
            // text as the reply; a failed run now commits nothing, so anything found is a real
            // answer and the task is complete.
            info!(
                "Idempotency Guard: Outbound reply already sent for message {}, completing task",
                inbound_msg.canonical_id
            );
            return Ok(TaskExecutionOutcome::Replied);
        }

        // Execute Agent and Dispatch Outbound Email
        let mut ingest_exec = ingest.clone();
        ingest_exec.task_id = Some(task.id);

        // Whether the reply is really sent was decided when the message came in, not here: a
        // mailbox send can ask to stay in-app, and this worker is a different process from the one
        // that took the request.
        let reply_delivery = ingest_exec.reply_delivery;
        // Boxed for the same reason as the scheduled branch above: both descend into the agent, and
        // both would otherwise be stored inline in this frame.
        let dispatch = Box::pin(
            self.thread_use_cases
                .execute_claimed_agent_task_and_dispatch(
                    &ingest_exec,
                    reply_delivery,
                    lease,
                    task.correlation_id,
                ),
        )
        .await
        .map_err(|error| match error {
            crate::app_error::AppError::Timeout(message) => RunFailure::TimedOut(message),
            other => RunFailure::Retryable(other.to_string()),
        })?;

        Ok(match dispatch {
            DispatchOutcome::Suspended => TaskExecutionOutcome::Suspended,
            DispatchOutcome::Skipped | DispatchOutcome::Replied(_) => TaskExecutionOutcome::Replied,
        })
    }

    async fn execute_single_task_with_lease(
        &self,
        task: &crate::entities::task::BackgroundTask,
        lease: TaskLeaseRef,
        attempt: TaskAttemptRef,
    ) -> TaskExecutionOutcome {
        // Open the ledger row alongside the lease: both describe this one run, and both are wanted
        // even if it never reaches a terminal state. A failure to open it is logged, not fatal —
        // the run is the point, the bookkeeping is not.
        if let Err(error) = self.task_persistence.begin_task_attempt(attempt).await {
            warn!(
                "Could not open the attempt ledger for task {}: {error}",
                task.id
            );
        }

        // One span for the whole run, so every event the agent and its tools emit
        // underneath it carries the chain without each of them having to know about it.
        let run_span = tracing::info_span!(
            "task-run",
            correlation_id = %task.correlation_id,
            task_id = %task.id,
            task_type = %task.task_type,
            company_id = %task.company_id,
            channel_id = %task.channel_id,
            attempt = attempt.attempt_number,
        );

        let lease = TaskLease::worker(lease);
        // `run_task` reaches the agent, and its future is the largest thing on this stack. Boxing
        // it leaves a pointer in this frame instead of the whole state machine, which is what keeps
        // the chain below from spending the thread's stack before it gets there.
        let supervised = while_leased(
            &*self.task_persistence,
            &lease,
            Box::pin(self.run_task(task, lease.reference)).instrument(run_span),
        );
        match supervised.await {
            Leased::Finished(Ok(outcome)) => outcome,
            Leased::Finished(Err(RunFailure::Retryable(message))) => {
                TaskExecutionOutcome::RetryableFailure(message)
            }
            Leased::Finished(Err(RunFailure::TimedOut(message))) => {
                TaskExecutionOutcome::TimedOut(message)
            }
            Leased::Finished(Err(RunFailure::Terminal(message))) => {
                TaskExecutionOutcome::TerminalFailure(message)
            }
            // This run is no longer the one of record, so it must not report a result. Reporting
            // it as retryable is safe: every write it can still make is fenced on the worker id
            // the replacement run does not share.
            Leased::Lost => {
                TaskExecutionOutcome::LeaseLost("Task lease was lost during execution".into())
            }
        }
    }

    /// The company and channel one task belongs to, reloaded.
    ///
    /// `Ok(None)` only when a row this task names is gone, which is a stopped or deleted channel
    /// rather than a fault: the notice is skipped and the caller carries on.
    async fn task_scope(
        &self,
        task: &BackgroundTask,
    ) -> Result<
        Option<(
            crate::entities::company::Company,
            crate::entities::channel::Channel,
        )>,
        String,
    > {
        let company = self
            .thread_use_cases
            .company_persistence()
            .get_by_id(task.company_id)
            .await
            .map_err(|error| error.to_string())?;
        let channel = self
            .thread_use_cases
            .channel_persistence()
            .get_by_id(task.channel_id)
            .await
            .map_err(|error| error.to_string())?;
        Ok(company
            .zip(channel)
            .filter(|(company, channel)| channel.company_id == company.id))
    }

    /// Tell the people on a stopped task's thread that it will not answer.
    ///
    /// Addressed from the thread rather than from a copy of the message the task carried: the
    /// participants are on the thread, and the message the run was answering is in it.
    async fn notify_stopped_task(&self, task: &BackgroundTask) -> Result<(), String> {
        let Some((company, channel)) = self.task_scope(task).await? else {
            return Ok(());
        };
        let Some(thread_id) = task.thread_id else {
            return Ok(());
        };
        let Some(thread) = self
            .thread_use_cases
            .get_thread(thread_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(());
        };
        // The mail-shaped facts of the thread's newest turn, and only those: a notice is an email,
        // so this is the one projection that has headers in it.
        let Some(reply_to) = self
            .thread_use_cases
            .latest_email_reply_context(thread_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(());
        };
        // Nobody to mail. A thread whose newest turn came over a transport with no mailbox behind
        // it is not told by email that its task stopped -- it is shown in the app like every other
        // message, and inventing an address here would send the notice to a stranger.
        let Some(recipient) = reply_to.author_email.clone() else {
            return Ok(());
        };

        let subject = format!("[STOPPED] Re: {}", thread.subject);
        let body = format!(
            "Notice: The automated channel processing for thread '{}' has been manually stopped \
             by the system administrator.",
            thread.subject
        );

        // The notice is a message in the thread first and a delivery second. It used to be neither
        // -- it was composed, sent fire-and-forget, and left no trace anyone reading the thread
        // could see -- which meant a stopped task looked, in the app, exactly like one still
        // thinking.
        let message = MessageWrite::internal(
            thread_id,
            MessageAuthorWrite::Platform,
            subject.clone(),
            body.clone(),
            MessageDirection::Outbound,
            MessageRole::System,
            task.correlation_id,
        );

        let context = EmailDeliveryContext {
            from: Channel::address_for(&channel.slug, &company.slug, &self.config.app_domain_name),
            from_name: Some(channel.name.clone()),
            recipient_to: recipient,
            recipients_cc: reply_to.cc.clone(),
            threading: EmailThreading::received(
                reply_to.rfc_message_id.clone(),
                reply_to.references.clone(),
            ),
            // The notice ends the chain rather than continuing it: with no relay trace it carries
            // no hop count and no channel id, so nothing on the receiving side may answer it.
            relay: None,
        };

        let content = CanonicalContent::parse(subject, body).map_err(|error| error.to_string())?;
        let delivery = self
            .thread_use_cases
            .compose_delivery(DeliveryRequest {
                company_id: company.id,
                channel_id: channel.id,
                message_id: message.id,
                task_id: Some(task.id),
                correlation_id: task.correlation_id,
                purpose: DeliveryPurpose::Notification,
                // Keyed on the task, so stopping an already-stopped task does not send a second
                // notice.
                source_key: format!("task:{}:stopped", task.id),
                content: &content,
                context: DeliveryContext::Email(context),
            })
            .await
            .map_err(|error| error.to_string())?;

        self.thread_use_cases
            .save_message_with_deliveries(&message, &[delivery.delivery])
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub async fn stop_task_and_notify(
        &self,
        task_id: uuid::Uuid,
        actor: StopActor,
    ) -> Result<(), String> {
        let task = self
            .task_persistence
            .stop_task(task_id, actor)
            .await
            .map_err(|error| error.to_string())?;

        // The notice is addressed from ids, not from a copy of the message: the stopped task's
        // own thread is what its participants are reading, and the thread holds the message.
        if let Err(error) = self.notify_stopped_task(&task).await {
            warn!(task_id = %task.id, %error, "Could not deliver a stop notification");
        }

        Ok(())
    }

    pub async fn resume_task(&self, task_id: uuid::Uuid, actor: ResumeActor) -> Result<(), String> {
        self.task_persistence
            .resume_task(task_id, actor)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::channel::ChannelAccessMode;
    use crate::entities::company::CompanyAccess;
    use crate::entities::company_member::CompanyMembership;
    use crate::entities::task::NewTask;
    use crate::entities::task::TaskLeaseRef;
    use crate::entities::value_objects::MessageId;
    use crate::entities::correlation::CorrelationId;
    use crate::task_queue::{AgentDispatchCommit, DispatchCommit};
    use crate::transport::NewDelivery;
    use crate::use_cases::participant::test_support::{InMemoryParticipantDirectory, TeamFixture};
    use crate::use_cases::thread::test_support::{EmailMessageDraft, InMemoryThreads, email_write};
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::{Notify, Semaphore};
    use uuid::Uuid;

    use crate::{
        app_error::AppResult,
        entities::{
            agent::Agent,
            channel::Channel,
            company::Company,
            task::{BackgroundTask, TaskStatus},
            thread::Thread,
        },
        use_cases::{
            agent::{AgentPersistence, AgentWrite},
            company::{CompanyPersistence, CompanyWrite},
            thread::ThreadPersistence,
        },
    };

    struct MockAgentPersistence {
        agent: Agent,
    }

    #[async_trait]
    impl AgentPersistence for MockAgentPersistence {
        async fn create(&self, _company_id: Uuid, _write: AgentWrite) -> AppResult<Agent> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Agent>> {
            Ok((self.agent.id == id).then(|| self.agent.clone()))
        }
        async fn get_by_company_slug_and_agent_slug(
            &self,
            _company_slug: &str,
            _agent_slug: &str,
        ) -> AppResult<Option<Agent>> {
            unimplemented!()
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Agent>> {
            unimplemented!()
        }
        async fn update(&self, _id: Uuid, _write: AgentWrite) -> AppResult<Agent> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    fn active_agent(company_id: Uuid, id: Uuid) -> Agent {
        Agent {
            memory_enabled: false,
            id,
            company_id: Some(company_id),
            name: "Test agent".into(),
            slug: "test-agent".into(),
            provider: None,
            model: None,
            run_timeout_secs: None,
            system_prompt: Some("Help with the request.".into()),
            description: None,
            config_json: None,
            avatar_url: None,
            memory_persistence_mode: Default::default(),
            memory_recall_mode: Default::default(),
            memory_max_results: crate::entities::memory::default_memory_max_results(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        }
    }

    #[derive(Default)]
    struct MockCompanyPersistence {
        company: Option<Company>,
        model_api_key: Option<String>,
        model_connections: Vec<crate::entities::company::CompanyModelConnection>,
    }
    /// The worker's tests drive dispatch, not authorization: every sender is a colleague.
    #[async_trait]
    impl TeamFixture for MockCompanyPersistence {
        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }

        async fn company_access(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
        ) -> AppResult<Option<CompanyAccess>> {
            unimplemented!("this double is not exercised on the signed-in path")
        }
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
            Ok(self.company.clone().filter(|company| company.id == id))
        }
        async fn get_by_slug(&self, _slug: &str) -> AppResult<Option<Company>> {
            unimplemented!()
        }
        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }
        async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }

        async fn list_company_team_accounts(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
            unimplemented!("this double is not exercised on the team-account path")
        }

        async fn list_model_connections(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
            if !self.model_connections.is_empty() {
                return Ok(self.model_connections.clone());
            }
            Ok(vec![crate::entities::company::CompanyModelConnection {
                provider: "google".into(),
                models: vec!["gemini-2.5-flash".into()],
                is_default: true,
                has_api_key: false,
            }])
        }

        /// Matches the `has_api_key: false` above: the connection exists, the credential does not, so
        /// a run reaching this point fails at parameter resolution instead of calling a provider.
        async fn model_api_key(
            &self,
            _company_id: Uuid,
            _provider: &crate::entities::value_objects::ModelProvider,
        ) -> AppResult<Option<String>> {
            Ok(self.model_api_key.clone())
        }

        async fn replace_model_connections_for_user(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
            _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
        ) -> AppResult<()> {
            unimplemented!("the worker never writes model connections")
        }
    }

    use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};

    struct MockChannelPersistence {
        channel: Option<Channel>,
    }
    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(&self, _company_id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
            Ok(self.channel.clone().filter(|channel| channel.id == id))
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &crate::entities::value_objects::CompanySlug,
            _channel_slug: &crate::entities::value_objects::ChannelSlug,
        ) -> AppResult<Option<Channel>> {
            unimplemented!()
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(vec![])
        }
        async fn update(&self, _id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    #[derive(Default)]
    struct MockTaskPersistence {
        tasks: Mutex<Vec<BackgroundTask>>,
        /// Lease renewals seen, so a test can prove the heartbeat actually fired.
        renewals: Mutex<usize>,
        /// How many renewals to grant before reporting the lease gone. `None` grants every one.
        renewals_before_loss: Option<usize>,
        /// Deliveries queued on their own -- a scheduled digest whose answer was already stored.
        /// The trait's default refuses, which would let a test claiming a send happened fail for
        /// the wrong reason.
        queued_deliveries: Mutex<Vec<NewDelivery>>,
    }

    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        /// No fixture here sends an outreach, so nothing ever asks one to be recorded.
        async fn record_outreach_request_message(
            &self,
            _delivery_id: crate::entities::transport::DeliveryId,
            _write: &crate::use_cases::thread::MessageWrite,
        ) -> AppResult<crate::entities::message::CanonicalMessageId> {
            unreachable!("no fixture here sends an outreach")
        }

        /// The task's own channel and thread. These fixtures never enqueue a multi-channel run, so
        /// stating one target is the honest answer rather than an empty list the worker would read
        /// as "answer nowhere".
        async fn list_task_channel_targets(
            &self,
            _company_id: Uuid,
            task_id: Uuid,
        ) -> AppResult<Vec<crate::use_cases::thread::TaskChannelTarget>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .find(|task| task.id == task_id)
                .and_then(|task| {
                    task.thread_id
                        .map(|thread_id| crate::use_cases::thread::TaskChannelTarget {
                            channel_id: task.channel_id,
                            thread_id,
                            recipient_role: crate::transport::RecipientRole::To,
                        })
                })
                .into_iter()
                .collect())
        }

        async fn commit_agent_dispatch(
            &self,
            commit: AgentDispatchCommit<'_>,
        ) -> AppResult<DispatchCommit> {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(task) = tasks.iter_mut().find(|t| t.id == commit.lease.task_id) {
                task.payload = commit.payload.clone();
            }
            let mut deliveries = self.queued_deliveries.lock().unwrap();
            for delivery in commit.deliveries {
                deliveries.push(delivery);
            }
            Ok(DispatchCommit::Committed {
                deliveries: Vec::new(),
            })
        }

        async fn enqueue_delivery(
            &self,
            delivery: NewDelivery,
        ) -> AppResult<crate::transport::DeliveryCreation> {
            let created = crate::transport::DeliveryCreation::Created(delivery.id);
            self.queued_deliveries.lock().unwrap().push(delivery);
            Ok(created)
        }

        async fn renew_task_lease(
            &self,
            _lease: TaskLeaseRef,
            _lock_expires_at: chrono::DateTime<Utc>,
        ) -> AppResult<bool> {
            let mut renewals = self.renewals.lock().unwrap();
            *renewals += 1;
            Ok(match self.renewals_before_loss {
                Some(granted) => *renewals <= granted,
                None => true,
            })
        }

        async fn enqueue_task(
            &self,
            NewTask {
                company_id,
                channel_id,
                thread_id,
                targets: _,
                task_type,
                payload,
                source: _,
                correlation_id,
            }: NewTask,
        ) -> AppResult<BackgroundTask> {
            let task = BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id,
                correlation_id,
                task_type,
                status: TaskStatus::Pending,
                payload,
                retry_count: 0,
                max_retries: 3,
                last_error: None,
                worker_id: None,
                execution_generation: None,
                locked_at: None,
                lock_expires_at: None,
                run_at: Utc::now(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.tasks.lock().unwrap().push(task.clone());
            Ok(task)
        }

        async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn update_task_payload(&self, id: Uuid, payload: serde_json::Value) -> AppResult<()> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.payload = payload;
            }
            Ok(())
        }

        /// Mirrors the real reaper: an expired lease costs an attempt and the row goes back to
        /// pending, or dead-letters once the attempts are spent.
        async fn reap_expired_task_leases(&self) -> AppResult<u64> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            let mut reaped = 0;
            for task in list.iter_mut().filter(|task| {
                task.status == TaskStatus::Processing
                    && task.lock_expires_at.is_none_or(|expires| expires <= now)
            }) {
                task.retry_count += 1;
                task.status = if task.retry_count >= task.max_retries {
                    TaskStatus::DeadLetter
                } else {
                    TaskStatus::Pending
                };
                task.last_error =
                    Some("Task lease expired without the run reporting a result".to_string());
                task.run_at =
                    now + chrono::Duration::seconds(30 * (1 << task.retry_count.min(10)) as i64);
                task.worker_id = None;
                task.execution_generation = None;
                task.locked_at = None;
                task.lock_expires_at = None;
                reaped += 1;
            }
            Ok(reaped)
        }

        async fn claim_pending_tasks(
            &self,
            worker_id: Uuid,
            lock_expires_at: chrono::DateTime<chrono::Utc>,
            limit: i64,
        ) -> AppResult<Vec<BackgroundTask>> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            let mut claimed = Vec::new();
            for task in list
                .iter_mut()
                .filter(|task| task.status == TaskStatus::Pending && task.run_at <= now)
                .take(limit as usize)
            {
                task.status = TaskStatus::Processing;
                task.worker_id = Some(worker_id);
                task.execution_generation = Some(Uuid::new_v4());
                task.locked_at = Some(now);
                task.lock_expires_at = Some(lock_expires_at);
                claimed.push(task.clone());
            }
            Ok(claimed)
        }

        async fn claim_task(
            &self,
            id: Uuid,
            worker_id: Uuid,
            lock_expires_at: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            if let Some(t) = list
                .iter_mut()
                .find(|t| t.id == id && t.status == TaskStatus::Pending && t.run_at <= now)
            {
                t.status = TaskStatus::Processing;
                t.worker_id = Some(worker_id);
                t.locked_at = Some(now);
                t.lock_expires_at = Some(lock_expires_at);
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn mark_task_completed(&self, lease: TaskLeaseRef) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == lease.task_id
                    && t.status == TaskStatus::Processing
                    && t.worker_id == Some(lease.worker_id)
                    && t.lock_expires_at.is_some_and(|expires| expires > now)
            }) {
                t.status = TaskStatus::Completed;
                t.worker_id = None;
                t.locked_at = None;
                t.lock_expires_at = None;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn mark_task_failed(&self, failure: TaskFailure<'_>) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == failure.lease.task_id
                    && t.status == TaskStatus::Processing
                    && t.worker_id == Some(failure.lease.worker_id)
                    && t.lock_expires_at.is_some_and(|expires| expires > now)
            }) {
                t.last_error = Some(failure.error.to_string());
                t.retry_count += 1;
                t.run_at = failure.next_run_at;
                t.status = failure.outcome.status();
                t.worker_id = None;
                t.locked_at = None;
                t.lock_expires_at = None;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn stop_task(&self, id: Uuid, _actor: StopActor) -> AppResult<BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list
                .iter_mut()
                .find(|t| {
                    t.id == id
                        && matches!(
                            t.status,
                            TaskStatus::Pending
                                | TaskStatus::Processing
                                | TaskStatus::PendingApproval
                                | TaskStatus::WaitingForThirdPartyReply
                                | TaskStatus::Failed
                        )
                })
                .unwrap();
            t.status = TaskStatus::Stopped;
            t.worker_id = None;
            t.locked_at = None;
            t.lock_expires_at = None;
            Ok(t.clone())
        }

        async fn resume_task(&self, id: Uuid, _actor: ResumeActor) -> AppResult<BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list
                .iter_mut()
                .find(|t| {
                    t.id == id
                        && matches!(
                            t.status,
                            TaskStatus::Stopped
                                | TaskStatus::PendingApproval
                                | TaskStatus::WaitingForThirdPartyReply
                                | TaskStatus::Failed
                        )
                })
                .unwrap();
            t.status = TaskStatus::Pending;
            t.run_at = Utc::now();
            t.worker_id = None;
            t.locked_at = None;
            t.lock_expires_at = None;
            Ok(t.clone())
        }

        async fn list_company_tasks(
            &self,
            company_id: Uuid,
            _channel_id: Option<Uuid>,
            _status: Option<TaskStatus>,
            _sort_asc: bool,
        ) -> AppResult<Vec<BackgroundTask>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.company_id == company_id)
                .cloned()
                .collect())
        }
    }

    #[tokio::test]
    async fn bounded_task_loop_runs_up_to_its_limit_and_refills_finished_slots() {
        let remaining = Arc::new(AtomicUsize::new(8));
        let claim_sizes = Arc::new(Mutex::new(Vec::new()));
        let running = Arc::new(AtomicUsize::new(0));
        let maximum_running = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let permits = Arc::new(Semaphore::new(0));
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let claim_remaining = Arc::clone(&remaining);
        let observed_claim_sizes = Arc::clone(&claim_sizes);
        let execution_running = Arc::clone(&running);
        let execution_maximum = Arc::clone(&maximum_running);
        let execution_started = Arc::clone(&started);
        let execution_permits = Arc::clone(&permits);
        let driver = tokio::spawn(run_bounded_task_loop(
            Duration::from_secs(3600),
            shutdown_rx,
            4,
            move |available| {
                observed_claim_sizes.lock().unwrap().push(available);
                let count = claim_remaining
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                        Some(remaining.saturating_sub(available))
                    })
                    .unwrap()
                    .min(available);
                async move { Ok((0..count).collect::<Vec<_>>()) }
            },
            move |_, _shutdown| {
                let running = Arc::clone(&execution_running);
                let maximum = Arc::clone(&execution_maximum);
                let started = Arc::clone(&execution_started);
                let permits = Arc::clone(&execution_permits);
                async move {
                    let current = running.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(current, Ordering::SeqCst);
                    started.fetch_add(1, Ordering::SeqCst);
                    permits.acquire().await.unwrap().forget();
                    running.fetch_sub(1, Ordering::SeqCst);
                }
            },
        ));

        tokio::time::timeout(Duration::from_secs(1), async {
            while started.load(Ordering::SeqCst) < 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(running.load(Ordering::SeqCst), 4);

        permits.add_permits(1);
        tokio::time::timeout(Duration::from_secs(1), async {
            while started.load(Ordering::SeqCst) < 5 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        assert_eq!(maximum_running.load(Ordering::SeqCst), 4);
        assert_eq!(&*claim_sizes.lock().unwrap(), &[4, 1]);

        let _ = shutdown_tx.send(());
        permits.add_permits(8);
        driver.await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_interrupts_active_work_without_claiming_again() {
        let claim_calls = Arc::new(AtomicUsize::new(0));
        let interrupted = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Notify::new());
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let observed_claim_calls = Arc::clone(&claim_calls);
        let observed_interrupted = Arc::clone(&interrupted);
        let execution_started = Arc::clone(&started);
        let driver = tokio::spawn(run_bounded_task_loop(
            Duration::ZERO,
            shutdown_rx,
            1,
            move |_| {
                let call = observed_claim_calls.fetch_add(1, Ordering::SeqCst);
                async move { Ok(if call == 0 { vec![()] } else { Vec::new() }) }
            },
            move |_, mut shutdown| {
                let interrupted = Arc::clone(&observed_interrupted);
                let started = Arc::clone(&execution_started);
                async move {
                    started.notify_one();
                    let _ = shutdown.recv().await;
                    interrupted.fetch_add(1, Ordering::SeqCst);
                }
            },
        ));

        started.notified().await;
        shutdown_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), driver)
            .await
            .expect("task loop should drain promptly")
            .unwrap();

        assert_eq!(claim_calls.load(Ordering::SeqCst), 1);
        assert_eq!(interrupted.load(Ordering::SeqCst), 1);
    }

    /// The driver runs before it sleeps. A queue with something already in it at startup must be
    /// served now, not one interval from now — that delay was the whole cost of the old loop.
    #[tokio::test(start_paused = true)]
    async fn the_first_iteration_does_not_wait_for_the_interval() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let counter = Arc::clone(&calls);
        let driver = tokio::spawn(poll_until_shutdown(
            "test",
            Duration::from_secs(3600),
            shutdown_rx,
            counter,
            |calls| async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Polled::Idle)
            },
        ));

        // Only yields — no clock advance at all, so anything that ran did so before the sleep.
        tokio::task::yield_now().await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let _ = shutdown_tx.send(());
        driver.await.unwrap();
    }

    /// A full batch means more is behind it. Draining a backlog one interval at a time is what the
    /// old one-task-per-tick loop did, and it is what `MoreWaiting` exists to avoid.
    #[tokio::test(start_paused = true)]
    async fn a_full_batch_comes_straight_back_without_pausing() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let counter = Arc::clone(&calls);
        let driver = tokio::spawn(poll_until_shutdown(
            "test",
            Duration::from_secs(3600),
            shutdown_rx,
            counter,
            |calls| async move {
                // Three full batches, then the queue is empty.
                let seen = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(if seen < 2 {
                    Polled::MoreWaiting
                } else {
                    Polled::Idle
                })
            },
        ));

        // Still no clock advance: all three iterations have to fit before the first real pause.
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);

        let _ = shutdown_tx.send(());
        driver.await.unwrap();
    }

    /// A failing iteration backs off rather than retrying at queue cadence — at 500ms a database
    /// outage would otherwise log twice a second, per loop.
    #[tokio::test(start_paused = true)]
    async fn a_failed_iteration_backs_off_instead_of_spinning() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let counter = Arc::clone(&calls);
        let driver = tokio::spawn(poll_until_shutdown(
            "test",
            Duration::from_millis(500),
            shutdown_rx,
            counter,
            |calls| async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err("database is unreachable".to_string())
            },
        ));

        // One poll interval in, a spinning loop would already have run again.
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        tokio::time::sleep(ERROR_BACKOFF).await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);

        let _ = shutdown_tx.send(());
        driver.await.unwrap();
    }

    /// Every loop must come down on the shared shutdown broadcast, or the process hangs past its
    /// drain deadline waiting for a poller that is still sleeping.
    #[tokio::test(start_paused = true)]
    async fn shutdown_stops_a_loop_that_is_between_iterations() {
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let driver = tokio::spawn(poll_until_shutdown(
            "test",
            Duration::from_secs(3600),
            shutdown_rx,
            Arc::new(()),
            |_| async move { Ok(Polled::Idle) },
        ));

        tokio::task::yield_now().await;
        let _ = shutdown_tx.send(());

        // No clock advance: the loop must not have to wait out its hour-long pause to notice.
        driver.await.unwrap();
    }

    /// A run longer than one lease term must keep its claim, or the poller reclaims a task that is
    /// still being worked on and the same agent runs twice.
    #[tokio::test(start_paused = true)]
    async fn work_outliving_its_lease_term_keeps_renewing() {
        let persistence = MockTaskPersistence::default();
        let lease = TaskLease::worker(TaskLeaseRef {
            task_id: Uuid::new_v4(),
            worker_id: Uuid::new_v4(),
            execution_generation: Uuid::new_v4(),
        });

        // 1000s of work against a 900s lease: beats land at 300s, 600s and 900s.
        let outcome = while_leased(
            &persistence,
            &lease,
            tokio::time::sleep(Duration::from_secs(1000)),
        )
        .await;

        assert!(matches!(outcome, Leased::Finished(())));
        assert_eq!(*persistence.renewals.lock().unwrap(), 3);
    }

    /// Losing the lease means someone else owns the task now. The work must be dropped rather than
    /// left to finish and write a result over theirs.
    #[tokio::test(start_paused = true)]
    async fn a_lost_lease_abandons_the_work() {
        struct DropGuard(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let persistence = MockTaskPersistence {
            renewals_before_loss: Some(1),
            ..Default::default()
        };
        let lease = TaskLease::worker(TaskLeaseRef {
            task_id: Uuid::new_v4(),
            worker_id: Uuid::new_v4(),
            execution_generation: Uuid::new_v4(),
        });
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let ran = Arc::clone(&finished);
        let drop_flag = Arc::clone(&dropped);
        let outcome = while_leased(&persistence, &lease, async move {
            let _guard = DropGuard(drop_flag);
            tokio::time::sleep(Duration::from_secs(6000)).await;
            ran.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .await;

        assert!(matches!(outcome, Leased::Lost));
        assert!(
            !finished.load(std::sync::atomic::Ordering::SeqCst),
            "work must be dropped the moment the lease is gone"
        );
        assert!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            "lease loss must drop the provider future itself"
        );
    }

    #[tokio::test]
    async fn expired_lease_is_reclaimed_and_stale_worker_cannot_complete() {
        let persistence = MockTaskPersistence::default();
        let _task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                Uuid::new_v4(),
                Uuid::new_v4(),
                None,
                "email_agent_dispatch",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        let first_worker = Uuid::new_v4();
        let second_worker = Uuid::new_v4();

        let first_claim = persistence
            .claim_pending_tasks(first_worker, Utc::now() + chrono::Duration::minutes(1), 1)
            .await
            .unwrap();
        assert_eq!(first_claim.len(), 1);
        let first_lease = TaskLeaseRef::of(&first_claim[0]).expect("a claim records its lease");

        // The first run's lease lapses and the reaper hands the task back.
        persistence.tasks.lock().unwrap()[0].lock_expires_at =
            Some(Utc::now() - chrono::Duration::seconds(1));
        assert_eq!(persistence.reap_expired_task_leases().await.unwrap(), 1);

        {
            // Reaping costs an attempt and applies the backoff, which is the whole difference
            // from the old behaviour of stealing the row and re-running it immediately.
            let reaped = &persistence.tasks.lock().unwrap()[0];
            assert_eq!(reaped.status, TaskStatus::Pending);
            assert_eq!(reaped.retry_count, 1);
            assert!(reaped.worker_id.is_none());
            assert!(reaped.execution_generation.is_none());
            assert!(
                reaped.run_at > Utc::now(),
                "an expired lease must back off rather than be retried at once"
            );
        }

        // Let the backoff elapse.
        persistence.tasks.lock().unwrap()[0].run_at = Utc::now() - chrono::Duration::seconds(1);

        let second_claim = persistence
            .claim_pending_tasks(second_worker, Utc::now() + chrono::Duration::minutes(1), 1)
            .await
            .unwrap();
        assert_eq!(second_claim.len(), 1);
        assert_eq!(second_claim[0].worker_id, Some(second_worker));
        let second_lease = TaskLeaseRef::of(&second_claim[0]).expect("a claim records its lease");
        assert_ne!(
            first_lease.execution_generation, second_lease.execution_generation,
            "each claim must mint its own generation, or the fence cannot tell the runs apart"
        );

        // The abandoned run cannot close out the task the replacement now owns.
        assert!(!persistence.mark_task_completed(first_lease).await.unwrap());
        assert!(persistence.mark_task_completed(second_lease).await.unwrap());
    }

    #[tokio::test]
    async fn test_task_worker_stop_and_resume_flow() {
        let task_persistence = Arc::new(MockTaskPersistence::default());
        let thread_persistence = Arc::new(InMemoryThreads::new());
        let company_persistence = Arc::new(MockCompanyPersistence { company: None, ..Default::default() });
        let channel_persistence = Arc::new(MockChannelPersistence { channel: None });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        });

        let thread_use_cases = Arc::new(ThreadUseCases::for_test(
            thread_persistence,
            channel_persistence,
            company_persistence.clone(),
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
            task_persistence.clone(),
            config.clone(),
        ));

        let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases, config);

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let task = task_persistence
            .enqueue_task(NewTask::starting_new_chain(
                company_id,
                channel_id,
                None,
                "email_agent_dispatch",
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(task.status, TaskStatus::Pending);

        // Stop task
        worker
            .stop_task_and_notify(task.id, StopActor::Operator(Uuid::new_v4()))
            .await
            .unwrap();
        let stopped_task = task_persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stopped_task.status, TaskStatus::Stopped);

        // Resume task
        worker
            .resume_task(task.id, ResumeActor::Operator(Uuid::new_v4()))
            .await
            .unwrap();
        let resumed_task = task_persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resumed_task.status, TaskStatus::Pending);
    }

    #[tokio::test]
    async fn test_task_worker_marks_task_failed_on_agent_runner_failure() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();

        let task_persistence = Arc::new(MockTaskPersistence::default());
        let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

        let company = crate::entities::company::Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Corp".to_string(),
            slug: "test".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            owner_agent_id: None,
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            agent_ids: Some(vec![agent_id]),
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };

        let company_persistence = Arc::new(MockCompanyPersistence {
            company: Some(company.clone()),
            ..Default::default()
        });
        let channel_persistence = Arc::new(MockChannelPersistence {
            channel: Some(channel.clone()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        });

        let thread_use_cases = Arc::new(
            ThreadUseCases::for_test(
                thread_persistence.clone(),
                channel_persistence,
                company_persistence.clone(),
                Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
                task_persistence.clone(),
                config.clone(),
            )
            .with_agent_persistence(Arc::new(MockAgentPersistence {
                agent: active_agent(company_id, agent_id),
            })),
        );

        let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases, config);

        // The task carries ids; the message it answers is in the store. Building the fixture the
        // way ingest builds it is the point -- a payload assembled by hand could not fail the way
        // a real one does.
        thread_persistence.insert_thread(crate::entities::thread::Thread {
            id: thread_id,
            channel_id,
            subject: "Help".to_string(),
            participant_principal_ids: Vec::new(),
            participant_projection: crate::entities::thread::ThreadParticipantProjection {
                identities: vec![
                    crate::adapters::protocols::email::EmailIdentity::parse("user@test.com".into())
                        .unwrap()
                        .qualify_default(),
                ],
            },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
        let stored = thread_persistence
            .create_message(&email_write(EmailMessageDraft {
                thread_id,
                message_id: "<msg1@test.com>".into(),
                sender: "user@test.com".into(),
                recipients_to: vec!["support@test.mailagents.com".into()],
                subject: "Help".to_string(),
                clean_text_body: "Need help".to_string(),
                ..EmailMessageDraft::default()
            }))
            .await
            .unwrap();

        let payload_json = InboundTaskPayload::v1(crate::transport::InboundTaskPayloadV1 {
            company_id,
            channel_id,
            thread_id,
            source_message_id: stored.canonical_id,
            correlation_id: crate::entities::correlation::CorrelationId::new(),
            hop_count: 0,
            trace_channels: crate::transport::BoundedVec::empty(),
            is_forwarded: false,
            reply_delivery: crate::use_cases::thread::ReplyDelivery::Send,
        })
        .encode()
        .unwrap();
        let task = task_persistence
            .enqueue_task(NewTask::starting_new_chain(
                company_id,
                channel_id,
                Some(thread_id),
                "email_agent_dispatch",
                payload_json,
            ))
            .await
            .unwrap();

        worker.process_next_task_batch().await.unwrap();

        let failed_task = task_persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failed_task.status, TaskStatus::Pending);
        assert_eq!(failed_task.retry_count, 1);
        assert!(
            failed_task
                .last_error
                .unwrap()
                .contains("API key is missing")
        );
    }

    #[tokio::test]
    async fn a_scheduled_run_without_an_api_key_fails_the_task_for_retry() {
        let task_persistence = Arc::new(MockTaskPersistence::default());
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();

        let company = Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Company".to_string(),
            slug: "test-co".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            owner_agent_id: None,
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Audit Channel".to_string(),
            description: None,
            slug: "audit".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            agent_ids: Some(vec![agent_id]),
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };

        let thread_persistence = Arc::new(InMemoryThreads::new());
        let company_persistence = Arc::new(MockCompanyPersistence {
            company: Some(company.clone()),
            ..Default::default()
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        });

        let thread_use_cases = Arc::new(
            ThreadUseCases::for_test(
                thread_persistence.clone(),
                Arc::new(MockChannelPersistence {
                    channel: Some(channel.clone()),
                }),
                company_persistence.clone(),
                Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
                task_persistence.clone(),
                config.clone(),
            )
            .with_agent_persistence(Arc::new(MockAgentPersistence {
                agent: active_agent(company_id, agent_id),
            })),
        );

        let worker = Arc::new(TaskWorker::new(
            task_persistence.clone(),
            thread_use_cases,
            config,
        ));

        let scheduled_payload = serde_json::json!({
            "schedule_id": Uuid::new_v4(),
            "schedule_name": "Nightly Audit",
            "channel_id": channel_id,
            "company_id": company_id,
            "thread_id": thread_id,
            "subject": "Audit Report",
            "prompt": "Run audit",
            "delivery_mode": "mailbox_only",
            "recipient_emails": [],
            "run_key": Uuid::new_v4(),
            "prompt_message_id": Uuid::new_v4(),
        });

        let task = task_persistence
            .enqueue_task(NewTask::starting_new_chain(
                company_id,
                channel_id,
                Some(thread_id),
                "scheduled_agent_run",
                scheduled_payload,
            ))
            .await
            .unwrap();

        worker.process_next_task_batch().await.unwrap();

        let processed = task_persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();

        // Reached `execute_scheduled_task` -- and failed there for a reason the payload parse
        // would have pre-empted, so this also proves the typed payload accepted a real one.
        assert_eq!(processed.status, TaskStatus::Pending);
        assert_eq!(processed.retry_count, 1);
        assert!(processed.last_error.unwrap().contains("API key is missing"));
    }

    /// A retry must not run the agent again. The guard finds the answer a previous attempt saved
    /// and goes straight to delivery -- which is what may have failed the first time.
    #[tokio::test]
    async fn a_retried_scheduled_run_reuses_its_answer_instead_of_calling_the_agent_twice() {
        let task_persistence = Arc::new(MockTaskPersistence::default());
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();
        let trigger = MessageId::new("<TRIGGER123@domain.com>");
        let agent_id = Uuid::new_v4();

        let company = Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Company".to_string(),
            slug: "test-co".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            owner_agent_id: None,
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Audit Channel".to_string(),
            description: None,
            slug: "audit".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            agent_ids: Some(vec![agent_id]),
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };

        let thread_persistence = Arc::new(InMemoryThreads::new());
        thread_persistence.insert_thread(Thread {
            id: thread_id,
            channel_id,
            subject: "Audit Report".into(),
            participant_principal_ids: Vec::new(),
            participant_projection: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
        // The schedule's own prompt, and the answer a previous attempt already saved after it.
        let prompt = thread_persistence
            .create_message(&MessageWrite::internal(
                thread_id,
                MessageAuthorWrite::Platform,
                "Audit Report",
                "Run audit",
                MessageDirection::Inbound,
                MessageRole::System,
                CorrelationId::new(),
            ))
            .await
            .unwrap();
        thread_persistence
            .create_message(&email_write(EmailMessageDraft {
                thread_id,
                message_id: MessageId::new("<already-answered@domain.com>"),
                in_reply_to: Some(trigger.clone()),
                references_list: vec![trigger.clone()],
                sender: EmailAddress::from("audit@test-co.mailagents.com"),
                subject: "Re: Audit Report".into(),
                clean_text_body: "Audit complete: nothing to report.".into(),
                direction: MessageDirection::Outbound,
                role: MessageRole::Agent,
                created_at: chrono::Utc::now() + chrono::Duration::seconds(1),
                ..Default::default()
            }))
            .await
            .unwrap();
        let company_persistence = Arc::new(MockCompanyPersistence {
            company: Some(company.clone()),
            ..Default::default()
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        });

        let thread_use_cases = Arc::new(
            ThreadUseCases::for_test(
                thread_persistence.clone(),
                Arc::new(MockChannelPersistence {
                    channel: Some(channel.clone()),
                }),
                company_persistence.clone(),
                Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
                task_persistence.clone(),
                config.clone(),
            )
            .with_agent_persistence(Arc::new(MockAgentPersistence {
                agent: active_agent(company_id, agent_id),
            })),
        );

        let worker = Arc::new(TaskWorker::new(
            task_persistence.clone(),
            thread_use_cases,
            config,
        ));

        let task = task_persistence
            .enqueue_task(NewTask::starting_new_chain(
                company_id,
                channel_id,
                Some(thread_id),
                SCHEDULED_AGENT_RUN_TASK,
                serde_json::json!({
                    "schedule_id": Uuid::new_v4(),
                    "schedule_name": "Nightly Audit",
                    "channel_id": channel_id,
                    "company_id": company_id,
                    "thread_id": thread_id,
                    "subject": "Audit Report",
                    "prompt": "Run audit",
                    "delivery_mode": "email_custom",
                    "recipient_emails": ["ops@example.com", "cc@example.com"],
                    "run_key": Uuid::new_v4(),
                    "prompt_message_id": prompt.canonical_id,
                }),
            ))
            .await
            .unwrap();

        worker.process_next_task_batch().await.unwrap();

        let processed = task_persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();

        // No API-key failure: the agent was never reached, even though this company has no key.
        assert_eq!(
            processed.status,
            TaskStatus::Completed,
            "last_error: {:?}",
            processed.last_error
        );

        // The prompt and one answer -- the retry appended nothing.
        assert_eq!(
            thread_persistence.messages().len(),
            2,
            "a retry must not append a second answer"
        );

        // Delivery still ran, addressed from the schedule's own recipient list -- and it was
        // queued on its own rather than with a second copy of the answer, because the answer was
        // already there.
        let queued = task_persistence.queued_deliveries.lock().unwrap();
        assert_eq!(queued.len(), 1);
        let email = frozen_email(&queued[0]);
        assert_eq!(
            email.recipients_to,
            vec![EmailAddress::from("ops@example.com")]
        );
        assert_eq!(
            email.recipients_cc,
            vec![EmailAddress::from("cc@example.com")]
        );
        assert_eq!(
            email.body_text,
            "Audit complete: nothing to report.\n\nDone by busybots.net"
        );
        assert_eq!(email.subject, "Re: Audit Report");
        // Keyed on the task, so the retry that skipped the agent re-derived the first attempt's
        // key rather than mailing the digest twice.
        assert!(
            queued[0]
                .idempotency_key
                .as_str()
                .contains("scheduled-email"),
            "{}",
            queued[0].idempotency_key
        );
    }

    #[tokio::test]
    async fn a_successful_scheduled_run_records_execution_parameters_and_result() {
        use crate::services::test_support::{
            scripted_agent_config, scripted_llm, LlmTurn, SCRIPTED_MODEL, SCRIPTED_PROVIDER,
        };
        let _llm = scripted_llm(vec![LlmTurn::text("Audit complete: all good.")]).await;

        let task_persistence = Arc::new(MockTaskPersistence::default());
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();

        let company = Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Company".to_string(),
            slug: "test-co".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            owner_agent_id: None,
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Audit Channel".to_string(),
            description: None,
            slug: "audit".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            agent_ids: Some(vec![agent_id]),
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };

        let thread_persistence = Arc::new(InMemoryThreads::new());
        thread_persistence.insert_thread(Thread {
            id: thread_id,
            channel_id,
            subject: "Audit Report".into(),
            participant_principal_ids: Vec::new(),
            participant_projection: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
        let prompt = thread_persistence
            .create_message(&MessageWrite::internal(
                thread_id,
                MessageAuthorWrite::Platform,
                "Audit Report",
                "Run audit",
                MessageDirection::Inbound,
                MessageRole::System,
                CorrelationId::new(),
            ))
            .await
            .unwrap();

        let company_persistence = Arc::new(MockCompanyPersistence {
            company: Some(company.clone()),
            model_api_key: Some("test-key".to_string()),
            model_connections: vec![crate::entities::company::CompanyModelConnection {
                provider: SCRIPTED_PROVIDER.into(),
                models: vec![SCRIPTED_MODEL.into()],
                is_default: true,
                has_api_key: true,
            }],
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        });

        let mut agent = active_agent(company_id, agent_id);
        agent.config_json = Some(scripted_agent_config(&_llm.base_url));

        let thread_use_cases = Arc::new(
            ThreadUseCases::for_test(
                thread_persistence.clone(),
                Arc::new(MockChannelPersistence {
                    channel: Some(channel.clone()),
                }),
                company_persistence.clone(),
                Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
                task_persistence.clone(),
                config.clone(),
            )
            .with_agent_persistence(Arc::new(MockAgentPersistence { agent })),
        );

        let worker = Arc::new(TaskWorker::new(
            task_persistence.clone(),
            thread_use_cases,
            config,
        ));

        let scheduled_payload = serde_json::json!({
            "schedule_id": Uuid::new_v4(),
            "schedule_name": "Nightly Audit",
            "channel_id": channel_id,
            "company_id": company_id,
            "thread_id": thread_id,
            "subject": "Audit Report",
            "prompt": "Run audit",
            "delivery_mode": "mailbox_only",
            "recipient_emails": [],
            "run_key": Uuid::new_v4(),
            "prompt_message_id": prompt.canonical_id,
        });

        let task = task_persistence
            .enqueue_task(NewTask::starting_new_chain(
                company_id,
                channel_id,
                Some(thread_id),
                SCHEDULED_AGENT_RUN_TASK,
                scheduled_payload,
            ))
            .await
            .unwrap();

        worker.process_next_task_batch().await.unwrap();

        let processed = task_persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            processed.status,
            TaskStatus::Completed,
            "last_error: {:?}",
            processed.last_error
        );

        // Verify execution_parameters in payload
        let exec_params = processed
            .payload
            .get("execution_parameters")
            .expect("execution_parameters must be present");
        assert_eq!(
            exec_params.get("agent_name").and_then(|v| v.as_str()),
            Some("Test agent")
        );
        assert_eq!(
            exec_params.get("prompt").and_then(|v| v.as_str()),
            Some("Run audit")
        );
        assert!(exec_params.get("executed_at").is_some());

        // Verify execution_result in payload
        let exec_result = processed
            .payload
            .get("execution_result")
            .expect("execution_result must be present");
        assert_eq!(
            exec_result.get("response").and_then(|v| v.as_str()),
            Some("Audit complete: all good.")
        );
        assert!(exec_result.get("reply_message_id").is_some());
        assert!(exec_result.get("outbound_message_id").is_some());
        assert_eq!(
            exec_result.get("email_sent").and_then(|v| v.as_bool()),
            Some(false)
        );

        // Verify token_usage is present on BackgroundTask
        let token_usage = processed
            .token_usage()
            .expect("token_usage must be readable on BackgroundTask");
        assert!(token_usage.total_tokens > 0);
    }

    /// The one part an email delivery freezes, decoded back into the adapter's own shape.
    fn frozen_email(delivery: &NewDelivery) -> crate::adapters::protocols::email::OutboundEmailV1 {
        delivery.parts[0]
            .payload
            .decode(
                crate::entities::transport::TransportKind::Email,
                crate::adapters::protocols::email::OUTBOUND_EMAIL_VERSION,
            )
            .expect("the email renderer froze this part")
    }

    #[test]
    fn a_reply_subject_does_not_stack_prefixes() {
        assert_eq!(reply_subject("Audit Report"), "Re: Audit Report");
        assert_eq!(reply_subject("Re: Audit Report"), "Re: Audit Report");
        assert_eq!(reply_subject("RE: Audit Report"), "RE: Audit Report");
    }

    /// A schedule's mail threads onto its own run slot, and says so with a key rather than with a
    /// header it invented.
    ///
    /// The anchor is what a recipient's client files every firing of one schedule under, so it has
    /// to be identical across retries of the same firing and different across schedules. It is
    /// deliberately not an RFC `Message-ID`: the email adapter turns it into one, and a schedule
    /// that one day delivers over Slack hands the same key to a renderer with no use for it.
    #[test]
    fn a_scheduled_run_threads_onto_its_slot_rather_than_an_invented_header() {
        let payload = scheduled_payload();
        assert_eq!(
            scheduled_run_anchor(&payload),
            scheduled_run_anchor(&payload)
        );
        assert!(
            scheduled_run_anchor(&payload)
                .as_str()
                .contains(&payload.run_key.to_string()),
            "the anchor names the run slot, not the prompt message"
        );
        assert!(
            !scheduled_run_anchor(&payload).as_str().contains('<'),
            "an anchor is not a Message-ID; only the email renderer may make one"
        );

        let other = ScheduledRunPayload {
            run_key: Uuid::new_v4(),
            ..scheduled_payload()
        };
        assert_ne!(
            scheduled_run_anchor(&payload),
            scheduled_run_anchor(&other),
            "two schedules must not share one conversation"
        );
    }

    /// The payload carries the run's own identity, not a mail header. A run delivered over
    /// another transport changes nothing here.
    #[test]
    fn a_scheduled_payload_carries_no_transport_identifier() {
        let encoded = serde_json::to_value(scheduled_payload()).unwrap();
        assert!(encoded.get("run_key").is_some());
        assert!(encoded.get("trigger_message_id").is_none());
        assert!(
            !encoded.to_string().contains('<'),
            "no RFC id may appear in a scheduled payload: {encoded}"
        );
    }

    fn scheduled_payload() -> ScheduledRunPayload {
        ScheduledRunPayload {
            schedule_run_id: None,
            schedule_id: Uuid::new_v4(),
            schedule_name: "Nightly Audit".into(),
            channel_id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            subject: "Audit Report".into(),
            prompt: "Run audit".into(),
            delivery_mode: crate::entities::schedule::ScheduleDeliveryMode::MailboxOnly,
            recipient_emails: Vec::new(),
            run_as: None,
            run_key: Uuid::new_v4(),
            prompt_message_id: crate::entities::message::CanonicalMessageId::random(),
        }
    }
}
