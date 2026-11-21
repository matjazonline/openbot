use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        company::{Company, CompanyAccess, CompanyChannelDefaults, CompanyModelConnection},
        company_member::CompanyMembership,
        value_objects::{AvatarUrl, CompanySlug, ModelName, ModelProvider},
    },
    use_cases::company::{
        CompanyModelConnectionWrite, CompanyPersistence, CompanyWrite, company_not_found,
    },
};

mod member_retirement;
mod root;
pub(crate) use member_retirement::retire_member_on;
pub(crate) use root::{
    RootJoin, bootstrap_root_on, join_root_on, root_member_on, root_on, set_admin_on,
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyDb {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub slug: String,
    pub default_add_3rd_party: bool,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<CompanyDb> for Company {
    fn from(db: CompanyDb) -> Self {
        Company {
            id: db.id,
            user_id: db.user_id,
            name: db.name,
            slug: CompanySlug::from(db.slug),
            channel_defaults: CompanyChannelDefaults {
                add_3rd_party: db.default_add_3rd_party,
            },
            avatar_url: db.avatar_url.map(AvatarUrl::from),
            created_at: db.created_at,
        }
    }
}

/// A company row plus its caller's canonical root membership and knowledge level.
#[derive(sqlx::FromRow, Debug)]
struct AccessibleCompanyDb {
    #[sqlx(flatten)]
    company: CompanyDb,
    role: String,
    access_level: i16,
    administers_units: bool,
}

impl TryFrom<AccessibleCompanyDb> for CompanyAccess {
    type Error = AppError;

    fn try_from(db: AccessibleCompanyDb) -> AppResult<Self> {
        let membership = match db.role.as_str() {
            "owner" => CompanyMembership::Owner,
            "admin" => CompanyMembership::Admin,
            "member" => CompanyMembership::Member,
            // The role CHECK constraint makes this unreachable; a new role must be mapped here
            // rather than read as some lesser access.
            other => {
                return Err(AppError::Internal(format!(
                    "Unknown company membership role '{other}'"
                )));
            }
        };
        let access_level = crate::entities::access_level::AccessLevel::try_from(db.access_level)
            .map_err(|error| {
                AppError::Internal(format!(
                    "Invalid stored company level {}: {error}",
                    db.company.id
                ))
            })?;
        if membership.is_owner() && access_level != crate::entities::access_level::AccessLevel::MAX
        {
            return Err(AppError::Internal(format!(
                "Invalid stored owner level {}",
                db.company.id
            )));
        }
        Ok(CompanyAccess {
            company: db.company.into(),
            membership,
            access_level,
            administers_units: db.administers_units,
        })
    }
}

/// The columns of [`AccessibleCompanyDb`], joined from `company` and `member`.
const ACCESSIBLE_COMPANY_COLUMNS: &str = "company.id, company.user_id, company.name, company.slug, \
     company.default_add_3rd_party, company.avatar_url, company.created_at, member.role, member.access_level::smallint AS access_level, \
     any_unit_management_role($1, company.id) IS NOT NULL AS administers_units";

/// How many dependent agents a rejected model-connection change names before it stops listing
/// them. The message is for a person reading a form, not an inventory.
const MAX_REPORTED_ORPHANED_AGENTS: usize = 20;

async fn delete_company_with_cleanup(
    persistence: &PostgresPersistence,
    id: Uuid,
    owner_id: Option<Uuid>,
) -> AppResult<bool> {
    let mut transaction = persistence.pool.begin().await.map_err(AppError::from)?;
    // Lock the company first (memory E2): an enable or a lazy provisioning either committed
    // before this, so its lifecycle is retired below, or waits and then finds no company.
    let locked: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM companies WHERE id = $1 AND ($2::uuid IS NULL OR user_id = $2) FOR UPDATE",
    )
    .bind(id)
    .bind(owner_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(AppError::from)?;
    if locked.is_none() {
        return Ok(false);
    }
    super::integrations::lifecycle::revoke_company_on(
        &mut transaction,
        &crate::adapters::integrations::owner::client_namespace(),
        &crate::adapters::integrations::owner::company_binding(id),
    )
    .await
    .map_err(AppError::from)?;
    super::memory::retire_company_on(&mut transaction, id).await?;
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
    transaction.commit().await.map_err(AppError::from)?;
    Ok(true)
}

#[async_trait]
impl CompanyPersistence for PostgresPersistence {
    async fn create(&self, user_id: Uuid, write: CompanyWrite) -> AppResult<Company> {
        let uuid = Uuid::new_v4();
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(AppError::from)?;
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"INSERT INTO companies (id, user_id, name, slug, avatar_url, default_add_3rd_party)
               VALUES ($1, $2, $3, $4, $5, $6)
               RETURNING id, user_id, name, slug, default_add_3rd_party, avatar_url, created_at"#,
        )
        .bind(uuid)
        .bind(user_id)
        .bind(&write.name)
        .bind(&write.slug)
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(write.channel_defaults.add_3rd_party)
        .fetch_one(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        let company: Company = db.into();
        bootstrap_root_on(&mut transaction, &company).await?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(company)
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug, default_add_3rd_party, avatar_url, created_at
               FROM companies WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug, default_add_3rd_party, avatar_url, created_at
               FROM companies WHERE slug = $1"#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        let db_list = sqlx::query_as::<_, CompanyDb>(
            r#"SELECT id, user_id, name, slug, default_add_3rd_party, avatar_url, created_at
               FROM companies WHERE user_id = $1
               ORDER BY created_at DESC, id DESC LIMIT 200"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
    }

    async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
        // Feeds switchers and default selection only: an explicitly requested company is
        // authorized by `company_access`, so this cap never denies anything.
        let db_list = sqlx::query_as::<_, AccessibleCompanyDb>(&format!(
            r#"SELECT {ACCESSIBLE_COMPANY_COLUMNS}
               FROM department_members AS current_member
               JOIN companies AS company ON company.id = current_member.company_id
               JOIN LATERAL membership_of($1, company.id) AS member ON member.id = current_member.id
               WHERE current_member.root_user_id = $1
               ORDER BY company.created_at DESC, company.id DESC LIMIT 200"#
        ))
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn company_access(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>> {
        let db = sqlx::query_as::<_, AccessibleCompanyDb>(&format!(
            r#"SELECT {ACCESSIBLE_COMPANY_COLUMNS}
               FROM companies AS company
               JOIN LATERAL membership_of($1, company.id) AS member ON true
               WHERE company.id = $2"#
        ))
        .bind(user_id)
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn update(&self, id: Uuid, write: CompanyWrite) -> AppResult<Company> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"UPDATE companies SET name = $1, slug = $2, avatar_url = $3,
                      default_add_3rd_party = $4
               WHERE id = $5
               RETURNING id, user_id, name, slug, default_add_3rd_party, avatar_url, created_at"#,
        )
        .bind(&write.name)
        .bind(&write.slug)
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(write.channel_defaults.add_3rd_party)
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        delete_company_with_cleanup(self, id, None).await?;
        Ok(())
    }

    async fn update_for_user(
        &self,
        user_id: Uuid,
        id: Uuid,
        write: CompanyWrite,
    ) -> AppResult<Company> {
        let db = sqlx::query_as::<_, CompanyDb>(
            r#"UPDATE companies
               SET name = $1, slug = $2, avatar_url = $3, default_add_3rd_party = $4
               WHERE id = $5 AND user_id = $6
               RETURNING id, user_id, name, slug, default_add_3rd_party, avatar_url, created_at"#,
        )
        .bind(&write.name)
        .bind(&write.slug)
        .bind(write.avatar_url.as_ref().map(AvatarUrl::as_str))
        .bind(write.channel_defaults.add_3rd_party)
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?
        .ok_or_else(company_not_found)?;
        Ok(db.into())
    }

    async fn delete_for_user(&self, user_id: Uuid, id: Uuid) -> AppResult<()> {
        if !delete_company_with_cleanup(self, id, Some(user_id)).await? {
            return Err(company_not_found());
        }
        Ok(())
    }

    async fn list_model_connections(
        &self,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyModelConnection>> {
        let mut db = self.pool.acquire().await?;
        model_connections_on(&mut db, company_id).await
    }

    async fn model_api_key(
        &self,
        company_id: Uuid,
        provider: &ModelProvider,
    ) -> AppResult<Option<String>> {
        let mut db = self.pool.acquire().await?;
        model_api_key_on(self, &mut db, company_id, provider).await
    }

    async fn replace_model_connections_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        replace_unit_model_connections(self, user_id, company_id, None, connections).await
    }

    async fn replace_unit_model_connections_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        replace_unit_model_connections(self, user_id, company_id, Some(unit_id), connections).await
    }

    async fn set_unit_budget(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        period: &str,
        cap_micros: i64,
    ) -> AppResult<()> {
        let period = super::unit_budgets::period(period)?;
        let mut tx = self.pool.begin().await?;
        super::unit_budgets::set_on(
            &mut tx,
            crate::application::workflow::WorkflowActor::authenticated(user_id)?,
            crate::domain::workflow::CompanyId::new(company_id),
            crate::entities::department::DepartmentId::new(unit_id),
            period,
            cap_micros,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn replace_unit_budgets(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        day_micros: Option<i64>,
        month_micros: Option<i64>,
    ) -> AppResult<()> {
        let actor = crate::application::workflow::WorkflowActor::authenticated(user_id)?;
        let company = crate::domain::workflow::CompanyId::new(company_id);
        let unit = crate::entities::department::DepartmentId::new(unit_id);
        // Both periods in one transaction: a refused amount leaves both caps as they were.
        let mut tx = self.pool.begin().await?;
        for (period, cap) in [("day", day_micros), ("month", month_micros)] {
            match cap {
                Some(cap) => {
                    super::unit_budgets::set_on(&mut tx, actor, company, unit, period, cap).await?
                }
                None => {
                    super::unit_budgets::clear_on(&mut tx, actor, company, unit, period).await?
                }
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn unit_budget_states(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
    ) -> AppResult<Vec<crate::application::ports::unit_resources::UnitBudgetState>> {
        let mut tx = self.pool.begin().await?;
        let states = super::unit_budgets::states_on(
            &mut tx,
            crate::application::workflow::WorkflowActor::authenticated(user_id)?,
            crate::domain::workflow::CompanyId::new(company_id),
            crate::entities::department::DepartmentId::new(unit_id),
        )
        .await?;
        tx.commit().await?;
        Ok(states)
    }

    async fn clear_unit_budget(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        period: &str,
    ) -> AppResult<()> {
        let period = super::unit_budgets::period(period)?;
        let mut tx = self.pool.begin().await?;
        super::unit_budgets::clear_on(
            &mut tx,
            crate::application::workflow::WorkflowActor::authenticated(user_id)?,
            crate::domain::workflow::CompanyId::new(company_id),
            crate::entities::department::DepartmentId::new(unit_id),
            period,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn model_connections_in_agent_reach(
        &self,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Vec<Uuid>> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM company_model_connections \
             WHERE company_id = $1 AND agent_reaches($1, $2, unit_id)",
        )
        .bind(company_id)
        .bind(agent_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn move_model_connection(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
        target: Uuid,
    ) -> AppResult<crate::application::ports::unit_resources::MovedResource> {
        let mut tx = self.pool.begin().await?;
        let moved = super::unit_resources::move_resource_on(
            &mut tx,
            crate::application::workflow::WorkflowActor::authenticated(user_id)?,
            crate::domain::workflow::CompanyId::new(company_id),
            crate::application::ports::unit_resources::UnitResourceKind::ModelConnection,
            id,
            crate::entities::department::DepartmentId::new(target),
        )
        .await?;
        tx.commit().await?;
        Ok(moved)
    }
}

/// Replace the model connections of one unit, the root when `unit` is `None` (Plan07 02A,
/// decisions a and g): whoever manages the unit, under the company and person locks every
/// replace and move takes. Other units' rows are untouched; a provider another unit owns is a
/// conflict, never an overwrite of its key.
async fn replace_unit_model_connections(
    persistence: &PostgresPersistence,
    user_id: Uuid,
    company_id: Uuid,
    unit: Option<Uuid>,
    connections: Vec<CompanyModelConnectionWrite>,
) -> AppResult<()> {
    let company = crate::domain::workflow::CompanyId::new(company_id);
    let actor = crate::application::workflow::WorkflowActor::authenticated(user_id)?;
    let mut transaction = persistence.pool.begin().await.map_err(AppError::from)?;
    super::workflow::authority::authorize_person_with_locks(
        &mut transaction,
        company,
        actor,
        super::workflow::row_locks::RowLocks::Wait,
    )
    .await
    .map_err(|error| match error {
        AppError::NotFound(_) => crate::use_cases::company::company_not_found(),
        other => other,
    })?;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id = $1 AND is_root")
            .bind(company_id)
            .fetch_one(&mut *transaction)
            .await?;
    // A department's form may empty its own unit, but never the company's last connection.
    let named = unit.is_some();
    let unit = unit.unwrap_or(root);
    super::administration::management_role_on(
        &mut transaction,
        user_id,
        company,
        crate::application::ports::administration::ManagementTarget::Unit(
            crate::entities::department::DepartmentId::new(unit),
        ),
    )
    .await?
    .ok_or_else(crate::use_cases::company::company_not_found)?;

    sqlx::query(
        "UPDATE company_model_connections SET default_for_unit = FALSE \
         WHERE company_id = $1 AND unit_id = $2 AND default_for_unit",
    )
    .bind(company_id)
    .bind(unit)
    .execute(&mut *transaction)
    .await?;

    for connection in &connections {
        let encrypted_api_key = persistence.encrypt_credential(connection.api_key.as_deref())?;
        let models: Vec<String> = connection
            .models
            .iter()
            .map(|model| model.as_str().to_string())
            .collect();
        let changed = match encrypted_api_key {
            // Updates only a row this unit owns: another unit's provider is never rewritten.
            Some(api_key) => sqlx::query(
                r#"INSERT INTO company_model_connections
                       (company_id, unit_id, provider, api_key, models, default_for_unit)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (company_id, provider) DO UPDATE
                   SET api_key = EXCLUDED.api_key, models = EXCLUDED.models,
                       default_for_unit = EXCLUDED.default_for_unit, updated_at = CURRENT_TIMESTAMP
                   WHERE company_model_connections.unit_id = EXCLUDED.unit_id"#,
            )
            .bind(company_id)
            .bind(unit)
            .bind(connection.provider.as_str())
            .bind(api_key)
            .bind(&models)
            .bind(connection.is_default)
            .execute(&mut *transaction)
            .await?
            .rows_affected(),
            None => sqlx::query(
                r#"UPDATE company_model_connections
                   SET models = $4, default_for_unit = $5, updated_at = CURRENT_TIMESTAMP
                   WHERE company_id = $1 AND unit_id = $2 AND provider = $3"#,
            )
            .bind(company_id)
            .bind(unit)
            .bind(connection.provider.as_str())
            .bind(&models)
            .bind(connection.is_default)
            .execute(&mut *transaction)
            .await?
            .rows_affected(),
        };
        if changed == 0 {
            let owner: Option<String> = sqlx::query_scalar(
                "SELECT department.name FROM company_model_connections AS connection \
                 JOIN departments AS department ON department.company_id = connection.company_id \
                  AND department.id = connection.unit_id \
                 WHERE connection.company_id = $1 AND connection.provider = $2 \
                   AND connection.unit_id <> $3",
            )
            .bind(company_id)
            .bind(connection.provider.as_str())
            .bind(unit)
            .fetch_optional(&mut *transaction)
            .await?;
            return Err(match owner {
                Some(owner) => AppError::Conflict(format!(
                    "Provider '{}' is configured by the department {owner}. Edit it there, or move \
                     it here.",
                    connection.provider
                )),
                None => AppError::BadRequest(format!(
                    "An API key is required when adding provider '{}'.",
                    connection.provider
                )),
            });
        }
    }

    let retained: Vec<String> = connections
        .iter()
        .map(|connection| connection.provider.as_str().to_string())
        .collect();
    sqlx::query(
        "DELETE FROM company_model_connections \
         WHERE company_id = $1 AND unit_id = $2 AND NOT (provider = ANY($3))",
    )
    .bind(company_id)
    .bind(unit)
    .bind(&retained)
    .execute(&mut *transaction)
    .await?;

    check_model_connection_invariants_on(&mut transaction, company_id, root).await?;
    if named {
        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM company_model_connections WHERE company_id = $1",
        )
        .bind(company_id)
        .fetch_one(&mut *transaction)
        .await?;
        if remaining == 0 {
            return Err(AppError::BadRequest(
                "A company must keep at least one model provider configured.".into(),
            ));
        }
    }

    // Agents are validated against the connections when *they* are written; this is the other
    // direction. Narrowing a `models` list or dropping a provider out from under a pinned
    // agent would otherwise commit cleanly and only surface much later, per run, as
    // "Model 'x' is not enabled for provider 'y'" on a task that had been working. The check
    // runs inside the transaction, so a refusal rolls the whole replace back.
    let orphaned: Vec<String> = sqlx::query_scalar(
        r#"SELECT agent.name
           FROM agents AS agent
           WHERE agent.company_id = $1
             AND agent.provider IS NOT NULL
             AND NOT EXISTS (
                 SELECT 1 FROM company_model_connections AS connection
                 WHERE connection.company_id = agent.company_id
                   AND connection.provider = lower(btrim(agent.provider))
                   AND agent.model = ANY(connection.models)
             )
           ORDER BY agent.name
           LIMIT $2"#,
    )
    .bind(company_id)
    .bind(i64::try_from(MAX_REPORTED_ORPHANED_AGENTS).unwrap_or(i64::MAX))
    .fetch_all(&mut *transaction)
    .await?;
    if !orphaned.is_empty() {
        return Err(AppError::BadRequest(format!(
            "{} still {} a model this change removes. Point {} at an enabled model first.",
            orphaned.join(", "),
            if orphaned.len() == 1 { "uses" } else { "use" },
            if orphaned.len() == 1 { "it" } else { "them" },
        )));
    }

    transaction.commit().await.map_err(AppError::from)
}

/// The root holds exactly one default whenever it holds a connection (Plan07 G1 r2 D1); checked
/// after every replace and move. A replace never empties the root's own form (the use case
/// refuses that), as before.
pub(super) async fn check_model_connection_invariants_on(
    db: &mut sqlx::PgConnection,
    company_id: Uuid,
    root: Uuid,
) -> AppResult<()> {
    let (root_connections, root_defaults): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE default_for_unit) \
         FROM company_model_connections WHERE company_id = $1 AND unit_id = $2",
    )
    .bind(company_id)
    .bind(root)
    .fetch_one(&mut *db)
    .await?;
    if root_connections > 0 && root_defaults != 1 {
        return Err(AppError::BadRequest(
            "Exactly one of the company's own model providers must be the company default.".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "company_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "company_access_tests.rs"]
mod access_tests;

pub(crate) async fn model_connections_on(
    db: &mut sqlx::PgConnection,
    company_id: Uuid,
) -> AppResult<Vec<CompanyModelConnection>> {
    // The root's default first, then each unit's own (Plan07 02A).
    let rows = sqlx::query_as::<_, (Uuid, Uuid, String, bool, String, Vec<String>, bool, bool)>(
        r#"SELECT connection.id, connection.unit_id, unit.name, unit.is_root, connection.provider,
                  connection.models, connection.default_for_unit,
                  (connection.api_key IS NOT NULL) AS has_api_key
               FROM company_model_connections AS connection
               JOIN departments AS unit
                 ON unit.company_id = connection.company_id AND unit.id = connection.unit_id
               WHERE connection.company_id = $1
               ORDER BY (unit.is_root AND connection.default_for_unit) DESC, connection.provider"#,
    )
    .bind(company_id)
    .fetch_all(db)
    .await
    .map_err(AppError::from)?;

    Ok(rows
        .into_iter()
        .map(
            |(id, unit_id, unit_name, in_root, provider, models, is_default, has_api_key)| {
                CompanyModelConnection {
                    id,
                    unit_id,
                    unit_name,
                    in_root,
                    provider: ModelProvider::from(provider),
                    models: models.into_iter().map(ModelName::from).collect(),
                    is_default,
                    has_api_key,
                }
            },
        )
        .collect())
}

pub(crate) async fn model_api_key_on(
    persistence: &PostgresPersistence,
    db: &mut sqlx::PgConnection,
    company_id: Uuid,
    provider: &ModelProvider,
) -> AppResult<Option<String>> {
    // Folded by `ModelProvider::canonical` before it gets here, so the column is compared as
    // stored rather than through a function that would also hide a miscased write.
    let stored: Option<String> = sqlx::query_scalar(
        r#"SELECT api_key FROM company_model_connections
               WHERE company_id = $1 AND provider = $2"#,
    )
    .bind(company_id)
    .bind(provider.as_str())
    .fetch_optional(db)
    .await
    .map_err(AppError::from)?;
    persistence.decrypt_credential(stored)
}
