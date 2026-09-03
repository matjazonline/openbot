//! Thread use cases: everything that happens to a message between arriving at the platform and
//! the agent's reply going back out.
//!
//! This module owns the types and the [`ThreadUseCases`] handle; the two pipelines live next to
//! it in [`ingest`] and [`dispatch`], with their shared helpers in [`support`].

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crate::services::agent_channel_tool::AgentChannelProvisioning;
use crate::{
    adapters::persistence::task::TaskPersistence,
    adapters::protocols::email::{
        EmailChannelSelectorParser, EmailIdentity, EmailIngressAdapter, EmailRecipientDestination,
    },
    adapters::storage::FileStorage,
    app_error::{AppError, AppResult},
    domain::monitoring::MonitoringService,
    entities::{
        channel::Channel,
        company::Company,
        correlation::CorrelationId,
        cursor::{MessageCursor, ThreadCursor},
        email_message::EmailMessageMetadata,
        message::{
            CanonicalMessageId, Message, MessageDirection, MessageParticipantKind, MessageRole,
        },
        message_contract::NormalizedInboundMessage,
        participant::{IdentityClaimMetadata, IdentityProvenance, PrincipalAccessContext},
        task::{ThreadActivity, TokenUsage},
        thread::Thread,
        transport::{ChannelSelector, QualifiedIdentity, TransportKind},
        value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId, ThreadIndex},
    },
    infra::config::AppConfig,
    services::{
        email_parser::{ParsedEmail, RawInboundPayload},
        memory_coordinator::MemoryCoordinator,
        outbound_dispatcher::{MailTransport, OutboundDispatcher, OutboundEmail, SentEmailResult},
    },
    use_cases::{
        agent::AgentPersistence,
        approval::ApprovalUseCases,
        channel::ChannelPersistence,
        company::CompanyPersistence,
        participant::{IdentityObservation, ParticipantPersistence, observe_email_access_context},
    },
};

mod dispatch;
pub use dispatch::DispatchOutcome;
mod ingest;
pub use ingest::{ReplyDelivery, SYSTEM_ADDRESS_ANSWERED};
mod message_write;
pub use message_write::{
    MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite,
};
mod support;

/// Addressing role and multi-channel pipeline position are transport vocabulary; they are declared
/// with the other transport contracts and re-exported here so call sites read unchanged.
pub use crate::transport::{PipelineStep, RecipientRole};

#[cfg(test)]
pub mod test_support;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "inter_channel_tests.rs"]
mod inter_channel_tests;

pub const MAX_THREAD_MESSAGES_PER_HOUR: usize = 60;

pub fn qualified_email_identity(address: impl Into<String>) -> AppResult<QualifiedIdentity> {
    EmailIdentity::parse(EmailAddress::from(address.into()))
        .map(EmailIdentity::qualify_default)
        .map_err(|error| AppError::Internal(format!("Invalid email identity: {error}")))
}

#[derive(Clone, Copy)]
struct InternalChannelSource {
    company_id: Uuid,
    channel_id: Uuid,
}

/// Where an inbound-shaped message entered the application.
///
/// Mailbox and simulation messages are authorized by the signed-in HTTP route, so they have no
/// meaningful DMARC result. Keeping that fact separate from [`AuthVerdict::Pass`] prevents a
/// synthetic application message from masquerading as externally authenticated email.
#[derive(Clone, Copy)]
enum InboundOrigin {
    ExternalEmail,
    AuthenticatedApplication,
    InternalChannel,
}

#[async_trait]
pub trait ThreadPersistence: Send + Sync {
    async fn create_thread(
        &self,
        channel_id: Uuid,
        subject: &str,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread>;

    /// Creates the thread for a durable schedule run and records that identity atomically.
    /// Test doubles may use the ordinary creation path; PostgreSQL provides the crash-safe form.
    async fn ensure_schedule_run_thread(
        &self,
        _run_id: Uuid,
        channel_id: Uuid,
        subject: &str,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread> {
        self.create_thread(channel_id, subject, participant_emails)
            .await
    }

    async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>>;

    async fn list_threads_by_channel_id(
        &self,
        channel_id: Uuid,
        before: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>>;

    /// Who sent the newest message in each of these threads.
    ///
    /// Only the live column asks for this, to tell an agent's reply apart from a person's message
    /// when marking a thread the reader does not have open. A default of "nothing known" is a safe
    /// degradation -- the mark is simply not shown -- so test doubles need not implement it.
    async fn list_thread_last_roles(
        &self,
        _thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, MessageRole>> {
        Ok(HashMap::new())
    }

    /// The channel's threads touched since `after`, oldest first — what a live column has not
    /// reflected yet. `None` means from the start of the channel.
    async fn list_threads_updated_after(
        &self,
        channel_id: Uuid,
        after: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>>;

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread>;

    async fn find_thread_by_message_ids(
        &self,
        channel_id: Uuid,
        message_ids: &[MessageId],
    ) -> AppResult<Option<Thread>>;

    async fn find_thread_by_thread_index(
        &self,
        channel_id: Uuid,
        thread_index: &ThreadIndex,
    ) -> AppResult<Option<Thread>>;

    async fn count_recent_messages(&self, thread_id: Uuid, duration_secs: i64) -> AppResult<usize>;

    /// Store one canonical message and associate it with its thread.
    ///
    /// Idempotent per provider key: a redelivery of a message already stored returns what is
    /// stored. A redelivery whose content changed is refused rather than allowed to rewrite a
    /// message agents have already read -- see `ExternalMessageCollision`.
    async fn create_message(&self, write: &MessageWrite) -> AppResult<Message>;

    /// The newest outbound message this thread gained *after* the message named here, if any.
    ///
    /// This is the idempotency guard for "has the agent already answered?", and it is deliberately
    /// positional rather than header-based: a scheduled run and a Slack reply have no `In-Reply-To`
    /// to match on, and one dispatch produces one reply per thread. Outbound messages that an
    /// outreach sent are excluded -- those are the agent asking a third party something, not the
    /// answer to this turn.
    async fn find_outbound_reply_after(
        &self,
        thread_id: Uuid,
        answering: CanonicalMessageId,
    ) -> AppResult<Option<Message>>;

    /// Associate a canonical message that already exists with another thread of its own company.
    ///
    /// One message, several conversations: an email addressed to three channels is stored once and
    /// associated three times. The composite foreign key refuses a thread in another company, so a
    /// caller cannot leak a message across tenants by naming the wrong thread.
    async fn associate_message(
        &self,
        thread_id: Uuid,
        message: CanonicalMessageId,
    ) -> AppResult<Message>;

    async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>>;

    /// The thread's messages newer than `after`, oldest first — what a live reader has not seen
    /// yet. `None` means from the start of the thread.
    async fn list_messages_after(
        &self,
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<Message>>;
}

#[derive(Clone)]
pub struct ThreadUseCases {
    thread_persistence: Arc<dyn ThreadPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    company_persistence: Arc<dyn CompanyPersistence>,
    participant_persistence: Arc<dyn ParticipantPersistence>,
    task_persistence: Arc<dyn TaskPersistence>,
    agent_persistence: Option<Arc<dyn AgentPersistence>>,
    agent_channel_provisioning: Option<Arc<dyn AgentChannelProvisioning>>,
    approval_use_cases: Option<Arc<ApprovalUseCases>>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    mail_dispatcher: Arc<OutboundDispatcher>,
    /// Where inbound attachments are kept; `None` on a deployment with no private bucket.
    file_storage: Option<Arc<dyn FileStorage>>,
    memory: Option<Arc<MemoryCoordinator>>,
    config: Arc<AppConfig>,
    agent_run_timeout: std::time::Duration,
}

impl ThreadUseCases {
    pub fn new(
        thread_persistence: Arc<dyn ThreadPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        company_persistence: Arc<dyn CompanyPersistence>,
        participant_persistence: Arc<dyn ParticipantPersistence>,
        task_persistence: Arc<dyn TaskPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        let mail_dispatcher = Arc::new(OutboundDispatcher::disabled(config.clone()));

        Self {
            thread_persistence,
            channel_persistence,
            company_persistence,
            participant_persistence,
            task_persistence,
            agent_persistence: None,
            agent_channel_provisioning: None,
            approval_use_cases: None,
            monitoring: None,
            mail_dispatcher,
            file_storage: None,
            memory: None,
            config,
            agent_run_timeout: std::time::Duration::from_secs(300),
        }
    }

    pub(crate) async fn observe_email_access_context(
        &self,
        company_id: Uuid,
        address: &str,
    ) -> AppResult<PrincipalAccessContext> {
        observe_email_access_context(self.participant_persistence.as_ref(), company_id, address)
            .await
    }

    pub(crate) async fn preferred_email_for_principal(
        &self,
        company_id: Uuid,
        principal_id: crate::entities::transport::PrincipalId,
    ) -> AppResult<Option<EmailAddress>> {
        let identities = self
            .participant_persistence
            .identities_for_principals(company_id, &[principal_id], TransportKind::Email)
            .await?;
        Ok(identities
            .into_iter()
            .next()
            .map(|identity| EmailAddress::from(identity.subject.into_string())))
    }

    pub fn with_agent_run_timeout(mut self, timeout: std::time::Duration) -> Self {
        assert!(!timeout.is_zero(), "agent run timeout must be positive");
        self.agent_run_timeout = timeout;
        self
    }

    pub fn with_mail_transport(mut self, transport: Arc<dyn MailTransport>) -> Self {
        self.mail_dispatcher = Arc::new(OutboundDispatcher::new(self.config.clone(), transport));
        self
    }

    pub fn mail_dispatcher(&self) -> &Arc<OutboundDispatcher> {
        &self.mail_dispatcher
    }

    pub fn with_agent_persistence(mut self, agent_persistence: Arc<dyn AgentPersistence>) -> Self {
        self.agent_persistence = Some(agent_persistence);
        self
    }

    pub fn with_agent_channel_provisioning(
        mut self,
        persistence: Arc<dyn AgentChannelProvisioning>,
    ) -> Self {
        self.agent_channel_provisioning = Some(persistence);
        self
    }

    pub fn with_approval_use_cases(mut self, approval_use_cases: Arc<ApprovalUseCases>) -> Self {
        self.approval_use_cases = Some(approval_use_cases);
        self
    }

    pub fn get_approval_use_cases(&self) -> Option<Arc<ApprovalUseCases>> {
        self.approval_use_cases.clone()
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    /// Where inbound attachments are kept. Without this, mail still arrives; its attachments are
    /// recorded but not stored.
    pub fn with_file_storage(mut self, file_storage: Option<Arc<dyn FileStorage>>) -> Self {
        self.file_storage = file_storage;
        self
    }

    pub fn with_memory(mut self, memory: Arc<MemoryCoordinator>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// The storage inbound attachments go to, for the adapters that parse on this use case's behalf.
    pub fn file_storage(&self) -> Option<&dyn FileStorage> {
        self.file_storage.as_deref()
    }

    pub fn channel_persistence(&self) -> &Arc<dyn ChannelPersistence> {
        &self.channel_persistence
    }

    pub fn company_persistence(&self) -> &Arc<dyn CompanyPersistence> {
        &self.company_persistence
    }

    pub fn thread_persistence(&self) -> &Arc<dyn ThreadPersistence> {
        &self.thread_persistence
    }

    pub fn task_persistence(&self) -> &Arc<dyn TaskPersistence> {
        &self.task_persistence
    }

    pub fn agent_persistence(&self) -> Option<&Arc<dyn AgentPersistence>> {
        self.agent_persistence.as_ref()
    }

    pub fn config(&self) -> &Arc<AppConfig> {
        &self.config
    }

    pub fn memory_coordinator(&self) -> Option<&Arc<MemoryCoordinator>> {
        self.memory.as_ref()
    }

    fn internal_channel_selector(&self, recipient: &str) -> AppResult<Option<ChannelSelector>> {
        let destination = EmailChannelSelectorParser::new(&self.config.app_domain_name)
            .classify(EmailAddress::from(recipient.trim().to_ascii_lowercase()));
        match destination {
            EmailRecipientDestination::External(_) => Ok(None),
            EmailRecipientDestination::InvalidPlatformAddress => Err(AppError::Internal(format!(
                "Invalid platform channel address: {recipient}"
            ))),
            EmailRecipientDestination::Channel(selection)
                if selection.delivery().is_context_only() || selection.selectors().len() != 1 =>
            {
                Err(AppError::Internal(
                    "Internal channel delivery requires one direct channel address".into(),
                ))
            }
            EmailRecipientDestination::Channel(selection) => {
                Ok(selection.into_selectors().into_iter().next())
            }
        }
    }

    async fn resolve_internal_destination(
        &self,
        source_channel_id: Uuid,
        selector: &ChannelSelector,
    ) -> AppResult<Option<(Channel, Channel, Company)>> {
        let source = self
            .channel_persistence
            .get_by_id(source_channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Internal source channel was not found".into()))?;
        let target = match selector {
            ChannelSelector::CurrentCompany(channel_slug) => self
                .channel_persistence
                .list_by_company_id(source.company_id)
                .await?
                .into_iter()
                .find(|channel| channel.matches_slug(channel_slug)),
            ChannelSelector::Qualified { company, channel } => {
                self.channel_persistence
                    .get_by_company_slug_and_channel_slug(company, channel)
                    .await?
            }
        }
        .ok_or_else(|| {
            AppError::Internal(format!("Platform channel does not exist: {selector}"))
        })?;
        if source.company_id != target.company_id {
            return Err(AppError::Internal(
                "Cross-company internal channel delivery is not allowed".into(),
            ));
        }
        if source.id == target.id {
            return Err(AppError::Internal(
                "A channel cannot deliver an internal message to itself".into(),
            ));
        }
        if !target.enabled {
            return Err(AppError::Internal(format!(
                "Target channel '{}' is disabled",
                target.slug
            )));
        }
        if target.agent_ids.as_ref().is_none_or(Vec::is_empty) {
            return Err(AppError::Internal(format!(
                "Target channel '{}' has no configured agent",
                target.slug
            )));
        }
        let company = self
            .company_persistence
            .get_by_id(source.company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Internal source company was not found".into()))?;
        if selector
            .company()
            .is_some_and(|selected| !company.slug.eq_ignore_ascii_case(selected))
        {
            return Err(AppError::Internal(
                "Internal recipient company does not match the source company".into(),
            ));
        }
        Ok(Some((source, target, company)))
    }

    pub async fn prepare_internal_channel_delivery(
        &self,
        email: OutboundEmail,
        idempotency_key: Option<&str>,
    ) -> AppResult<Option<SentEmailResult>> {
        let Some(selector) = self.internal_channel_selector(&email.recipient_to)? else {
            return Ok(None);
        };
        self.resolve_internal_destination(email.channel_id, &selector)
            .await?;
        let prepared = match idempotency_key {
            Some(key) => self.mail_dispatcher.prepare_idempotent(email, key)?,
            None => self.mail_dispatcher.prepare(email)?,
        };
        Ok(Some(prepared))
    }

    pub async fn ingest_prepared_internal_message(
        &self,
        sent: &SentEmailResult,
    ) -> AppResult<InboundIngestResult> {
        let source_channel_id = sent.source_channel_id.ok_or_else(|| {
            AppError::Internal("Internal delivery has no source channel identity".into())
        })?;
        let recipient = sent
            .recipients_to
            .first()
            .ok_or_else(|| AppError::Internal("Internal delivery has no recipient".into()))?;
        let selector = self
            .internal_channel_selector(recipient)?
            .ok_or_else(|| AppError::Internal("Recipient is not an internal channel".into()))?;
        let (source, _, company) = self
            .resolve_internal_destination(source_channel_id, &selector)
            .await?
            .ok_or_else(|| AppError::Internal("Recipient is not an internal channel".into()))?;
        let expected_sender = format!(
            "{}@{}.{}",
            source.slug, company.slug, self.config.app_domain_name
        );
        if !sent.from_address.eq_ignore_ascii_case(&expected_sender) {
            return Err(AppError::Internal(
                "Internal sender address does not match its source channel".into(),
            ));
        }

        let norm = NormalizedInboundMessage {
            message_id: sent.outbound_message_id.clone(),
            thread_ref: Some(sent.in_reply_to.clone()),
            references: sent.references.clone(),
            thread_index: None,
            sender: qualified_email_identity(sent.from_address.clone())?,
            recipients_to: sent
                .recipients_to
                .iter()
                .cloned()
                .map(qualified_email_identity)
                .collect::<AppResult<Vec<_>>>()?,
            recipients_cc: sent
                .recipients_cc
                .iter()
                .cloned()
                .map(qualified_email_identity)
                .collect::<AppResult<Vec<_>>>()?,
            subject: sent.subject.clone(),
            clean_text: sent.body_text.clone(),
            raw_text: Some(sent.body_text.clone()),
            raw_html: None,
            attachments: Vec::new(),
            is_auto_reply: true,
            is_forwarded: false,
            channel_id_header: Some(source_channel_id),
            hop_count: sent.hop_count,
            trace_channels: sent.trace_channels.clone(),
            // Internal delivery never touches the wire, so the header the SMTP path would have
            // carried is passed straight across instead. Agent A's run and agent B's run are one
            // chain, which is the case the correlation id exists for.
            correlation_id: sent.correlation_id,
            transport: TransportKind::Email,
            spf_status: Default::default(),
            dkim_status: Default::default(),
            dmarc_status: Default::default(),
            spam_score: None,
            is_context_only: false,
        };
        self.ingest_normalized_message_with_source(
            norm,
            Some(InternalChannelSource {
                company_id: company.id,
                channel_id: source_channel_id,
            }),
            InboundOrigin::InternalChannel,
            ingest::ReplyDelivery::Send,
        )
        .await
    }

    pub async fn record_outreach_outbound_message(
        &self,
        outbox_id: Uuid,
        sent: &crate::services::outbound_dispatcher::SentEmailResult,
    ) -> AppResult<()> {
        let Some(thread_id) = self
            .task_persistence
            .get_outreach_thread_for_outbox(outbox_id)
            .await?
        else {
            return Ok(());
        };
        let sender = qualified_email_identity(sent.from_address.clone())?;
        let mut participants = vec![MessageParticipantWrite::new(
            MessageParticipantKind::Sender,
            sender.clone(),
        )];
        for (kind, addresses) in [
            (MessageParticipantKind::To, &sent.recipients_to),
            (MessageParticipantKind::Cc, &sent.recipients_cc),
        ] {
            for address in addresses.iter() {
                participants.push(MessageParticipantWrite::new(
                    kind,
                    qualified_email_identity(address.clone())?,
                ));
            }
        }
        // The email extension must match what `ingest_prepared_internal_message` stores for the
        // very same mail: both sides key one canonical message on the same `Message-ID`, and the
        // stored message is only reused when the content hashes agree. Leaving `raw_text_body`
        // `None` here while the receiving side wrote `Some(body)` made every internal delegation
        // hop fail its outbox delivery and retry forever.
        let metadata = EmailMessageMetadata::new(sent.outbound_message_id.clone())
            .in_reply_to(Some(sent.in_reply_to.clone()))
            .references(sent.references.to_vec())
            .raw_bodies(Some(sent.body_text.clone()), None);

        self.thread_persistence
            .create_message(&MessageWrite {
                thread_id,
                author: MessageAuthorWrite::Observed(IdentityObservation {
                    identity: sender,
                    display_label: sent.from_name.clone(),
                    claim_metadata: IdentityClaimMetadata::observation(),
                    provenance: IdentityProvenance::Agent,
                }),
                subject: sent.subject.clone(),
                clean_text_body: sent.body_text.clone(),
                attachments: Vec::new(),
                direction: MessageDirection::Outbound,
                role: MessageRole::Agent,
                correlation_id: sent.correlation_id,
                participants,
                correlation: MessageCorrelation::Email(metadata),
                created_at: chrono::Utc::now(),
            })
            .await?;
        Ok(())
    }

    /// Take in a message from an authenticated application route and queue its agent run.
    ///
    /// Returns as soon as the message is committed. The worker picks the task up on its next poll
    /// and the reply reaches the open thread over the message stream — the same route every piece
    /// of real inbound mail already takes.
    ///
    /// This skips DMARC because there is no email transport at this boundary. Callers must derive
    /// the sender from the authenticated account and authorize the requested company/channel (and
    /// thread, for replies) before calling. Ingest still enforces channel participant access so a
    /// fabricated sender or recipient is not accepted merely because this method was selected.
    pub(crate) async fn queue_authenticated_inbound_for_agent(
        &self,
        raw_payload: RawInboundPayload,
        delivery: ReplyDelivery,
    ) -> AppResult<InboundIngestResult> {
        self.ingest_composed_message(raw_payload, delivery).await
    }

    /// Tell a sender their message could not be routed.
    ///
    /// Fire and forget, deliberately: a relay that is down must not turn an undeliverable message
    /// into retried work. This is a notification delivery in the target model, and becomes one in
    /// step 9 when the generic outbox exists to carry it.
    pub async fn handle_bounce_dispatch(&self, ingest: &InboundIngestResult) {
        let Some(bounce) = ingest.bounce_info.as_ref() else {
            return;
        };
        let body = format_bounce_email_body(bounce, &self.config.app_domain_name);
        if let Err(error) = self
            .mail_dispatcher
            .send_bounce(&bounce.recipient_to, &bounce.original_subject, &body)
            .await
        {
            tracing::warn!(
                "Could not deliver a bounce to '{}': {error}",
                bounce.recipient_to
            );
        }
    }

    pub async fn get_thread(&self, thread_id: Uuid) -> AppResult<Option<Thread>> {
        self.thread_persistence.get_thread_by_id(thread_id).await
    }

    pub async fn list_channel_threads(
        &self,
        channel_id: Uuid,
        before: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        self.thread_persistence
            .list_threads_by_channel_id(channel_id, before, limit)
            .await
    }

    /// Who sent the newest message in each of these threads, for the live column's reply mark.
    pub async fn thread_last_roles(
        &self,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, MessageRole>> {
        self.thread_persistence
            .list_thread_last_roles(thread_ids)
            .await
    }

    /// The threads a live column is missing, oldest first.
    ///
    /// Cursor-driven for the same reason the message stream is: one call covers a single bumped
    /// thread, a burst that overflowed the broadcast channel, and a reconnect after an outage.
    pub async fn get_channel_threads_after(
        &self,
        channel_id: Uuid,
        after: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        self.thread_persistence
            .list_threads_updated_after(channel_id, after, limit)
            .await
    }

    pub async fn get_thread_history(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
        self.thread_persistence
            .list_messages_by_thread_id(thread_id)
            .await
    }

    /// The messages a live reader is missing, oldest first.
    ///
    /// Driving the message stream off a cursor rather than off the notification payload means one
    /// call covers three cases with the same code: a single new message, a burst that overflowed
    /// the broadcast channel, and a reconnect after the connection dropped.
    pub async fn get_thread_messages_after(
        &self,
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<Message>> {
        self.thread_persistence
            .list_messages_after(thread_id, after, limit)
            .await
    }

    pub async fn find_outbound_reply_after(
        &self,
        thread_id: Uuid,
        answering: CanonicalMessageId,
    ) -> AppResult<Option<Message>> {
        self.thread_persistence
            .find_outbound_reply_after(thread_id, answering)
            .await
    }

    pub async fn save_message(&self, message: &MessageWrite) -> AppResult<Message> {
        self.thread_persistence.create_message(message).await
    }

    /// Attach a message that is already stored to another of its company's threads.
    pub async fn associate_message(
        &self,
        thread_id: Uuid,
        message: CanonicalMessageId,
    ) -> AppResult<Message> {
        self.thread_persistence
            .associate_message(thread_id, message)
            .await
    }

    pub async fn hydrate_ingest_configuration(
        &self,
        ingest: &mut InboundIngestResult,
    ) -> AppResult<()> {
        if let Some(company) = ingest.company.as_mut() {
            *company = self
                .company_persistence
                .get_by_id(company.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal("Task company no longer exists".into())
                })?;
        }
        if let Some(channel) = ingest.channel.as_mut() {
            let current = self
                .channel_persistence
                .get_by_id(channel.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal("Task channel no longer exists".into())
                })?;
            if ingest
                .company
                .as_ref()
                .is_some_and(|company| company.id != current.company_id)
            {
                return Err(crate::app_error::AppError::Internal(
                    "Task channel does not belong to its company".into(),
                ));
            }
            *channel = current;
        }
        for channel_match in &mut ingest.channel_matches {
            let company = self
                .company_persistence
                .get_by_id(channel_match.company.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal(
                        "Task target company no longer exists".into(),
                    )
                })?;
            let channel = self
                .channel_persistence
                .get_by_id(channel_match.channel.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal(
                        "Task target channel no longer exists".into(),
                    )
                })?;
            if channel.company_id != company.id {
                return Err(crate::app_error::AppError::Internal(
                    "Task target channel does not belong to its company".into(),
                ));
            }
            let thread = self
                .thread_persistence
                .get_thread_by_id(channel_match.thread.id)
                .await?
                .ok_or_else(|| {
                    crate::app_error::AppError::Internal(
                        "Task target thread no longer exists".into(),
                    )
                })?;
            if thread.channel_id != channel.id {
                return Err(crate::app_error::AppError::Internal(
                    "Task target thread does not belong to its channel".into(),
                ));
            }
            channel_match.company = company;
            channel_match.channel = channel;
            channel_match.thread = thread;
        }
        Ok(())
    }

    pub async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<crate::entities::task::TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
        self.task_persistence
            .list_company_tasks(company_id, channel_id, status, sort_asc)
            .await
    }

    pub async fn list_company_tasks_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<crate::entities::task::TaskStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
        self.task_persistence
            .list_company_tasks_page(company_id, channel_id, status, sort_asc, offset, limit)
            .await
    }

    pub async fn get_task_persistence(&self) -> Arc<dyn TaskPersistence> {
        self.task_persistence.clone()
    }

    /// What each of these threads is currently doing, for the mailbox's activity indicators.
    ///
    /// Threads with nothing in flight are absent from the map rather than present-and-idle, so a
    /// caller renders "no badge" by finding nothing.
    pub async fn thread_activity(
        &self,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadActivity>> {
        self.task_persistence.list_thread_activity(thread_ids).await
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMatch {
    pub company: Company,
    pub channel: Channel,
    /// The address this message actually arrived on — an alias, or the canonical slug.
    ///
    /// Optional because durable task payloads written before aliases existed carry no value, and
    /// `ChannelSlug::default()` is the empty string, which would silently send from
    /// `@company.domain`.
    #[serde(default)]
    pub matched_slug: Option<ChannelSlug>,
    pub thread: Thread,
    pub inbound_message: Message,
    pub recipient_role: RecipientRole,
    /// Where this match sits in a multi-channel pipeline.
    pub step: PipelineStep,
}

impl ChannelMatch {
    /// The slug a reply goes out from: the alias the sender wrote to, falling back to the
    /// channel's canonical slug for matches recorded before aliases existed.
    pub fn reply_slug(&self) -> ChannelSlug {
        self.matched_slug
            .clone()
            .unwrap_or_else(|| self.channel.slug.clone())
    }
}

fn durable_ingest_payload(ingest: &InboundIngestResult) -> serde_json::Value {
    let durable = ingest.clone();
    serde_json::to_value(durable).unwrap_or_default()
}

fn scrub_json_secrets(value: Option<&mut serde_json::Value>) {
    let Some(value) = value else {
        return;
    };
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let normalized = key.to_ascii_lowercase();
                if matches!(
                    normalized.as_str(),
                    "api_key"
                        | "apikey"
                        | "access_token"
                        | "token"
                        | "secret"
                        | "secret_key"
                        | "private_key"
                        | "app_key"
                        | "app_secret"
                        | "auth"
                        | "authorization"
                        | "password"
                        | "bearer"
                ) {
                    *value = serde_json::Value::Null;
                } else {
                    scrub_json_secrets(Some(value));
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                scrub_json_secrets(Some(value));
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BounceSuggestion {
    pub invalid_slug: ChannelSlug,
    pub suggestions: Vec<ChannelSlug>,
}

/// One channel the bouncing sender could have written to instead.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelDirectoryEntry {
    pub address: EmailAddress,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BounceInfo {
    pub recipient_to: EmailAddress,
    pub company_slug: Option<CompanySlug>,
    pub invalid_slugs: Vec<ChannelSlug>,
    /// Channels that exist but are switched off. Defaulted on deserialize so bounces queued
    /// before this field existed still re-hydrate.
    #[serde(default)]
    pub disabled_slugs: Vec<ChannelSlug>,
    pub suggestions: Vec<BounceSuggestion>,
    /// The channels this sender may write to, listed only when they are on the company's team.
    ///
    /// Empty for everyone else, so a stranger who guesses at an address never learns the company's
    /// channel directory from the bounce it earns. Defaulted on deserialize for the same reason as
    /// `disabled_slugs`.
    #[serde(default)]
    pub available_channels: Vec<ChannelDirectoryEntry>,
    pub original_subject: String,
}

pub fn format_bounce_email_body(bounce: &BounceInfo, app_domain: &str) -> String {
    let domain_part = bounce
        .company_slug
        .as_deref()
        .map(|c| format!("{c}.{app_domain}"))
        .unwrap_or_else(|| app_domain.to_string());

    let mut body = String::new();
    body.push_str("Undeliverable Mail Notification\n\n");
    body.push_str(&format!(
        "Your email could not be delivered because {}.\n\n",
        bounce_cause(bounce)
    ));

    if let Some(ref comp) = bounce.company_slug {
        body.push_str(&format!("Company Domain: {comp}.{app_domain}\n\n"));
    }

    body.push_str("Details:\n");
    for s in &bounce.suggestions {
        body.push_str(&format!(
            "  - Invalid Channel Slug: '{}@{}'\n",
            s.invalid_slug, domain_part
        ));
        if !s.suggestions.is_empty() {
            body.push_str("    Did you mean:\n");
            for sug in &s.suggestions {
                body.push_str(&format!("      * {sug}@{domain_part}\n"));
            }
        } else {
            body.push_str("    No similar channel suggestions found.\n");
        }
        body.push('\n');
    }

    for slug in &bounce.disabled_slugs {
        body.push_str(&format!(
            "  - Disabled Channel: '{slug}@{domain_part}'\n    This address is correct, but the \
             channel is currently switched off and is not accepting mail.\n\n"
        ));
    }

    push_channel_directory(&mut body, &bounce.available_channels);

    body.push_str("Please check the channel address and try sending your email again.\n");
    body
}

/// The "here is what you could have written to" section, appended only for a sender the platform
/// recognizes as one of the company's own people.
///
/// Nothing at all is rendered for an empty list, so a stranger's bounce is byte-for-byte what it
/// was before this section existed.
fn push_channel_directory(body: &mut String, entries: &[ChannelDirectoryEntry]) {
    if entries.is_empty() {
        return;
    }

    body.push_str("Channels you can write to:\n");
    for entry in entries {
        body.push_str(&format!("  - {} \u{2014} {}\n", entry.address, entry.name));
        if let Some(description) = entry.description.as_deref().map(str::trim)
            && !description.is_empty()
        {
            body.push_str(&format!("      {description}\n"));
        }
    }
    body.push('\n');
}

/// The reply `_help@{company}.{app_domain}` sends back.
///
/// `entries` is whatever [`ThreadUseCases::writable_channel_directory`] allowed this sender, so the
/// team-only rule is decided there rather than restated here; an empty list still gets the syntax
/// section, which discloses nothing about the company.
///
/// Every example below is checked against the real parsers: the `+` split in
/// `parse_recipient_address_pipeline`, the separators in `strip_context_suffix_from_slug`, and the
/// bracket forms in `EmailParser::check_body_context_trigger`.
pub fn format_help_email_body(
    entries: &[ChannelDirectoryEntry],
    company_slug: &CompanySlug,
    app_domain: &str,
) -> String {
    let domain_part = format!("{company_slug}.{app_domain}");
    let example = entries
        .first()
        .map(|entry| {
            entry
                .address
                .split('@')
                .next()
                .unwrap_or("support")
                .to_string()
        })
        .unwrap_or_else(|| "support".to_string());

    let mut body = String::from("Mail Agents Help\n\n");
    push_channel_directory(&mut body, entries);
    if entries.is_empty() {
        body.push_str(
            "You are not currently a participant of any channel in this company, so there is \
             nothing to list. Ask a colleague to add you.\n\n",
        );
    }

    body.push_str("Addressing tricks:\n");
    body.push_str(&format!(
        "  {example}+billing@{domain_part}\n      Send to both channels, in that order.\n"
    ));
    body.push_str(&format!(
        "  {example}+quiet@{domain_part}\n      File it on the thread without running the agent.\n"
    ));
    body.push_str(
        "  [[quiet]]\n      As the first thing in the body, does the same as +quiet.\n\n",
    );
    body.push_str(&format!(
        "  {} all mean \"file it, don't answer\", and attach to a\n  channel name with '+', '.', \
         '-' or '_'.\n\n",
        crate::services::email_parser::RESERVED_CONTEXT_SUFFIXES.join(", ")
    ));

    body.push_str("Reply to this address at any time to see this again.\n");
    body
}

/// The one sentence that says why a bounce happened, for both bounce renderers.
pub fn bounce_cause(bounce: &BounceInfo) -> &'static str {
    match (
        !bounce.invalid_slugs.is_empty(),
        !bounce.disabled_slugs.is_empty(),
    ) {
        (true, true) => {
            "one or more channel addresses were not found or misspelled, and one or more are disabled"
        }
        (true, false) => "one or more channel addresses were not found or misspelled",
        (false, true) => "one or more channels are currently disabled",
        (false, false) => "the message could not be routed",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundIngestResult {
    pub accepted: bool,
    pub reason: Option<String>,
    pub thread: Option<Thread>,
    pub inbound_message: Option<Message>,
    pub company: Option<Company>,
    pub channel: Option<Channel>,
    pub parsed_email: Option<ParsedEmail>,
    pub normalized_message: Option<NormalizedInboundMessage>,
    pub task_id: Option<Uuid>,
    /// Whether the agent's reply should actually leave the building. Real inbound mail always gets
    /// a real answer; a mailbox send can ask to stay in-app.
    ///
    /// Required rather than defaulted: the worker that eventually answers is a different process
    /// from the one that took the message in, and a payload it cannot read this from is one it
    /// must refuse rather than guess at.
    pub reply_delivery: ReplyDelivery,
    #[serde(default)]
    pub channel_matches: Vec<ChannelMatch>,
    #[serde(default)]
    pub bounce_info: Option<BounceInfo>,
}

impl InboundIngestResult {
    /// The chain this ingest belongs to.
    ///
    /// `None` only for a rejection, which never got as far as a normalized message and so has no
    /// chain to be on. Anything that dispatches an agent has one, and must carry it rather than
    /// mint a replacement.
    pub fn correlation_id(&self) -> Option<CorrelationId> {
        self.normalized_message
            .as_ref()
            .map(|norm| norm.correlation_id)
    }

    pub fn rejected(reason: &str) -> Self {
        Self {
            accepted: false,
            reason: Some(reason.to_string()),
            thread: None,
            inbound_message: None,
            company: None,
            channel: None,
            parsed_email: None,
            normalized_message: None,
            task_id: None,
            reply_delivery: ReplyDelivery::Send,
            channel_matches: Vec::new(),
            bounce_info: None,
        }
    }

    pub fn rejected_with_bounce(reason: &str, bounce: BounceInfo) -> Self {
        Self {
            accepted: false,
            reason: Some(reason.to_string()),
            thread: None,
            inbound_message: None,
            company: None,
            channel: None,
            parsed_email: None,
            normalized_message: None,
            task_id: None,
            reply_delivery: ReplyDelivery::Send,
            channel_matches: Vec::new(),
            bounce_info: Some(bounce),
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
    pub token_usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
