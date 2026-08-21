use serde::Serialize;
use uuid::Uuid;

use crate::entities::value_objects::{AvatarUrl, EmailAddress};

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

/// A signed-in account, as the read guards need it.
///
/// The id says which account, and the address is what a channel's participant list is written in
/// terms of -- so the two travel together rather than being looked up separately at each guard.
#[derive(Debug, Clone)]
pub struct Viewer {
    pub user_id: Uuid,
    pub email: EmailAddress,
}
