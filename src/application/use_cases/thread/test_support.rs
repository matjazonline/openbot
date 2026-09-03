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
    app_error::{AppError, AppResult},
    entities::participant::{IdentityClaimMetadata, IdentityProvenance},
    entities::{
        correlation::CorrelationId,
        cursor::{MessageCursor, ThreadCursor},
        email_message::EmailMessageMetadata,
        message::{
            AttachmentMetadata, CanonicalMessageId, Message, MessageAuthor, MessageDirection,
            MessageParticipant, MessageParticipantKind, MessageRole,
        },
        thread::{Thread, ThreadParticipantProjection},
        transport::{ParticipantIdentityId, PrincipalId, QualifiedIdentity},
        value_objects::{EmailAddress, MessageId, ThreadIndex},
    },
    use_cases::{
        participant::IdentityObservation,
        participant::test_support::{principal_for_email, principal_for_identity},
        thread::{
            MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite,
            ThreadPersistence,
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
}

/// An in-memory [`ThreadPersistence`].
#[derive(Clone, Default)]
pub struct InMemoryThreads {
    store: Arc<Mutex<Store>>,
    company_id: Uuid,
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

    async fn find_thread_by_message_ids(
        &self,
        channel_id: Uuid,
        message_ids: &[MessageId],
    ) -> AppResult<Option<Thread>> {
        let store = self.store.lock().unwrap();
        // Conversation bindings first, then message keys: the same order the real reader uses, so
        // reply-before-root resolves here too.
        for message_id in message_ids {
            let key = ProviderKey {
                channel_id,
                key: message_id.as_str().to_string(),
            };
            if let Some(thread_id) = store.external_threads.get(&key) {
                let thread_id = *thread_id;
                return Ok(store
                    .threads
                    .iter()
                    .find(|thread| thread.id == thread_id)
                    .cloned());
            }
        }
        for message_id in message_ids {
            let key = ProviderKey {
                channel_id,
                key: message_id.as_str().to_string(),
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
                return Ok(store
                    .threads
                    .iter()
                    .find(|thread| thread.id == thread_id)
                    .cloned());
            }
        }
        Ok(None)
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
            id: CanonicalMessageId::random(),
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

    async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
        let store = self.store.lock().unwrap();
        let mut messages: Vec<Message> = store
            .associations
            .iter()
            .filter(|association| association.thread_id == thread_id)
            .filter_map(|association| self.read(&store, association))
            .collect();
        messages.sort_by_key(Message::cursor);
        Ok(messages)
    }

    async fn list_messages_after(
        &self,
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<Message>> {
        let store = self.store.lock().unwrap();
        let mut messages: Vec<Message> = store
            .associations
            .iter()
            .filter(|association| association.thread_id == thread_id)
            .filter_map(|association| self.read(&store, association))
            .filter(|message| after.is_none_or(|cursor| message.cursor() > cursor))
            .collect();
        messages.sort_by_key(Message::cursor);
        messages.truncate(limit);
        Ok(messages)
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
