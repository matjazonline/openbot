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
    use_cases::company::{CompanyPersistence, owned_company, readable_company},
};

#[path = "company_invite_commands.rs"]
mod commands;
pub use commands::*;
#[path = "company_level_use_cases.rs"]
mod levels;
pub(crate) use levels::{level_actor, level_error};

/// An invite belonging to another company is reported exactly like a missing one, so an id probe
/// cannot tell a foreign invite from a nonexistent one. See [`owned_company`].
fn invite_not_found() -> AppError {
    AppError::NotFound("Invite not found in this company.".into())
}

fn unverified_email_error() -> AppError {
    AppError::BadRequest("Verify your current email address before using invitations.".into())
}

fn not_pending_error() -> AppError {
    AppError::BadRequest("Only a pending invitation can be changed.".into())
}

/// The address an invite is written to: trimmed and lower-cased, and refused as a form error when
/// it is plainly not an address.
pub(crate) fn invite_address(raw: &str) -> AppResult<String> {
    let email = raw.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::BadRequest(
            "Please provide a valid email address.".into(),
        ));
    }
    Ok(email)
}

#[async_trait]
pub trait CompanyInvitePersistence: Send + Sync {
    async fn create_invite(&self, command: CreateCompanyInvite) -> AppResult<CompanyInvite>;
    async fn get_invite_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<Option<CompanyInvite>>;
    async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>>;
    /// Whether `email` is the address of this company's owner.
    async fn is_company_owner_address(&self, company_id: Uuid, email: &str) -> AppResult<bool>;
    /// Rewrites the invite only while it is still pending and belongs to `company_id`; `None`
    /// when it no longer is.
    async fn update_invite(&self, command: UpdateCompanyInvite)
    -> AppResult<Option<CompanyInvite>>;
    async fn get_member_by_user_id(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<CompanyMember>>;
    async fn set_member_access_level(
        &self,
        change: MemberLevelChange,
    ) -> AppResult<Option<CompanyMember>>;
    async fn set_invite_access_level(
        &self,
        change: InviteLevelChange,
    ) -> AppResult<Option<CompanyInvite>>;
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

    /// The owner already holds the company; an invite to their own address could only demote
    /// them, so it is refused as a form error rather than written.
    async fn refuse_owner_address(&self, company_id: Uuid, email: &str) -> AppResult<()> {
        if self
            .invite_persistence
            .is_company_owner_address(company_id, email)
            .await?
        {
            return Err(AppError::BadRequest(
                "The company owner cannot be invited to their own company.".into(),
            ));
        }
        Ok(())
    }

    #[instrument(skip_all)]
    pub async fn create_company_invite(
        &self,
        mut command: CreateCompanyInvite,
    ) -> AppResult<CompanyInvite> {
        let access = self
            .level_manager(command.actor_id, command.company_id)
            .await?;
        if !access.membership.is_owner() {
            return Err(AppError::Forbidden(
                "Only the company owner can do this.".into(),
            ));
        }
        crate::entities::access_level::may_assign_level(
            level_actor(&access),
            crate::entities::access_level::LevelTarget::Assignable(
                crate::entities::access_level::AccessLevel::PUBLIC,
            ),
            command.access_level,
        )
        .map_err(level_error)?;
        command.email = invite_address(&command.email)?.into();
        self.refuse_owner_address(command.company_id, &command.email)
            .await?;
        self.invite_persistence.create_invite(command).await
    }

    #[instrument(skip_all)]
    pub async fn list_company_invites(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyInvite>> {
        self.level_manager(user_id, company_id).await?;
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
        self.level_manager(user_id, company_id).await?;
        let invite = self
            .invite_persistence
            .get_invite_by_id(company_id, invite_id)
            .await?;
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
        mut command: UpdateCompanyInvite,
    ) -> AppResult<CompanyInvite> {
        let access = self
            .level_manager(command.actor_id, command.company_id)
            .await?;
        if !access.membership.is_owner() {
            return Err(AppError::Forbidden(
                "Only the company owner can do this.".into(),
            ));
        }
        let invite = self
            .invite_persistence
            .get_invite_by_id(command.company_id, command.invite_id)
            .await?
            .ok_or_else(invite_not_found)?;
        if invite.status != "pending" {
            return Err(not_pending_error());
        }
        let requested = command.access_level.unwrap_or(invite.access_level);
        crate::entities::access_level::may_assign_level(
            level_actor(&access),
            crate::entities::access_level::LevelTarget::Assignable(invite.access_level),
            requested,
        )
        .map_err(level_error)?;
        command.email = invite_address(&command.email)?.into();
        self.refuse_owner_address(command.company_id, &command.email)
            .await?;
        self.invite_persistence
            .update_invite(command)
            .await?
            .ok_or_else(not_pending_error)
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
            .get_invite_by_id(company_id, invite_id)
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
        // Every member may see who else is on the team; a stranger is told exactly what someone
        // asking about a nonexistent company is told.
        readable_company(self.company_persistence.as_ref(), user_id, company_id).await?;
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
            return Err(AppError::BadRequest(
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
            return Err(AppError::BadRequest(
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
