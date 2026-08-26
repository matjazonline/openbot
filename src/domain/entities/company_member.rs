use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::value_objects::AvatarUrl;

/// The access granted to somebody who joins a company through an invitation.
///
/// Keep this as one parsed value across HTTP, application and persistence boundaries: both
/// invitation and member writes accept it, and the database constrains the same two spellings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompanyAccessRole {
    #[default]
    Member,
    Admin,
}

impl CompanyAccessRole {
    pub const ALL: [Self; 2] = [Self::Member, Self::Admin];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Admin => "admin",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Member => "Member",
            Self::Admin => "Admin",
        }
    }
}

impl fmt::Display for CompanyAccessRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CompanyAccessRole {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "member" => Ok(Self::Member),
            "admin" => Ok(Self::Admin),
            _ => Err("Access role must be either member or admin.".into()),
        }
    }
}

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
    /// A `company_members` row whose access role is `admin`.
    Admin,
    /// A `company_members` row whose access role is `member`.
    Member,
    /// Neither: as far as this company is concerned, a stranger.
    None,
}

impl CompanyMembership {
    /// Whether this is somebody on the company's team at all.
    pub fn is_team(self) -> bool {
        matches!(self, Self::Owner | Self::Admin | Self::Member)
    }

    pub fn is_owner(self) -> bool {
        matches!(self, Self::Owner)
    }

    /// Whether this account may manage the company's operational workspaces and automation.
    pub fn manages_company_operations(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
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
    pub role: CompanyAccessRole,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
