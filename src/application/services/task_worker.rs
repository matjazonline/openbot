use std::sync::Arc;
use tokio::time::{Duration, sleep};
use tracing::{error, info, warn};

use crate::{
    adapters::persistence::task::{TASK_LEASE_SECONDS, TaskPersistence},
    domain::monitoring::{MonitoringService, TaskExecutionMetrics, TaskStatusMetric},
    infra::config::AppConfig,
    services::outbound_dispatcher::{OutboundDispatcher, OutboundEmail},
    use_cases::thread::{InboundIngestResult, ThreadUseCases},
};

pub struct TaskWorker {
    task_persistence: Arc<dyn TaskPersistence>,
    thread_use_cases: Arc<ThreadUseCases>,
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
            config,
            monitoring: None,
            worker_id: uuid::Uuid::new_v4(),
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    /// Continuous background poller running every 3 seconds
    pub async fn start_worker_loop(
        self: Arc<Self>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) {
        info!("Starting Background Task Worker Loop (poll_interval = 3s)...");

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received. Stopping Background Task Worker Loop...");
                    break;
                }
                _ = sleep(Duration::from_secs(3)) => {
                    if let Err(err) = self.process_next_batch().await {
                        warn!("Error in background task poller iteration: {}", err);
                    }
                }
            }
        }
    }

    pub async fn process_next_batch(&self) -> Result<(), String> {
        self.process_outbox_emails().await?;
        let _ = self.check_quorum_timeouts().await;

        let tasks = self
            .task_persistence
            .claim_pending_tasks(
                self.worker_id,
                chrono::Utc::now().naive_utc() + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                1,
            )
            .await
            .map_err(|e| e.to_string())?;

        for task in tasks {
            let task_id = task.id;
            info!("Processing task {} (type = '{}')", task_id, task.task_type);

            let start_time = std::time::Instant::now();
            let result = self.execute_single_task_with_lease(&task).await;
            let duration_ms = start_time.elapsed().as_millis() as u64;

            match result {
                Ok(_) => {
                    if let Ok(Some(current)) = self.task_persistence.get_task_by_id(task_id).await
                        && matches!(
                            current.status,
                            crate::entities::task::TaskStatus::PendingApproval
                                | crate::entities::task::TaskStatus::WaitingForThirdPartyReply
                        )
                    {
                        info!(
                            "Background task {} suspended with status {}",
                            task_id,
                            current.status.as_str()
                        );
                        continue;
                    }
                    info!("Successfully completed background task {}", task_id);
                    match self
                        .task_persistence
                        .mark_task_completed(task_id, self.worker_id)
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => warn!(
                            "Task {} completion ignored because its lease or status changed",
                            task_id
                        ),
                        Err(err) => error!("Failed to complete task {}: {}", task_id, err),
                    }
                    if let Some(ref m) = self.monitoring {
                        m.record_task_execution(&TaskExecutionMetrics {
                            company_id: Some(task.company_id),
                            channel_id: Some(task.channel_id),
                            task_type: task.task_type.clone(),
                            duration_ms,
                            status: TaskStatusMetric::Completed,
                            retry_count: task.retry_count as u32,
                        });
                    }
                }
                Err(err_msg) => {
                    warn!("Failed background task {}: {}", task_id, err_msg);
                    let next_retry = task.retry_count + 1;
                    let is_dead_letter = next_retry >= task.max_retries;

                    // Exponential backoff: 30s * 2^retry
                    let backoff_secs = 30 * (1 << next_retry.min(10));
                    let next_run =
                        chrono::Utc::now().naive_utc() + chrono::Duration::seconds(backoff_secs);

                    match self
                        .task_persistence
                        .mark_task_failed(
                            task_id,
                            self.worker_id,
                            &err_msg,
                            next_run,
                            is_dead_letter,
                        )
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => warn!(
                            "Task {} failure ignored because its lease or status changed",
                            task_id
                        ),
                        Err(err) => error!("Failed to fail task {}: {}", task_id, err),
                    }

                    if let Some(ref m) = self.monitoring {
                        m.record_task_execution(&TaskExecutionMetrics {
                            company_id: Some(task.company_id),
                            channel_id: Some(task.channel_id),
                            task_type: task.task_type.clone(),
                            duration_ms,
                            status: TaskStatusMetric::Failed,
                            retry_count: task.retry_count as u32,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_outbox_emails(&self) -> Result<(), String> {
        let emails = self
            .task_persistence
            .claim_outbox_emails(
                self.worker_id,
                chrono::Utc::now().naive_utc() + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                10,
            )
            .await
            .map_err(|error| error.to_string())?;

        for queued in emails {
            if !self
                .task_persistence
                .is_outbox_delivery_active(queued.id)
                .await
                .unwrap_or(false)
            {
                let _ = self
                    .task_persistence
                    .cancel_claimed_outbox(queued.id, self.worker_id)
                    .await;
                continue;
            }
            let email: OutboundEmail = match serde_json::from_value(queued.payload) {
                Ok(email) => email,
                Err(error) => {
                    let _ = self
                        .task_persistence
                        .mark_outbox_email_failed(queued.id, self.worker_id, &error.to_string())
                        .await;
                    continue;
                }
            };
            match OutboundDispatcher::send_idempotent(
                &self.config,
                email,
                &format!("outbox:{}", queued.id),
            )
            .await
            {
                Ok(sent) => {
                    let _ = self
                        .task_persistence
                        .mark_outbox_email_sent(
                            queued.id,
                            self.worker_id,
                            &sent.outbound_message_id,
                        )
                        .await;
                    if let Err(error) = self
                        .thread_use_cases
                        .record_outreach_outbound_message(queued.id, &sent)
                        .await
                    {
                        warn!(
                            "Failed to record sent outreach outbox {} in thread history: {}",
                            queued.id, error
                        );
                    }
                }
                Err(error) => {
                    let _ = self
                        .task_persistence
                        .mark_outbox_email_failed(queued.id, self.worker_id, &error.to_string())
                        .await;
                }
            }
        }
        Ok(())
    }

    pub async fn check_quorum_timeouts(&self) -> Result<(), String> {
        let now = chrono::Utc::now().naive_utc();
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
            let Some(approval_use_cases) = self.thread_use_cases.get_approval_use_cases() else {
                continue;
            };
            let Some(task) = self
                .task_persistence
                .get_task_by_id(outreach.task_id)
                .await
                .map_err(|error| error.to_string())?
            else {
                continue;
            };
            let ingest: InboundIngestResult =
                serde_json::from_value(task.payload.clone()).map_err(|error| error.to_string())?;
            let channel = ingest.channel.or_else(|| {
                ingest
                    .channel_matches
                    .first()
                    .map(|matched| matched.channel.clone())
            });
            let company = ingest.company.or_else(|| {
                ingest
                    .channel_matches
                    .first()
                    .map(|matched| matched.company.clone())
            });
            let (Some(channel), Some(company)) = (channel, company) else {
                continue;
            };
            let team_approver = self
                .thread_use_cases
                .company_persistence()
                .list_company_team_emails(task.company_id)
                .await
                .unwrap_or_default()
                .into_iter()
                .next();
            let approver_email = channel
                .participant_emails
                .as_ref()
                .and_then(|participants| {
                    participants
                        .iter()
                        .find(|email| !email.eq_ignore_ascii_case("@public"))
                })
                .cloned()
                .or(team_approver)
                .unwrap_or_default();
            if approver_email.is_empty() {
                warn!("No approver configured for outreach task {}", task.id);
                continue;
            }
            if !self
                .task_persistence
                .mark_outreach_timeout_pending(outreach.outreach_id)
                .await
                .unwrap_or(false)
            {
                continue;
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
                        outreach.expires_at.and_utc().timestamp()
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
                let _ = self
                    .task_persistence
                    .restore_outreach_waiting(outreach.outreach_id)
                    .await;
                return Err(error.to_string());
            }
        }

        Ok(())
    }

    async fn execute_single_task(
        &self,
        task: &crate::entities::task::BackgroundTask,
    ) -> Result<(), String> {
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

        self.thread_use_cases
            .execute_claimed_agent_task_and_dispatch(&ingest_exec, true, self.worker_id)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    async fn execute_single_task_with_lease(
        &self,
        task: &crate::entities::task::BackgroundTask,
    ) -> Result<(), String> {
        let execution = self.execute_single_task(task);
        tokio::pin!(execution);
        let mut heartbeat =
            tokio::time::interval(Duration::from_secs((TASK_LEASE_SECONDS / 3).max(1) as u64));
        heartbeat.tick().await;

        loop {
            tokio::select! {
                result = &mut execution => return result,
                _ = heartbeat.tick() => {
                    let renewed = self
                        .task_persistence
                        .renew_task_lease(
                            task.id,
                            self.worker_id,
                            chrono::Utc::now().naive_utc()
                                + chrono::Duration::seconds(TASK_LEASE_SECONDS),
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                    if !renewed {
                        return Err("Task lease was lost during execution".to_string());
                    }
                }
            }
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
                    trigger_message_id: parsed.message_id.clone(),
                    thread_references: parsed.references.clone(),
                    recipient_to: parsed.sender.clone(),
                    recipients_cc: parsed.recipients_cc.clone(),
                    subject: format!("[STOPPED] Re: {}", parsed.subject),
                    body_text: format!(
                        "Notice: The automated channel processing for thread '{}' has been manually stopped by the system administrator.",
                        parsed.subject
                    ),
                    hop_count: parsed.hop_count,
                    trace_channels: parsed.trace_channels,
                };

                let _ = OutboundDispatcher::send(&self.config, stop_email).await;
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
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex;
    use uuid::Uuid;

    use crate::{
        app_error::AppResult,
        entities::{
            channel::Channel,
            company::Company,
            message::Message,
            task::{BackgroundTask, TaskStatus},
            thread::Thread,
        },
        use_cases::{company::CompanyPersistence, thread::ThreadPersistence},
    };

    struct MockCompanyPersistence {
        company: Option<Company>,
    }
    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(
            &self,
            _user_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
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
        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn is_company_team_member(&self, _company_id: Uuid, _email: &str) -> AppResult<bool> {
            Ok(true)
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }
    }

    use crate::use_cases::channel::ChannelPersistence;

    struct MockChannelPersistence {
        channel: Option<Channel>,
    }
    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(
            &self,
            _company_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _agent_ids: Option<Vec<Uuid>>,
            _channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
            Ok(self.channel.clone().filter(|channel| channel.id == id))
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &str,
            _channel_slug: &str,
        ) -> AppResult<Option<Channel>> {
            unimplemented!()
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(vec![])
        }
        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _agent_ids: Option<Vec<Uuid>>,
            _channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockThreadPersistence {
        messages: Mutex<Vec<Message>>,
    }

    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(
            &self,
            _channel_id: Uuid,
            _subject: &str,
            _participant_emails: &[String],
        ) -> AppResult<Thread> {
            unimplemented!()
        }
        async fn get_thread_by_id(&self, _id: Uuid) -> AppResult<Option<Thread>> {
            unimplemented!()
        }
        async fn list_threads_by_channel_id(
            &self,
            _channel_id: Uuid,
            _before: Option<(chrono::NaiveDateTime, Uuid)>,
            _limit: usize,
        ) -> AppResult<Vec<Thread>> {
            unimplemented!()
        }
        async fn update_thread_participants(
            &self,
            _id: Uuid,
            _participant_emails: &[String],
        ) -> AppResult<Thread> {
            unimplemented!()
        }
        async fn find_thread_by_message_ids(
            &self,
            _channel_id: Uuid,
            _message_ids: &[String],
        ) -> AppResult<Option<Thread>> {
            unimplemented!()
        }
        async fn find_thread_by_thread_index(
            &self,
            _channel_id: Uuid,
            _thread_index_prefix: &str,
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
            _message_id: &str,
        ) -> AppResult<Option<Message>> {
            unimplemented!()
        }
        async fn find_outbound_reply(
            &self,
            thread_id: Uuid,
            in_reply_to: &str,
        ) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|message| {
                    message.thread_id == thread_id
                        && message.direction == crate::entities::message::MessageDirection::Outbound
                        && message.in_reply_to.as_deref() == Some(in_reply_to)
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
    }

    struct MockTaskPersistence {
        tasks: Mutex<Vec<BackgroundTask>>,
    }

    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
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
                run_at: Utc::now().naive_utc(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
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
            lock_expires_at: chrono::NaiveDateTime,
            limit: i64,
        ) -> AppResult<Vec<BackgroundTask>> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now().naive_utc();
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
            lock_expires_at: chrono::NaiveDateTime,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now().naive_utc();
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
            let now = Utc::now().naive_utc();
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
            next_run_at: chrono::NaiveDateTime,
            is_dead_letter: bool,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now().naive_utc();
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
            t.run_at = Utc::now().naive_utc();
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

    #[tokio::test]
    async fn expired_lease_is_reclaimed_and_stale_worker_cannot_complete() {
        let persistence = MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        };
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
            .claim_pending_tasks(
                first_worker,
                Utc::now().naive_utc() + chrono::Duration::minutes(1),
                1,
            )
            .await
            .unwrap();
        assert_eq!(first_claim.len(), 1);

        persistence.tasks.lock().unwrap()[0].lock_expires_at =
            Some(Utc::now().naive_utc() - chrono::Duration::seconds(1));

        let second_claim = persistence
            .claim_pending_tasks(
                second_worker,
                Utc::now().naive_utc() + chrono::Duration::minutes(1),
                1,
            )
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
        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });
        let thread_persistence = Arc::new(MockThreadPersistence {
            messages: Mutex::new(Vec::new()),
        });
        let company_persistence = Arc::new(MockCompanyPersistence { company: None });
        let channel_persistence = Arc::new(MockChannelPersistence { channel: None });

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
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

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });
        let thread_persistence = Arc::new(MockThreadPersistence {
            messages: Mutex::new(Vec::new()),
        });

        let company = crate::entities::company::Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Corp".to_string(),
            slug: "test".to_string(),
            api_key: None,
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let channel = Channel {
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            slug: "support".to_string(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            created_at: chrono::Utc::now().naive_utc(),
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
            spf: None,
            dkim: None,
            dmarc: None,
        };
        let parsed_email = crate::services::email_parser::EmailParser::parse(raw, "mailagents.com");

        let ingest = crate::use_cases::thread::InboundIngestResult {
            accepted: true,
            reason: None,
            thread: Some(crate::entities::thread::Thread {
                id: thread_id,
                channel_id,
                subject: "Help".to_string(),
                participant_emails: vec!["user@test.com".to_string()],
                created_at: chrono::Utc::now().naive_utc(),
                updated_at: chrono::Utc::now().naive_utc(),
            }),
            inbound_message: Some(crate::entities::message::Message {
                id: Uuid::new_v4(),
                thread_id,
                message_id: "<msg1@test.com>".to_string(),
                in_reply_to: None,
                references_list: vec![],
                sender: "user@test.com".to_string(),
                recipients_to: vec!["support@test.mailagents.com".to_string()],
                recipients_cc: vec![],
                subject: "Help".to_string(),
                clean_text_body: "Need help".to_string(),
                raw_text_body: None,
                raw_html_body: None,
                attachments: None,
                direction: crate::entities::message::MessageDirection::Inbound,
                role: crate::entities::message::MessageRole::Human,
                thread_index: Some("1".to_string()),
                created_at: chrono::Utc::now().naive_utc(),
            }),
            company: Some(company),
            channel: Some(channel),
            parsed_email: Some(parsed_email),
            normalized_message: None,
            task_id: None,
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

        worker.process_next_batch().await.unwrap();

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
}
