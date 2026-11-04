//! Persisted protocol metadata shared by message producers and readers.
use crate::entities::{
    email_message::EmailMessageMetadata,
    transport::{ChannelBindingId, ExternalEventKey},
};
use serde::{Deserialize, Serialize};
/// The provider-specific facts that survive alongside the canonical message.
///
/// Versioned and discriminated for the same reason [`crate::entities::message::MessageAttachments`]
/// is: it is persisted JSON read back by a process that may be older or newer than the one that
/// wrote it, so an unknown shape must fail to decode rather than be guessed at.
///
/// [`ProtocolExtension::StoredEvent`] is the escape hatch for providers whose payload is large or
/// sensitive: the bounded raw event is already durable in its own row at ingress, so this names it
/// instead of copying it into anything a task or a queue would carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProtocolExtension {
    Email {
        version: u8,
        metadata: EmailMessageMetadata,
    },
    StoredEvent {
        version: u8,
        binding_id: ChannelBindingId,
        event_key: ExternalEventKey,
    },
    /// Nothing the transport added: a schedule's prompt, an approval note.
    None { version: u8 },
}

impl ProtocolExtension {
    pub const fn email(metadata: EmailMessageMetadata) -> Self {
        Self::Email {
            version: 1,
            metadata,
        }
    }

    pub const fn stored_event(binding_id: ChannelBindingId, event_key: ExternalEventKey) -> Self {
        Self::StoredEvent {
            version: 1,
            binding_id,
            event_key,
        }
    }

    pub const fn none() -> Self {
        Self::None { version: 1 }
    }

    pub const fn email_metadata(&self) -> Option<&EmailMessageMetadata> {
        match self {
            Self::Email { metadata, .. } => Some(metadata),
            Self::StoredEvent { .. } | Self::None { .. } => None,
        }
    }
}
