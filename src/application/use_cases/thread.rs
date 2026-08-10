use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    adapters::persistence::task::TaskPersistence,
    app_error::AppResult,
    entities::{
        company::Company,
        message::{Message, MessageDirection, MessageRole},
        thread::Thread,
        workflow::Workflow,
    },
    infra::config::AppConfig,
    services::{
        agent_runner::AgentRunner,
        email_parser::{EmailParser, MAX_WORKFLOW_HOPS, ParsedEmail, RawInboundPayload},
        outbound_dispatcher::{OutboundDispatcher, OutboundEmail},
    },
    use_cases::{
        company::CompanyPersistence,
        workflow::{WorkflowPersistence, parse_recipient_address},
    },
};

pub const MAX_THREAD_MESSAGES_PER_HOUR: usize = 20;

#[async_trait]
pub trait ThreadPersistence: Send + Sync {
    async fn create_thread(
        &self,
        workflow_id: Uuid,
        subject: &str,
        participant_emails: &[String],
    ) -> AppResult<Thread>;

    async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>>;

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[String],
    ) -> AppResult<Thread>;

    async fn find_thread_by_message_ids(&self, message_ids: &[String])
    -> AppResult<Option<Thread>>;

    async fn find_thread_by_thread_index(
        &self,
        thread_index_prefix: &str,
    ) -> AppResult<Option<Thread>>;

    async fn count_recent_messages(&self, thread_id: Uuid, duration_secs: i64) -> AppResult<usize>;

    async fn create_message(&self, message: &Message) -> AppResult<Message>;

    async fn get_message_by_message_id(&self, message_id: &str) -> AppResult<Option<Message>>;

    async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>>;
}

#[derive(Clone)]
pub struct ThreadUseCases {
    thread_persistence: Arc<dyn ThreadPersistence>,
    workflow_persistence: Arc<dyn WorkflowPersistence>,
    company_persistence: Arc<dyn CompanyPersistence>,
    task_persistence: Arc<dyn TaskPersistence>,
    config: Arc<AppConfig>,
}

impl ThreadUseCases {
    pub fn new(
        thread_persistence: Arc<dyn ThreadPersistence>,
        workflow_persistence: Arc<dyn WorkflowPersistence>,
        company_persistence: Arc<dyn CompanyPersistence>,
        task_persistence: Arc<dyn TaskPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            thread_persistence,
            workflow_persistence,
            company_persistence,
            task_persistence,
            config,
        }
    }

    #[instrument(skip(self, raw_payload))]
    pub async fn ingest_and_save_inbound_message(
        &self,
        raw_payload: RawInboundPayload,
    ) -> AppResult<InboundIngestResult> {
        let parsed = EmailParser::parse(raw_payload, &self.config.app_domain_name);

        info!(
            "Ingesting email Message-ID: {}, From: {}, To: {:?}",
            parsed.message_id, parsed.sender, parsed.recipients_to
        );

        // SendGrid Webhook Redelivery Idempotency Check
        if let Ok(Some(_)) = self
            .thread_persistence
            .get_message_by_message_id(&parsed.message_id)
            .await
        {
            warn!(
                "SendGrid Webhook Redelivery: Duplicate Message-ID '{}' already processed",
                parsed.message_id
            );
            return Ok(InboundIngestResult::rejected(
                "Duplicate Message-ID already processed",
            ));
        }
        let recipient_str = parsed.recipients_to.first().cloned().unwrap_or_default();
        let recipient_parse = parse_recipient_address(&recipient_str, &self.config.app_domain_name);

        let (company_slug, workflow_slug) = match recipient_parse {
            Some(res) => res,
            None => {
                warn!("Invalid recipient address format: '{}'", recipient_str);
                return Ok(InboundIngestResult::rejected(
                    "Invalid recipient address format",
                ));
            }
        };

        let company = self.company_persistence.get_by_slug(&company_slug).await?;
        let workflow = self
            .workflow_persistence
            .get_by_company_slug_and_workflow_slug(&company_slug, &workflow_slug)
            .await?;

        let (company, workflow) = match (company, workflow) {
            (Some(c), Some(w)) => (c, w),
            _ => {
                warn!(
                    "Company '{}' or Workflow '{}' not found",
                    company_slug, workflow_slug
                );
                return Ok(InboundIngestResult::rejected(
                    "Company or Workflow not found",
                ));
            }
        };

        // ACL Check with SPF/DKIM verification when participants are restricted
        let is_inter_workflow = parsed.workflow_id_header.is_some()
            || parsed
                .sender
                .ends_with(&format!(".{}", self.config.app_domain_name));

        if let Some(ref allowed) = workflow.participant_emails {
            if !allowed.is_empty() {
                let sender_allowed = allowed
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(&parsed.sender));
                if !sender_allowed && !is_inter_workflow {
                    warn!(
                        "Sender '{}' unauthorized for workflow '{}'",
                        parsed.sender, workflow.slug
                    );
                    return Ok(InboundIngestResult::rejected(
                        "Sender unauthorized for workflow",
                    ));
                }

                // Check SPF / DKIM verification failure
                if let Some(ref spf) = parsed.spf_status {
                    if spf.eq_ignore_ascii_case("fail") {
                        warn!("SPF authentication failed for sender '{}'", parsed.sender);
                        return Ok(InboundIngestResult::rejected("SPF authentication failed"));
                    }
                }
                if let Some(ref dkim) = parsed.dkim_status {
                    if dkim.eq_ignore_ascii_case("fail") {
                        warn!("DKIM authentication failed for sender '{}'", parsed.sender);
                        return Ok(InboundIngestResult::rejected("DKIM authentication failed"));
                    }
                }
            }
        }

        // Loop Guard Engine & Inter-Workflow Guards
        if is_inter_workflow {
            // Hop limit check
            if parsed.hop_count >= MAX_WORKFLOW_HOPS {
                warn!(
                    "Max inter-workflow hop count ({}) reached for Message-ID: {}",
                    parsed.hop_count, parsed.message_id
                );
                return Ok(InboundIngestResult::rejected(
                    "Max inter-workflow hop count reached",
                ));
            }

            // Cycle detection
            if parsed.trace_workflows.contains(&workflow.id) {
                warn!(
                    "Inter-workflow cycle detected for workflow '{}' in Message-ID: {}",
                    workflow.id, parsed.message_id
                );
                return Ok(InboundIngestResult::rejected(
                    "Inter-workflow loop cycle detected",
                ));
            }
        } else if parsed.is_auto_reply {
            warn!(
                "External auto-reply loop detected for Message-ID: {}, dropping message",
                parsed.message_id
            );
            return Ok(InboundIngestResult::rejected(
                "External auto-reply loop detected",
            ));
        }

        // Thread Resolution (RFC 5322 Message-ID / References OR Outlook Thread-Index)
        let mut lookup_ids = Vec::new();
        if let Some(ref reply_id) = parsed.in_reply_to {
            lookup_ids.push(reply_id.clone());
        }
        lookup_ids.extend(parsed.references.clone());

        let mut existing_thread = if !lookup_ids.is_empty() {
            self.thread_persistence
                .find_thread_by_message_ids(&lookup_ids)
                .await?
        } else {
            None
        };

        // Fallback to Outlook Thread-Index
        if existing_thread.is_none() {
            if let Some(ref idx) = parsed.thread_index {
                existing_thread = self
                    .thread_persistence
                    .find_thread_by_thread_index(idx)
                    .await?;
            }
        }

        let thread = match existing_thread {
            Some(t) if t.workflow_id == workflow.id => t,
            _ => {
                let mut participants = vec![parsed.sender.clone()];
                participants.extend(parsed.recipients_cc.clone());
                participants.dedup();

                self.thread_persistence
                    .create_thread(workflow.id, &parsed.subject, &participants)
                    .await?
            }
        };

        // Thread Turn Limit Check (Max 20 messages / hour per thread)
        let recent_count = self
            .thread_persistence
            .count_recent_messages(thread.id, 3600)
            .await?;
        if recent_count >= MAX_THREAD_MESSAGES_PER_HOUR {
            warn!(
                "Thread turn limit ({}/hr) exceeded for thread_id {}, dropping response to prevent ping-pong loop",
                recent_count, thread.id
            );
            return Ok(InboundIngestResult::rejected("Thread turn limit exceeded"));
        }

        // Update thread participants
        let mut current_participants = thread.participant_emails.clone();
        let mut participant_added = false;
        if !current_participants
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&parsed.sender))
        {
            current_participants.push(parsed.sender.clone());
            participant_added = true;
        }
        for cc in &parsed.recipients_cc {
            if !current_participants
                .iter()
                .any(|p| p.eq_ignore_ascii_case(cc))
            {
                current_participants.push(cc.clone());
                participant_added = true;
            }
        }
        let thread = if participant_added {
            self.thread_persistence
                .update_thread_participants(thread.id, &current_participants)
                .await?
        } else {
            thread
        };

        // Fetch thread history for quote stripping fallback
        let history_messages = self
            .thread_persistence
            .list_messages_by_thread_id(thread.id)
            .await?;
        let history_clean_bodies: Vec<String> = history_messages
            .iter()
            .map(|m| m.clean_text_body.clone())
            .collect();

        let clean_text_body = if parsed.is_forwarded {
            parsed.clean_text_body.clone()
        } else {
            EmailParser::strip_historical_quotes_fallback(
                &parsed.clean_text_body,
                &history_clean_bodies,
            )
        };

        // Role assignment: Agent if from another workflow, Human otherwise
        let inbound_role = if is_inter_workflow {
            MessageRole::Agent
        } else {
            MessageRole::Human
        };

        let inbound_msg_id = Uuid::new_v4();
        let inbound_message = Message {
            id: inbound_msg_id,
            thread_id: thread.id,
            message_id: parsed.message_id.clone(),
            in_reply_to: parsed.in_reply_to.clone(),
            references_list: parsed.references.clone(),
            sender: parsed.sender.clone(),
            recipients_to: parsed.recipients_to.clone(),
            recipients_cc: parsed.recipients_cc.clone(),
            subject: parsed.subject.clone(),
            clean_text_body,
            raw_text_body: parsed.raw_text_body.clone(),
            raw_html_body: parsed.raw_html_body.clone(),
            attachments: if parsed.attachments.is_empty() {
                None
            } else {
                Some(parsed.attachments.clone())
            },
            direction: MessageDirection::Inbound,
            role: inbound_role,
            thread_index: parsed.thread_index.clone(),
            created_at: chrono::Utc::now().naive_utc(),
        };

        let saved_inbound = self
            .thread_persistence
            .create_message(&inbound_message)
            .await?;

        let ingest_result = InboundIngestResult {
            accepted: true,
            reason: None,
            thread: Some(thread.clone()),
            inbound_message: Some(saved_inbound.clone()),
            company: Some(company.clone()),
            workflow: Some(workflow.clone()),
            parsed_email: Some(parsed.clone()),
        };

        // Enqueue background task for durable processing & crash recovery
        let payload_json = serde_json::to_value(&ingest_result).unwrap_or_default();
        let _ = self
            .task_persistence
            .enqueue_task(
                company.id,
                workflow.id,
                Some(thread.id),
                "email_agent_dispatch",
                payload_json,
            )
            .await;

        Ok(ingest_result)
    }

    pub async fn execute_agent_and_dispatch(
        &self,
        ingest: &InboundIngestResult,
        send_email: bool,
    ) -> AppResult<Option<AgentExecutionResult>> {
        let (thread, company, workflow, parsed) = match (
            &ingest.thread,
            &ingest.company,
            &ingest.workflow,
            &ingest.parsed_email,
        ) {
            (Some(t), Some(c), Some(w), Some(p)) => (t, c, w, p),
            _ => return Ok(None),
        };

        let history_messages = self
            .thread_persistence
            .list_messages_by_thread_id(thread.id)
            .await?;

        let api_key = workflow
            .api_key
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| company.api_key.as_deref().filter(|s| !s.trim().is_empty()));

        let provider = workflow
            .provider
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| company.provider.as_deref().filter(|s| !s.trim().is_empty()));

        let model = workflow
            .model
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| company.model.as_deref().filter(|s| !s.trim().is_empty()));

        // Execute AI Agent
        let runner_res = AgentRunner::new(&parsed.prompt_text)
            .history(&history_messages)
            .workflow_config(workflow.workflow_config.as_ref())
            .api_key(api_key)
            .provider(provider)
            .model(model)
            .execute()
            .await;

        let (agent_response, execution_error) = match runner_res {
            Ok(res) => (res, None),
            Err(err) => {
                let err_msg = format!("Agent execution failed: {err}");
                (err_msg.clone(), Some(err_msg))
            }
        };

        let (
            sent_message_id,
            in_reply_to,
            references,
            from_address,
            recipients_to,
            recipients_cc,
            subject,
            email_sent,
        ) = if send_email {
            // Construct Outbound Email
            let mut references_for_outbound = parsed.references.clone();
            if let Some(ref reply_to) = parsed.in_reply_to {
                if !references_for_outbound.contains(reply_to) {
                    references_for_outbound.push(reply_to.clone());
                }
            }

            let mut outbound_cc = parsed.recipients_cc.clone();
            if let Some(ref wf_participants) = workflow.participant_emails {
                for p in wf_participants {
                    if !p.eq_ignore_ascii_case(&parsed.sender) && !outbound_cc.contains(p) {
                        outbound_cc.push(p.clone());
                    }
                }
            }

            let outbound_email = OutboundEmail {
                workflow_id: workflow.id,
                workflow_name: workflow.name.clone(),
                workflow_slug: workflow.slug.clone(),
                company_slug: company.slug.clone(),
                trigger_message_id: parsed.message_id.clone(),
                thread_references: references_for_outbound,
                recipient_to: parsed.sender.clone(),
                recipients_cc: outbound_cc,
                subject: parsed.subject.clone(),
                body_text: agent_response.clone(),
                hop_count: parsed.hop_count,
                trace_workflows: parsed.trace_workflows.clone(),
            };

            let sent_result = OutboundDispatcher::send(&self.config, outbound_email).await?;
            (
                sent_result.outbound_message_id,
                sent_result.in_reply_to,
                sent_result.references,
                sent_result.from_address,
                sent_result.recipients_to,
                sent_result.recipients_cc,
                sent_result.subject,
                true,
            )
        } else {
            let outbound_uuid = Uuid::new_v4();
            let simulated_msg_id = format!(
                "<simulated-test-{}@{}>",
                outbound_uuid, self.config.app_domain_name
            );
            let from_email = format!(
                "{}@{}.{}",
                workflow.slug, company.slug, self.config.app_domain_name
            );
            info!(
                "Simulation test mode (Run_Test): Skipped SMTP email dispatch for Message-ID {}",
                simulated_msg_id
            );
            (
                simulated_msg_id,
                parsed.message_id.clone(),
                parsed.references.clone(),
                from_email,
                vec![parsed.sender.clone()],
                parsed.recipients_cc.clone(),
                if parsed.subject.to_lowercase().starts_with("re:") {
                    parsed.subject.clone()
                } else {
                    format!("Re: {}", parsed.subject)
                },
                false,
            )
        };

        // Save Outbound Agent Message
        let outbound_msg_id = Uuid::new_v4();
        let outbound_message = Message {
            id: outbound_msg_id,
            thread_id: thread.id,
            message_id: sent_message_id.clone(),
            in_reply_to: Some(in_reply_to),
            references_list: references,
            sender: from_address,
            recipients_to,
            recipients_cc,
            subject,
            clean_text_body: agent_response.clone(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let _ = self
            .thread_persistence
            .create_message(&outbound_message)
            .await?;

        if let Some(err_msg) = execution_error {
            return Err(crate::app_error::AppError::Internal(err_msg));
        }

        Ok(Some(AgentExecutionResult {
            outbound_message_id: Some(sent_message_id),
            agent_response,
            email_sent,
        }))
    }

    pub async fn execute_simulation(
        &self,
        raw_payload: RawInboundPayload,
        mode: SimulationMode,
    ) -> AppResult<SimulationExecutionResult> {
        let ingest = self.ingest_and_save_inbound_message(raw_payload).await?;

        if !ingest.accepted {
            return Ok(SimulationExecutionResult {
                ingest_result: ingest,
                agent_execution: None,
                simulation_mode: mode,
            });
        }

        let send_email = mode == SimulationMode::Run;
        let agent_execution = self.execute_agent_and_dispatch(&ingest, send_email).await?;

        Ok(SimulationExecutionResult {
            ingest_result: ingest,
            agent_execution,
            simulation_mode: mode,
        })
    }

    pub async fn process_and_dispatch_email(
        &self,
        raw_payload: RawInboundPayload,
    ) -> AppResult<ProcessEmailResult> {
        let ingest = self.ingest_and_save_inbound_message(raw_payload).await?;
        if !ingest.accepted {
            return Ok(ProcessEmailResult {
                processed: false,
                reason: ingest.reason,
                thread_id: None,
                inbound_message_id: None,
                outbound_message_id: None,
            });
        }

        let thread_id = ingest.thread.as_ref().map(|t| t.id);
        let inbound_message_id = ingest
            .inbound_message
            .as_ref()
            .map(|m| m.message_id.clone());

        let agent_res = self.execute_agent_and_dispatch(&ingest, true).await?;
        let outbound_msg_id = agent_res.and_then(|r| r.outbound_message_id);

        Ok(ProcessEmailResult {
            processed: true,
            reason: None,
            thread_id,
            inbound_message_id,
            outbound_message_id: outbound_msg_id,
        })
    }

    pub async fn get_thread_history(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
        self.thread_persistence
            .list_messages_by_thread_id(thread_id)
            .await
    }

    pub async fn list_company_tasks(
        &self,
        company_id: Uuid,
        workflow_id: Option<Uuid>,
        status: Option<crate::entities::task::TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
        self.task_persistence
            .list_company_tasks(company_id, workflow_id, status, sort_asc)
            .await
    }

    pub async fn get_task_persistence(&self) -> Arc<dyn TaskPersistence> {
        self.task_persistence.clone()
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundIngestResult {
    pub accepted: bool,
    pub reason: Option<String>,
    pub thread: Option<Thread>,
    pub inbound_message: Option<Message>,
    pub company: Option<Company>,
    pub workflow: Option<Workflow>,
    pub parsed_email: Option<ParsedEmail>,
}

impl InboundIngestResult {
    pub fn rejected(reason: &str) -> Self {
        Self {
            accepted: false,
            reason: Some(reason.to_string()),
            thread: None,
            inbound_message: None,
            company: None,
            workflow: None,
            parsed_email: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulationMode {
    Verify,
    RunTest,
    Run,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionResult {
    pub outbound_message_id: Option<String>,
    pub agent_response: String,
    pub email_sent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationExecutionResult {
    pub ingest_result: InboundIngestResult,
    pub agent_execution: Option<AgentExecutionResult>,
    pub simulation_mode: SimulationMode,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessEmailResult {
    pub processed: bool,
    pub reason: Option<String>,
    pub thread_id: Option<Uuid>,
    pub inbound_message_id: Option<String>,
    pub outbound_message_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::sync::Mutex;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>) -> AppResult<Company> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Company>> {
            unimplemented!()
        }
        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug == slug)
                .cloned())
        }
        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }
        async fn update(&self, _id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockWorkflowPersistence {
        workflows: Mutex<Vec<Workflow>>,
    }

    #[async_trait]
    impl WorkflowPersistence for MockWorkflowPersistence {
        async fn create(
            &self,
            _company_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _workflow_config: Option<serde_json::Value>,
        ) -> AppResult<Workflow> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Workflow>> {
            unimplemented!()
        }
        async fn get_by_company_slug_and_workflow_slug(
            &self,
            _company_slug: &str,
            workflow_slug: &str,
        ) -> AppResult<Option<Workflow>> {
            Ok(self
                .workflows
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.slug == workflow_slug)
                .cloned())
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Workflow>> {
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
            _participant_emails: Option<Vec<String>>,
            _workflow_config: Option<serde_json::Value>,
        ) -> AppResult<Workflow> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockThreadPersistence {
        threads: Mutex<Vec<Thread>>,
        messages: Mutex<Vec<Message>>,
    }

    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(
            &self,
            workflow_id: Uuid,
            subject: &str,
            participant_emails: &[String],
        ) -> AppResult<Thread> {
            let thread = Thread {
                id: Uuid::new_v4(),
                workflow_id,
                subject: subject.to_string(),
                participant_emails: participant_emails.to_vec(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            };
            self.threads.lock().unwrap().push(thread.clone());
            Ok(thread)
        }

        async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>> {
            Ok(self
                .threads
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn update_thread_participants(
            &self,
            id: Uuid,
            participant_emails: &[String],
        ) -> AppResult<Thread> {
            let mut list = self.threads.lock().unwrap();
            let thread = list.iter_mut().find(|t| t.id == id).unwrap();
            thread.participant_emails = participant_emails.to_vec();
            Ok(thread.clone())
        }

        async fn find_thread_by_message_ids(
            &self,
            message_ids: &[String],
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| message_ids.contains(&m.message_id))
                    .map(|m| m.thread_id)
            };
            if let Some(tid) = thread_id {
                return self.get_thread_by_id(tid).await;
            }
            Ok(None)
        }

        async fn find_thread_by_thread_index(
            &self,
            thread_index_prefix: &str,
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| {
                        m.thread_index
                            .as_deref()
                            .unwrap_or_default()
                            .starts_with(thread_index_prefix)
                    })
                    .map(|m| m.thread_id)
            };
            if let Some(tid) = thread_id {
                return self.get_thread_by_id(tid).await;
            }
            Ok(None)
        }

        async fn count_recent_messages(
            &self,
            thread_id: Uuid,
            _duration_secs: i64,
        ) -> AppResult<usize> {
            let msgs = self.messages.lock().unwrap();
            Ok(msgs.iter().filter(|m| m.thread_id == thread_id).count())
        }

        async fn create_message(&self, message: &Message) -> AppResult<Message> {
            self.messages.lock().unwrap().push(message.clone());
            Ok(message.clone())
        }

        async fn get_message_by_message_id(&self, message_id: &str) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.message_id == message_id)
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
        tasks: Mutex<Vec<crate::entities::task::BackgroundTask>>,
    }

    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        async fn enqueue_task(
            &self,
            company_id: Uuid,
            workflow_id: Uuid,
            thread_id: Option<Uuid>,
            task_type: &str,
            payload: serde_json::Value,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            let task = crate::entities::task::BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                workflow_id,
                thread_id,
                task_type: task_type.to_string(),
                status: crate::entities::task::TaskStatus::Pending,
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

        async fn get_task_by_id(
            &self,
            id: Uuid,
        ) -> AppResult<Option<crate::entities::task::BackgroundTask>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn poll_next_pending_tasks(
            &self,
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.status == crate::entities::task::TaskStatus::Pending)
                .cloned()
                .collect())
        }

        async fn mark_task_processing(&self, id: Uuid) -> AppResult<()> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.status = crate::entities::task::TaskStatus::Processing;
            }
            Ok(())
        }

        async fn mark_task_completed(&self, id: Uuid) -> AppResult<()> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.status = crate::entities::task::TaskStatus::Completed;
            }
            Ok(())
        }

        async fn mark_task_failed(
            &self,
            id: Uuid,
            error_msg: &str,
            _next_run_at: chrono::NaiveDateTime,
            is_dead_letter: bool,
        ) -> AppResult<()> {
            let mut list = self.tasks.lock().unwrap();
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.last_error = Some(error_msg.to_string());
                t.status = if is_dead_letter {
                    crate::entities::task::TaskStatus::DeadLetter
                } else {
                    crate::entities::task::TaskStatus::Failed
                };
            }
            Ok(())
        }

        async fn stop_task(&self, id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = crate::entities::task::TaskStatus::Stopped;
            Ok(t.clone())
        }

        async fn resume_task(&self, id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = crate::entities::task::TaskStatus::Pending;
            Ok(t.clone())
        }

        async fn list_company_tasks(
            &self,
            company_id: Uuid,
            _workflow_id: Option<Uuid>,
            _status: Option<crate::entities::task::TaskStatus>,
            _sort_asc: bool,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
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
    async fn test_inter_workflow_hop_limit_rejection() {
        let company_id = Uuid::new_v4();
        let workflow_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let workflow_persistence = Arc::new(MockWorkflowPersistence {
            workflows: Mutex::new(vec![Workflow {
                id: workflow_id,
                company_id,
                name: "Inbound Flow".to_string(),
                slug: "inbound".to_string(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                workflow_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
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
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            workflow_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "inbound@acme.mailagents.com".to_string(),
            from: "other_wf@acme.mailagents.com".to_string(),
            subject: Some("Test Inter Workflow".to_string()),
            text: Some("Hello".to_string()),
            headers: Some(format!(
                "X-MailAgents-Workflow-ID: {}\nX-MailAgents-Hop-Count: 5\n",
                Uuid::new_v4()
            )),
            ..Default::default()
        };

        let result = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();
        assert!(!result.accepted);
        assert_eq!(
            result.reason.as_deref(),
            Some("Max inter-workflow hop count reached")
        );
    }

    #[tokio::test]
    async fn test_spf_authentication_failure_rejection() {
        let company_id = Uuid::new_v4();
        let workflow_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let workflow_persistence = Arc::new(MockWorkflowPersistence {
            workflows: Mutex::new(vec![Workflow {
                id: workflow_id,
                company_id,
                name: "Restricted Flow".to_string(),
                slug: "restricted".to_string(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["agent@example.com".to_string()]),
                workflow_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
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
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = ThreadUseCases::new(
            thread_persistence,
            workflow_persistence,
            company_persistence,
            task_persistence,
            config,
        );

        let raw_payload = RawInboundPayload {
            to: "restricted@acme.mailagents.com".to_string(),
            from: "agent@example.com".to_string(),
            subject: Some("Spoofed email".to_string()),
            text: Some("Hello".to_string()),
            spf: Some("fail".to_string()),
            ..Default::default()
        };

        let result = thread_use_cases
            .ingest_and_save_inbound_message(raw_payload)
            .await
            .unwrap();
        assert!(!result.accepted);
        assert_eq!(result.reason.as_deref(), Some("SPF authentication failed"));
    }
}
