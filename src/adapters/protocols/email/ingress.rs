use crate::{
    adapters::storage::FileStorage,
    entities::{
        channel::{ChannelType, ParticipantIdentity},
        correlation::{CORRELATION_HEADER, CorrelationId},
        message_contract::NormalizedInboundMessage,
        value_objects::MessageId,
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
        // Read before `payload` is consumed. Extracted here rather than in `EmailParser` because
        // this is the protocol boundary the id crosses, and because `parse_headers` already
        // returns a nine-tuple that a tenth element would not improve.
        let correlation_id = correlation_from_headers(payload.headers.as_deref());
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
            correlation_id,
            protocol: ChannelType::Email,
            spf_status: parsed.spf_status,
            dkim_status: parsed.dkim_status,
            dmarc_status: parsed.dmarc_status,
            spam_score: parsed.spam_score,
            is_context_only: parsed.is_context_only,
        }
    }
}

/// The chain this message already belongs to, or a fresh one.
///
/// A missing or malformed header is not an error: an external sender has no reason to set one, so
/// the absence simply means this message starts its own chain.
fn correlation_from_headers(headers: Option<&str>) -> CorrelationId {
    let prefix = format!("{}:", CORRELATION_HEADER.to_lowercase());
    let supplied = headers.and_then(|headers| {
        headers
            .lines()
            .map(str::trim)
            .find(|line| line.to_lowercase().starts_with(&prefix))
            .map(|line| line[prefix.len()..].trim().to_string())
    });
    CorrelationId::parse_or_new(supplied.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inter_channel_reply_stays_on_the_chain_it_came_from() {
        let id = CorrelationId::new();
        let headers = format!(
            "Message-ID: <a@example.com>\nX-MailAgents-Correlation-ID: {id}\nSubject: Re: hi"
        );
        assert_eq!(correlation_from_headers(Some(&headers)), id);
    }

    #[test]
    fn header_matching_ignores_case_the_way_rfc_5322_does() {
        let id = CorrelationId::new();
        let headers = format!("x-mailagents-correlation-id:  {id}  ");
        assert_eq!(correlation_from_headers(Some(&headers)), id);
    }

    #[test]
    fn a_message_from_outside_starts_its_own_chain() {
        // No header at all, and a header we cannot read, both mean "mint one" rather than "fail".
        assert_ne!(
            correlation_from_headers(None),
            correlation_from_headers(None)
        );
        let garbage = "X-MailAgents-Correlation-ID: not-a-uuid";
        assert_ne!(
            correlation_from_headers(Some(garbage)),
            correlation_from_headers(Some(garbage))
        );
    }
}
