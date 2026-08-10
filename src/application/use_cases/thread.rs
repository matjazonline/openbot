use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        message::{Message, MessageDirection, MessageRole},
        thread::Thread,
    },
    infra::config::AppConfig,
    services::{
        agent_runner::AgentRunner,
        email_parser::{EmailParser, RawInboundPayload},
        outbound_dispatcher::{OutboundDispatcher, OutboundEmail},
    },
    use_cases::{
        company::CompanyPersistence,
        workflow::{parse_recipient_address, WorkflowPersistence},
    },
};

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

    async fn find_thread_by_message_ids(&self, message_ids: &[String]) -> AppResult<Option<Thread>>;

    async fn create_message(&self, message: &Message) -> AppResult<Message>;

    async fn get_message_by_message_id(&self, message_id: &str) -> AppResult<Option<Message>>;

    async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>>;
}

#[derive(Clone)]
pub struct ThreadUseCases {
    thread_persistence: Arc<dyn ThreadPersistence>,
    workflow_persistence: Arc<dyn WorkflowPersistence>,
    company_persistence: Arc<dyn CompanyPersistence>,
    config: Arc<AppConfig>,
}

impl ThreadUseCases {
    pub fn new(
        thread_persistence: Arc<dyn ThreadPersistence>,
        workflow_persistence: Arc<dyn WorkflowPersistence>,
        company_persistence: Arc<dyn CompanyPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            thread_persistence,
            workflow_persistence,
            company_persistence,
            config,
        }
    }

    #[instrument(skip(self, raw_payload))]
    pub async fn process_and_dispatch_email(
        &self,
        raw_payload: RawInboundPayload,
    ) -> AppResult<ProcessEmailResult> {
        let parsed = EmailParser::parse(raw_payload, &self.config.app_domain_name);

        info!(
            "Processing email Message-ID: {}, From: {}, To: {:?}",
            parsed.message_id, parsed.sender, parsed.recipients_to
        );

        let recipient_str = parsed.recipients_to.first().cloned().unwrap_or_default();
        let recipient_parse = parse_recipient_address(&recipient_str, &self.config.app_domain_name);

        let (company_slug, workflow_slug) = match recipient_parse {
            Some(res) => res,
            None => {
                warn!("Invalid recipient address format: '{}'", recipient_str);
                return Ok(ProcessEmailResult {
                    processed: false,
                    reason: Some("Invalid recipient address format".into()),
                    thread_id: None,
                    inbound_message_id: None,
                    outbound_message_id: None,
                });
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
                warn!("Company '{}' or Workflow '{}' not found", company_slug, workflow_slug);
                return Ok(ProcessEmailResult {
                    processed: false,
                    reason: Some("Company or Workflow not found".into()),
                    thread_id: None,
                    inbound_message_id: None,
                    outbound_message_id: None,
                });
            }
        };

        // ACL Check
        let sender_authorized = match &workflow.participant_emails {
            Some(allowed) if !allowed.is_empty() => {
                allowed.iter().any(|e| e.eq_ignore_ascii_case(&parsed.sender))
            }
            _ => true,
        };

        if !sender_authorized {
            warn!("Sender '{}' not authorized for workflow '{}'", parsed.sender, workflow.slug);
            return Ok(ProcessEmailResult {
                processed: false,
                reason: Some("Sender unauthorized for workflow".into()),
                thread_id: None,
                inbound_message_id: None,
                outbound_message_id: None,
            });
        }

        // Thread Resolution
        let mut lookup_ids = Vec::new();
        if let Some(ref reply_id) = parsed.in_reply_to {
            lookup_ids.push(reply_id.clone());
        }
        lookup_ids.extend(parsed.references.clone());

        let existing_thread = if !lookup_ids.is_empty() {
            self.thread_persistence.find_thread_by_message_ids(&lookup_ids).await?
        } else {
            None
        };

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

        // Update participants list if new participants arrived
        let mut current_participants = thread.participant_emails.clone();
        let mut participant_added = false;
        if !current_participants.iter().any(|p| p.eq_ignore_ascii_case(&parsed.sender)) {
            current_participants.push(parsed.sender.clone());
            participant_added = true;
        }
        for cc in &parsed.recipients_cc {
            if !current_participants.iter().any(|p| p.eq_ignore_ascii_case(cc)) {
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

        // Fetch thread history for quote stripping fallback and agent context
        let history_messages = self.thread_persistence.list_messages_by_thread_id(thread.id).await?;
        let history_clean_bodies: Vec<String> = history_messages
            .iter()
            .map(|m| m.clean_text_body.clone())
            .collect();

        // Fallback quote stripping
        let clean_text_body = EmailParser::strip_historical_quotes_fallback(
            &parsed.clean_text_body,
            &history_clean_bodies,
        );

        // Save Inbound Message
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
            attachments: if parsed.attachments.is_empty() { None } else { Some(parsed.attachments) },
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let saved_inbound = self.thread_persistence.create_message(&inbound_msg_id_message(&inbound_message)).await?;

        // Run AI Agent
        let agent_response = AgentRunner::execute(
            workflow.workflow_config.as_ref(),
            &parsed.prompt_text,
            &history_messages,
        )
        .await;

        // Outbound Dispatch
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
            workflow_name: workflow.name.clone(),
            workflow_slug: workflow.slug.clone(),
            company_slug: company.slug.clone(),
            trigger_message_id: parsed.message_id.clone(),
            thread_references: references_for_outbound,
            recipient_to: parsed.sender.clone(),
            recipients_cc: outbound_cc,
            subject: parsed.subject.clone(),
            body_text: agent_response,
        };

        let sent_result = OutboundDispatcher::send(&self.config, outbound_email).await?;

        // Save Outbound Agent Message
        let outbound_msg_id = Uuid::new_v4();
        let outbound_message = Message {
            id: outbound_msg_id,
            thread_id: thread.id,
            message_id: sent_result.outbound_message_id.clone(),
            in_reply_to: Some(sent_result.in_reply_to),
            references_list: sent_result.references,
            sender: sent_result.from_address,
            recipients_to: sent_result.recipients_to,
            recipients_cc: sent_result.recipients_cc,
            subject: sent_result.subject,
            clean_text_body: sent_result.body_text,
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let _ = self.thread_persistence.create_message(&outbound_message).await?;

        Ok(ProcessEmailResult {
            processed: true,
            reason: None,
            thread_id: Some(thread.id),
            inbound_message_id: Some(saved_inbound.message_id),
            outbound_message_id: Some(sent_result.outbound_message_id),
        })
    }

    pub async fn get_thread_history(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
        self.thread_persistence.list_messages_by_thread_id(thread_id).await
    }
}

fn inbound_msg_id_message(msg: &Message) -> Message {
    msg.clone()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProcessEmailResult {
    pub processed: bool,
    pub reason: Option<String>,
    pub thread_id: Option<Uuid>,
    pub inbound_message_id: Option<String>,
    pub outbound_message_id: Option<String>,
}
