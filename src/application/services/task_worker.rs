use std::future::Future;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{Duration, sleep};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    adapters::persistence::task::{
        Leased, OutboundSend, OutboxEmail, TASK_LEASE_SECONDS, TaskLease, TaskPersistence,
        report_outcome, while_leased,
    },
    domain::monitoring::{MonitoringService, TaskExecutionMetrics, TaskStatusMetric},
    entities::{
        agent::Agent,
        channel::{Channel, PUBLIC_PARTICIPANT},
        company::Company,
        message::{Message, MessageDirection, MessageRole},
        outreach::DueOutreach,
        schedule::ScheduledRunPayload,
        task::{BackgroundTask, TaskAttemptOutcome, TaskAttemptRef, TaskAttemptStatus},
        value_objects::{EmailAddress, MessageId},
    },
    infra::config::AppConfig,
    services::{
        agent_runner::{AgentRunner, ResolvedAgentParams},
        outbound_dispatcher::{OutboundDispatcher, OutboundEmail, agent_response_email_body},
    },
    use_cases::{
        schedule::{SCHEDULED_AGENT_RUN_TASK, ScheduleUseCases},
        thread::{InboundIngestResult, ThreadUseCases},
    },
};

/// How long the task loop waits before looking for work again. This is the whole delay between a
/// message being ingested and its agent starting, so it is short: the claim is one index scan
/// against `background_tasks_pending_ready_idx`, which costs nothing to run twice a second against
/// an empty queue.
const TASK_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long the outbox loop waits. It runs on its own cadence rather than behind an agent run, so a
/// queued reply goes out within half a second instead of whenever the current task happens to end.
const OUTBOX_POLL_INTERVAL: Duration = Duration::from_millis(500);

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

/// How many tasks one iteration claims. One, because the run happens inline on the loop — running
/// two agents at once is a different decision from how often to look for one.
const TASK_CLAIM_BATCH: i64 = 1;

/// How many queued emails one iteration claims.
const OUTBOX_CLAIM_BATCH: i64 = 10;

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
/// Shutdown is only observed between iterations. That is deliberate: an agent run cut off midway
/// through writing its result is worse than one that finishes while the process is winding down —
/// the lease is what protects a run that outlives its process, not cancellation here.
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

/// What a scheduled run needs loaded before the agent can answer: the company and channel it runs
/// as, and the agent configured on that channel, if any.
struct ScheduledRunContext {
    company: Company,
    channel: Channel,
    agent: Option<Agent>,
}

/// The Message-ID of a scheduled run's reply, derived from the task so a retry reuses it and the
/// saved message and the emailed copy always agree.
fn scheduled_reply_message_id(task_id: Uuid, domain: &str) -> MessageId {
    MessageId::new(format!("<schedule-reply-{task_id}@{domain}>"))
}

/// `Re:` a subject without stacking a second prefix.
fn reply_subject(subject: &str) -> String {
    if subject.trim_start().to_lowercase().starts_with("re:") {
        subject.to_string()
    } else {
        format!("Re: {subject}")
    }
}

/// Why one outbox delivery did not happen, and therefore whether trying again could help.
enum DeliveryFailure {
    /// Transport or database trouble: costs an attempt, comes back after a backoff.
    Retryable(String),
    /// Nothing about a later attempt would be different, so spend none of them.
    Permanent(String),
}

impl DeliveryFailure {
    fn retryable(error: impl std::fmt::Display) -> Self {
        Self::Retryable(error.to_string())
    }
}

pub struct TaskWorker {
    task_persistence: Arc<dyn TaskPersistence>,
    thread_use_cases: Arc<ThreadUseCases>,
    schedule_use_cases: Option<Arc<ScheduleUseCases>>,
    config: Arc<AppConfig>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    worker_id: uuid::Uuid,
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

    /// Run the worker's poll loops until shutdown.
    ///
    /// They are separate on purpose. A task run holds its loop for as long as the agent takes --
    /// seconds, sometimes minutes — and while the outbox shared that tick, no reply went out for
    /// the whole of it. Maintenance is split off for the opposite reason: its deadlines are minutes
    /// away, so it must not be re-run every time the queue loops look.
    pub async fn start_worker_loop(
        self: Arc<Self>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) {
        info!(
            "Starting Background Task Worker (tasks every {:?}, outbox every {:?}, schedules every {:?}, maintenance every {:?})...",
            TASK_POLL_INTERVAL, OUTBOX_POLL_INTERVAL, SCHEDULE_POLL_INTERVAL, MAINTENANCE_INTERVAL
        );

        let tasks = tokio::spawn(poll_until_shutdown(
            "task",
            TASK_POLL_INTERVAL,
            shutdown_rx.resubscribe(),
            Arc::clone(&self),
            |worker| async move { worker.process_next_task_batch().await },
        ));
        let outbox = tokio::spawn(poll_until_shutdown(
            "outbox",
            OUTBOX_POLL_INTERVAL,
            shutdown_rx.resubscribe(),
            Arc::clone(&self),
            |worker| async move { worker.process_outbox_emails().await },
        ));
        let schedules = if self.schedule_use_cases.is_some() {
            tokio::spawn(poll_until_shutdown(
                "schedule",
                SCHEDULE_POLL_INTERVAL,
                shutdown_rx.resubscribe(),
                Arc::clone(&self),
                |worker| async move { worker.process_due_schedules().await },
            ))
        } else {
            tokio::spawn(async {})
        };
        let maintenance = tokio::spawn(poll_until_shutdown(
            "maintenance",
            MAINTENANCE_INTERVAL,
            shutdown_rx,
            self,
            |worker| async move { worker.run_maintenance().await },
        ));

        let _ = tokio::join!(tasks, outbox, schedules, maintenance);
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

    /// Claim and run the next due task.
    async fn process_next_task_batch(&self) -> Result<Polled, String> {
        let tasks = self
            .task_persistence
            .claim_pending_tasks(
                self.worker_id,
                chrono::Utc::now() + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                TASK_CLAIM_BATCH,
            )
            .await
            .map_err(|e| e.to_string())?;

        let claimed = tasks.len();
        for task in tasks {
            info!("Processing task {} (type = '{}')", task.id, task.task_type);
            let start_time = std::time::Instant::now();
            let attempt = TaskAttemptRef::current(&task);
            let result = self.execute_single_task_with_lease(&task, attempt).await;
            let duration_ms = start_time.elapsed().as_millis() as u64;
            self.close_out_task(&task, attempt, result, duration_ms)
                .await;
        }

        Ok(polled(claimed, TASK_CLAIM_BATCH))
    }

    /// The slow lane: work whose deadlines are minutes away, kept off the queue loops so that
    /// shortening their interval does not multiply it.
    async fn run_maintenance(&self) -> Result<Polled, String> {
        // Give back the outbox rows whose delivery never reported a result, so each of those
        // redeliveries costs an attempt and a poison row eventually dead-letters.
        match self.task_persistence.reap_expired_outbox_leases().await {
            Ok(0) => {}
            Ok(reaped) => warn!(
                "Reaped {} outbox deliveries whose lease had expired",
                reaped
            ),
            Err(error) => warn!("Failed to reap expired outbox leases: {}", error),
        }

        self.check_quorum_timeouts().await?;
        Ok(Polled::Idle)
    }

    /// Record the fate of one executed task: completed, suspended awaiting someone else, or failed
    /// and scheduled for retry.
    async fn close_out_task(
        &self,
        task: &BackgroundTask,
        attempt: TaskAttemptRef,
        result: Result<(), String>,
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

        let err_msg = match result {
            Ok(()) => {
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
                    None,
                )
                .await;
                let outcome = self
                    .task_persistence
                    .mark_task_completed(task_id, self.worker_id)
                    .await;
                report_outcome("Task", task_id, "completion", outcome);
                self.record_task_metric(task, duration_ms, TaskStatusMetric::Completed);
                return;
            }
            Err(err_msg) => err_msg,
        };

        warn!("Failed background task {}: {}", task_id, err_msg);
        self.close_out_attempt(
            task,
            attempt,
            current.as_ref(),
            TaskAttemptStatus::Failed,
            Some(err_msg.clone()),
        )
        .await;
        let next_retry = task.retry_count + 1;
        let is_dead_letter = next_retry >= task.max_retries;
        // Exponential backoff: 30s * 2^retry, capped so the shift can't overflow.
        let backoff_secs = 30 * (1 << next_retry.min(10));
        let next_run = chrono::Utc::now() + chrono::Duration::seconds(backoff_secs);

        let outcome = self
            .task_persistence
            .mark_task_failed(task_id, self.worker_id, &err_msg, next_run, is_dead_letter)
            .await;
        report_outcome("Task", task_id, "failure", outcome);
        self.record_task_metric(task, duration_ms, TaskStatusMetric::Failed);
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
        error: Option<String>,
    ) {
        let outcome = TaskAttemptOutcome {
            attempt,
            status,
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
    ) {
        if let Some(ref m) = self.monitoring {
            m.record_task_execution(&TaskExecutionMetrics {
                company_id: Some(task.company_id),
                channel_id: Some(task.channel_id),
                task_type: task.task_type.clone(),
                duration_ms,
                status,
                retry_count: task.retry_count as u32,
            });
        }
    }

    /// Deliver the next batch of queued emails. Reaping expired delivery leases belongs to
    /// [`Self::run_maintenance`], which runs on the lease's own timescale rather than this one.
    async fn process_outbox_emails(&self) -> Result<Polled, String> {
        let emails = self
            .task_persistence
            .claim_outbox_emails(
                self.worker_id,
                chrono::Utc::now() + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                OUTBOX_CLAIM_BATCH,
            )
            .await
            .map_err(|error| error.to_string())?;

        let claimed = emails.len();
        for queued in emails {
            let outbox_id = queued.id;
            // The outreach behind this email may have been answered or cancelled since it queued.
            match self
                .task_persistence
                .is_outbox_delivery_active(outbox_id)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    let outcome = self
                        .task_persistence
                        .cancel_claimed_outbox(outbox_id, self.worker_id)
                        .await;
                    report_outcome("Outbox", outbox_id, "cancellation", outcome);
                    continue;
                }
                // Unknown is not closed: fail the attempt so it comes back with backoff, rather
                // than cancelling an email the outreach may still be waiting on.
                Err(error) => {
                    warn!(
                        "Could not tell whether outbox {} is still wanted: {}",
                        outbox_id, error
                    );
                    self.record_outbox_failure(outbox_id, &error.to_string())
                        .await;
                    continue;
                }
            }

            match self.deliver_outbox_email(queued).await {
                Ok(sent_message_id) => {
                    let outcome = self
                        .task_persistence
                        .mark_outbox_email_sent(outbox_id, self.worker_id, &sent_message_id)
                        .await;
                    report_outcome("Outbox", outbox_id, "delivery", outcome);
                }
                Err(DeliveryFailure::Retryable(error)) => {
                    self.record_outbox_failure(outbox_id, &error).await;
                }
                Err(DeliveryFailure::Permanent(error)) => {
                    warn!("Outbox {} can never be delivered: {}", outbox_id, error);
                    let outcome = self
                        .task_persistence
                        .mark_outbox_email_dead(outbox_id, self.worker_id, &error)
                        .await;
                    report_outcome("Outbox", outbox_id, "dead-lettering", outcome);
                }
            }
        }
        Ok(polled(claimed, OUTBOX_CLAIM_BATCH))
    }

    /// End this delivery attempt: back off and retry, or dead-letter once the attempts run out.
    async fn record_outbox_failure(&self, outbox_id: Uuid, error: &str) {
        let outcome = self
            .task_persistence
            .mark_outbox_email_failed(outbox_id, self.worker_id, error)
            .await;
        report_outcome("Outbox", outbox_id, "failure", outcome);
    }

    /// Send one queued outreach email, preferring the trusted internal transport when the
    /// recipient is another platform channel. Returns the delivered Message-ID.
    async fn deliver_outbox_email(
        &self,
        queued: OutboxEmail,
    ) -> Result<MessageId, DeliveryFailure> {
        let outbox_id = queued.id;
        // Derive from the row's own key, not from its id: the key is stable across every attempt
        // *and* known to whoever queued the row, so a queuer can persist the outbound message
        // before delivery and still match the Message-ID that eventually goes out.
        let idempotency_key = queued.idempotency_key.clone();
        // A payload that will not deserialize will not deserialize on the fifth attempt either.
        let email: OutboundEmail = serde_json::from_value(queued.payload)
            .map_err(|error| DeliveryFailure::Permanent(error.to_string()))?;

        let internal = self
            .thread_use_cases
            .prepare_internal_channel_delivery(email.clone(), Some(&idempotency_key))
            .await
            .map_err(DeliveryFailure::retryable)?;

        if let Some(sent) = internal {
            self.thread_use_cases
                .record_outreach_outbound_message(outbox_id, &sent)
                .await
                .map_err(DeliveryFailure::retryable)?;
            let ingest = self
                .thread_use_cases
                .ingest_prepared_internal_message(&sent)
                .await
                .map_err(DeliveryFailure::retryable)?;
            if !ingest.accepted
                && ingest.reason.as_deref() != Some("Duplicate Message-ID already processed")
            {
                return Err(DeliveryFailure::Retryable(ingest.reason.unwrap_or_else(
                    || "Internal channel delivery was rejected".into(),
                )));
            }
            info!(
                "Delivered outreach outbox {} through trusted internal channel transport",
                outbox_id
            );
            return Ok(sent.outbound_message_id);
        }

        let sent = OutboundDispatcher::send_idempotent(&self.config, email, &idempotency_key)
            .await
            .map_err(DeliveryFailure::retryable)?;
        // The email is out; failing to log it in the thread must not re-send it.
        if let Err(error) = self
            .thread_use_cases
            .record_outreach_outbound_message(outbox_id, &sent)
            .await
        {
            warn!(
                "Failed to record sent outreach outbox {} in thread history: {}",
                outbox_id, error
            );
        }
        Ok(sent.outbound_message_id)
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

        let ingest: InboundIngestResult =
            serde_json::from_value(task.payload.clone()).map_err(|error| error.to_string())?;
        let first_match = ingest.channel_matches.first();
        let channel = ingest
            .channel
            .or_else(|| first_match.map(|matched| matched.channel.clone()));
        let company = ingest
            .company
            .or_else(|| first_match.map(|matched| matched.company.clone()));
        let (Some(channel), Some(company)) = (channel, company) else {
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
        if !self
            .task_persistence
            .mark_outreach_timeout_pending(outreach.outreach_id)
            .await
            .unwrap_or(false)
        {
            return Ok(());
        }
        info!(
            "Task {} reached quorum timeout with {:.1}% responses (< {:.1}% required)",
            outreach.task_id, current_percent, outreach.required_threshold_percent
        );

        if let Err(error) = approval_use_cases
            .create_and_send_approval_request(
                task.company_id,
                task.channel_id,
                &channel.name,
                &channel.slug,
                &company.slug,
                task.thread_id,
                Some(task.id),
                &format!(
                    "quorum_timeout_{}_{}",
                    outreach.outreach_id,
                    outreach.expires_at.timestamp()
                ),
                &approver_email,
                "quorum_timeout",
                "Partial Quorum Timeout: Action Required",
                &format!(
                    "Outreach timed out with {}/{} responses ({:.1}%). Required: {:.1}%.",
                    outreach.response_count,
                    outreach.target_count,
                    current_percent,
                    outreach.required_threshold_percent
                ),
                serde_json::json!({
                    "outreach_id": outreach.outreach_id,
                    "current_percent": current_percent,
                    "required_percent": outreach.required_threshold_percent,
                    "current_count": outreach.response_count,
                    "total_targets": outreach.target_count,
                }),
            )
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
    async fn approver_for(&self, channel: &Channel, company_id: Uuid) -> Option<EmailAddress> {
        let approver = match channel.preferred_approver() {
            Some(approver) => Some(approver),
            None => self
                .thread_use_cases
                .company_persistence()
                .list_company_team_emails(company_id)
                .await
                .unwrap_or_default()
                .into_iter()
                .next()
                .map(EmailAddress::from),
        };
        approver.filter(|email| !email.is_empty())
    }

    async fn execute_single_task(
        &self,
        task: &crate::entities::task::BackgroundTask,
    ) -> Result<(), String> {
        if task.task_type == SCHEDULED_AGENT_RUN_TASK {
            return self.execute_scheduled_task(task).await;
        }

        // Parse payload
        let mut ingest: InboundIngestResult = serde_json::from_value(task.payload.clone())
            .map_err(|e| format!("Invalid task payload JSON: {}", e))?;

        self.thread_use_cases
            .hydrate_ingest_configuration(&mut ingest)
            .await
            .map_err(|e| e.to_string())?;

        if !ingest.accepted {
            return Ok(());
        }

        let inbound_msg = ingest
            .inbound_message
            .as_ref()
            .ok_or_else(|| "Missing inbound message in task payload".to_string())?;

        // Idempotency Guard: Check if an outbound email for this triggering message was already sent
        let target_thread_ids: Vec<_> = if ingest.channel_matches.is_empty() {
            vec![inbound_msg.thread_id]
        } else {
            ingest
                .channel_matches
                .iter()
                .map(|channel_match| channel_match.thread.id)
                .collect()
        };
        let mut outbound_reply = None;
        let mut missing_threads = Vec::new();
        for thread_id in target_thread_ids {
            match self
                .thread_use_cases
                .find_outbound_reply(thread_id, &inbound_msg.message_id)
                .await
                .map_err(|e| e.to_string())?
            {
                Some(message) => outbound_reply = Some(message),
                None => missing_threads.push(thread_id),
            }
        }

        if let Some(outbound) = outbound_reply {
            for thread_id in missing_threads {
                self.thread_use_cases
                    .save_message(&crate::entities::message::Message {
                        id: uuid::Uuid::new_v4(),
                        thread_id,
                        ..outbound.clone()
                    })
                    .await
                    .map_err(|error| error.to_string())?;
            }
            if outbound
                .clean_text_body
                .starts_with("Agent execution failed:")
            {
                info!(
                    "Idempotency Guard: Agent execution previously failed for message {}, failing task",
                    inbound_msg.message_id
                );
                return Err(outbound.clean_text_body.clone());
            } else {
                info!(
                    "Idempotency Guard: Outbound reply already sent for message {}, completing task",
                    inbound_msg.message_id
                );
                return Ok(());
            }
        }

        // Execute Agent and Dispatch Outbound Email
        let mut ingest_exec = ingest.clone();
        ingest_exec.task_id = Some(task.id);

        // Whether the reply is really sent was decided when the message came in, not here: a
        // mailbox send can ask to stay in-app, and this worker is a different process from the one
        // that took the request.
        let deliver = ingest_exec.deliver;
        self.thread_use_cases
            .execute_claimed_agent_task_and_dispatch(&ingest_exec, deliver, self.worker_id)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Run one `scheduled_agent_run` task: answer the schedule's prompt in its thread, and email
    /// the answer on if the schedule asked for that.
    ///
    /// Split from the inbound path because a scheduled run has no inbound message to ingest, but
    /// it shares the guard that matters — a retry must not run the agent, or reply, twice.
    async fn execute_scheduled_task(&self, task: &BackgroundTask) -> Result<(), String> {
        let payload: ScheduledRunPayload = serde_json::from_value(task.payload.clone())
            .map_err(|e| format!("Invalid scheduled task payload: {e}"))?;
        let context = self.load_scheduled_run_context(&payload).await?;

        // The reply hangs off the schedule's own prompt message, so finding one already saved
        // means a previous attempt got past the agent. Re-running would bill a second call and
        // append a second answer to the thread; delivery is still reached, because what failed
        // last time may have been the send.
        let answer = match self
            .thread_use_cases
            .find_outbound_reply(payload.thread_id, &payload.trigger_message_id)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(existing) => {
                info!(
                    "Idempotency Guard: schedule '{}' already answered in thread {}, skipping the agent",
                    payload.schedule_name, payload.thread_id
                );
                existing.clean_text_body
            }
            None => {
                let answer = self.run_scheduled_agent(&payload, &context).await?;
                self.save_scheduled_reply(task, &payload, &context, &answer)
                    .await?;
                answer
            }
        };

        self.deliver_scheduled_reply(task, &payload, &context, &answer)
            .await
    }

    /// Everything a scheduled run needs loaded before the agent can be built.
    async fn load_scheduled_run_context(
        &self,
        payload: &ScheduledRunPayload,
    ) -> Result<ScheduledRunContext, String> {
        let company = self
            .thread_use_cases
            .company_persistence()
            .get_by_id(payload.company_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Company {} not found", payload.company_id))?;

        let channel = self
            .thread_use_cases
            .channel_persistence()
            .get_by_id(payload.channel_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Channel {} not found", payload.channel_id))?;

        let first_agent_id = channel
            .agent_ids
            .as_ref()
            .and_then(|ids| ids.first().copied());
        let agent = match (self.thread_use_cases.agent_persistence(), first_agent_id) {
            (Some(agents), Some(id)) => agents.get_by_id(id).await.map_err(|e| e.to_string())?,
            _ => None,
        };

        Ok(ScheduledRunContext {
            company,
            channel,
            agent,
        })
    }

    /// The agent's answer to the schedule's prompt, with the thread so far as context.
    async fn run_scheduled_agent(
        &self,
        payload: &ScheduledRunPayload,
        context: &ScheduledRunContext,
    ) -> Result<String, String> {
        let history = self
            .thread_use_cases
            .thread_persistence()
            .list_messages_by_thread_id(payload.thread_id)
            .await
            .map_err(|e| e.to_string())?;

        let params = ResolvedAgentParams::new(
            Some(&context.company),
            Some(&context.channel),
            context.agent.as_ref(),
        )
        .map_err(|e| format!("Failed to resolve agent parameters: {e}"))?;

        let output = AgentRunner::new(&payload.prompt, &params)
            .history(&history)
            .monitoring(self.monitoring.clone())
            .config(Some(self.config.clone()))
            .company(Some(context.company.clone()))
            .ids(
                Some(context.company.id),
                Some(context.channel.id),
                context.agent.as_ref().map(|agent| agent.id),
            )
            .execute()
            .await
            .map_err(|e| format!("Agent execution failed: {e}"))?;

        Ok(output.content)
    }

    /// Record the answer in the schedule's thread, threaded onto the prompt that asked for it.
    async fn save_scheduled_reply(
        &self,
        task: &BackgroundTask,
        payload: &ScheduledRunPayload,
        context: &ScheduledRunContext,
        answer: &str,
    ) -> Result<(), String> {
        let sender = context
            .channel
            .inbound_address(&context.company.slug, &self.config.app_domain_name);

        let message = Message {
            id: Uuid::new_v4(),
            thread_id: payload.thread_id,
            message_id: scheduled_reply_message_id(task.id, &self.config.app_domain_name),
            in_reply_to: Some(payload.trigger_message_id.clone()),
            references_list: vec![payload.trigger_message_id.clone()],
            sender: sender.clone(),
            recipients_to: vec![sender],
            recipients_cc: vec![],
            subject: reply_subject(&payload.subject),
            clean_text_body: answer.to_string(),
            raw_text_body: Some(answer.to_string()),
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: chrono::Utc::now(),
        };

        self.thread_use_cases
            .save_message(&message)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Queue the answer for email, when the schedule delivers anywhere beyond its mailbox. The
    /// idempotency key makes this safe to reach on a retry that skipped the agent.
    async fn deliver_scheduled_reply(
        &self,
        task: &BackgroundTask,
        payload: &ScheduledRunPayload,
        context: &ScheduledRunContext,
        answer: &str,
    ) -> Result<(), String> {
        if !payload.wants_email() {
            return Ok(());
        }

        let ScheduledRunContext {
            company, channel, ..
        } = context;

        let recipients = self.scheduled_recipients(payload, channel).await;
        let Some((primary_to, cc_list)) = recipients.split_first() else {
            warn!(
                "Schedule '{}' asked for email delivery but resolved no recipients",
                payload.schedule_name
            );
            return Ok(());
        };

        let reply_message_id = scheduled_reply_message_id(task.id, &self.config.app_domain_name);
        let outbound_email = OutboundEmail {
            channel_id: channel.id,
            channel_name: channel.name.clone(),
            channel_slug: channel.slug.clone(),
            company_slug: company.slug.clone(),
            trigger_message_id: payload.trigger_message_id.clone(),
            thread_references: vec![reply_message_id],
            recipient_to: primary_to.clone(),
            recipients_cc: cc_list.to_vec(),
            subject: reply_subject(&payload.subject),
            body_text: agent_response_email_body(answer),
            hop_count: 0,
            trace_channels: vec![channel.id],
        };

        self.task_persistence
            .enqueue_outbound_send(OutboundSend {
                company_id: company.id,
                channel_id: channel.id,
                task_id: Some(task.id),
                idempotency_key: format!("task:{}:scheduled-email", task.id),
                payload: serde_json::to_value(&outbound_email)
                    .map_err(|e| format!("Serialization error: {e}"))?,
            })
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Who the answer is emailed to: the schedule's own list, or the channel's participants with
    /// the company team as the fallback when the channel names none.
    async fn scheduled_recipients(
        &self,
        payload: &ScheduledRunPayload,
        channel: &Channel,
    ) -> Vec<EmailAddress> {
        if let Some(custom) = payload.custom_recipients() {
            return custom.to_vec();
        }

        let participants: Vec<EmailAddress> = channel
            .participant_emails
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|email| !email.eq_ignore_ascii_case(PUBLIC_PARTICIPANT))
            .collect();

        if !participants.is_empty() {
            return participants;
        }

        self.thread_use_cases
            .company_persistence()
            .list_company_team_emails(payload.company_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(EmailAddress::from)
            .collect()
    }

    async fn execute_single_task_with_lease(
        &self,
        task: &crate::entities::task::BackgroundTask,
        attempt: TaskAttemptRef,
    ) -> Result<(), String> {
        // Open the ledger row alongside the lease: both describe this one run, and both are wanted
        // even if it never reaches a terminal state. A failure to open it is logged, not fatal —
        // the run is the point, the bookkeeping is not.
        if let Err(error) = self.task_persistence.begin_task_attempt(attempt).await {
            warn!(
                "Could not open the attempt ledger for task {}: {error}",
                task.id
            );
        }

        let lease = TaskLease::worker(self.worker_id);
        match while_leased(
            &*self.task_persistence,
            task.id,
            &lease,
            self.execute_single_task(task),
        )
        .await
        {
            Leased::Finished(result) => result,
            Leased::Lost => Err("Task lease was lost during execution".to_string()),
        }
    }

    pub async fn stop_task_and_notify(&self, task_id: uuid::Uuid) -> Result<(), String> {
        let task = self
            .task_persistence
            .stop_task(task_id)
            .await
            .map_err(|e| e.to_string())?;

        // Parse payload to notify participants
        if let Ok(ingest) = serde_json::from_value::<InboundIngestResult>(task.payload) {
            if let (Some(channel), Some(company), Some(parsed)) =
                (ingest.channel, ingest.company, ingest.parsed_email)
            {
                let stop_email = OutboundEmail {
                    channel_id: channel.id,
                    channel_name: channel.name.clone(),
                    channel_slug: channel.slug.clone(),
                    company_slug: company.slug.clone(),
                    trigger_message_id: parsed.message_id.clone().into(),
                    thread_references: parsed
                        .references
                        .iter()
                        .cloned()
                        .map(crate::entities::value_objects::MessageId::from)
                        .collect(),
                    recipient_to: parsed.sender.clone().into(),
                    recipients_cc: parsed
                        .recipients_cc
                        .iter()
                        .cloned()
                        .map(crate::entities::value_objects::EmailAddress::from)
                        .collect(),
                    subject: format!("[STOPPED] Re: {}", parsed.subject),
                    body_text: format!(
                        "Notice: The automated channel processing for thread '{}' has been manually stopped by the system administrator.",
                        parsed.subject
                    ),
                    hop_count: parsed.hop_count,
                    trace_channels: parsed.trace_channels,
                };

                match self
                    .thread_use_cases
                    .prepare_internal_channel_delivery(stop_email.clone(), None)
                    .await
                {
                    Ok(Some(prepared)) => {
                        let _ = self
                            .thread_use_cases
                            .ingest_prepared_internal_message(&prepared)
                            .await;
                    }
                    Ok(None) => {
                        let _ = OutboundDispatcher::send(&self.config, stop_email).await;
                    }
                    Err(error) => warn!("Failed to prepare stop notification: {error}"),
                }
            }
        }

        Ok(())
    }

    pub async fn resume_task(&self, task_id: uuid::Uuid) -> Result<(), String> {
        self.task_persistence
            .resume_task(task_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::company_member::CompanyMembership;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex;
    use uuid::Uuid;

    use crate::{
        app_error::AppResult,
        entities::{
            channel::Channel,
            company::Company,
            cursor::{MessageCursor, ThreadCursor},
            message::Message,
            task::{BackgroundTask, TaskStatus},
            thread::Thread,
        },
        use_cases::{
            company::{CompanyPersistence, CompanyWrite},
            thread::ThreadPersistence,
        },
    };

    struct MockCompanyPersistence {
        company: Option<Company>,
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
        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
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

    struct MockThreadPersistence {
        messages: Mutex<Vec<Message>>,
        threads: Mutex<Vec<Thread>>,
    }

    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(
            &self,
            _channel_id: Uuid,
            _subject: &str,
            _participant_emails: &[crate::entities::value_objects::EmailAddress],
        ) -> AppResult<Thread> {
            unimplemented!()
        }
        async fn get_thread_by_id(&self, _id: Uuid) -> AppResult<Option<Thread>> {
            unimplemented!()
        }
        async fn list_threads_by_channel_id(
            &self,
            _channel_id: Uuid,
            _before: Option<ThreadCursor>,
            _limit: usize,
        ) -> AppResult<Vec<Thread>> {
            unimplemented!()
        }

        async fn list_threads_updated_after(
            &self,
            channel_id: Uuid,
            after: Option<ThreadCursor>,
            limit: usize,
        ) -> AppResult<Vec<Thread>> {
            let mut threads: Vec<Thread> = self
                .threads
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.channel_id == channel_id)
                .filter(|t| after.is_none_or(|cursor| t.cursor() > cursor))
                .cloned()
                .collect();
            threads.sort_by_key(|t| t.cursor());
            threads.truncate(limit);
            Ok(threads)
        }
        async fn update_thread_participants(
            &self,
            _id: Uuid,
            _participant_emails: &[crate::entities::value_objects::EmailAddress],
        ) -> AppResult<Thread> {
            unimplemented!()
        }
        async fn find_thread_by_message_ids(
            &self,
            _channel_id: Uuid,
            _message_ids: &[crate::entities::value_objects::MessageId],
        ) -> AppResult<Option<Thread>> {
            unimplemented!()
        }
        async fn find_thread_by_thread_index(
            &self,
            _channel_id: Uuid,
            _thread_index_prefix: &crate::entities::value_objects::ThreadIndex,
        ) -> AppResult<Option<Thread>> {
            unimplemented!()
        }
        async fn count_recent_messages(
            &self,
            _thread_id: Uuid,
            _duration_secs: i64,
        ) -> AppResult<usize> {
            unimplemented!()
        }
        async fn create_message(&self, message: &Message) -> AppResult<Message> {
            self.messages.lock().unwrap().push(message.clone());
            Ok(message.clone())
        }
        async fn get_message_by_message_id(
            &self,
            _company_id: Uuid,
            _message_id: &crate::entities::value_objects::MessageId,
        ) -> AppResult<Option<Message>> {
            unimplemented!()
        }
        async fn find_outbound_reply(
            &self,
            thread_id: Uuid,
            in_reply_to: &crate::entities::value_objects::MessageId,
        ) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|message| {
                    message.thread_id == thread_id
                        && message.direction == crate::entities::message::MessageDirection::Outbound
                        && message.in_reply_to.as_ref() == Some(in_reply_to)
                })
                .cloned())
        }
        async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.thread_id == thread_id)
                .cloned()
                .collect())
        }

        async fn list_messages_after(
            &self,
            thread_id: Uuid,
            after: Option<MessageCursor>,
            limit: usize,
        ) -> AppResult<Vec<Message>> {
            let mut messages: Vec<Message> = self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.thread_id == thread_id)
                .filter(|m| after.is_none_or(|cursor| m.cursor() > cursor))
                .cloned()
                .collect();
            messages.sort_by_key(|m| m.cursor());
            messages.truncate(limit);
            Ok(messages)
        }
    }

    #[derive(Default)]
    struct MockTaskPersistence {
        tasks: Mutex<Vec<BackgroundTask>>,
        /// Lease renewals seen, so a test can prove the heartbeat actually fired.
        renewals: Mutex<usize>,
        /// How many renewals to grant before reporting the lease gone. `None` grants every one.
        renewals_before_loss: Option<usize>,
        /// Emails queued for delivery. The trait's default discards them, which would let a test
        /// claiming a send happened pass without one.
        outbound_sends: Mutex<Vec<OutboundSend>>,
    }

    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        async fn enqueue_outbound_send(&self, send: OutboundSend) -> AppResult<Option<Uuid>> {
            let id = Uuid::new_v4();
            self.outbound_sends.lock().unwrap().push(send);
            Ok(Some(id))
        }

        async fn renew_task_lease(
            &self,
            _id: Uuid,
            _worker_id: Uuid,
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
            company_id: Uuid,
            channel_id: Uuid,
            thread_id: Option<Uuid>,
            task_type: &str,
            payload: serde_json::Value,
        ) -> AppResult<BackgroundTask> {
            let task = BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id,
                task_type: task_type.to_string(),
                status: TaskStatus::Pending,
                payload,
                retry_count: 0,
                max_retries: 3,
                last_error: None,
                worker_id: None,
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
                .filter(|task| {
                    (task.status == TaskStatus::Pending && task.run_at <= now)
                        || (task.status == TaskStatus::Processing
                            && task.lock_expires_at.is_none_or(|expires| expires <= now))
                })
                .take(limit as usize)
            {
                task.status = TaskStatus::Processing;
                task.worker_id = Some(worker_id);
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

        async fn mark_task_completed(&self, id: Uuid, worker_id: Uuid) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == id
                    && t.status == TaskStatus::Processing
                    && t.worker_id == Some(worker_id)
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

        async fn mark_task_failed(
            &self,
            id: Uuid,
            worker_id: Uuid,
            error_msg: &str,
            next_run_at: chrono::DateTime<chrono::Utc>,
            is_dead_letter: bool,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == id
                    && t.status == TaskStatus::Processing
                    && t.worker_id == Some(worker_id)
                    && t.lock_expires_at.is_some_and(|expires| expires > now)
            }) {
                t.last_error = Some(error_msg.to_string());
                t.retry_count += 1;
                t.run_at = next_run_at;
                t.status = if is_dead_letter {
                    TaskStatus::DeadLetter
                } else {
                    TaskStatus::Pending
                };
                t.worker_id = None;
                t.locked_at = None;
                t.lock_expires_at = None;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn stop_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
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

        async fn resume_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
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

        async fn update_task_status(
            &self,
            id: Uuid,
            status: TaskStatus,
        ) -> AppResult<BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list
                .iter_mut()
                .find(|t| {
                    t.id == id
                        && match status {
                            TaskStatus::PendingApproval => matches!(
                                t.status,
                                TaskStatus::Processing | TaskStatus::WaitingForThirdPartyReply
                            ),
                            TaskStatus::WaitingForThirdPartyReply => matches!(
                                t.status,
                                TaskStatus::Processing | TaskStatus::PendingApproval
                            ),
                            _ => false,
                        }
                })
                .unwrap();
            t.status = status;
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
        let lease = TaskLease::worker(Uuid::new_v4());

        // 1000s of work against a 900s lease: beats land at 300s, 600s and 900s.
        let outcome = while_leased(
            &persistence,
            Uuid::new_v4(),
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
        let persistence = MockTaskPersistence {
            renewals_before_loss: Some(1),
            ..Default::default()
        };
        let lease = TaskLease::worker(Uuid::new_v4());
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let ran = Arc::clone(&finished);
        let outcome = while_leased(&persistence, Uuid::new_v4(), &lease, async move {
            tokio::time::sleep(Duration::from_secs(6000)).await;
            ran.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .await;

        assert!(matches!(outcome, Leased::Lost));
        assert!(
            !finished.load(std::sync::atomic::Ordering::SeqCst),
            "work must be dropped the moment the lease is gone"
        );
    }

    #[tokio::test]
    async fn expired_lease_is_reclaimed_and_stale_worker_cannot_complete() {
        let persistence = MockTaskPersistence::default();
        let task = persistence
            .enqueue_task(
                Uuid::new_v4(),
                Uuid::new_v4(),
                None,
                "email_agent_dispatch",
                serde_json::json!({}),
            )
            .await
            .unwrap();
        let first_worker = Uuid::new_v4();
        let second_worker = Uuid::new_v4();

        let first_claim = persistence
            .claim_pending_tasks(first_worker, Utc::now() + chrono::Duration::minutes(1), 1)
            .await
            .unwrap();
        assert_eq!(first_claim.len(), 1);

        persistence.tasks.lock().unwrap()[0].lock_expires_at =
            Some(Utc::now() - chrono::Duration::seconds(1));

        let second_claim = persistence
            .claim_pending_tasks(second_worker, Utc::now() + chrono::Duration::minutes(1), 1)
            .await
            .unwrap();
        assert_eq!(second_claim.len(), 1);
        assert_eq!(second_claim[0].worker_id, Some(second_worker));
        assert!(
            !persistence
                .mark_task_completed(task.id, first_worker)
                .await
                .unwrap()
        );
        assert!(
            persistence
                .mark_task_completed(task.id, second_worker)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_task_worker_stop_and_resume_flow() {
        let task_persistence = Arc::new(MockTaskPersistence::default());
        let thread_persistence = Arc::new(MockThreadPersistence {
            messages: Mutex::new(Vec::new()),
            threads: Mutex::new(Vec::new()),
        });
        let company_persistence = Arc::new(MockCompanyPersistence { company: None });
        let channel_persistence = Arc::new(MockChannelPersistence { channel: None });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
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

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence.clone(),
            config.clone(),
        ));

        let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases, config);

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let task = task_persistence
            .enqueue_task(
                company_id,
                channel_id,
                None,
                "email_agent_dispatch",
                serde_json::json!({}),
            )
            .await
            .unwrap();
        assert_eq!(task.status, TaskStatus::Pending);

        // Stop task
        worker.stop_task_and_notify(task.id).await.unwrap();
        let stopped_task = task_persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stopped_task.status, TaskStatus::Stopped);

        // Resume task
        worker.resume_task(task.id).await.unwrap();
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

        let task_persistence = Arc::new(MockTaskPersistence::default());
        let thread_persistence = Arc::new(MockThreadPersistence {
            messages: Mutex::new(Vec::new()),
            threads: Mutex::new(Vec::new()),
        });

        let company = crate::entities::company::Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Corp".to_string(),
            slug: "test".into(),
            api_key: None,
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };

        let company_persistence = Arc::new(MockCompanyPersistence {
            company: Some(company.clone()),
        });
        let channel_persistence = Arc::new(MockChannelPersistence {
            channel: Some(channel.clone()),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
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

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence,
            channel_persistence,
            company_persistence,
            task_persistence.clone(),
            config.clone(),
        ));

        let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases, config);

        let raw = crate::services::email_parser::RawInboundPayload {
            headers: Some("Message-ID: <msg1@test.com>\n".to_string()),
            subject: Some("Help".to_string()),
            text: Some("Need help".to_string()),
            html: None,
            from: "user@test.com".to_string(),
            to: "support@test.mailagents.com".to_string(),
            cc: None,
            spam_score: None,
            attachments_data: vec![],
            spf: Default::default(),
            dkim: Default::default(),
            dmarc: Default::default(),
        };
        let parsed_email = crate::services::email_parser::EmailParser::parse(raw, "mailagents.com");

        let ingest = crate::use_cases::thread::InboundIngestResult {
            accepted: true,
            reason: None,
            thread: Some(crate::entities::thread::Thread {
                id: thread_id,
                channel_id,
                subject: "Help".to_string(),
                participant_emails: vec!["user@test.com".into()],
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }),
            inbound_message: Some(crate::entities::message::Message {
                id: Uuid::new_v4(),
                thread_id,
                message_id: "<msg1@test.com>".into(),
                in_reply_to: None,
                references_list: vec![],
                sender: "user@test.com".into(),
                recipients_to: vec!["support@test.mailagents.com".into()],
                recipients_cc: vec![],
                subject: "Help".to_string(),
                clean_text_body: "Need help".to_string(),
                raw_text_body: None,
                raw_html_body: None,
                attachments: None,
                direction: crate::entities::message::MessageDirection::Inbound,
                role: crate::entities::message::MessageRole::Human,
                thread_index: Some("1".into()),
                created_at: chrono::Utc::now(),
            }),
            company: Some(company),
            channel: Some(channel),
            parsed_email: Some(parsed_email),
            normalized_message: None,
            task_id: None,
            deliver: true,
            channel_matches: vec![],
            bounce_info: None,
        };

        let payload_json = serde_json::to_value(&ingest).unwrap();
        let task = task_persistence
            .enqueue_task(
                company_id,
                channel_id,
                Some(thread_id),
                "email_agent_dispatch",
                payload_json,
            )
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

        let company = Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Company".to_string(),
            slug: "test-co".into(),
            api_key: None,
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Audit Channel".to_string(),
            description: None,
            slug: "audit".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };

        let thread_persistence = Arc::new(MockThreadPersistence {
            messages: Mutex::new(vec![]),
            threads: Mutex::new(vec![]),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
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

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence.clone(),
            Arc::new(MockChannelPersistence {
                channel: Some(channel.clone()),
            }),
            Arc::new(MockCompanyPersistence {
                company: Some(company.clone()),
            }),
            task_persistence.clone(),
            config.clone(),
        ));

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
            "trigger_message_id": "<TRIGGER123@domain.com>",
        });

        let task = task_persistence
            .enqueue_task(
                company_id,
                channel_id,
                Some(thread_id),
                "scheduled_agent_run",
                scheduled_payload,
            )
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

        let company = Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Company".to_string(),
            slug: "test-co".into(),
            api_key: None,
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Audit Channel".to_string(),
            description: None,
            slug: "audit".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };

        // The answer a previous attempt already saved, threaded onto the schedule's prompt.
        let existing_reply = Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: MessageId::new("<already-answered@domain.com>"),
            in_reply_to: Some(trigger.clone()),
            references_list: vec![trigger.clone()],
            sender: EmailAddress::from("audit@test-co.mailagents.com"),
            recipients_to: vec![],
            recipients_cc: vec![],
            subject: "Re: Audit Report".into(),
            clean_text_body: "Audit complete: nothing to report.".into(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: chrono::Utc::now(),
        };

        let thread_persistence = Arc::new(MockThreadPersistence {
            messages: Mutex::new(vec![existing_reply]),
            threads: Mutex::new(vec![]),
        });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
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

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence.clone(),
            Arc::new(MockChannelPersistence {
                channel: Some(channel.clone()),
            }),
            Arc::new(MockCompanyPersistence {
                company: Some(company.clone()),
            }),
            task_persistence.clone(),
            config.clone(),
        ));

        let worker = Arc::new(TaskWorker::new(
            task_persistence.clone(),
            thread_use_cases,
            config,
        ));

        let task = task_persistence
            .enqueue_task(
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
                    "trigger_message_id": trigger.to_string(),
                }),
            )
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

        // Exactly one reply in the thread -- the retry appended nothing.
        assert_eq!(
            thread_persistence.messages.lock().unwrap().len(),
            1,
            "a retry must not append a second answer"
        );

        // Delivery still ran, addressed from the schedule's own recipient list.
        let sends = task_persistence.outbound_sends.lock().unwrap();
        assert_eq!(sends.len(), 1);
        let email: OutboundEmail = serde_json::from_value(sends[0].payload.clone()).unwrap();
        assert_eq!(email.recipient_to, EmailAddress::from("ops@example.com"));
        assert_eq!(
            email.recipients_cc,
            vec![EmailAddress::from("cc@example.com")]
        );
        assert_eq!(
            email.body_text,
            "Audit complete: nothing to report.\n\nDone by busybots.net"
        );
        assert_eq!(email.subject, "Re: Audit Report");
    }

    #[test]
    fn a_reply_subject_does_not_stack_prefixes() {
        assert_eq!(reply_subject("Audit Report"), "Re: Audit Report");
        assert_eq!(reply_subject("Re: Audit Report"), "Re: Audit Report");
        assert_eq!(reply_subject("RE: Audit Report"), "RE: Audit Report");
    }

    #[test]
    fn a_scheduled_reply_message_id_is_stable_across_retries() {
        let task_id = Uuid::new_v4();
        assert_eq!(
            scheduled_reply_message_id(task_id, "mailagents.com"),
            scheduled_reply_message_id(task_id, "mailagents.com"),
            "the saved reply and the emailed copy must agree, on every attempt"
        );
    }
}
