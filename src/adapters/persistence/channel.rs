use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::Channel,
        creation::CreationProvenance,
        value_objects::{AvatarUrl, ChannelSlug, CompanySlug, EmailAddress},
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        channel::{ChannelPersistence, ChannelWrite},
    },
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct ChannelDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub slug: String,
    pub alias_slugs: Vec<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub enabled: bool,
    pub add_3rd_party: bool,
    pub retrieve_company_memory: bool,
    pub retrieve_agent_memory: bool,
    pub retrieve_user_memory: bool,
    pub persist_company_memory: bool,
    pub persist_agent_memory: bool,
    pub persist_user_memory: bool,
    pub created_by: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<ChannelDb> for Channel {
    type Error = AppError;

    fn try_from(db: ChannelDb) -> AppResult<Self> {
        Ok(Channel {
            id: db.id,
            company_id: db.company_id,
            name: db.name,
            description: db.description,
            slug: ChannelSlug::from(db.slug),
            alias_slugs: db.alias_slugs.into_iter().map(ChannelSlug::from).collect(),
            participant_emails: db
                .participant_emails
                .map(|emails| emails.into_iter().map(EmailAddress::from).collect()),
            agent_ids: db.agent_ids,
            enabled: db.enabled,
            add_3rd_party: db.add_3rd_party,
            retrieve_company_memory: db.retrieve_company_memory,
            retrieve_agent_memory: db.retrieve_agent_memory,
            retrieve_user_memory: db.retrieve_user_memory,
            persist_company_memory: db.persist_company_memory,
            persist_agent_memory: db.persist_agent_memory,
            persist_user_memory: db.persist_user_memory,
            created_by: serde_json::from_value(db.created_by).map_err(|err| {
                AppError::Internal(format!("Invalid channels.created_by provenance: {err}"))
            })?,
            created_at: db.created_at,
        })
    }
}

const CHANNEL_SELECT: &str = r#"
    SELECT ch.id, ch.company_id, ch.name, ch.description,
           (SELECT cs.slug::text FROM channel_slugs cs
            WHERE cs.channel_id = ch.id AND cs.is_primary) AS slug,
           COALESCE(
               (SELECT array_agg(cs.slug::text ORDER BY cs.slug::text)
                FROM channel_slugs cs
                WHERE cs.channel_id = ch.id AND NOT cs.is_primary),
               ARRAY[]::text[]) AS alias_slugs,
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
           ch.enabled, ch.add_3rd_party,
           ch.retrieve_company_memory, ch.retrieve_agent_memory, ch.retrieve_user_memory,
           ch.persist_company_memory, ch.persist_agent_memory, ch.persist_user_memory,
           ch.created_by, ch.created_at
    FROM channels ch
"#;

async fn load_channel(persistence: &PostgresPersistence, id: Uuid) -> AppResult<Option<Channel>> {
    let query = format!("{CHANNEL_SELECT} WHERE ch.id = $1");
    let db = sqlx::query_as::<_, ChannelDb>(&query)
        .bind(id)
        .fetch_optional(&persistence.pool)
        .await
        .map_err(AppError::from)?;

    db.map(TryInto::try_into).transpose()
}

/// Write a channel's canonical slug plus its aliases into the shared per-company slug namespace.
///
/// One statement per slug so a `UNIQUE (company_id, slug)` violation can name the address that
/// actually collided; the caller's transaction rolls the partial write back.
async fn insert_channel_slugs(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    slug: &str,
    alias_slugs: &[String],
) -> AppResult<()> {
    for (candidate, is_primary) in
        std::iter::once((slug, true)).chain(alias_slugs.iter().map(|alias| (alias.as_str(), false)))
    {
        sqlx::query(
            r#"INSERT INTO channel_slugs (company_id, channel_id, slug, is_primary)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(candidate)
        .bind(is_primary)
        .execute(&mut **tx)
        .await
        .map_err(|err| slug_conflict_error(err, candidate))?;
    }

    Ok(())
}

/// A taken address is routine user input, not a database fault, so it surfaces as a bad request
/// with the offending slug rather than a raw driver message.
fn slug_conflict_error(err: sqlx::Error, slug: &str) -> AppError {
    if let sqlx::Error::Database(ref db_err) = err
        && db_err.code().as_deref() == Some("23505")
    {
        return AppError::BadRequest(format!(
            "Address '{slug}' is already in use by another channel in this company."
        ));
    }
    AppError::from(err)
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
    async fn create(&self, company_id: Uuid, write: ChannelWrite) -> AppResult<Channel> {
        let uuid = Uuid::new_v4();
        let (access_mode, participants) = channel_access(write.participant_emails);
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        sqlx::query(
            r#"INSERT INTO channels (
                    id, company_id, name, description, access_mode, enabled, add_3rd_party, created_by,
                    retrieve_company_memory, retrieve_agent_memory, retrieve_user_memory,
                    persist_company_memory, persist_agent_memory, persist_user_memory
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                         $9, $10, $11, $12, $13, $14)"#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(write.name)
        .bind(write.description)
        .bind(access_mode)
        .bind(write.enabled)
        .bind(write.add_3rd_party)
        .bind(
            serde_json::to_value(write.created_by.unwrap_or_else(CreationProvenance::system))
                .map_err(|e| AppError::Internal(e.to_string()))?,
        )
        .bind(write.retrieve_company_memory)
        .bind(write.retrieve_agent_memory)
        .bind(write.retrieve_user_memory)
        .bind(write.persist_company_memory)
        .bind(write.persist_agent_memory)
        .bind(write.persist_user_memory)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        insert_channel_slugs(&mut tx, company_id, uuid, &write.slug, &write.alias_slugs).await?;

        for email in participants {
            sqlx::query("INSERT INTO channel_participants (channel_id, email) VALUES ($1, $2)")
                .bind(uuid)
                .bind(email)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
        }

        for (position, agent_id) in write.agent_ids.unwrap_or_default().into_iter().enumerate() {
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
        load_channel(self, uuid)
            .await?
            .ok_or_else(|| AppError::Internal("Created channel was not found".into()))
    }

    async fn create_with_agent(
        &self,
        company_id: Uuid,
        agent: AgentWrite,
        channel: ChannelWrite,
    ) -> AppResult<(Agent, Channel)> {
        let agent_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let run_timeout_secs = agent
            .run_timeout_secs
            .map(i32::try_from)
            .transpose()
            .map_err(|_| AppError::BadRequest("Agent run timeout is too large.".into()))?;
        let agent_created_by =
            serde_json::to_value(agent.created_by.unwrap_or_else(CreationProvenance::system))
                .map_err(|error| AppError::Internal(error.to_string()))?;
        let channel_created_by = serde_json::to_value(
            channel
                .created_by
                .clone()
                .unwrap_or_else(CreationProvenance::system),
        )
        .map_err(|error| AppError::Internal(error.to_string()))?;
        let (access_mode, participants) = channel_access(channel.participant_emails.clone());
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        sqlx::query(
            r#"INSERT INTO agents
               (id, company_id, name, slug, provider, model, system_prompt, description,
                config_json, avatar_url, created_by, run_timeout_secs,
                memory_persistence_mode, memory_recall_mode, memory_max_results)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)"#,
        )
        .bind(agent_id)
        .bind(company_id)
        .bind(&agent.name)
        .bind(&agent.slug)
        .bind(&agent.provider)
        .bind(&agent.model)
        .bind(&agent.system_prompt)
        .bind(&agent.description)
        .bind(&agent.config_json)
        .bind(agent.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(agent_created_by)
        .bind(run_timeout_secs)
        .bind(agent.memory_persistence_mode.as_str())
        .bind(agent.memory_recall_mode.as_str())
        .bind(i16::from(agent.memory_max_results))
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        sqlx::query(
            r#"INSERT INTO channels (
                    id, company_id, name, description, access_mode, enabled, add_3rd_party,
                    created_by, retrieve_company_memory, retrieve_agent_memory,
                    retrieve_user_memory, persist_company_memory, persist_agent_memory,
                    persist_user_memory)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"#,
        )
        .bind(channel_id)
        .bind(company_id)
        .bind(&channel.name)
        .bind(&channel.description)
        .bind(access_mode)
        .bind(channel.enabled)
        .bind(channel.add_3rd_party)
        .bind(channel_created_by)
        .bind(channel.retrieve_company_memory)
        .bind(channel.retrieve_agent_memory)
        .bind(channel.retrieve_user_memory)
        .bind(channel.persist_company_memory)
        .bind(channel.persist_agent_memory)
        .bind(channel.persist_user_memory)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        insert_channel_slugs(
            &mut tx,
            company_id,
            channel_id,
            &channel.slug,
            &channel.alias_slugs,
        )
        .await?;
        for email in participants {
            sqlx::query("INSERT INTO channel_participants (channel_id, email) VALUES ($1, $2)")
                .bind(channel_id)
                .bind(email)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
        }
        let mut assigned = vec![agent_id];
        assigned.extend(
            channel
                .agent_ids
                .unwrap_or_default()
                .into_iter()
                .filter(|id| *id != agent_id),
        );
        for (position, assigned_agent_id) in assigned.into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO channel_agents (company_id, channel_id, agent_id, position) VALUES ($1, $2, $3, $4)",
            )
            .bind(company_id)
            .bind(channel_id)
            .bind(assigned_agent_id)
            .bind(position as i32)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }
        tx.commit().await.map_err(AppError::from)?;

        let agent = AgentPersistence::get_by_id(self, agent_id)
            .await?
            .ok_or_else(|| AppError::Internal("Created agent was not found".into()))?;
        let channel = load_channel(self, channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Created channel was not found".into()))?;
        Ok((agent, channel))
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
        load_channel(self, id).await
    }

    async fn get_by_company_slug_and_channel_slug(
        &self,
        company_slug: &CompanySlug,
        channel_slug: &ChannelSlug,
    ) -> AppResult<Option<Channel>> {
        let query = format!(
            "{CHANNEL_SELECT} JOIN companies c ON c.id = ch.company_id \
             JOIN channel_slugs cs_lookup ON cs_lookup.channel_id = ch.id \
             WHERE c.slug = $1 AND cs_lookup.slug = $2"
        );
        let db = sqlx::query_as::<_, ChannelDb>(&query)
            .bind(company_slug.as_str())
            .bind(channel_slug.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
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

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn update(&self, id: Uuid, write: ChannelWrite) -> AppResult<Channel> {
        let (access_mode, participants) = channel_access(write.participant_emails);
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let result = sqlx::query(
            r#"UPDATE channels
               SET name = $1, description = $2, access_mode = $3, enabled = $4,
                   add_3rd_party = $5, retrieve_company_memory = $6,
                   retrieve_agent_memory = $7, retrieve_user_memory = $8,
                   persist_company_memory = $9, persist_agent_memory = $10,
                   persist_user_memory = $11
               WHERE id = $12"#,
        )
        .bind(write.name)
        .bind(write.description)
        .bind(access_mode)
        .bind(write.enabled)
        .bind(write.add_3rd_party)
        .bind(write.retrieve_company_memory)
        .bind(write.retrieve_agent_memory)
        .bind(write.retrieve_user_memory)
        .bind(write.persist_company_memory)
        .bind(write.persist_agent_memory)
        .bind(write.persist_user_memory)
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
        sqlx::query("DELETE FROM channel_slugs WHERE channel_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;

        let company_id: Uuid = sqlx::query_scalar("SELECT company_id FROM channels WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::from)?;
        insert_channel_slugs(&mut tx, company_id, id, &write.slug, &write.alias_slugs).await?;

        for email in participants {
            sqlx::query("INSERT INTO channel_participants (channel_id, email) VALUES ($1, $2)")
                .bind(id)
                .bind(email)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
        }

        for (position, agent_id) in write.agent_ids.unwrap_or_default().into_iter().enumerate() {
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
        load_channel(self, id)
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
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::agent::AgentPersistence;
    use crate::use_cases::agent::AgentWrite;
    use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
    use crate::use_cases::user::UserPersistence;

    #[tokio::test]
    async fn postgres_channel_persistence_works() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let persistence = PostgresPersistence::new(pool);

        // Create owner & company. The company slug is unique database-wide, so it carries the same
        // per-run suffix as the owner — a fixed slug collides with any concurrent run of this test.
        let suffix = Uuid::new_v4().simple().to_string();
        let owner_username = format!("owner_{suffix}");
        let owner_email = format!("{owner_username}@example.com");
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
            CompanyWrite {
                name: "Channel Corp".to_string(),
                slug: format!("ch-corp-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        // 1. Create Channel
        let emails = vec!["a@example.com".to_string(), "b@example.com".to_string()];

        let agent1 = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                name: "Primary Agent".to_string(),
                slug: "primary-agent".to_string(),
                ..AgentWrite::default()
            },
        )
        .await
        .unwrap();
        let agent2 = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                name: "Secondary Agent".to_string(),
                slug: "secondary-agent".to_string(),
                ..AgentWrite::default()
            },
        )
        .await
        .unwrap();
        let agent_ids = vec![agent1.id, agent2.id];

        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Inbound Email".into(),
                description: Some("Takes support mail from the website form.".into()),
                slug: "inbound-email".into(),
                alias_slugs: Vec::new(),
                participant_emails: Some(emails.clone()),
                agent_ids: Some(agent_ids.clone()),
                enabled: true,
                // Deliberately the opposite of `enabled`, so a swapped pair of same-typed binds
                // cannot pass this test.
                add_3rd_party: false,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(channel.name, "Inbound Email");
        assert_eq!(
            channel.description.as_deref(),
            Some("Takes support mail from the website form.")
        );
        assert_eq!(channel.slug, "inbound-email");
        assert_eq!(
            channel.participant_emails,
            Some(
                emails
                    .into_iter()
                    .map(EmailAddress::from)
                    .collect::<Vec<_>>()
            )
        );
        assert_eq!(channel.agent_ids, Some(agent_ids));
        assert!(channel.enabled);
        assert!(!channel.add_3rd_party);

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
            ChannelWrite {
                name: "Inbound Email V2".into(),
                description: Some("Now also handles refund requests.".into()),
                slug: "inbound-email-v2".into(),
                enabled: false,
                add_3rd_party: true,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: None,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "Inbound Email V2");
        assert_eq!(
            updated.description.as_deref(),
            Some("Now also handles refund requests."),
            "an edited description must survive a round trip"
        );
        assert_eq!(updated.participant_emails, None);
        assert!(!updated.enabled, "the off switch must survive a round trip");
        assert!(
            updated.add_3rd_party,
            "the third-party switch must survive a round trip"
        );

        let reread = ChannelPersistence::get_by_id(&persistence, channel.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!reread.enabled);
        assert!(reread.add_3rd_party);
        assert_eq!(
            reread.description.as_deref(),
            Some("Now also handles refund requests.")
        );

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

    #[tokio::test]
    async fn agent_and_enabled_channel_creation_is_atomic() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("atomic-channel-{suffix}@example.com");
        let user = persistence
            .create_user(&format!("atomic-channel-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            user.id,
            CompanyWrite {
                name: "Atomic Channel Corp".into(),
                slug: format!("atomic-channel-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Taken".into(),
                slug: "taken".into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let before = AgentPersistence::list_by_company_id(&persistence, company.id)
            .await
            .unwrap()
            .len();
        let failed = ChannelPersistence::create_with_agent(
            &persistence,
            company.id,
            AgentWrite {
                name: "Rolled back".into(),
                slug: "rolled-back".into(),
                ..AgentWrite::default()
            },
            ChannelWrite {
                name: "Collision".into(),
                slug: "taken".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await;
        assert!(failed.is_err());
        assert_eq!(
            AgentPersistence::list_by_company_id(&persistence, company.id)
                .await
                .unwrap()
                .len(),
            before,
            "a failed channel insert must roll its new agent back"
        );

        let (agent, channel) = ChannelPersistence::create_with_agent(
            &persistence,
            company.id,
            AgentWrite {
                name: "Atomic agent".into(),
                slug: "atomic-agent".into(),
                ..AgentWrite::default()
            },
            ChannelWrite {
                name: "Atomic channel".into(),
                slug: "atomic".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        assert!(channel.enabled);
        assert_eq!(channel.agent_ids, Some(vec![agent.id]));
    }

    /// Aliases and canonical slugs share one namespace per company, enforced by the database.
    #[tokio::test]
    async fn alias_slugs_are_unique_within_a_company_and_free_across_companies() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);

        let owner_username = format!("owner_{}", Uuid::new_v4().simple());
        let owner_email = format!("{owner_username}@example.com");
        let _ = persistence
            .create_user(&owner_username, &owner_email, "hash")
            .await;
        let owner = persistence
            .get_by_email(&owner_email)
            .await
            .unwrap()
            .unwrap();

        let company_slug = format!("alias-co-{}", Uuid::new_v4().simple());
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Alias Corp".to_string(),
                slug: company_slug.to_string(),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        let other_slug = format!("other-co-{}", Uuid::new_v4().simple());
        let other_company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Other Corp".to_string(),
                slug: other_slug.to_string(),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        let write = |slug: &str, aliases: &[&str]| ChannelWrite {
            name: "Support".into(),
            slug: slug.into(),
            alias_slugs: aliases.iter().map(|a| (*a).to_string()).collect(),
            enabled: false,
            ..ChannelWrite::default()
        };

        let support = ChannelPersistence::create(
            &persistence,
            company.id,
            write("support", &["sales", "help"]),
        )
        .await
        .unwrap();
        assert_eq!(support.slug, "support");
        assert_eq!(support.alias_slugs, ["help", "sales"]);

        // An alias is a real address: looking it up finds the channel that owns it.
        let by_alias = ChannelPersistence::get_by_company_slug_and_channel_slug(
            &persistence,
            &CompanySlug::from(company_slug.clone()),
            &ChannelSlug::from("sales"),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(by_alias.id, support.id);

        // A second channel may not claim a taken alias, nor collide with it via its own slug.
        let alias_conflict =
            ChannelPersistence::create(&persistence, company.id, write("billing", &["sales"]))
                .await;
        assert!(
            matches!(alias_conflict, Err(AppError::BadRequest(ref m)) if m.contains("sales")),
            "expected a named conflict, got {alias_conflict:?}"
        );
        let slug_conflict =
            ChannelPersistence::create(&persistence, company.id, write("help", &[])).await;
        assert!(
            matches!(slug_conflict, Err(AppError::BadRequest(ref m)) if m.contains("help")),
            "expected a named conflict, got {slug_conflict:?}"
        );

        // The failed writes rolled back whole, so neither left a partial channel behind.
        let channels = ChannelPersistence::list_by_company_id(&persistence, company.id)
            .await
            .unwrap();
        assert_eq!(channels.len(), 1);

        // The namespace is per company, so another company may use the same names.
        ChannelPersistence::create(&persistence, other_company.id, write("sales", &["support"]))
            .await
            .unwrap();

        // An update replaces the alias set wholesale, freeing the names it drops.
        let updated =
            ChannelPersistence::update(&persistence, support.id, write("support", &["contact"]))
                .await
                .unwrap();
        assert_eq!(updated.alias_slugs, ["contact"]);
        ChannelPersistence::create(&persistence, company.id, write("sales", &[]))
            .await
            .expect("a released alias is available again");

        let _ = CompanyPersistence::delete(&persistence, company.id).await;
        let _ = CompanyPersistence::delete(&persistence, other_company.id).await;
    }
}
