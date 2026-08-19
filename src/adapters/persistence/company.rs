use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{company::Company, value_objects::CompanySlug},
    use_cases::company::CompanyPersistence,
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
            created_at: db.created_at,
        }
    }
}

#[async_trait]
impl CompanyPersistence for PostgresPersistence {
    async fn create(
        &self,
        user_id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        enable_llm_spam_guardrail: Option<bool>,
    ) -> AppResult<Company> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as::<_, CompanyDb>(
            r#"INSERT INTO companies (id, user_id, name, slug, api_key, provider, model, enable_llm_spam_guardrail) 
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8) 
               RETURNING id, user_id, name, slug, api_key, provider, model, enable_llm_spam_guardrail, created_at"#,
        )
        .bind(uuid)
        .bind(user_id)
        .bind(name)
        .bind(slug)
        .bind(api_key)
        .bind(provider)
        .bind(model)
        .bind(enable_llm_spam_guardrail)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug, api_key, provider, model, enable_llm_spam_guardrail, created_at 
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
            r#"SELECT id, user_id, name, slug, api_key, provider, model, enable_llm_spam_guardrail, created_at
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
            r#"SELECT id, user_id, name, slug, api_key, provider, model, enable_llm_spam_guardrail, created_at 
               FROM companies WHERE user_id = $1
               ORDER BY created_at DESC, id DESC LIMIT 200"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
    }

    async fn update(
        &self,
        id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        enable_llm_spam_guardrail: Option<bool>,
    ) -> AppResult<Company> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"UPDATE companies SET name = $1, slug = $2, api_key = $3, provider = $4, model = $5, enable_llm_spam_guardrail = $6 
               WHERE id = $7 
               RETURNING id, user_id, name, slug, api_key, provider, model, enable_llm_spam_guardrail, created_at"#,
        )
        .bind(name)
        .bind(slug)
        .bind(api_key)
        .bind(provider)
        .bind(model)
        .bind(enable_llm_spam_guardrail)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        sqlx::query!("DELETE FROM companies WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(())
    }

    async fn update_for_user(
        &self,
        user_id: Uuid,
        id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        enable_llm_spam_guardrail: Option<bool>,
    ) -> AppResult<Company> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"UPDATE companies
               SET name = $1, slug = $2, api_key = $3, provider = $4, model = $5,
                   enable_llm_spam_guardrail = $6
               WHERE id = $7 AND user_id = $8
               RETURNING id, user_id, name, slug, api_key, provider, model,
                         enable_llm_spam_guardrail, created_at"#,
        )
        .bind(name)
        .bind(slug)
        .bind(api_key)
        .bind(provider)
        .bind(model)
        .bind(enable_llm_spam_guardrail)
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Internal("Company not found.".into()))?;
        Ok(db.into())
    }

    async fn delete_for_user(&self, user_id: Uuid, id: Uuid) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM companies WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;
        if result.rows_affected() != 1 {
            return Err(AppError::Internal("Company not found.".into()));
        }
        Ok(())
    }

    async fn is_company_team_member(&self, company_id: Uuid, email: &str) -> AppResult<bool> {
        let clean_email = email.trim().to_lowercase();
        let res = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                SELECT 1 FROM companies c JOIN users u ON c.user_id = u.id WHERE c.id = $1 AND u.email = $2
                UNION ALL
                SELECT 1 FROM company_members m JOIN users u ON m.user_id = u.id WHERE m.company_id = $1 AND u.email = $2
            ) as "exists!""#,
            company_id,
            clean_email
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(res)
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
