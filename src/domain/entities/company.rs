use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{company_member::CompanyMembership, value_objects::CompanySlug};

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
