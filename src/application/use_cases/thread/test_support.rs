//! One in-memory canonical message store, shared by every test that needs threads and messages.
//!
//! It exists because five near-identical hand-written doubles used to drift apart, and because the
//! canonical store has behaviour a naive `Vec<Message>` cannot stand in for: a payload is stored
//! once and associated with many threads, a redelivered provider key resolves to the message
//! already stored, and a *changed* redelivery is refused. A double that got any of those wrong
//! would let the ingest and dispatch tests pass over bugs the real store rejects.
//!
//! Principal ids are derived from the handle rather than allocated, matching
//! [`crate::use_cases::participant::test_support`], so a fixture can name an actor before anything
//! has observed it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    adapters::persistence::task::TaskPersistence,
    app_error::{AppError, AppResult},
    entities::participant::{IdentityClaimMetadata, IdentityProvenance},
    entities::{
        correlation::CorrelationId,
        creation::CreationProvenance,
        cursor::{MessageCursor, ThreadCursor},
        email_message::EmailMessageMetadata,
        message::{
            AttachmentMetadata, CanonicalMessageId, Message, MessageAuthor, MessageDirection,
            MessageParticipant, MessageParticipantKind, MessageRole,
        },
        message_view::{
            AgentHistoryMessage, AuthorView, EmailReplyContext, MessageAuditView,
            THREAD_HISTORY_LIMIT, ThreadMessageView,
        },
        task::{NewTask, TaskSource, TaskTarget},
        thread::{Thread, ThreadParticipantProjection},
        transport::{
            BindingAccessPolicy, BindingAccessSnapshot, BindingAuditEvent, BindingDeliveryPolicy,
            BindingStatus, ChannelBinding, ChannelBindingId, EndpointNamespace,
            ExternalEndpointKey, ExternalMessageKey, ExternalThreadKey, ParticipantIdentityId,
            PrincipalId, QualifiedIdentity, TransportKind,
        },
        value_objects::{EmailAddress, MessageId, ThreadIndex},
    },
    transport::{
        CommitDisposition, DeliveryCreation, ExternalCorrelationStore, InboundCommitOutcome,
        InboundCommitRequest, InboundEnvelope, InboundMessageCommitter, InboundTaskPayload,
        InboundTaskPayloadV1, NewDelivery, ThreadTarget,
    },
    use_cases::{
        integration::{
            BindingStatusChange, BindingWrite, ChannelBindingPersistence, InboundEndpoint,
        },
        participant::IdentityObservation,
        participant::test_support::{principal_for_email, principal_for_identity},
        thread::{
            InboundIngestPorts, MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite,
            MessageWrite, ThreadPersistence,
        },
    },
};

/// One stored canonical payload.
#[derive(Clone)]
struct Canonical {
    id: CanonicalMessageId,
    author: MessageAuthor,
    subject: String,
    clean_text_body: String,
    attachments: Option<Vec<AttachmentMetadata>>,
    direction: MessageDirection,
    role: MessageRole,
    correlation_id: CorrelationId,
    participants: Vec<MessageParticipant>,
    email: Option<EmailMessageMetadata>,
}

impl Canonical {
    /// What must be identical for a repeated provider key to count as a redelivery.
    ///
    /// The same choice the database makes in `canonical_message_hash`: everything the sender
    /// actually sent, and deliberately *not* `clean_text_body`, which is derived per thread.
    fn delivered_payload(
        &self,
    ) -> (
        &str,
        &MessageDirection,
        &MessageRole,
        &Option<EmailMessageMetadata>,
    ) {
        (&self.subject, &self.direction, &self.role, &self.email)
    }
}

/// One thread's membership of a canonical payload.
#[derive(Clone, Copy)]
struct Association {
    id: Uuid,
    thread_id: Uuid,
    message_id: CanonicalMessageId,
    created_at: DateTime<Utc>,
}

/// Provider keys are unique per binding; this double has one channel-shaped binding per channel,
/// so the key is scoped by channel.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ProviderKey {
    channel_id: Uuid,
    key: String,
}

#[derive(Default)]
struct Store {
    threads: Vec<Thread>,
    canonical: Vec<Canonical>,
    associations: Vec<Association>,
    external_messages: HashMap<ProviderKey, CanonicalMessageId>,
    external_threads: HashMap<ProviderKey, Uuid>,
    /// Association ids whose outbound message an outreach sent, so the idempotency guard can skip
    /// them the way the real query does.
    outreach_sent: Vec<CanonicalMessageId>,
    /// Deliveries queued alongside a message, so a test can assert that an answer was not only
    /// recorded but actually handed to a transport.
    deliveries: Vec<NewDelivery>,
}

/// An in-memory [`ThreadPersistence`].
#[derive(Clone, Default)]
pub struct InMemoryThreads {
    store: Arc<Mutex<Store>>,
    company_id: Uuid,
}

impl InMemoryThreads {
    /// Every delivery queued through [`ThreadPersistence::create_message_with_deliveries`].
    pub fn queued_deliveries(&self) -> Vec<NewDelivery> {
        self.store.lock().unwrap().deliveries.clone()
    }
}

impl InMemoryThreads {
    pub fn new() -> Self {
        Self::default()
    }

    /// Threads and messages this double attributes to one company, so derived principal ids match
    /// what a fixture's identity directory produces for the same company.
    pub fn for_company(company_id: Uuid) -> Self {
        Self {
            store: Arc::new(Mutex::new(Store::default())),
            company_id,
        }
    }

    /// Seed a thread without going through creation, for a fixture that starts mid-conversation.
    pub fn insert_thread(&self, thread: Thread) {
        self.store.lock().unwrap().threads.push(thread);
    }

    pub fn threads(&self) -> Vec<Thread> {
        self.store.lock().unwrap().threads.clone()
    }

    /// Every stored message, oldest association first.
    pub fn messages(&self) -> Vec<Message> {
        let store = self.store.lock().unwrap();
        let mut messages: Vec<Message> = store
            .associations
            .iter()
            .filter_map(|association| self.read(&store, association))
            .collect();
        messages.sort_by_key(Message::cursor);
        messages
    }

    /// Mark a stored outbound message as one an outreach sent, so the reply guard ignores it.
    pub fn mark_sent_by_outreach(&self, message: CanonicalMessageId) {
        self.store.lock().unwrap().outreach_sent.push(message);
    }

    /// The stable ids policy reads, derived from the addresses a fixture states -- the mirror of
    /// what `insert_thread_email_principals` writes in production.
    fn principals_for(&self, participant_emails: &[EmailAddress]) -> Vec<PrincipalId> {
        participant_emails
            .iter()
            .map(|email| principal_for_email(self.company_id, email))
            .collect()
    }

    fn read(&self, store: &Store, association: &Association) -> Option<Message> {
        let canonical = store
            .canonical
            .iter()
            .find(|canonical| canonical.id == association.message_id)?;
        Some(Message {
            id: association.id,
            canonical_id: canonical.id,
            company_id: self.company_id,
            thread_id: association.thread_id,
            author: canonical.author.clone(),
            subject: canonical.subject.clone(),
            clean_text_body: canonical.clean_text_body.clone(),
            attachments: canonical.attachments.clone(),
            direction: canonical.direction,
            role: canonical.role,
            correlation_id: canonical.correlation_id,
            participants: canonical.participants.clone(),
            email: canonical.email.clone(),
            created_at: association.created_at,
        })
    }

    /// One thread's stored messages, oldest first.
    fn thread_messages(&self, thread_id: Uuid) -> Vec<Message> {
        let store = self.store.lock().unwrap();
        let mut messages: Vec<Message> = store
            .associations
            .iter()
            .filter(|association| association.thread_id == thread_id)
            .filter_map(|association| self.read(&store, association))
            .collect();
        messages.sort_by_key(Message::cursor);
        messages
    }

    fn thread_channel(store: &Store, thread_id: Uuid) -> AppResult<Uuid> {
        store
            .threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .map(|thread| thread.channel_id)
            .ok_or_else(|| AppError::NotFound(format!("Thread {thread_id} was not found")))
    }

    fn resolve_author(&self, author: &MessageAuthorWrite) -> MessageAuthor {
        match author {
            MessageAuthorWrite::Observed(observation) => MessageAuthor {
                principal_id: principal_for_identity(self.company_id, &observation.identity),
                identity_id: Some(identity_id_for(&observation.identity)),
                label: observation
                    .display_label
                    .clone()
                    .unwrap_or_else(|| observation.identity.subject().as_str().to_string()),
                identity: Some(observation.identity.clone()),
            },
            MessageAuthorWrite::Principal(principal_id) => MessageAuthor {
                principal_id: *principal_id,
                identity_id: None,
                label: principal_id.to_string(),
                identity: None,
            },
            // Mirrors the writer's upsert: the agent's principal is stable per agent, so two
            // answers from one agent read as one author.
            MessageAuthorWrite::Agent(agent) => MessageAuthor {
                principal_id: PrincipalId::new(agent.agent_id),
                identity_id: None,
                label: agent.display_label.clone(),
                identity: None,
            },
            MessageAuthorWrite::Platform => MessageAuthor {
                principal_id: PrincipalId::new(self.company_id),
                identity_id: None,
                label: "System".to_string(),
                identity: None,
            },
        }
    }

    /// Positions assigned per kind from the order given, duplicates dropped -- the same rule the
    /// SQL writer applies, and what makes a rendered header reproducible.
    pub(crate) fn resolve_participants(
        participants: &[MessageParticipantWrite],
    ) -> Vec<MessageParticipant> {
        let mut resolved: Vec<MessageParticipant> = Vec::with_capacity(participants.len());
        for participant in participants {
            if resolved
                .iter()
                .any(|seen| seen.kind == participant.kind && seen.identity == participant.identity)
            {
                continue;
            }
            let position = resolved
                .iter()
                .filter(|seen| seen.kind == participant.kind)
                .count() as u16;
            resolved.push(MessageParticipant {
                kind: participant.kind,
                position,
                identity_id: identity_id_for(&participant.identity),
                identity: participant.identity.clone(),
            });
        }
        resolved
    }
}

/// A stable identity id for a handle, derived so two sightings of one address agree.
fn identity_id_for(identity: &QualifiedIdentity) -> ParticipantIdentityId {
    let mut digest = Sha256::new();
    digest.update(b"identity:");
    digest.update(identity.transport().as_str().as_bytes());
    digest.update(b":");
    digest.update(identity.namespace().as_str().as_bytes());
    digest.update(b":");
    digest.update(identity.subject().as_str().as_bytes());
    let bytes: [u8; 16] = digest.finalize()[..16]
        .try_into()
        .expect("sha256 yields at least 16 bytes");
    ParticipantIdentityId::new(Uuid::from_bytes(bytes))
}

#[async_trait]
impl ThreadPersistence for InMemoryThreads {
    async fn create_thread(
        &self,
        channel_id: Uuid,
        subject: &str,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread> {
        let thread = Thread {
            id: Uuid::new_v4(),
            channel_id,
            subject: subject.to_string(),
            participant_principal_ids: self.principals_for(participant_emails),
            participant_projection: ThreadParticipantProjection {
                email_addresses: participant_emails.to_vec(),
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.store.lock().unwrap().threads.push(thread.clone());
        Ok(thread)
    }

    async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>> {
        Ok(self
            .store
            .lock()
            .unwrap()
            .threads
            .iter()
            .find(|thread| thread.id == id)
            .cloned())
    }

    async fn list_threads_by_channel_id(
        &self,
        channel_id: Uuid,
        before: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        let mut threads: Vec<Thread> = self
            .store
            .lock()
            .unwrap()
            .threads
            .iter()
            .filter(|thread| thread.channel_id == channel_id)
            .filter(|thread| before.is_none_or(|cursor| thread.cursor() < cursor))
            .cloned()
            .collect();
        threads.sort_by_key(|thread| std::cmp::Reverse(thread.cursor()));
        threads.truncate(limit);
        Ok(threads)
    }

    async fn list_threads_updated_after(
        &self,
        channel_id: Uuid,
        after: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        let mut threads: Vec<Thread> = self
            .store
            .lock()
            .unwrap()
            .threads
            .iter()
            .filter(|thread| thread.channel_id == channel_id)
            .filter(|thread| after.is_none_or(|cursor| thread.cursor() > cursor))
            .cloned()
            .collect();
        threads.sort_by_key(Thread::cursor);
        threads.truncate(limit);
        Ok(threads)
    }

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread> {
        let mut store = self.store.lock().unwrap();
        let thread = store
            .threads
            .iter_mut()
            .find(|thread| thread.id == id)
            .ok_or_else(|| AppError::NotFound(format!("Thread {id} was not found")))?;
        thread.participant_principal_ids = participant_emails
            .iter()
            .map(|email| principal_for_email(self.company_id, email))
            .collect();
        thread.participant_projection.email_addresses = participant_emails.to_vec();
        Ok(thread.clone())
    }

    async fn get_thread_message(
        &self,
        thread_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<Message>> {
        let store = self.store.lock().unwrap();
        Ok(store
            .associations
            .iter()
            .find(|association| {
                association.thread_id == thread_id && association.message_id == message_id
            })
            .and_then(|association| self.read(&store, association)))
    }

    async fn find_thread_by_thread_index(
        &self,
        channel_id: Uuid,
        thread_index: &ThreadIndex,
    ) -> AppResult<Option<Thread>> {
        let ancestors = thread_index.ancestor_chain().unwrap_or_default();
        let store = self.store.lock().unwrap();
        let matched = store
            .associations
            .iter()
            .filter_map(|association| {
                let canonical = store
                    .canonical
                    .iter()
                    .find(|canonical| canonical.id == association.message_id)?;
                let stored = canonical.email.as_ref()?.thread_index.as_ref()?;
                ancestors
                    .contains(stored)
                    .then_some((stored.len(), association))
            })
            .max_by_key(|(length, association)| (*length, association.created_at))
            .map(|(_, association)| association.thread_id);

        Ok(matched.and_then(|thread_id| {
            store
                .threads
                .iter()
                .find(|thread| thread.id == thread_id && thread.channel_id == channel_id)
                .cloned()
        }))
    }

    async fn count_recent_messages(
        &self,
        thread_id: Uuid,
        _duration_secs: i64,
    ) -> AppResult<usize> {
        Ok(self
            .store
            .lock()
            .unwrap()
            .associations
            .iter()
            .filter(|association| association.thread_id == thread_id)
            .count())
    }

    async fn create_message(&self, write: &MessageWrite) -> AppResult<Message> {
        let author = self.resolve_author(&write.author);
        let participants = Self::resolve_participants(&write.participants);
        let mut store = self.store.lock().unwrap();
        let channel_id = Self::thread_channel(&store, write.thread_id)?;

        let incoming = Canonical {
            // The producer's own id, exactly as `insert_message_on` uses it. Minting one here
            // instead made every delivery a producer built against `write.id` name a message this
            // double had not stored.
            id: write.id,
            author,
            subject: write.subject.clone(),
            clean_text_body: write.clean_text_body.clone(),
            attachments: (!write.attachments.is_empty()).then(|| write.attachments.clone()),
            direction: write.direction,
            role: write.role,
            correlation_id: write.correlation_id,
            participants,
            email: write.email_metadata().cloned(),
        };

        let canonical_id = match &write.correlation {
            MessageCorrelation::Email(metadata) => {
                let message_key = ProviderKey {
                    channel_id,
                    key: metadata.rfc_message_id.as_str().to_string(),
                };
                let thread_key = ProviderKey {
                    channel_id,
                    key: metadata.conversation_root_key().as_str().to_string(),
                };
                let canonical_id = match store.external_messages.get(&message_key).copied() {
                    Some(existing_id) => {
                        let existing = store
                            .canonical
                            .iter()
                            .find(|canonical| canonical.id == existing_id)
                            .expect("a mapped provider key names a stored message");
                        if existing.delivered_payload() != incoming.delivered_payload() {
                            return Err(AppError::Conflict(format!(
                                "provider message key '{}' already stores canonical message {} \
                                 with different content",
                                message_key.key, existing_id
                            )));
                        }
                        existing_id
                    }
                    None => {
                        let id = incoming.id;
                        store.canonical.push(incoming);
                        store.external_messages.insert(message_key, id);
                        id
                    }
                };
                store
                    .external_threads
                    .entry(thread_key)
                    .or_insert(write.thread_id);
                canonical_id
            }
            MessageCorrelation::Internal => {
                let id = incoming.id;
                store.canonical.push(incoming);
                id
            }
        };

        let association = associate(&mut store, write.thread_id, canonical_id, write.created_at);
        let message = self
            .read(&store, &association)
            .expect("the association was just written");
        bump_thread(&mut store, write.thread_id);
        Ok(message)
    }

    async fn create_message_with_deliveries(
        &self,
        write: &MessageWrite,
        deliveries: &[NewDelivery],
    ) -> AppResult<(Message, Vec<DeliveryCreation>)> {
        let stored = self.create_message(write).await?;
        let mut created = Vec::with_capacity(deliveries.len());
        {
            let mut store = self.store.lock().unwrap();
            for delivery in deliveries {
                if delivery.message_id != stored.canonical_id {
                    return Err(AppError::Internal(format!(
                        "Delivery '{}' names message {} but this transaction stored {}",
                        delivery.idempotency_key, delivery.message_id, stored.canonical_id
                    )));
                }
                // The real store's unique index, in one line: whoever gets there first owns the
                // key, and a second call with the same key is absorbed onto the first delivery
                // rather than queueing a second send.
                let key = (
                    delivery.destination_binding_id,
                    delivery.idempotency_key.clone(),
                );
                match store.deliveries.iter().find(|queued| {
                    (
                        queued.destination_binding_id,
                        queued.idempotency_key.clone(),
                    ) == key
                }) {
                    Some(existing) => created.push(DeliveryCreation::Absorbed(existing.id)),
                    None => {
                        created.push(DeliveryCreation::Created(delivery.id));
                        store.deliveries.push(delivery.clone());
                    }
                }
            }
        }
        Ok((stored, created))
    }

    async fn associate_message(
        &self,
        thread_id: Uuid,
        message: CanonicalMessageId,
    ) -> AppResult<Message> {
        let mut store = self.store.lock().unwrap();
        Self::thread_channel(&store, thread_id)?;
        if !store
            .canonical
            .iter()
            .any(|canonical| canonical.id == message)
        {
            return Err(AppError::NotFound(format!(
                "Message {message} was not found"
            )));
        }
        let association = associate(&mut store, thread_id, message, Utc::now());
        let read = self
            .read(&store, &association)
            .expect("the association was just written");
        bump_thread(&mut store, thread_id);
        Ok(read)
    }

    async fn find_outbound_reply_after(
        &self,
        thread_id: Uuid,
        answering: CanonicalMessageId,
    ) -> AppResult<Option<Message>> {
        let store = self.store.lock().unwrap();
        let Some(answered) = store
            .associations
            .iter()
            .find(|association| {
                association.thread_id == thread_id && association.message_id == answering
            })
            .copied()
        else {
            return Ok(None);
        };
        let mut candidates: Vec<Message> = store
            .associations
            .iter()
            .filter(|association| association.thread_id == thread_id)
            .filter(|association| {
                (association.created_at, association.id) > (answered.created_at, answered.id)
            })
            .filter(|association| !store.outreach_sent.contains(&association.message_id))
            .filter_map(|association| self.read(&store, association))
            .filter(|message| message.direction == MessageDirection::Outbound)
            .collect();
        candidates.sort_by_key(Message::cursor);
        Ok(candidates.pop())
    }

    async fn list_thread_message_views(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Vec<ThreadMessageView>> {
        let mut messages = self.thread_messages(thread_id);
        // The newest window, then oldest-first, exactly as the SQL read does it.
        if messages.len() > THREAD_HISTORY_LIMIT {
            messages.drain(..messages.len() - THREAD_HISTORY_LIMIT);
        }
        Ok(messages.iter().map(thread_message_view).collect())
    }

    async fn list_thread_message_views_after(
        &self,
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<ThreadMessageView>> {
        let mut messages = self.thread_messages(thread_id);
        messages.retain(|message| after.is_none_or(|cursor| message.cursor() > cursor));
        messages.truncate(limit.min(THREAD_HISTORY_LIMIT));
        Ok(messages.iter().map(thread_message_view).collect())
    }

    async fn get_thread_message_view(
        &self,
        thread_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<ThreadMessageView>> {
        Ok(self
            .thread_messages(thread_id)
            .iter()
            .find(|message| message.canonical_id == message_id)
            .map(thread_message_view))
    }

    async fn list_agent_history(&self, thread_id: Uuid) -> AppResult<Vec<AgentHistoryMessage>> {
        let mut messages = self.thread_messages(thread_id);
        if messages.len() > THREAD_HISTORY_LIMIT {
            messages.drain(..messages.len() - THREAD_HISTORY_LIMIT);
        }
        Ok(messages
            .iter()
            .map(|message| AgentHistoryMessage {
                role: message.role,
                author_display: message.author.display().to_string(),
                subject: message.subject.clone(),
                body: message.clean_text_body.clone(),
            })
            .collect())
    }

    async fn latest_email_reply_context(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<EmailReplyContext>> {
        Ok(self
            .thread_messages(thread_id)
            .last()
            .map(|message| EmailReplyContext {
                canonical_id: message.canonical_id,
                author_email: message.author.email_address(),
                rfc_message_id: message.rfc_message_id().cloned(),
                references: message
                    .email
                    .as_ref()
                    .map(|email| email.references.clone())
                    .unwrap_or_default(),
                cc: message.email_recipients(MessageParticipantKind::Cc),
            }))
    }

    async fn get_message_audit(
        &self,
        _company_id: Uuid,
        _association_id: Uuid,
    ) -> AppResult<Option<MessageAuditView>> {
        // Only the diagnostic pane reads this, and no in-memory test exercises it. Stated rather
        // than defaulted, so a test that starts needing it fails here instead of seeing "no such
        // message" and concluding the authorization worked.
        unimplemented!("MessageAuditView is exercised against the database, not this double")
    }
}

/// The email-shaped stored message projected the way the SQL reads project it.
fn thread_message_view(message: &Message) -> ThreadMessageView {
    ThreadMessageView {
        id: message.id,
        canonical_id: message.canonical_id,
        thread_id: message.thread_id,
        author: AuthorView {
            principal_id: message.author.principal_id,
            label: message.author.label.clone(),
            handle: message
                .author
                .identity
                .as_ref()
                .map(|identity| identity.subject().as_str().to_string()),
            transport: message
                .author
                .identity
                .as_ref()
                .map(|identity| identity.transport()),
        },
        subject: message.subject.clone(),
        body: message.clean_text_body.clone(),
        attachments: message.attachments.clone().unwrap_or_default(),
        direction: message.direction,
        role: message.role,
        created_at: message.created_at,
    }
}

/// Attach a payload to a thread, returning the association a thread already had if it has one.
fn associate(
    store: &mut Store,
    thread_id: Uuid,
    message_id: CanonicalMessageId,
    created_at: DateTime<Utc>,
) -> Association {
    if let Some(existing) = store.associations.iter().find(|association| {
        association.thread_id == thread_id && association.message_id == message_id
    }) {
        return *existing;
    }
    let association = Association {
        id: Uuid::new_v4(),
        thread_id,
        message_id,
        created_at,
    };
    store.associations.push(association);
    association
}

fn bump_thread(store: &mut Store, thread_id: Uuid) {
    if let Some(thread) = store
        .threads
        .iter_mut()
        .find(|thread| thread.id == thread_id)
    {
        thread.updated_at = Utc::now();
    }
}

/// The email-shaped description of one *stored* message, for tests that render history or scan
/// it rather than write it.
///
/// Deliberately keeps the shape the pre-canonical `Message` had: a test scenario reads best when
/// it states a sender, a Message-ID and a recipient list, and turning those into an author
/// principal, a participant projection and an email extension is exactly the mapping production
/// does. `Default` supplies everything a scenario does not care about.
#[derive(Debug, Clone)]
pub struct EmailMessageDraft {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub message_id: MessageId,
    pub in_reply_to: Option<MessageId>,
    pub references_list: Vec<MessageId>,
    pub sender: EmailAddress,
    pub recipients_to: Vec<EmailAddress>,
    pub recipients_cc: Vec<EmailAddress>,
    pub subject: String,
    pub clean_text_body: String,
    pub raw_text_body: Option<String>,
    pub raw_html_body: Option<String>,
    pub attachments: Option<Vec<AttachmentMetadata>>,
    pub direction: MessageDirection,
    pub role: MessageRole,
    pub thread_index: Option<ThreadIndex>,
    pub created_at: DateTime<Utc>,
}

impl Default for EmailMessageDraft {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            message_id: MessageId::from("<draft@example.com>"),
            in_reply_to: None,
            references_list: Vec::new(),
            sender: EmailAddress::from("sender@example.com"),
            recipients_to: Vec::new(),
            recipients_cc: Vec::new(),
            subject: "Subject".to_string(),
            clean_text_body: String::new(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            thread_index: None,
            created_at: Utc::now(),
        }
    }
}

/// The stored message a draft describes.
pub fn stored_email(draft: EmailMessageDraft) -> Message {
    let sender = super::qualified_email_identity(draft.sender.as_str())
        .expect("test fixture sender address is parseable");
    let mut participants = vec![MessageParticipantWrite::new(
        MessageParticipantKind::Sender,
        sender.clone(),
    )];
    for (kind, addresses) in [
        (MessageParticipantKind::To, &draft.recipients_to),
        (MessageParticipantKind::Cc, &draft.recipients_cc),
    ] {
        for address in addresses {
            participants.push(MessageParticipantWrite::new(
                kind,
                super::qualified_email_identity(address.as_str())
                    .expect("test fixture recipient address is parseable"),
            ));
        }
    }

    Message {
        id: draft.id,
        canonical_id: CanonicalMessageId::random(),
        company_id: Uuid::nil(),
        thread_id: draft.thread_id,
        author: MessageAuthor {
            principal_id: principal_for_identity(Uuid::nil(), &sender),
            identity_id: Some(identity_id_for(&sender)),
            label: draft.sender.as_str().to_string(),
            identity: Some(sender),
        },
        subject: draft.subject,
        clean_text_body: draft.clean_text_body,
        attachments: draft.attachments,
        direction: draft.direction,
        role: draft.role,
        correlation_id: CorrelationId::new(),
        participants: InMemoryThreads::resolve_participants(&participants),
        email: Some(
            EmailMessageMetadata::new(draft.message_id)
                .in_reply_to(draft.in_reply_to)
                .references(draft.references_list)
                .thread_index(draft.thread_index)
                .raw_bodies(draft.raw_text_body, draft.raw_html_body),
        ),
        created_at: draft.created_at,
    }
}

/// The stored message a draft describes, as a page reads it.
///
/// Projected from [`stored_email`] rather than built beside it, so a fixture and the thing it
/// stands for cannot drift.
pub fn stored_email_view(draft: EmailMessageDraft) -> ThreadMessageView {
    thread_message_view(&stored_email(draft))
}

/// The stored message a draft describes, as an agent prompt reads it.
pub fn stored_email_history(draft: EmailMessageDraft) -> AgentHistoryMessage {
    let message = stored_email(draft);
    AgentHistoryMessage {
        role: message.role,
        author_display: message.author.display().to_string(),
        subject: message.subject.clone(),
        body: message.clean_text_body,
    }
}

/// The write a draft describes: what a producer hands to `create_message`.
pub fn email_write(draft: EmailMessageDraft) -> MessageWrite {
    let sender = super::qualified_email_identity(draft.sender.as_str())
        .expect("test fixture sender address is parseable");
    let mut participants = vec![MessageParticipantWrite::new(
        MessageParticipantKind::Sender,
        sender.clone(),
    )];
    for (kind, addresses) in [
        (MessageParticipantKind::To, &draft.recipients_to),
        (MessageParticipantKind::Cc, &draft.recipients_cc),
    ] {
        for address in addresses {
            participants.push(MessageParticipantWrite::new(
                kind,
                super::qualified_email_identity(address.as_str())
                    .expect("test fixture recipient address is parseable"),
            ));
        }
    }

    MessageWrite {
        id: CanonicalMessageId::random(),
        thread_id: draft.thread_id,
        author: MessageAuthorWrite::Observed(IdentityObservation {
            identity: sender,
            display_label: None,
            claim_metadata: IdentityClaimMetadata::observation(),
            provenance: IdentityProvenance::EmailIngress,
        }),
        subject: draft.subject,
        clean_text_body: draft.clean_text_body,
        attachments: draft.attachments.unwrap_or_default(),
        direction: draft.direction,
        role: draft.role,
        correlation_id: CorrelationId::new(),
        participants,
        correlation: MessageCorrelation::Email(
            EmailMessageMetadata::new(draft.message_id)
                .in_reply_to(draft.in_reply_to)
                .references(draft.references_list)
                .thread_index(draft.thread_index)
                .raw_bodies(draft.raw_text_body, draft.raw_html_body),
        ),
        created_at: draft.created_at,
    }
}

impl InMemoryThreads {
    /// Which conversation these provider keys belong to, through the external maps.
    ///
    /// Conversation bindings first, then message keys: the same order the SQL reader uses, so
    /// reply-before-root resolves here too.
    fn find_thread_by_keys(&self, channel_id: Uuid, keys: &[String]) -> Option<Thread> {
        let store = self.store.lock().unwrap();
        for key in keys {
            let key = ProviderKey {
                channel_id,
                key: key.clone(),
            };
            if let Some(thread_id) = store.external_threads.get(&key).copied() {
                return store
                    .threads
                    .iter()
                    .find(|thread| thread.id == thread_id)
                    .cloned();
            }
        }
        for key in keys {
            let key = ProviderKey {
                channel_id,
                key: key.clone(),
            };
            let Some(canonical_id) = store.external_messages.get(&key).copied() else {
                continue;
            };
            let thread_id = store
                .associations
                .iter()
                .filter(|association| association.message_id == canonical_id)
                .max_by_key(|association| association.created_at)
                .map(|association| association.thread_id);
            if let Some(thread_id) = thread_id {
                return store
                    .threads
                    .iter()
                    .find(|thread| thread.id == thread_id)
                    .cloned();
            }
        }
        None
    }

    /// The canonical message a provider key already names in one channel.
    fn message_for_key(&self, channel_id: Uuid, key: &str) -> Option<CanonicalMessageId> {
        self.store
            .lock()
            .unwrap()
            .external_messages
            .get(&ProviderKey {
                channel_id,
                key: key.to_string(),
            })
            .copied()
    }
}

/// The ingest ports, backed by the same in-memory stores a fixture asserts against.
///
/// **Not transactional, and deliberately so.** Atomicity is a property of the SQL committer and is
/// tested against a real database; what this double provides is the same *ordering* and the same
/// dedup rule, so a policy test can run the whole pipeline without one. A test that cares whether
/// a partial commit is possible is a database test by definition.
///
/// One synthetic email binding per channel, with the channel's own id, so a binding-qualified key
/// and a channel-qualified key are the same fact here -- which is exactly what production means by
/// "email is a deployment transport and every channel has one interface".
#[derive(Clone)]
pub struct InMemoryIngress {
    threads: Arc<InMemoryThreads>,
    tasks: Arc<dyn TaskPersistence>,
}

impl InMemoryIngress {
    pub fn new(threads: Arc<InMemoryThreads>, tasks: Arc<dyn TaskPersistence>) -> Self {
        Self { threads, tasks }
    }

    /// The ports [`ThreadUseCases::for_test`] wires, all reading the stores given here.
    pub fn ports(
        threads: Arc<InMemoryThreads>,
        tasks: Arc<dyn TaskPersistence>,
    ) -> InboundIngestPorts {
        let ingress = Arc::new(Self::new(threads, tasks));
        InboundIngestPorts {
            committer: ingress.clone(),
            correlation: ingress.clone(),
            bindings: ingress,
        }
    }

    /// The channel a synthetic binding stands for.
    fn channel_of(binding_id: ChannelBindingId) -> Uuid {
        binding_id.as_uuid()
    }

    fn binding_of(channel_id: Uuid) -> ChannelBinding {
        ChannelBinding {
            id: ChannelBindingId::new(channel_id),
            company_id: Uuid::nil(),
            channel_id,
            installation_id: None,
            transport: TransportKind::Email,
            namespace: EndpointNamespace::parse("email").expect("a valid namespace"),
            external_endpoint_key: ExternalEndpointKey::parse(channel_id.to_string())
                .expect("a UUID is a valid endpoint key"),
            display_label: format!("{channel_id}@test"),
            access_policy: BindingAccessPolicy::ChannelAcl,
            delivery_policy: BindingDeliveryPolicy::ReplyAndInitiate,
            status: BindingStatus::Active,
            disabled_reason: None,
            created_by: CreationProvenance::system(),
            access_snapshot: BindingAccessSnapshot::deployment_endpoint(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

#[async_trait]
impl ChannelBindingPersistence for InMemoryIngress {
    async fn create_binding(&self, _write: BindingWrite) -> AppResult<ChannelBinding> {
        Err(AppError::Internal(
            "This double serves the one email interface every channel already has".into(),
        ))
    }

    async fn active_bindings_for_channel(
        &self,
        _company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelBinding>> {
        Ok(vec![Self::binding_of(channel_id)])
    }

    async fn find_active_binding_by_endpoint(
        &self,
        _endpoint: &InboundEndpoint,
    ) -> AppResult<Option<ChannelBinding>> {
        Ok(None)
    }

    async fn list_bindings_for_company(&self, _company_id: Uuid) -> AppResult<Vec<ChannelBinding>> {
        Ok(Vec::new())
    }

    async fn get_binding(
        &self,
        _company_id: Uuid,
        binding_id: ChannelBindingId,
    ) -> AppResult<Option<ChannelBinding>> {
        Ok(Some(Self::binding_of(Self::channel_of(binding_id))))
    }

    async fn set_binding_status(&self, _change: BindingStatusChange) -> AppResult<ChannelBinding> {
        Err(AppError::Internal(
            "This double does not change binding status".into(),
        ))
    }

    async fn list_binding_audit_events(
        &self,
        _company_id: Uuid,
        _binding_id: ChannelBindingId,
        _limit: i64,
    ) -> AppResult<Vec<BindingAuditEvent>> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ExternalCorrelationStore for InMemoryIngress {
    async fn thread_for_thread_keys(
        &self,
        binding_id: ChannelBindingId,
        thread_keys: &[ExternalThreadKey],
    ) -> AppResult<Option<Uuid>> {
        let keys: Vec<String> = thread_keys
            .iter()
            .map(|key| key.as_str().to_string())
            .collect();
        Ok(self
            .threads
            .find_thread_by_keys(Self::channel_of(binding_id), &keys)
            .map(|thread| thread.id))
    }

    async fn thread_for_message_keys(
        &self,
        binding_id: ChannelBindingId,
        message_keys: &[ExternalMessageKey],
    ) -> AppResult<Option<Uuid>> {
        let keys: Vec<String> = message_keys
            .iter()
            .map(|key| key.as_str().to_string())
            .collect();
        Ok(self
            .threads
            .find_thread_by_keys(Self::channel_of(binding_id), &keys)
            .map(|thread| thread.id))
    }

    async fn message_for_external_key(
        &self,
        binding_id: ChannelBindingId,
        message_key: &ExternalMessageKey,
    ) -> AppResult<Option<CanonicalMessageId>> {
        Ok(self
            .threads
            .message_for_key(Self::channel_of(binding_id), message_key.as_str()))
    }
}

#[async_trait]
impl InboundMessageCommitter for InMemoryIngress {
    async fn commit_inbound(
        &self,
        request: InboundCommitRequest,
    ) -> AppResult<InboundCommitOutcome> {
        let mut thread_ids = Vec::with_capacity(request.associations.len());
        for association in &request.associations {
            let emails: Vec<EmailAddress> = association
                .principals
                .iter()
                .filter(|intent| {
                    intent.role == crate::entities::participant::ThreadPrincipalRole::Participant
                        && intent.identity.transport() == TransportKind::Email
                })
                .map(|intent| EmailAddress::from(intent.identity.subject().as_str()))
                .collect();
            let thread_id = match &association.target {
                ThreadTarget::Existing(thread_id) => {
                    if !emails.is_empty() {
                        let thread = self
                            .threads
                            .get_thread_by_id(*thread_id)
                            .await?
                            .ok_or_else(|| AppError::NotFound("Thread was not found".into()))?;
                        let mut participants =
                            thread.participant_projection.email_addresses.clone();
                        for handle in &emails {
                            if !participants
                                .iter()
                                .any(|existing| existing.eq_ignore_ascii_case(handle))
                            {
                                participants.push(handle.clone());
                            }
                        }
                        self.threads
                            .update_thread_participants(*thread_id, &participants)
                            .await?;
                    }
                    *thread_id
                }
                ThreadTarget::Create { subject } => {
                    self.threads
                        .create_thread(association.channel_id, subject, &emails)
                        .await?
                        .id
                }
            };
            thread_ids.push(thread_id);
        }

        let envelope = &request.envelope;
        // Asked before the write, because `create_message` returns the stored message either way:
        // whether this delivery is the first one is what the caller needs to know.
        let already_stored = self
            .threads
            .message_for_key(
                Self::channel_of(envelope.source.binding_id),
                envelope.source.message_key.as_str(),
            )
            .is_some();
        let write = inbound_message_write(envelope, thread_ids[0]);
        // `create_message` is where the double's dedup lives: a repeated provider key returns the
        // stored message, and one carrying different content is refused.
        let stored = self.threads.create_message(&write).await?;
        let mut association_by_channel = std::collections::HashMap::new();
        association_by_channel.insert(request.associations[0].channel_id, stored.id);
        for (association, thread_id) in request.associations.iter().zip(&thread_ids).skip(1) {
            let message = self
                .threads
                .associate_message(*thread_id, stored.canonical_id)
                .await?;
            association_by_channel.insert(association.channel_id, message.id);
        }
        if !already_stored {
            for transition in &request.outreach_transitions {
                let response_id = *association_by_channel
                    .get(&transition.channel_id)
                    .ok_or_else(|| {
                        AppError::BadRequest(
                            "An outreach transition has no message association".into(),
                        )
                    })?;
                self.tasks
                    .record_outreach_reply(&transition.matched, response_id)
                    .await?;
            }
        }

        let mut task_id = None;
        if let Some(task) = request.task.as_ref() {
            let thread_of: std::collections::HashMap<Uuid, Uuid> = request
                .associations
                .iter()
                .map(|association| association.channel_id)
                .zip(thread_ids.iter().copied())
                .collect();
            let targets: Vec<TaskTarget> = task
                .targets
                .iter()
                .map(|target| {
                    Ok(TaskTarget {
                        channel_id: target.channel_id,
                        thread_id: *thread_of.get(&target.channel_id).ok_or_else(|| {
                            AppError::Internal("A task target has no association".into())
                        })?,
                        recipient_role: target.role,
                    })
                })
                .collect::<AppResult<_>>()?;
            let primary = *targets
                .first()
                .ok_or_else(|| AppError::Internal("An inbound task names no channel".into()))?;
            let payload = InboundTaskPayload::v1(InboundTaskPayloadV1 {
                company_id: request.company_id,
                channel_id: primary.channel_id,
                thread_id: primary.thread_id,
                source_message_id: stored.canonical_id,
                correlation_id: envelope.correlation_id,
                hop_count: envelope.directives.hop_count,
                trace_channels: envelope.directives.trace_channels.clone(),
                is_forwarded: envelope.directives.is_forwarded,
                reply_delivery: request.reply_delivery,
            })
            .encode()?;
            let created = self
                .tasks
                .enqueue_task(NewTask {
                    company_id: request.company_id,
                    channel_id: primary.channel_id,
                    thread_id: Some(primary.thread_id),
                    task_type: task.task_type.clone(),
                    payload,
                    targets,
                    source: TaskSource::Message(stored.canonical_id),
                    correlation_id: envelope.correlation_id,
                })
                .await?;
            task_id = Some(created.id);
        }

        Ok(InboundCommitOutcome {
            disposition: if already_stored {
                CommitDisposition::Duplicate
            } else {
                CommitDisposition::Created
            },
            message_id: stored.canonical_id,
            thread_ids,
            task_id,
            delivery_ids: Vec::new(),
        })
    }
}

/// The producer vocabulary for one arriving message, mirroring what the SQL committer projects.
fn inbound_message_write(envelope: &InboundEnvelope, thread_id: Uuid) -> MessageWrite {
    let mut participants = vec![MessageParticipantWrite::new(
        MessageParticipantKind::Sender,
        envelope.author.clone(),
    )];
    for addressed in &envelope.addressed {
        participants.push(MessageParticipantWrite::new(
            addressed.role.participant_kind(),
            addressed.identity.clone(),
        ));
    }
    MessageWrite {
        id: CanonicalMessageId::random(),
        thread_id,
        author: MessageAuthorWrite::Observed(IdentityObservation {
            identity: envelope.author.clone(),
            display_label: None,
            claim_metadata: IdentityClaimMetadata::observation(),
            provenance: IdentityProvenance::EmailIngress,
        }),
        subject: envelope.content.subject().to_string(),
        clean_text_body: envelope.content.body_text().to_string(),
        attachments: envelope.attachments.to_vec(),
        direction: MessageDirection::Inbound,
        role: if envelope.directives.source_channel_id.is_some() {
            MessageRole::Agent
        } else {
            MessageRole::Human
        },
        correlation_id: envelope.correlation_id,
        participants,
        correlation: match envelope.extension.email_metadata() {
            Some(metadata) => MessageCorrelation::Email(metadata.clone()),
            None => MessageCorrelation::Internal,
        },
        created_at: Utc::now(),
    }
}

impl crate::use_cases::thread::ThreadUseCases {
    /// Ingest one mail exactly as the SMTP listener does: through the email adapter, with the
    /// verdicts a verifying boundary established, as external transport traffic.
    ///
    /// Tests reach for this rather than assembling an envelope by hand so that the address
    /// grammar, the bounds and the policy facts are the ones production applies -- a fixture that
    /// built its own envelope could not fail the way a real message fails.
    pub async fn ingest_test_email(
        &self,
        payload: crate::adapters::protocols::email::parser::RawInboundPayload,
    ) -> AppResult<crate::use_cases::thread::InboundIngestResult> {
        self.ingest_test_email_as(
            payload,
            crate::use_cases::thread::IngressOrigin::ExternalTransport,
        )
        .await
    }

    /// The same, for a message an authenticated route composed instead.
    pub async fn ingest_test_email_as(
        &self,
        payload: crate::adapters::protocols::email::parser::RawInboundPayload,
        origin: crate::use_cases::thread::IngressOrigin,
    ) -> AppResult<crate::use_cases::thread::InboundIngestResult> {
        use crate::adapters::protocols::email::{
            EmailIngressAdapter, EmailIngressTrust, VerifiedEmailAuth,
        };
        let trust = match origin {
            crate::use_cases::thread::IngressOrigin::ExternalTransport => {
                EmailIngressTrust::Verified(VerifiedEmailAuth {
                    spf: payload.spf,
                    dkim: payload.dkim,
                    dmarc: payload.dmarc,
                    spam_score: payload.spam_score,
                })
            }
            _ => EmailIngressTrust::Application,
        };
        let accepted = EmailIngressAdapter::for_config(self.config()).accept(payload, trust)?;
        self.ingest(accepted.into_inbound(origin, crate::use_cases::thread::ReplyDelivery::Send))
            .await
    }
}
