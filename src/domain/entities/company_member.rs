use serde::Serialize;
use uuid::Uuid;

use crate::entities::value_objects::AvatarUrl;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CompanyMember {
    pub id: Uuid,
    pub company_id: Uuid,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub email: Option<String>,
    /// The member's own profile picture, carried from their account so the team list can show it.
    pub avatar_url: Option<AvatarUrl>,
    pub role: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
