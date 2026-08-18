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
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<EmailAddress>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub channel_config: Option<serde_json::Value>,
    pub created_at: chrono::NaiveDateTime,
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

    /// The address inbound mail reaches this channel on: `{channel}@{company}.{app domain}`.
    ///
    /// The same address the simulator sends to and the mailbox composes to, so it lives here
    /// rather than being re-`format!`ed per page.
    pub fn inbound_address(
        &self,
        company_slug: &CompanySlug,
        app_domain_name: &str,
    ) -> EmailAddress {
        EmailAddress::new(format!("{}@{company_slug}.{app_domain_name}", self.slug))
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
