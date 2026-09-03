use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    auth::AuthVerdict,
    correlation::CorrelationId,
    message::AttachmentMetadata,
    transport::{QualifiedIdentity, TransportKind},
    value_objects::{MessageId, ThreadIndex},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedInboundMessage {
    pub message_id: MessageId,
    pub thread_ref: Option<MessageId>,
    pub references: Vec<MessageId>,
    pub thread_index: Option<ThreadIndex>,
    pub sender: QualifiedIdentity,
    pub recipients_to: Vec<QualifiedIdentity>,
    pub recipients_cc: Vec<QualifiedIdentity>,
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
    /// The chain this message belongs to: adopted from its `X-MailAgents-Correlation-ID` header
    /// when it has one, minted at ingress when it does not.
    ///
    /// Deliberately not `#[serde(default)]`: this round-trips through the durable task payload,
    /// and a default would quietly mint a second chain for a message that already had one.
    pub correlation_id: CorrelationId,
    pub transport: TransportKind,
    #[serde(default)]
    pub spf_status: AuthVerdict,
    #[serde(default)]
    pub dkim_status: AuthVerdict,
    #[serde(default)]
    pub dmarc_status: AuthVerdict,
    pub spam_score: Option<f64>,
    pub is_context_only: bool,
}
