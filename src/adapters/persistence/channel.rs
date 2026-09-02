use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{collections::HashSet, str::FromStr};
use uuid::Uuid;

use crate::adapters::persistence::integration::email_binding::{
    CanonicalEmailBinding, write_canonical_email_binding,
};
use crate::adapters::persistence::participant::{
    create_agent_principal_on, resolve_or_create_external_identity_on,
};
use crate::adapters::protocols::email::EmailIdentity;
use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::{Channel, ChannelAccessMode},
        creation::CreationProvenance,
        participant::{IdentityClaimMetadata, IdentityProvenance, PrincipalCapability},
        value_objects::{AvatarUrl, ChannelSlug, CompanySlug, EmailAddress},
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite, OwnedAgentChannelPersistence},
        channel::{ChannelPersistence, ChannelWrite},
        participant::{IdentityObservation, IdentityResolution},
    },
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct ChannelDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub owner_agent_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub slug: String,
    pub alias_slugs: Vec<String>,
    pub participant_emails: Option<Vec<String>>,
    pub access_mode: String,
    pub principal_grants: serde_json::Value,
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
            owner_agent_id: db.owner_agent_id,
            name: db.name,
            description: db.description,
            slug: ChannelSlug::from(db.slug),
            alias_slugs: db.alias_slugs.into_iter().map(ChannelSlug::from).collect(),
            participant_emails: db
                .participant_emails
                .map(|emails| emails.into_iter().map(EmailAddress::from).collect()),
            access_mode: ChannelAccessMode::from_str(&db.access_mode)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            principal_grants: serde_json::from_value(db.principal_grants).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid channel principal grants for {}: {error}",
                    db.id
                ))
            })?,
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
    SELECT ch.id, ch.company_id, ch.owner_agent_id, ch.name, ch.description,
           (SELECT cs.slug::text FROM channel_slugs cs
            WHERE cs.channel_id = ch.id AND cs.is_primary) AS slug,
           COALESCE(
               (SELECT array_agg(cs.slug::text ORDER BY cs.slug::text)
                FROM channel_slugs cs
                WHERE cs.channel_id = ch.id AND NOT cs.is_primary),
               ARRAY[]::text[]) AS alias_slugs,
           CASE ch.access_mode
               WHEN 'public' THEN ARRAY['@public']::text[] || COALESCE(
                   (SELECT array_agg(identity.subject ORDER BY identity.subject)
                    FROM channel_principal_grants AS channel_grant
                    JOIN participant_identities AS identity
                      ON (identity.company_id, identity.principal_id) =
                         (channel_grant.company_id, channel_grant.principal_id)
                    WHERE channel_grant.company_id = ch.company_id AND channel_grant.channel_id = ch.id
                      AND channel_grant.capability = 'participate'
                      AND channel_grant.provenance = 'email_allowlist'
                      AND identity.transport = 'email' AND identity.status <> 'disabled'),
                   ARRAY[]::text[])
               WHEN 'allowlist' THEN COALESCE(
                   (SELECT array_agg(identity.subject ORDER BY identity.subject)
                    FROM channel_principal_grants AS channel_grant
                    JOIN participant_identities AS identity
                      ON (identity.company_id, identity.principal_id) =
                         (channel_grant.company_id, channel_grant.principal_id)
                    WHERE channel_grant.company_id = ch.company_id AND channel_grant.channel_id = ch.id
                      AND channel_grant.capability = 'participate'
                      AND channel_grant.provenance = 'email_allowlist'
                      AND identity.transport = 'email' AND identity.status <> 'disabled'),
                   ARRAY[]::text[])
               ELSE NULL::text[]
           END AS participant_emails,
           ch.access_mode,
           COALESCE(
               (SELECT jsonb_agg(jsonb_build_object(
                           'principal_id', channel_grant.principal_id,
                           'capability', channel_grant.capability,
                           'provenance', channel_grant.provenance,
                           'created_at', channel_grant.created_at)
                       ORDER BY channel_grant.created_at, channel_grant.principal_id, channel_grant.capability)
                FROM channel_principal_grants AS channel_grant
                WHERE channel_grant.company_id = ch.company_id AND channel_grant.channel_id = ch.id),
               '[]'::jsonb) AS principal_grants,
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

/// Turn the form's email allowlist into the channel's stored access mode and the addresses that
/// become principal grants. `@public` selects a mode; it is never a participant of its own.
fn channel_access(participant_emails: Option<Vec<String>>) -> (ChannelAccessMode, Vec<String>) {
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
        ChannelAccessMode::Public
    } else if participants.is_empty() {
        ChannelAccessMode::Team
    } else {
        ChannelAccessMode::Allowlist
    };

    (mode, participants)
}

pub(crate) async fn insert_email_allowlist_grants(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    participants: Vec<String>,
) -> AppResult<()> {
    for email in participants {
        let identity = EmailIdentity::parse(EmailAddress::from(email))
            .map(EmailIdentity::qualify_default)
            .map_err(|error| AppError::BadRequest(format!("Invalid participant email: {error}")))?;
        let IdentityResolution { principal, .. } = resolve_or_create_external_identity_on(
            transaction,
            company_id,
            IdentityObservation {
                identity,
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::ChannelAllowlist,
            },
        )
        .await?;

        for capability in [PrincipalCapability::Participate, PrincipalCapability::View] {
            sqlx::query(
                r#"INSERT INTO channel_principal_grants
                       (company_id, channel_id, principal_id, capability, provenance)
                   VALUES ($1, $2, $3, $4, 'email_allowlist')
                   ON CONFLICT (company_id, channel_id, principal_id, capability)
                   DO UPDATE SET provenance = EXCLUDED.provenance"#,
            )
            .bind(company_id)
            .bind(channel_id)
            .bind(principal.id.as_uuid())
            .bind(capability.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(AppError::from)?;
        }
    }
    Ok(())
}

/// Every channel owns one canonical email interface, written in the same transaction as the
/// channel itself so the two can never disagree.
///
/// The three creation paths and the update path all route through here rather than each building
/// their own binding: the endpoint key, access policy and delivery policy are one decision.
async fn write_channel_email_binding(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    write: &ChannelWrite,
) -> AppResult<()> {
    let channel_slug = ChannelSlug::new(write.slug.clone());
    let created_by = write
        .created_by
        .clone()
        .unwrap_or_else(CreationProvenance::system);
    write_canonical_email_binding(
        transaction,
        CanonicalEmailBinding {
            company_id,
            channel_id,
            channel_slug: &channel_slug,
            channel_name: &write.name,
            created_by: &created_by,
        },
    )
    .await
}

#[async_trait]
impl ChannelPersistence for PostgresPersistence {
    async fn create(&self, company_id: Uuid, write: ChannelWrite) -> AppResult<Channel> {
        let uuid = Uuid::new_v4();
        let (access_mode, participants) = channel_access(write.participant_emails.clone());
        let created_by = serde_json::to_value(
            write
                .created_by
                .clone()
                .unwrap_or_else(CreationProvenance::system),
        )
        .map_err(|error| AppError::Internal(error.to_string()))?;
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
        .bind(&write.name)
        .bind(&write.description)
        .bind(access_mode.as_str())
        .bind(write.enabled)
        .bind(write.add_3rd_party)
        .bind(created_by)
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

        write_channel_email_binding(&mut tx, company_id, uuid, &write).await?;

        insert_email_allowlist_grants(&mut tx, company_id, uuid, participants).await?;

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
                memory_enabled, memory_persistence_mode, memory_recall_mode, memory_max_results)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)"#,
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
        .bind(agent.memory_enabled)
        .bind(agent.memory_persistence_mode.as_str())
        .bind(agent.memory_recall_mode.as_str())
        .bind(i16::from(agent.memory_max_results))
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        create_agent_principal_on(&mut tx, company_id, agent_id, &agent.name).await?;

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
        .bind(access_mode.as_str())
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
        write_channel_email_binding(&mut tx, company_id, channel_id, &channel).await?;
        insert_email_allowlist_grants(&mut tx, company_id, channel_id, participants).await?;
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
        let (access_mode, participants) = channel_access(write.participant_emails.clone());
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
        .bind(&write.name)
        .bind(&write.description)
        .bind(access_mode.as_str())
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

        // Only the grants this form owns: a grant written by another provenance is not the email
        // allowlist's to replace.
        sqlx::query(
            r#"DELETE FROM channel_principal_grants
               WHERE channel_id = $1 AND provenance = 'email_allowlist'"#,
        )
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

        // A renamed address moves the channel's canonical email interface rather than replacing
        // it, so the binding's audit history survives the rename.
        write_channel_email_binding(&mut tx, company_id, id, &write).await?;

        insert_email_allowlist_grants(&mut tx, company_id, id, participants).await?;

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
            .map_err(|error| {
                if error
                    .as_database_error()
                    .and_then(|db| db.code())
                    .as_deref()
                    == Some("23503")
                {
                    AppError::Conflict(
                        "This is an agent-owned personal channel. Delete the owning agent instead."
                            .into(),
                    )
                } else {
                    AppError::from(error)
                }
            })?;

        Ok(())
    }
}

#[async_trait]
impl OwnedAgentChannelPersistence for PostgresPersistence {
    async fn create_owned_agent_channel(
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
        let agent_created_by = serde_json::to_value(
            agent
                .created_by
                .clone()
                .unwrap_or_else(CreationProvenance::system),
        )
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

        let company_slug: String =
            sqlx::query_scalar("SELECT slug::text FROM companies WHERE id = $1")
                .bind(company_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(AppError::from)?;
        let address = format!("{}@{}", channel.slug, company_slug);
        sqlx::query(
            r#"INSERT INTO agents
               (id, company_id, name, slug, provider, model, system_prompt, description,
                config_json, avatar_url, created_by, run_timeout_secs, memory_enabled,
                memory_persistence_mode, memory_recall_mode, memory_max_results)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)"#,
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
        .bind(agent.memory_enabled)
        .bind(agent.memory_persistence_mode.as_str())
        .bind(agent.memory_recall_mode.as_str())
        .bind(i16::from(agent.memory_max_results))
        .execute(&mut *tx)
        .await
        .map_err(|error| owned_address_error(error, &address))?;

        create_agent_principal_on(&mut tx, company_id, agent_id, &agent.name).await?;

        sqlx::query(
            r#"INSERT INTO channels (
                    id, company_id, owner_agent_id, name, description, access_mode, enabled,
                    add_3rd_party, created_by, retrieve_company_memory, retrieve_agent_memory,
                    retrieve_user_memory, persist_company_memory, persist_agent_memory,
                    persist_user_memory)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)"#,
        )
        .bind(channel_id)
        .bind(company_id)
        .bind(agent_id)
        .bind(&channel.name)
        .bind(&channel.description)
        .bind(access_mode.as_str())
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
        .await
        .map_err(|error| match error {
            AppError::BadRequest(_) => AppError::BadRequest(format!(
                "Address '{address}' is already in use; no agent was created."
            )),
            other => other,
        })?;
        write_channel_email_binding(&mut tx, company_id, channel_id, &channel).await?;
        insert_email_allowlist_grants(&mut tx, company_id, channel_id, participants).await?;
        sqlx::query(
            "INSERT INTO channel_agents (company_id, channel_id, agent_id, position) VALUES ($1, $2, $3, 0)",
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(agent_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;

        let agent = AgentPersistence::get_by_id(self, agent_id)
            .await?
            .ok_or_else(|| AppError::Internal("Created agent was not found".into()))?;
        let channel = load_channel(self, channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Created channel was not found".into()))?;
        Ok((agent, channel))
    }

    async fn update_agent_and_owned_address(
        &self,
        agent_id: Uuid,
        write: AgentWrite,
    ) -> AppResult<Agent> {
        crate::adapters::persistence::agent::update_agent_and_owned_address(self, agent_id, write)
            .await
    }
}

fn owned_address_error(error: sqlx::Error, address: &str) -> AppError {
    if error
        .as_database_error()
        .and_then(|db| db.code())
        .as_deref()
        == Some("23505")
    {
        return AppError::BadRequest(format!(
            "Address '{address}' is already in use; no agent was created."
        ));
    }
    AppError::from(error)
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

    #[tokio::test]
    async fn owned_agent_channel_lifecycle_is_atomic_guarded_and_cascades() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("owned-{suffix}@example.com");
        let user = persistence
            .create_user(&format!("owned-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            user.id,
            CompanyWrite {
                name: "Owned channels".into(),
                slug: format!("owned-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let (agent, channel) = OwnedAgentChannelPersistence::create_owned_agent_channel(
            &persistence,
            company.id,
            AgentWrite {
                name: "Personal agent".into(),
                slug: "personal-agent".into(),
                memory_enabled: true,
                created_by: Some(CreationProvenance::user(user.id)),
                ..AgentWrite::default()
            },
            ChannelWrite {
                name: "Personal agent".into(),
                slug: "personal-agent".into(),
                participant_emails: Some(vec!["outside@example.com".into()]),
                enabled: true,
                add_3rd_party: false,
                retrieve_user_memory: true,
                persist_user_memory: true,
                created_by: Some(CreationProvenance::user(user.id)),
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(channel.owner_agent_id, Some(agent.id));
        assert_eq!(channel.agent_ids, Some(vec![agent.id]));
        assert_eq!(channel.participant_emails.as_ref().map(Vec::len), Some(1));
        assert!(channel.retrieve_user_memory && channel.persist_user_memory);

        let contender_agent = AgentWrite {
            name: "Contended".into(),
            slug: "contended".into(),
            ..AgentWrite::default()
        };
        let contender_channel = ChannelWrite {
            name: "Contended".into(),
            slug: "contended".into(),
            enabled: true,
            ..ChannelWrite::default()
        };
        let (left, right) = tokio::join!(
            OwnedAgentChannelPersistence::create_owned_agent_channel(
                &persistence,
                company.id,
                contender_agent.clone(),
                contender_channel.clone(),
            ),
            OwnedAgentChannelPersistence::create_owned_agent_channel(
                &persistence,
                company.id,
                contender_agent,
                contender_channel,
            ),
        );
        let outcomes = [left, right];
        assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
        let loser = outcomes
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one competing address claimant must lose");
        assert!(
            matches!(loser, AppError::BadRequest(message) if message.contains(&format!("contended@{}", company.slug)))
        );
        let contender_agents: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE company_id = $1 AND slug = $2")
                .bind(company.id)
                .bind("contended")
                .fetch_one(persistence.pool())
                .await
                .unwrap();
        assert_eq!(contender_agents, 1, "the losing transaction left no agent");

        let collision = OwnedAgentChannelPersistence::update_agent_and_owned_address(
            &persistence,
            agent.id,
            AgentWrite {
                name: agent.name.clone(),
                slug: "contended".into(),
                memory_enabled: true,
                created_by: Some(agent.created_by.clone()),
                ..AgentWrite::default()
            },
        )
        .await;
        assert!(
            matches!(collision, Err(AppError::BadRequest(message)) if message.contains(&format!("contended@{}", company.slug)))
        );
        assert_eq!(
            AgentPersistence::get_by_id(&persistence, agent.id)
                .await
                .unwrap()
                .unwrap()
                .slug,
            "personal-agent",
            "an address collision must roll back the agent update"
        );

        assert!(matches!(
            ChannelPersistence::delete(&persistence, channel.id).await,
            Err(AppError::Conflict(_))
        ));

        let renamed = OwnedAgentChannelPersistence::update_agent_and_owned_address(
            &persistence,
            agent.id,
            AgentWrite {
                name: agent.name.clone(),
                slug: "renamed-agent".into(),
                memory_enabled: true,
                created_by: Some(agent.created_by.clone()),
                ..AgentWrite::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(renamed.slug, "renamed-agent");
        OwnedAgentChannelPersistence::update_agent_and_owned_address(
            &persistence,
            agent.id,
            AgentWrite {
                name: renamed.name.clone(),
                slug: renamed.slug.clone(),
                memory_enabled: true,
                created_by: Some(renamed.created_by.clone()),
                ..AgentWrite::default()
            },
        )
        .await
        .expect("retrying the same rename is idempotent");
        let addresses: Vec<(String, bool)> = sqlx::query_as(
            "SELECT slug::text, is_primary FROM channel_slugs WHERE channel_id = $1 ORDER BY slug",
        )
        .bind(channel.id)
        .fetch_all(persistence.pool())
        .await
        .unwrap();
        assert_eq!(
            addresses,
            vec![
                ("personal-agent".into(), false),
                ("renamed-agent".into(), true)
            ]
        );

        let blocker = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Standalone blocker".into(),
                slug: "standalone-blocker".into(),
                agent_ids: Some(vec![agent.id]),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            AgentPersistence::delete(&persistence, agent.id).await,
            Err(AppError::Conflict(_))
        ));
        assert!(
            ChannelPersistence::get_by_id(&persistence, channel.id)
                .await
                .unwrap()
                .is_some(),
            "the blocked owner deletion must roll back its owned-channel cascade"
        );
        ChannelPersistence::delete(&persistence, blocker.id)
            .await
            .unwrap();

        sqlx::query("UPDATE channels SET enabled = FALSE WHERE id = $1")
            .bind(channel.id)
            .execute(persistence.pool())
            .await
            .unwrap();
        let mut invalid_assignment = persistence.pool().begin().await.unwrap();
        sqlx::query("DELETE FROM channel_agents WHERE channel_id = $1 AND position = 0")
            .bind(channel.id)
            .execute(&mut *invalid_assignment)
            .await
            .unwrap();
        assert!(
            invalid_assignment.commit().await.is_err(),
            "a disabled owned channel still requires its owner at position zero"
        );
        let disabled = ChannelPersistence::get_by_id(&persistence, channel.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.agent_ids, Some(vec![agent.id]));
        sqlx::query("UPDATE channels SET enabled = TRUE WHERE id = $1")
            .bind(channel.id)
            .execute(persistence.pool())
            .await
            .unwrap();

        AgentPersistence::delete(&persistence, agent.id)
            .await
            .expect("deleting an owner cascades its personal channel");
        assert!(
            ChannelPersistence::get_by_id(&persistence, channel.id)
                .await
                .unwrap()
                .is_none()
        );
        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }

    /// The form still edits addresses; storage no longer does. A round trip therefore has to go
    /// address -> identity -> principal -> grant and back, and `@public` has to survive that as an
    /// access *mode* rather than turning into a grant for a principal named "@public".
    #[tokio::test]
    async fn a_channel_form_round_trips_its_email_allowlist_through_principal_grants() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());

        let suffix = Uuid::new_v4().simple().to_string();
        let owner_username = format!("grants_{suffix}");
        let owner_email = format!("{owner_username}@example.com");
        persistence
            .create_user(&owner_username, &owner_email, "hash")
            .await
            .unwrap();
        let owner = persistence
            .get_by_email(&owner_email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Grants Corp".to_string(),
                slug: format!("grants-corp-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let agent = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                name: "Desk Agent".to_string(),
                slug: "desk-agent".to_string(),
                ..AgentWrite::default()
            },
        )
        .await
        .unwrap();

        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Front Desk".into(),
                slug: "front-desk".into(),
                // Mixed case and surrounding space, because the form is a text box.
                participant_emails: Some(vec![
                    "  Dana@Partner.test ".to_string(),
                    "@public".to_string(),
                ]),
                agent_ids: Some(vec![agent.id]),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(channel.access_mode, ChannelAccessMode::Public);
        assert_eq!(
            channel.participant_emails,
            Some(vec![
                EmailAddress::from("@public"),
                EmailAddress::from("dana@partner.test"),
            ]),
            "the form reads back the list it submitted, normalized"
        );

        // Authorization reads the grants, and `@public` is not one of them.
        let dana = channel
            .principal_grants
            .iter()
            .map(|grant| grant.principal_id)
            .collect::<HashSet<_>>();
        assert_eq!(dana.len(), 1, "one address, one principal");
        let capabilities: HashSet<_> = channel
            .principal_grants
            .iter()
            .map(|grant| grant.capability)
            .collect();
        assert_eq!(
            capabilities,
            HashSet::from([PrincipalCapability::Participate, PrincipalCapability::View]),
            "the allowlist form grants participation and UI read together"
        );

        let subject: String = sqlx::query_scalar(
            r#"SELECT identity.subject
               FROM channel_principal_grants AS channel_grant
               JOIN participant_identities AS identity
                 ON (identity.company_id, identity.principal_id) =
                    (channel_grant.company_id, channel_grant.principal_id)
               WHERE channel_grant.channel_id = $1 AND channel_grant.capability = 'participate'"#,
        )
        .bind(channel.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(subject, "dana@partner.test");

        // Editing the list replaces the grants rather than accumulating them.
        let updated = ChannelPersistence::update(
            &persistence,
            channel.id,
            ChannelWrite {
                name: "Front Desk".into(),
                slug: "front-desk".into(),
                participant_emails: Some(vec!["sam@partner.test".to_string()]),
                agent_ids: Some(vec![agent.id]),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.access_mode, ChannelAccessMode::Allowlist);
        assert_eq!(
            updated.participant_emails,
            Some(vec![EmailAddress::from("sam@partner.test")])
        );
        assert_eq!(updated.principal_grants.len(), 2);

        // A malformed address is a form error, not a silent "not authorized".
        let malformed = ChannelPersistence::update(
            &persistence,
            channel.id,
            ChannelWrite {
                name: "Front Desk".into(),
                slug: "front-desk".into(),
                participant_emails: Some(vec!["not-an-address".to_string()]),
                agent_ids: Some(vec![agent.id]),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await;
        assert!(matches!(malformed, Err(AppError::BadRequest(_))));
        let unchanged = ChannelPersistence::get_by_id(&persistence, channel.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            unchanged.participant_emails,
            Some(vec![EmailAddress::from("sam@partner.test")]),
            "the rejected edit rolled back whole"
        );

        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }
}
