use serde::{Deserialize, Serialize};

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
