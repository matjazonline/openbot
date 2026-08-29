use async_trait::async_trait;
use std::sync::Arc;

use crate::{
    adapters::protocols::ProtocolEgressAdapter,
    app_error::AppResult,
    entities::{channel::ChannelType, message_contract::NormalizedOutboundMessage},
    services::outbound_dispatcher::{OutboundDispatcher, OutboundEmail},
    use_cases::thread::{BounceInfo, format_bounce_email_body},
};

pub struct EmailEgressAdapter {
    dispatcher: Arc<OutboundDispatcher>,
}

impl EmailEgressAdapter {
    pub fn new(dispatcher: Arc<OutboundDispatcher>) -> Self {
        Self { dispatcher }
    }
}

#[async_trait]
impl ProtocolEgressAdapter for EmailEgressAdapter {
    fn protocol(&self) -> ChannelType {
        ChannelType::Email
    }

    async fn dispatch(&self, message: &NormalizedOutboundMessage) -> AppResult<()> {
        let recipient_to = message
            .recipients_to
            .first()
            .map(|p| p.identity.clone().into())
            .unwrap_or_default();

        let recipients_cc = message
            .recipients_cc
            .iter()
            .map(|p| p.identity.clone().into())
            .collect();

        let trigger_message_id = message.in_reply_to_ref.clone().unwrap_or_default();

        let outbound = OutboundEmail {
            channel_id: message.channel_id,
            channel_name: "Channel".to_string(),
            channel_slug: "channel".into(),
            company_slug: "company".into(),
            trigger_message_id,
            thread_references: message.references.clone(),
            recipient_to,
            recipients_cc,
            subject: message.subject.clone(),
            body_text: message.content.clone(),
            hop_count: message.hop_count,
            trace_channels: message.trace_channels.clone(),
        };

        self.dispatcher.send(outbound).await?;
        Ok(())
    }

    async fn dispatch_bounce(&self, bounce_info: &BounceInfo) -> AppResult<()> {
        self.dispatcher
            .send_bounce(
                &bounce_info.recipient_to,
                &bounce_info.original_subject,
                &format_bounce_email_body(bounce_info, self.dispatcher.app_domain_name()),
            )
            .await?;

        Ok(())
    }
}
