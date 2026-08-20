use serde::Serialize;
use uuid::Uuid;

use crate::entities::value_objects::AvatarUrl;

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    /// The profile picture shown wherever the account is; `None` renders as a letter bubble.
    pub avatar_url: Option<AvatarUrl>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
