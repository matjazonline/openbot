use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::{
    adapters::persistence::task::TaskPersistence,
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
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    /// Continuous background poller running every 3 seconds
    pub async fn start_worker_loop(self: Arc<Self>, mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) {
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
        let tasks = self
            .task_persistence
            .poll_next_pending_tasks(10)
            .await
            .map_err(|e| e.to_string())?;

        for task in tasks {
            let task_id = task.id;
            info!("Processing task {} (type = '{}')", task_id, task.task_type);

            match self.task_persistence.mark_task_processing(task_id).await {
                Ok(true) => {}
                Ok(false) => {
                    info!("Task {} already claimed by another worker, skipping", task_id);
                    continue;
                }
                Err(err) => {
                    error!("Failed to mark task {} as processing: {}", task_id, err);
                    continue;
                }
            }

            let start_time = std::time::Instant::now();
            let result = self.execute_single_task(&task).await;
            let duration_ms = start_time.elapsed().as_millis() as u64;

            match result {
                Ok(_) => {
                    info!("Successfully completed background task {}", task_id);
                    let _ = self.task_persistence.mark_task_completed(task_id).await;
                    if let Some(ref m) = self.monitoring {
                        m.record_task_execution(&TaskExecutionMetrics {
                            company_id: Some(task.company_id),
                            workflow_id: Some(task.workflow_id),
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
                    let next_run = chrono::Utc::now().naive_utc() + chrono::Duration::seconds(backoff_secs);

                    let _ = self
                        .task_persistence
                        .mark_task_failed(task_id, &err_msg, next_run, is_dead_letter)
                        .await;

                    if let Some(ref m) = self.monitoring {
                        m.record_task_execution(&TaskExecutionMetrics {
                            company_id: Some(task.company_id),
                            workflow_id: Some(task.workflow_id),
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

    async fn execute_single_task(&self, task: &crate::entities::task::BackgroundTask) -> Result<(), String> {
        // Parse payload
        let ingest: InboundIngestResult = serde_json::from_value(task.payload.clone())
            .map_err(|e| format!("Invalid task payload JSON: {}", e))?;

        if !ingest.accepted {
            return Ok(());
        }

        let inbound_msg = ingest
            .inbound_message
            .as_ref()
            .ok_or_else(|| "Missing inbound message in task payload".to_string())?;

        // Idempotency Guard: Check if an outbound email for this triggering message was already sent
        let thread_messages = self
            .thread_use_cases
            .get_thread_history(inbound_msg.thread_id)
            .await
            .map_err(|e| e.to_string())?;

        let outbound_reply = thread_messages.iter().find(|m| {
            m.direction == crate::entities::message::MessageDirection::Outbound
                && m.in_reply_to.as_deref() == Some(&inbound_msg.message_id)
        });

        if let Some(outbound) = outbound_reply {
            if outbound.clean_text_body.starts_with("Agent execution failed:") {
                info!("Idempotency Guard: Agent execution previously failed for message {}, failing task", inbound_msg.message_id);
                return Err(outbound.clean_text_body.clone());
            } else {
                info!("Idempotency Guard: Outbound reply already sent for message {}, completing task", inbound_msg.message_id);
                return Ok(());
            }
        }

        // Execute Agent and Dispatch Outbound Email
        let mut ingest_exec = ingest.clone();
        ingest_exec.task_id = None;

        self.thread_use_cases
            .execute_agent_and_dispatch(&ingest_exec, true)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub async fn stop_task_and_notify(&self, task_id: uuid::Uuid) -> Result<(), String> {
        let task = self
            .task_persistence
            .stop_task(task_id)
            .await
            .map_err(|e| e.to_string())?;

        // Parse payload to notify participants
        if let Ok(ingest) = serde_json::from_value::<InboundIngestResult>(task.payload) {
            if let (Some(workflow), Some(company), Some(parsed)) =
                (ingest.workflow, ingest.company, ingest.parsed_email)
            {
                let stop_email = OutboundEmail {
                    workflow_id: workflow.id,
                    workflow_name: workflow.name.clone(),
                    workflow_slug: workflow.slug.clone(),
                    company_slug: company.slug.clone(),
                    trigger_message_id: parsed.message_id.clone(),
                    thread_references: parsed.references.clone(),
                    recipient_to: parsed.sender.clone(),
                    recipients_cc: parsed.recipients_cc.clone(),
                    subject: format!("[STOPPED] Re: {}", parsed.subject),
                    body_text: format!(
                        "Notice: The automated workflow processing for thread '{}' has been manually stopped by the system administrator.",
                        parsed.subject
                    ),
                    hop_count: parsed.hop_count,
                    trace_workflows: parsed.trace_workflows,
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
            company::Company,
            message::Message,
            task::{BackgroundTask, TaskStatus},
            thread::Thread,
            workflow::Workflow,
        },
        use_cases::{
            company::CompanyPersistence,
            thread::ThreadPersistence,
            workflow::WorkflowPersistence,
        },
    };

    struct MockCompanyPersistence;
    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>) -> AppResult<Company> { unimplemented!() }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Company>> { unimplemented!() }
        async fn get_by_slug(&self, _slug: &str) -> AppResult<Option<Company>> { unimplemented!() }
        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> { unimplemented!() }
        async fn update(&self, _id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>) -> AppResult<Company> { unimplemented!() }
        async fn delete(&self, _id: Uuid) -> AppResult<()> { unimplemented!() }
    }

    struct MockWorkflowPersistence;
    #[async_trait]
    impl WorkflowPersistence for MockWorkflowPersistence {
        async fn create(&self, _company_id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>, _participant_emails: Option<Vec<String>>, _agent_ids: Option<Vec<Uuid>>, _workflow_config: Option<serde_json::Value>) -> AppResult<Workflow> { unimplemented!() }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Workflow>> { unimplemented!() }
        async fn get_by_company_slug_and_workflow_slug(&self, _company_slug: &str, _workflow_slug: &str) -> AppResult<Option<Workflow>> { unimplemented!() }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Workflow>> { unimplemented!() }
        async fn update(&self, _id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>, _participant_emails: Option<Vec<String>>, _agent_ids: Option<Vec<Uuid>>, _workflow_config: Option<serde_json::Value>) -> AppResult<Workflow> { unimplemented!() }
        async fn delete(&self, _id: Uuid) -> AppResult<()> { unimplemented!() }
    }

    struct MockThreadPersistence {
        messages: Mutex<Vec<Message>>,
    }

    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(&self, _workflow_id: Uuid, _subject: &str, _participant_emails: &[String]) -> AppResult<Thread> { unimplemented!() }
        async fn get_thread_by_id(&self, _id: Uuid) -> AppResult<Option<Thread>> { unimplemented!() }
        async fn update_thread_participants(&self, _id: Uuid, _participant_emails: &[String]) -> AppResult<Thread> { unimplemented!() }
        async fn find_thread_by_message_ids(&self, _message_ids: &[String]) -> AppResult<Option<Thread>> { unimplemented!() }
        async fn find_thread_by_thread_index(&self, _thread_index_prefix: &str) -> AppResult<Option<Thread>> { unimplemented!() }
        async fn count_recent_messages(&self, _thread_id: Uuid, _duration_secs: i64) -> AppResult<usize> { unimplemented!() }
        async fn create_message(&self, message: &Message) -> AppResult<Message> {
            self.messages.lock().unwrap().push(message.clone());
            Ok(message.clone())
        }
        async fn get_message_by_message_id(&self, _message_id: &str) -> AppResult<Option<Message>> { unimplemented!() }
        async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
            Ok(self.messages.lock().unwrap().iter().filter(|m| m.thread_id == thread_id).cloned().collect())
        }
    }

    struct MockTaskPersistence {
        tasks: Mutex<Vec<BackgroundTask>>,
    }

    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        async fn enqueue_task(&self, company_id: Uuid, workflow_id: Uuid, thread_id: Option<Uuid>, task_type: &str, payload: serde_json::Value) -> AppResult<BackgroundTask> {
            let task = BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                workflow_id,
                thread_id,
                task_type: task_type.to_string(),
                status: TaskStatus::Pending,
                payload,
                retry_count: 0,
                max_retries: 3,
                last_error: None,
                run_at: Utc::now().naive_utc(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            };
            self.tasks.lock().unwrap().push(task.clone());
            Ok(task)
        }

        async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>> {
            Ok(self.tasks.lock().unwrap().iter().find(|t| t.id == id).cloned())
        }

        async fn update_task_payload(&self, id: Uuid, payload: serde_json::Value) -> AppResult<()> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.payload = payload;
            }
            Ok(())
        }

        async fn poll_next_pending_tasks(&self, _limit: i64) -> AppResult<Vec<BackgroundTask>> {
            Ok(self.tasks.lock().unwrap().iter().filter(|t| t.status == TaskStatus::Pending).cloned().collect())
        }

        async fn mark_task_processing(&self, id: Uuid) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id && t.status == TaskStatus::Pending) {
                t.status = TaskStatus::Processing;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn mark_task_completed(&self, id: Uuid) -> AppResult<()> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.status = TaskStatus::Completed;
            }
            Ok(())
        }

        async fn mark_task_failed(&self, id: Uuid, error_msg: &str, _next_run_at: chrono::NaiveDateTime, is_dead_letter: bool) -> AppResult<()> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.last_error = Some(error_msg.to_string());
                t.status = if is_dead_letter { TaskStatus::DeadLetter } else { TaskStatus::Failed };
            }
            Ok(())
        }

        async fn stop_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = TaskStatus::Stopped;
            Ok(t.clone())
        }

        async fn resume_task(&self, id: Uuid) -> AppResult<BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = TaskStatus::Pending;
            Ok(t.clone())
        }

        async fn list_company_tasks(&self, company_id: Uuid, _workflow_id: Option<Uuid>, _status: Option<TaskStatus>, _sort_asc: bool) -> AppResult<Vec<BackgroundTask>> {
            Ok(self.tasks.lock().unwrap().iter().filter(|t| t.company_id == company_id).cloned().collect())
        }
    }

    #[tokio::test]
    async fn test_task_worker_stop_and_resume_flow() {
        let task_persistence = Arc::new(MockTaskPersistence { tasks: Mutex::new(Vec::new()) });
        let thread_persistence = Arc::new(MockThreadPersistence { messages: Mutex::new(Vec::new()) });
        let company_persistence = Arc::new(MockCompanyPersistence);
        let workflow_persistence = Arc::new(MockWorkflowPersistence);

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
        });

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence,
            workflow_persistence,
            company_persistence,
            task_persistence.clone(),
            config.clone(),
        ));

        let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases, config);

        let company_id = Uuid::new_v4();
        let workflow_id = Uuid::new_v4();

        let task = task_persistence.enqueue_task(company_id, workflow_id, None, "email_agent_dispatch", serde_json::json!({})).await.unwrap();
        assert_eq!(task.status, TaskStatus::Pending);

        // Stop task
        worker.stop_task_and_notify(task.id).await.unwrap();
        let stopped_task = task_persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        assert_eq!(stopped_task.status, TaskStatus::Stopped);

        // Resume task
        worker.resume_task(task.id).await.unwrap();
        let resumed_task = task_persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        assert_eq!(resumed_task.status, TaskStatus::Pending);
    }

    #[tokio::test]
    async fn test_task_worker_marks_task_failed_on_agent_runner_failure() {
        let company_id = Uuid::new_v4();
        let workflow_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();

        let task_persistence = Arc::new(MockTaskPersistence { tasks: Mutex::new(Vec::new()) });
        let thread_persistence = Arc::new(MockThreadPersistence { messages: Mutex::new(Vec::new()) });

        let company = crate::entities::company::Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Corp".to_string(),
            slug: "test".to_string(),
            api_key: None,
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            created_at: chrono::Utc::now().naive_utc(),
        };

        let workflow = crate::entities::workflow::Workflow {
            id: workflow_id,
            company_id,
            name: "Support".to_string(),
            slug: "support".to_string(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            workflow_config: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let company_persistence = Arc::new(MockCompanyPersistence);
        let workflow_persistence = Arc::new(MockWorkflowPersistence);

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
        });

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence,
            workflow_persistence,
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
        };
        let parsed_email = crate::services::email_parser::EmailParser::parse(raw, "mailagents.com");

        let ingest = crate::use_cases::thread::InboundIngestResult {
            accepted: true,
            reason: None,
            thread: Some(crate::entities::thread::Thread {
                id: thread_id,
                workflow_id,
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
            workflow: Some(workflow),
            parsed_email: Some(parsed_email),
            task_id: None,
        };

        let payload_json = serde_json::to_value(&ingest).unwrap();
        let task = task_persistence.enqueue_task(company_id, workflow_id, Some(thread_id), "email_agent_dispatch", payload_json).await.unwrap();

        worker.process_next_batch().await.unwrap();

        let failed_task = task_persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        assert_eq!(failed_task.status, TaskStatus::Failed);
        assert!(failed_task.last_error.unwrap().contains("API key is missing"));
    }
}
