use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::HashSet;
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

const CHANNEL_SELECT: &str = r#"
    SELECT ch.id, ch.company_id, ch.name, ch.slug::text AS slug,
           ch.api_key, ch.provider, ch.model,
           CASE ch.access_mode
               WHEN 'public' THEN ARRAY['@public']::text[] || COALESCE(
                   (SELECT array_agg(cp.email::text ORDER BY cp.email::text)
                    FROM channel_participants cp WHERE cp.channel_id = ch.id),
                   ARRAY[]::text[])
               WHEN 'allowlist' THEN COALESCE(
                   (SELECT array_agg(cp.email::text ORDER BY cp.email::text)
                    FROM channel_participants cp WHERE cp.channel_id = ch.id),
                   ARRAY[]::text[])
               ELSE NULL::text[]
           END AS participant_emails,
           (SELECT array_agg(ca.agent_id ORDER BY ca.position)
            FROM channel_agents ca WHERE ca.channel_id = ch.id) AS agent_ids,
           ch.channel_config, ch.created_at
    FROM channels ch
"#;

async fn load_channel(pool: &PgPool, id: Uuid) -> AppResult<Option<Channel>> {
    let query = format!("{CHANNEL_SELECT} WHERE ch.id = $1");
    let db = sqlx::query_as::<_, ChannelDb>(&query)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?;

    Ok(db.map(Into::into))
}

fn channel_access(participant_emails: Option<Vec<String>>) -> (&'static str, Vec<String>) {
    let mut seen = HashSet::new();
    let mut participants = Vec::new();
    let mut is_public = false;

    for email in participant_emails.unwrap_or_default() {
        let normalized = email.trim().to_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if normalized == "@public" {
            is_public = true;
        } else if seen.insert(normalized.clone()) {
            participants.push(normalized);
        }
    }

    let mode = if is_public {
        "public"
    } else if participants.is_empty() {
        "team"
    } else {
        "allowlist"
    };

    (mode, participants)
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
        let (access_mode, participants) = channel_access(participant_emails);
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        sqlx::query(
            r#"INSERT INTO channels (
                    id, company_id, name, slug, access_mode, api_key, provider, model, channel_config
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(name)
        .bind(slug)
        .bind(access_mode)
        .bind(api_key)
        .bind(provider)
        .bind(model)
        .bind(channel_config)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        for email in participants {
            sqlx::query("INSERT INTO channel_participants (channel_id, email) VALUES ($1, $2)")
                .bind(uuid)
                .bind(email)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
        }

        for (position, agent_id) in agent_ids.unwrap_or_default().into_iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO channel_agents (company_id, channel_id, agent_id, position)
                   VALUES ($1, $2, $3, $4)"#,
            )
            .bind(company_id)
            .bind(uuid)
            .bind(agent_id)
            .bind(position as i32)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        tx.commit().await.map_err(AppError::from)?;
        load_channel(&self.pool, uuid)
            .await?
            .ok_or_else(|| AppError::Internal("Created channel was not found".into()))
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
        load_channel(&self.pool, id).await
    }

    async fn get_by_company_slug_and_channel_slug(
        &self,
        company_slug: &str,
        channel_slug: &str,
    ) -> AppResult<Option<Channel>> {
        let query = format!(
            "{CHANNEL_SELECT} JOIN companies c ON c.id = ch.company_id \
             WHERE c.slug = $1 AND ch.slug = $2"
        );
        let db = sqlx::query_as::<_, ChannelDb>(&query)
            .bind(company_slug)
            .bind(channel_slug)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
        let query = format!(
            "{CHANNEL_SELECT} WHERE ch.company_id = $1 ORDER BY ch.created_at DESC, ch.id DESC"
        );
        let db_list = sqlx::query_as::<_, ChannelDb>(&query)
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
        let (access_mode, participants) = channel_access(participant_emails);
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let result = sqlx::query(
            r#"UPDATE channels
               SET name = $1, slug = $2, access_mode = $3, api_key = $4,
                   provider = $5, model = $6, channel_config = $7
               WHERE id = $8"#,
        )
        .bind(name)
        .bind(slug)
        .bind(access_mode)
        .bind(api_key)
        .bind(provider)
        .bind(model)
        .bind(channel_config)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        if result.rows_affected() == 0 {
            return Err(AppError::Internal("Channel not found".into()));
        }

        sqlx::query("DELETE FROM channel_participants WHERE channel_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        sqlx::query("DELETE FROM channel_agents WHERE channel_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;

        for email in participants {
            sqlx::query("INSERT INTO channel_participants (channel_id, email) VALUES ($1, $2)")
                .bind(id)
                .bind(email)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
        }

        for (position, agent_id) in agent_ids.unwrap_or_default().into_iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO channel_agents (company_id, channel_id, agent_id, position)
                   SELECT company_id, id, $2, $3 FROM channels WHERE id = $1"#,
            )
            .bind(id)
            .bind(agent_id)
            .bind(position as i32)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        tx.commit().await.map_err(AppError::from)?;
        load_channel(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Updated channel was not found".into()))
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
    use crate::use_cases::agent::AgentPersistence;
    use crate::use_cases::company::CompanyPersistence;
    use crate::use_cases::user::UserPersistence;
    use serde_json::json;

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
            "Channel Corp",
            "ch-corp",
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // 1. Create Channel
        let emails = vec!["a@example.com".to_string(), "b@example.com".to_string()];
        let config = json!({ "key": "value" });

        let agent1 = AgentPersistence::create(
            &persistence,
            company.id,
            "Primary Agent",
            "primary-agent",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let agent2 = AgentPersistence::create(
            &persistence,
            company.id,
            "Secondary Agent",
            "secondary-agent",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let agent_ids = vec![agent1.id, agent2.id];

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
        let fetched = ChannelPersistence::get_by_id(&persistence, channel.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, channel.id);

        // 3. List by company ID
        let list = ChannelPersistence::list_by_company_id(&persistence, company.id)
            .await
            .unwrap();
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
        ChannelPersistence::delete(&persistence, channel.id)
            .await
            .unwrap();
        let list_after = ChannelPersistence::list_by_company_id(&persistence, company.id)
            .await
            .unwrap();
        assert_eq!(list_after.len(), 0);

        // Cleanup
        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }
}
