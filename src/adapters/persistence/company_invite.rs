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
    use_cases::company_invite::CompanyInvitePersistence,
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyInviteDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub company_name: Option<String>,
    pub email: String,
    pub role: String,
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
            created_at: db.created_at,
        })
    }
}

#[async_trait]
impl CompanyInvitePersistence for PostgresPersistence {
    async fn create_invite(
        &self,
        company_id: Uuid,
        email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH invite AS (
                INSERT INTO company_invites (id, company_id, email, role, status)
                VALUES ($1, $2, $3, $4, 'pending')
                ON CONFLICT (company_id, email)
                DO UPDATE SET role = EXCLUDED.role, status = 'pending', created_at = CURRENT_TIMESTAMP
                RETURNING id, company_id, email, role, status, created_at
            )
            SELECT invite.id, invite.company_id, company.name AS company_name, invite.email,
                   invite.role, invite.status, invite.created_at
            FROM invite
            JOIN companies AS company ON company.id = invite.company_id
            "#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(email)
        .bind(role.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn get_invite_by_id(&self, id: Uuid) -> AppResult<Option<CompanyInvite>> {
        let db = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT invite.id, invite.company_id, company.name AS "company_name?", invite.email,
                      invite.role, invite.status, invite.created_at AS "created_at!"
               FROM company_invites AS invite
               JOIN companies AS company ON company.id = invite.company_id
               WHERE invite.id = $1"#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>> {
        let db_list = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT invite.id, invite.company_id, company.name AS "company_name?", invite.email,
                      invite.role, invite.status, invite.created_at AS "created_at!"
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

    async fn update_invite(
        &self,
        id: Uuid,
        new_email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite> {
        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH invite AS (
                UPDATE company_invites
                SET email = $1, role = $2
                WHERE id = $3
                RETURNING id, company_id, email, role, status, created_at
            )
            SELECT invite.id, invite.company_id, company.name AS company_name, invite.email,
                   invite.role, invite.status, invite.created_at
            FROM invite
            JOIN companies AS company ON company.id = invite.company_id
            "#,
        )
        .bind(new_email)
        .bind(role.as_str())
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into)
            .transpose()?
            .ok_or_else(|| AppError::Internal("Invite not found.".into()))
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
                      invite.role, invite.status, invite.created_at AS "created_at!"
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
                RETURNING id, company_id, email, role, status, created_at
            ), membership AS (
                INSERT INTO company_members (id, company_id, user_id, role)
                SELECT $3, company_id, $4, role
                FROM accepted
                ON CONFLICT (company_id, user_id)
                DO UPDATE SET role = EXCLUDED.role
                RETURNING company_id
            )
            SELECT accepted.id, accepted.company_id, company.name AS company_name, accepted.email,
                   accepted.role, accepted.status, accepted.created_at
            FROM accepted
            JOIN membership ON membership.company_id = accepted.company_id
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
                          invite.role, invite.status, invite.created_at
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
                RETURNING id, company_id, email, role, status, created_at
            )
            SELECT declined.id, declined.company_id, company.name AS company_name, declined.email,
                   declined.role, declined.status, declined.created_at
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
                          invite.role, invite.status, invite.created_at
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
                      member.role, member.created_at AS "created_at!"
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
                   RETURNING id, company_id, user_id, role, created_at
               )
               SELECT updated.id, updated.company_id, updated.user_id,
                      account.username AS "username?", account.email AS "email?", account.avatar_url,
                      updated.role, updated.created_at AS "created_at!"
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
