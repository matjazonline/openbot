use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        user::User,
        value_objects::{AvatarUrl, EmailAddress},
    },
    use_cases::user::{
        AccountChange, AccountChangeKind, AccountChangePersistence, ExternalIdentity, LoginMethod,
        PendingAccountChange, PendingChange, PendingRegistration, ProfileUpdate,
        RegistrationPersistence, UserPersistence,
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

        if let Some(user) = &user {
            sqlx::query(
                "INSERT INTO user_login_methods (user_id, provider) VALUES ($1, 'password')",
            )
            .bind(user.id)
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;
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

        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let db = sqlx::query_as::<_, UserDb>(
            r#"INSERT INTO users (id, username, email, password_hash)
               VALUES ($1, $2, $3, $4)
               RETURNING id, username, email, password_hash, avatar_url, created_at"#,
        )
        .bind(uuid)
        .bind(username)
        .bind(email)
        .bind(password_hash)
        .fetch_one(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        sqlx::query("INSERT INTO user_login_methods (user_id, provider) VALUES ($1, 'password')")
            .bind(uuid)
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;
        transaction.commit().await.map_err(AppError::from)?;

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

    async fn activate_password(&self, id: Uuid, password_hash: &str) -> AppResult<Option<User>> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let result = sqlx::query_as::<_, UserDb>(
            r#"UPDATE users SET password_hash = $2 WHERE id = $1
               RETURNING id, username, email, password_hash, avatar_url, created_at"#,
        )
        .bind(id)
        .bind(password_hash)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        if result.is_some() {
            sqlx::query(
                "INSERT INTO user_login_methods (user_id, provider) VALUES ($1, 'password') ON CONFLICT (user_id, provider) DO NOTHING",
            )
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;
        }
        transaction.commit().await.map_err(AppError::from)?;
        Ok(result.map(Into::into))
    }

    async fn has_login_method(&self, id: Uuid, method: LoginMethod) -> AppResult<bool> {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM user_login_methods WHERE user_id = $1 AND provider = $2)",
        )
        .bind(id)
        .bind(method.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)
    }

    async fn create_external_user(&self, identity: ExternalIdentity<'_>) -> AppResult<User> {
        if identity.provider == LoginMethod::Password {
            return Err(AppError::BadRequest(
                "Password registration is not an external-provider flow.".into(),
            ));
        }
        let id = Uuid::new_v4();
        let base = external_username(identity.display_name, identity.email.as_str());
        let username = format!("{}-{}", base, &id.simple().to_string()[..8]);
        // Not an Argon2 hash, and password authentication checks the method table before parsing.
        let password_hash = format!("{}-only:{id}", identity.provider.as_str());
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let user = sqlx::query_as::<_, UserDb>(
            r#"INSERT INTO users (id, username, email, password_hash)
               VALUES ($1, $2, $3, $4)
               RETURNING id, username, email, password_hash, avatar_url, created_at"#,
        )
        .bind(id)
        .bind(username)
        .bind(identity.email.as_str())
        .bind(password_hash)
        .fetch_one(&mut *transaction)
        .await
        .map_err(external_account_conflict)?;
        sqlx::query(
            "INSERT INTO user_login_methods (user_id, provider, provider_subject) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(identity.provider.as_str())
        .bind(identity.subject)
        .execute(&mut *transaction)
        .await
        .map_err(external_identity_conflict)?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(user.into())
    }

    async fn get_by_external_subject(
        &self,
        provider: LoginMethod,
        subject: &str,
    ) -> AppResult<Option<User>> {
        sqlx::query_as::<_, UserDb>(
            r#"SELECT users.id, users.username, users.email, users.password_hash,
                      users.avatar_url, users.created_at
               FROM users
               JOIN user_login_methods methods ON methods.user_id = users.id
               WHERE methods.provider = $1 AND methods.provider_subject = $2"#,
        )
        .bind(provider.as_str())
        .bind(subject)
        .fetch_optional(&self.pool)
        .await
        .map(|user| user.map(Into::into))
        .map_err(AppError::from)
    }

    async fn link_external_identity(
        &self,
        user_id: Uuid,
        identity: ExternalIdentity<'_>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO user_login_methods (user_id, provider, provider_subject) VALUES ($1, $2, $3)",
        )
        .bind(user_id)
        .bind(identity.provider.as_str())
        .bind(identity.subject)
        .execute(&self.pool)
        .await
        .map_err(external_identity_conflict)?;
        Ok(())
    }
}

fn external_username(display_name: Option<&str>, email: &str) -> String {
    let source = display_name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| email.split('@').next().unwrap_or("user"));
    let normalized: String = source
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if character == ' ' || character == '-' || character == '_' {
                Some('-')
            } else {
                None
            }
        })
        .take(40)
        .collect();
    let name = normalized.trim_matches('-');
    if name.is_empty() {
        "user".to_string()
    } else {
        name.to_string()
    }
}

fn external_identity_conflict(err: sqlx::Error) -> AppError {
    if is_unique_violation(&err) {
        AppError::Conflict("That login method is already connected.".into())
    } else {
        AppError::from(err)
    }
}

fn external_account_conflict(err: sqlx::Error) -> AppError {
    if is_unique_violation(&err) {
        AppError::BadRequest("An account with that email already exists.".into())
    } else {
        AppError::from(err)
    }
}

/// A username or address somebody else already holds is routine user input, not a database fault,
/// so it surfaces as a bad request naming the field that clashed rather than a raw driver message.
///
/// Which of the two clashed comes from the constraint the row violated, because the statement
/// writes both at once and the message has to point at the field the reader must change.
fn taken_identity_error(err: sqlx::Error, profile: &ProfileUpdate<'_>) -> AppError {
    if !is_unique_violation(&err) {
        return AppError::from(err);
    }
    let sqlx::Error::Database(ref db_err) = err else {
        return AppError::from(err);
    };

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

/// Whether the driver is reporting a duplicate key rather than a fault. Both places that turn one
/// into a readable refusal ask this, so the code lives in one place.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23505"))
}

/// One row of `pending_account_changes`, before its `kind` and payload are folded into the
/// [`PendingChange`] the domain speaks in.
#[derive(sqlx::FromRow, Debug)]
struct PendingAccountChangeDb {
    kind: String,
    new_email: Option<String>,
    expires_at: DateTime<Utc>,
}

impl PendingAccountChangeDb {
    /// The row as the domain reads it. `None` for a row whose `kind` is not one this build knows
    /// -- the schema's CHECK should make that unreachable, and a row from the future is better
    /// skipped than guessed at.
    fn into_pending(self) -> Option<PendingChange> {
        match AccountChangeKind::parse(&self.kind)? {
            AccountChangeKind::Email => Some(PendingChange::Email {
                new_email: EmailAddress::from(self.new_email?),
                expires_at: self.expires_at,
            }),
            AccountChangeKind::Password => Some(PendingChange::Password {
                expires_at: self.expires_at,
            }),
        }
    }
}

#[async_trait]
impl AccountChangePersistence for PostgresPersistence {
    async fn save_pending_account_change(
        &self,
        pending: PendingAccountChange<'_>,
    ) -> AppResult<()> {
        // Split here rather than in the SQL: the columns a kind may fill are the one thing the
        // table's CHECK constraint enforces, and the enum is what makes filling the wrong one
        // impossible to write in the first place.
        let (new_email, new_password_hash) = match &pending.change {
            AccountChange::Email(email) => (Some(email.as_str()), None),
            AccountChange::Password(hash) => (None, Some(*hash)),
        };

        sqlx::query!(
            r#"INSERT INTO pending_account_changes
                   (user_id, kind, new_email, new_password_hash, confirmation_code_hash, expires_at)
               VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT (user_id, kind) DO UPDATE SET
                   new_email = EXCLUDED.new_email,
                   new_password_hash = EXCLUDED.new_password_hash,
                   confirmation_code_hash = EXCLUDED.confirmation_code_hash,
                   expires_at = EXCLUDED.expires_at,
                   created_at = CURRENT_TIMESTAMP"#,
            pending.user_id,
            pending.change.kind().as_str(),
            new_email,
            new_password_hash,
            pending.confirmation_code_hash,
            pending.expires_at,
        )
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }

    async fn confirm_pending_account_change(
        &self,
        user_id: Uuid,
        kind: AccountChangeKind,
        confirmation_code_hash: &str,
    ) -> AppResult<Option<User>> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;

        // Deleting is what claims the request, and it happens in the same transaction as the
        // write: two browsers racing the same code cannot both find a row to spend.
        let claimed = sqlx::query!(
            r#"DELETE FROM pending_account_changes
               WHERE user_id = $1
                 AND kind = $2
                 AND confirmation_code_hash = $3
                 AND expires_at > CURRENT_TIMESTAMP
               RETURNING new_email, new_password_hash"#,
            user_id,
            kind.as_str(),
            confirmation_code_hash,
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        let Some(claimed) = claimed else {
            transaction.rollback().await.map_err(AppError::from)?;
            return Ok(None);
        };

        let updated = match kind {
            AccountChangeKind::Email => {
                sqlx::query_as!(
                    UserDb,
                    r#"UPDATE users SET email = $2 WHERE id = $1
                       RETURNING id, username, email, password_hash, avatar_url, created_at as "created_at!""#,
                    user_id,
                    claimed.new_email,
                )
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|err| taken_address_error(err, claimed.new_email.as_deref()))?
            }
            AccountChangeKind::Password => {
                let updated = sqlx::query_as!(
                    UserDb,
                    r#"UPDATE users SET password_hash = $2 WHERE id = $1
                       RETURNING id, username, email, password_hash, avatar_url, created_at as "created_at!""#,
                    user_id,
                    claimed.new_password_hash,
                )
                .fetch_optional(&mut *transaction)
                .await
                .map_err(AppError::from)?;
                if updated.is_some() {
                    sqlx::query(
                        "INSERT INTO user_login_methods (user_id, provider) VALUES ($1, 'password') ON CONFLICT (user_id, provider) DO NOTHING",
                    )
                    .bind(user_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(AppError::from)?;
                }
                updated
            }
        };

        transaction.commit().await.map_err(AppError::from)?;
        Ok(updated.map(Into::into))
    }

    async fn list_pending_account_changes(&self, user_id: Uuid) -> AppResult<Vec<PendingChange>> {
        let rows = sqlx::query_as!(
            PendingAccountChangeDb,
            r#"SELECT kind, new_email as "new_email: String", expires_at
               FROM pending_account_changes
               WHERE user_id = $1 AND expires_at > CURRENT_TIMESTAMP
               ORDER BY kind"#,
            user_id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(rows
            .into_iter()
            .filter_map(PendingAccountChangeDb::into_pending)
            .collect())
    }

    async fn discard_pending_account_change(
        &self,
        user_id: Uuid,
        kind: AccountChangeKind,
    ) -> AppResult<()> {
        sqlx::query!(
            "DELETE FROM pending_account_changes WHERE user_id = $1 AND kind = $2",
            user_id,
            kind.as_str(),
        )
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }
}

/// The address having been taken in the fifteen minutes since the code went out is the one thing
/// that can still refuse a confirmation, and the reader is the only one who can resolve it.
fn taken_address_error(err: sqlx::Error, address: Option<&str>) -> AppError {
    if is_unique_violation(&err) {
        return AppError::BadRequest(format!(
            "An account already uses the address '{}'.",
            address.unwrap_or_default()
        ));
    }
    AppError::from(err)
}

#[cfg(test)]
mod tests {
    //! What a pending account change does is decided by SQL — the CHECK constraint that keeps a
    //! row's payload matching its kind, the `DELETE ... RETURNING` that makes spending a code
    //! atomic, and the `expires_at` comparison — so none of it can be shown anywhere but against a
    //! real database.

    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::use_cases::user::{AccountChangePersistence, UserCredentialsHasher};

    async fn account(persistence: &PostgresPersistence) -> User {
        let username = format!("pending_{}", Uuid::new_v4().simple());
        persistence
            .create_user(&username, &format!("{username}@example.com"), "hash")
            .await
            .expect("an account")
    }

    fn in_fifteen_minutes() -> DateTime<Utc> {
        Utc::now() + chrono::Duration::minutes(15)
    }

    #[tokio::test]
    async fn a_confirmed_code_moves_the_address_and_cannot_be_spent_twice() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = std::sync::Arc::new(PostgresPersistence::new(pool));
        let user = account(&persistence).await;
        let moved_to = EmailAddress::from(format!("moved_{}@example.com", Uuid::new_v4().simple()));

        persistence
            .save_pending_account_change(PendingAccountChange {
                user_id: user.id,
                change: AccountChange::Email(&moved_to),
                confirmation_code_hash: "hash-of-123456",
                expires_at: in_fifteen_minutes(),
            })
            .await
            .expect("a stored request");

        // Until it is confirmed the account still has the address it signed up with.
        let stored = persistence
            .get_by_id(user.id)
            .await
            .expect("a lookup")
            .expect("the account");
        assert_eq!(stored.email, user.email);

        let pending = persistence
            .list_pending_account_changes(user.id)
            .await
            .expect("the pending list");
        assert!(
            matches!(pending.as_slice(), [PendingChange::Email { new_email, .. }] if new_email == &moved_to),
            "the request reads back as the address it will move to: {pending:?}"
        );

        let confirmed = persistence
            .confirm_pending_account_change(user.id, AccountChangeKind::Email, "hash-of-123456")
            .await
            .expect("a confirmation")
            .expect("the updated account");
        assert_eq!(confirmed.email, moved_to.as_str());

        // The delete is what claims the request, so the same code cannot be replayed.
        assert!(
            persistence
                .confirm_pending_account_change(user.id, AccountChangeKind::Email, "hash-of-123456")
                .await
                .expect("a second attempt")
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_wrong_or_expired_code_changes_nothing() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let user = account(&persistence).await;

        persistence
            .save_pending_account_change(PendingAccountChange {
                user_id: user.id,
                change: AccountChange::Password("new-hash"),
                confirmation_code_hash: "hash-of-123456",
                expires_at: in_fifteen_minutes(),
            })
            .await
            .expect("a stored request");

        assert!(
            persistence
                .confirm_pending_account_change(
                    user.id,
                    AccountChangeKind::Password,
                    "hash-of-000000"
                )
                .await
                .expect("a wrong code")
                .is_none()
        );

        // A code that missed its window is refused, and does not show up as something to confirm.
        persistence
            .save_pending_account_change(PendingAccountChange {
                user_id: user.id,
                change: AccountChange::Password("new-hash"),
                confirmation_code_hash: "hash-of-123456",
                expires_at: Utc::now() - chrono::Duration::minutes(1),
            })
            .await
            .expect("an expired request");

        assert!(
            persistence
                .confirm_pending_account_change(
                    user.id,
                    AccountChangeKind::Password,
                    "hash-of-123456"
                )
                .await
                .expect("an expired code")
                .is_none()
        );
        assert!(
            persistence
                .list_pending_account_changes(user.id)
                .await
                .expect("the pending list")
                .is_empty()
        );
        assert_eq!(
            persistence
                .get_by_id(user.id)
                .await
                .expect("a lookup")
                .expect("the account")
                .password_hash,
            "hash",
            "neither refusal may write the new password"
        );
    }

    #[tokio::test]
    async fn each_kind_gets_one_request_and_a_second_ask_replaces_it() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let user = account(&persistence).await;
        let first = EmailAddress::from(format!("first_{}@example.com", Uuid::new_v4().simple()));
        let second = EmailAddress::from(format!("second_{}@example.com", Uuid::new_v4().simple()));

        for (address, code) in [(&first, "hash-of-111111"), (&second, "hash-of-222222")] {
            persistence
                .save_pending_account_change(PendingAccountChange {
                    user_id: user.id,
                    change: AccountChange::Email(address),
                    confirmation_code_hash: code,
                    expires_at: in_fifteen_minutes(),
                })
                .await
                .expect("a stored request");
        }
        persistence
            .save_pending_account_change(PendingAccountChange {
                user_id: user.id,
                change: AccountChange::Password("new-hash"),
                confirmation_code_hash: "hash-of-333333",
                expires_at: in_fifteen_minutes(),
            })
            .await
            .expect("a stored request");

        // One per kind: asking twice for an address change leaves the second ask, not both.
        let pending = persistence
            .list_pending_account_changes(user.id)
            .await
            .expect("the pending list");
        assert_eq!(pending.len(), 2);
        assert!(matches!(
            pending.iter().find(|p| p.kind() == AccountChangeKind::Email),
            Some(PendingChange::Email { new_email, .. }) if new_email == &second
        ));

        // ...and the code the first ask mailed out is dead.
        assert!(
            persistence
                .confirm_pending_account_change(user.id, AccountChangeKind::Email, "hash-of-111111")
                .await
                .expect("the superseded code")
                .is_none()
        );

        // Discarding one kind leaves the other alone.
        persistence
            .discard_pending_account_change(user.id, AccountChangeKind::Email)
            .await
            .expect("a discard");
        let pending = persistence
            .list_pending_account_changes(user.id)
            .await
            .expect("the pending list");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind(), AccountChangeKind::Password);
    }

    #[tokio::test]
    async fn an_address_taken_since_the_code_was_mailed_refuses_the_confirmation() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let mover = account(&persistence).await;
        let squatter = account(&persistence).await;

        persistence
            .save_pending_account_change(PendingAccountChange {
                user_id: mover.id,
                change: AccountChange::Email(&EmailAddress::from(squatter.email.as_str())),
                confirmation_code_hash: "hash-of-123456",
                expires_at: in_fifteen_minutes(),
            })
            .await
            .expect("a stored request");

        let refused = persistence
            .confirm_pending_account_change(mover.id, AccountChangeKind::Email, "hash-of-123456")
            .await;
        assert!(
            matches!(refused, Err(AppError::BadRequest(ref message)) if message.contains(&squatter.email)),
            "a taken address must be named, not reported as a driver fault: {refused:?}"
        );

        // The transaction rolled back, so the request is still there to retry with another address.
        assert_eq!(
            persistence
                .get_by_id(mover.id)
                .await
                .expect("a lookup")
                .expect("the account")
                .email,
            mover.email
        );
    }

    #[tokio::test]
    async fn accounts_only_receive_the_login_method_they_registered() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = std::sync::Arc::new(PostgresPersistence::new(pool));
        let suffix = Uuid::new_v4().simple().to_string();
        let password_user = persistence
            .create_user(
                &format!("password-{suffix}"),
                &format!("password-{suffix}@example.com"),
                "hash",
            )
            .await
            .expect("password registration");
        assert!(
            persistence
                .has_login_method(password_user.id, LoginMethod::Password)
                .await
                .expect("method lookup")
        );
        assert!(
            !persistence
                .has_login_method(password_user.id, LoginMethod::Google)
                .await
                .expect("method lookup")
        );

        let google_email = EmailAddress::from(format!("google-{suffix}@example.com"));
        let google_subject = format!("google-subject-{suffix}");
        let google_user = persistence
            .create_external_user(ExternalIdentity {
                provider: LoginMethod::Google,
                subject: &google_subject,
                email: &google_email,
                display_name: Some("Google Person"),
            })
            .await
            .expect("Google registration");
        assert!(
            persistence
                .has_login_method(google_user.id, LoginMethod::Google)
                .await
                .expect("method lookup")
        );
        assert!(
            !persistence
                .has_login_method(google_user.id, LoginMethod::Password)
                .await
                .expect("method lookup")
        );
        assert_eq!(
            persistence
                .get_by_external_subject(LoginMethod::Google, &google_subject)
                .await
                .expect("Google lookup")
                .expect("registered identity")
                .id,
            google_user.id
        );
        assert!(
            persistence
                .get_by_external_subject(LoginMethod::Google, "some-other-google-subject")
                .await
                .expect("Google lookup")
                .is_none()
        );

        let use_cases = crate::use_cases::user::UserUseCases::new(
            std::sync::Arc::new(crate::infra::argon2_password_hasher()),
            persistence.clone(),
        );
        assert!(matches!(
            use_cases
                .login(
                    &google_user.email,
                    &secrecy::SecretString::from("irrelevant-password")
                )
                .await,
            Err(AppError::InvalidCredentials)
        ));
        assert!(matches!(
            use_cases
                .login_external(LoginMethod::Google, "some-other-google-subject")
                .await,
            Err(AppError::InvalidCredentials)
        ));
        assert_eq!(
            use_cases
                .login_external(LoginMethod::Google, &google_subject)
                .await
                .expect("registered Google login")
                .id,
            google_user.id
        );

        let password = "new-password-login";
        let password_hash = crate::infra::argon2_password_hasher()
            .hash_password(password)
            .expect("password hash");
        persistence
            .activate_password(google_user.id, &password_hash)
            .await
            .expect("activate password")
            .expect("Google user still exists");
        assert!(
            persistence
                .has_login_method(google_user.id, LoginMethod::Password)
                .await
                .expect("password method lookup")
        );
        assert_eq!(
            use_cases
                .login(&google_user.email, &secrecy::SecretString::from(password))
                .await
                .expect("added password login")
                .id,
            google_user.id
        );

        let linked_subject = format!("linked-google-{suffix}");
        let password_email = EmailAddress::from(password_user.email.as_str());
        use_cases
            .link_external(
                password_user.id,
                ExternalIdentity {
                    provider: LoginMethod::Google,
                    subject: &linked_subject,
                    email: &password_email,
                    display_name: None,
                },
            )
            .await
            .expect("link Google to password account");
        assert_eq!(
            use_cases
                .login_external(LoginMethod::Google, &linked_subject)
                .await
                .expect("linked Google login")
                .id,
            password_user.id
        );

        let apple_email = EmailAddress::from(format!("apple-{suffix}@example.com"));
        let apple_subject = format!("apple-subject-{suffix}");
        let apple_user = persistence
            .create_external_user(ExternalIdentity {
                provider: LoginMethod::Apple,
                subject: &apple_subject,
                email: &apple_email,
                display_name: Some("Apple Person"),
            })
            .await
            .expect("Apple registration");
        assert!(
            persistence
                .has_login_method(apple_user.id, LoginMethod::Apple)
                .await
                .expect("Apple method lookup")
        );
        assert_eq!(
            use_cases
                .login_external(LoginMethod::Apple, &apple_subject)
                .await
                .expect("Apple login")
                .id,
            apple_user.id
        );
    }
}
