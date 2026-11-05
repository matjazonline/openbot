//! Account orchestration; ports, identity, confirmation and profile operations have their own modules.
use crate::{
    app_error::{AppError, AppResult},
    entities::{
        notification::NotificationPreferences,
        user::User,
        value_objects::{AvatarUrl, EmailAddress},
    },
};
use secrecy::{ExposeSecret, SecretString};
use std::{net::IpAddr, sync::Arc};
use tokio::sync::Semaphore;
use tracing::{info, instrument};
use uuid::Uuid;
mod auth;
mod codes;
mod confirmation;
mod external_identity;
mod ports;
mod profile;
pub(crate) use auth::auth_attempt_key;
pub use auth::{AuthAttemptKey, AuthAttemptSecret, AuthWorkLifetime};
use codes::{account_change_code_hash, confirmation_code_hash, confirmation_number};
pub use confirmation::EmailConfirmation;
pub use ports::*;
#[cfg(test)]
mod tests;
const CONFIRMATION_TTL_MINUTES: i64 = 15;
const MAX_PASSWORD_HASH_JOBS: usize = 4;
/// A valid PHC string with the production Argon2 parameters. Verifying against it consumes the
/// same bounded work for absent and OAuth-only accounts, then always fails.
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// What saving the account's details did.
///
/// The account fields saved and the address-specific result of the same submission.
#[derive(Debug)]
pub struct Saved {
    pub user: User,
    pub address: AddressSaveOutcome,
}

#[derive(Debug)]
pub enum AddressSaveOutcome {
    Unchanged,
    ConfirmationPending(PendingChange),
    InstalledUnverified,
    Unavailable,
    CurrentAddressChanged,
}

/// What changing the password did: written, or mailed a code that has to come back first.
#[derive(Debug)]
pub enum PasswordOutcome {
    Changed,
    ConfirmationSent(PendingChange),
}

#[derive(Clone)]
pub struct UserUseCases {
    hasher: Arc<dyn UserCredentialsHasher>,
    hash_slots: Arc<Semaphore>,
    persistence: Arc<dyn UserPersistence>,
    attempt_secret: AuthAttemptSecret,
    confirmation: Option<EmailConfirmation>,
}

impl UserUseCases {
    pub fn new(
        hasher: Arc<dyn UserCredentialsHasher>,
        persistence: Arc<dyn UserPersistence>,
        attempt_secret: AuthAttemptSecret,
    ) -> Self {
        Self {
            hasher,
            hash_slots: Arc::new(Semaphore::new(MAX_PASSWORD_HASH_JOBS)),
            persistence,
            attempt_secret,
            confirmation: None,
        }
    }

    async fn password_job<T: Send + 'static>(
        &self,
        operation: impl FnOnce(Arc<dyn UserCredentialsHasher>) -> AppResult<T> + Send + 'static,
        work_lifetime: Option<AuthWorkLifetime>,
    ) -> AppResult<T> {
        let permit = self.hash_slots.clone().try_acquire_owned().map_err(|_| {
            AppError::ServiceUnavailable("Password hashing capacity exhausted".into())
        })?;
        let hasher = self.hasher.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _source_lifetime = work_lifetime;
            operation(hasher)
        })
        .await
        .map_err(|_| AppError::Internal("Password worker stopped".into()))?
    }

    async fn hash_password(
        &self,
        password: &str,
        work_lifetime: Option<AuthWorkLifetime>,
    ) -> AppResult<String> {
        let password = password.to_owned();
        self.password_job(move |hasher| hasher.hash_password(&password), work_lifetime)
            .await
    }

    async fn verify_password(
        &self,
        password: &str,
        hash: &str,
        work_lifetime: Option<AuthWorkLifetime>,
    ) -> AppResult<bool> {
        let password = password.to_owned();
        let hash = hash.to_owned();
        self.password_job(
            move |hasher| hasher.verify_password(&password, &hash),
            work_lifetime,
        )
        .await
    }

    pub fn with_email_confirmation(mut self, confirmation: EmailConfirmation) -> Self {
        self.confirmation = Some(confirmation);
        self
    }

    /// The confirmation machinery, only when startup constructed an outbound SMTP client.
    fn confirming(&self) -> Option<&EmailConfirmation> {
        self.confirmation
            .as_ref()
            .filter(|confirmation| confirmation.transport_available)
    }

    fn unverified_email_bypass(&self) -> bool {
        self.confirmation.as_ref().is_some_and(|confirmation| {
            !confirmation.transport_available && confirmation.allow_unverified_without_smtp
        })
    }
    #[instrument(skip_all)]
    pub async fn add(
        &self,
        username: &str,
        email: &str,
        password: &SecretString,
        work_lifetime: Option<AuthWorkLifetime>,
    ) -> AppResult<User> {
        info!("Adding user...");

        let hash = self
            .hash_password(password.expose_secret(), work_lifetime)
            .await?;
        let user = self.persistence.create_user(username, email, &hash).await?;

        info!("Adding user finished.");

        Ok(user)
    }

    #[instrument(skip_all)]
    pub async fn login(
        &self,
        email_or_username: &str,
        password: &SecretString,
        source: Option<IpAddr>,
        work_lifetime: Option<AuthWorkLifetime>,
    ) -> AppResult<User> {
        info!("Attempting user login...");

        let normalized = self
            .persistence
            .canonical_auth_identifier(email_or_username.trim())
            .await?;
        let source = source.unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
        let source = source.to_string();
        let key = auth_attempt_key(
            self.attempt_secret.expose_secret(),
            "password-login:v1",
            &[normalized.as_bytes(), source.as_bytes()],
        );
        if !self.persistence.reserve_login_attempt(&key).await? {
            return Err(AppError::InvalidCredentials);
        }

        let user = if let Some(user) = self.persistence.get_by_email(&normalized).await? {
            user
        } else if let Some(user) = self.persistence.get_by_username(&normalized).await? {
            user
        } else {
            self.verify_password(password.expose_secret(), DUMMY_PASSWORD_HASH, work_lifetime)
                .await?;
            return Err(AppError::InvalidCredentials);
        };

        if !self
            .persistence
            .has_login_method(user.id, LoginMethod::Password)
            .await?
        {
            self.verify_password(password.expose_secret(), DUMMY_PASSWORD_HASH, work_lifetime)
                .await?;
            return Err(AppError::InvalidCredentials);
        }

        let Some(hash) = user.password_hash.as_deref() else {
            self.verify_password(password.expose_secret(), DUMMY_PASSWORD_HASH, work_lifetime)
                .await?;
            return Err(AppError::InvalidCredentials);
        };
        let is_valid = self
            .verify_password(password.expose_secret(), hash, work_lifetime)
            .await?;

        if !is_valid {
            return Err(AppError::InvalidCredentials);
        }

        self.persistence.clear_login_attempt(&key).await?;
        info!(user_id = %user.id, "User login successful");
        Ok(user)
    }
    #[instrument(skip_all)]
    pub async fn get_user_by_id(&self, id: Uuid) -> AppResult<Option<User>> {
        self.persistence.get_by_id(id).await
    }

    pub async fn notification_preferences(&self, id: Uuid) -> AppResult<NotificationPreferences> {
        self.persistence.notification_preferences(id).await
    }

    pub async fn set_notification_preferences(
        &self,
        id: Uuid,
        preferences: NotificationPreferences,
    ) -> AppResult<()> {
        self.persistence
            .set_notification_preferences(id, preferences)
            .await
    }
}
fn is_plausible_address(email: &EmailAddress) -> bool {
    let mut parts = email.splitn(2, '@');
    let (Some(local), Some(domain)) = (parts.next(), parts.next()) else {
        return false;
    };

    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.contains(char::is_whitespace)
}
