use super::*;
use crate::entities::{
    access_level::{LevelActor, LevelRefusal, LevelTarget, may_assign_level},
    company::CompanyAccess,
};

pub(crate) fn level_error(error: LevelRefusal) -> AppError {
    match error {
        LevelRefusal::OwnerFixed => AppError::BadRequest(error.to_string()),
        _ => AppError::Forbidden(error.to_string()),
    }
}
pub(crate) fn level_actor(access: &CompanyAccess) -> LevelActor {
    LevelActor {
        membership: access.membership,
        access_level: access.access_level,
    }
}
impl CompanyInviteUseCases {
    pub(crate) async fn level_manager(
        &self,
        actor: Uuid,
        company: Uuid,
    ) -> AppResult<CompanyAccess> {
        let access = readable_company(self.company_persistence.as_ref(), actor, company).await?;
        if !level_actor(&access).manages_levels() {
            return Err(level_error(LevelRefusal::NotAdministrator));
        }
        Ok(access)
    }

    #[instrument(skip_all)]
    pub async fn get_company_team_member(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        member_user_id: Uuid,
    ) -> AppResult<Option<CompanyMember>> {
        readable_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        self.invite_persistence
            .get_member_by_user_id(company_id, member_user_id)
            .await
    }

    #[instrument(skip_all)]
    pub async fn set_member_access_level(
        &self,
        change: MemberLevelChange,
    ) -> AppResult<CompanyMember> {
        let access = self
            .level_manager(change.actor_id, change.company_id)
            .await?;
        let target = if change.member_user_id == access.company.user_id {
            LevelTarget::Owner
        } else {
            let member = self
                .invite_persistence
                .get_member_by_user_id(change.company_id, change.member_user_id)
                .await?
                .ok_or_else(|| AppError::NotFound("Team member not found".into()))?;
            LevelTarget::Assignable(member.access_level)
        };
        may_assign_level(level_actor(&access), target, change.access_level).map_err(level_error)?;
        self.invite_persistence
            .set_member_access_level(change)
            .await?
            .ok_or_else(|| AppError::NotFound("Team member not found".into()))
    }

    #[instrument(skip_all)]
    pub async fn set_invite_access_level(
        &self,
        change: InviteLevelChange,
    ) -> AppResult<CompanyInvite> {
        let access = self
            .level_manager(change.actor_id, change.company_id)
            .await?;
        let invite = self
            .invite_persistence
            .get_invite_by_id(change.company_id, change.invite_id)
            .await?
            .ok_or_else(invite_not_found)?;
        if invite.status != "pending" {
            return Err(not_pending_error());
        }
        may_assign_level(
            level_actor(&access),
            LevelTarget::Assignable(invite.access_level),
            change.access_level,
        )
        .map_err(level_error)?;
        self.invite_persistence
            .set_invite_access_level(change)
            .await?
            .ok_or_else(not_pending_error)
    }
}
