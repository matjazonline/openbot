use crate::entities::{
    transport::{ChannelSelector, ExternalDestination},
    value_objects::{ChannelSlug, CompanySlug, EmailAddress},
};
use crate::services::email_parser::RESERVED_CONTEXT_SUFFIXES;

/// Email-only delivery behavior encoded in an otherwise ordinary recipient address.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EmailDeliveryMode {
    #[default]
    Reply,
    ContextOnly,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmailDeliveryHints {
    pub mode: EmailDeliveryMode,
}

impl EmailDeliveryHints {
    pub const fn is_context_only(self) -> bool {
        matches!(self.mode, EmailDeliveryMode::ContextOnly)
    }
}

/// The ordered business-channel intents encoded by one platform email address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailChannelSelection {
    selectors: Vec<ChannelSelector>,
    delivery: EmailDeliveryHints,
}

impl EmailChannelSelection {
    pub fn primary(&self) -> &ChannelSelector {
        // Construction rejects an empty selector list.
        &self.selectors[0]
    }

    pub fn selectors(&self) -> &[ChannelSelector] {
        &self.selectors
    }

    pub fn into_selectors(self) -> Vec<ChannelSelector> {
        self.selectors
    }

    pub const fn delivery(&self) -> EmailDeliveryHints {
        self.delivery
    }
}

/// Classification performed before application routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailRecipientDestination {
    Channel(EmailChannelSelection),
    External(ExternalDestination),
    InvalidPlatformAddress,
}

/// Pure parser for the email adapter's channel-address syntax.
#[derive(Debug, Clone)]
pub struct EmailChannelSelectorParser {
    app_domain: String,
}

impl EmailChannelSelectorParser {
    pub fn new(app_domain: impl AsRef<str>) -> Self {
        Self {
            app_domain: app_domain.as_ref().trim().to_ascii_lowercase(),
        }
    }

    pub fn parse(&self, address: &str) -> Option<EmailChannelSelection> {
        let (company, channel_part) = self.parse_platform_address(address)?;
        let mut context_only = false;
        let mut slugs = Vec::new();

        for part in channel_part.split('+') {
            let (slug, has_context_hint) = strip_context_suffix(part);
            context_only |= has_context_hint;
            if !slug.is_empty() && !slugs.contains(&slug) {
                slugs.push(slug);
            }
        }
        if slugs.is_empty() {
            return None;
        }

        Some(EmailChannelSelection {
            selectors: slugs
                .into_iter()
                .map(|channel| ChannelSelector::Qualified {
                    company: company.clone(),
                    channel: ChannelSlug::new(channel),
                })
                .collect(),
            delivery: EmailDeliveryHints {
                mode: if context_only {
                    EmailDeliveryMode::ContextOnly
                } else {
                    EmailDeliveryMode::Reply
                },
            },
        })
    }

    pub fn classify(&self, address: EmailAddress) -> EmailRecipientDestination {
        if let Some(selection) = self.parse(address.as_str()) {
            return EmailRecipientDestination::Channel(selection);
        }

        let domain = address.as_str().rsplit_once('@').map(|(_, domain)| domain);
        if domain.is_some_and(|domain| self.is_platform_domain(domain)) {
            EmailRecipientDestination::InvalidPlatformAddress
        } else {
            EmailRecipientDestination::External(ExternalDestination::Email(address))
        }
    }

    pub fn is_platform_domain(&self, domain: &str) -> bool {
        let domain = domain.trim();
        domain.eq_ignore_ascii_case(&self.app_domain)
            || domain
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", self.app_domain))
    }

    pub fn parse_platform_address(&self, value: &str) -> Option<(CompanySlug, String)> {
        let address = angle_address(value);
        let cleaned = address.trim().to_ascii_lowercase();
        let (local, domain) = cleaned.split_once('@')?;
        if local.is_empty() || domain.is_empty() || domain.contains('@') {
            return None;
        }

        let company = domain.strip_suffix(&format!(".{}", self.app_domain))?;
        if company.is_empty() {
            return None;
        }
        Some((CompanySlug::new(company), local.to_string()))
    }
}

fn angle_address(value: &str) -> &str {
    match (value.find('<'), value.rfind('>')) {
        (Some(start), Some(end)) if start < end => &value[start + 1..end],
        _ => value,
    }
}

fn strip_context_suffix(raw: &str) -> (String, bool) {
    let lower = raw.trim().to_ascii_lowercase();
    for suffix in RESERVED_CONTEXT_SUFFIXES {
        for separator in ['.', '+', '-', '_'] {
            let marker = format!("{separator}{suffix}");
            if let Some(base) = lower.strip_suffix(&marker)
                && !base.trim().is_empty()
            {
                return (base.trim().to_string(), true);
            }
        }
        if lower == *suffix {
            return (String::new(), true);
        }
    }
    (lower, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_addresses_produce_qualified_selectors() {
        let parser = EmailChannelSelectorParser::new("mailagents.com");
        let parsed = parser
            .parse("Support Desk <SUPPORT+Billing@Acme.mailagents.com>")
            .unwrap();
        assert_eq!(
            parsed.selectors(),
            &[
                ChannelSelector::Qualified {
                    company: CompanySlug::new("acme"),
                    channel: ChannelSlug::new("support"),
                },
                ChannelSelector::Qualified {
                    company: CompanySlug::new("acme"),
                    channel: ChannelSlug::new("billing"),
                },
            ]
        );
        assert_eq!(parsed.delivery().mode, EmailDeliveryMode::Reply);
        assert!(parser.parse("person@example.com").is_none());
    }

    #[test]
    fn quiet_and_noagent_are_typed_delivery_hints() {
        let parser = EmailChannelSelectorParser::new("mailagents.com");
        for address in [
            "support.quiet@acme.mailagents.com",
            "support+noagent@acme.mailagents.com",
        ] {
            let parsed = parser.parse(address).unwrap();
            assert_eq!(parsed.delivery().mode, EmailDeliveryMode::ContextOnly);
            assert_eq!(parsed.primary().channel(), "support");
        }
    }

    #[test]
    fn external_recipients_are_explicit_destinations() {
        let parser = EmailChannelSelectorParser::new("mailagents.com");
        assert!(matches!(
            parser.classify(EmailAddress::from("person@example.com")),
            EmailRecipientDestination::External(ExternalDestination::Email(_))
        ));
        assert_eq!(
            parser.classify(EmailAddress::from("person@mailagents.com")),
            EmailRecipientDestination::InvalidPlatformAddress
        );
    }
}
