use std::sync::Arc;

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use tracing::{info, instrument};

use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        user::User,
        value_objects::{AvatarUrl, EmailAddress},
    },
    infra::config::AppConfig,
    services::outbound_dispatcher::OutboundDispatcher,
};

const CONFIRMATION_TTL_MINUTES: i64 = 15;

pub struct PendingRegistration<'a> {
    pub username: &'a str,
    pub email: &'a EmailAddress,
    pub password_hash: &'a str,
    pub confirmation_code_hash: &'a str,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
pub trait RegistrationPersistence: Send + Sync {
    async fn save_pending_registration(&self, pending: PendingRegistration<'_>) -> AppResult<()>;
    async fn confirm_pending_registration(
        &self,
        email: &EmailAddress,
        confirmation_code_hash: &str,
    ) -> AppResult<Option<User>>;
}

pub enum RegistrationOutcome {
    Created(User),
    ConfirmationSent,
}

#[async_trait]
pub trait UserPersistence: Send + Sync {
    /// Stores a new account and returns it, so a caller that has just registered somebody can
    /// act as them (sign them in) without a second lookup by a name it would have to trust.
    async fn create_user(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
    ) -> AppResult<User>;
    async fn get_by_email(&self, email: &str) -> AppResult<Option<User>>;
    async fn get_by_username(&self, username: &str) -> AppResult<Option<User>>;
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<User>>;

    /// Stores the account's profile picture, or clears it with `None`. `Ok(None)` means no such
    /// user, so a stale session cannot silently write nothing.
    async fn update_avatar_url(
        &self,
        id: Uuid,
        avatar_url: Option<&AvatarUrl>,
    ) -> AppResult<Option<User>>;
}

pub trait UserCredentialsHasher: Send + Sync {
    fn hash_password(&self, password: &str) -> AppResult<String>;
    fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool>;
}

#[derive(Clone)]
pub struct UserUseCases {
    hasher: Arc<dyn UserCredentialsHasher>,
    persistence: Arc<dyn UserPersistence>,
    confirmation: Option<(Arc<dyn RegistrationPersistence>, Arc<AppConfig>)>,
}

impl UserUseCases {
    pub fn new(
        hasher: Arc<dyn UserCredentialsHasher>,
        persistence: Arc<dyn UserPersistence>,
    ) -> Self {
        Self {
            hasher,
            persistence,
            confirmation: None,
        }
    }

    pub fn with_email_confirmation(
        mut self,
        persistence: Arc<dyn RegistrationPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        self.confirmation = Some((persistence, config));
        self
    }

    pub async fn register(
        &self,
        username: &str,
        email: &str,
        password: &SecretString,
    ) -> AppResult<RegistrationOutcome> {
        let Some((confirmation, config)) = &self.confirmation else {
            return self
                .add(username, email, password)
                .await
                .map(RegistrationOutcome::Created);
        };
        if !config.email_confirmation_enabled() {
            return self
                .add(username, email, password)
                .await
                .map(RegistrationOutcome::Created);
        }

        let email = EmailAddress::from(email.trim());
        if self.persistence.get_by_email(&email).await?.is_some()
            || self
                .persistence
                .get_by_username(username.trim())
                .await?
                .is_some()
        {
            return Err(AppError::BadRequest(
                "An account with that username or email already exists".into(),
            ));
        }
        let password_hash = self.hasher.hash_password(password.expose_secret())?;
        let code = format!("{:06}", confirmation_number());
        let code_hash = confirmation_code_hash(config, &email, &code);
        confirmation
            .save_pending_registration(PendingRegistration {
                username: username.trim(),
                email: &email,
                password_hash: &password_hash,
                confirmation_code_hash: &code_hash,
                expires_at: chrono::Utc::now()
                    + chrono::Duration::minutes(CONFIRMATION_TTL_MINUTES),
            })
            .await?;
        OutboundDispatcher::send_registration_confirmation(config, &email, &code).await?;
        Ok(RegistrationOutcome::ConfirmationSent)
    }

    pub async fn confirm_registration(&self, email: &str, code: &str) -> AppResult<User> {
        if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(AppError::BadRequest(
                "Invalid or expired confirmation code".into(),
            ));
        }
        let Some((confirmation, config)) = &self.confirmation else {
            return Err(AppError::BadRequest(
                "Email confirmation is not enabled".into(),
            ));
        };
        if !config.email_confirmation_enabled() {
            return Err(AppError::BadRequest(
                "Email confirmation is not enabled".into(),
            ));
        }
        let email = EmailAddress::from(email.trim());
        confirmation
            .confirm_pending_registration(&email, &confirmation_code_hash(config, &email, code))
            .await?
            .ok_or_else(|| AppError::BadRequest("Invalid or expired confirmation code".into()))
    }

    #[instrument(skip(self, password))]
    pub async fn add(
        &self,
        username: &str,
        email: &str,
        password: &SecretString,
    ) -> AppResult<User> {
        info!("Adding user...");

        let hash = self.hasher.hash_password(password.expose_secret())?;
        let user = self.persistence.create_user(username, email, &hash).await?;

        info!("Adding user finished.");

        Ok(user)
    }

    #[instrument(skip(self, password))]
    pub async fn login(&self, email_or_username: &str, password: &SecretString) -> AppResult<User> {
        info!("Attempting user login...");

        let user = if let Some(user) = self.persistence.get_by_email(email_or_username).await? {
            user
        } else if let Some(user) = self.persistence.get_by_username(email_or_username).await? {
            user
        } else {
            return Err(AppError::InvalidCredentials);
        };

        let is_valid = self
            .hasher
            .verify_password(password.expose_secret(), &user.password_hash)?;

        if !is_valid {
            return Err(AppError::InvalidCredentials);
        }

        info!("User login successful for {}", user.username);
        Ok(user)
    }

    #[instrument(skip(self))]
    pub async fn get_user_by_id(&self, id: Uuid) -> AppResult<Option<User>> {
        self.persistence.get_by_id(id).await
    }

    /// Points the account at a new profile picture, or back at its letter bubble with `None`.
    ///
    /// The URL arrives already parsed: [`AvatarUrl::parse`] is what rejects a scheme no `<img>`
    /// should be pointed at, and it runs where the form is read.
    #[instrument(skip(self))]
    pub async fn set_avatar(&self, id: Uuid, avatar_url: Option<&AvatarUrl>) -> AppResult<User> {
        info!("Updating avatar for user {id}...");

        self.persistence
            .update_avatar_url(id, avatar_url)
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".into()))
    }
}

fn confirmation_number() -> u32 {
    let bytes = *Uuid::new_v4().as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 1_000_000
}

fn confirmation_code_hash(config: &AppConfig, email: &EmailAddress, code: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(&config.jwt_secret);
    digest.update(":");
    digest.update(email.trim().to_ascii_lowercase());
    digest.update(":");
    digest.update(code);
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod test {
    use async_trait::async_trait;
    use uuid::Uuid;

    use super::*;

    struct MockUserPersistence;

    #[async_trait]
    impl UserPersistence for MockUserPersistence {
        async fn create_user(
            &self,
            username: &str,
            email: &str,
            password_hash: &str,
        ) -> AppResult<User> {
            assert_eq!(username, "testuser");
            assert_eq!(email, "testuser@gmail.com");

            Ok(User {
                id: Uuid::new_v4(),
                username: username.to_string(),
                email: email.to_string(),
                password_hash: password_hash.to_string(),
                avatar_url: None,
                created_at: chrono::Utc::now(),
            })
        }

        async fn get_by_email(&self, email: &str) -> AppResult<Option<User>> {
            if email == "testuser@gmail.com" {
                Ok(Some(User {
                    id: Uuid::new_v4(),
                    username: "testuser".to_string(),
                    email: "testuser@gmail.com".to_string(),
                    password_hash: "secret_hash".to_string(),
                    avatar_url: None,
                    created_at: chrono::Utc::now(),
                }))
            } else {
                Ok(None)
            }
        }

        async fn get_by_username(&self, username: &str) -> AppResult<Option<User>> {
            if username == "testuser" {
                Ok(Some(User {
                    id: Uuid::new_v4(),
                    username: "testuser".to_string(),
                    email: "testuser@gmail.com".to_string(),
                    password_hash: "secret_hash".to_string(),
                    avatar_url: None,
                    created_at: chrono::Utc::now(),
                }))
            } else {
                Ok(None)
            }
        }

        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<User>> {
            Ok(Some(User {
                id: Uuid::new_v4(),
                username: "testuser".to_string(),
                email: "testuser@gmail.com".to_string(),
                password_hash: "secret_hash".to_string(),
                avatar_url: None,
                created_at: chrono::Utc::now(),
            }))
        }

        async fn update_avatar_url(
            &self,
            id: Uuid,
            avatar_url: Option<&AvatarUrl>,
        ) -> AppResult<Option<User>> {
            Ok(Some(User {
                id,
                username: "testuser".to_string(),
                email: "testuser@gmail.com".to_string(),
                password_hash: "secret_hash".to_string(),
                avatar_url: avatar_url.cloned(),
                created_at: chrono::Utc::now(),
            }))
        }
    }

    struct MockUserCredentialsHasher;

    impl UserCredentialsHasher for MockUserCredentialsHasher {
        fn hash_password(&self, password: &str) -> AppResult<String> {
            Ok(format!("{}_hash", password))
        }

        fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool> {
            Ok(hash == &format!("{}_hash", password)
                || hash == "secret_hash" && password == "secret")
        }
    }

    #[tokio::test]
    async fn add_user_works() {
        let user_use_cases = UserUseCases::new(
            Arc::new(MockUserCredentialsHasher),
            Arc::new(MockUserPersistence),
        );

        let user = user_use_cases
            .add("testuser", "testuser@gmail.com", &"testuser_pw".into())
            .await
            .unwrap();

        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, "testuser@gmail.com");
    }

    #[tokio::test]
    async fn login_user_works() {
        let user_use_cases = UserUseCases::new(
            Arc::new(MockUserCredentialsHasher),
            Arc::new(MockUserPersistence),
        );

        let user = user_use_cases
            .login("testuser@gmail.com", &"secret".into())
            .await
            .unwrap();

        assert_eq!(user.username, "testuser");
    }
}
