use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{agent::Agent, value_objects::AvatarUrl},
    use_cases::agent::{AgentPersistence, AgentWrite},
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct AgentDb {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub description: Option<String>,
    pub config_json: Option<serde_json::Value>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
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
            description: db.description,
            config_json: db.config_json,
            avatar_url: db.avatar_url.map(AvatarUrl::from),
            created_at: db.created_at,
        }
    }
}

#[async_trait]
impl AgentPersistence for PostgresPersistence {
    async fn create(&self, company_id: Uuid, write: AgentWrite) -> AppResult<Agent> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as::<_, AgentDb>(
            r#"INSERT INTO agents (id, company_id, name, slug, provider, model, api_key, system_prompt, description, config_json, avatar_url)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
               RETURNING id, company_id, name, slug, provider, model, api_key, system_prompt, description, config_json, avatar_url, created_at"#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(&write.name)
        .bind(&write.slug)
        .bind(&write.provider)
        .bind(&write.model)
        .bind(&write.api_key)
        .bind(&write.system_prompt)
        .bind(&write.description)
        .bind(&write.config_json)
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn create_library(&self, write: AgentWrite) -> AppResult<Agent> {
        let uuid = Uuid::new_v4();
        let db = sqlx::query_as::<_, AgentDb>(
            r#"INSERT INTO agents (id, company_id, name, slug, provider, model, api_key, system_prompt, description, config_json, avatar_url)
               VALUES ($1, NULL, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               RETURNING id, company_id, name, slug, provider, model, api_key, system_prompt, description, config_json, avatar_url, created_at"#,
        )
        .bind(uuid)
        .bind(&write.name)
        .bind(&write.slug)
        .bind(&write.provider)
        .bind(&write.model)
        .bind(&write.api_key)
        .bind(&write.system_prompt)
        .bind(&write.description)
        .bind(&write.config_json)
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(db.into())
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Agent>> {
        let db = sqlx::query_as::<_, AgentDb>(
            r#"SELECT id, company_id, name, slug, provider, model, api_key, system_prompt, description, config_json, avatar_url, created_at
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
            r#"SELECT a.id, a.company_id, a.name, a.slug, a.provider, a.model, a.api_key, a.system_prompt, a.description, a.config_json, a.avatar_url, a.created_at
               FROM agents a
               JOIN companies c ON c.id = a.company_id
               WHERE c.slug = $1 AND a.slug = $2"#,
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
            r#"SELECT id, company_id, name, slug, provider, model, api_key, system_prompt, description, config_json, avatar_url, created_at
               FROM agents WHERE company_id = $1
               ORDER BY created_at DESC, id DESC LIMIT 200"#,
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
    }

    async fn list_library(&self) -> AppResult<Vec<Agent>> {
        let rows = sqlx::query_as::<_, AgentDb>(
            r#"SELECT id, company_id, name, slug, provider, model, api_key, system_prompt, description, config_json, avatar_url, created_at
               FROM agents WHERE company_id IS NULL
               ORDER BY created_at DESC, id DESC LIMIT 200"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn update(&self, id: Uuid, write: AgentWrite) -> AppResult<Agent> {
        let db = sqlx::query_as::<_, AgentDb>(
            r#"UPDATE agents
               SET name = $1, slug = $2, provider = $3, model = $4, api_key = $5, system_prompt = $6, description = $7, config_json = $8, avatar_url = $9
               WHERE id = $10
               RETURNING id, company_id, name, slug, provider, model, api_key, system_prompt, description, config_json, avatar_url, created_at"#,
        )
        .bind(&write.name)
        .bind(&write.slug)
        .bind(&write.provider)
        .bind(&write.model)
        .bind(&write.api_key)
        .bind(&write.system_prompt)
        .bind(&write.description)
        .bind(&write.config_json)
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
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
            .map_err(|error| {
                if error
                    .as_database_error()
                    .and_then(|db| db.code())
                    .as_deref()
                    == Some("23503")
                {
                    AppError::Conflict(
                        "This library agent is assigned to one or more channels.".into(),
                    )
                } else {
                    AppError::from(error)
                }
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::agent::AgentWrite;
    use crate::use_cases::company::CompanyPersistence;
    use crate::use_cases::user::UserPersistence;
    use serde_json::json;

    #[tokio::test]
    async fn postgres_agent_persistence_works() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let persistence = PostgresPersistence::new(pool);

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

        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            "Agent Corp",
            "agent-corp",
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let config = json!({ "prompt": "System prompt" });

        let agent = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                name: "Support Agent".to_string(),
                slug: "support-agent".to_string(),
                provider: Some("openai".to_string()),
                model: Some("gpt-4o".to_string()),
                api_key: Some("key_123".to_string()),
                system_prompt: Some("You are a helpful support agent.".to_string()),
                description: Some("Answers customer support questions.".to_string()),
                config_json: Some(config.clone()),
                avatar_url: Some(AvatarUrl::from("https://example.com/support.png")),
            },
        )
        .await
        .unwrap();

        assert_eq!(agent.name, "Support Agent");
        assert_eq!(agent.slug, "support-agent");
        assert_eq!(agent.provider.as_deref(), Some("openai"));
        assert_eq!(agent.model.as_deref(), Some("gpt-4o"));
        assert_eq!(agent.api_key.as_deref(), Some("key_123"));
        assert_eq!(
            agent.system_prompt.as_deref(),
            Some("You are a helpful support agent.")
        );
        assert_eq!(agent.config_json, Some(config));
        assert_eq!(
            agent.avatar_url,
            Some(AvatarUrl::from("https://example.com/support.png"))
        );

        let fetched = AgentPersistence::get_by_id(&persistence, agent.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, agent.id);

        let list = AgentPersistence::list_by_company_id(&persistence, company.id)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        let updated = AgentPersistence::update(
            &persistence,
            agent.id,
            AgentWrite {
                name: "Support Agent V2".to_string(),
                slug: "support-agent-v2".to_string(),
                ..AgentWrite::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "Support Agent V2");
        assert_eq!(updated.api_key, None);
        // Clearing the field clears the picture -- a blank avatar box is how one is removed.
        assert_eq!(updated.avatar_url, None);

        AgentPersistence::delete(&persistence, agent.id)
            .await
            .unwrap();
        let list_after = AgentPersistence::list_by_company_id(&persistence, company.id)
            .await
            .unwrap();
        assert_eq!(list_after.len(), 0);

        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }
}
