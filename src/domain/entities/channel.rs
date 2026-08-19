use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::value_objects::{ChannelSlug, CompanySlug, EmailAddress};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Email,
    WebChat,
    Slack,
    WhatsApp,
    Api,
}

impl ChannelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelType::Email => "email",
            ChannelType::WebChat => "webchat",
            ChannelType::Slack => "slack",
            ChannelType::WhatsApp => "whatsapp",
            ChannelType::Api => "api",
        }
    }
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ParticipantIdentity {
    pub identity: String,
    pub protocol: ChannelType,
}

impl ParticipantIdentity {
    pub fn new(identity: impl Into<String>, protocol: ChannelType) -> Self {
        Self {
            identity: identity.into().trim().to_lowercase(),
            protocol,
        }
    }

    pub fn email(addr: impl Into<String>) -> Self {
        Self::new(addr, ChannelType::Email)
    }

    pub fn matches(&self, other_raw: &str) -> bool {
        let clean = other_raw.trim().to_lowercase();
        self.identity == clean
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Channel {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub slug: ChannelSlug,
    /// Extra local parts this channel also answers on, canonical [`Channel::slug`] excluded.
    ///
    /// Defaulted on deserialize for the same reason as `enabled`: durable `background_tasks`
    /// payloads written before aliases existed must still re-hydrate.
    #[serde(default)]
    pub alias_slugs: Vec<ChannelSlug>,
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<EmailAddress>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub channel_config: Option<serde_json::Value>,
    /// Whether the channel takes traffic at all. A disabled channel keeps its threads and tasks
    /// but bounces inbound mail and cannot be an internal delivery target.
    ///
    /// Defaulted on deserialize because `Channel` is stored inside durable `background_tasks`
    /// payloads: tasks queued before this field existed must still re-hydrate.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

fn enabled_by_default() -> bool {
    true
}

/// Participant list entry that opens a channel to any sender.
pub const PUBLIC_PARTICIPANT: &str = "@public";

/// What a channel's participant list says about one sender.
///
/// `authorized` decides whether the message may enter the channel at all; `trusted` additionally
/// waives spam scoring and permits pulling third-party recipients into the thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantAccess {
    pub authorized: bool,
    pub trusted: bool,
}

impl Channel {
    /// The single definition of "may this sender use this channel".
    ///
    /// An empty or absent participant list means "company team members only". A list containing
    /// `@public` opens the channel to anyone, but only explicitly listed senders (or team members)
    /// are *trusted*.
    pub fn participant_access(&self, sender: &str, is_team_member: bool) -> ParticipantAccess {
        match &self.participant_emails {
            Some(allowed) if !allowed.is_empty() => {
                let is_public = allowed
                    .iter()
                    .any(|e| e.trim().eq_ignore_ascii_case(PUBLIC_PARTICIPANT));
                let explicitly_listed = allowed.iter().any(|e| {
                    !e.trim().eq_ignore_ascii_case(PUBLIC_PARTICIPANT)
                        && e.eq_ignore_ascii_case(sender)
                });
                ParticipantAccess {
                    authorized: is_public || explicitly_listed,
                    trusted: explicitly_listed || is_team_member,
                }
            }
            _ => ParticipantAccess {
                authorized: is_team_member,
                trusted: is_team_member,
            },
        }
    }

    /// Who should be asked to approve an action on this channel: the first listed participant
    /// that names an actual person rather than the `@public` wildcard.
    pub fn preferred_approver(&self) -> Option<EmailAddress> {
        self.participant_emails
            .as_ref()?
            .iter()
            .find(|email| !email.eq_ignore_ascii_case(PUBLIC_PARTICIPANT))
            .cloned()
    }

    /// Every local part this channel answers on: the canonical slug first, then its aliases.
    pub fn slugs(&self) -> impl Iterator<Item = &ChannelSlug> {
        std::iter::once(&self.slug).chain(self.alias_slugs.iter())
    }

    /// The single definition of "does this addressed slug reach this channel".
    pub fn matches_slug(&self, slug: &str) -> bool {
        self.slugs().any(|own| own.eq_ignore_ascii_case(slug))
    }

    /// The address inbound mail reaches this channel on: `{channel}@{company}.{app domain}`.
    ///
    /// The same address the simulator sends to and the mailbox composes to, so it lives here
    /// rather than being re-`format!`ed per page.
    pub fn inbound_address(
        &self,
        company_slug: &CompanySlug,
        app_domain_name: &str,
    ) -> EmailAddress {
        Self::address_for(&self.slug, company_slug, app_domain_name)
    }

    /// Every address this channel is reachable at, canonical first.
    pub fn inbound_addresses(
        &self,
        company_slug: &CompanySlug,
        app_domain_name: &str,
    ) -> Vec<EmailAddress> {
        self.slugs()
            .map(|slug| Self::address_for(slug, company_slug, app_domain_name))
            .collect()
    }

    /// The address form, for a slug that may be an alias rather than [`Channel::slug`].
    pub fn address_for(
        channel_slug: &ChannelSlug,
        company_slug: &CompanySlug,
        app_domain_name: &str,
    ) -> EmailAddress {
        EmailAddress::new(format!("{channel_slug}@{company_slug}.{app_domain_name}"))
    }

    pub fn default_config() -> serde_json::Value {
        serde_json::json!({
            "name": "MinimalAgent",
            "system_prompt": "You are a helpful assistant.",
            "llm": {
              "provider": "google",
              "model": "gemini-2.5-flash",
              "api_key": null
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_with_aliases(aliases: &[&str]) -> Channel {
        Channel {
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            name: "Support".to_string(),
            slug: "support".into(),
            alias_slugs: aliases.iter().map(|a| ChannelSlug::from(*a)).collect(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            enabled: true,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn matches_slug_accepts_canonical_and_alias_case_insensitively() {
        let channel = channel_with_aliases(&["sales", "help"]);

        assert!(channel.matches_slug("support"));
        assert!(channel.matches_slug("SALES"));
        assert!(channel.matches_slug("Help"));
        assert!(!channel.matches_slug("billing"));
    }

    #[test]
    fn addresses_list_the_canonical_slug_first() {
        let channel = channel_with_aliases(&["sales"]);
        let addresses = channel.inbound_addresses(&CompanySlug::from("acme"), "mailagents.com");

        assert_eq!(
            addresses,
            vec![
                EmailAddress::from("support@acme.mailagents.com"),
                EmailAddress::from("sales@acme.mailagents.com"),
            ]
        );
    }

    #[test]
    fn a_channel_without_aliases_answers_on_one_address_only() {
        let channel = channel_with_aliases(&[]);

        assert!(channel.matches_slug("support"));
        assert!(!channel.matches_slug("sales"));
        assert_eq!(
            channel.inbound_addresses(&CompanySlug::from("acme"), "mailagents.com"),
            vec![channel.inbound_address(&CompanySlug::from("acme"), "mailagents.com")]
        );
    }
}
