use serde::Serialize;
use uuid::Uuid;

use crate::entities::value_objects::AvatarUrl;

/// What a signed-in account is to one company.
///
/// The read side of the app asks this question constantly -- may this person see this channel,
/// this thread, this document -- and a bare `bool` cannot answer it, because the owner and an
/// invited member are not the same authority: the owner sees a restricted channel they are not a
/// participant of, and a member does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CompanyMembership {
    /// The account named by `companies.user_id`.
    Owner,
    /// A `company_members` row.
    Member,
    /// Neither: as far as this company is concerned, a stranger.
    None,
}

impl CompanyMembership {
    /// Whether this is somebody on the company's team at all.
    pub fn is_team(self) -> bool {
        matches!(self, Self::Owner | Self::Member)
    }

    pub fn is_owner(self) -> bool {
        matches!(self, Self::Owner)
    }
}

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
