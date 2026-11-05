//! Short team mutations serialize against the account, invitation, and current membership facts.
use super::*;
use crate::entities::access_level::{AccessLevel, LevelActor, LevelTarget, may_assign_level};
use crate::use_cases::company_invite::level_error;
use sqlx::{PgPool, Postgres, Transaction};

#[derive(Clone, Copy)]
pub(in crate::adapters::persistence) struct TeamScope {
    pub(in crate::adapters::persistence) actor_id: Uuid,
    pub(in crate::adapters::persistence) company_id: Uuid,
}
#[derive(sqlx::FromRow)]
pub(super) struct LockedMember {
    pub user_id: Uuid,
    role: String,
    access_level: i16,
}
impl LockedMember {
    pub(super) fn actor(&self) -> AppResult<LevelActor> {
        let membership = super::super::participant::membership_from_db(Some(&self.role))?;
        let access_level = AccessLevel::try_from(self.access_level).map_err(|error| {
            AppError::Internal(format!(
                "Invalid stored team level {}: {error}",
                self.user_id
            ))
        })?;
        if membership.is_owner() && access_level != AccessLevel::MAX {
            return Err(AppError::Internal(format!(
                "Invalid stored owner level {}",
                self.user_id
            )));
        }
        Ok(LevelActor {
            membership,
            access_level,
        })
    }
}
pub(in crate::adapters::persistence) struct TeamAuthority {
    pub(in crate::adapters::persistence) owner_id: Uuid,
    pub(in crate::adapters::persistence) actor: LevelActor,
    members: Vec<LockedMember>,
}
impl TeamAuthority {
    pub(super) fn member(&self, user_id: Uuid) -> Option<&LockedMember> {
        self.members.iter().find(|member| member.user_id == user_id)
    }
    pub(super) fn require_owner(&self) -> AppResult<()> {
        if !self.actor.membership.is_owner() {
            return Err(AppError::Forbidden(
                "Only the company owner may change invitations.".into(),
            ));
        }
        Ok(())
    }
    pub(super) fn assign(&self, current: LevelTarget, requested: AccessLevel) -> AppResult<()> {
        may_assign_level(self.actor, current, requested).map_err(level_error)
    }
}
pub(in crate::adapters::persistence) fn database(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .and_then(|e| e.code())
        .as_deref()
        .is_some_and(|code| matches!(code, "55P03" | "57014" | "40P01"))
    {
        return AppError::Conflict("Team access is busy or changed; retry the request.".into());
    }
    AppError::from(error)
}
pub(in crate::adapters::persistence) async fn start(
    pool: &PgPool,
    scope: TeamScope,
) -> AppResult<Transaction<'_, Postgres>> {
    let mut transaction = pool.begin().await.map_err(database)?;
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','5000ms',true)")
        .execute(&mut *transaction).await.map_err(database)?;
    let actor: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(scope.actor_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database)?;
    if actor.is_none() {
        return Err(AppError::NotFound("Company not found".into()));
    }
    Ok(transaction)
}
pub(in crate::adapters::persistence) async fn lock_team(
    tx: &mut Transaction<'_, Postgres>,
    scope: TeamScope,
    target: Option<Uuid>,
) -> AppResult<TeamAuthority> {
    let owner_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM companies WHERE id=$1 FOR NO KEY UPDATE",
    )
    .bind(scope.company_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database)?
    .ok_or_else(|| AppError::NotFound("Company not found".into()))?;
    let mut users = vec![scope.actor_id];
    if let Some(target) = target {
        users.push(target);
    }
    let members = sqlx::query_as::<_, LockedMember>(
        "SELECT user_id,role,access_level::smallint AS access_level FROM company_members WHERE company_id=$1 AND user_id=ANY($2) ORDER BY user_id FOR UPDATE")
        .bind(scope.company_id).bind(users).fetch_all(&mut **tx).await.map_err(database)?;
    let actor = members
        .iter()
        .find(|member| member.user_id == scope.actor_id)
        .ok_or_else(|| AppError::NotFound("Company not found".into()))?
        .actor()?;
    Ok(TeamAuthority {
        owner_id,
        actor,
        members,
    })
}
const INVITE_SELECT: &str = "SELECT invite.id,invite.company_id,company.name AS company_name,
    invite.email::text AS email,invite.role,invite.access_level::smallint AS access_level,
    invite.status,invite.created_at FROM company_invites AS invite
    JOIN companies AS company ON company.id=invite.company_id WHERE invite.company_id=$1";
pub(super) enum InviteTarget<'a> {
    Id(Uuid),
    Email(&'a str),
}
pub(super) async fn lock_invite(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    target: InviteTarget<'_>,
) -> AppResult<Option<CompanyInviteDb>> {
    let mut query = sqlx::QueryBuilder::<Postgres>::new(INVITE_SELECT);
    // The first placeholder belongs to the shared SELECT; target values are always bound.
    let mut args = sqlx::postgres::PgArguments::default();
    use sqlx::Arguments;
    args.add(company)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    match target {
        InviteTarget::Id(id) => {
            query.push(" AND invite.id=$2");
            args.add(id)
                .map_err(|e| AppError::Internal(e.to_string()))?;
        }
        InviteTarget::Email(email) => {
            query.push(" AND invite.email=$2::citext");
            args.add(email)
                .map_err(|e| AppError::Internal(e.to_string()))?;
        }
    }
    query.push(" FOR UPDATE OF invite");
    sqlx::query_as_with::<_, CompanyInviteDb, _>(query.sql(), args)
        .fetch_optional(&mut **tx)
        .await
        .map_err(database)
}
pub(super) async fn read_invite(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    id: Uuid,
) -> AppResult<Option<CompanyInviteDb>> {
    sqlx::query_as(&format!("{INVITE_SELECT} AND invite.id=$2"))
        .bind(company)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(database)
}
pub(super) async fn refuse_owner_address(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    email: &str,
) -> AppResult<()> {
    let own: bool = sqlx::query_scalar("SELECT email=$2::citext FROM users WHERE id=$1")
        .bind(owner)
        .bind(email)
        .fetch_one(&mut **tx)
        .await
        .map_err(database)?;
    if own {
        return Err(AppError::BadRequest(
            "The company owner cannot be invited to their own company.".into(),
        ));
    }
    Ok(())
}
pub(super) const MEMBER_SELECT: &str = "SELECT member.id,member.company_id,member.user_id,
    account.username::text AS username,account.email::text AS email,account.avatar_url,
    member.role,member.access_level::smallint AS access_level,member.created_at
    FROM company_members AS member JOIN users AS account ON account.id=member.user_id
    WHERE member.company_id=$1 AND member.user_id=$2 AND member.role<>'owner'";
