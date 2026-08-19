use async_trait::async_trait;
use std::sync::Arc;

use crate::{
    adapters::protocols::ProtocolEgressAdapter,
    app_error::AppResult,
    entities::{channel::ChannelType, message_contract::NormalizedOutboundMessage},
    infra::config::AppConfig,
    services::outbound_dispatcher::{OutboundDispatcher, OutboundEmail},
    use_cases::thread::{BounceInfo, bounce_cause},
};

pub struct EmailEgressAdapter {
    config: Arc<AppConfig>,
}

impl EmailEgressAdapter {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self { config }
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

        OutboundDispatcher::send(&self.config, outbound).await?;
        Ok(())
    }

    async fn dispatch_bounce(&self, bounce_info: &BounceInfo) -> AppResult<()> {
        let mut bounce_body = format!(
            "Your email to company '{}' could not be delivered because {}:\n",
            bounce_info.company_slug.as_deref().unwrap_or("unknown"),
            bounce_cause(bounce_info)
        );

        for invalid in &bounce_info.invalid_slugs {
            bounce_body.push_str(&format!(" - Invalid: {}\n", invalid));
        }

        for disabled in &bounce_info.disabled_slugs {
            bounce_body.push_str(&format!(
                " - Disabled (address is correct, but the channel is switched off): {}\n",
                disabled
            ));
        }

        if !bounce_info.suggestions.is_empty() {
            bounce_body.push_str("\nDid you mean one of the following valid channel addresses?\n");
            for sug in &bounce_info.suggestions {
                if !sug.suggestions.is_empty() {
                    bounce_body.push_str(&format!(
                        " - For '{}', suggested: {}\n",
                        sug.invalid_slug,
                        sug.suggestions.join(", ")
                    ));
                }
            }
        }

        bounce_body.push_str("\nPlease check the recipient address and try again.");

        OutboundDispatcher::send_bounce(
            &self.config,
            &bounce_info.recipient_to,
            &bounce_info.original_subject,
            &bounce_body,
        )
        .await?;

        Ok(())
    }
}
