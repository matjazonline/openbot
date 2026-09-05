//! Skill persistence. Every query makes company/library ownership part of the SQL predicate.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        creation::CreationProvenance,
        skill::{Skill, SkillInstruction},
        value_objects::SkillSlug,
    },
    use_cases::skill::{SkillManagementPersistence, SkillPage, SkillPageRequest, SkillWrite},
};

pub(crate) const SKILL_COLUMNS: &str = "\
    skill.id, skill.company_id, skill.slug::text AS slug, skill.name, skill.description, \
    skill.trigger, skill.instructions, skill.created_by, skill.created_at, skill.updated_at";

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct SkillDb {
    id: Uuid,
    company_id: Option<Uuid>,
    slug: String,
    name: String,
    description: String,
    trigger: String,
    instructions: serde_json::Value,
    created_by: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<SkillDb> for Skill {
    type Error = AppError;

    fn try_from(row: SkillDb) -> AppResult<Self> {
        let id = row.id;
        let skill = Skill {
            id,
            company_id: row.company_id,
            slug: SkillSlug::parse(&row.slug)
                .map_err(|error| AppError::Internal(format!("Invalid skill {id} slug: {error}")))?,
            name: row.name,
            description: row.description,
            trigger: row.trigger,
            instructions: serde_json::from_value::<Vec<SkillInstruction>>(row.instructions)
                .map_err(|error| {
                    AppError::Internal(format!("Invalid skill {id} instructions: {error}"))
                })?,
            created_by: serde_json::from_value(row.created_by).map_err(|error| {
                AppError::Internal(format!("Invalid skill {id} creation provenance: {error}"))
            })?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        };
        skill
            .validate()
            .map_err(|error| AppError::Internal(format!("Invalid skill {id}: {error}")))?;
        Ok(skill)
    }
}

fn encode_write(write: &SkillWrite) -> AppResult<(serde_json::Value, serde_json::Value)> {
    let instructions = serde_json::to_value(&write.instructions)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let created_by = serde_json::to_value(
        write
            .created_by
            .clone()
            .unwrap_or_else(CreationProvenance::system),
    )
    .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok((instructions, created_by))
}

fn write_error(error: sqlx::Error) -> AppError {
    let Some(database) = error.as_database_error() else {
        return AppError::from(error);
    };
    match database.code().as_deref() {
        Some("23505") => {
            AppError::Conflict("A skill with that slug already exists in this library.".into())
        }
        Some("23514") => AppError::BadRequest(
            "The skill relationship does not belong to the same company.".into(),
        ),
        _ => AppError::from(error),
    }
}

fn delete_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .as_deref()
        == Some("23503")
    {
        return AppError::Conflict(
            "This library skill is still used by one or more agents; remove it from them first."
                .into(),
        );
    }
    AppError::from(error)
}

async fn insert_skill<'e, E>(
    executor: E,
    company_id: Option<Uuid>,
    write: &SkillWrite,
) -> AppResult<Skill>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let mut validated = write.clone();
    validated.normalize()?;
    let (instructions, created_by) = encode_write(&validated)?;
    let row = sqlx::query_as::<_, SkillDb>(&format!(
        "INSERT INTO skills AS skill \
             (id, company_id, slug, name, description, trigger, instructions, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING {SKILL_COLUMNS}"
    ))
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(&validated.slug)
    .bind(&validated.name)
    .bind(&validated.description)
    .bind(&validated.trigger)
    .bind(instructions)
    .bind(created_by)
    .fetch_one(executor)
    .await
    .map_err(write_error)?;
    row.try_into()
}

fn copied_slug(original: &str, suffix: u8) -> String {
    if suffix == 1 {
        return original.to_string();
    }
    let ending = format!("-{suffix}");
    let stem_len = SkillSlug::MAX_CHARS - ending.len();
    format!("{}{}", &original[..original.len().min(stem_len)], ending)
}

/// Copy one library row inside the caller's transaction, retrying only slug occupancy.
pub(crate) async fn copy_library_skill_on(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    skill_id: Uuid,
    created_by: &CreationProvenance,
) -> AppResult<Skill> {
    let source = sqlx::query_as::<_, SkillDb>(&format!(
        "SELECT {SKILL_COLUMNS} FROM skills AS skill \
         WHERE skill.company_id IS NULL AND skill.id = $1"
    ))
    .bind(skill_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(AppError::from)?
    .ok_or_else(|| AppError::NotFound("Library skill not found.".into()))?;
    let source: Skill = source.try_into()?;
    let provenance =
        serde_json::to_value(created_by).map_err(|error| AppError::Internal(error.to_string()))?;
    let instructions = serde_json::to_value(&source.instructions)
        .map_err(|error| AppError::Internal(error.to_string()))?;

    for suffix in 1..=100 {
        let candidate = copied_slug(source.slug.as_str(), suffix);
        let row = sqlx::query_as::<_, SkillDb>(&format!(
            "INSERT INTO skills AS skill \
                 (id, company_id, slug, name, description, trigger, instructions, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (company_id, slug) DO NOTHING \
             RETURNING {SKILL_COLUMNS}"
        ))
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(candidate)
        .bind(&source.name)
        .bind(&source.description)
        .bind(&source.trigger)
        .bind(&instructions)
        .bind(&provenance)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(write_error)?;
        if let Some(row) = row {
            return row.try_into();
        }
    }

    Err(AppError::Conflict(
        "Could not choose an available slug after 100 bounded attempts.".into(),
    ))
}

#[async_trait]
impl SkillManagementPersistence for PostgresPersistence {
    async fn create_company(&self, company_id: Uuid, write: SkillWrite) -> AppResult<Skill> {
        insert_skill(&self.pool, Some(company_id), &write).await
    }

    async fn create_library(&self, write: SkillWrite) -> AppResult<Skill> {
        insert_skill(&self.pool, None, &write).await
    }

    async fn copy_library_to_company(
        &self,
        company_id: Uuid,
        skill_id: Uuid,
        created_by: CreationProvenance,
    ) -> AppResult<Skill> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let skill =
            copy_library_skill_on(&mut transaction, company_id, skill_id, &created_by).await?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(skill)
    }

    async fn get_company(&self, company_id: Uuid, skill_id: Uuid) -> AppResult<Option<Skill>> {
        let row = sqlx::query_as::<_, SkillDb>(&format!(
            "SELECT {SKILL_COLUMNS} FROM skills AS skill \
             WHERE skill.company_id = $1 AND skill.id = $2"
        ))
        .bind(company_id)
        .bind(skill_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn get_library(&self, skill_id: Uuid) -> AppResult<Option<Skill>> {
        let row = sqlx::query_as::<_, SkillDb>(&format!(
            "SELECT {SKILL_COLUMNS} FROM skills AS skill \
             WHERE skill.company_id IS NULL AND skill.id = $1"
        ))
        .bind(skill_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn list_company_page(
        &self,
        company_id: Uuid,
        page: SkillPageRequest,
    ) -> AppResult<SkillPage> {
        let before_at = page.before.map(|cursor| cursor.updated_at);
        let before_id = page.before.map(|cursor| cursor.id);
        let rows = sqlx::query_as::<_, SkillDb>(&format!(
            "SELECT {SKILL_COLUMNS} FROM skills AS skill \
             WHERE skill.company_id = $1 \
               AND ($2::timestamptz IS NULL OR (skill.updated_at, skill.id) < ($2, $3)) \
             ORDER BY skill.updated_at DESC, skill.id DESC LIMIT $4"
        ))
        .bind(company_id)
        .bind(before_at)
        .bind(before_id)
        .bind(i64::from(page.limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        let skills = rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<AppResult<Vec<_>>>()?;
        Ok(SkillPage::from_probe(skills, page.limit))
    }

    async fn count_company(&self, company_id: Uuid) -> AppResult<u64> {
        let count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM skills WHERE company_id = $1")
                .bind(company_id)
                .fetch_one(&self.pool)
                .await
                .map_err(AppError::from)?;
        u64::try_from(count)
            .map_err(|_| AppError::Internal("A company skill count was negative.".into()))
    }

    async fn list_library_page(&self, page: SkillPageRequest) -> AppResult<SkillPage> {
        let before_at = page.before.map(|cursor| cursor.updated_at);
        let before_id = page.before.map(|cursor| cursor.id);
        let rows = sqlx::query_as::<_, SkillDb>(&format!(
            "SELECT {SKILL_COLUMNS} FROM skills AS skill \
             WHERE skill.company_id IS NULL \
               AND ($1::timestamptz IS NULL OR (skill.updated_at, skill.id) < ($1, $2)) \
             ORDER BY skill.updated_at DESC, skill.id DESC LIMIT $3"
        ))
        .bind(before_at)
        .bind(before_id)
        .bind(i64::from(page.limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        let skills = rows
            .into_iter()
            .map(TryInto::try_into)
            .collect::<AppResult<Vec<_>>>()?;
        Ok(SkillPage::from_probe(skills, page.limit))
    }

    async fn update_company(
        &self,
        company_id: Uuid,
        skill_id: Uuid,
        write: SkillWrite,
    ) -> AppResult<Skill> {
        update_skill(&self.pool, Some(company_id), skill_id, &write).await
    }

    async fn update_library(&self, skill_id: Uuid, write: SkillWrite) -> AppResult<Skill> {
        update_skill(&self.pool, None, skill_id, &write).await
    }

    async fn delete_company(&self, company_id: Uuid, skill_id: Uuid) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM skills WHERE company_id = $1 AND id = $2")
            .bind(company_id)
            .bind(skill_id)
            .execute(&self.pool)
            .await
            .map_err(delete_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "Skill not found in this company.".into(),
            ));
        }
        Ok(())
    }

    async fn delete_library(&self, skill_id: Uuid) -> AppResult<()> {
        let result = sqlx::query("DELETE FROM skills WHERE company_id IS NULL AND id = $1")
            .bind(skill_id)
            .execute(&self.pool)
            .await
            .map_err(delete_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Library skill not found.".into()));
        }
        Ok(())
    }
}

async fn update_skill<'e, E>(
    executor: E,
    company_id: Option<Uuid>,
    skill_id: Uuid,
    write: &SkillWrite,
) -> AppResult<Skill>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let mut validated = write.clone();
    validated.normalize()?;
    let instructions = serde_json::to_value(&validated.instructions)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let row = sqlx::query_as::<_, SkillDb>(&format!(
        "UPDATE skills AS skill SET slug = $1, name = $2, description = $3, trigger = $4, \
             instructions = $5, updated_at = CURRENT_TIMESTAMP \
         WHERE skill.id = $6 AND skill.company_id IS NOT DISTINCT FROM $7 \
         RETURNING {SKILL_COLUMNS}"
    ))
    .bind(&validated.slug)
    .bind(&validated.name)
    .bind(&validated.description)
    .bind(&validated.trigger)
    .bind(instructions)
    .bind(skill_id)
    .bind(company_id)
    .fetch_optional(executor)
    .await
    .map_err(write_error)?
    .ok_or_else(|| AppError::NotFound("Skill not found in this library.".into()))?;
    row.try_into()
}

#[cfg(test)]
#[path = "skill_tests.rs"]
mod tests;
