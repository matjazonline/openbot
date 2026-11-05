use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::{PostgresPersistence, participant::create_person_principal_on},
    app_error::{AppError, AppResult},
    entities::{
        company_invite::CompanyInvite,
        company_member::{CompanyAccessRole, CompanyMember},
        value_objects::AvatarUrl,
    },
    use_cases::company_invite::{
        CompanyInvitePersistence, CreateCompanyInvite, InviteLevelChange, MemberLevelChange,
        UpdateCompanyInvite, invite_address,
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
        let mut tx = authority::start(&self.pool, scope).await?;
        let previous =
            authority::lock_invite(&mut tx, scope.company_id, InviteTarget::Email(&email)).await?;
        let team = authority::lock_team(&mut tx, scope, None).await?;
        team.require_owner()?;
        let current = previous
            .as_ref()
            .map(|row| AccessLevel::try_from(row.access_level))
            .transpose()
            .map_err(|e| AppError::Internal(e.into()))?
            .unwrap_or(AccessLevel::PUBLIC);
        team.assign(LevelTarget::Assignable(current), command.access_level)?;
        authority::refuse_owner_address(&mut tx, team.owner_id, &email).await?;
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
        let mut tx = authority::start(&self.pool, scope).await?;
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
        team.require_owner()?;
        if previous.status != "pending" {
            return Ok(None);
        }
        let current = AccessLevel::try_from(previous.access_level)
            .map_err(|e| AppError::Internal(e.into()))?;
        let requested = command.access_level.unwrap_or(current);
        team.assign(LevelTarget::Assignable(current), requested)?;
        authority::refuse_owner_address(&mut tx, team.owner_id, &email).await?;
        let role = command
            .role
            .unwrap_or(CompanyAccessRole::from_str(&previous.role).map_err(AppError::Internal)?);
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
        let mut tx = authority::start(&self.pool, scope).await?;
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
        sqlx::query("UPDATE company_members SET access_level=$3::smallint WHERE company_id=$1 AND user_id=$2 AND role<>'owner'")
            .bind(scope.company_id).bind(change.member_user_id).bind(change.access_level.as_i16())
            .execute(&mut *tx).await.map_err(authority::database)?;
        let row = sqlx::query_as::<_, CompanyMemberDb>(authority::MEMBER_SELECT)
            .bind(scope.company_id)
            .bind(change.member_user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(authority::database)?;
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
        let mut tx = authority::start(&self.pool, scope).await?;
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

    async fn delete_invite(&self, id: Uuid) -> AppResult<()> {
        sqlx::query!("DELETE FROM company_invites WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(())
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
        let member_id = Uuid::new_v4();
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        if !lock_verified_account_email(&mut transaction, user_id, user_email).await? {
            return Ok(None);
        }
        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH accepted AS (
                UPDATE company_invites
                SET status = 'accepted'
                WHERE id = $1 AND status = 'pending' AND email = $2
                RETURNING id, company_id, email, role, access_level::smallint AS access_level, status, created_at
            ), membership AS (
                -- An invite never rewrites the owner's row: accepting one addressed to the owner
                -- leaves them the owner (the database also refuses an owner row that disagrees
                -- with companies.user_id). A data-modifying CTE runs whether or not it is read, so
                -- the invite still reads back accepted when this writes nothing.
                INSERT INTO company_members (id, company_id, user_id, role, access_level)
                SELECT $3, company_id, $4, role, access_level
                FROM accepted
                ON CONFLICT (company_id, user_id)
                DO UPDATE SET role = EXCLUDED.role WHERE company_members.role <> 'owner'
                RETURNING company_id
            )
            SELECT accepted.id, accepted.company_id, company.name AS company_name, accepted.email,
                   accepted.role, accepted.access_level::smallint AS access_level, accepted.status, accepted.created_at
            FROM accepted
            JOIN companies AS company ON company.id = accepted.company_id
            "#,
        )
        .bind(invite_id)
        .bind(user_email)
        .bind(member_id)
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        if let Some(invite) = &db {
            let username: String =
                sqlx::query_scalar("SELECT username::text FROM users WHERE id = $1")
                    .bind(user_id)
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(AppError::from)?;
            create_person_principal_on(
                &mut transaction,
                invite.company_id,
                user_id,
                &username,
                user_email,
            )
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
    /// The owner also holds a `company_members` row -- it is the relational fact a person
    /// principal's foreign key needs -- but `owner` is not an invitation role, so this surface
    /// and the two writes below leave that row alone.
    async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>> {
        let db_list = sqlx::query_as!(
            CompanyMemberDb,
            r#"SELECT member.id, member.company_id, member.user_id,
                      account.username AS "username?", account.email AS "email?", account.avatar_url,
                      member.role, member.access_level::smallint AS "access_level!: i16", member.created_at AS "created_at!"
               FROM company_members AS member
               JOIN users AS account ON account.id = member.user_id
               WHERE member.company_id = $1 AND member.role <> 'owner'
               ORDER BY member.created_at ASC, member.id ASC LIMIT 200"#,
            company_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn update_member_role(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        role: CompanyAccessRole,
    ) -> AppResult<Option<CompanyMember>> {
        let db = sqlx::query_as!(
            CompanyMemberDb,
            r#"WITH updated AS (
                   UPDATE company_members
                   SET role = $3
                   WHERE company_id = $1 AND user_id = $2 AND role <> 'owner'
                   RETURNING id, company_id, user_id, role, access_level, created_at
               )
               SELECT updated.id, updated.company_id, updated.user_id,
                      account.username AS "username?", account.email AS "email?", account.avatar_url,
                      updated.role, updated.access_level::smallint AS "access_level!: i16", updated.created_at AS "created_at!"
               FROM updated
               JOIN users AS account ON account.id = updated.user_id"#,
            company_id,
            user_id,
            role.as_str(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    /// Take somebody off the team without erasing what they were party to.
    ///
    /// Their principal is demoted to an external actor first. The member row is what a person
    /// principal's foreign key hangs on, so deleting it alone would cascade the principal away and
    /// take its identities, grants and thread participation with it -- a former colleague would
    /// vanish from every thread they ever wrote in.
    ///
    /// Lock order is principal, then member, then the member's notices -- the order the notice
    /// producers take them (the notification projection locks `principal, member` `FOR SHARE`; the
    /// decision-reviewer guard locks the principal). A producer already holding them commits its
    /// notice before this transaction proceeds, so the cancellation below sees it; one arriving
    /// later finds an external principal and no member, and composes nothing.
    async fn remove_member(&self, company_id: Uuid, user_id: Uuid) -> AppResult<()> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        sqlx::query!(
            r#"UPDATE principals
               SET kind = 'external', user_id = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND user_id = $2
                 AND EXISTS (
                     SELECT 1 FROM company_members AS member
                     WHERE member.company_id = $1 AND member.user_id = $2
                       AND member.role <> 'owner'
                 )"#,
            company_id,
            user_id
        )
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        let removable = sqlx::query_scalar!(
            "SELECT id FROM company_members
             WHERE company_id = $1 AND user_id = $2 AND role <> 'owner'
             FOR UPDATE",
            company_id,
            user_id
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        if removable.is_none() {
            // An owner, or nobody: removal is a no-op, and so was the demotion above.
            transaction.commit().await.map_err(AppError::from)?;
            return Ok(());
        }
        super::delivery::cancellation::cancel_member_notices_on(
            &mut transaction,
            company_id,
            user_id,
        )
        .await?;
        sqlx::query!(
            "DELETE FROM company_members
             WHERE company_id = $1 AND user_id = $2 AND role <> 'owner'",
            company_id,
            user_id
        )
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        transaction.commit().await.map_err(AppError::from)?;

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
