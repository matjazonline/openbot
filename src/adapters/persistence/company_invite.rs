use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        company_invite::CompanyInvite,
        company_member::{CompanyAccessRole, CompanyMember},
        value_objects::AvatarUrl,
    },
    use_cases::company_invite::{
        CompanyInvitePersistence, CreateCompanyInvite, DeleteCompanyInvite, InviteLevelChange,
        MemberLevelChange, MemberRoleChange, RemoveCompanyMember, UpdateCompanyInvite,
        invite_address,
    },
};

#[path = "company_level_authority.rs"]
pub(super) mod authority;
use authority::{InviteTarget, TeamScope};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyInviteDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub company_name: Option<String>,
    pub email: String,
    pub role: String,
    pub access_level: i16,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<CompanyInviteDb> for CompanyInvite {
    type Error = AppError;

    fn try_from(db: CompanyInviteDb) -> Result<Self, Self::Error> {
        Ok(CompanyInvite {
            id: db.id,
            company_id: db.company_id,
            company_name: db.company_name,
            email: db.email,
            role: CompanyAccessRole::from_str(&db.role).map_err(AppError::Internal)?,
            access_level: crate::entities::access_level::AccessLevel::try_from(db.access_level)
                .map_err(|error| {
                    AppError::Internal(format!(
                        "Invalid stored company access level {}: {error}",
                        db.id
                    ))
                })?,
            status: db.status,
            created_at: db.created_at,
        })
    }
}

/// Serialize invitation decisions with address/proof writes on the same account row.
async fn lock_verified_account_email(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    expected_email: &str,
) -> AppResult<bool> {
    let account: Option<(String, bool)> = sqlx::query_as(
        "SELECT email::text, email_verified_at IS NOT NULL FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(AppError::from)?;
    Ok(account
        .is_some_and(|(email, verified)| verified && email.eq_ignore_ascii_case(expected_email)))
}

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyMemberDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub role: String,
    pub access_level: i16,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<CompanyMemberDb> for CompanyMember {
    type Error = AppError;

    fn try_from(db: CompanyMemberDb) -> Result<Self, Self::Error> {
        Ok(CompanyMember {
            id: db.id,
            company_id: db.company_id,
            user_id: db.user_id,
            username: db.username,
            email: db.email,
            avatar_url: db.avatar_url.map(AvatarUrl::from),
            role: CompanyAccessRole::from_str(&db.role).map_err(AppError::Internal)?,
            access_level: crate::entities::access_level::AccessLevel::try_from(db.access_level)
                .map_err(|error| {
                    AppError::Internal(format!(
                        "Invalid stored company access level {}: {error}",
                        db.id
                    ))
                })?,
            created_at: db.created_at,
        })
    }
}

#[async_trait]
impl CompanyInvitePersistence for PostgresPersistence {
    async fn create_invite(&self, command: CreateCompanyInvite) -> AppResult<CompanyInvite> {
        use crate::entities::access_level::{AccessLevel, LevelTarget};
        let email = invite_address(&command.email)?;
        let scope = TeamScope {
            actor_id: command.actor_id,
            company_id: command.company_id,
        };
        let mut tx = authority::start(&self.pool, scope, None).await?;
        authority::lock_invite(&mut tx, scope.company_id, InviteTarget::Email(&email)).await?;
        let team = authority::lock_team(&mut tx, scope, None).await?;
        // Creation upserts by address, so it may also rewrite an existing invitation. Authorize
        // against the row as committed under the company lock: one created while this writer
        // waited is invisible to the earlier row lock.
        let previous = authority::read_invite_by_email(&mut tx, scope.company_id, &email).await?;
        let previous_role = previous
            .as_ref()
            .map(|row| CompanyAccessRole::from_str(&row.role).map_err(AppError::Internal))
            .transpose()?;
        team.require_inviter(previous_role.into_iter().chain([command.role]))?;
        let current = previous
            .as_ref()
            .map(|row| AccessLevel::try_from(row.access_level))
            .transpose()
            .map_err(|e| AppError::Internal(e.into()))?
            .unwrap_or(AccessLevel::PUBLIC);
        team.assign(LevelTarget::Assignable(current), command.access_level)?;
        authority::refuse_owner_address(&mut tx, team.owner_id, &email).await?;
        authority::refuse_admin_address(&mut tx, &team, scope.company_id, &email).await?;
        let id: Uuid = sqlx::query_scalar("INSERT INTO company_invites(id,company_id,email,role,access_level,status)
            VALUES($1,$2,$3,$4,$5::smallint,'pending') ON CONFLICT(company_id,email)
            DO UPDATE SET role=EXCLUDED.role,access_level=EXCLUDED.access_level,status='pending',created_at=CURRENT_TIMESTAMP RETURNING id")
            .bind(Uuid::new_v4()).bind(scope.company_id).bind(&email).bind(command.role.as_str())
            .bind(command.access_level.as_i16()).fetch_one(&mut *tx).await.map_err(authority::database)?;
        let row = authority::read_invite(&mut tx, scope.company_id, id)
            .await?
            .ok_or_else(|| AppError::Internal("Created invitation disappeared".into()))?;
        tx.commit().await.map_err(authority::database)?;
        row.try_into()
    }

    async fn get_invite_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<Option<CompanyInvite>> {
        let db = sqlx::query_as::<_, CompanyInviteDb>("SELECT invite.id,invite.company_id,company.name AS company_name,
            invite.email::text AS email,invite.role,invite.access_level::smallint AS access_level,invite.status,invite.created_at
            FROM company_invites AS invite JOIN companies AS company ON company.id=invite.company_id
            WHERE invite.company_id=$1 AND invite.id=$2")
            .bind(company_id).bind(id).fetch_optional(&self.pool).await?;
        db.map(TryInto::try_into).transpose()
    }

    async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>> {
        let db_list = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT invite.id, invite.company_id, company.name AS "company_name?", invite.email,
                      invite.role, invite.access_level::smallint AS "access_level!: i16", invite.status, invite.created_at AS "created_at!"
               FROM company_invites AS invite
               JOIN companies AS company ON company.id = invite.company_id
               WHERE invite.company_id = $1
               ORDER BY invite.created_at DESC, invite.id DESC LIMIT 200"#,
            company_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn is_company_owner_address(&self, company_id: Uuid, email: &str) -> AppResult<bool> {
        let owned = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM companies AS company
                   JOIN users AS account ON account.id = company.user_id
                   WHERE company.id = $1 AND account.email = $2::text::citext
               ) AS "owned!""#,
            company_id,
            email
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(owned)
    }

    /// Rewrites a pending invite of this company. The status and company are matched in the
    /// `UPDATE` itself, so an invite accepted after the caller read it is left as it was accepted.
    async fn update_invite(
        &self,
        command: UpdateCompanyInvite,
    ) -> AppResult<Option<CompanyInvite>> {
        use crate::entities::access_level::{AccessLevel, LevelTarget};
        let email = invite_address(&command.email)?;
        let scope = TeamScope {
            actor_id: command.actor_id,
            company_id: command.company_id,
        };
        let mut tx = authority::start(&self.pool, scope, None).await?;
        let Some(previous) = authority::lock_invite(
            &mut tx,
            scope.company_id,
            InviteTarget::Id(command.invite_id),
        )
        .await?
        else {
            return Ok(None);
        };
        let team = authority::lock_team(&mut tx, scope, None).await?;
        let previous_role =
            CompanyAccessRole::from_str(&previous.role).map_err(AppError::Internal)?;
        let role = command.role.unwrap_or(previous_role);
        if previous.status != "pending" {
            return Ok(None);
        }
        team.require_inviter([previous_role, role])?;
        let current = AccessLevel::try_from(previous.access_level)
            .map_err(|e| AppError::Internal(e.into()))?;
        let requested = command.access_level.unwrap_or(current);
        team.assign(LevelTarget::Assignable(current), requested)?;
        authority::refuse_owner_address(&mut tx, team.owner_id, &email).await?;
        authority::refuse_admin_address(&mut tx, &team, scope.company_id, &email).await?;
        sqlx::query("UPDATE company_invites SET email=$3,role=$4,access_level=$5::smallint WHERE company_id=$1 AND id=$2 AND status='pending'")
            .bind(scope.company_id).bind(command.invite_id).bind(email).bind(role.as_str()).bind(requested.as_i16())
            .execute(&mut *tx).await.map_err(authority::database)?;
        let row = authority::read_invite(&mut tx, scope.company_id, command.invite_id).await?;
        tx.commit().await.map_err(authority::database)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn get_member_by_user_id(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<CompanyMember>> {
        sqlx::query_as::<_, CompanyMemberDb>(authority::MEMBER_SELECT)
            .bind(company_id)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn set_member_access_level(
        &self,
        change: MemberLevelChange,
    ) -> AppResult<Option<CompanyMember>> {
        use crate::entities::access_level::LevelTarget;
        let scope = TeamScope {
            actor_id: change.actor_id,
            company_id: change.company_id,
        };
        let mut tx = authority::start(&self.pool, scope, Some(change.member_user_id)).await?;
        let team = authority::lock_team(&mut tx, scope, Some(change.member_user_id)).await?;
        let Some(target) = team.member(change.member_user_id) else {
            return Ok(None);
        };
        let current = if change.member_user_id == team.owner_id {
            LevelTarget::Owner
        } else {
            LevelTarget::Assignable(target.actor()?.access_level)
        };
        team.assign(current, change.access_level)?;
        sqlx::query("UPDATE department_members AS member SET access_level=$3::smallint WHERE member.company_id=$1 AND member.root_user_id=$2 AND NOT EXISTS (SELECT 1 FROM companies AS company WHERE company.id=$1 AND company.user_id=$2)")
            .bind(scope.company_id).bind(change.member_user_id).bind(change.access_level.as_i16())
            .execute(&mut *tx).await.map_err(authority::database)?;
        let row = sqlx::query_as::<_, CompanyMemberDb>(authority::MEMBER_SELECT)
            .bind(scope.company_id)
            .bind(change.member_user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(authority::database)?;
        Box::pin(super::workflow::refresh_member_dependents_on(
            &mut tx,
            crate::domain::workflow::CompanyId::new(scope.company_id),
        ))
        .await?;
        tx.commit().await.map_err(authority::database)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn set_invite_access_level(
        &self,
        change: InviteLevelChange,
    ) -> AppResult<Option<CompanyInvite>> {
        use crate::entities::access_level::{AccessLevel, LevelTarget};
        let scope = TeamScope {
            actor_id: change.actor_id,
            company_id: change.company_id,
        };
        let mut tx = authority::start(&self.pool, scope, None).await?;
        let Some(previous) = authority::lock_invite(
            &mut tx,
            scope.company_id,
            InviteTarget::Id(change.invite_id),
        )
        .await?
        else {
            return Ok(None);
        };
        let team = authority::lock_team(&mut tx, scope, None).await?;
        if previous.status != "pending" {
            return Ok(None);
        }
        team.require_inviter([
            CompanyAccessRole::from_str(&previous.role).map_err(AppError::Internal)?
        ])?;
        let current = AccessLevel::try_from(previous.access_level)
            .map_err(|e| AppError::Internal(e.into()))?;
        team.assign(LevelTarget::Assignable(current), change.access_level)?;
        sqlx::query("UPDATE company_invites SET access_level=$3::smallint WHERE company_id=$1 AND id=$2 AND status='pending'")
            .bind(scope.company_id).bind(change.invite_id).bind(change.access_level.as_i16())
            .execute(&mut *tx).await.map_err(authority::database)?;
        let row = authority::read_invite(&mut tx, scope.company_id, change.invite_id).await?;
        tx.commit().await.map_err(authority::database)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn delete_invite(&self, command: DeleteCompanyInvite) -> AppResult<()> {
        let scope = TeamScope {
            actor_id: command.actor_id,
            company_id: command.company_id,
        };
        let mut tx = authority::start(&self.pool, scope, None).await?;
        let invite = authority::lock_invite(
            &mut tx,
            scope.company_id,
            InviteTarget::Id(command.invite_id),
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Invite not found".into()))?;
        let team = authority::lock_team(&mut tx, scope, None).await?;
        team.require_inviter([
            CompanyAccessRole::from_str(&invite.role).map_err(AppError::Internal)?
        ])?;
        sqlx::query("DELETE FROM company_invites WHERE company_id=$1 AND id=$2")
            .bind(scope.company_id)
            .bind(command.invite_id)
            .execute(&mut *tx)
            .await
            .map_err(authority::database)?;
        tx.commit().await.map_err(authority::database)
    }

    async fn list_invites_for_user(
        &self,
        user_id: Uuid,
        expected_email: &str,
    ) -> AppResult<Vec<CompanyInvite>> {
        let db_list = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT invite.id, invite.company_id, company.name AS "company_name?", invite.email,
                      invite.role, invite.access_level::smallint AS "access_level!: i16", invite.status, invite.created_at AS "created_at!"
               FROM company_invites AS invite
               JOIN companies AS company ON company.id = invite.company_id
               JOIN users AS account ON account.id = $1
               WHERE invite.email = account.email AND account.email = $2
                 AND account.email_verified_at IS NOT NULL
               ORDER BY invite.created_at DESC, invite.id DESC LIMIT 200"#,
            user_id,
            expected_email
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn accept_pending_invite(
        &self,
        invite_id: Uuid,
        user_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        if !lock_verified_account_email(&mut transaction, user_id, user_email).await? {
            return Ok(None);
        }
        let company_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT company_id FROM company_invites WHERE id = $1 AND email = $2",
        )
        .bind(invite_id)
        .bind(user_email)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(company_id) = company_id else {
            return Ok(None);
        };
        sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
            .bind(company_id)
            .fetch_optional(&mut *transaction)
            .await?;
        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"WITH accepted AS (
                UPDATE company_invites SET status = 'accepted'
                WHERE id = $1 AND status = 'pending' AND email = $2
                RETURNING id, company_id, email, role, access_level, status, created_at
            ) SELECT accepted.id, accepted.company_id, company.name AS company_name,
                     accepted.email, accepted.role, accepted.access_level::smallint AS access_level,
                     accepted.status, accepted.created_at
              FROM accepted JOIN companies AS company ON company.id = accepted.company_id"#,
        )
        .bind(invite_id)
        .bind(user_email)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(invite) = &db {
            let joined = super::company::join_root_on(
                &mut transaction,
                super::company::RootJoin {
                    company: crate::domain::workflow::CompanyId::new(invite.company_id),
                    user: user_id,
                    role: CompanyAccessRole::from_str(&invite.role).map_err(AppError::Internal)?,
                    access_level: crate::entities::access_level::AccessLevel::try_from(
                        invite.access_level,
                    )
                    .map_err(|error| AppError::Internal(error.to_string()))?,
                },
            )
            .await?;
            super::department_desks::sync_department_grants_on(
                &mut transaction,
                crate::domain::workflow::CompanyId::new(invite.company_id),
                &[joined.department],
            )
            .await?;
            Box::pin(super::workflow::refresh_member_dependents_on(
                &mut transaction,
                crate::domain::workflow::CompanyId::new(invite.company_id),
            ))
            .await?;
        }
        let result = if db.is_some() {
            db
        } else {
            sqlx::query_as::<_, CompanyInviteDb>(
                r#"SELECT invite.id, invite.company_id, company.name AS company_name, invite.email,
                          invite.role, invite.access_level::smallint AS access_level, invite.status, invite.created_at
                   FROM company_invites AS invite
                   JOIN companies AS company ON company.id = invite.company_id
                   WHERE invite.id = $1 AND invite.email = $2 AND invite.status = 'accepted'"#,
            )
            .bind(invite_id)
            .bind(user_email)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(AppError::from)?
        };
        transaction.commit().await.map_err(AppError::from)?;

        result.map(TryInto::try_into).transpose()
    }

    async fn decline_pending_invite(
        &self,
        invite_id: Uuid,
        user_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        if !lock_verified_account_email(&mut transaction, user_id, user_email).await? {
            return Ok(None);
        }
        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH declined AS (
                UPDATE company_invites
                SET status = 'declined'
                WHERE id = $1 AND status = 'pending' AND email = $2
                RETURNING id, company_id, email, role, access_level::smallint AS access_level, status, created_at
            )
            SELECT declined.id, declined.company_id, company.name AS company_name, declined.email,
                   declined.role, declined.access_level::smallint AS access_level, declined.status, declined.created_at
            FROM declined
            JOIN companies AS company ON company.id = declined.company_id
            "#,
        )
        .bind(invite_id)
        .bind(user_email)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        let result = if db.is_some() {
            db
        } else {
            sqlx::query_as::<_, CompanyInviteDb>(
                r#"SELECT invite.id, invite.company_id, company.name AS company_name, invite.email,
                          invite.role, invite.access_level::smallint AS access_level, invite.status, invite.created_at
                   FROM company_invites AS invite
                   JOIN companies AS company ON company.id = invite.company_id
                   WHERE invite.id = $1 AND invite.email = $2 AND invite.status = 'declined'"#,
            )
            .bind(invite_id)
            .bind(user_email)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(AppError::from)?
        };
        transaction.commit().await.map_err(AppError::from)?;
        result.map(TryInto::try_into).transpose()
    }

    /// The people invited into this company.
    ///
    /// Team rows are current root person memberships; ownership stays on companies.
    async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>> {
        let rows = sqlx::query_as::<_, CompanyMemberDb>(
            "SELECT member.id, member.company_id, member.user_id, account.username::text AS username, \
                    account.email::text AS email, account.avatar_url, member.role, \
                    member.access_level::smallint AS access_level, member.created_at \
             FROM principals AS principal \
             JOIN LATERAL membership_of(principal.user_id, principal.company_id) AS member ON true \
             JOIN users AS account ON account.id = principal.user_id \
             WHERE principal.company_id = $1 AND member.role <> 'owner' \
             ORDER BY member.created_at, member.id LIMIT 200",
        ).bind(company_id).fetch_all(&self.pool).await?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn update_member_role(
        &self,
        change: MemberRoleChange,
    ) -> AppResult<Option<CompanyMember>> {
        let scope = TeamScope {
            actor_id: change.actor_id,
            company_id: change.company_id,
        };
        let mut tx = authority::start(&self.pool, scope, Some(change.member_user_id)).await?;
        let team = authority::lock_team(&mut tx, scope, Some(change.member_user_id)).await?;
        team.require_owner()?;
        if change.member_user_id == team.owner_id {
            return Err(AppError::BadRequest(
                "The company owner's access role cannot be changed.".into(),
            ));
        }
        let company = crate::domain::workflow::CompanyId::new(change.company_id);
        let Some(member) =
            super::company::root_member_on(&mut tx, company, change.member_user_id).await?
        else {
            return Ok(None);
        };
        super::company::set_admin_on(&mut tx, company, &member, change.role).await?;
        Box::pin(super::workflow::refresh_member_dependents_on(
            &mut tx, company,
        ))
        .await?;
        let row = sqlx::query_as::<_, CompanyMemberDb>(authority::MEMBER_SELECT)
            .bind(change.company_id)
            .bind(change.member_user_id)
            .fetch_optional(&mut *tx)
            .await?;
        tx.commit().await.map_err(authority::database)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn remove_member(&self, command: RemoveCompanyMember) -> AppResult<()> {
        let scope = TeamScope {
            actor_id: command.actor_id,
            company_id: command.company_id,
        };
        let mut tx = authority::start(&self.pool, scope, Some(command.member_user_id)).await?;
        let team = authority::lock_team(&mut tx, scope, Some(command.member_user_id)).await?;
        team.require_owner()?;
        if command.member_user_id == team.owner_id {
            return Err(AppError::BadRequest(
                "Cannot remove company owner from the team.".into(),
            ));
        }
        super::company::retire_member_on(
            &mut tx,
            crate::domain::workflow::CompanyId::new(command.company_id),
            command.member_user_id,
        )
        .await?;
        tx.commit().await.map_err(authority::database)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "company_invite_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "company_owner_tests.rs"]
mod owner_tests;

#[cfg(test)]
#[path = "company_access_level_tests.rs"]
mod access_level_tests;

#[cfg(test)]
#[path = "delegated_invite_tests.rs"]
mod delegated_invite_tests;
