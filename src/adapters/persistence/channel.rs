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
use crate::adapters::persistence::{agent::insert_agent_on, skill::copy_library_skill_on};
use crate::adapters::protocols::email::EmailIdentity;
use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::{Channel, ChannelAccessMode},
        creation::CreationProvenance,
        participant::{IdentityClaimMetadata, IdentityProvenance, PrincipalCapability},
        value_objects::{ChannelSlug, CompanySlug, EmailAddress},
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite, LibraryAgentCopyPersistence},
        channel::{ChannelPersistence, ChannelUpdate, ChannelWrite},
        participant::{IdentityObservation, IdentityResolution},
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
    pub access_mode: String,
    pub principal_grants: serde_json::Value,
    pub enabled: bool,
    pub add_3rd_party: bool,
    pub settings_revision: i64,
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
            access_mode: ChannelAccessMode::from_str(&db.access_mode)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            principal_grants: serde_json::from_value(db.principal_grants).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid channel principal grants for {}: {error}",
                    db.id
                ))
            })?,
            enabled: db.enabled,
            add_3rd_party: db.add_3rd_party,
            settings_revision: db.settings_revision,
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
                   (SELECT array_agg(identity.subject ORDER BY identity.subject)
                    FROM channel_principal_grants AS channel_grant
                    JOIN participant_identities AS identity
                      ON (identity.company_id, identity.principal_id) =
                         (channel_grant.company_id, channel_grant.principal_id)
                    WHERE channel_grant.company_id = ch.company_id AND channel_grant.channel_id = ch.id
                      AND channel_grant.capability = 'participate'
                      AND channel_grant.provenance = 'configured_allowlist'
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
                      AND channel_grant.provenance = 'configured_allowlist'
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
           ch.enabled, ch.add_3rd_party, ch.settings_revision, ch.created_by, ch.created_at
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
pub(super) async fn insert_channel_slugs(
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
pub(super) fn channel_access(
    participant_emails: Option<Vec<String>>,
) -> (ChannelAccessMode, Vec<String>) {
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
                   VALUES ($1, $2, $3, $4, 'configured_allowlist')
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
pub(super) async fn write_channel_email_binding(
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

/// The `channels` row of a new channel, with its stored access mode; every creation path writes
/// it here, then its slugs, email binding and allowlist grants through their own helpers. Returns
/// the participants that become allowlist grants.
pub(crate) async fn insert_channel_row_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    write: &ChannelWrite,
) -> AppResult<Vec<String>> {
    let (access_mode, participants) = channel_access(write.participant_emails.clone());
    let created_by = serde_json::to_value(
        write
            .created_by
            .clone()
            .unwrap_or_else(CreationProvenance::system),
    )
    .map_err(|error| AppError::Internal(error.to_string()))?;
    sqlx::query(
        r#"INSERT INTO channels (
                id, company_id, name, description, access_mode, enabled, add_3rd_party,
                created_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
    )
    .bind(channel_id)
    .bind(company_id)
    .bind(&write.name)
    .bind(&write.description)
    .bind(access_mode.as_str())
    .bind(write.enabled)
    .bind(write.add_3rd_party)
    .bind(created_by)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(participants)
}

#[async_trait]
impl ChannelPersistence for PostgresPersistence {
    async fn create(&self, company_id: Uuid, write: ChannelWrite) -> AppResult<Channel> {
        let uuid = Uuid::new_v4();
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let participants = insert_channel_row_on(&mut tx, company_id, uuid, &write).await?;

        insert_channel_slugs(&mut tx, company_id, uuid, &write.slug, &write.alias_slugs).await?;

        write_channel_email_binding(&mut tx, company_id, uuid, &write).await?;

        insert_email_allowlist_grants(&mut tx, company_id, uuid, participants).await?;

        tx.commit().await.map_err(AppError::from)?;
        load_channel(self, uuid)
            .await?
            .ok_or_else(|| AppError::Internal("Created channel was not found".into()))
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

    async fn update(&self, request: ChannelUpdate) -> AppResult<Channel> {
        let channel_id = request.channel_id;
        crate::adapters::persistence::channel_edit::update_channel_within(
            &self.pool,
            request,
            crate::adapters::persistence::channel_edit::EditBudget::DEFAULT,
        )
        .await?;
        load_channel(self, channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Updated channel was not found".into()))
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        let channel: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM channels WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(_) = channel else { return Ok(()) };
        let retained: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE channel_id=$1) \
             OR EXISTS(SELECT 1 FROM workflow_bindings WHERE channel_id=$1) \
             OR EXISTS(SELECT 1 FROM channel_schedules WHERE channel_id=$1) \
             OR EXISTS(SELECT 1 FROM workflow_runs WHERE channel_id=$1)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if retained {
            return Err(AppError::Conflict(
                "This channel has retained conversations or workflow configuration and cannot be deleted"
                    .into(),
            ));
        }
        sqlx::query("DELETE FROM channels WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|error| {
                if error
                    .as_database_error()
                    .is_some_and(|error| error.is_foreign_key_violation())
                {
                    AppError::Conflict(
                        "This channel has retained conversations or workflow configuration and cannot be deleted"
                            .into(),
                    )
                } else {
                    AppError::from(error)
                }
            })?;
        tx.commit().await?;
        Ok(())
    }
}

/// Every setting one channel edit writes, on the caller's transaction.
///
/// The caller holds the channel's gate and row lock and has checked its tenant
/// (`channel_edit`).
pub(super) async fn write_channel_settings_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    id: Uuid,
    write: &ChannelWrite,
) -> AppResult<()> {
    let (access_mode, participants) = channel_access(write.participant_emails.clone());
    let result = sqlx::query(
        r#"UPDATE channels
               SET name = $1, description = $2, access_mode = $3, enabled = $4,
                   add_3rd_party = $5, settings_revision = settings_revision + 1
               WHERE id = $6 AND company_id = $7"#,
    )
    .bind(&write.name)
    .bind(&write.description)
    .bind(access_mode.as_str())
    .bind(write.enabled)
    .bind(write.add_3rd_party)
    .bind(id)
    .bind(company_id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;

    if result.rows_affected() == 0 {
        return Err(AppError::Internal("Channel not found".into()));
    }

    // Only the grants this form owns: a grant written by another provenance is not the email
    // allowlist's to replace.
    sqlx::query(
        r#"DELETE FROM channel_principal_grants
               WHERE channel_id = $1 AND provenance = 'configured_allowlist'"#,
    )
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query("DELETE FROM channel_slugs WHERE channel_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    insert_channel_slugs(tx, company_id, id, &write.slug, &write.alias_slugs).await?;

    // A renamed address moves the channel's canonical email interface rather than replacing
    // it, so the binding's audit history survives the rename.
    write_channel_email_binding(tx, company_id, id, write).await?;

    insert_email_allowlist_grants(tx, company_id, id, participants).await?;
    Ok(())
}

#[async_trait]
impl LibraryAgentCopyPersistence for PostgresPersistence {
    async fn create_agent_from_library(
        &self,
        company_id: Uuid,
        library_agent_id: Uuid,
        mut agent: AgentWrite,
    ) -> AppResult<Agent> {
        let agent_id = Uuid::new_v4();
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        prepare_library_agent_on(&mut tx, company_id, library_agent_id, &mut agent).await?;
        insert_agent_on(&mut tx, agent_id, Some(company_id), &agent).await?;
        create_agent_principal_on(&mut tx, company_id, agent_id, &agent.name).await?;
        tx.commit().await.map_err(AppError::from)?;
        AgentPersistence::get_by_id(self, agent_id)
            .await?
            .ok_or_else(|| AppError::Internal("Created agent was not found".into()))
    }
}

/// Copy a library definition into the company inside `tx`: its global skills become tenant-owned
/// copies the agent is attached to, and its workflow tool grants come with it. A copy that grants
/// tools and would inherit its model gets the company default connection's first model, because a
/// workflow runs an agent only on an explicit model (p08 D2.6).
async fn prepare_library_agent_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    library_agent_id: Uuid,
    agent: &mut AgentWrite,
) -> AppResult<()> {
    let source_exists = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM agents \
             WHERE id = $1 AND company_id IS NULL FOR KEY SHARE",
    )
    .bind(library_agent_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?
    .is_some();
    if !source_exists {
        return Err(AppError::NotFound("Library agent not found.".into()));
    }

    let source_skill_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT skill_id FROM agent_skills \
             WHERE agent_id = $1 ORDER BY position",
    )
    .bind(library_agent_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let created_by = agent
        .created_by
        .clone()
        .unwrap_or_else(CreationProvenance::system);
    let mut copied_skill_ids = Vec::with_capacity(source_skill_ids.len());
    for source_skill_id in source_skill_ids {
        let copied = copy_library_skill_on(tx, company_id, source_skill_id, &created_by).await?;
        copied_skill_ids.push(copied.id);
    }
    agent.skill_ids = copied_skill_ids;

    // The definition's workflow tool grants come with it (p08 D2.6). A workflow runs an agent
    // only on an explicit model, so a copy that grants tools and would inherit gets the
    // company's default connection's first model, when it has a key.
    let workflow_tools: Vec<String> =
        sqlx::query_scalar("SELECT workflow_tools FROM agents WHERE id = $1")
            .bind(library_agent_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(AppError::from)?;
    if !workflow_tools.is_empty() {
        if agent.provider.is_none() && agent.model.is_none() {
            let default: Option<(String, String)> = sqlx::query_as(
                "SELECT provider, models[1] FROM company_model_connections \
                 WHERE company_id = $1 AND is_default AND api_key IS NOT NULL \
                   AND cardinality(models) > 0",
            )
            .bind(company_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(AppError::from)?;
            if let Some((provider, model)) = default {
                agent.provider = Some(provider);
                agent.model = Some(model);
            }
        }
        agent.workflow_tools = Some(
            workflow_tools
                .iter()
                .map(|name| {
                    crate::domain::workflow::TypeName::parse(name).map_err(|_| {
                        AppError::Internal("Stored workflow tool grant is invalid".into())
                    })
                })
                .collect::<AppResult<_>>()?,
        );
    }
    Ok(())
}

/// Visibility inputs for a requested run set; never loads unrelated company channels.
pub(super) async fn channels_by_ids(
    p: &PostgresPersistence,
    company: Uuid,
    ids: &[Uuid],
) -> AppResult<Vec<Channel>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let query = format!("{CHANNEL_SELECT} WHERE ch.company_id=$1 AND ch.id=ANY($2)");
    let rows = sqlx::query_as::<_, ChannelDb>(&query)
        .bind(company)
        .bind(ids)
        .fetch_all(p.pool())
        .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::agent::AgentPersistence;
    use crate::use_cases::agent::AgentWrite;
    use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
    use crate::use_cases::user::UserPersistence;

    /// An edit made by a manager these tests do not otherwise need.
    fn test_update(company_id: Uuid, channel_id: Uuid, write: ChannelWrite) -> ChannelUpdate {
        ChannelUpdate {
            company_id,
            channel_id,
            write,
        }
    }

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

        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Inbound Email".into(),
                description: Some("Takes support mail from the website form.".into()),
                slug: "inbound-email".into(),
                alias_slugs: Vec::new(),
                participant_emails: Some(emails.clone()),
                enabled: true,
                // Deliberately the opposite of `enabled`, so a swapped pair of same-typed binds
                // cannot pass this test.
                add_3rd_party: false,
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
            test_update(
                company.id,
                channel.id,
                ChannelWrite {
                    name: "Inbound Email V2".into(),
                    description: Some("Now also handles refund requests.".into()),
                    slug: "inbound-email-v2".into(),
                    enabled: false,
                    add_3rd_party: true,
                    created_by: None,
                    ..ChannelWrite::default()
                },
            ),
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

        // A retained conversation prevents deleting the channel, even after it is disabled.
        let thread = Uuid::new_v4();
        sqlx::query("INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1,$2,$3,'Retained')")
            .bind(thread).bind(company.id).bind(channel.id).execute(persistence.pool()).await.unwrap();
        assert!(matches!(
            ChannelPersistence::delete(&persistence, channel.id).await,
            Err(AppError::Conflict(_))
        ));
        assert!(
            ChannelPersistence::get_by_id(&persistence, channel.id)
                .await
                .unwrap()
                .is_some()
        );
        sqlx::query("DELETE FROM threads WHERE id=$1")
            .bind(thread)
            .execute(persistence.pool())
            .await
            .unwrap();

        // 5. Delete an empty channel.
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
        let updated = ChannelPersistence::update(
            &persistence,
            test_update(company.id, support.id, write("support", &["contact"])),
        )
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
    async fn an_enabled_channel_needs_no_agent() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let owner = persistence
            .create_user(&suffix, &format!("{suffix}@example.com"), "hash")
            .await
            .unwrap()
            .id;
        let company = CompanyPersistence::create(
            &persistence,
            owner,
            CompanyWrite {
                name: "Plain".into(),
                slug: format!("plain-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let plain = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Plain".into(),
                slug: "plain".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .expect("an enabled plain channel is creatable");
        assert!(plain.enabled);

        let agent = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                name: "Only".into(),
                slug: "only".into(),
                ..AgentWrite::default()
            },
        )
        .await
        .unwrap();
        let assigned = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Assigned".into(),
                slug: "assigned".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        AgentPersistence::delete(&persistence, agent.id)
            .await
            .expect("an enabled channel's only agent can be deleted");
        let left = ChannelPersistence::get_by_id(&persistence, assigned.id)
            .await
            .unwrap()
            .expect("the channel stays");
        assert!(left.enabled);
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
            test_update(
                company.id,
                channel.id,
                ChannelWrite {
                    name: "Front Desk".into(),
                    slug: "front-desk".into(),
                    participant_emails: Some(vec!["sam@partner.test".to_string()]),
                    enabled: true,
                    ..ChannelWrite::default()
                },
            ),
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
            test_update(
                company.id,
                channel.id,
                ChannelWrite {
                    name: "Front Desk".into(),
                    slug: "front-desk".into(),
                    participant_emails: Some(vec!["not-an-address".to_string()]),
                    enabled: true,
                    ..ChannelWrite::default()
                },
            ),
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
