use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{user::User, value_objects::AvatarUrl},
    use_cases::user::{
        PendingRegistration, ProfileUpdate, RegistrationPersistence, UserPersistence,
    },
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

#[async_trait]
impl RegistrationPersistence for PostgresPersistence {
    async fn save_pending_registration(&self, pending: PendingRegistration<'_>) -> AppResult<()> {
        sqlx::query(
            r#"INSERT INTO pending_user_registrations
                   (email, username, password_hash, confirmation_code_hash, expires_at)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (email) DO UPDATE SET
                   username = EXCLUDED.username,
                   password_hash = EXCLUDED.password_hash,
                   confirmation_code_hash = EXCLUDED.confirmation_code_hash,
                   expires_at = EXCLUDED.expires_at,
                   created_at = CURRENT_TIMESTAMP"#,
        )
        .bind(pending.email.as_str())
        .bind(pending.username)
        .bind(pending.password_hash)
        .bind(pending.confirmation_code_hash)
        .bind(pending.expires_at)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(())
    }

    async fn confirm_pending_registration(
        &self,
        email: &crate::entities::value_objects::EmailAddress,
        confirmation_code_hash: &str,
    ) -> AppResult<Option<User>> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let user = sqlx::query_as::<_, UserDb>(
            r#"INSERT INTO users (id, username, email, password_hash)
               SELECT $3, pending.username, pending.email, pending.password_hash
               FROM pending_user_registrations AS pending
               WHERE pending.email = $1
                 AND pending.confirmation_code_hash = $2
                 AND pending.expires_at > CURRENT_TIMESTAMP
               RETURNING id, username, email, password_hash, avatar_url, created_at"#,
        )
        .bind(email.as_str())
        .bind(confirmation_code_hash)
        .bind(Uuid::new_v4())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        if user.is_some() {
            sqlx::query("DELETE FROM pending_user_registrations WHERE email = $1")
                .bind(email.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(AppError::from)?;
        }
        transaction.commit().await.map_err(AppError::from)?;
        Ok(user.map(Into::into))
    }
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
    async fn create_user(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> AppResult<User> {
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

    async fn update_profile(
        &self,
        id: Uuid,
        profile: ProfileUpdate<'_>,
    ) -> AppResult<Option<User>> {
        let result = sqlx::query_as!(
            UserDb,
            r#"UPDATE users SET username = $2, email = $3, avatar_url = $4 WHERE id = $1
               RETURNING id, username, email, password_hash, avatar_url, created_at as "created_at!""#,
            id,
            profile.username,
            profile.email.as_str(),
            profile.avatar_url.map(AvatarUrl::as_str)
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| taken_identity_error(err, &profile))?;

        Ok(result.map(Into::into))
    }

    async fn update_password_hash(&self, id: Uuid, password_hash: &str) -> AppResult<Option<User>> {
        let result = sqlx::query_as!(
            UserDb,
            r#"UPDATE users SET password_hash = $2 WHERE id = $1
               RETURNING id, username, email, password_hash, avatar_url, created_at as "created_at!""#,
            id,
            password_hash
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.map(Into::into))
    }
}

/// A username or address somebody else already holds is routine user input, not a database fault,
/// so it surfaces as a bad request naming the field that clashed rather than a raw driver message.
///
/// Which of the two clashed comes from the constraint the row violated, because the statement
/// writes both at once and the message has to point at the field the reader must change.
fn taken_identity_error(err: sqlx::Error, profile: &ProfileUpdate<'_>) -> AppError {
    let sqlx::Error::Database(ref db_err) = err else {
        return AppError::from(err);
    };
    if db_err.code().as_deref() != Some("23505") {
        return AppError::from(err);
    }

    match db_err.constraint() {
        Some("users_username_key") => AppError::BadRequest(format!(
            "The username '{}' is already taken.",
            profile.username
        )),
        Some("users_email_key") => AppError::BadRequest(format!(
            "An account already uses the address '{}'.",
            profile.email
        )),
        _ => AppError::BadRequest("That username or email is already taken.".into()),
    }
}
