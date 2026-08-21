use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::entities::cursor::MessageCursor;
use crate::entities::value_objects::{EmailAddress, MessageId, ObjectKey, ThreadIndex};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
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
