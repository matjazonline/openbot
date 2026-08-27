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

    async fn list_invites_by_email(&self, email: &str) -> AppResult<Vec<CompanyInvite>> {
        let db_list = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT invite.id, invite.company_id, company.name AS "company_name?", invite.email,
                      invite.role, invite.status, invite.created_at AS "created_at!"
               FROM company_invites AS invite
               JOIN companies AS company ON company.id = invite.company_id
               WHERE invite.email = $1
               ORDER BY invite.created_at DESC, invite.id DESC LIMIT 200"#,
            email
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
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn decline_pending_invite(
        &self,
        invite_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>> {
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
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>> {
        let db_list = sqlx::query_as!(
            CompanyMemberDb,
            r#"SELECT member.id, member.company_id, member.user_id,
                      account.username AS "username?", account.email AS "email?", account.avatar_url,
                      member.role, member.created_at AS "created_at!"
               FROM company_members AS member
               JOIN users AS account ON account.id = member.user_id
               WHERE member.company_id = $1
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
                   WHERE company_id = $1 AND user_id = $2
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

    async fn remove_member(&self, company_id: Uuid, user_id: Uuid) -> AppResult<()> {
        sqlx::query!(
            "DELETE FROM company_members WHERE company_id = $1 AND user_id = $2",
            company_id,
            user_id
        )
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
    use crate::use_cases::user::UserPersistence;

    #[tokio::test]
    async fn postgres_company_invite_and_member_persistence_works() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let persistence = PostgresPersistence::new(pool);

        // Create test owner and user
        let owner_username = format!("owner_{}", Uuid::new_v4().simple());
        let owner_email = format!("{}@example.com", owner_username);
        let _ = persistence
            .create_user(&owner_username, &owner_email, "hash")
            .await;
        let owner = persistence
            .get_by_email(&owner_email)
            .await
            .unwrap()
            .unwrap();

        let member_username = format!("member_{}", Uuid::new_v4().simple());
        let member_email = format!("{}@example.com", member_username);
        let _ = persistence
            .create_user(&member_username, &member_email, "hash")
            .await;
        let member = persistence
            .get_by_email(&member_email)
            .await
            .unwrap()
            .unwrap();

        // Create company
        let company = persistence
            .create(
                owner.id,
                CompanyWrite {
                    name: "Test Corp".to_string(),
                    slug: "test-corp".to_string(),
                    ..CompanyWrite::default()
                },
            )
            .await
            .unwrap();

        // 1. Create Invite
        let invite = persistence
            .create_invite(company.id, &member_email, CompanyAccessRole::Admin)
            .await
            .unwrap();
        assert_eq!(invite.email, member_email);
        assert_eq!(invite.role, CompanyAccessRole::Admin);
        assert_eq!(invite.status, "pending");

        // 2. List Invites by Company
        let invites = persistence
            .list_invites_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(invites.len(), 1);

        // 3. Update Invite Email
        let updated_email = format!("new_{}", member_email);
        let updated = persistence
            .update_invite(invite.id, &updated_email, CompanyAccessRole::Member)
            .await
            .unwrap();
        assert_eq!(updated.email, updated_email);
        assert_eq!(updated.role, CompanyAccessRole::Member);

        // Update back
        let _ = persistence
            .update_invite(invite.id, &member_email, CompanyAccessRole::Admin)
            .await
            .unwrap();

        // 4. Accept invite and add the member atomically
        let accepted = persistence
            .accept_pending_invite(invite.id, member.id, &member_email)
            .await
            .unwrap();
        assert_eq!(accepted.unwrap().status, "accepted");
        let accepted_again = persistence
            .accept_pending_invite(invite.id, member.id, &member_email)
            .await
            .unwrap();
        assert!(accepted_again.is_none());

        // 5. List team members
        let members = persistence
            .list_members_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].username, Some(member_username));
        assert_eq!(members[0].role, CompanyAccessRole::Admin);

        let changed = persistence
            .update_member_role(company.id, member.id, CompanyAccessRole::Member)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(changed.role, CompanyAccessRole::Member);
        let members = persistence
            .list_members_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(members[0].role, CompanyAccessRole::Member);

        // 6. Remove team member
        persistence
            .remove_member(company.id, member.id)
            .await
            .unwrap();
        let members_after = persistence
            .list_members_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(members_after.len(), 0);

        // 7. Delete invite
        persistence.delete_invite(invite.id).await.unwrap();
        let invites_after = persistence
            .list_invites_by_company(company.id)
            .await
            .unwrap();
        assert_eq!(invites_after.len(), 0);

        // Cleanup company
        let _ = persistence.delete(company.id).await;
    }
}
