use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::channel::Channel,
    use_cases::channel::ChannelPersistence,
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct ChannelDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub slug: String,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub channel_config: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
}

impl From<ChannelDb> for Channel {
    fn from(db: ChannelDb) -> Self {
        Channel {
            id: db.id,
            company_id: db.company_id,
            name: db.name,
            slug: db.slug,
            api_key: db.api_key,
            provider: db.provider,
            model: db.model,
            participant_emails: db.participant_emails,
            agent_ids: db.agent_ids,
            channel_config: db.channel_config,
            created_at: db.created_at,
        }
    }
}

#[async_trait]
impl ChannelPersistence for PostgresPersistence {
    async fn create(
        &self,
        company_id: Uuid,
        name: &str,
        slug: &str,
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        participant_emails: Option<Vec<String>>,
        agent_ids: Option<Vec<Uuid>>,
        channel_config: Option<serde_json::Value>,
    ) -> AppResult<Channel> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as::<_, ChannelDb>(
            r#"INSERT INTO channels (id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, channel_config)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               RETURNING id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, channel_config, created_at"#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(name)
        .bind(slug)
        .bind(api_key)
        .bind(provider)
        .bind(model)
        .bind(participant_emails.as_deref())
        .bind(agent_ids.as_deref())
        .bind(channel_config)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
        let db = sqlx::query_as::<_, ChannelDb>(
            r#"SELECT id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, channel_config, created_at
               FROM channels WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn get_by_company_slug_and_channel_slug(
        &self,
        company_slug: &str,
        channel_slug: &str,
    ) -> AppResult<Option<Channel>> {
        let db = sqlx::query_as::<_, ChannelDb>(
            r#"SELECT ch.id, ch.company_id, ch.name, ch.slug, ch.api_key, ch.provider, ch.model, ch.participant_emails, ch.agent_ids, ch.channel_config, ch.created_at
               FROM channels ch
               JOIN companies c ON c.id = ch.company_id
               WHERE LOWER(c.slug) = LOWER($1) AND LOWER(ch.slug) = LOWER($2)"#,
        )
        .bind(company_slug)
        .bind(channel_slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
        let db_list = sqlx::query_as::<_, ChannelDb>(
            r#"SELECT id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, channel_config, created_at
               FROM channels WHERE company_id = $1 ORDER BY created_at DESC"#,
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
        api_key: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        participant_emails: Option<Vec<String>>,
        agent_ids: Option<Vec<Uuid>>,
        channel_config: Option<serde_json::Value>,
    ) -> AppResult<Channel> {
        let db = sqlx::query_as::<_, ChannelDb>(
            r#"UPDATE channels
               SET name = $1, slug = $2, api_key = $3, provider = $4, model = $5, participant_emails = $6, agent_ids = $7, channel_config = $8
               WHERE id = $9
               RETURNING id, company_id, name, slug, api_key, provider, model, participant_emails, agent_ids, channel_config, created_at"#,
        )
        .bind(name)
        .bind(slug)
        .bind(api_key)
        .bind(provider)
        .bind(model)
        .bind(participant_emails.as_deref())
        .bind(agent_ids.as_deref())
        .bind(channel_config)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        sqlx::query("DELETE FROM channels WHERE id = $1")
            .bind(id)
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
    async fn postgres_channel_persistence_works() {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => return, // Skip test if DATABASE_URL is not set
        };

        let pool = match sqlx::PgPool::connect(&database_url).await {
            Ok(p) => p,
            Err(_) => return,
        };

        let persistence = PostgresPersistence::new(pool);

        // Create owner & company
        let owner_username = format!("owner_{}", Uuid::new_v4().simple());
        let owner_email = format!("{}@example.com", owner_username);
        let _ = persistence.create_user(&owner_username, &owner_email, "hash").await;
        let owner = persistence.get_by_email(&owner_email).await.unwrap().unwrap();

        let company = CompanyPersistence::create(&persistence, owner.id, "Channel Corp", "ch-corp", None, None, None, None)
            .await
            .unwrap();

        // 1. Create Channel
        let emails = vec!["a@example.com".to_string(), "b@example.com".to_string()];
        let config = json!({ "key": "value" });

        let agent_id1 = Uuid::new_v4();
        let agent_id2 = Uuid::new_v4();
        let agent_ids = vec![agent_id1, agent_id2];

        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            "Inbound Email",
            "inbound-email",
            Some("ch_key_123"),
            Some("openai"),
            Some("gpt-4o"),
            Some(emails.clone()),
            Some(agent_ids.clone()),
            Some(config.clone()),
        )
        .await
        .unwrap();

        assert_eq!(channel.name, "Inbound Email");
        assert_eq!(channel.slug, "inbound-email");
        assert_eq!(channel.api_key.as_deref(), Some("ch_key_123"));
        assert_eq!(channel.provider.as_deref(), Some("openai"));
        assert_eq!(channel.model.as_deref(), Some("gpt-4o"));
        assert_eq!(channel.participant_emails, Some(emails));
        assert_eq!(channel.agent_ids, Some(agent_ids));
        assert_eq!(channel.channel_config, Some(config));

        // 2. Get by ID
        let fetched = ChannelPersistence::get_by_id(&persistence, channel.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, channel.id);

        // 3. List by company ID
        let list = ChannelPersistence::list_by_company_id(&persistence, company.id).await.unwrap();
        assert_eq!(list.len(), 1);

        // 4. Update
        let updated = ChannelPersistence::update(
            &persistence,
            channel.id,
            "Inbound Email V2",
            "inbound-email-v2",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "Inbound Email V2");
        assert_eq!(updated.api_key, None);
        assert_eq!(updated.participant_emails, None);

        // 5. Delete
        ChannelPersistence::delete(&persistence, channel.id).await.unwrap();
        let list_after = ChannelPersistence::list_by_company_id(&persistence, company.id).await.unwrap();
        assert_eq!(list_after.len(), 0);

        // Cleanup
        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }
}
