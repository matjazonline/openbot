//! Thread use cases: everything that happens to a message between arriving at the platform and
//! the agent's reply going back out.
//!
//! This module owns the types and the [`ThreadUseCases`] handle; the two pipelines live next to
//! it in [`ingest`] and [`dispatch`], with their shared helpers in [`support`].

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::services::agent_channel_tool::AgentChannelProvisioning;
use crate::{
    app_error::{AppError, AppResult},
    domain::monitoring::MonitoringService,
    entities::{
        channel::Channel,
        company::Company,
        correlation::CorrelationId,
        cursor::{MessageCursor, ThreadCursor},
        email_message::EmailMessageMetadata,
        message::{CanonicalMessageId, Message, MessageRole},
        message_view::{
            AgentHistoryMessage, EmailReplyContext, MessageAuditView, ThreadMessageView,
        },
        task::{ThreadActivity, TokenUsage},
        thread::Thread,
        transport::{
            ChannelSelector, ExternalMessageKey, ExternalThreadKey, IdentityNamespace,
            IdentitySubject, QualifiedIdentity, TransportKind,
        },
        value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId, ThreadIndex},
    },
    infra::config::AppConfig,
    services::memory_coordinator::MemoryCoordinator,
    task_queue::TaskPersistence,
    transport::{
        AddressedIdentity, AddressedRecipient, AddressedTarget, BoundedVec, CanonicalContent,
        ComposedDelivery, DeliveryComposer, DeliveryCreation, DeliveryRequest,
        ExternalCorrelationStore, InboundDraft, InboundEnvelope, InboundMessageCommitter,
        InboundRouting, IngressDirectives, IngressPolicyFacts, InternalMailRelay,
        InternalRelayMail, MessageDisposition, NewDelivery, ProtocolExtension, RelayDisposition,
        StandaloneDeliveryEnqueuer, StandaloneDeliveryRequest, ports::TransportRenderers,
    },
    use_cases::{
        agent::AgentPersistence, approval::ApprovalUseCases, channel::ChannelPersistence,
        company::CompanyPersistence, integration::ChannelBindingPersistence,
        participant::ParticipantPersistence,
    },
};

mod dispatch;
pub use dispatch::{AgentReply, DispatchOutcome};
mod ingest;
pub use ingest::{
    InboundMessage, InboundPreflight, IngestRejection, IngressOrigin, PreparedInbound, UnusableHint,
};
mod message_write;
mod reload;
pub use message_write::{
    AgentAuthor, MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite,
};
mod support;

/// Addressing role and multi-channel pipeline position are transport vocabulary; they are declared
/// with the other transport contracts and re-exported here so call sites read unchanged.
pub use crate::transport::{PipelineStep, RecipientRole, ReplyDelivery};

/// One channel's agent answering another channel of the same company.
///
/// Registered with the email sender, which reaches it *before* SMTP: a message addressed to one of
/// this deployment's own channels never leaves the building, so there is no DMARC verdict to earn
/// and no mail to parse back. Rendering it to RFC 5322 and re-ingesting the result was how a
/// mailbox became the internal identity of a channel.
#[async_trait]
impl InternalMailRelay for ThreadUseCases {
    async fn relay_internal(&self, mail: &InternalRelayMail<'_>) -> AppResult<RelayDisposition> {
        // A recipient outside this deployment, or one whose channel this source may not address,
        // is not a fault: it is the ordinary case, and the sender posts it over SMTP.
        let Some((selector, company_id)) = self.authorize_relay(mail).await? else {
            return Ok(RelayDisposition::NotInternal);
        };

        let ingest = self
            .ingest_relayed_message(mail, &selector, company_id)
            .await?;
        if ingest.accepted {
            return Ok(RelayDisposition::Relayed);
        }
        // A refusal by one of our own channels -- a spent hop budget, a disabled channel, an ACL
        // -- will read the same way on every retry, so it is reported as definite rather than left
        // to burn five attempts reaching the same verdict.
        Ok(RelayDisposition::Refused(
            ingest
                .reason()
                .unwrap_or("The receiving channel refused the message")
                .to_string(),
        ))
    }
}

#[cfg(test)]
pub mod test_support;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "inter_channel_tests.rs"]
mod inter_channel_tests;

#[cfg(test)]
#[path = "external_reply_tests.rs"]
mod external_reply_tests;

pub const MAX_THREAD_MESSAGES_PER_HOUR: usize = 60;

/// Apply the product footer shared by agent replies before they become canonical content.
pub fn agent_response_body(response: &str) -> String {
    format!("{response}\n\nDone by busybots.net")
}

/// One channel an already-queued agent run drives.
///
/// Written by the commit that created the task and read back when the run starts, so the worker
/// walks the channels the ingest actually authorized rather than a list re-derived from a payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskChannelTarget {
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub recipient_role: RecipientRole,
}

pub fn qualified_email_identity(address: impl Into<String>) -> AppResult<QualifiedIdentity> {
    let subject = IdentitySubject::parse(address.into().trim().to_ascii_lowercase())
        .map_err(|error| AppError::Internal(format!("Invalid email identity: {error}")))?;
    let namespace =
        IdentityNamespace::parse("email").expect("the fixed email identity namespace is valid");
    Ok(QualifiedIdentity::new(
        TransportKind::Email,
        namespace,
        subject,
    ))
}

/// One RFC Message-ID as an opaque provider message key, for the internal relay that mints them.
fn external_message_key(id: &MessageId) -> AppResult<ExternalMessageKey> {
    ExternalMessageKey::parse(id.as_str().trim())
        .map_err(|error| AppError::Internal(format!("Unusable relayed message key: {error}")))
}

fn external_thread_key(id: &MessageId) -> AppResult<ExternalThreadKey> {
    ExternalThreadKey::parse(id.as_str().trim())
        .map_err(|error| AppError::Internal(format!("Unusable relayed thread key: {error}")))
}

/// The ports one accepted inbound message is committed through.
///
/// Grouped rather than appended to a constructor that already takes five stores: three more
/// positional `Arc`s of the same shape is exactly the argument-swap `src/AGENTS.md` names. They
/// travel together because they are one job -- decide against the correlation store, commit through
/// the committer, and address the bindings the binding store resolved.
#[derive(Clone)]
pub struct InboundIngestPorts {
    pub committer: Arc<dyn InboundMessageCommitter>,
    pub correlation: Arc<dyn ExternalCorrelationStore>,
    pub bindings: Arc<dyn ChannelBindingPersistence>,
    pub standalone_deliveries: Arc<dyn StandaloneDeliveryEnqueuer>,
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

    async fn find_thread_by_thread_index(
        &self,
        channel_id: Uuid,
        thread_index: &ThreadIndex,
    ) -> AppResult<Option<Thread>>;

    async fn count_recent_messages(&self, thread_id: Uuid, duration_secs: i64) -> AppResult<usize>;

    /// One canonical message as it appears in one thread.
    ///
    /// The reader a queued run starts from: it holds only ids, and this is what turns the one it
    /// answers back into a message. `None` means the association is gone, which is a task failure
    /// rather than an empty history.
    async fn get_thread_message(
        &self,
        thread_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<Message>>;

    /// The protocol extension stored beside a canonical message.
    ///
    /// Kept off [`Message`]: provider headers are reload input for the owning transport, not part
    /// of the canonical payload read by every message consumer.
    async fn get_message_protocol_extension(
        &self,
        company_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<ProtocolExtension>;

    /// Store one canonical message and associate it with its thread.
    ///
    /// Idempotent per provider key: a redelivery of a message already stored returns what is
    /// stored. A redelivery whose content changed is refused rather than allowed to rewrite a
    /// message agents have already read -- see `ExternalMessageCollision`.
    async fn create_message(&self, write: &MessageWrite) -> AppResult<Message>;

    /// Store one canonical message together with every delivery it is owed.
    ///
    /// One transaction, because a message visible in a thread but never queued for delivery -- or
    /// queued but invisible -- is worse than neither. This is the shape for producers with no task
    /// row to fence on: a schedule's answer, a stop notice, a direct ingest's reply. Work driven
    /// by a task commits through `TaskPersistence::commit_agent_dispatch` instead, which fences
    /// the same pair on the run's lease.
    ///
    /// Not defaulted: a double that stored the message and quietly dropped the deliveries would
    /// let a test assert the reply exists while nothing would ever send it.
    async fn create_message_with_deliveries(
        &self,
        write: &MessageWrite,
        deliveries: &[NewDelivery],
    ) -> AppResult<(Message, Vec<DeliveryCreation>)>;

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

    /// The newest turns of a thread, oldest first, as a page renders them.
    ///
    /// Bounded by `THREAD_HISTORY_LIMIT` at the query: a thread is appended to by everyone who can
    /// reach the channel, so an unbounded read is a page whose size a correspondent chooses.
    async fn list_thread_message_views(&self, thread_id: Uuid)
    -> AppResult<Vec<ThreadMessageView>>;

    /// The turns newer than `after`, oldest first — what a live reader has not seen yet. `None`
    /// means from the start of the thread.
    async fn list_thread_message_views_after(
        &self,
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<ThreadMessageView>>;

    /// One message as a page renders it, reached through the thread it is being read from.
    async fn get_thread_message_view(
        &self,
        thread_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<ThreadMessageView>>;

    /// The thread so far, as an agent prompt reads it: role, author name, topic, words.
    async fn list_agent_history(&self, thread_id: Uuid) -> AppResult<Vec<AgentHistoryMessage>>;

    /// What the mail renderer needs to answer this thread's newest turn.
    ///
    /// `Some` with empty header fields is the honest answer for a thread whose newest turn came
    /// over a transport that has none; a caller that needs a `Message-ID` must say what it does
    /// instead rather than be handed a fabricated one.
    async fn latest_email_reply_context(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<EmailReplyContext>>;

    /// The newest RFC Message-ID in a thread, looking back past turns with no email headers.
    async fn latest_thread_rfc_message_id(&self, thread_id: Uuid) -> AppResult<Option<MessageId>>;

    /// One message's operational detail, including the provider keys that reach it.
    ///
    /// Company-scoped in the query rather than by the caller's belief: this is the read that would
    /// otherwise let a guessed association id return another tenant's correlation trail.
    async fn get_message_audit(
        &self,
        company_id: Uuid,
        association_id: Uuid,
    ) -> AppResult<Option<MessageAuditView>>;
}

#[derive(Clone)]
pub struct ThreadUseCases {
    thread_persistence: Arc<dyn ThreadPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    company_persistence: Arc<dyn CompanyPersistence>,
    participant_persistence: Arc<dyn ParticipantPersistence>,
    task_persistence: Arc<dyn TaskPersistence>,
    committer: Arc<dyn InboundMessageCommitter>,
    correlation_store: Arc<dyn ExternalCorrelationStore>,
    binding_persistence: Arc<dyn ChannelBindingPersistence>,
    standalone_deliveries: Arc<dyn StandaloneDeliveryEnqueuer>,
    /// Resolves an interface and freezes what will be sent, so a reply is queued in the same
    /// transaction that records it. Required rather than optional: a deployment that forgot to
    /// wire it would answer every customer in the thread and mail nobody.
    deliveries: DeliveryComposer,
    agent_persistence: Option<Arc<dyn AgentPersistence>>,
    agent_channel_provisioning: Option<Arc<dyn AgentChannelProvisioning>>,
    approval_use_cases: Option<Arc<ApprovalUseCases>>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    memory: Option<Arc<MemoryCoordinator>>,
    config: Arc<AppConfig>,
    agent_run_timeout: std::time::Duration,
}

/// The stores one [`ThreadUseCases`] reads and writes.
///
/// A named struct rather than five positional `Arc<dyn ...>` parameters: they are all the same
/// shape at the call site, so a transposed pair compiles and then reads companies out of the
/// channel store. `src/adapters/persistence/AGENTS.md` names this exact fix.
#[derive(Clone)]
pub struct ThreadStores {
    pub threads: Arc<dyn ThreadPersistence>,
    pub channels: Arc<dyn ChannelPersistence>,
    pub companies: Arc<dyn CompanyPersistence>,
    pub participants: Arc<dyn ParticipantPersistence>,
    pub tasks: Arc<dyn TaskPersistence>,
}

impl ThreadUseCases {
    pub fn new(
        stores: ThreadStores,
        ingest: InboundIngestPorts,
        renderers: Arc<TransportRenderers>,
        config: Arc<AppConfig>,
    ) -> Self {
        let deliveries = DeliveryComposer::new(renderers, ingest.bindings.clone());

        Self {
            thread_persistence: stores.threads,
            channel_persistence: stores.channels,
            company_persistence: stores.companies,
            participant_persistence: stores.participants,
            task_persistence: stores.tasks,
            committer: ingest.committer,
            correlation_store: ingest.correlation,
            standalone_deliveries: ingest.standalone_deliveries,
            binding_persistence: ingest.bindings,
            deliveries,
            agent_persistence: None,
            agent_channel_provisioning: None,
            approval_use_cases: None,
            monitoring: None,
            memory: None,
            config,
            agent_run_timeout: std::time::Duration::from_secs(300),
        }
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

    pub fn with_memory(mut self, memory: Arc<MemoryCoordinator>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn channel_persistence(&self) -> &Arc<dyn ChannelPersistence> {
        &self.channel_persistence
    }

    pub fn company_persistence(&self) -> &Arc<dyn CompanyPersistence> {
        &self.company_persistence
    }

    pub fn participant_persistence(&self) -> &Arc<dyn ParticipantPersistence> {
        &self.participant_persistence
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

    /// Ingest a message one channel's agent addressed to another channel of the same company.
    ///
    /// The relay is a *transport*, so it produces exactly what any other transport produces: an
    /// [`InboundDraft`] and an [`InboundRouting`], built here from resolved values. Nothing is
    /// rendered to RFC 5322 and re-parsed -- the previous shape serialized an outbound email purely
    /// so the ingress path would have something email-shaped to read back.
    ///
    /// Authentication is the relay's own identity, not a header and not a fabricated DMARC pass:
    /// [`IngressOrigin::InternalChannel`] can only be constructed by this path, and the guard
    /// checks the stated source channel against it.
    async fn ingest_relayed_message(
        &self,
        mail: &InternalRelayMail<'_>,
        selector: &ChannelSelector,
        company_id: Uuid,
    ) -> AppResult<InboundIngestResult> {
        let message = self.internal_inbound_message(mail, selector, company_id)?;
        self.ingest(message).await
    }

    /// The canonical inbound message one internal hop produces.
    ///
    /// Every provider key is derived from the relay's own RFC ids, so the receiving channel
    /// deduplicates a repeated hop exactly as it would a repeated delivery from outside.
    fn internal_inbound_message(
        &self,
        mail: &InternalRelayMail<'_>,
        selector: &ChannelSelector,
        company_id: Uuid,
    ) -> AppResult<InboundMessage> {
        let metadata = EmailMessageMetadata::new(mail.message_id.clone())
            .in_reply_to(mail.in_reply_to.cloned())
            .references(mail.references.to_vec())
            .raw_bodies(Some(mail.body_text.to_string()), None);
        let author = qualified_email_identity(mail.from.clone())?;
        let recipient_handle = qualified_email_identity(mail.recipient_to.clone())?;

        let draft = InboundDraft {
            // No durable inbound event: the relay hands the message over in-process, and this
            // ingest is the only claim on it.
            event_key: None,
            message_key: external_message_key(&metadata.rfc_message_id)?,
            thread_key: external_thread_key(metadata.conversation_root_key())?,
            reply_message_keys: BoundedVec::parse(
                "reply message candidates",
                metadata
                    .reference_candidates()
                    .iter()
                    .map(external_message_key)
                    .collect::<AppResult<Vec<_>>>()?,
            )?,
            reply_thread_keys: BoundedVec::parse(
                "reply thread candidates",
                vec![external_thread_key(metadata.conversation_root_key())?],
            )?,
            author,
            addressed: BoundedVec::parse(
                "addressed identities",
                vec![AddressedIdentity::new(
                    RecipientRole::To,
                    recipient_handle.clone(),
                )],
            )?,
            content: CanonicalContent::parse(mail.subject, mail.body_text)?,
            attachments: BoundedVec::empty(),
            directives: IngressDirectives {
                hop_count: mail.hop_count,
                trace_channels: BoundedVec::parse("trace channels", mail.trace.clone())?,
                disposition: MessageDisposition::Answer,
                source_channel_id: Some(mail.source_channel_id),
                target_thread_id: None,
                // An agent answering another agent is machine-generated by construction; the guard
                // exempts the internal path precisely because this is expected here.
                is_auto_reply: true,
                is_forwarded: false,
            },
            // The relay is trusted because of who ran it, not because of a verdict it could
            // fabricate. There is no `AuthVerdict` on this path at all.
            policy: IngressPolicyFacts::TrustedApplication,
            // Carried, never re-minted: agent A's run and agent B's run are one chain.
            correlation_id: mail.correlation_id,
            extension: ProtocolExtension::email(metadata),
        };

        let routing = InboundRouting::parse(vec![AddressedRecipient {
            role: RecipientRole::To,
            handle: recipient_handle,
            target: AddressedTarget::Channels(vec![selector.clone()]),
            disposition: MessageDisposition::Answer,
        }])?;

        Ok(InboundMessage::arriving(
            draft,
            routing,
            IngressOrigin::InternalChannel {
                company_id,
                channel_id: mail.source_channel_id,
            },
        ))
    }

    /// Everything an internal hop has to be true for before it is ingested on the target channel.
    ///
    /// The sender address is checked against the channel the mail *claims* to come from, so a
    /// relayed message cannot borrow another channel's identity: the ingress guard trusts
    /// [`IngressOrigin::InternalChannel`] absolutely, and this is what earns that trust.
    async fn authorize_relay(
        &self,
        mail: &InternalRelayMail<'_>,
    ) -> AppResult<Option<(ChannelSelector, Uuid)>> {
        let Some((source, _target, company)) = self
            .resolve_internal_destination(mail.source_channel_id, mail.target)
            .await?
        else {
            return Ok(None);
        };
        let expected_sender =
            Channel::address_for(&source.slug, &company.slug, &self.config.app_domain_name);
        if !mail.from.eq_ignore_ascii_case(&expected_sender) {
            return Err(AppError::Internal(
                "Internal sender address does not match its source channel".into(),
            ));
        }
        Ok(Some((mail.target.clone(), company.id)))
    }

    /// Tell a sender their message could not be routed.
    ///
    /// The HTTP request task that calls this method is supervised; this method makes the work
    /// durable before that task returns. The generic delivery worker owns every provider attempt,
    /// including retry and ambiguous-outcome handling.
    pub async fn handle_bounce_dispatch(&self, ingest: &InboundIngestResult) -> AppResult<()> {
        let Some(bounce) = ingest.bounce_info() else {
            return Ok(());
        };
        let body = format_bounce_email_body(bounce, &self.config.app_domain_name);
        let subject = if bounce
            .original_subject
            .to_ascii_lowercase()
            .starts_with("[undeliverable]")
        {
            bounce.original_subject.clone()
        } else {
            format!("[Undeliverable] {}", bounce.original_subject)
        };
        let content = CanonicalContent::parse(subject, body)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let serialized = serde_json::to_vec(bounce).map_err(|error| {
            AppError::Internal(format!("Could not key a rejection bounce: {error}"))
        })?;
        let source_key = format!("bounce:{:x}", Sha256::digest(serialized));
        let delivery = self
            .deliveries
            .compose_standalone(StandaloneDeliveryRequest {
                correlation_id: CorrelationId::new(),
                purpose: crate::entities::transport::DeliveryPurpose::Notification,
                source_key,
                content: &content,
                context: crate::transport::DeliveryContext::Email(
                    crate::transport::EmailDeliveryContext {
                        from: EmailAddress::from(format!(
                            "mailer-daemon@{}",
                            self.config.app_domain_name
                        )),
                        from_name: Some("Mail Agents Server".to_string()),
                        recipient_to: bounce.recipient_to.clone(),
                        recipients_cc: Vec::new(),
                        threading: crate::transport::EmailThreading::Standalone,
                        relay: None,
                    },
                ),
            })?;
        self.standalone_deliveries
            .enqueue_standalone_delivery(delivery)
            .await?;
        Ok(())
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

    /// The newest turns of a thread, oldest first, as a page renders them.
    pub async fn get_thread_history(&self, thread_id: Uuid) -> AppResult<Vec<ThreadMessageView>> {
        self.thread_persistence
            .list_thread_message_views(thread_id)
            .await
    }

    /// The thread so far, as an agent prompt reads it.
    pub async fn get_agent_history(&self, thread_id: Uuid) -> AppResult<Vec<AgentHistoryMessage>> {
        self.thread_persistence.list_agent_history(thread_id).await
    }

    /// What the mail renderer needs to answer this thread's newest turn.
    pub async fn latest_email_reply_context(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<EmailReplyContext>> {
        self.thread_persistence
            .latest_email_reply_context(thread_id)
            .await
    }

    /// The newest RFC Message-ID in a thread, looking back past turns with no email headers.
    pub async fn latest_thread_rfc_message_id(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<MessageId>> {
        self.thread_persistence
            .latest_thread_rfc_message_id(thread_id)
            .await
    }

    /// One message's operational detail, for an authorized diagnostic pane.
    pub async fn get_message_audit(
        &self,
        company_id: Uuid,
        association_id: Uuid,
    ) -> AppResult<Option<MessageAuditView>> {
        self.thread_persistence
            .get_message_audit(company_id, association_id)
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
    ) -> AppResult<Vec<ThreadMessageView>> {
        self.thread_persistence
            .list_thread_message_views_after(thread_id, after, limit)
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

    /// Store one message together with every delivery it is owed, in one transaction.
    ///
    /// The shape for a producer with no task lease to fence on. A caller that wrote the two
    /// separately would leave a thread showing an answer nobody will be sent, or a delivery
    /// pointing at a message that is not there.
    pub async fn save_message_with_deliveries(
        &self,
        message: &MessageWrite,
        deliveries: &[NewDelivery],
    ) -> AppResult<Message> {
        let (stored, created) = self
            .thread_persistence
            .create_message_with_deliveries(message, deliveries)
            .await?;
        for delivery in created {
            if !delivery.was_created() {
                tracing::info!(
                    delivery_id = %delivery.delivery_id(),
                    "An equivalent delivery was already queued; not queueing it again"
                );
            }
        }
        Ok(stored)
    }

    /// Resolve an interface and freeze what a delivery will be sent as, without writing anything.
    ///
    /// The caller commits the result inside whichever transaction creates the state the delivery
    /// answers for.
    pub async fn compose_delivery(
        &self,
        request: DeliveryRequest<'_>,
    ) -> AppResult<ComposedDelivery> {
        self.deliveries.compose(request).await
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BounceSuggestion {
    pub invalid_slug: ChannelSlug,
    pub suggestions: Vec<ChannelSlug>,
}

/// One channel the bouncing sender could have written to instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelDirectoryEntry {
    pub address: EmailAddress,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BounceInfo {
    /// The provider message this notification answers. Included in the delivery key so a webhook
    /// redelivery is absorbed without merging two distinct rejected messages with the same
    /// subject.
    pub source_message_key: ExternalMessageKey,
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
        crate::entities::channel::RESERVED_SLUG_SUFFIXES.join(", ")
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

/// What one inbound message became.
///
/// Deliberately **not** serializable. The pre-canonical shape was written straight into
/// `background_tasks.payload`, which made every queued row a snapshot of the domain model: a field
/// rename broke rows already in flight, stale configuration was replayed hours later, and raw
/// provider content sat inside the task protocol. A durable task now carries
/// [`InboundTaskPayloadV1`](crate::transport::InboundTaskPayloadV1) -- identifiers only -- and this
/// is the in-process handoff to whatever runs the agent next.
#[derive(Debug, Clone)]
pub struct InboundIngestResult {
    pub accepted: bool,
    /// Why nothing was stored. `None` on the accepted path.
    pub rejection: Option<IngestRejection>,
    pub thread: Option<Thread>,
    pub inbound_message: Option<Message>,
    pub company: Option<Company>,
    pub channel: Option<Channel>,
    /// What arrived, in canonical form.
    ///
    /// Shared rather than cloned: the dispatch pipeline passes it through several frames, and the
    /// envelope holds the whole message body.
    pub envelope: Option<Arc<InboundEnvelope>>,
    pub task_id: Option<Uuid>,
    /// Whether the agent's reply should actually leave the building. Real inbound mail always gets
    /// a real answer; a mailbox send can ask to stay in-app.
    pub reply_delivery: ReplyDelivery,
    pub channel_matches: Vec<ChannelMatch>,
}

impl InboundIngestResult {
    /// The chain this ingest belongs to.
    ///
    /// `None` only for a rejection, which never became a canonical message and so has no chain to
    /// be on. Anything that dispatches an agent has one, and must carry it rather than mint a
    /// replacement.
    pub fn correlation_id(&self) -> Option<CorrelationId> {
        self.envelope
            .as_ref()
            .map(|envelope| envelope.correlation_id)
    }

    /// The sentence a synchronous transport reports, when this ingest refused the message.
    pub fn reason(&self) -> Option<&'static str> {
        self.rejection.as_ref().map(IngestRejection::as_str)
    }

    /// The bounce this rejection owes the sender, if any.
    pub fn bounce_info(&self) -> Option<&BounceInfo> {
        self.rejection.as_ref().and_then(IngestRejection::bounce)
    }

    /// Whether the agent runs for this message, or whether it was only filed on its threads.
    pub fn answers(&self) -> bool {
        self.envelope
            .as_ref()
            .is_some_and(|envelope| envelope.directives.disposition.answers())
    }

    /// The durable payload this ingest's task carries, for the audit record a run appends to it.
    ///
    /// Rebuilt rather than kept: the run holds resolved entities, and the payload holds the ids
    /// they were loaded from, so one shape is derived from the other rather than the two being
    /// maintained in parallel.
    pub fn durable_task_payload(&self) -> serde_json::Value {
        let Some((envelope, primary)) = self.envelope.as_deref().zip(self.channel_matches.first())
        else {
            return serde_json::Value::Null;
        };
        crate::transport::InboundTaskPayload::v1(crate::transport::InboundTaskPayloadV1 {
            company_id: primary.company.id,
            channel_id: primary.channel.id,
            thread_id: primary.thread.id,
            source_message_id: primary.inbound_message.canonical_id,
            correlation_id: envelope.correlation_id,
            hop_count: envelope.directives.hop_count,
            trace_channels: envelope.directives.trace_channels.clone(),
            is_forwarded: envelope.directives.is_forwarded,
            reply_delivery: self.reply_delivery,
        })
        .encode()
        .unwrap_or(serde_json::Value::Null)
    }

    pub fn rejected(rejection: IngestRejection) -> Self {
        Self {
            accepted: false,
            rejection: Some(rejection),
            thread: None,
            inbound_message: None,
            company: None,
            channel: None,
            envelope: None,
            task_id: None,
            reply_delivery: ReplyDelivery::Send,
            channel_matches: Vec::new(),
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
    /// The canonical reply this run produced, named by its own id rather than by the RFC header of
    /// whichever mail happened to carry it.
    pub reply_message_id: Option<CanonicalMessageId>,
    pub agent_response: String,
    pub email_sent: bool,
    pub token_usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
