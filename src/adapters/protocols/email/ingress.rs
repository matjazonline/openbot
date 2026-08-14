use crate::{
    entities::{
        channel::{ChannelType, ParticipantIdentity},
        message_contract::NormalizedInboundMessage,
    },
    infra::config::AppConfig,
    services::email_parser::{EmailParser, RawInboundPayload},
};

pub struct EmailIngressAdapter;

impl EmailIngressAdapter {
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
            message_id: parsed.message_id,
            thread_ref: parsed.in_reply_to,
            references: parsed.references,
            thread_index: parsed.thread_index,
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
