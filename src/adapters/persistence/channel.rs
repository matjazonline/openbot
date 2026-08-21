use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        channel::Channel,
        value_objects::{ChannelSlug, CompanySlug, EmailAddress},
    },
    use_cases::channel::{ChannelPersistence, ChannelWrite},
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct ChannelDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub slug: String,
    pub alias_slugs: Vec<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub channel_config: Option<serde_json::Value>,
    pub enabled: bool,
    pub add_3rd_party: bool,
    pub created_at: DateTime<Utc>,
}

impl From<ChannelDb> for Channel {
    fn from(db: ChannelDb) -> Self {
        Channel {
            id: db.id,
            company_id: db.company_id,
            name: db.name,
            slug: ChannelSlug::from(db.slug),
            alias_slugs: db.alias_slugs.into_iter().map(ChannelSlug::from).collect(),
            api_key: db.api_key,
            provider: db.provider,
            model: db.model,
            participant_emails: db
                .participant_emails
                .map(|emails| emails.into_iter().map(EmailAddress::from).collect()),
            agent_ids: db.agent_ids,
            channel_config: db.channel_config,
            enabled: db.enabled,
            add_3rd_party: db.add_3rd_party,
            created_at: db.created_at,
        }
    }
}

const CHANNEL_SELECT: &str = r#"
    SELECT ch.id, ch.company_id, ch.name,
           (SELECT cs.slug::text FROM channel_slugs cs
            WHERE cs.channel_id = ch.id AND cs.is_primary) AS slug,
           COALESCE(
               (SELECT array_agg(cs.slug::text ORDER BY cs.slug::text)
                FROM channel_slugs cs
                WHERE cs.channel_id = ch.id AND NOT cs.is_primary),
               ARRAY[]::text[]) AS alias_slugs,
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
           ch.channel_config, ch.enabled, ch.add_3rd_party, ch.created_at
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
                    id, company_id, name, access_mode, api_key, provider, model,
                    channel_config, enabled, add_3rd_party
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
        )
        .bind(uuid)
        .bind(company_id)
        .bind(write.name)
        .bind(access_mode)
        .bind(write.api_key)
        .bind(write.provider)
        .bind(write.model)
        .bind(write.channel_config)
        .bind(write.enabled)
        .bind(write.add_3rd_party)
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
        load_channel(&self.pool, uuid)
            .await?
            .ok_or_else(|| AppError::Internal("Created channel was not found".into()))
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
        load_channel(&self.pool, id).await
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

    async fn update(&self, id: Uuid, write: ChannelWrite) -> AppResult<Channel> {
        let (access_mode, participants) = channel_access(write.participant_emails);
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let result = sqlx::query(
            r#"UPDATE channels
               SET name = $1, access_mode = $2, api_key = $3,
                   provider = $4, model = $5, channel_config = $6, enabled = $7,
                   add_3rd_party = $8
               WHERE id = $9"#,
        )
        .bind(write.name)
        .bind(access_mode)
        .bind(write.api_key)
        .bind(write.provider)
        .bind(write.model)
        .bind(write.channel_config)
        .bind(write.enabled)
        .bind(write.add_3rd_party)
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
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::agent::AgentPersistence;
    use crate::use_cases::agent::AgentWrite;
    use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
    use crate::use_cases::user::UserPersistence;
    use serde_json::json;

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
        let config = json!({ "key": "value" });

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
                slug: "inbound-email".into(),
                alias_slugs: Vec::new(),
                api_key: Some("ch_key_123".into()),
                provider: Some("openai".into()),
                model: Some("gpt-4o".into()),
                participant_emails: Some(emails.clone()),
                agent_ids: Some(agent_ids.clone()),
                channel_config: Some(config.clone()),
                enabled: true,
                // Deliberately the opposite of `enabled`, so a swapped pair of same-typed binds
                // cannot pass this test.
                add_3rd_party: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(channel.name, "Inbound Email");
        assert_eq!(channel.slug, "inbound-email");
        assert_eq!(channel.api_key.as_deref(), Some("ch_key_123"));
        assert_eq!(channel.provider.as_deref(), Some("openai"));
        assert_eq!(channel.model.as_deref(), Some("gpt-4o"));
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
        assert_eq!(channel.channel_config, Some(config));
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
                slug: "inbound-email-v2".into(),
                enabled: false,
                add_3rd_party: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "Inbound Email V2");
        assert_eq!(updated.api_key, None);
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
            enabled: true,
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
