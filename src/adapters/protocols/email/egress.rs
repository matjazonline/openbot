use std::sync::Arc;
use async_trait::async_trait;

use crate::{
    adapters::protocols::ProtocolEgressAdapter,
    app_error::AppResult,
    entities::{
        channel::ChannelType,
        message_contract::NormalizedOutboundMessage,
    },
    infra::config::AppConfig,
    services::outbound_dispatcher::{OutboundDispatcher, OutboundEmail},
    use_cases::thread::BounceInfo,
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
            .map(|p| p.identity.clone())
            .unwrap_or_default();

        let recipients_cc = message
            .recipients_cc
            .iter()
            .map(|p| p.identity.clone())
            .collect();

        let trigger_message_id = message
            .in_reply_to_ref
            .clone()
            .unwrap_or_default();

        let outbound = OutboundEmail {
            workflow_id: message.channel_id,
            workflow_name: "Channel".to_string(),
            workflow_slug: "channel".to_string(),
            company_slug: "company".to_string(),
            trigger_message_id,
            thread_references: message.references.clone(),
            recipient_to,
            recipients_cc,
            subject: message.subject.clone(),
            body_text: message.content.clone(),
            hop_count: message.hop_count,
            trace_workflows: message.trace_channels.clone(),
        };

        OutboundDispatcher::send(&self.config, outbound).await?;
        Ok(())
    }

    async fn dispatch_bounce(&self, bounce_info: &BounceInfo) -> AppResult<()> {
        let mut bounce_body = format!(
            "Your email to company '{}' could not be delivered because the requested workflow address(es) were invalid:\n",
            bounce_info.company_slug.as_deref().unwrap_or("unknown")
        );

        for invalid in &bounce_info.invalid_slugs {
            bounce_body.push_str(&format!(" - Invalid: {}\n", invalid));
        }

        if !bounce_info.suggestions.is_empty() {
            bounce_body.push_str("\nDid you mean one of the following valid workflow addresses?\n");
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
