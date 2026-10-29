use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company_invite::CompanyInvite,
        company_member::{CompanyAccessRole, CompanyMember},
        user::{User, VerifiedEmail},
    },
    use_cases::company::{CompanyPersistence, company_not_found, owned_company},
};

/// An invite belonging to another company is reported exactly like a missing one, so an id probe
/// cannot tell a foreign invite from a nonexistent one. See [`owned_company`].
fn invite_not_found() -> AppError {
    AppError::NotFound("Invite not found in this company.".into())
}

fn unverified_email_error() -> AppError {
    AppError::BadRequest("Verify your current email address before using invitations.".into())
}

#[async_trait]
pub trait CompanyInvitePersistence: Send + Sync {
    async fn create_invite(
        &self,
        company_id: Uuid,
        email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite>;
    async fn get_invite_by_id(&self, id: Uuid) -> AppResult<Option<CompanyInvite>>;
    async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>>;
    async fn update_invite(
        &self,
        id: Uuid,
        new_email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite>;
    async fn delete_invite(&self, id: Uuid) -> AppResult<()>;
    async fn list_invites_for_user(
        &self,
        user_id: Uuid,
        expected_email: &str,
    ) -> AppResult<Vec<CompanyInvite>>;
    async fn accept_pending_invite(
        &self,
        invite_id: Uuid,
        user_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>>;
    async fn decline_pending_invite(
        &self,
        invite_id: Uuid,
        user_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>>;
    async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>>;
    async fn update_member_role(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        role: CompanyAccessRole,
    ) -> AppResult<Option<CompanyMember>>;
    async fn remove_member(&self, company_id: Uuid, user_id: Uuid) -> AppResult<()>;
}

#[derive(Clone)]
pub struct CompanyInviteUseCases {
    company_persistence: Arc<dyn CompanyPersistence>,
    invite_persistence: Arc<dyn CompanyInvitePersistence>,
}

impl CompanyInviteUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        invite_persistence: Arc<dyn CompanyInvitePersistence>,
    ) -> Self {
        Self {
            company_persistence,
            invite_persistence,
        }
    }

    async fn verify_company_owner(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        owned_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    #[instrument(skip_all)]
    pub async fn create_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite> {
        self.verify_company_owner(user_id, company_id).await?;

        let email_trimmed = email.trim().to_lowercase();
        if email_trimmed.is_empty() || !email_trimmed.contains('@') {
            return Err(AppError::Internal(
                "Please provide a valid email address.".into(),
            ));
        }

        info!(%company_id, "Creating invite");
        self.invite_persistence
            .create_invite(company_id, &email_trimmed, role)
            .await
    }

    #[instrument(skip_all)]
    pub async fn list_company_invites(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyInvite>> {
        self.verify_company_owner(user_id, company_id).await?;
        self.invite_persistence
            .list_invites_by_company(company_id)
            .await
    }

    #[instrument(skip_all)]
    pub async fn get_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        invite_id: Uuid,
    ) -> AppResult<Option<CompanyInvite>> {
        self.verify_company_owner(user_id, company_id).await?;
        let invite = self.invite_persistence.get_invite_by_id(invite_id).await?;
        if let Some(ref inv) = invite
            && inv.company_id != company_id
        {
            return Ok(None);
        }
        Ok(invite)
    }

    #[instrument(skip_all)]
    pub async fn update_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        invite_id: Uuid,
        new_email: &str,
        role: Option<CompanyAccessRole>,
    ) -> AppResult<CompanyInvite> {
        self.verify_company_owner(user_id, company_id).await?;

        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(invite_not_found)?;

        if invite.company_id != company_id {
            return Err(invite_not_found());
        }
        if invite.status != "pending" {
            return Err(AppError::BadRequest(
                "Only a pending invitation can be changed.".into(),
            ));
        }

        let email_trimmed = new_email.trim().to_lowercase();
        if email_trimmed.is_empty() || !email_trimmed.contains('@') {
            return Err(AppError::Internal(
                "Please provide a valid email address.".into(),
            ));
        }

        info!(%invite_id, "Updating invite email");
        self.invite_persistence
            .update_invite(invite_id, &email_trimmed, role.unwrap_or(invite.role))
            .await
    }

    #[instrument(skip_all)]
    pub async fn delete_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        invite_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_owner(user_id, company_id).await?;

        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(invite_not_found)?;

        if invite.company_id != company_id {
            return Err(invite_not_found());
        }

        info!("Deleting invite {} for company {}", invite_id, company_id);
        self.invite_persistence.delete_invite(invite_id).await
    }

    #[instrument(skip_all)]
    pub async fn list_user_invites(&self, user: &User) -> AppResult<Vec<CompanyInvite>> {
        VerifiedEmail::from_user(user).ok_or_else(unverified_email_error)?;
        self.invite_persistence
            .list_invites_for_user(user.id, &user.email)
            .await
    }

    /// Whether an account has an invitation that still needs an answer.
    #[instrument(skip_all)]
    pub async fn has_pending_user_invites(&self, user: &User) -> AppResult<bool> {
        if VerifiedEmail::from_user(user).is_none() {
            return Ok(false);
        }
        Ok(self
            .list_user_invites(user)
            .await?
            .iter()
            .any(|invite| invite.status == "pending"))
    }

    #[instrument(skip_all)]
    pub async fn accept_invite(&self, user: &User, invite_id: Uuid) -> AppResult<CompanyInvite> {
        VerifiedEmail::from_user(user).ok_or_else(unverified_email_error)?;
        info!("User {} accepting invite {}", user.id, invite_id);
        self.invite_persistence
            .accept_pending_invite(invite_id, user.id, &user.email)
            .await?
            .ok_or_else(invite_not_found)
    }

    #[instrument(skip_all)]
    pub async fn decline_invite(&self, user: &User, invite_id: Uuid) -> AppResult<CompanyInvite> {
        VerifiedEmail::from_user(user).ok_or_else(unverified_email_error)?;
        info!("User {} declining invite {}", user.id, invite_id);
        self.invite_persistence
            .decline_pending_invite(invite_id, user.id, &user.email)
            .await?
            .ok_or_else(invite_not_found)
    }

    #[instrument(skip_all)]
    pub async fn list_company_team_members(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyMember>> {
        // The one place a non-owner has legitimate access, so it cannot delegate to
        // `owned_company` outright — but a caller who is neither owner nor member is told exactly
        // what a stranger asking about a nonexistent company is told.
        match owned_company(self.company_persistence.as_ref(), user_id, company_id).await {
            Ok(_) => {}
            Err(AppError::NotFound(_)) => {
                let members = self
                    .invite_persistence
                    .list_members_by_company(company_id)
                    .await?;
                return if members.iter().any(|m| m.user_id == user_id) {
                    Ok(members)
                } else {
                    Err(company_not_found())
                };
            }
            Err(err) => return Err(err),
        }

        self.invite_persistence
            .list_members_by_company(company_id)
            .await
    }

    #[instrument(skip_all)]
    pub async fn remove_company_team_member(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        member_user_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_owner(user_id, company_id).await?;

        if user_id == member_user_id {
            return Err(AppError::Internal(
                "Cannot remove company owner from the team.".into(),
            ));
        }

        info!(
            "Removing user {} from company {} team",
            member_user_id, company_id
        );
        self.invite_persistence
            .remove_member(company_id, member_user_id)
            .await
    }

    #[instrument(skip_all)]
    pub async fn update_company_team_member_role(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        member_user_id: Uuid,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyMember> {
        self.verify_company_owner(user_id, company_id).await?;

        if user_id == member_user_id {
            return Err(AppError::Internal(
                "The company owner's access role cannot be changed.".into(),
            ));
        }

        info!(
            "Updating user {} to role {} in company {}",
            member_user_id, role, company_id
        );
        self.invite_persistence
            .update_member_role(company_id, member_user_id, role)
            .await?
            .ok_or_else(|| AppError::NotFound("Team member not found".into()))
    }
}

#[cfg(test)]
#[path = "company_invite_tests.rs"]
mod tests;
