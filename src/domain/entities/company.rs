use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    company_member::CompanyMembership,
    memory::MemoryProviderKind,
    value_objects::{AvatarUrl, CompanySlug, EmailAddress, ModelName, ModelProvider},
};

/// Defaults copied into each newly provisioned personal agent channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompanyChannelDefaults {
    pub add_3rd_party: bool,
    pub participant_emails: Option<Vec<EmailAddress>>,
    pub retrieve_company_memory: bool,
    pub retrieve_agent_memory: bool,
    pub retrieve_user_memory: bool,
    pub persist_company_memory: bool,
    pub persist_agent_memory: bool,
    pub persist_user_memory: bool,
}

impl Default for CompanyChannelDefaults {
    fn default() -> Self {
        Self {
            add_3rd_party: true,
            participant_emails: None,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
        }
    }
}

/// Non-secret company model configuration suitable for settings and agent-selection views.
/// Credentials are deliberately loaded through the narrow persistence method only at execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompanyModelConnection {
    pub provider: ModelProvider,
    pub models: Vec<ModelName>,
    pub is_default: bool,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Company {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub slug: CompanySlug,
    pub enable_llm_spam_guardrail: Option<bool>,
    #[serde(default)]
    pub memory_provider: Option<MemoryProviderKind>,
    #[serde(default)]
    pub channel_defaults: CompanyChannelDefaults,
    /// The company's picture; `None` falls back to the letter bubble.
    pub avatar_url: Option<AvatarUrl>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A company a signed-in account can reach, and what they are to it.
///
/// The two travel together because every read guard needs both: the company scopes the query, and
/// the membership decides which of its channels the caller may actually see.
#[derive(Debug, Clone, PartialEq)]
pub struct CompanyAccess {
    pub company: Company,
    pub membership: CompanyMembership,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_defaults_match_database_defaults() {
        let defaults = CompanyChannelDefaults::default();
        assert!(defaults.add_3rd_party);
        assert!(defaults.participant_emails.is_none());
        assert!(!defaults.retrieve_company_memory);
        assert!(!defaults.retrieve_agent_memory);
        assert!(!defaults.retrieve_user_memory);
        assert!(!defaults.persist_company_memory);
        assert!(!defaults.persist_agent_memory);
        assert!(!defaults.persist_user_memory);
    }
}
