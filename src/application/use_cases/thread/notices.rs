//! Bounce/help notices describe the incoming delivery or directory, not a stored classified reply.
//! Bounce/help notices and their durable dispatch.
use super::*;
use serde::{Deserialize, Serialize};
impl ThreadUseCases {
    /// Tell a sender their message could not be routed.
    ///
    /// The HTTP request task that calls this method is supervised; this method makes the work
    /// durable before that task returns. The generic delivery worker owns every provider attempt,
    /// including retry and ambiguous-outcome handling.
    pub async fn handle_bounce_dispatch(&self, ingest: &InboundIngestResult) -> AppResult<()> {
        let Some(bounce) = ingest.bounce_info() else {
            return Ok(());
        };
        let body = format_bounce_email_body(bounce, &self.settings.mail_domain);
        let subject = if bounce
            .original_subject
            .to_ascii_lowercase()
            .starts_with("[undeliverable]")
        {
            bounce.original_subject.clone()
        } else {
            format!("[Undeliverable] {}", bounce.original_subject)
        };
        let content = CanonicalContent::parse(subject, body)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let serialized = serde_json::to_vec(bounce).map_err(|error| {
            AppError::Internal(format!("Could not key a rejection bounce: {error}"))
        })?;
        let source_key = format!("bounce:{:x}", Sha256::digest(serialized));
        let delivery = self
            .deliveries
            .compose_standalone(StandaloneDeliveryRequest {
                company_id: bounce.company_id,
                correlation_id: CorrelationId::new(),
                purpose: crate::entities::transport::DeliveryPurpose::Notification,
                source_key,
                content: &content,
                context: crate::transport::DeliveryContext::Email(
                    crate::transport::EmailDeliveryContext {
                        from: EmailAddress::from(format!(
                            "mailer-daemon@{}",
                            self.settings.mail_domain
                        )),
                        from_name: Some("Mail Agents Server".to_string()),
                        recipient_to: bounce.recipient_to.clone(),
                        recipients_cc: Vec::new(),
                        threading: crate::transport::EmailThreading::Standalone,
                        relay: None,
                    },
                ),
            })?;
        self.standalone_deliveries
            .enqueue_standalone_delivery(delivery)
            .await?;
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BounceSuggestion {
    pub invalid_slug: ChannelSlug,
    pub suggestions: Vec<ChannelSlug>,
}

/// One channel the bouncing sender could have written to instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelDirectoryEntry {
    pub address: EmailAddress,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BounceInfo {
    /// The provider message this notification answers. Included in the delivery key so a webhook
    /// redelivery is absorbed without merging two distinct rejected messages with the same
    /// subject.
    pub source_message_key: ExternalMessageKey,
    pub recipient_to: EmailAddress,
    /// The company whose address was mailed. A bounce is only ever earned once an address resolved
    /// to a company, and the queued notice belongs to that company.
    pub company_id: Uuid,
    pub company_slug: CompanySlug,
    pub invalid_slugs: Vec<ChannelSlug>,
    /// Channels that exist but are switched off. Defaulted on deserialize so bounces queued
    /// before this field existed still re-hydrate.
    #[serde(default)]
    pub disabled_slugs: Vec<ChannelSlug>,
    pub suggestions: Vec<BounceSuggestion>,
    /// The channels this sender may write to, listed only when they are on the company's team.
    ///
    /// Empty for everyone else, so a stranger who guesses at an address never learns the company's
    /// channel directory from the bounce it earns. Defaulted on deserialize for the same reason as
    /// `disabled_slugs`.
    #[serde(default)]
    pub available_channels: Vec<ChannelDirectoryEntry>,
    pub original_subject: String,
}

pub fn format_bounce_email_body(bounce: &BounceInfo, app_domain: &str) -> String {
    let domain_part = format!("{}.{app_domain}", bounce.company_slug);

    let mut body = String::new();
    body.push_str("Undeliverable Mail Notification\n\n");
    body.push_str(&format!(
        "Your email could not be delivered because {}.\n\n",
        bounce_cause(bounce)
    ));

    body.push_str(&format!("Company Domain: {domain_part}\n\n"));

    body.push_str("Details:\n");
    for s in &bounce.suggestions {
        body.push_str(&format!(
            "  - Invalid Channel Slug: '{}@{}'\n",
            s.invalid_slug, domain_part
        ));
        if !s.suggestions.is_empty() {
            body.push_str("    Did you mean:\n");
            for sug in &s.suggestions {
                body.push_str(&format!("      * {sug}@{domain_part}\n"));
            }
        } else {
            body.push_str("    No similar channel suggestions found.\n");
        }
        body.push('\n');
    }

    for slug in &bounce.disabled_slugs {
        body.push_str(&format!(
            "  - Disabled Channel: '{slug}@{domain_part}'\n    This address is correct, but the \
             channel is currently switched off and is not accepting mail.\n\n"
        ));
    }

    push_channel_directory(&mut body, &bounce.available_channels);

    body.push_str("Please check the channel address and try sending your email again.\n");
    body
}

/// The "here is what you could have written to" section, appended only for a sender the platform
/// recognizes as one of the company's own people.
///
/// Nothing at all is rendered for an empty list, so a stranger's bounce is byte-for-byte what it
/// was before this section existed.
fn push_channel_directory(body: &mut String, entries: &[ChannelDirectoryEntry]) {
    if entries.is_empty() {
        return;
    }

    body.push_str("Channels you can write to:\n");
    for entry in entries {
        body.push_str(&format!("  - {} \u{2014} {}\n", entry.address, entry.name));
        if let Some(description) = entry.description.as_deref().map(str::trim)
            && !description.is_empty()
        {
            body.push_str(&format!("      {description}\n"));
        }
    }
    body.push('\n');
}

/// The reply `_help@{company}.{app_domain}` sends back.
///
/// `entries` is whatever [`ThreadUseCases::writable_channel_directory`] allowed this sender, so the
/// team-only rule is decided there rather than restated here; an empty list still gets the syntax
/// section, which discloses nothing about the company.
///
/// Every example below is checked against the real parsers: the `+` split in
/// `parse_recipient_address_pipeline`, the separators in `strip_context_suffix_from_slug`, and the
/// bracket forms in `EmailParser::check_body_context_trigger`.
pub fn format_help_email_body(
    entries: &[ChannelDirectoryEntry],
    company_slug: &CompanySlug,
    app_domain: &str,
) -> String {
    let domain_part = format!("{company_slug}.{app_domain}");
    let example = entries
        .first()
        .map(|entry| {
            entry
                .address
                .split('@')
                .next()
                .unwrap_or("support")
                .to_string()
        })
        .unwrap_or_else(|| "support".to_string());

    let mut body = String::from("Mail Agents Help\n\n");
    push_channel_directory(&mut body, entries);
    if entries.is_empty() {
        body.push_str(
            "You are not currently a participant of any channel in this company, so there is \
             nothing to list. Ask a colleague to add you.\n\n",
        );
    }

    body.push_str("Addressing tricks:\n");
    body.push_str(&format!(
        "  {example}+billing@{domain_part}\n      Send to both channels, in that order.\n"
    ));
    body.push_str(&format!(
        "  {example}+quiet@{domain_part}\n      File it on the thread without running the agent.\n"
    ));
    body.push_str(
        "  [[quiet]]\n      As the first thing in the body, does the same as +quiet.\n\n",
    );
    body.push_str(&format!(
        "  {} all mean \"file it, don't answer\", and attach to a\n  channel name with '+', '.', \
         '-' or '_'.\n\n",
        crate::entities::channel::RESERVED_SLUG_SUFFIXES.join(", ")
    ));

    body.push_str("Reply to this address at any time to see this again.\n");
    body
}

/// The one sentence that says why a bounce happened, for both bounce renderers.
pub fn bounce_cause(bounce: &BounceInfo) -> &'static str {
    match (
        !bounce.invalid_slugs.is_empty(),
        !bounce.disabled_slugs.is_empty(),
    ) {
        (true, true) => {
            "one or more channel addresses were not found or misspelled, and one or more are disabled"
        }
        (true, false) => "one or more channel addresses were not found or misspelled",
        (false, true) => "one or more channels are currently disabled",
        (false, false) => "the message could not be routed",
    }
}
