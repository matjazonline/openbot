use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{company_invite::CompanyInvite, company_member::CompanyMember},
    use_cases::company_invite::CompanyInvitePersistence,
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyInviteDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub company_name: Option<String>,
    pub email: String,
    pub status: String,
    pub created_at: NaiveDateTime,
}

impl From<CompanyInviteDb> for CompanyInvite {
    fn from(db: CompanyInviteDb) -> Self {
        CompanyInvite {
            id: db.id,
            company_id: db.company_id,
            company_name: db.company_name,
            email: db.email,
            status: db.status,
            created_at: db.created_at,
        }
    }
}

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyMemberDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub email: Option<String>,
    pub role: String,
    pub created_at: NaiveDateTime,
}

impl From<CompanyMemberDb> for CompanyMember {
    fn from(db: CompanyMemberDb) -> Self {
        CompanyMember {
            id: db.id,
            company_id: db.company_id,
            user_id: db.user_id,
            username: db.username,
            email: db.email,
            role: db.role,
            created_at: db.created_at,
        }
    }
}

#[async_trait]
impl CompanyInvitePersistence for PostgresPersistence {
    async fn create_invite(&self, company_id: Uuid, email: &str) -> AppResult<CompanyInvite> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH invite AS (
                INSERT INTO company_invites (id, company_id, email, status)
                VALUES ($1, $2, $3, 'pending')
                ON CONFLICT (company_id, email)
                DO UPDATE SET status = 'pending', created_at = CURRENT_TIMESTAMP
                RETURNING id, company_id, email, status, created_at
            )
            SELECT i.id, i.company_id, c.name AS company_name, i.email, i.status, i.created_at
            FROM invite i
            JOIN companies c ON c.id = i.company_id
            "#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(email)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_invite_by_id(&self, id: Uuid) -> AppResult<Option<CompanyInvite>> {
        let db = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT i.id, i.company_id, c.name as "company_name?", i.email, i.status, i.created_at as "created_at!"
               FROM company_invites i
               JOIN companies c ON c.id = i.company_id
               WHERE i.id = $1"#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>> {
        let db_list = sqlx::query_as!(
            CompanyInviteDb,
            r#"SELECT i.id, i.company_id, c.name as "company_name?", i.email, i.status, i.created_at as "created_at!"
               FROM company_invites i
               JOIN companies c ON c.id = i.company_id
               WHERE i.company_id = $1
               ORDER BY i.created_at DESC, i.id DESC LIMIT 200"#,
            company_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
    }

    async fn update_invite_email(&self, id: Uuid, new_email: &str) -> AppResult<CompanyInvite> {
        let db = sqlx::query_as::<_, CompanyInviteDb>(
            r#"
            WITH invite AS (
                UPDATE company_invites
                SET email = $1
                WHERE id = $2
                RETURNING id, company_id, email, status, created_at
            )
            SELECT i.id, i.company_id, c.name AS company_name, i.email, i.status, i.created_at
            FROM invite i
            JOIN companies c ON c.id = i.company_id
            "#,
        )
        .bind(new_email)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(Into::into)
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
            r#"SELECT i.id, i.company_id, c.name as "company_name?", i.email, i.status, i.created_at as "created_at!"
               FROM company_invites i
               JOIN companies c ON c.id = i.company_id
               WHERE i.email = $1
               ORDER BY i.created_at DESC, i.id DESC LIMIT 200"#,
            email
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
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
                RETURNING id, company_id, email, status, created_at
            ), membership AS (
                INSERT INTO company_members (id, company_id, user_id, role)
                SELECT $3, company_id, $4, 'member'
                FROM accepted
                ON CONFLICT (company_id, user_id)
                DO UPDATE SET role = company_members.role
                RETURNING company_id
            )
            SELECT a.id, a.company_id, c.name AS company_name, a.email, a.status, a.created_at
            FROM accepted a
            JOIN membership m ON m.company_id = a.company_id
            JOIN companies c ON c.id = a.company_id
            "#,
        )
        .bind(invite_id)
        .bind(user_email)
        .bind(member_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
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
                RETURNING id, company_id, email, status, created_at
            )
            SELECT i.id, i.company_id, c.name AS company_name, i.email, i.status, i.created_at
            FROM declined i
            JOIN companies c ON c.id = i.company_id
            "#,
        )
        .bind(invite_id)
        .bind(user_email)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>> {
        let db_list = sqlx::query_as!(
            CompanyMemberDb,
            r#"SELECT m.id, m.company_id, m.user_id, u.username as "username?", u.email as "email?", m.role, m.created_at as "created_at!"
               FROM company_members m
               JOIN users u ON u.id = m.user_id
               WHERE m.company_id = $1
               ORDER BY m.created_at ASC, m.id ASC LIMIT 200"#,
            company_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
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
    use crate::use_cases::company::CompanyPersistence;
    use crate::use_cases::user::UserPersistence;

    #[tokio::test]
    async fn postgres_company_invite_and_member_persistence_works() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => return, // Skip test if DATABASE_URL is not set
        };

        let pool = match sqlx::PgPool::connect(&database_url).await {
            Ok(p) => p,
            Err(_) => return,
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
            .create(owner.id, "Test Corp", "test-corp", None, None, None, None)
            .await
            .unwrap();

        // 1. Create Invite
        let invite = persistence
            .create_invite(company.id, &member_email)
            .await
            .unwrap();
        assert_eq!(invite.email, member_email);
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
            .update_invite_email(invite.id, &updated_email)
            .await
            .unwrap();
        assert_eq!(updated.email, updated_email);

        // Update back
        let _ = persistence
            .update_invite_email(invite.id, &member_email)
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
