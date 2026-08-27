use crate::{
    adapters::storage::FileStorage,
    entities::{
        channel::{ChannelType, ParticipantIdentity},
        message_contract::NormalizedInboundMessage,
        value_objects::{MessageId, ThreadIndex},
    },
    infra::config::AppConfig,
    services::{
        attachment_store::store_inbound_attachments,
        email_parser::{EmailParser, RawInboundPayload},
    },
};

pub struct EmailIngressAdapter;

impl EmailIngressAdapter {
    /// Parse, having first put any attachments somewhere they can be fetched from again.
    ///
    /// The storing happens here rather than inside [`EmailParser::parse`] because this is the last
    /// point that holds the bytes *and* may await: everything past it carries metadata only.
    /// With no storage configured this is exactly [`EmailIngressAdapter::parse`].
    pub async fn parse_and_store(
        mut payload: RawInboundPayload,
        config: &AppConfig,
        storage: Option<&dyn FileStorage>,
    ) -> NormalizedInboundMessage {
        if let (Some(storage), Some(gcs)) = (storage, config.gcs.as_ref())
            && gcs.attachments_bucket.is_some()
            && !payload.attachments_data.is_empty()
        {
            store_inbound_attachments(
                storage,
                &gcs.attachments_folder,
                &mut payload.attachments_data,
            )
            .await;
        }

        Self::parse(payload, config)
    }

    pub fn parse(payload: RawInboundPayload, config: &AppConfig) -> NormalizedInboundMessage {
        let parsed = EmailParser::parse(payload, &config.app_domain_name);

        let sender = ParticipantIdentity::email(&parsed.sender);
        let recipients_to = parsed
            .recipients_to
            .iter()
            .map(ParticipantIdentity::email)
            .collect();
        let recipients_cc = parsed
            .recipients_cc
            .iter()
            .map(ParticipantIdentity::email)
            .collect();

        NormalizedInboundMessage {
            message_id: MessageId::from(parsed.message_id),
            thread_ref: parsed.in_reply_to.map(MessageId::from),
            references: parsed.references.into_iter().map(MessageId::from).collect(),
            thread_index: parsed.thread_index.map(ThreadIndex::from),
            sender,
            recipients_to,
            recipients_cc,
            subject: parsed.subject,
            clean_text: parsed.clean_text_body,
            raw_text: parsed.raw_text_body,
            raw_html: parsed.raw_html_body,
            attachments: parsed.attachments,
            is_auto_reply: parsed.is_auto_reply,
            is_forwarded: parsed.is_forwarded,
            channel_id_header: parsed.channel_id_header,
            hop_count: parsed.hop_count,
            trace_channels: parsed.trace_channels,
            protocol: ChannelType::Email,
            spf_status: parsed.spf_status,
            dkim_status: parsed.dkim_status,
            dmarc_status: parsed.dmarc_status,
            spam_score: parsed.spam_score,
            is_context_only: parsed.is_context_only,
        }
    }
}
