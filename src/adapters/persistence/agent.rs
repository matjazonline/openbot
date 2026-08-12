use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::agent::Agent,
    use_cases::agent::AgentPersistence,
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct AgentDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub config_json: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
}

impl From<AgentDb> for Agent {
    fn from(db: AgentDb) -> Self {
        Agent {
            id: db.id,
            company_id: db.company_id,
            name: db.name,
            slug: db.slug,
            provider: db.provider,
            model: db.model,
            api_key: db.api_key,
            system_prompt: db.system_prompt,
            config_json: db.config_json,
            created_at: db.created_at,
        }
    }
}

#[async_trait]
impl AgentPersistence for PostgresPersistence {
    async fn create(
        &self,
        company_id: Uuid,
        name: &str,
        slug: &str,
        provider: Option<&str>,
        model: Option<&str>,
        api_key: Option<&str>,
        system_prompt: Option<&str>,
        config_json: Option<serde_json::Value>,
    ) -> AppResult<Agent> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as::<_, AgentDb>(
            r#"INSERT INTO agents (id, company_id, name, slug, provider, model, api_key, system_prompt, config_json)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
               RETURNING id, company_id, name, slug, provider, model, api_key, system_prompt, config_json, created_at"#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(name)
        .bind(slug)
        .bind(provider)
        .bind(model)
        .bind(api_key)
        .bind(system_prompt)
        .bind(config_json)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Agent>> {
        let db = sqlx::query_as::<_, AgentDb>(
            r#"SELECT id, company_id, name, slug, provider, model, api_key, system_prompt, config_json, created_at
               FROM agents WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn get_by_company_slug_and_agent_slug(
        &self,
        company_slug: &str,
        agent_slug: &str,
    ) -> AppResult<Option<Agent>> {
        let db = sqlx::query_as::<_, AgentDb>(
            r#"SELECT a.id, a.company_id, a.name, a.slug, a.provider, a.model, a.api_key, a.system_prompt, a.config_json, a.created_at
               FROM agents a
               JOIN companies c ON c.id = a.company_id
               WHERE LOWER(c.slug) = LOWER($1) AND LOWER(a.slug) = LOWER($2)"#,
        )
        .bind(company_slug)
        .bind(agent_slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Agent>> {
        let db_list = sqlx::query_as::<_, AgentDb>(
            r#"SELECT id, company_id, name, slug, provider, model, api_key, system_prompt, config_json, created_at
               FROM agents WHERE company_id = $1 ORDER BY created_at DESC"#,
        )
        .bind(company_id)
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
        provider: Option<&str>,
        model: Option<&str>,
        api_key: Option<&str>,
        system_prompt: Option<&str>,
        config_json: Option<serde_json::Value>,
    ) -> AppResult<Agent> {
        let db = sqlx::query_as::<_, AgentDb>(
            r#"UPDATE agents
               SET name = $1, slug = $2, provider = $3, model = $4, api_key = $5, system_prompt = $6, config_json = $7
               WHERE id = $8
               RETURNING id, company_id, name, slug, provider, model, api_key, system_prompt, config_json, created_at"#,
        )
        .bind(name)
        .bind(slug)
        .bind(provider)
        .bind(model)
        .bind(api_key)
        .bind(system_prompt)
        .bind(config_json)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        sqlx::query!("DELETE FROM agents WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::use_cases::company::CompanyPersistence;
    use crate::use_cases::user::UserPersistence;

    #[tokio::test]
    async fn postgres_agent_persistence_works() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => return,
        };

        let pool = match sqlx::PgPool::connect(&database_url).await {
            Ok(p) => p,
            Err(_) => return,
        };

        let persistence = PostgresPersistence::new(pool);

        let owner_username = format!("owner_{}", Uuid::new_v4().simple());
        let owner_email = format!("{}@example.com", owner_username);
        let _ = persistence.create_user(&owner_username, &owner_email, "hash").await;
        let owner = persistence.get_by_email(&owner_email).await.unwrap().unwrap();

        let company = CompanyPersistence::create(&persistence, owner.id, "Agent Corp", "agent-corp", None, None, None, None)
            .await
            .unwrap();

        let config = json!({ "prompt": "System prompt" });

        let agent = AgentPersistence::create(
            &persistence,
            company.id,
            "Support Agent",
            "support-agent",
            Some("openai"),
            Some("gpt-4o"),
            Some("key_123"),
            Some("You are a helpful support agent."),
            Some(config.clone()),
        )
        .await
        .unwrap();

        assert_eq!(agent.name, "Support Agent");
        assert_eq!(agent.slug, "support-agent");
        assert_eq!(agent.provider.as_deref(), Some("openai"));
        assert_eq!(agent.model.as_deref(), Some("gpt-4o"));
        assert_eq!(agent.api_key.as_deref(), Some("key_123"));
        assert_eq!(agent.system_prompt.as_deref(), Some("You are a helpful support agent."));
        assert_eq!(agent.config_json, Some(config));

        let fetched = AgentPersistence::get_by_id(&persistence, agent.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, agent.id);

        let list = AgentPersistence::list_by_company_id(&persistence, company.id).await.unwrap();
        assert_eq!(list.len(), 1);

        let updated = AgentPersistence::update(
            &persistence,
            agent.id,
            "Support Agent V2",
            "support-agent-v2",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "Support Agent V2");
        assert_eq!(updated.api_key, None);

        AgentPersistence::delete(&persistence, agent.id).await.unwrap();
        let list_after = AgentPersistence::list_by_company_id(&persistence, company.id).await.unwrap();
        assert_eq!(list_after.len(), 0);

        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }
}
