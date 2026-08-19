use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::value_objects::CompanySlug;

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
