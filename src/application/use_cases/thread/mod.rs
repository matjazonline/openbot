//! Thread use cases: everything that happens to a message between arriving at the platform and
//! being filed in its threads, and the messages people write back.
//!
//! This module owns the types and the [`ThreadUseCases`] handle; the ingest pipeline lives next to
//! it in [`ingest`], with its shared helpers in [`support`].

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app_error::AppError;
use crate::app_error::AppResult;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::integration::ChannelBindingPersistence;
use crate::application::ports::thread::ProtocolExtension;
use crate::application::settings::ThreadSettings;
use crate::domain::monitoring::MonitoringService;
use crate::entities::channel::Channel;
use crate::entities::company::Company;
use crate::entities::correlation::CorrelationId;
use crate::entities::cursor::MessageCursor;
use crate::entities::cursor::ThreadCursor;
use crate::entities::email_message::EmailMessageMetadata;
use crate::entities::internal_note::AddInternalNote;
use crate::entities::internal_note::InternalNoteView;
use crate::entities::internal_note::TombstoneInternalNote;
use crate::entities::message::CanonicalMessageId;
use crate::entities::message::Message;
use crate::entities::message::MessageRole;
use crate::entities::message::ThreadEntryKind;
use crate::entities::message_view::EmailReplyContext;
use crate::entities::message_view::MessageAuditView;
use crate::entities::message_view::ThreadMessageView;
use crate::entities::thread::Thread;
use crate::entities::transport::ChannelSelector;
use crate::entities::transport::ExternalMessageKey;
use crate::entities::transport::ExternalThreadKey;
use crate::entities::transport::QualifiedIdentity;
use crate::entities::user::Viewer;
use crate::entities::value_objects::ChannelSlug;
use crate::entities::value_objects::CompanySlug;
use crate::entities::value_objects::EmailAddress;
use crate::entities::value_objects::MessageId;
use crate::transport::AddressedIdentity;
use crate::transport::BoundedVec;
use crate::transport::CanonicalContent;
use crate::transport::DeliveryComposer;
use crate::transport::ExternalCorrelationStore;
use crate::transport::InboundDraft;
use crate::transport::InboundEnvelope;
use crate::transport::InboundMessageCommitter;
use crate::transport::IngressDirectives;
use crate::transport::IngressPolicyFacts;
use crate::transport::InternalMailRelay;
use crate::transport::InternalRelayMail;
use crate::transport::MessageDisposition;
use crate::transport::RelayDisposition;
use crate::transport::StandaloneDeliveryEnqueuer;
use crate::transport::StandaloneDeliveryRequest;
use crate::transport::ports::TransportRenderers;
use crate::use_cases::channel::ChannelReaders;
use crate::use_cases::channel::readable_channel;
use crate::use_cases::company::CompanyPersistence;
use crate::use_cases::participant::ParticipantPersistence;

mod ingest;
#[cfg(test)]
use crate::application::ports::thread::MessageAuthorWrite;
use crate::application::ports::thread::{
    MessageWrite, ThreadPersistence, qualified_email_identity,
};
pub use ingest::{
    CanonicalMessageIngress, InboundMessage, InboundPreflight, IngestRejection, IngressOrigin,
    PreparedInbound, UnusableHint,
};
mod notices;
mod reads;
#[cfg(test)]
pub(crate) use reads::project_message;
pub use reads::{ChannelReadScope, ThreadReadScope};
mod support;
pub use notices::{
    BounceInfo, BounceSuggestion, ChannelDirectoryEntry, bounce_cause, format_bounce_email_body,
    format_help_email_body,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenderTrust {
    Trusted,
    Stranger,
}

/// Addressing role and multi-channel pipeline position are transport vocabulary; they are declared
/// with the other transport contracts and re-exported here so call sites read unchanged.
pub use crate::transport::{PipelineStep, RecipientRole, ReplyDelivery};

#[cfg(test)]
pub mod test_support;

#[cfg(test)]
mod tests;

pub const MAX_THREAD_MESSAGES_PER_HOUR: usize = 60;

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

#[derive(Clone)]
pub struct ThreadUseCases {
    thread_persistence: Arc<dyn ThreadPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    company_persistence: Arc<dyn CompanyPersistence>,
    participant_persistence: Arc<dyn ParticipantPersistence>,
    committer: Arc<dyn InboundMessageCommitter>,
    correlation_store: Arc<dyn ExternalCorrelationStore>,
    binding_persistence: Arc<dyn ChannelBindingPersistence>,
    standalone_deliveries: Arc<dyn StandaloneDeliveryEnqueuer>,
    /// Resolves an interface and freezes what will be sent, so a reply is queued in the same
    /// transaction that records it. Required rather than optional: a deployment that forgot to
    /// wire it would answer every customer in the thread and mail nobody.
    deliveries: DeliveryComposer,
    monitoring: Option<Arc<dyn MonitoringService>>,
    /// Woken after a commit wrote workflow-admission intents (p08 D1.4); the worker also polls.
    intent_wakeups: Option<Arc<tokio::sync::Notify>>,
    settings: Arc<ThreadSettings>,
}

/// The stores one [`ThreadUseCases`] reads and writes.
///
/// A named struct rather than four positional `Arc<dyn ...>` parameters: they are all the same
/// shape at the call site, so a transposed pair compiles and then reads companies out of the
/// channel store. `src/adapters/persistence/AGENTS.md` names this exact fix.
#[derive(Clone)]
pub struct ThreadStores {
    pub threads: Arc<dyn ThreadPersistence>,
    pub channels: Arc<dyn ChannelPersistence>,
    pub companies: Arc<dyn CompanyPersistence>,
    pub participants: Arc<dyn ParticipantPersistence>,
}

impl ThreadUseCases {
    pub fn new(
        stores: ThreadStores,
        ingest: InboundIngestPorts,
        renderers: Arc<TransportRenderers>,
        settings: Arc<ThreadSettings>,
    ) -> Self {
        let deliveries = DeliveryComposer::new(renderers, ingest.bindings.clone());

        Self {
            thread_persistence: stores.threads,
            channel_persistence: stores.channels,
            company_persistence: stores.companies,
            participant_persistence: stores.participants,
            committer: ingest.committer,
            correlation_store: ingest.correlation,
            standalone_deliveries: ingest.standalone_deliveries,
            binding_persistence: ingest.bindings,
            deliveries,
            monitoring: None,
            intent_wakeups: None,
            settings,
        }
    }

    pub fn with_intent_wakeups(mut self, wakeups: Arc<tokio::sync::Notify>) -> Self {
        self.intent_wakeups = Some(wakeups);
        self
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    /// Read-only trust decision shared with routing; failures remain distinct from strangers.
    pub async fn sender_trust(
        &self,
        selector: &ChannelSelector,
        sender: &QualifiedIdentity,
    ) -> AppResult<SenderTrust> {
        let Some(slug) = selector.company() else {
            return Ok(SenderTrust::Stranger);
        };
        let mut directory = support::DirectoryCache::new(self);
        let Some(company) = directory.company(slug).await? else {
            return Ok(SenderTrust::Stranger);
        };
        let channels = directory.channels(company.id).await?;
        let Some(channel) = channels
            .iter()
            .find(|channel| channel.matches_slug(selector.channel()))
        else {
            return Ok(SenderTrust::Stranger);
        };
        let context = directory.access_context(company.id, sender).await?;
        Ok(if channel.participant_access(context).trusted {
            SenderTrust::Trusted
        } else {
            SenderTrust::Stranger
        })
    }

    /// A thread the viewer may read, found by its whole path: the channel must be one the viewer
    /// may read in `company_id`, and the thread must belong to that channel.
    ///
    /// `None` for an unknown thread, another channel's, another company's and one in a channel the
    /// viewer may not read alike, so a guessed id learns nothing. A lookup failure is an error.
    pub async fn readable_thread(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Uuid,
    ) -> AppResult<Option<Thread>> {
        let readers = ChannelReaders {
            companies: self.company_persistence.as_ref(),
            channels: self.channel_persistence.as_ref(),
            participants: self.participant_persistence.as_ref(),
        };
        let Some(channel) = readable_channel(&readers, viewer, company_id, channel_id).await?
        else {
            return Ok(None);
        };
        let thread = self
            .thread_persistence
            .get_thread_by_id(thread_id)
            .await?
            .filter(|thread| thread.channel_id == channel.id);
        let projected = self
            .project_thread_titles(viewer, company_id, thread.into_iter().collect())
            .await?;
        Ok(projected.into_iter().next())
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

    /// Internal email threading metadata; viewer rendering uses viewer_email_reply_context.
    pub async fn latest_email_reply_context(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<EmailReplyContext>> {
        self.thread_persistence
            .latest_email_reply_context(thread_id)
            .await
    }

    pub async fn latest_replyable_email_context(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<EmailReplyContext>> {
        self.thread_persistence
            .latest_replyable_email_context(thread_id)
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

    /// The newest canonical message ID in a thread.
    pub async fn latest_thread_message_id(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<CanonicalMessageId>> {
        let messages = self
            .thread_persistence
            .list_thread_message_views(thread_id)
            .await?;
        Ok(messages.last().map(|m| m.canonical_id))
    }

    async fn resolve_target_thread(
        &self,
        channel_id: Uuid,
        target_thread_id: Option<Uuid>,
        reply_to_message_id: Option<CanonicalMessageId>,
    ) -> AppResult<Option<Thread>> {
        let thread = match target_thread_id {
            Some(thread_id) => self
                .thread_persistence
                .get_thread_by_id(thread_id)
                .await?
                .filter(|t| t.channel_id == channel_id),
            None => match reply_to_message_id {
                Some(reply_to) => {
                    self.thread_persistence
                        .find_thread_for_message(channel_id, reply_to)
                        .await?
                }
                None => None,
            },
        };

        if let (Some(thread), Some(reply_to)) = (&thread, reply_to_message_id) {
            let msg = self
                .thread_persistence
                .get_thread_message(thread.id, reply_to)
                .await?;
            if msg.is_none() {
                return Ok(None);
            }
        }

        Ok(thread)
    }

    async fn resolve_in_reply_to_rfc(
        &self,
        company_id: Uuid,
        thread_id: Option<Uuid>,
        reply_to_message_id: Option<CanonicalMessageId>,
    ) -> AppResult<Option<MessageId>> {
        if let Some(reply_to) = reply_to_message_id
            && let Ok(ext) = self
                .thread_persistence
                .get_message_protocol_extension(company_id, reply_to)
                .await
            && let Some(rfc) = ext.email_metadata().map(|m| m.rfc_message_id.clone())
        {
            return Ok(Some(rfc));
        }
        if let Some(thread_id) = thread_id {
            return self.latest_thread_rfc_message_id(thread_id).await;
        }
        Ok(None)
    }

    /// Internal outbound correlation for runtime processing; not a viewer projection.
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

    pub async fn add_internal_note(
        &self,
        command: AddInternalNote,
        actor: crate::entities::participant::PrincipalAccessContext,
    ) -> AppResult<ThreadMessageView> {
        command.validate().map_err(AppError::BadRequest)?;
        let principal_id = actor
            .principal_id
            .ok_or_else(|| AppError::NotFound("Thread not found.".into()))?;
        let channel = self
            .channel_persistence
            .get_by_id(command.channel_id)
            .await?
            .filter(|channel| channel.company_id == command.company_id)
            .ok_or_else(|| AppError::NotFound("Thread not found.".into()))?;
        if !channel.viewer_access(actor) {
            return Err(AppError::NotFound("Thread not found.".into()));
        }
        let thread = self
            .thread_persistence
            .get_thread_by_id(command.thread_id)
            .await?
            .filter(|thread| thread.channel_id == channel.id)
            .ok_or_else(|| AppError::NotFound("Thread not found.".into()))?;
        debug_assert_eq!(thread.channel_id, command.channel_id);
        self.thread_persistence
            .create_internal_note(&command, principal_id)
            .await
    }

    pub async fn tombstone_internal_note(
        &self,
        command: TombstoneInternalNote,
        actor: crate::entities::participant::PrincipalAccessContext,
    ) -> AppResult<InternalNoteView> {
        let principal_id = actor
            .principal_id
            .ok_or_else(|| AppError::NotFound("Thread not found.".into()))?;
        let channel = self
            .channel_persistence
            .get_by_id(command.channel_id)
            .await?
            .filter(|channel| channel.company_id == command.company_id)
            .ok_or_else(|| AppError::NotFound("Thread not found.".into()))?;
        if !channel.viewer_access(actor) {
            return Err(AppError::NotFound("Thread not found.".into()));
        }
        let thread = self
            .thread_persistence
            .get_thread_by_id(command.thread_id)
            .await?
            .filter(|thread| thread.channel_id == channel.id)
            .ok_or_else(|| AppError::NotFound("Thread not found.".into()))?;
        debug_assert_eq!(thread.channel_id, command.channel_id);
        self.thread_persistence
            .tombstone_internal_note(&command, principal_id)
            .await
    }

    /// Attach a message that is already stored to another of its company's threads.
    pub async fn associate_message(
        &self,
        thread_id: Uuid,
        message: CanonicalMessageId,
        entry_kind: ThreadEntryKind,
    ) -> AppResult<Message> {
        self.thread_persistence
            .associate_message(thread_id, message, entry_kind)
            .await
    }

    pub async fn principal_access_for_user(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<crate::entities::participant::PrincipalAccessContext>> {
        self.participant_persistence
            .access_context_for_user(company_id, user_id)
            .await
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

/// What one inbound message became.
///
/// Deliberately **not** serializable: it holds resolved entities and provider-derived content, and
/// is only the in-process result of one ingest. What outlives the request is the stored message and
/// the admission intents the commit wrote, which carry identifiers only.
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
    /// Admission intents created or recognised by this ingest, in association order (p08 D1.3).
    pub intent_ids: Vec<Uuid>,
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

    pub fn rejected(rejection: IngestRejection) -> Self {
        Self {
            accepted: false,
            rejection: Some(rejection),
            thread: None,
            inbound_message: None,
            company: None,
            channel: None,
            envelope: None,
            intent_ids: Vec::new(),
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
