//! Purpose-built reads of a canonical message.
//!
//! [`crate::entities::message::Message`] is the whole stored message: author, participants, email
//! headers, raw bodies, attachments. Almost nothing reads all of that. A mailbox bubble wants a
//! name, a body and a timestamp; an agent prompt wants a role and a body; the mail renderer wants
//! headers and nothing else.
//!
//! So each reader gets its own projection rather than a widening entity, for three reasons:
//!
//! - a projection that has no email fields *cannot* be written to assume mail, which is what makes
//!   a Slack post render correctly by construction rather than by review;
//! - the query behind it selects only its own columns, so a thread page stops paying for raw MIME
//!   bodies it never displays; and
//! - every one of them is bounded at the read, so no page or prompt is sized by how long a
//!   conversation has been running.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::entities::{
    correlation::CorrelationId,
    cursor::MessageCursor,
    message::{AttachmentMetadata, CanonicalMessageId, MessageDirection, MessageRole},
    transport::{ChannelBindingId, ExternalMessageKey, PrincipalId, TransportKind},
    value_objects::{EmailAddress, MessageId},
};

/// The newest messages a thread page or an agent prompt reads.
///
/// A cap rather than a preference: a thread is appended to by everyone who can reach the channel,
/// so an unbounded history read is a page and a prompt whose size a correspondent chooses.
pub const THREAD_HISTORY_LIMIT: usize = 200;

/// Who a message is shown as being from.
///
/// The principal is the decision-grade identity and the only field authorization would ever be
/// entitled to look at -- but nothing here is an authorization input. `label` and `handle` are
/// display, and `transport` is the badge that says which interface carried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorView {
    pub principal_id: PrincipalId,
    /// `principals.display_label`: the actor's name, independent of any transport.
    pub label: String,
    /// The handle the author wrote under, when a transport named one at all.
    pub handle: Option<String>,
    /// Which interface that handle belongs to, for the transport badge beside the name.
    pub transport: Option<TransportKind>,
}

impl AuthorView {
    /// What to print as the author's name.
    ///
    /// The principal's label wins over the handle: a message from a known member reads as their
    /// name rather than as whichever mailbox or workspace account they happened to use, and a
    /// message from a stranger falls back to the handle because that is all there is.
    pub fn display(&self) -> &str {
        match self.label.trim() {
            "" => self.handle.as_deref().unwrap_or("Unknown"),
            label => label,
        }
    }

    /// This author's mailbox, when the handle they wrote under is an email one.
    pub fn email_address(&self) -> Option<EmailAddress> {
        self.handle
            .as_deref()
            .filter(|_| self.transport == Some(TransportKind::Email))
            .map(EmailAddress::from)
    }
}

/// One message as a thread page renders it: mailbox, task detail, simulation.
///
/// Has no recipients and no headers, and that is the point. A conversation message may be
/// addressed to a Slack conversation rather than to a `To:` list, so a page built on this one
/// renders every transport without a branch. Where an operator genuinely needs the envelope, they
/// read [`MessageAuditView`] instead.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadMessageView {
    /// `thread_messages.id` -- this message *in this thread*, and what the live stream resumes on.
    pub id: Uuid,
    /// `messages.id` -- the payload every thread association of this message shares.
    pub canonical_id: CanonicalMessageId,
    pub thread_id: Uuid,
    pub task_id: Option<Uuid>,
    pub author: AuthorView,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<AttachmentMetadata>,
    pub direction: MessageDirection,
    pub role: MessageRole,
    pub created_at: DateTime<Utc>,
}

impl ThreadMessageView {
    /// This message's position in its thread -- what a live reader resumes from.
    pub fn cursor(&self) -> MessageCursor {
        MessageCursor {
            created_at: self.created_at,
            id: self.id,
        }
    }

    /// Whether an agent, rather than a person, wrote this.
    pub fn is_agent(&self) -> bool {
        self.role == MessageRole::Agent || self.direction == MessageDirection::Outbound
    }
}

/// One turn of a conversation as an agent prompt renders it.
///
/// The narrowest projection in the file: a role, who said it, what topic it was under, and the
/// words. An agent needs no message ids, no addresses and no headers to read a thread, and giving
/// it any would put provider strings inside the prompt fence for no gain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentHistoryMessage {
    pub role: MessageRole,
    /// The author's display name, rendered as data inside the untrusted fence -- never as a key.
    pub author_display: String,
    pub subject: String,
    pub body: String,
}

/// What the mail renderer needs to thread a reply onto a message already stored.
///
/// Every field is optional or empty for a message no mail carried, which is the honest answer:
/// a Slack post has no `Message-ID` to reply to, and a caller that needs one has to say what it
/// will do instead rather than receive a fabricated header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailReplyContext {
    pub canonical_id: CanonicalMessageId,
    /// Where a reply is addressed, when the author is reachable by mail.
    pub author_email: Option<EmailAddress>,
    /// The header a reply sets `In-Reply-To` from.
    pub rfc_message_id: Option<MessageId>,
    /// The chain a reply extends.
    pub references: Vec<MessageId>,
    /// Who else the message copied, so a reply keeps them on it.
    pub cc: Vec<EmailAddress>,
}

/// One message with everything an operator needs to trace it, and nothing a page renders.
///
/// This is the only projection that exposes provider identifiers, which is why it is separate:
/// a diagnostic pane behind an authorization check can show a Message-ID or a Slack timestamp,
/// and no ordinary read path can reach one by accident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageAuditView {
    pub id: Uuid,
    pub canonical_id: CanonicalMessageId,
    pub company_id: Uuid,
    pub thread_id: Uuid,
    pub channel_id: Uuid,
    pub author: AuthorView,
    pub direction: MessageDirection,
    pub role: MessageRole,
    pub correlation_id: CorrelationId,
    /// The provider keys this message is reachable by, one per interface that carried it.
    pub external_keys: Vec<ExternalMessageRef>,
    pub created_at: DateTime<Utc>,
}

/// One provider key, qualified by the interface it belongs to.
///
/// Qualified because it has to be: the same `Message-ID` text is one outbound message on the
/// sending channel's binding and one inbound message on the receiving channel's, and a bare key
/// names neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalMessageRef {
    pub binding_id: ChannelBindingId,
    pub transport: TransportKind,
    pub key: ExternalMessageKey,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::transport::PrincipalId;

    fn author(label: &str, handle: Option<&str>) -> AuthorView {
        AuthorView {
            principal_id: PrincipalId::random(),
            label: label.to_string(),
            handle: handle.map(str::to_string),
            transport: handle.map(|_| TransportKind::Email),
        }
    }

    #[test]
    fn a_named_principal_is_shown_by_name_not_by_the_handle_it_used() {
        let view = author("Ada Lovelace", Some("ada@example.com"));
        assert_eq!(view.display(), "Ada Lovelace");
        assert_eq!(
            view.email_address(),
            Some(EmailAddress::from("ada@example.com"))
        );
    }

    /// The transport badge is what tells a reader a message came in over Slack, so a handle
    /// without one is not silently rendered as a mailbox.
    #[test]
    fn a_handle_on_another_transport_is_not_an_email_address() {
        let view = AuthorView {
            transport: Some(TransportKind::Slack),
            ..author("Ada", Some("U0123ABC"))
        };
        assert_eq!(view.email_address(), None);
        assert_eq!(view.display(), "Ada");
    }

    /// An external principal created from a sighting has a label, but a blank one must not render
    /// as an empty name where a handle is available.
    #[test]
    fn a_blank_label_falls_back_to_the_handle_and_then_to_a_placeholder() {
        assert_eq!(
            author("  ", Some("ada@example.com")).display(),
            "ada@example.com"
        );
        assert_eq!(author("  ", None).display(), "Unknown");
    }
}
