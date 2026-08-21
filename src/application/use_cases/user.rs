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

/// The shortest password a *change* will store. Deliberately stricter than what registration
/// grandfathered in: raising the floor for an account that is already signed in costs its owner
/// one longer password, and refusing to lower it costs nobody anything.
pub const MIN_PASSWORD_CHARS: usize = 8;

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

/// The account fields their owner may edit, as one value rather than three parameters: two of
/// them are strings, and a swapped pair would write an address into the name column silently.
#[derive(Debug)]
pub struct ProfileUpdate<'a> {
    pub username: &'a str,
    pub email: &'a EmailAddress,
    /// The profile picture, or `None` for the letter bubble. Already parsed -- see
    /// [`AvatarUrl::parse`], which runs where the form is read.
    pub avatar_url: Option<&'a AvatarUrl>,
}

/// A password change as its form submits it: what the account currently has, and what it should
/// have twice over.
///
/// The current password is what authorizes the change -- a session alone must not be enough, or a
/// borrowed browser is a taken account.
pub struct PasswordChange<'a> {
    pub current: &'a SecretString,
    pub new: &'a SecretString,
    pub confirmation: &'a SecretString,
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

    /// Stores the identity fields the account owner may edit, all in one write so a rejected
    /// address cannot leave a renamed account behind. `Ok(None)` means no such user.
    async fn update_profile(&self, id: Uuid, profile: ProfileUpdate<'_>)
    -> AppResult<Option<User>>;

    /// Stores an already-hashed password. Takes the hash rather than the password so the plaintext
    /// never reaches the adapter layer. `Ok(None)` means no such user.
    async fn update_password_hash(&self, id: Uuid, password_hash: &str) -> AppResult<Option<User>>;
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

    /// Saves the account's own details: what it is called, where its mail goes, and its picture.
    ///
    /// Name and address are trimmed and checked here rather than at the form, so the API and the
    /// page cannot disagree about what an acceptable account looks like. Whether the address is
    /// already somebody else's is the database's call -- see the unique constraints on `users` --
    /// because a check made here would be a check made before the row it guards.
    #[instrument(skip(self))]
    pub async fn update_profile(&self, id: Uuid, profile: ProfileUpdate<'_>) -> AppResult<User> {
        let username = profile.username.trim();
        let email = EmailAddress::from(profile.email.trim());

        if username.is_empty() {
            return Err(AppError::BadRequest("A username is required.".into()));
        }
        if !is_plausible_address(&email) {
            return Err(AppError::BadRequest(
                "Enter an email address of the form name@example.com.".into(),
            ));
        }

        info!("Updating profile for user {id}...");

        self.persistence
            .update_profile(
                id,
                ProfileUpdate {
                    username,
                    email: &email,
                    avatar_url: profile.avatar_url,
                },
            )
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".into()))
    }

    /// Replaces the account's password, once the account has proved it knows the current one.
    ///
    /// Every rejection here propagates: an error reaching the hasher or the store must not be
    /// mistaken for "the password did not match", nor for a change that went through.
    #[instrument(skip(self, change))]
    pub async fn change_password(&self, id: Uuid, change: PasswordChange<'_>) -> AppResult<()> {
        let user = self
            .persistence
            .get_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?;

        if !self
            .hasher
            .verify_password(change.current.expose_secret(), &user.password_hash)?
        {
            return Err(AppError::InvalidCredentials);
        }

        let new_password = change.new.expose_secret();
        if new_password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(AppError::BadRequest(format!(
                "A new password needs at least {MIN_PASSWORD_CHARS} characters."
            )));
        }
        if new_password != change.confirmation.expose_secret() {
            return Err(AppError::BadRequest(
                "The new password and its confirmation do not match.".into(),
            ));
        }

        info!("Changing password for user {id}...");

        let hash = self.hasher.hash_password(new_password)?;
        self.persistence
            .update_password_hash(id, &hash)
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".into()))?;

        Ok(())
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

/// Whether an address is shaped like one at all: exactly one `@`, with something either side and
/// a dot in the domain. Deliverability is the mail server's answer, not ours -- this only catches
/// the typo that would otherwise be stored as somebody's login.
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

    /// The stored account, so a write is observable by the read that follows it.
    struct MockUserPersistence {
        stored: std::sync::Mutex<User>,
    }

    impl MockUserPersistence {
        fn new() -> Self {
            Self {
                stored: std::sync::Mutex::new(User {
                    id: Uuid::new_v4(),
                    username: "testuser".to_string(),
                    email: "testuser@gmail.com".to_string(),
                    password_hash: "secret_hash".to_string(),
                    avatar_url: None,
                    created_at: chrono::Utc::now(),
                }),
            }
        }

        fn id(&self) -> Uuid {
            self.stored.lock().unwrap().id
        }
    }

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
            Ok(Some(self.stored.lock().unwrap().clone()))
        }

        async fn update_avatar_url(
            &self,
            _id: Uuid,
            avatar_url: Option<&AvatarUrl>,
        ) -> AppResult<Option<User>> {
            let mut stored = self.stored.lock().unwrap();
            stored.avatar_url = avatar_url.cloned();
            Ok(Some(stored.clone()))
        }

        async fn update_profile(
            &self,
            _id: Uuid,
            profile: ProfileUpdate<'_>,
        ) -> AppResult<Option<User>> {
            let mut stored = self.stored.lock().unwrap();
            stored.username = profile.username.to_string();
            stored.email = profile.email.to_string();
            stored.avatar_url = profile.avatar_url.cloned();
            Ok(Some(stored.clone()))
        }

        async fn update_password_hash(
            &self,
            _id: Uuid,
            password_hash: &str,
        ) -> AppResult<Option<User>> {
            let mut stored = self.stored.lock().unwrap();
            stored.password_hash = password_hash.to_string();
            Ok(Some(stored.clone()))
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
            Arc::new(MockUserPersistence::new()),
        );

        let user = user_use_cases
            .add("testuser", "testuser@gmail.com", &"testuser_pw".into())
            .await
            .unwrap();

        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, "testuser@gmail.com");
    }

    fn use_cases(persistence: Arc<MockUserPersistence>) -> UserUseCases {
        UserUseCases::new(Arc::new(MockUserCredentialsHasher), persistence)
    }

    #[tokio::test]
    async fn updating_a_profile_trims_and_stores_every_field() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();

        let saved = use_cases(persistence)
            .update_profile(
                id,
                ProfileUpdate {
                    username: "  renamed  ",
                    email: &EmailAddress::from("  new@example.com  "),
                    avatar_url: Some(&AvatarUrl::from("https://cdn.example.com/me.png")),
                },
            )
            .await
            .unwrap();

        assert_eq!(saved.username, "renamed");
        assert_eq!(saved.email, "new@example.com");
        assert_eq!(
            saved.avatar_url.as_deref(),
            Some("https://cdn.example.com/me.png")
        );
    }

    #[tokio::test]
    async fn a_profile_needs_a_name_and_an_address_shaped_like_one() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        let use_cases = use_cases(persistence);

        for (username, email) in [("   ", "fine@example.com"), ("named", "not-an-address")] {
            let rejected = use_cases
                .update_profile(
                    id,
                    ProfileUpdate {
                        username,
                        email: &EmailAddress::from(email),
                        avatar_url: None,
                    },
                )
                .await;

            assert!(
                matches!(rejected, Err(AppError::BadRequest(_))),
                "{username}/{email} should have been refused, got {rejected:?}"
            );
        }
    }

    #[tokio::test]
    async fn changing_a_password_rehashes_it_only_for_whoever_knows_the_current_one() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        let use_cases = use_cases(persistence.clone());

        let wrong_current = use_cases
            .change_password(
                id,
                PasswordChange {
                    current: &"not-the-password".into(),
                    new: &"a-longer-secret".into(),
                    confirmation: &"a-longer-secret".into(),
                },
            )
            .await;
        assert!(matches!(wrong_current, Err(AppError::InvalidCredentials)));

        use_cases
            .change_password(
                id,
                PasswordChange {
                    current: &"secret".into(),
                    new: &"a-longer-secret".into(),
                    confirmation: &"a-longer-secret".into(),
                },
            )
            .await
            .unwrap();

        assert_eq!(
            persistence.stored.lock().unwrap().password_hash,
            "a-longer-secret_hash"
        );
    }

    #[tokio::test]
    async fn a_new_password_must_be_long_enough_and_confirmed() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        let use_cases = use_cases(persistence.clone());

        for (new, confirmation) in [("short", "short"), ("a-longer-secret", "a-longer-secrat")] {
            let rejected = use_cases
                .change_password(
                    id,
                    PasswordChange {
                        current: &"secret".into(),
                        new: &new.into(),
                        confirmation: &confirmation.into(),
                    },
                )
                .await;

            assert!(
                matches!(rejected, Err(AppError::BadRequest(_))),
                "{new}/{confirmation} should have been refused, got {rejected:?}"
            );
        }

        assert_eq!(
            persistence.stored.lock().unwrap().password_hash,
            "secret_hash",
            "a refused change must leave the stored password alone"
        );
    }

    #[tokio::test]
    async fn login_user_works() {
        let user_use_cases = UserUseCases::new(
            Arc::new(MockUserCredentialsHasher),
            Arc::new(MockUserPersistence::new()),
        );

        let user = user_use_cases
            .login("testuser@gmail.com", &"secret".into())
            .await
            .unwrap();

        assert_eq!(user.username, "testuser");
    }
}
