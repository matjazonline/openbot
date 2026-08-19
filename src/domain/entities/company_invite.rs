use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CompanyInvite {
    pub id: Uuid,
    pub company_id: Uuid,
    pub company_name: Option<String>,
    pub email: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
