use serde::Serialize;
use uuid::Uuid;

use crate::entities::value_objects::{AvatarUrl, EmailAddress};

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: Uuid,
    /// Fences authentication completed before a committed account security change.
    #[serde(skip_serializing)]
    pub auth_revision: i64,
    pub username: String,
    pub email: String,
    /// Proof of control of the current address; a signed session is not this proof.
    pub email_verified_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
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
    pub email_verified_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Who is making a request, as the use cases that care about deployment-wide authority see it.
///
/// Operator status is decided once, where the request is authenticated, from the account and the
/// deployment's operator list; a use case that needs it checks this value rather than deciding
/// again. Nothing here says anything about a company -- that is `CompanyAccess`'s job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    pub user_id: Uuid,
    operator: bool,
}

impl Caller {
    /// An ordinary signed-in account.
    pub const fn user(user_id: Uuid) -> Self {
        Self {
            user_id,
            operator: false,
        }
    }

    /// A deployment operator. Only the authentication boundary decides this.
    pub const fn operator(user_id: Uuid) -> Self {
        Self {
            user_id,
            operator: true,
        }
    }

    pub const fn is_operator(&self) -> bool {
        self.operator
    }
}

/// An address proven for the account's current stored email. Construct only from a freshly loaded
/// account or viewer; a signed session supplies an id, never this proof.
#[derive(Debug, Clone)]
pub struct VerifiedEmail(EmailAddress);

impl VerifiedEmail {
    pub fn from_user(user: &User) -> Option<Self> {
        user.email_verified_at
            .map(|_| Self(EmailAddress::from(user.email.as_str())))
    }

    pub fn from_viewer(viewer: &Viewer) -> Option<Self> {
        viewer.email_verified_at.map(|_| Self(viewer.email.clone()))
    }

    pub fn as_address(&self) -> &EmailAddress {
        &self.0
    }
}
