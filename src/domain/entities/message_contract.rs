use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    channel::{ChannelType, ParticipantIdentity},
    message::AttachmentMetadata,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedInboundMessage {
    pub message_id: String,
    pub thread_ref: Option<String>,
    pub references: Vec<String>,
    pub thread_index: Option<String>,
    pub sender: ParticipantIdentity,
    pub recipients_to: Vec<ParticipantIdentity>,
    pub recipients_cc: Vec<ParticipantIdentity>,
    pub subject: String,
    pub clean_text: String,
    pub raw_text: Option<String>,
    pub raw_html: Option<String>,
    pub attachments: Vec<AttachmentMetadata>,
    pub is_auto_reply: bool,
    pub is_forwarded: bool,
    pub channel_id_header: Option<Uuid>,
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
    pub protocol: ChannelType,
    pub spf_status: Option<String>,
    pub dkim_status: Option<String>,
    pub dmarc_status: Option<String>,
    pub spam_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedOutboundMessage {
    pub thread_id: Uuid,
    pub in_reply_to_ref: Option<String>,
    pub references: Vec<String>,
    pub recipients_to: Vec<ParticipantIdentity>,
    pub recipients_cc: Vec<ParticipantIdentity>,
    pub subject: String,
    pub content: String,
    pub attachments: Vec<AttachmentMetadata>,
    pub protocol: ChannelType,
    pub channel_id: Uuid,
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
}
