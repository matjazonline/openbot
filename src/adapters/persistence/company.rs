use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        company::{Company, CompanyAccess, CompanyModelConnection},
        company_member::CompanyMembership,
        memory::{MEMORY_DELETION_QUIESCENCE_SECONDS, MemoryProviderKind},
        value_objects::{AvatarUrl, CompanySlug, ModelName, ModelProvider},
    },
    use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence, CompanyWrite},
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyDb {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub slug: String,
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
            enable_llm_spam_guardrail: db.enable_llm_spam_guardrail,
            // `none` is accepted only as a legacy stored representation, and an unknown value
            // reads as "no provider" rather than failing the row: memory is optional, and a
            // company must stay loadable after a provider is withdrawn from a deployment.
            memory_provider: db
                .memory_provider
                .as_deref()
                .and_then(MemoryProviderKind::parse),
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

/// How many dependent agents a rejected model-connection change names before it stops listing
/// them. The message is for a person reading a form, not an inventory.
const MAX_REPORTED_ORPHANED_AGENTS: usize = 20;

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
    // Keep the lock order used by memory workers: execution row before lifecycle row. This also
    // prevents the cascade below from deleting a job while deletion is deriving its fence.
    sqlx::query("SELECT id FROM memory_provisioning_jobs WHERE company_id = $1 FOR UPDATE")
        .bind(id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(AppError::from)?;
    // Every provider this company holds remote data for, not just one of them: a lifecycle left
    // behind here is remote data nothing remembers exists.
    let remote_resources = sqlx::query_as::<_, (String, String)>(
        r#"SELECT provider, remote_database_id
           FROM memory_remote_resource_lifecycles
           WHERE company_id = $1
           FOR UPDATE"#,
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(AppError::from)?;
    let quiesce_until = Utc::now() + chrono::Duration::seconds(MEMORY_DELETION_QUIESCENCE_SECONDS);
    for (provider, remote_database_id) in remote_resources {
        sqlx::query(
            r#"UPDATE memory_remote_resource_lifecycles
               SET company_id = NULL, desired_state = 'absent',
                   quiesce_until = GREATEST(quiesce_until, $3),
                   last_error = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE provider = $1 AND remote_database_id = $2"#,
        )
        .bind(&provider)
        .bind(&remote_database_id)
        .bind(quiesce_until)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"INSERT INTO memory_cleanup_jobs
                   (id, provider, remote_database_id, available_at)
               VALUES ($1, $4, $2, $3)
               ON CONFLICT (provider, remote_database_id) DO UPDATE
               SET status = 'pending', attempts = 0, available_at = EXCLUDED.available_at,
                   lease_token = NULL, lease_expires_at = NULL,
                   operation_generation = NULL, last_error = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE memory_cleanup_jobs.status <> 'leased'"#,
        )
        .bind(Uuid::new_v4())
        .bind(remote_database_id)
        .bind(quiesce_until)
        .bind(provider)
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

        let db = sqlx::query_as::<_, CompanyDb>(
            r#"INSERT INTO companies (id, user_id, name, slug,
                                      enable_llm_spam_guardrail, memory_provider, avatar_url)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING id, user_id, name, slug, enable_llm_spam_guardrail, memory_provider,
                      avatar_url, created_at"#,
        )
        .bind(uuid)
        .bind(user_id)
        .bind(&write.name)
        .bind(&write.slug)
        .bind(write.enable_llm_spam_guardrail)
        .bind(write.memory_provider.map(MemoryProviderKind::as_str))
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug,
                      enable_llm_spam_guardrail, memory_provider, avatar_url, created_at
               FROM companies WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug,
                      enable_llm_spam_guardrail, memory_provider, avatar_url, created_at
               FROM companies WHERE slug = $1"#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        let db_list = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug,
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
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"UPDATE companies SET name = $1, slug = $2,
                      enable_llm_spam_guardrail = $3,
                      memory_provider = COALESCE($4, memory_provider), avatar_url = $5
               WHERE id = $6
               RETURNING id, user_id, name, slug, enable_llm_spam_guardrail, memory_provider,
                      avatar_url, created_at"#,
        )
        .bind(&write.name)
        .bind(&write.slug)
        .bind(write.enable_llm_spam_guardrail)
        .bind(write.memory_provider.map(MemoryProviderKind::as_str))
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
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
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"UPDATE companies
               SET name = $1, slug = $2, enable_llm_spam_guardrail = $3,
                   memory_provider = COALESCE($4, memory_provider), avatar_url = $5
               WHERE id = $6 AND user_id = $7
               RETURNING id, user_id, name, slug,
                         enable_llm_spam_guardrail, memory_provider, avatar_url, created_at"#,
        )
        .bind(&write.name)
        .bind(&write.slug)
        .bind(write.enable_llm_spam_guardrail)
        .bind(write.memory_provider.map(MemoryProviderKind::as_str))
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Internal("Company not found.".into()))?;
        Ok(db.into())
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

    async fn list_model_connections(
        &self,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyModelConnection>> {
        let rows = sqlx::query_as::<_, (String, Vec<String>, bool, bool)>(
            r#"SELECT provider, models, is_default, (api_key IS NOT NULL) AS has_api_key
               FROM company_model_connections
               WHERE company_id = $1
               ORDER BY is_default DESC, provider"#,
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(rows
            .into_iter()
            .map(
                |(provider, models, is_default, has_api_key)| CompanyModelConnection {
                    provider: ModelProvider::from(provider),
                    models: models.into_iter().map(ModelName::from).collect(),
                    is_default,
                    has_api_key,
                },
            )
            .collect())
    }

    async fn model_api_key(
        &self,
        company_id: Uuid,
        provider: &ModelProvider,
    ) -> AppResult<Option<String>> {
        // Folded by `ModelProvider::canonical` before it gets here, so the column is compared as
        // stored rather than through a function that would also hide a miscased write.
        let stored: Option<String> = sqlx::query_scalar(
            r#"SELECT api_key FROM company_model_connections
               WHERE company_id = $1 AND provider = $2"#,
        )
        .bind(company_id)
        .bind(provider.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        self.decrypt_credential(stored)
    }

    async fn replace_model_connections_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let owned = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM companies WHERE id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(company_id)
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        if owned.is_none() {
            return Err(crate::use_cases::company::company_not_found());
        }

        sqlx::query(
            "UPDATE company_model_connections SET is_default = FALSE WHERE company_id = $1 AND is_default",
        )
        .bind(company_id)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        for connection in &connections {
            let encrypted_api_key = self.encrypt_credential(connection.api_key.as_deref())?;
            let models: Vec<String> = connection
                .models
                .iter()
                .map(|model| model.as_str().to_string())
                .collect();
            let changed = match encrypted_api_key {
                Some(api_key) => sqlx::query(
                    r#"INSERT INTO company_model_connections
                           (company_id, provider, api_key, models, is_default)
                       VALUES ($1, $2, $3, $4, $5)
                       ON CONFLICT (company_id, provider) DO UPDATE
                       SET api_key = EXCLUDED.api_key, models = EXCLUDED.models,
                           is_default = EXCLUDED.is_default, updated_at = CURRENT_TIMESTAMP"#,
                )
                .bind(company_id)
                .bind(connection.provider.as_str())
                .bind(api_key)
                .bind(&models)
                .bind(connection.is_default)
                .execute(&mut *transaction)
                .await
                .map_err(AppError::from)?
                .rows_affected(),
                None => sqlx::query(
                    r#"UPDATE company_model_connections
                       SET models = $3, is_default = $4, updated_at = CURRENT_TIMESTAMP
                       WHERE company_id = $1 AND provider = $2"#,
                )
                .bind(company_id)
                .bind(connection.provider.as_str())
                .bind(&models)
                .bind(connection.is_default)
                .execute(&mut *transaction)
                .await
                .map_err(AppError::from)?
                .rows_affected(),
            };
            if changed == 0 {
                return Err(AppError::BadRequest(format!(
                    "An API key is required when adding provider '{}'.",
                    connection.provider
                )));
            }
        }

        let retained: Vec<String> = connections
            .iter()
            .map(|connection| connection.provider.as_str().to_string())
            .collect();
        sqlx::query(
            "DELETE FROM company_model_connections WHERE company_id = $1 AND NOT (provider = ANY($2))",
        )
        .bind(company_id)
        .bind(&retained)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        // Agents are validated against the connections when *they* are written; this is the other
        // direction. Narrowing a `models` list or dropping a provider out from under a pinned
        // agent would otherwise commit cleanly and only surface much later, per run, as
        // "Model 'x' is not enabled for provider 'y'" on a task that had been working. The check
        // runs inside the transaction, so a refusal rolls the whole replace back.
        let orphaned: Vec<String> = sqlx::query_scalar(
            r#"SELECT agent.name
               FROM agents AS agent
               WHERE agent.company_id = $1
                 AND agent.provider IS NOT NULL
                 AND NOT EXISTS (
                     SELECT 1 FROM company_model_connections AS connection
                     WHERE connection.company_id = agent.company_id
                       AND connection.provider = lower(btrim(agent.provider))
                       AND agent.model = ANY(connection.models)
                 )
               ORDER BY agent.name
               LIMIT $2"#,
        )
        .bind(company_id)
        .bind(i64::try_from(MAX_REPORTED_ORPHANED_AGENTS).unwrap_or(i64::MAX))
        .fetch_all(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        if !orphaned.is_empty() {
            return Err(AppError::BadRequest(format!(
                "{} still {} a model this change removes. Point {} at an enabled model first.",
                orphaned.join(", "),
                if orphaned.len() == 1 { "uses" } else { "use" },
                if orphaned.len() == 1 { "it" } else { "them" },
            )));
        }

        transaction.commit().await.map_err(AppError::from)
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
                    ..CompanyWrite::default()
                },
            )
            .await
            .expect("a company");
        let listed = persistence
            .list_by_user_id(owner.0)
            .await
            .expect("the ordinary company list");
        assert_eq!(listed[0].id, company.id);

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

    #[tokio::test]
    async fn a_company_can_replace_multiple_encrypted_model_connections_without_resending_keys() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::with_credential_cipher(
            pool.clone(),
            crate::adapters::persistence::credentials::CredentialCipher::for_test(),
        );
        let owner = create_account(&persistence, "model_connections").await;
        let stranger = create_account(&persistence, "model_connections_stranger").await;
        let company = persistence
            .create(
                owner.0,
                CompanyWrite {
                    name: "Model Connections".into(),
                    slug: format!("models-{}", Uuid::new_v4().simple()),
                    ..CompanyWrite::default()
                },
            )
            .await
            .expect("a company");

        let denied = persistence
            .replace_model_connections_for_user(
                stranger.0,
                company.id,
                vec![
                    CompanyModelConnectionWrite::new(
                        "openai",
                        Some("stolen-secret".into()),
                        vec!["gpt-a".into()],
                        true,
                    )
                    .unwrap(),
                ],
            )
            .await;
        assert!(denied.is_err());

        persistence
            .replace_model_connections_for_user(
                owner.0,
                company.id,
                vec![
                    CompanyModelConnectionWrite::new(
                        "openai",
                        Some("openai-secret".into()),
                        vec!["gpt-a".into(), "gpt-b".into()],
                        true,
                    )
                    .unwrap(),
                    CompanyModelConnectionWrite::new(
                        "anthropic",
                        Some("anthropic-secret".into()),
                        vec!["claude-a".into()],
                        false,
                    )
                    .unwrap(),
                ],
            )
            .await
            .expect("both providers save");

        let metadata = persistence
            .list_model_connections(company.id)
            .await
            .expect("connection metadata");
        assert_eq!(metadata.len(), 2);
        assert!(metadata.iter().all(|connection| connection.has_api_key));
        assert_eq!(
            persistence
                .model_api_key(company.id, &ModelProvider::canonical("anthropic"))
                .await
                .unwrap()
                .as_deref(),
            Some("anthropic-secret")
        );
        let stored: Vec<String> = sqlx::query_scalar(
            "SELECT api_key FROM company_model_connections WHERE company_id = $1 ORDER BY provider",
        )
        .bind(company.id)
        .fetch_all(&pool)
        .await
        .expect("stored credentials");
        assert!(stored.iter().all(|key| key.starts_with("enc:v1:1:")));
        assert!(stored.iter().all(|key| !key.contains("secret")));

        // Browsers never receive stored keys. A blank key on an existing row retains its secret
        // while the model allow-list and default selection can still change.
        persistence
            .replace_model_connections_for_user(
                owner.0,
                company.id,
                vec![
                    CompanyModelConnectionWrite::new("openai", None, vec!["gpt-b".into()], false)
                        .unwrap(),
                    CompanyModelConnectionWrite::new(
                        "anthropic",
                        None,
                        vec!["claude-a".into()],
                        true,
                    )
                    .unwrap(),
                ],
            )
            .await
            .expect("metadata changes without key material");
        assert_eq!(
            persistence
                .model_api_key(company.id, &ModelProvider::canonical("openai"))
                .await
                .unwrap()
                .as_deref(),
            Some("openai-secret")
        );
        let connections = CompanyPersistence::list_model_connections(&persistence, company.id)
            .await
            .unwrap();
        let default = connections
            .iter()
            .find(|connection| connection.is_default)
            .expect("default connection remains");
        assert_eq!(default.provider.as_ref(), "anthropic");
        assert_eq!(default.models[0].as_ref(), "claude-a");

        // The form's explicit remove operation becomes omission from this wholesale replacement.
        // The omitted provider and its encrypted credential are deleted together.
        persistence
            .replace_model_connections_for_user(
                owner.0,
                company.id,
                vec![
                    CompanyModelConnectionWrite::new("openai", None, vec!["gpt-b".into()], true)
                        .unwrap(),
                ],
            )
            .await
            .expect("one provider is deliberately removed");
        assert_eq!(
            persistence
                .model_api_key(company.id, &ModelProvider::canonical("anthropic"))
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            persistence
                .list_model_connections(company.id)
                .await
                .unwrap()
                .len(),
            1
        );

        persistence.delete(company.id).await.ok();
    }

    #[tokio::test]
    async fn a_replace_that_would_strand_a_pinned_agent_is_refused_whole() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::with_credential_cipher(
            pool.clone(),
            crate::adapters::persistence::credentials::CredentialCipher::for_test(),
        );
        let owner = create_account(&persistence, "orphan_guard").await;
        let company = persistence
            .create(
                owner.0,
                CompanyWrite {
                    name: "Orphan Guard".into(),
                    slug: format!("orphan-{}", Uuid::new_v4().simple()),
                    ..CompanyWrite::default()
                },
            )
            .await
            .expect("a company");

        persistence
            .replace_model_connections_for_user(
                owner.0,
                company.id,
                vec![
                    CompanyModelConnectionWrite::new(
                        "openai",
                        Some("openai-secret".into()),
                        vec!["gpt-a".into(), "gpt-b".into()],
                        true,
                    )
                    .unwrap(),
                ],
            )
            .await
            .expect("the initial connection saves");

        crate::use_cases::agent::AgentPersistence::create(
            &persistence,
            company.id,
            crate::use_cases::agent::AgentWrite {
                name: "Pinned".into(),
                slug: "pinned".into(),
                provider: Some("openai".into()),
                model: Some("gpt-b".into()),
                ..Default::default()
            },
        )
        .await
        .expect("an agent pinned to gpt-b");

        // Narrowing the allow-list out from under the agent would leave it unable to run, so the
        // whole replace is refused rather than committing half a configuration.
        let refused = persistence
            .replace_model_connections_for_user(
                owner.0,
                company.id,
                vec![
                    CompanyModelConnectionWrite::new("openai", None, vec!["gpt-a".into()], true)
                        .unwrap(),
                ],
            )
            .await
            .expect_err("the pinned agent blocks the narrowing");
        assert!(
            refused.to_string().contains("Pinned"),
            "the refusal should name the agent: {refused}"
        );

        // ... and nothing was written: gpt-b is still enabled.
        let connections = CompanyPersistence::list_model_connections(&persistence, company.id)
            .await
            .expect("connections survive the rolled-back replace");
        assert_eq!(connections.len(), 1);
        assert!(connections[0].models.iter().any(|model| model == "gpt-b"));

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
