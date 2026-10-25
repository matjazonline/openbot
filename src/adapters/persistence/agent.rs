use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::{PostgresPersistence, participant::create_agent_principal_on},
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent, creation::CreationProvenance, skill::Skill, value_objects::AvatarUrl,
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        builtin_agent_library::{
            BuiltinAgentDefinition, BuiltinAgentInstallOutcome, BuiltinAgentLibraryPersistence,
        },
        skill::{AgentCapabilityReader, StoredAgentCapabilities},
    },
};

pub(crate) const AGENT_COLUMNS: &str = "\
    agent.id, agent.company_id, agent.name, agent.slug::text AS slug, agent.provider, agent.model, \
    agent.system_prompt, agent.description, agent.avatar_url, agent.created_by, agent.created_at";

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct AgentDb {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub created_by: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<AgentDb> for Agent {
    type Error = AppError;

    fn try_from(db: AgentDb) -> AppResult<Self> {
        Ok(Agent {
            id: db.id,
            company_id: db.company_id,
            name: db.name,
            slug: db.slug,
            provider: db.provider,
            model: db.model,
            system_prompt: db.system_prompt,
            description: db.description,
            avatar_url: db.avatar_url.map(AvatarUrl::from),
            created_by: serde_json::from_value(db.created_by).map_err(|err| {
                AppError::Internal(format!("Invalid agents.created_by provenance: {err}"))
            })?,
            created_at: db.created_at,
        })
    }
}

/// The provenance column for a write, after re-running its validation.
///
/// Application use cases normalize first, but persistence is also called by provisioning adapters
/// and maintenance code. Re-running the bounded, fail-closed validation before any SQL means an
/// alternate caller cannot store a value the normal write path would reject.
fn validated_created_by(write: &AgentWrite) -> AppResult<serde_json::Value> {
    let mut validated = write.clone();
    validated.normalize()?;
    serde_json::to_value(
        write
            .created_by
            .clone()
            .unwrap_or_else(CreationProvenance::system),
    )
    .map_err(|error| AppError::Internal(error.to_string()))
}

pub(crate) async fn insert_agent_on(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    company_id: Option<Uuid>,
    write: &AgentWrite,
) -> AppResult<()> {
    let created_by = validated_created_by(write)?;
    sqlx::query(
        r#"INSERT INTO agents
           (id, company_id, name, slug, provider, model, system_prompt, description,
            avatar_url, created_by, workflow_tools)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, COALESCE($11, '{}'::text[]))"#,
    )
    .bind(id)
    .bind(company_id)
    .bind(&write.name)
    .bind(&write.slug)
    .bind(&write.provider)
    .bind(&write.model)
    .bind(&write.system_prompt)
    .bind(&write.description)
    .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
    .bind(created_by)
    .bind(write.workflow_tools.as_ref().map(|names| {
        names
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect::<Vec<_>>()
    }))
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
    write: &AgentWrite,
) -> AppResult<Vec<Skill>> {
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

    Ok(skills)
}

pub(crate) async fn replace_agent_capabilities_on(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Option<Uuid>,
    agent_id: Uuid,
    write: &AgentWrite,
) -> AppResult<()> {
    let _ = validate_agent_capabilities_on(transaction, company_id, write).await?;
    sqlx::query("DELETE FROM agent_skills WHERE agent_id = $1")
        .bind(agent_id)
        .execute(&mut **transaction)
        .await
        .map_err(AppError::from)?;

    // A statement per row, deliberately. This list comes from a person editing the agent form -- a
    // handful of skills -- so an `unnest` rewrite would buy nothing measurable. The collections
    // that arrive from outside (message participants, inbound associations) are the ones written
    // as one statement; `plan/db_audit/phase2.md` has the reasoning.
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
    Ok(())
}

fn capability_relationship_error(error: sqlx::Error) -> AppError {
    let Some(database) = error.as_database_error() else {
        return AppError::from(error);
    };
    match database.constraint() {
        Some("agent_skills_agent_scope_check") | Some("agent_skills_skill_scope_check") => {
            AppError::BadRequest("The selected skill belongs to another library.".into())
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

/// Save an agent's settings and capabilities in one transaction.
pub(crate) async fn update_agent(
    persistence: &PostgresPersistence,
    id: Uuid,
    write: AgentWrite,
) -> AppResult<Agent> {
    validated_created_by(&write)?;
    let mut tx = persistence.pool.begin().await.map_err(AppError::from)?;
    let company_id: Option<Uuid> =
        sqlx::query_scalar("SELECT company_id FROM agents WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(AppError::from)?
            .ok_or_else(crate::use_cases::agent::agent_not_found)?;
    replace_agent_capabilities_on(&mut tx, company_id, id, &write).await?;
    let db = sqlx::query_as::<_, AgentDb>(&format!(
        r#"UPDATE agents AS agent
           SET name = $1, slug = $2, provider = $3, model = $4, system_prompt = $5,
               description = $6, avatar_url = $7,
               workflow_tools = COALESCE($8, agent.workflow_tools)
           WHERE id = $9
           RETURNING {AGENT_COLUMNS}"#
    ))
    .bind(&write.name)
    .bind(&write.slug)
    .bind(&write.provider)
    .bind(&write.model)
    .bind(&write.system_prompt)
    .bind(&write.description)
    .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
    .bind(write.workflow_tools.as_ref().map(|names| {
        names
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect::<Vec<_>>()
    }))
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(slug_conflict)?;
    tx.commit().await.map_err(AppError::from)?;
    db.try_into()
}

fn slug_conflict(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .and_then(|db| db.code())
        .as_deref()
        == Some("23505")
    {
        return AppError::Conflict("An agent with that slug already exists.".into());
    }
    AppError::from(error)
}

#[async_trait]
impl AgentPersistence for PostgresPersistence {
    async fn create(&self, company_id: Uuid, write: AgentWrite) -> AppResult<Agent> {
        let id = Uuid::new_v4();
        let mut transaction = self.pool.begin().await?;
        insert_agent_on(&mut transaction, id, Some(company_id), &write).await?;
        create_agent_principal_on(&mut transaction, company_id, id, &write.name).await?;
        let db = sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent WHERE id = $1"
        ))
        .bind(id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        db.try_into()
    }

    async fn create_library(&self, write: AgentWrite) -> AppResult<Agent> {
        let id = Uuid::new_v4();
        let mut transaction = self.pool.begin().await?;
        insert_agent_on(&mut transaction, id, None, &write).await?;
        let db = sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent WHERE id = $1"
        ))
        .bind(id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
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
        update_agent(self, id, write).await
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
    async fn load_capabilities(
        &self,
        company_id: Uuid,
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
        .bind(company_id)
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
        transaction.commit().await.map_err(AppError::from)?;
        Ok(Some(StoredAgentCapabilities { agent, skills }))
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
            id: Uuid::new_v4(),
            company_id: None,
            name: "Broken".into(),
            slug: "broken".into(),
            provider: None,
            model: None,
            system_prompt: None,
            description: None,
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
                ..Default::default()
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

        let agent = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                name: "Support Agent".to_string(),
                slug: "support-agent".to_string(),
                provider: Some("openai".to_string()),
                model: Some("gpt-4o".to_string()),
                system_prompt: Some("You are a helpful support agent.".to_string()),
                description: Some("Answers customer support questions.".to_string()),
                avatar_url: Some(AvatarUrl::from("https://example.com/support.png")),
                created_by: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(agent.name, "Support Agent");
        assert_eq!(agent.slug, "support-agent");
        assert_eq!(agent.provider.as_deref(), Some("openai"));
        assert_eq!(agent.model.as_deref(), Some("gpt-4o"));
        assert_eq!(
            agent.system_prompt.as_deref(),
            Some("You are a helpful support agent.")
        );
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
                ..Default::default()
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
