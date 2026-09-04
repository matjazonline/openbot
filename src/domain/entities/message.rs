//! The canonical message: what was said, by which actor, in which conversation.
//!
//! Nothing here is email-shaped. A message has an author *principal*, a subject, a body and a
//! role; it does not have a sender address, a Message-ID, or To/Cc arrays, because a Slack post, a
//! schedule's prompt and an approval note have none of those and would have to fabricate them.
//!
//! Protocol facts are loaded through application transport projections. [`MessageParticipant`]
//! retains only a qualified identity and its generic addressing role.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::correlation::CorrelationId;
use crate::entities::cursor::MessageCursor;
use crate::entities::transport::{
    ChannelBindingId, ExternalMessageKey, ParticipantIdentityId, PrincipalId, QualifiedIdentity,
    uuid_id,
};
use crate::entities::value_objects::ObjectKey;

uuid_id!(CanonicalMessageId);

/// A provider replayed a message key it had already used, carrying different content.
///
/// Raised rather than absorbed: silently updating the stored message would rewrite history that
/// agents have already read and replied to, and silently ignoring the new content would hide a
/// provider or adapter fault. The caller decides, with the identifiers needed to investigate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "provider message key '{external_message_key}' on binding {binding_id} already stores \
     canonical message {existing_message_id} with different content"
)]
pub struct ExternalMessageCollision {
    pub binding_id: ChannelBindingId,
    pub external_message_key: ExternalMessageKey,
    pub existing_message_id: CanonicalMessageId,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    Human,
    Agent,
    System,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::Human => "human",
            MessageRole::Agent => "agent",
            MessageRole::System => "system",
        }
    }
}

impl FromStr for MessageRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "human" => Ok(MessageRole::Human),
            "agent" => Ok(MessageRole::Agent),
            "system" => Ok(MessageRole::System),
            _ => Err(format!("Unknown message role: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageDirection {
    Inbound,
    Outbound,
}

impl MessageDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageDirection::Inbound => "inbound",
            MessageDirection::Outbound => "outbound",
        }
    }
}

impl FromStr for MessageDirection {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "inbound" => Ok(MessageDirection::Inbound),
            "outbound" => Ok(MessageDirection::Outbound),
            _ => Err(format!("Unknown message direction: {}", s)),
        }
    }
}

/// Which role one identity played on a message.
///
/// Only transports that address recipients by name populate these. Slack posts into a
/// conversation rather than to a list, and a schedule prompt is addressed to nobody at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageParticipantKind {
    Sender,
    To,
    Cc,
}

impl MessageParticipantKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sender => "sender",
            Self::To => "to",
            Self::Cc => "cc",
        }
    }
}

impl FromStr for MessageParticipantKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "sender" => Ok(Self::Sender),
            "to" => Ok(Self::To),
            "cc" => Ok(Self::Cc),
            _ => Err(format!("Unknown message participant kind: {value}")),
        }
    }
}

/// One identity's part in a message, at a fixed position.
///
/// `position` is stored rather than derived: the order a message was addressed in is what a
/// rendered `To:` header must reproduce, and a query's incidental sort order is not that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageParticipant {
    pub kind: MessageParticipantKind,
    pub position: u16,
    pub identity_id: ParticipantIdentityId,
    /// The handle itself, filled in on read so a caller rendering a header needs no second query.
    pub identity: QualifiedIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentMetadata {
    pub filename: String,
    pub content_type: String,
    pub sha256_hash: String,
    pub size_bytes: usize,
    /// Where the bytes are kept, as a key inside the private bucket -- never a URL.
    ///
    /// A URL here would be a URL somewhere, and what arrives in the mail is not for anyone
    /// holding a link: it is served by the app, to whoever the channel's rules allow.
    /// `None` is mail that arrived before there was anywhere to put it, or whose upload failed.
    #[serde(default, alias = "storage_url")]
    pub storage_key: Option<ObjectKey>,
}

/// The stored form of a message's attachments.
///
/// Versioned and discriminated because this is untrusted data read back long after it was
/// written: the database bounds it and checks its shape, and the decode here is fallible so a
/// payload from a newer writer is an error rather than a panic. The `messages_attachments_check`
/// constraint in the migration is this type expressed in SQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum MessageAttachments {
    #[serde(rename = "1")]
    V1 { items: Vec<AttachmentMetadata> },
}

impl MessageAttachments {
    pub fn new(items: Vec<AttachmentMetadata>) -> Self {
        Self::V1 { items }
    }

    pub fn into_items(self) -> Vec<AttachmentMetadata> {
        match self {
            Self::V1 { items } => items,
        }
    }
}

/// Who a message is attributed to.
///
/// The principal is the decision-grade identity; the handle and the label are read-side
/// enrichment for rendering, and neither is consulted by authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageAuthor {
    pub principal_id: PrincipalId,
    pub identity_id: Option<ParticipantIdentityId>,
    /// `principals.display_label` -- what a reader sees when no handle is available.
    pub label: String,
    /// The handle the author used, when a transport named one.
    pub identity: Option<QualifiedIdentity>,
}

impl MessageAuthor {
    /// What to show as the author: the handle if there is one, otherwise the principal's label.
    pub fn display(&self) -> &str {
        match self.identity.as_ref() {
            Some(identity) => identity.subject().as_str(),
            None => &self.label,
        }
    }
}

/// One canonical message as it appears in one thread.
///
/// [`Message::id`] is the *association*: a message that landed in three channels' threads is one
/// canonical row and three of these. [`Message::canonical_id`] is the payload the three share.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// `thread_messages.id`. The identity the UI, the message cursor and
    /// `task_outreach_targets.response_association_id` name.
    pub id: Uuid,
    /// `messages.id`. The payload every thread association of this message shares.
    pub canonical_id: CanonicalMessageId,
    pub company_id: Uuid,
    pub thread_id: Uuid,
    pub author: MessageAuthor,
    pub subject: String,
    pub clean_text_body: String,
    pub attachments: Option<Vec<AttachmentMetadata>>,
    pub direction: MessageDirection,
    pub role: MessageRole,
    /// The chain this message belongs to. Inherited from the event that caused it, never minted
    /// here -- see [`CorrelationId`].
    pub correlation_id: CorrelationId,
    /// The sender/to/cc projection, for a transport that has one. Empty otherwise.
    #[serde(default)]
    pub participants: Vec<MessageParticipant>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Message {
    /// This message's position in its thread — what a reader resumes a live stream from.
    pub fn cursor(&self) -> MessageCursor {
        MessageCursor {
            created_at: self.created_at,
            id: self.id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_decode_only_from_their_own_version() {
        let stored = MessageAttachments::new(vec![AttachmentMetadata {
            filename: "report.pdf".into(),
            content_type: "application/pdf".into(),
            sha256_hash: "abc".into(),
            size_bytes: 12,
            storage_key: None,
        }]);
        let encoded = serde_json::to_value(&stored).unwrap();
        assert_eq!(encoded["version"], "1");
        assert_eq!(
            serde_json::from_value::<MessageAttachments>(encoded).unwrap(),
            stored
        );

        let future = serde_json::json!({ "version": "2", "items": [] });
        assert!(serde_json::from_value::<MessageAttachments>(future).is_err());
    }
}
