use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::{
        PostgresPersistence,
        integration::email_binding::{CanonicalEmailBinding, write_canonical_email_binding},
        participant::create_agent_principal_on,
    },
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        creation::CreationProvenance,
        harness::{HarnessConfig, HarnessKind, NativeToolPolicy, SubAgentScope},
        memory::{MemoryPersistenceMode, MemoryRecallMode},
        skill::Skill,
        value_objects::{AvatarUrl, ChannelSlug, ToolId},
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite, validate_effective_capabilities},
        builtin_agent_library::{
            BuiltinAgentDefinition, BuiltinAgentInstallOutcome, BuiltinAgentLibraryPersistence,
        },
        skill::{AgentCapabilityReader, StoredAgentCapabilities},
    },
};

pub(crate) const AGENT_COLUMNS: &str = "\
    agent.id, agent.company_id, agent.name, agent.slug::text AS slug, agent.provider, agent.model, \
    agent.system_prompt, agent.description, agent.config_json, agent.avatar_url, agent.created_by, \
    agent.created_at, agent.run_timeout_secs, agent.memory_enabled, \
    agent.memory_persistence_mode, agent.memory_recall_mode, agent.memory_max_results, \
    agent.harness_kind, agent.granted_tool_ids, agent.native_tool_policy";

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct AgentDb {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub run_timeout_secs: Option<i32>,
    pub system_prompt: Option<String>,
    pub description: Option<String>,
    pub config_json: Option<serde_json::Value>,
    pub memory_enabled: bool,
    pub memory_persistence_mode: String,
    pub memory_recall_mode: String,
    pub memory_max_results: i16,
    pub avatar_url: Option<String>,
    pub created_by: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub harness_kind: String,
    pub granted_tool_ids: Vec<String>,
    pub native_tool_policy: serde_json::Value,
}

impl TryFrom<AgentDb> for Agent {
    type Error = AppError;

    fn try_from(db: AgentDb) -> AppResult<Self> {
        let harness_kind = HarnessKind::parse(&db.harness_kind).ok_or_else(|| {
            AppError::Internal(format!(
                "Invalid agents.harness_kind '{}' for agent {}",
                db.harness_kind, db.id
            ))
        })?;
        HarnessConfig::parse(harness_kind, db.config_json.as_ref()).map_err(|error| {
            AppError::Internal(format!("Invalid agent {} harness config: {error}", db.id))
        })?;
        let native_tool_policy =
            NativeToolPolicy::parse(&db.native_tool_policy).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid agent {} native tool policy: {error}",
                    db.id
                ))
            })?;
        Ok(Agent {
            id: db.id,
            company_id: db.company_id,
            name: db.name,
            slug: db.slug,
            provider: db.provider,
            model: db.model,
            run_timeout_secs: db
                .run_timeout_secs
                .map(u32::try_from)
                .transpose()
                .map_err(|_| AppError::Internal("Invalid agents.run_timeout_secs".into()))?,
            system_prompt: db.system_prompt,
            description: db.description,
            harness_kind,
            granted_tool_ids: db.granted_tool_ids.into_iter().map(ToolId::from).collect(),
            native_tool_policy,
            config_json: db.config_json,
            memory_enabled: db.memory_enabled,
            memory_persistence_mode: match db.memory_persistence_mode.as_str() {
                "scope_specific_facts" => MemoryPersistenceMode::ScopeSpecificFacts,
                _ => MemoryPersistenceMode::AudienceOnly,
            },
            memory_recall_mode: match db.memory_recall_mode.as_str() {
                "thinking" => MemoryRecallMode::Thinking,
                _ => MemoryRecallMode::Fast,
            },
            memory_max_results: u8::try_from(db.memory_max_results)
                .map_err(|_| AppError::Internal("Invalid agents.memory_max_results".into()))?,
            avatar_url: db.avatar_url.map(AvatarUrl::from),
            created_by: serde_json::from_value(db.created_by).map_err(|err| {
                AppError::Internal(format!("Invalid agents.created_by provenance: {err}"))
            })?,
            created_at: db.created_at,
        })
    }
}

struct AgentJsonFields {
    harness_config: Option<serde_json::Value>,
    native_tool_policy: serde_json::Value,
    created_by: serde_json::Value,
}

fn agent_json_fields(write: &AgentWrite) -> AppResult<AgentJsonFields> {
    // Application use cases normalize first, but persistence is also called by provisioning
    // adapters and maintenance code. Re-run the bounded, fail-closed validation before any SQL so
    // an alternate caller cannot store a grant or policy the normal write path would reject.
    let mut validated = write.clone();
    validated.normalize()?;
    let harness_config = HarnessConfig::parse(write.harness_kind, write.config_json.as_ref())
        .map_err(AppError::BadRequest)?;
    let canonical = harness_config.to_json().map_err(AppError::BadRequest)?;
    Ok(AgentJsonFields {
        harness_config: (canonical != serde_json::json!({"version": 1})).then_some(canonical),
        native_tool_policy: serde_json::to_value(&write.native_tool_policy)
            .map_err(|error| AppError::Internal(error.to_string()))?,
        created_by: serde_json::to_value(
            write
                .created_by
                .clone()
                .unwrap_or_else(CreationProvenance::system),
        )
        .map_err(|error| AppError::Internal(error.to_string()))?,
    })
}

pub(crate) async fn insert_agent_on(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    company_id: Option<Uuid>,
    write: &AgentWrite,
) -> AppResult<()> {
    let run_timeout_secs = write
        .run_timeout_secs
        .map(i32::try_from)
        .transpose()
        .map_err(|_| AppError::BadRequest("Agent run timeout is too large.".into()))?;
    let json = agent_json_fields(write)?;
    sqlx::query(
        r#"INSERT INTO agents
           (id, company_id, name, slug, provider, model, system_prompt, description,
            config_json, avatar_url, created_by, run_timeout_secs, memory_enabled,
            memory_persistence_mode, memory_recall_mode, memory_max_results,
            harness_kind, granted_tool_ids, native_tool_policy)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                   $15, $16, $17, $18, $19)"#,
    )
    .bind(id)
    .bind(company_id)
    .bind(&write.name)
    .bind(&write.slug)
    .bind(&write.provider)
    .bind(&write.model)
    .bind(&write.system_prompt)
    .bind(&write.description)
    .bind(json.harness_config)
    .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
    .bind(json.created_by)
    .bind(run_timeout_secs)
    .bind(write.memory_enabled)
    .bind(write.memory_persistence_mode.as_str())
    .bind(write.memory_recall_mode.as_str())
    .bind(i16::from(write.memory_max_results))
    .bind(write.harness_kind.as_str())
    .bind(
        write
            .granted_tool_ids
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
    )
    .bind(json.native_tool_policy)
    .execute(&mut **transaction)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref()
            == Some("23505")
        {
            AppError::Conflict("An agent with that slug already exists.".into())
        } else {
            AppError::from(error)
        }
    })?;
    replace_agent_capabilities_on(transaction, company_id, id, write).await
}

pub(crate) async fn validate_agent_capabilities_on(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Option<Uuid>,
    agent_id: Uuid,
    write: &AgentWrite,
) -> AppResult<Vec<Skill>> {
    if company_id.is_none() && !write.sub_agent_ids.is_empty() {
        return Err(AppError::BadRequest(
            "A global library agent cannot store a sub-agent allowlist.".into(),
        ));
    }
    if write.sub_agent_ids.contains(&agent_id) {
        return Err(AppError::BadRequest(
            "An agent cannot delegate to itself.".into(),
        ));
    }

    let mut skills = Vec::with_capacity(write.skill_ids.len());
    for skill_id in &write.skill_ids {
        let row = sqlx::query_as::<_, crate::adapters::persistence::skill::SkillDb>(&format!(
            "SELECT {} FROM skills AS skill \
             WHERE skill.id = $1 AND skill.company_id IS NOT DISTINCT FROM $2 \
             FOR KEY SHARE",
            crate::adapters::persistence::skill::SKILL_COLUMNS
        ))
        .bind(skill_id)
        .bind(company_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Skill {skill_id} does not belong to the agent's library."
            ))
        })?;
        skills.push(row.try_into()?);
    }

    if let Some(company_id) = company_id {
        for sub_agent_id in &write.sub_agent_ids {
            let exists = sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM agents \
                 WHERE company_id = $1 AND id = $2 FOR KEY SHARE",
            )
            .bind(company_id)
            .bind(sub_agent_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(AppError::from)?
            .is_some();
            if !exists {
                return Err(AppError::BadRequest(format!(
                    "Sub-agent {sub_agent_id} does not belong to this company."
                )));
            }
        }
    }

    validate_effective_capabilities(write, &skills)?;
    Ok(skills)
}

pub(crate) async fn replace_agent_capabilities_on(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Option<Uuid>,
    agent_id: Uuid,
    write: &AgentWrite,
) -> AppResult<()> {
    let _ = validate_agent_capabilities_on(transaction, company_id, agent_id, write).await?;
    sqlx::query("DELETE FROM agent_skills WHERE agent_id = $1")
        .bind(agent_id)
        .execute(&mut **transaction)
        .await
        .map_err(AppError::from)?;
    sqlx::query("DELETE FROM agent_sub_agents WHERE agent_id = $1")
        .bind(agent_id)
        .execute(&mut **transaction)
        .await
        .map_err(AppError::from)?;

    for (position, skill_id) in write.skill_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO agent_skills (company_id, agent_id, skill_id, position) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(company_id)
        .bind(agent_id)
        .bind(skill_id)
        .bind(position as i32)
        .execute(&mut **transaction)
        .await
        .map_err(capability_relationship_error)?;
    }
    if let Some(company_id) = company_id {
        for (position, sub_agent_id) in write.sub_agent_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO agent_sub_agents \
                     (company_id, agent_id, sub_agent_id, position) \
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(company_id)
            .bind(agent_id)
            .bind(sub_agent_id)
            .bind(position as i32)
            .execute(&mut **transaction)
            .await
            .map_err(capability_relationship_error)?;
        }
    }
    Ok(())
}

fn capability_relationship_error(error: sqlx::Error) -> AppError {
    let Some(database) = error.as_database_error() else {
        return AppError::from(error);
    };
    match database.constraint() {
        Some("agent_sub_agents_not_self") => {
            AppError::BadRequest("An agent cannot delegate to itself.".into())
        }
        Some("agent_skills_agent_scope_check") | Some("agent_skills_skill_scope_check") => {
            AppError::BadRequest("The selected skill belongs to another library.".into())
        }
        Some("agent_sub_agents_agent_fk") | Some("agent_sub_agents_sub_fk") => {
            AppError::BadRequest("The selected sub-agent belongs to another company.".into())
        }
        _ => AppError::from(error),
    }
}

#[async_trait]
impl BuiltinAgentLibraryPersistence for PostgresPersistence {
    async fn ensure_library_agent(
        &self,
        mut definition: BuiltinAgentDefinition,
    ) -> AppResult<BuiltinAgentInstallOutcome> {
        definition.write.normalize()?;
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let lock_key = format!("builtin-agent-library:{}", definition.write.slug);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;

        let already_present: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE company_id IS NULL AND slug = $1)",
        )
        .bind(&definition.write.slug)
        .fetch_one(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        if already_present {
            transaction.commit().await.map_err(AppError::from)?;
            return Ok(BuiltinAgentInstallOutcome::AlreadyPresent);
        }

        insert_agent_on(&mut transaction, definition.id, None, &definition.write).await?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(BuiltinAgentInstallOutcome::Created)
    }
}

pub(crate) async fn update_agent_and_owned_address(
    persistence: &PostgresPersistence,
    id: Uuid,
    write: AgentWrite,
) -> AppResult<Agent> {
    let run_timeout_secs = write
        .run_timeout_secs
        .map(i32::try_from)
        .transpose()
        .map_err(|_| AppError::BadRequest("Agent run timeout is too large.".into()))?;
    let mut tx = persistence.pool.begin().await.map_err(AppError::from)?;
    let json = agent_json_fields(&write)?;
    let (company_id, company_slug): (Option<Uuid>, Option<String>) = sqlx::query_as(
        r#"SELECT agent.company_id, company.slug::text
           FROM agents AS agent
           LEFT JOIN companies AS company ON company.id = agent.company_id
           WHERE agent.id = $1
           FOR UPDATE OF agent"#,
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(AppError::from)?;
    replace_agent_capabilities_on(&mut tx, company_id, id, &write).await?;
    // The name comes back with the id because the channel's canonical email binding is relabelled
    // from it below, and this path must not overwrite that label with the agent's name.
    let owned_channel: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM channels WHERE owner_agent_id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(AppError::from)?;

    let db = sqlx::query_as::<_, AgentDb>(&format!(
        r#"UPDATE agents AS agent
           SET name = $1, slug = $2, provider = $3, model = $4, system_prompt = $5,
               description = $6, config_json = $7, avatar_url = $8, run_timeout_secs = $9,
               memory_enabled = $10, memory_persistence_mode = $11,
               memory_recall_mode = $12, memory_max_results = $13, harness_kind = $14,
               granted_tool_ids = $15, native_tool_policy = $16
           WHERE id = $17
           RETURNING {AGENT_COLUMNS}"#
    ))
    .bind(&write.name)
    .bind(&write.slug)
    .bind(&write.provider)
    .bind(&write.model)
    .bind(&write.system_prompt)
    .bind(&write.description)
    .bind(&json.harness_config)
    .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
    .bind(run_timeout_secs)
    .bind(write.memory_enabled)
    .bind(write.memory_persistence_mode.as_str())
    .bind(write.memory_recall_mode.as_str())
    .bind(i16::from(write.memory_max_results))
    .bind(write.harness_kind.as_str())
    .bind(
        write
            .granted_tool_ids
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
    )
    .bind(&json.native_tool_policy)
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| address_update_error(error, &write.slug, company_slug.as_deref()))?;

    if let (Some((channel_id, channel_name)), Some(company_id)) = (owned_channel, company_id) {
        let current_slug: String = sqlx::query_scalar(
            "SELECT slug::text FROM channel_slugs WHERE channel_id = $1 AND is_primary FOR UPDATE",
        )
        .bind(channel_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if !current_slug.eq_ignore_ascii_case(&write.slug) {
            let occupied: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM channel_slugs WHERE company_id = $1 AND slug = $2 AND channel_id <> $3)",
            )
            .bind(company_id)
            .bind(&write.slug)
            .bind(channel_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::from)?;
            if occupied {
                return Err(address_update_error_message(
                    &write.slug,
                    company_slug.as_deref(),
                ));
            }
            sqlx::query(
                "UPDATE channel_slugs SET is_primary = FALSE WHERE channel_id = $1 AND is_primary",
            )
            .bind(channel_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            sqlx::query(
                r#"INSERT INTO channel_slugs (company_id, channel_id, slug, is_primary)
                   VALUES ($1, $2, $3, TRUE)
                   ON CONFLICT (channel_id, slug) DO UPDATE SET is_primary = TRUE"#,
            )
            .bind(company_id)
            .bind(channel_id)
            .bind(&write.slug)
            .execute(&mut *tx)
            .await
            .map_err(|error| address_update_error(error, &write.slug, company_slug.as_deref()))?;

            // An agent's address *is* its channel's address, so the channel's email interface has
            // to move with it. Same writer as every other rename, in this same transaction: a
            // binding left on the old local part would stop resolving inbound mail entirely.
            //
            // The actor is `system` because this path has none -- `update_library_agent` is an
            // operator route that takes no user id. Recording that honestly beats attributing the
            // move to someone who did not make it.
            write_canonical_email_binding(
                &mut tx,
                CanonicalEmailBinding {
                    company_id,
                    channel_id,
                    channel_slug: &ChannelSlug::new(write.slug.clone()),
                    channel_name: &channel_name,
                    created_by: &CreationProvenance::system(),
                },
            )
            .await?;
        }
    }
    tx.commit().await.map_err(AppError::from)?;
    db.try_into()
}

fn address_update_error(error: sqlx::Error, slug: &str, company_slug: Option<&str>) -> AppError {
    if error
        .as_database_error()
        .and_then(|db| db.code())
        .as_deref()
        == Some("23505")
    {
        return address_update_error_message(slug, company_slug);
    }
    AppError::from(error)
}

fn address_update_error_message(slug: &str, company_slug: Option<&str>) -> AppError {
    let address =
        company_slug.map_or_else(|| slug.to_string(), |company| format!("{slug}@{company}"));
    AppError::BadRequest(format!(
        "Address '{address}' is already in use; the agent was not changed."
    ))
}

#[async_trait]
impl AgentPersistence for PostgresPersistence {
    async fn create(&self, company_id: Uuid, write: AgentWrite) -> AppResult<Agent> {
        let uuid = Uuid::new_v4();
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let run_timeout_secs = write
            .run_timeout_secs
            .map(i32::try_from)
            .transpose()
            .map_err(|_| AppError::BadRequest("Agent run timeout is too large.".into()))?;
        let json = agent_json_fields(&write)?;

        let db = sqlx::query_as::<_, AgentDb>(
            &format!(r#"INSERT INTO agents AS agent (id, company_id, name, slug, provider, model, system_prompt, description, config_json, avatar_url, created_by, run_timeout_secs, memory_enabled, memory_persistence_mode, memory_recall_mode, memory_max_results, harness_kind, granted_tool_ids, native_tool_policy)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
               RETURNING {AGENT_COLUMNS}"#),
        )
        .bind(uuid)
        .bind(company_id)
        .bind(&write.name)
        .bind(&write.slug)
        .bind(&write.provider)
        .bind(&write.model)
        .bind(&write.system_prompt)
        .bind(&write.description)
        .bind(&json.harness_config)
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(&json.created_by)
        .bind(run_timeout_secs)
        .bind(write.memory_enabled)
        .bind(write.memory_persistence_mode.as_str())
        .bind(write.memory_recall_mode.as_str())
        .bind(i16::from(write.memory_max_results))
        .bind(write.harness_kind.as_str())
        .bind(write.granted_tool_ids.iter().map(|id| id.as_str()).collect::<Vec<_>>())
        .bind(&json.native_tool_policy)
        .fetch_one(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        replace_agent_capabilities_on(&mut transaction, Some(company_id), uuid, &write).await?;
        create_agent_principal_on(&mut transaction, company_id, uuid, &write.name).await?;
        transaction.commit().await.map_err(AppError::from)?;

        db.try_into()
    }

    async fn create_library(&self, write: AgentWrite) -> AppResult<Agent> {
        let uuid = Uuid::new_v4();
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let run_timeout_secs = write
            .run_timeout_secs
            .map(i32::try_from)
            .transpose()
            .map_err(|_| AppError::BadRequest("Agent run timeout is too large.".into()))?;
        let json = agent_json_fields(&write)?;
        let db = sqlx::query_as::<_, AgentDb>(
            &format!(r#"INSERT INTO agents AS agent (id, company_id, name, slug, provider, model, system_prompt, description, config_json, avatar_url, created_by, run_timeout_secs, memory_enabled, memory_persistence_mode, memory_recall_mode, memory_max_results, harness_kind, granted_tool_ids, native_tool_policy)
               VALUES ($1, NULL, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
               RETURNING {AGENT_COLUMNS}"#),
        )
        .bind(uuid)
        .bind(&write.name)
        .bind(&write.slug)
        .bind(&write.provider)
        .bind(&write.model)
        .bind(&write.system_prompt)
        .bind(&write.description)
        .bind(&json.harness_config)
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(&json.created_by)
        .bind(run_timeout_secs)
        .bind(write.memory_enabled)
        .bind(write.memory_persistence_mode.as_str())
        .bind(write.memory_recall_mode.as_str())
        .bind(i16::from(write.memory_max_results))
        .bind(write.harness_kind.as_str())
        .bind(write.granted_tool_ids.iter().map(|id| id.as_str()).collect::<Vec<_>>())
        .bind(&json.native_tool_policy)
        .fetch_one(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        replace_agent_capabilities_on(&mut transaction, None, uuid, &write).await?;
        transaction.commit().await.map_err(AppError::from)?;
        db.try_into()
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Agent>> {
        let db = sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent WHERE agent.id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn get_by_company_slug_and_agent_slug(
        &self,
        company_slug: &str,
        agent_slug: &str,
    ) -> AppResult<Option<Agent>> {
        let db = sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent \
             JOIN companies AS company ON company.id = agent.company_id \
             WHERE company.slug = $1 AND agent.slug = $2"
        ))
        .bind(company_slug)
        .bind(agent_slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Agent>> {
        let db_list = sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent WHERE agent.company_id = $1 \
             ORDER BY agent.created_at DESC, agent.id DESC LIMIT 200"
        ))
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn list_library(&self) -> AppResult<Vec<Agent>> {
        let rows = sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent WHERE agent.company_id IS NULL \
             ORDER BY agent.created_at DESC, agent.id DESC LIMIT 200"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn update(&self, id: Uuid, write: AgentWrite) -> AppResult<Agent> {
        update_agent_and_owned_address(self, id, write).await
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
                    .is_some_and(|code| code == "23503" || code == "23514")
                {
                    AppError::Conflict(
                        "This agent is active on an enabled channel. Disable or reassign the channel before deleting it.".into(),
                    )
                } else {
                    AppError::from(error)
                }
            })?;

        Ok(())
    }
}

#[async_trait]
impl AgentCapabilityReader for PostgresPersistence {
    async fn load_for_execution(
        &self,
        execution_company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<StoredAgentCapabilities>> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;
        let row = sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent \
             WHERE agent.id = $1 \
               AND (agent.company_id = $2 OR agent.company_id IS NULL)"
        ))
        .bind(agent_id)
        .bind(execution_company_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(AppError::from)?;
            return Ok(None);
        };
        let agent: Agent = row.try_into()?;
        let skill_rows =
            sqlx::query_as::<_, crate::adapters::persistence::skill::SkillDb>(&format!(
                "SELECT {} FROM agent_skills AS selection \
                 JOIN skills AS skill ON skill.id = selection.skill_id \
                 WHERE selection.agent_id = $1 ORDER BY selection.position",
                crate::adapters::persistence::skill::SKILL_COLUMNS
            ))
            .bind(agent_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(AppError::from)?;
        let skills = skill_rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<AppResult<Vec<_>>>()?;
        let sub_agent_scope = if agent.company_id.is_none() {
            SubAgentScope::AllCompanySiblings
        } else {
            let ids = sqlx::query_scalar::<_, Uuid>(
                "SELECT sub_agent_id FROM agent_sub_agents \
                 WHERE agent_id = $1 ORDER BY position",
            )
            .bind(agent_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(AppError::from)?;
            if ids.is_empty() {
                SubAgentScope::AllCompanySiblings
            } else {
                SubAgentScope::Restricted(ids)
            }
        };
        transaction.commit().await.map_err(AppError::from)?;
        Ok(Some(StoredAgentCapabilities {
            agent,
            skills,
            sub_agent_scope,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::agent::AgentWrite;
    use crate::use_cases::builtin_agent_library::{
        BuiltinAgentDefinition, BuiltinAgentInstallOutcome, BuiltinAgentLibraryPersistence,
    };
    use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
    use crate::use_cases::user::UserPersistence;
    use serde_json::json;

    #[test]
    fn malformed_provenance_is_a_conversion_error_not_a_panic() {
        let db = AgentDb {
            memory_enabled: false,
            memory_persistence_mode: "audience_only".into(),
            memory_recall_mode: "fast".into(),
            memory_max_results: 5,
            id: Uuid::new_v4(),
            company_id: None,
            name: "Broken".into(),
            slug: "broken".into(),
            provider: None,
            model: None,
            run_timeout_secs: None,
            system_prompt: None,
            description: None,
            harness_kind: "ai_agents".into(),
            granted_tool_ids: Vec::new(),
            native_tool_policy: json!({"version": 1}),
            config_json: None,
            avatar_url: None,
            created_by: json!({}),
            created_at: Utc::now(),
        };

        let error = Agent::try_from(db).expect_err("malformed provenance must be fallible");
        assert!(error.to_string().contains("agents.created_by"));
    }

    #[tokio::test]
    async fn competing_builtin_installers_create_one_library_agent() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let definition = BuiltinAgentDefinition {
            id: Uuid::new_v4(),
            write: AgentWrite {
                name: "Built-in test agent".into(),
                slug: format!("builtin-{suffix}"),
                system_prompt: Some("Test the startup installer.".into()),
                ..AgentWrite::default()
            },
        };

        let (first, second) = tokio::join!(
            persistence.ensure_library_agent(definition.clone()),
            persistence.ensure_library_agent(definition.clone())
        );
        let outcomes = [first.unwrap(), second.unwrap()];

        assert!(outcomes.contains(&BuiltinAgentInstallOutcome::Created));
        assert!(outcomes.contains(&BuiltinAgentInstallOutcome::AlreadyPresent));
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agents WHERE company_id IS NULL AND slug = $1",
        )
        .bind(&definition.write.slug)
        .fetch_one(persistence.pool())
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

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
            CompanyWrite {
                name: "Agent Corp".to_string(),
                slug: "agent-corp".to_string(),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        let config = json!({ "version": 1 });

        let agent = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                memory_enabled: false,
                memory_persistence_mode:
                    crate::entities::memory::MemoryPersistenceMode::AudienceOnly,
                memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
                memory_max_results: 5,
                name: "Support Agent".to_string(),
                slug: "support-agent".to_string(),
                provider: Some("openai".to_string()),
                model: Some("gpt-4o".to_string()),
                run_timeout_secs: Some(45),
                system_prompt: Some("You are a helpful support agent.".to_string()),
                description: Some("Answers customer support questions.".to_string()),
                config_json: Some(config.clone()),
                avatar_url: Some(AvatarUrl::from("https://example.com/support.png")),
                created_by: None,
                ..AgentWrite::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(agent.name, "Support Agent");
        assert_eq!(agent.slug, "support-agent");
        assert_eq!(agent.provider.as_deref(), Some("openai"));
        assert_eq!(agent.model.as_deref(), Some("gpt-4o"));
        assert_eq!(agent.run_timeout_secs, Some(45));
        assert_eq!(
            agent.system_prompt.as_deref(),
            Some("You are a helpful support agent.")
        );
        assert_eq!(agent.config_json, None);
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
