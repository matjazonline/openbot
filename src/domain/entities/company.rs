use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    company_member::CompanyMembership,
    memory::MemoryProviderKind,
    value_objects::{AvatarUrl, CompanySlug},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Company {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub slug: CompanySlug,
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enable_llm_spam_guardrail: Option<bool>,
    #[serde(default)]
    pub memory_provider: Option<MemoryProviderKind>,
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
