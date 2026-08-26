use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        company::{Company, CompanyAccess},
        company_member::CompanyMembership,
        memory::MemoryProviderKind,
        value_objects::{AvatarUrl, CompanySlug},
    },
    use_cases::company::{CompanyPersistence, CompanyWrite},
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyDb {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub slug: String,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enable_llm_spam_guardrail: Option<bool>,
    pub memory_provider: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<CompanyDb> for Company {
    fn from(db: CompanyDb) -> Self {
        Company {
            id: db.id,
            user_id: db.user_id,
            name: db.name,
            slug: CompanySlug::from(db.slug),
            api_key: db.api_key,
            provider: db.provider,
            model: db.model,
            enable_llm_spam_guardrail: db.enable_llm_spam_guardrail,
            memory_provider: match db.memory_provider.as_deref() {
                Some("hydradb") => Some(MemoryProviderKind::Hydradb),
                // `none` is accepted only as a legacy stored representation. New writes use NULL.
                Some("none") | None => None,
                Some(_) => None,
            },
            avatar_url: db.avatar_url.map(AvatarUrl::from),
            created_at: db.created_at,
        }
    }
}

/// A company row plus whether the caller owns it, for [`CompanyPersistence::list_accessible_by_user_id`].
#[derive(sqlx::FromRow, Debug)]
struct AccessibleCompanyDb {
    #[sqlx(flatten)]
    company: CompanyDb,
    is_owner: bool,
    is_admin: bool,
}

impl From<AccessibleCompanyDb> for CompanyAccess {
    fn from(db: AccessibleCompanyDb) -> Self {
        CompanyAccess {
            membership: if db.is_owner {
                CompanyMembership::Owner
            } else if db.is_admin {
                CompanyMembership::Admin
            } else {
                CompanyMembership::Member
            },
            company: db.company.into(),
        }
    }
}

impl PostgresPersistence {
    fn decode_company(&self, mut db: CompanyDb) -> AppResult<Company> {
        db.api_key = self.decrypt_credential(db.api_key)?;
        Ok(db.into())
    }
}

async fn delete_company_with_cleanup(
    persistence: &PostgresPersistence,
    id: Uuid,
    owner_id: Option<Uuid>,
) -> AppResult<bool> {
    let mut transaction = persistence.pool.begin().await.map_err(AppError::from)?;
    let company_exists: bool = match owner_id {
        Some(owner_id) => sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1 AND user_id = $2)",
        )
        .bind(id)
        .bind(owner_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(AppError::from)?,
        None => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1)")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(AppError::from)?,
    };
    if !company_exists {
        return Ok(false);
    }
    let remote_database_ids = sqlx::query_scalar::<_, String>(
        r#"SELECT remote_database_id
           FROM memory_provider_connections
           WHERE company_id = $1 AND provider = 'hydradb'"#,
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(AppError::from)?;
    for remote_database_id in remote_database_ids {
        sqlx::query(
            r#"INSERT INTO memory_cleanup_jobs
                   (id, provider, remote_database_id)
               VALUES ($1, 'hydradb', $2)
               ON CONFLICT (provider, remote_database_id) DO NOTHING"#,
        )
        .bind(Uuid::new_v4())
        .bind(remote_database_id)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
    }
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
    transaction.commit().await.map_err(AppError::from)?;
    Ok(true)
}

#[async_trait]
impl CompanyPersistence for PostgresPersistence {
    async fn create(&self, user_id: Uuid, write: CompanyWrite) -> AppResult<Company> {
        let uuid = Uuid::new_v4();

        let encrypted_api_key = self.encrypt_credential(write.api_key.as_deref())?;
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"INSERT INTO companies (id, user_id, name, slug, api_key, provider, model,
                                      enable_llm_spam_guardrail, memory_provider, avatar_url)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               RETURNING id, user_id, name, slug, api_key, provider, model, enable_llm_spam_guardrail, memory_provider,
                      avatar_url, created_at"#,
        )
        .bind(uuid)
        .bind(user_id)
        .bind(&write.name)
        .bind(&write.slug)
        .bind(encrypted_api_key)
        .bind(&write.provider)
        .bind(&write.model)
        .bind(write.enable_llm_spam_guardrail)
        .bind(write.memory_provider.map(MemoryProviderKind::as_str))
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        self.decode_company(db)
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug, api_key, provider, model,
                      enable_llm_spam_guardrail, memory_provider, avatar_url, created_at
               FROM companies WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(|db| self.decode_company(db)).transpose()
    }

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug, api_key, provider, model,
                      enable_llm_spam_guardrail, memory_provider, avatar_url, created_at
               FROM companies WHERE slug = $1"#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(|db| self.decode_company(db)).transpose()
    }

    async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        let db_list = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug, NULL::text AS api_key, provider, model,
                      enable_llm_spam_guardrail, memory_provider, avatar_url, created_at
               FROM companies WHERE user_id = $1
               ORDER BY created_at DESC, id DESC LIMIT 200"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
    }

    async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
        // The owner ∪ members union `membership_for_email` already asks by address, asked here by
        // account id -- the read guards start from a session, the inbound path from an envelope,
        // and both need to know *which* of the two the caller is.
        let db_list = sqlx::query_as::<_, AccessibleCompanyDb>(
            r#"SELECT company.id, company.user_id, company.name, company.slug,
                      NULL::text AS api_key, company.provider, company.model,
                      company.enable_llm_spam_guardrail, company.memory_provider,
                      company.avatar_url, company.created_at,
                      (company.user_id = $1) AS is_owner,
                      COALESCE((
                          SELECT member.role = 'admin'
                          FROM company_members AS member
                          WHERE member.company_id = company.id AND member.user_id = $1
                      ), FALSE) AS is_admin
               FROM companies AS company
               WHERE company.user_id = $1
                  OR EXISTS (
                       SELECT 1 FROM company_members AS member
                       WHERE member.company_id = company.id AND member.user_id = $1
                     )
               ORDER BY company.created_at DESC, company.id DESC LIMIT 200"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
    }

    async fn update(&self, id: Uuid, write: CompanyWrite) -> AppResult<Company> {
        let encrypted_api_key = self.encrypt_credential(write.api_key.as_deref())?;
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"UPDATE companies SET name = $1, slug = $2, api_key = $3, provider = $4, model = $5,
                      enable_llm_spam_guardrail = $6,
                      memory_provider = COALESCE($7, memory_provider), avatar_url = $8
               WHERE id = $9
               RETURNING id, user_id, name, slug, api_key, provider, model, enable_llm_spam_guardrail, memory_provider,
                      avatar_url, created_at"#,
        )
        .bind(&write.name)
        .bind(&write.slug)
        .bind(encrypted_api_key)
        .bind(&write.provider)
        .bind(&write.model)
        .bind(write.enable_llm_spam_guardrail)
        .bind(write.memory_provider.map(MemoryProviderKind::as_str))
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        self.decode_company(db)
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        delete_company_with_cleanup(self, id, None).await?;
        Ok(())
    }

    async fn update_for_user(
        &self,
        user_id: Uuid,
        id: Uuid,
        write: CompanyWrite,
    ) -> AppResult<Company> {
        let encrypted_api_key = self.encrypt_credential(write.api_key.as_deref())?;
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"UPDATE companies
               SET name = $1, slug = $2, api_key = $3, provider = $4, model = $5,
                   enable_llm_spam_guardrail = $6,
                   memory_provider = COALESCE($7, memory_provider), avatar_url = $8
               WHERE id = $9 AND user_id = $10
               RETURNING id, user_id, name, slug, api_key, provider, model,
                         enable_llm_spam_guardrail, memory_provider, avatar_url, created_at"#,
        )
        .bind(&write.name)
        .bind(&write.slug)
        .bind(encrypted_api_key)
        .bind(&write.provider)
        .bind(&write.model)
        .bind(write.enable_llm_spam_guardrail)
        .bind(write.memory_provider.map(MemoryProviderKind::as_str))
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Internal("Company not found.".into()))?;
        self.decode_company(db)
    }

    async fn delete_for_user(&self, user_id: Uuid, id: Uuid) -> AppResult<()> {
        if !delete_company_with_cleanup(self, id, Some(user_id)).await? {
            return Err(AppError::Internal("Company not found.".into()));
        }
        Ok(())
    }

    async fn membership_for_email(
        &self,
        company_id: Uuid,
        email: &str,
    ) -> AppResult<CompanyMembership> {
        let clean_email = email.trim().to_lowercase();
        let is_owner = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                SELECT 1
                FROM companies AS company
                JOIN users AS account ON company.user_id = account.id
                WHERE company.id = $1 AND LOWER(account.email) = $2
            ) as "exists!""#,
            company_id,
            clean_email
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        if is_owner {
            return Ok(CompanyMembership::Owner);
        }

        let role = sqlx::query_scalar!(
            r#"SELECT member.role
               FROM company_members AS member
               JOIN users AS account ON member.user_id = account.id
               WHERE member.company_id = $1 AND LOWER(account.email) = $2"#,
            company_id,
            clean_email
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        match role.as_deref() {
            Some("admin") => Ok(CompanyMembership::Admin),
            Some("member") => Ok(CompanyMembership::Member),
            Some(role) => Err(AppError::Internal(format!(
                "Unknown company access role: {role}"
            ))),
            None => Ok(CompanyMembership::None),
        }
    }

    async fn list_company_team_emails(&self, company_id: Uuid) -> AppResult<Vec<String>> {
        let rows = sqlx::query_scalar!(
            r#"SELECT DISTINCT LOWER(u.email) as "email!"
               FROM (
                   SELECT u.email FROM companies c JOIN users u ON c.user_id = u.id WHERE c.id = $1
                   UNION ALL
                   SELECT u.email FROM company_members m JOIN users u ON m.user_id = u.id WHERE m.company_id = $1
               ) u"#,
            company_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::persistence::test_support::test_pool,
        entities::company_member::CompanyAccessRole,
        use_cases::{company_invite::CompanyInvitePersistence, user::UserPersistence},
    };

    /// The accessible-companies query is built at runtime, so nothing but a real database can say
    /// whether its `is_owner` projection and its `company_members` join do what they claim.
    #[tokio::test]
    async fn a_company_is_accessible_to_its_owner_and_to_whoever_was_invited_to_it() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::with_credential_cipher(
            pool.clone(),
            crate::adapters::persistence::credentials::CredentialCipher::for_test(),
        );

        let owner = create_account(&persistence, "owner").await;
        let admin = create_account(&persistence, "admin").await;
        let member = create_account(&persistence, "member").await;
        let stranger = create_account(&persistence, "stranger").await;

        let company = persistence
            .create(
                owner.0,
                CompanyWrite {
                    name: "Acme".to_string(),
                    slug: format!("acme-{}", Uuid::new_v4().simple()),
                    api_key: Some("sk-company-secret".to_string()),
                    ..CompanyWrite::default()
                },
            )
            .await
            .expect("a company");
        assert_eq!(company.api_key.as_deref(), Some("sk-company-secret"));
        let stored: String = sqlx::query_scalar("SELECT api_key FROM companies WHERE id = $1")
            .bind(company.id)
            .fetch_one(&pool)
            .await
            .expect("the stored credential");
        assert!(stored.starts_with("enc:v1:1:"));
        assert!(!stored.contains("sk-company-secret"));

        let listed = persistence
            .list_by_user_id(owner.0)
            .await
            .expect("the ordinary company list");
        assert_eq!(listed[0].api_key, None);

        // Before the invite is accepted, the member is a stranger to it.
        assert!(accessible_ids(&persistence, member.0).await.is_empty());

        let invite = persistence
            .create_invite(company.id, &member.1, CompanyAccessRole::Member)
            .await
            .expect("an invite");
        persistence
            .accept_pending_invite(invite.id, member.0, &member.1)
            .await
            .expect("the invite is accepted");
        let admin_invite = persistence
            .create_invite(company.id, &admin.1, CompanyAccessRole::Admin)
            .await
            .expect("an admin invite");
        persistence
            .accept_pending_invite(admin_invite.id, admin.0, &admin.1)
            .await
            .expect("the admin invite is accepted");

        // The owner owns it, the member was let in, and the stranger sees nothing.
        let owner_view = persistence
            .company_access(owner.0, company.id)
            .await
            .expect("a lookup")
            .expect("the owner reaches their own company");
        assert_eq!(owner_view.membership, CompanyMembership::Owner);
        assert_eq!(owner_view.company.id, company.id);

        let member_view = persistence
            .company_access(member.0, company.id)
            .await
            .expect("a lookup")
            .expect("an accepted invite reaches the company");
        assert_eq!(member_view.membership, CompanyMembership::Member);

        let admin_view = persistence
            .company_access(admin.0, company.id)
            .await
            .expect("a lookup")
            .expect("an accepted admin reaches the company");
        assert_eq!(admin_view.membership, CompanyMembership::Admin);

        assert!(
            persistence
                .company_access(stranger.0, company.id)
                .await
                .expect("a lookup")
                .is_none()
        );

        // The inbound path asks the same question by address, and must tell the owner apart from
        // the rest of the team: `Channel::participant_access` hands the owner a restricted channel
        // they are not listed on, and hands a member nothing.
        assert_eq!(
            persistence
                .membership_for_email(company.id, &owner.1.to_uppercase())
                .await
                .expect("a lookup"),
            CompanyMembership::Owner
        );
        assert_eq!(
            persistence
                .membership_for_email(company.id, &member.1)
                .await
                .expect("a lookup"),
            CompanyMembership::Member
        );
        assert_eq!(
            persistence
                .membership_for_email(company.id, &admin.1)
                .await
                .expect("a lookup"),
            CompanyMembership::Admin
        );
        assert_eq!(
            persistence
                .membership_for_email(company.id, &stranger.1)
                .await
                .expect("a lookup"),
            CompanyMembership::None
        );
        assert_eq!(
            persistence
                .membership_for_email(company.id, "nobody@example.com")
                .await
                .expect("a lookup"),
            CompanyMembership::None
        );

        // ...and the ownership-scoped listing the administration pages use is unchanged by any
        // of this: an invited member still owns nothing.
        assert!(
            persistence
                .list_by_user_id(member.0)
                .await
                .expect("a listing")
                .is_empty()
        );

        persistence.remove_member(company.id, member.0).await.ok();
        persistence.remove_member(company.id, admin.0).await.ok();
        persistence.delete_invite(invite.id).await.ok();
        persistence.delete_invite(admin_invite.id).await.ok();
        persistence.delete(company.id).await.ok();
    }

    /// A fresh account, as its id and address.
    async fn create_account(persistence: &PostgresPersistence, prefix: &str) -> (Uuid, String) {
        let username = format!("{prefix}_{}", Uuid::new_v4().simple());
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .expect("an account");

        let user = persistence
            .get_by_email(&email)
            .await
            .expect("a lookup")
            .expect("the account just created");

        (user.id, email)
    }

    async fn accessible_ids(persistence: &PostgresPersistence, user_id: Uuid) -> Vec<Uuid> {
        persistence
            .list_accessible_by_user_id(user_id)
            .await
            .expect("a listing")
            .into_iter()
            .map(|access| access.company.id)
            .collect()
    }
}
