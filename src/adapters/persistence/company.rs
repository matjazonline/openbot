use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::company::Company,
    use_cases::company::CompanyPersistence,
};

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct CompanyDb {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: NaiveDateTime,
}

impl From<CompanyDb> for Company {
    fn from(db: CompanyDb) -> Self {
        Company {
            id: db.id,
            user_id: db.user_id,
            name: db.name,
            slug: db.slug,
            created_at: db.created_at,
        }
    }
}

#[async_trait]
impl CompanyPersistence for PostgresPersistence {
    async fn create(&self, user_id: Uuid, name: &str, slug: &str) -> AppResult<Company> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as!(
            CompanyDb,
            r#"INSERT INTO companies (id, user_id, name, slug) 
               VALUES ($1, $2, $3, $4) 
               RETURNING id, user_id, name, slug, created_at as "created_at!""#,
            uuid,
            user_id,
            name,
            slug
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
        let db = sqlx::query_as!(
            CompanyDb,
            r#"SELECT id, user_id, name, slug, created_at as "created_at!" 
               FROM companies WHERE id = $1"#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        let db = sqlx::query_as!(
            CompanyDb,
            r#"SELECT id, user_id, name, slug, created_at as "created_at!" 
               FROM companies WHERE LOWER(slug) = LOWER($1)"#,
            slug
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        let db_list = sqlx::query_as!(
            CompanyDb,
            r#"SELECT id, user_id, name, slug, created_at as "created_at!" 
               FROM companies WHERE user_id = $1 ORDER BY created_at DESC"#,
            user_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db_list.into_iter().map(Into::into).collect())
    }

    async fn update(&self, id: Uuid, name: &str, slug: &str) -> AppResult<Company> {
        let db = sqlx::query_as!(
            CompanyDb,
            r#"UPDATE companies SET name = $1, slug = $2 
               WHERE id = $3 
               RETURNING id, user_id, name, slug, created_at as "created_at!""#,
            name,
            slug,
            id
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        sqlx::query!("DELETE FROM companies WHERE id = $1", id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(())
    }
}
