use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CompanyMember {
    pub id: Uuid,
    pub company_id: Uuid,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub email: Option<String>,
    pub role: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
