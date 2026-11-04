//! Shared message writes and thread persistence; no runtime or use-case dependency.
use crate::{
    app_error::{AppError, AppResult},
    entities::{
        cursor::{MessageCursor, ThreadCursor},
        internal_note::{AddInternalNote, InternalNoteView, TombstoneInternalNote},
        message::{CanonicalMessageId, Message, MessageRole, ThreadEntryKind},
        message_view::{
            AgentHistoryMessage, EmailReplyContext, MessageAuditView, ThreadMessageView,
        },
        thread::Thread,
        transport::{IdentityNamespace, IdentitySubject, QualifiedIdentity, TransportKind},
        value_objects::{EmailAddress, MessageId, ThreadIndex},
    },
};
use async_trait::async_trait;
use std::collections::HashMap;
use uuid::Uuid;
mod message_write;
mod protocol_extension;
pub use message_write::{
    AgentAuthor, MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite,
};
pub use protocol_extension::ProtocolExtension;
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

#[async_trait]
pub trait ThreadPersistence: Send + Sync {
    async fn create_thread(
        &self,
        channel_id: Uuid,
        subject: &str,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread>;

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

    /// Find the thread containing this message within a given channel, if any.
    async fn find_thread_for_message(
        &self,
        channel_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<Thread>>;

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
        entry_kind: ThreadEntryKind,
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

    /// The newest inbound email turn a human final response can safely answer, looking past
    /// internal notes and agent turns that have no external author address.
    ///
    /// Required: falling back to the newest turn would let an internal note or an agent turn
    /// redirect a human's final response.
    async fn latest_replyable_email_context(
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

    /// Create one private note and its immutable lifecycle row in the same transaction.
    async fn create_internal_note(
        &self,
        command: &AddInternalNote,
        actor: crate::entities::transport::PrincipalId,
    ) -> AppResult<ThreadMessageView>;

    /// Record removal without deleting or rewriting the canonical note body.
    async fn tombstone_internal_note(
        &self,
        command: &TombstoneInternalNote,
        actor: crate::entities::transport::PrincipalId,
    ) -> AppResult<InternalNoteView>;
}
