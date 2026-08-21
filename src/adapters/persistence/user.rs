use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{user::User, value_objects::AvatarUrl},
    use_cases::user::UserPersistence,
};

// User struct as stored in the db.
#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct UserDb {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<UserDb> for User {
    fn from(user_db: UserDb) -> Self {
        User {
            id: user_db.id,
            username: user_db.username,
            email: user_db.email,
            password_hash: user_db.password_hash,
            avatar_url: user_db.avatar_url.map(AvatarUrl::from),
            created_at: user_db.created_at,
        }
    }
}

#[async_trait]
impl UserPersistence for PostgresPersistence {
    async fn create_user(&self, username: &str, email: &str, password_hash: &str) -> AppResult<User> {
        let uuid = Uuid::new_v4();

        let db = sqlx::query_as!(
            UserDb,
            r#"INSERT INTO users (id, username, email, password_hash)
               VALUES ($1, $2, $3, $4)
               RETURNING id, username, email, password_hash, avatar_url, created_at as "created_at!""#,
            uuid,
            username,
            email,
            password_hash
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_by_email(&self, email: &str) -> AppResult<Option<User>> {
        let result = sqlx::query_as!(
            UserDb,
            r#"SELECT id, username, email, password_hash, avatar_url, created_at as "created_at!" FROM users WHERE email = $1"#,
            email
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.map(Into::into))
    }

    async fn get_by_username(&self, username: &str) -> AppResult<Option<User>> {
        let result = sqlx::query_as!(
            UserDb,
            r#"SELECT id, username, email, password_hash, avatar_url, created_at as "created_at!" FROM users WHERE username = $1"#,
            username
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.map(Into::into))
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<User>> {
        let result = sqlx::query_as!(
            UserDb,
            r#"SELECT id, username, email, password_hash, avatar_url, created_at as "created_at!" FROM users WHERE id = $1"#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.map(Into::into))
    }

    async fn update_avatar_url(
        &self,
        id: Uuid,
        avatar_url: Option<&AvatarUrl>,
    ) -> AppResult<Option<User>> {
        let result = sqlx::query_as!(
            UserDb,
            r#"UPDATE users SET avatar_url = $2 WHERE id = $1
               RETURNING id, username, email, password_hash, avatar_url, created_at as "created_at!""#,
            id,
            avatar_url.map(AvatarUrl::as_str)
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.map(Into::into))
    }
}
