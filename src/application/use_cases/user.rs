use std::sync::Arc;
use std::{any::Any, net::IpAddr};

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tracing::{info, instrument};

use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        notification::NotificationPreferences,
        user::User,
        value_objects::{AvatarUrl, EmailAddress},
    },
    infra::config::AppConfig,
};

const CONFIRMATION_TTL_MINUTES: i64 = 15;
const MAX_PASSWORD_HASH_JOBS: usize = 4;
/// A valid PHC string with the production Argon2 parameters. Verifying against it consumes the
/// same bounded work for absent and OAuth-only accounts, then always fails.
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// A domain-separated keyed digest. Neither identity nor source is persisted in plaintext.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AuthAttemptKey([u8; 32]);

/// Opaque ownership of an admitted authentication request. The HTTP adapter supplies a source
/// guard; a blocking password job retains a clone until the worker actually finishes.
#[derive(Clone)]
pub struct AuthWorkLifetime {
    _owner: Arc<dyn Any + Send + Sync>,
}

impl AuthWorkLifetime {
    pub(crate) fn new<T: Any + Send + Sync>(owner: T) -> Self {
        Self {
            _owner: Arc::new(owner),
        }
    }
}

impl AuthAttemptKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub(crate) fn auth_attempt_key(secret: &str, domain: &str, components: &[&[u8]]) -> AuthAttemptKey {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(domain.as_bytes());
    for component in components {
        mac.update(&(component.len() as u64).to_be_bytes());
        mac.update(component);
    }
    AuthAttemptKey(mac.finalize().into_bytes().into())
}

/// The shortest password a *change* will store. Deliberately stricter than what registration
/// grandfathered in: raising the floor for an account that is already signed in costs its owner
/// one longer password, and refusing to lower it costs nobody anything.
pub const MIN_PASSWORD_CHARS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    Password,
    Google,
    Apple,
}

impl LoginMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Google => "google",
            Self::Apple => "apple",
        }
    }
}

#[derive(Clone, Copy)]
pub struct ExternalIdentity<'a> {
    pub provider: LoginMethod,
    pub subject: &'a str,
    pub email: &'a EmailAddress,
    /// Set only from a validated provider claim for this exact address.
    pub email_verified: bool,
    pub display_name: Option<&'a str>,
}

pub struct LoginMethods {
    pub password: bool,
    pub google: bool,
    pub apple: bool,
}

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
        attempt_key: &AuthAttemptKey,
    ) -> AppResult<Option<User>>;
}

pub enum RegistrationOutcome {
    CreatedUnverified(User),
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

pub struct PasswordSetup<'a> {
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

    /// Use the database's citext case folding so equivalent lookup values share one budget.
    async fn canonical_auth_identifier(&self, value: &str) -> AppResult<String>;

    /// Atomically reserves a candidate in the shared two-minute identifier/source budget.
    async fn reserve_login_attempt(&self, key: &AuthAttemptKey) -> AppResult<bool>;
    async fn clear_login_attempt(&self, key: &AuthAttemptKey) -> AppResult<()>;

    async fn notification_preferences(&self, id: Uuid) -> AppResult<NotificationPreferences>;

    async fn set_notification_preferences(
        &self,
        id: Uuid,
        preferences: NotificationPreferences,
    ) -> AppResult<()>;

    /// Stores the account's profile picture, or clears it with `None`. `Ok(None)` means no such
    /// user, so a stale session cannot silently write nothing.
    async fn update_avatar_url(
        &self,
        id: Uuid,
        avatar_url: Option<&AvatarUrl>,
    ) -> AppResult<Option<User>>;

    /// Saves only display fields. The database write must never touch email or its proof, even if
    /// another transaction confirms a new address after the caller reads the profile.
    async fn update_profile_display(
        &self,
        id: Uuid,
        username: &str,
        avatar_url: Option<&AvatarUrl>,
    ) -> AppResult<Option<User>>;

    /// Explicitly installs an address with the display fields. The adapter must clear proof in
    /// the same write whenever the stored address differs. `Ok(None)` means no such user.
    async fn update_profile(&self, id: Uuid, profile: ProfileUpdate<'_>)
    -> AppResult<Option<User>>;

    /// Stores an already-hashed password. Takes the hash rather than the password so the plaintext
    /// never reaches the adapter layer. `Ok(None)` means no such user.
    async fn update_password_hash(&self, id: Uuid, password_hash: &str) -> AppResult<Option<User>>;

    async fn activate_password(&self, id: Uuid, password_hash: &str) -> AppResult<Option<User>> {
        self.update_password_hash(id, password_hash).await
    }

    async fn has_login_method(&self, _id: Uuid, method: LoginMethod) -> AppResult<bool> {
        // Keeps lightweight persistence doubles focused on the behavior they test. Production
        // persistence overrides this and treats the table as authoritative.
        Ok(method == LoginMethod::Password)
    }

    async fn create_external_user(&self, _identity: ExternalIdentity<'_>) -> AppResult<User> {
        Err(AppError::Internal(
            "External-provider registration is not supported by this persistence".into(),
        ))
    }

    async fn get_by_external_subject(
        &self,
        _provider: LoginMethod,
        _subject: &str,
    ) -> AppResult<Option<User>> {
        Ok(None)
    }

    /// Refresh proof only if the provider's verified claim names the current stored address.
    async fn login_external_identity(
        &self,
        identity: ExternalIdentity<'_>,
    ) -> AppResult<Option<User>>;

    async fn link_external_identity(
        &self,
        _user_id: Uuid,
        _identity: ExternalIdentity<'_>,
    ) -> AppResult<()> {
        Err(AppError::Internal(
            "External-provider linking is not supported by this persistence".into(),
        ))
    }
}

/// Which account field a pending change will write when its code arrives.
///
/// Parsed from the stored `kind` column once, at the adapter boundary, so every place that acts on
/// a pending change matches exhaustively and a third kind is a compile error rather than a silently
/// ignored row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountChangeKind {
    Email,
    Password,
}

impl AccountChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountChangeKind::Email => "email",
            AccountChangeKind::Password => "password",
        }
    }

    /// What the stored column means. `None` for anything else, which is a row the schema's own
    /// CHECK constraint should already have refused.
    pub fn parse(stored: &str) -> Option<Self> {
        match stored {
            "email" => Some(AccountChangeKind::Email),
            "password" => Some(AccountChangeKind::Password),
            _ => None,
        }
    }
}

/// The value a confirmed change will write, carried as one enum so a row cannot claim to be an
/// email change while holding a password hash.
#[derive(Debug)]
pub enum AccountChange<'a> {
    Email(&'a EmailAddress),
    /// Already hashed. The plaintext is not the adapter layer's to hold, not even briefly.
    Password(&'a str),
}

impl AccountChange<'_> {
    pub fn kind(&self) -> AccountChangeKind {
        match self {
            AccountChange::Email(_) => AccountChangeKind::Email,
            AccountChange::Password(_) => AccountChangeKind::Password,
        }
    }
}

/// A change waiting on the code that was mailed out for it.
pub struct PendingAccountChange<'a> {
    pub user_id: Uuid,
    pub change: AccountChange<'a>,
    pub confirmation_code_hash: &'a str,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// A change this account has asked for and not yet confirmed, as the page needs to show it.
///
/// The address a code went to is part of the email variant because that is the *new* one -- the
/// whole point of confirming an address change is that the code goes somewhere the account cannot
/// yet receive mail as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingChange {
    Email {
        new_email: EmailAddress,
        expires_at: chrono::DateTime<chrono::Utc>,
    },
    Password {
        expires_at: chrono::DateTime<chrono::Utc>,
    },
}

impl PendingChange {
    pub fn kind(&self) -> AccountChangeKind {
        match self {
            PendingChange::Email { .. } => AccountChangeKind::Email,
            PendingChange::Password { .. } => AccountChangeKind::Password,
        }
    }
}

/// Delivers a confirmation code to an address.
///
/// A port rather than a direct call into the SMTP dispatcher, because a code that only exists in
/// somebody's inbox cannot be asserted on: this is the seam a test hands a recorder to, so the
/// confirm half of every flow below is exercised rather than assumed.
#[async_trait]
pub trait ConfirmationCodeSender: Send + Sync {
    async fn send_code(
        &self,
        recipient: &EmailAddress,
        code: &str,
        purpose: ConfirmationPurpose,
    ) -> AppResult<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationPurpose {
    Registration,
    EmailChange,
    PasswordChange,
}

#[async_trait]
pub trait AccountChangePersistence: Send + Sync {
    /// Records a change against its code, replacing any earlier request of the same kind so an
    /// abandoned code cannot still be confirmed after a second one has been sent.
    async fn save_pending_account_change(&self, pending: PendingAccountChange<'_>)
    -> AppResult<()>;

    /// Writes the pending change of `kind` if the code matches and has not expired, and clears the
    /// request. `Ok(None)` means there was no such pending change to confirm -- a wrong code, an
    /// expired one, or one already used.
    async fn confirm_pending_account_change(
        &self,
        user_id: Uuid,
        kind: AccountChangeKind,
        confirmation_code_hash: &str,
        attempt_key: &AuthAttemptKey,
    ) -> AppResult<Option<User>>;

    /// What this account is waiting on, so a reloaded page asks for the code again rather than
    /// silently forgetting a change was requested. Expired requests are not returned.
    async fn list_pending_account_changes(&self, user_id: Uuid) -> AppResult<Vec<PendingChange>>;

    /// Abandons a request, so its owner can go back to the form and ask for something else.
    async fn discard_pending_account_change(
        &self,
        user_id: Uuid,
        kind: AccountChangeKind,
    ) -> AppResult<()>;
}

pub trait UserCredentialsHasher: Send + Sync {
    fn hash_password(&self, password: &str) -> AppResult<String>;
    fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool>;
}

/// Proof machinery and the immutable outbound transport policy selected at startup.
///
/// A struct rather than the four positional `Arc`s it would otherwise be: `registrations` and
/// `account_changes` are the same value on every real deployment, and positionally they would be
/// indistinguishable.
#[derive(Clone)]
pub struct EmailConfirmation {
    pub registrations: Arc<dyn RegistrationPersistence>,
    pub account_changes: Arc<dyn AccountChangePersistence>,
    pub codes: Arc<dyn ConfirmationCodeSender>,
    pub config: Arc<AppConfig>,
    pub transport_available: bool,
    pub allow_unverified_without_smtp: bool,
}

impl EmailConfirmation {
    /// Mails a fresh code for `change` and records what it will write, returning the request as
    /// the page has to show it.
    ///
    /// The mail is sent *after* the request is stored, so a code that reaches somebody's inbox is
    /// always one this deployment can still honour.
    async fn request(
        &self,
        user_id: Uuid,
        change: AccountChange<'_>,
        send_to: &EmailAddress,
    ) -> AppResult<PendingChange> {
        let kind = change.kind();
        let code = format!("{:06}", confirmation_number());
        let expires_at = chrono::Utc::now() + chrono::Duration::minutes(CONFIRMATION_TTL_MINUTES);

        self.account_changes
            .save_pending_account_change(PendingAccountChange {
                user_id,
                change,
                confirmation_code_hash: &account_change_code_hash(
                    &self.config,
                    user_id,
                    kind,
                    &code,
                ),
                expires_at,
            })
            .await?;

        self.codes
            .send_code(
                send_to,
                &code,
                match kind {
                    AccountChangeKind::Email => ConfirmationPurpose::EmailChange,
                    AccountChangeKind::Password => ConfirmationPurpose::PasswordChange,
                },
            )
            .await?;

        Ok(match kind {
            AccountChangeKind::Email => PendingChange::Email {
                new_email: send_to.clone(),
                expires_at,
            },
            AccountChangeKind::Password => PendingChange::Password { expires_at },
        })
    }
}

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
    attempt_secret: Arc<str>,
    confirmation: Option<EmailConfirmation>,
}

impl UserUseCases {
    pub fn new(
        hasher: Arc<dyn UserCredentialsHasher>,
        persistence: Arc<dyn UserPersistence>,
        attempt_secret: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            hasher,
            hash_slots: Arc::new(Semaphore::new(MAX_PASSWORD_HASH_JOBS)),
            persistence,
            attempt_secret: attempt_secret.into(),
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

    pub async fn register(
        &self,
        username: &str,
        email: &str,
        password: &SecretString,
        work_lifetime: Option<AuthWorkLifetime>,
    ) -> AppResult<RegistrationOutcome> {
        let confirming = self.confirming();
        if confirming.is_none() && !self.unverified_email_bypass() {
            return Err(AppError::BadRequest(
                "Password registration requires email confirmation; outbound SMTP is not configured."
                    .into(),
            ));
        }
        let username = username.trim();
        let email = EmailAddress::from(email.trim());
        if username.is_empty() {
            return Err(AppError::BadRequest("A username is required.".into()));
        }
        if !is_plausible_address(&email) {
            return Err(AppError::BadRequest(
                "Enter an email address of the form name@example.com.".into(),
            ));
        }
        if self.persistence.get_by_email(&email).await?.is_some()
            || self.persistence.get_by_username(username).await?.is_some()
        {
            return Err(AppError::BadRequest(
                "An account with that username or email already exists".into(),
            ));
        }
        let Some(confirmation) = confirming else {
            return self
                .add(username, email.as_str(), password, work_lifetime)
                .await
                .map(RegistrationOutcome::CreatedUnverified);
        };
        let config = &confirmation.config;
        let password_hash = self
            .hash_password(password.expose_secret(), work_lifetime)
            .await?;
        let code = format!("{:06}", confirmation_number());
        let canonical_email = self
            .persistence
            .canonical_auth_identifier(email.as_str())
            .await?;
        let code_hash = confirmation_code_hash(config, &canonical_email, &code);
        confirmation
            .registrations
            .save_pending_registration(PendingRegistration {
                username,
                email: &email,
                password_hash: &password_hash,
                confirmation_code_hash: &code_hash,
                expires_at: chrono::Utc::now()
                    + chrono::Duration::minutes(CONFIRMATION_TTL_MINUTES),
            })
            .await?;
        confirmation
            .codes
            .send_code(&email, &code, ConfirmationPurpose::Registration)
            .await?;
        Ok(RegistrationOutcome::ConfirmationSent)
    }

    pub async fn confirm_registration(&self, email: &str, code: &str) -> AppResult<User> {
        let Some(confirmation) = self.confirming() else {
            return Err(AppError::BadRequest(
                "Email confirmation is not enabled".into(),
            ));
        };
        let email = EmailAddress::from(email.trim());
        let normalized = self
            .persistence
            .canonical_auth_identifier(email.as_str())
            .await?;
        let attempt_key = auth_attempt_key(
            &self.attempt_secret,
            "registration-code:v1",
            &[normalized.as_bytes()],
        );
        confirmation
            .registrations
            .confirm_pending_registration(
                &email,
                &confirmation_code_hash(&confirmation.config, &normalized, code),
                &attempt_key,
            )
            .await?
            .ok_or_else(|| AppError::BadRequest("Invalid or expired confirmation code".into()))
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
            &self.attempt_secret,
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

        let is_valid = self
            .verify_password(password.expose_secret(), &user.password_hash, work_lifetime)
            .await?;

        if !is_valid {
            return Err(AppError::InvalidCredentials);
        }

        self.persistence.clear_login_attempt(&key).await?;
        info!(user_id = %user.id, "User login successful");
        Ok(user)
    }

    pub async fn register_external(&self, identity: ExternalIdentity<'_>) -> AppResult<User> {
        require_verified_external_identity(&identity)?;
        if self
            .persistence
            .get_by_email(identity.email)
            .await?
            .is_some()
            || self
                .persistence
                .get_by_external_subject(identity.provider, identity.subject)
                .await?
                .is_some()
        {
            return Err(AppError::BadRequest(
                "An account with that identity or email already exists. Sign in using a connected method.".into(),
            ));
        }
        self.persistence.create_external_user(identity).await
    }

    pub async fn login_external(&self, identity: ExternalIdentity<'_>) -> AppResult<User> {
        require_verified_external_identity(&identity)?;
        self.persistence
            .login_external_identity(identity)
            .await?
            .ok_or(AppError::InvalidCredentials)
    }

    pub async fn link_external(
        &self,
        user_id: Uuid,
        identity: ExternalIdentity<'_>,
    ) -> AppResult<User> {
        require_verified_external_identity(&identity)?;
        let user = self.account(user_id).await?;
        let account_email = EmailAddress::from(user.email.as_str());
        if !account_email.eq_ignore_case(identity.email) {
            return Err(AppError::BadRequest(format!(
                "The verified {} email must match your account email.",
                identity.provider.as_str()
            )));
        }
        if let Some(owner) = self
            .persistence
            .get_by_external_subject(identity.provider, identity.subject)
            .await?
        {
            return if owner.id == user_id {
                self.persistence
                    .login_external_identity(identity)
                    .await?
                    .ok_or(AppError::InvalidCredentials)
            } else {
                Err(AppError::Conflict(
                    "That login identity is already connected to another account.".into(),
                ))
            };
        }
        if self
            .persistence
            .has_login_method(user_id, identity.provider)
            .await?
        {
            return Err(AppError::Conflict(format!(
                "This account already has a {} login.",
                identity.provider.as_str()
            )));
        }
        self.persistence
            .link_external_identity(user_id, identity)
            .await?;
        self.account(user_id).await
    }

    pub async fn login_methods(&self, id: Uuid) -> AppResult<LoginMethods> {
        self.account(id).await?;
        Ok(LoginMethods {
            password: self
                .persistence
                .has_login_method(id, LoginMethod::Password)
                .await?,
            google: self
                .persistence
                .has_login_method(id, LoginMethod::Google)
                .await?,
            apple: self
                .persistence
                .has_login_method(id, LoginMethod::Apple)
                .await?,
        })
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

    /// Saves the account's own details: what it is called, where its mail goes, and its picture.
    ///
    /// Name and picture are written straight away. A *different* address is not: it is mailed a
    /// code first, and only becomes the account's when that code comes back -- an address written
    /// on nothing but a signed-in session is one a borrowed browser could point at itself.
    ///
    /// Name and address are trimmed and checked here rather than at the form, so the API and the
    /// page cannot disagree about what an acceptable account looks like.
    #[instrument(skip_all)]
    pub async fn update_profile(&self, id: Uuid, profile: ProfileUpdate<'_>) -> AppResult<Saved> {
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

        let account = self.account(id).await?;
        let stored_email = EmailAddress::from(account.email.as_str());
        let address_changed = !stored_email.eq_ignore_case(&email);

        info!("Updating profile for user {id}...");

        // The address stays as it is until it is proved; everything else on the form is written
        // now, so a rejected or abandoned code does not also lose a rename.
        let confirming = self.confirming().filter(|_| address_changed);
        let bypass = address_changed && self.unverified_email_bypass();
        let user = if bypass {
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
        } else {
            self.persistence
                .update_profile_display(id, username, profile.avatar_url)
                .await?
        }
        .ok_or_else(|| AppError::NotFound("User not found".into()))?;

        let Some(confirmation) = confirming else {
            let address = if bypass {
                AddressSaveOutcome::InstalledUnverified
            } else if !user.email.eq_ignore_ascii_case(stored_email.as_str()) {
                AddressSaveOutcome::CurrentAddressChanged
            } else if address_changed {
                AddressSaveOutcome::Unavailable
            } else {
                AddressSaveOutcome::Unchanged
            };
            return Ok(Saved { user, address });
        };

        // Whether the address is free is asked here so the reader hears about it now rather than
        // after a round trip through their inbox. It is asked again by the unique constraint the
        // confirmation writes through, which is what actually decides a race.
        if self.persistence.get_by_email(&email).await?.is_some() {
            return Err(AppError::BadRequest(format!(
                "An account already uses the address '{email}'."
            )));
        }

        let pending = confirmation
            .request(id, AccountChange::Email(&email), &email)
            .await?;

        Ok(Saved {
            user,
            address: AddressSaveOutcome::ConfirmationPending(pending),
        })
    }

    /// Replaces the account's password, once the account has proved it knows the current one --
    /// and, where codes are mailed at all, that it can read the address the account is registered
    /// at. Knowing the old password and holding the mailbox are two different proofs, and a
    /// password is worth both.
    ///
    /// Every rejection here propagates: an error reaching the hasher or the store must not be
    /// mistaken for "the password did not match", nor for a change that went through.
    #[instrument(skip_all)]
    pub async fn change_password(
        &self,
        id: Uuid,
        change: PasswordChange<'_>,
        work_lifetime: Option<AuthWorkLifetime>,
    ) -> AppResult<PasswordOutcome> {
        let user = self.account(id).await?;

        if !self
            .persistence
            .has_login_method(id, LoginMethod::Password)
            .await?
        {
            return Err(AppError::BadRequest(
                "This account was not registered with a password.".into(),
            ));
        }

        if !self
            .verify_password(
                change.current.expose_secret(),
                &user.password_hash,
                work_lifetime.clone(),
            )
            .await?
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
        let hash = self.hash_password(new_password, work_lifetime).await?;

        let Some(confirmation) = self.confirming() else {
            self.persistence
                .update_password_hash(id, &hash)
                .await?
                .ok_or_else(|| AppError::NotFound("User not found".into()))?;
            return Ok(PasswordOutcome::Changed);
        };

        // The code goes to the address the account already has, not to anything on the form: this
        // proves the mailbox, and the mailbox is the one thing an attacker holding the session
        // does not have.
        let account_email = EmailAddress::from(user.email.as_str());
        let pending = confirmation
            .request(id, AccountChange::Password(&hash), &account_email)
            .await?;

        Ok(PasswordOutcome::ConfirmationSent(pending))
    }

    /// Adds password authentication to an OAuth-only account. The mailbox confirmation is the
    /// re-authentication proof because there is deliberately no current password to ask for.
    #[instrument(skip_all)]
    pub async fn set_password(
        &self,
        id: Uuid,
        setup: PasswordSetup<'_>,
        work_lifetime: Option<AuthWorkLifetime>,
    ) -> AppResult<PasswordOutcome> {
        let user = self.account(id).await?;
        if self
            .persistence
            .has_login_method(id, LoginMethod::Password)
            .await?
        {
            return Err(AppError::Conflict(
                "This account already has a password. Change it instead.".into(),
            ));
        }

        let new_password = setup.new.expose_secret();
        if new_password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(AppError::BadRequest(format!(
                "A new password needs at least {MIN_PASSWORD_CHARS} characters."
            )));
        }
        if new_password != setup.confirmation.expose_secret() {
            return Err(AppError::BadRequest(
                "The new password and its confirmation do not match.".into(),
            ));
        }
        let hash = self.hash_password(new_password, work_lifetime).await?;
        let Some(confirmation) = self.confirming() else {
            self.persistence
                .activate_password(id, &hash)
                .await?
                .ok_or_else(|| AppError::NotFound("User not found".into()))?;
            return Ok(PasswordOutcome::Changed);
        };
        let account_email = EmailAddress::from(user.email.as_str());
        let pending = confirmation
            .request(id, AccountChange::Password(&hash), &account_email)
            .await?;
        Ok(PasswordOutcome::ConfirmationSent(pending))
    }

    /// Writes a change its owner has now proved, and clears the request.
    ///
    /// A wrong code, an expired one and one already spent are deliberately one answer: telling
    /// them apart tells somebody guessing which of their guesses was closest.
    #[instrument(skip_all)]
    pub async fn confirm_account_change(
        &self,
        id: Uuid,
        kind: AccountChangeKind,
        code: &str,
    ) -> AppResult<User> {
        let Some(confirmation) = self.confirming() else {
            return Err(AppError::BadRequest(
                "Email confirmation is not enabled".into(),
            ));
        };
        info!("Confirming {} change for user {id}...", kind.as_str());

        let attempt_key = auth_attempt_key(
            &self.attempt_secret,
            "account-change-code:v1",
            &[id.as_bytes(), kind.as_str().as_bytes()],
        );

        confirmation
            .account_changes
            .confirm_pending_account_change(
                id,
                kind,
                &account_change_code_hash(&confirmation.config, id, kind, code),
                &attempt_key,
            )
            .await?
            .ok_or_else(|| AppError::BadRequest("Invalid or expired confirmation code".into()))
    }

    /// Abandons a requested change, so its owner can go back to the form and ask for something
    /// else rather than waiting the code out.
    #[instrument(skip_all)]
    pub async fn discard_account_change(&self, id: Uuid, kind: AccountChangeKind) -> AppResult<()> {
        let Some(confirmation) = self.confirming() else {
            return Ok(());
        };

        confirmation
            .account_changes
            .discard_pending_account_change(id, kind)
            .await
    }

    /// What this account has asked for and not yet proved, so a reloaded page keeps asking for the
    /// code instead of quietly forgetting the request.
    #[instrument(skip_all)]
    pub async fn pending_account_changes(&self, id: Uuid) -> AppResult<Vec<PendingChange>> {
        let Some(confirmation) = self.confirming() else {
            return Ok(Vec::new());
        };

        confirmation
            .account_changes
            .list_pending_account_changes(id)
            .await
    }

    /// The stored account, or the `NotFound` a stale session deserves.
    async fn account(&self, id: Uuid) -> AppResult<User> {
        self.persistence
            .get_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".into()))
    }

    /// Points the account at a new profile picture, or back at its letter bubble with `None`.
    ///
    /// The URL arrives already parsed: [`AvatarUrl::parse`] is what rejects a scheme no `<img>`
    /// should be pointed at, and it runs where the form is read.
    #[instrument(skip_all)]
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
fn require_verified_external_identity(identity: &ExternalIdentity<'_>) -> AppResult<()> {
    if identity.provider == LoginMethod::Password || !identity.email_verified {
        return Err(AppError::BadRequest(
            "An external identity must carry a verified provider email claim.".into(),
        ));
    }
    Ok(())
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

fn confirmation_number() -> u32 {
    let bytes = *Uuid::new_v4().as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 1_000_000
}

/// A change's code, bound to the account *and* the field it changes.
///
/// Both are in the digest on purpose: without the kind, a code mailed for a password change would
/// also confirm a pending address change, and the two are mailed to different places.
fn account_change_code_hash(
    config: &AppConfig,
    user_id: Uuid,
    kind: AccountChangeKind,
    code: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(&config.jwt_secret);
    digest.update(":");
    digest.update(user_id.as_bytes());
    digest.update(":");
    digest.update(kind.as_str());
    digest.update(":");
    digest.update(code);
    format!("{:x}", digest.finalize())
}

fn confirmation_code_hash(config: &AppConfig, canonical_email: &str, code: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(&config.jwt_secret);
    digest.update(":");
    digest.update(canonical_email);
    digest.update(":");
    digest.update(code);
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod test {
    use async_trait::async_trait;
    use uuid::Uuid;

    use super::*;

    #[tokio::test]
    async fn security_auth_limits_unknown_and_oauth_only_use_dummy_verification() {
        struct RecordingHasher(std::sync::Mutex<Vec<String>>);
        impl UserCredentialsHasher for RecordingHasher {
            fn hash_password(&self, _: &str) -> AppResult<String> {
                unreachable!()
            }
            fn verify_password(&self, _: &str, hash: &str) -> AppResult<bool> {
                self.0.lock().unwrap().push(hash.to_owned());
                Ok(false)
            }
        }
        let hasher = Arc::new(RecordingHasher(std::sync::Mutex::new(Vec::new())));
        let persistence = Arc::new(MockUserPersistence::new());
        let users = UserUseCases::new(hasher.clone(), persistence.clone(), "test-attempt-secret");
        assert!(matches!(
            users.login("testuser", &"wrong".into(), None, None).await,
            Err(AppError::InvalidCredentials)
        ));
        assert!(matches!(
            users.login("missing", &"wrong".into(), None, None).await,
            Err(AppError::InvalidCredentials)
        ));
        persistence
            .password_enabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(
            users.login("testuser", &"wrong".into(), None, None).await,
            Err(AppError::InvalidCredentials)
        ));
        assert_eq!(
            *hasher.0.lock().unwrap(),
            ["secret_hash", DUMMY_PASSWORD_HASH, DUMMY_PASSWORD_HASH]
        );
    }

    #[tokio::test]
    async fn security_auth_limits_dummy_hash_is_valid_argon2() {
        let users = UserUseCases::new(
            Arc::new(crate::infra::argon2_password_hasher()),
            Arc::new(MockUserPersistence::new()),
            "test-attempt-secret",
        );
        assert!(matches!(
            users.login("missing", &"wrong".into(), None, None).await,
            Err(AppError::InvalidCredentials)
        ));
    }

    #[tokio::test]
    async fn security_auth_limits_hash_capacity_fails_before_fifth_job() {
        use std::sync::{
            Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        struct HeldHasher {
            started: AtomicUsize,
            release: (Mutex<bool>, Condvar),
        }
        impl UserCredentialsHasher for HeldHasher {
            fn hash_password(&self, _: &str) -> AppResult<String> {
                self.started.fetch_add(1, Ordering::SeqCst);
                let (lock, condition) = &self.release;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = condition.wait(released).unwrap();
                }
                Ok("hash".into())
            }
            fn verify_password(&self, _: &str, _: &str) -> AppResult<bool> {
                unreachable!()
            }
        }
        struct ReleaseOnDrop(Arc<HeldHasher>);
        impl Drop for ReleaseOnDrop {
            fn drop(&mut self) {
                let (lock, condition) = &self.0.release;
                *lock.lock().unwrap() = true;
                condition.notify_all();
            }
        }
        let hasher = Arc::new(HeldHasher {
            started: AtomicUsize::new(0),
            release: (Mutex::new(false), Condvar::new()),
        });
        let release_guard = ReleaseOnDrop(hasher.clone());
        let users = Arc::new(UserUseCases::new(
            hasher.clone(),
            Arc::new(MockUserPersistence::new()),
            "test-attempt-secret",
        ));
        let jobs: Vec<_> = (0..MAX_PASSWORD_HASH_JOBS)
            .map(|_| {
                let users = users.clone();
                tokio::spawn(async move {
                    users
                        .add("testuser", "testuser@gmail.com", &"secret".into(), None)
                        .await
                })
            })
            .collect();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while hasher.started.load(Ordering::SeqCst) < MAX_PASSWORD_HASH_JOBS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            users
                .add("testuser", "testuser@gmail.com", &"secret".into(), None)
                .await,
            Err(AppError::ServiceUnavailable(_))
        ));
        assert_eq!(
            hasher.started.load(Ordering::SeqCst),
            MAX_PASSWORD_HASH_JOBS
        );
        drop(release_guard);
        for job in jobs {
            job.await.unwrap().unwrap();
        }
    }

    #[tokio::test]
    async fn security_auth_limits_cancelled_hash_keeps_source_capacity() {
        use std::{
            net::SocketAddr,
            sync::{
                Condvar, Mutex,
                atomic::{AtomicUsize, Ordering},
            },
        };

        use axum::{
            Extension, Router,
            body::Body,
            extract::connect_info::ConnectInfo,
            http::{Request, StatusCode},
            middleware,
            routing::post,
        };
        use tower::ServiceExt;

        use crate::adapters::http::auth_admission::{self, AuthAdmission};

        struct HeldHasher {
            started: AtomicUsize,
            release: (Mutex<bool>, Condvar),
        }
        impl UserCredentialsHasher for HeldHasher {
            fn hash_password(&self, _: &str) -> AppResult<String> {
                self.started.fetch_add(1, Ordering::SeqCst);
                let (lock, condition) = &self.release;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = condition.wait(released).unwrap();
                }
                Ok("secret_hash".into())
            }
            fn verify_password(&self, _: &str, _: &str) -> AppResult<bool> {
                unreachable!()
            }
        }
        struct ReleaseOnDrop(Arc<HeldHasher>);
        impl Drop for ReleaseOnDrop {
            fn drop(&mut self) {
                let (lock, condition) = &self.0.release;
                *lock.lock().unwrap() = true;
                condition.notify_all();
            }
        }

        fn request(peer: &str) -> Request<Body> {
            let mut request = Request::post("/api/json/user/register")
                .body(Body::empty())
                .unwrap();
            request
                .extensions_mut()
                .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
            request
        }

        let hasher = Arc::new(HeldHasher {
            started: AtomicUsize::new(0),
            release: (Mutex::new(false), Condvar::new()),
        });
        let release = ReleaseOnDrop(hasher.clone());
        let users = Arc::new(UserUseCases::new(
            hasher.clone(),
            Arc::new(MockUserPersistence::new()),
            "test-attempt-secret",
        ));
        let app = Router::new()
            .route(
                "/api/json/user/register",
                post(move |Extension(work): Extension<AuthWorkLifetime>| {
                    let users = users.clone();
                    async move {
                        users
                            .add(
                                "testuser",
                                "testuser@gmail.com",
                                &"secret".into(),
                                Some(work),
                            )
                            .await
                            .map(|_| StatusCode::OK)
                            .unwrap_or(StatusCode::SERVICE_UNAVAILABLE)
                    }
                }),
            )
            .layer(middleware::from_fn_with_state(
                Arc::new(AuthAdmission::new(vec![])),
                auth_admission::admit,
            ));

        let first = tokio::spawn(app.clone().oneshot(request("192.0.2.10:1001")));
        let second = tokio::spawn(app.clone().oneshot(request("192.0.2.10:1002")));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while hasher.started.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("two hash jobs started");

        first.abort();
        second.abort();
        let _ = first.await;
        let _ = second.await;
        assert_eq!(
            app.clone()
                .oneshot(request("192.0.2.10:1003"))
                .await
                .unwrap()
                .status(),
            StatusCode::TOO_MANY_REQUESTS,
            "cancelled requests still own source capacity while their hashes run"
        );

        let other = tokio::spawn(app.clone().oneshot(request("192.0.2.11:1001")));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while hasher.started.load(Ordering::SeqCst) < 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("another source reaches a free global hash slot");
        drop(release);
        assert_eq!(other.await.unwrap().unwrap().status(), StatusCode::OK);
        assert_eq!(
            app.oneshot(request("192.0.2.10:1004"))
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
            "source capacity recovers when held workers finish"
        );
    }

    /// The stored account, so a write is observable by the read that follows it.
    struct MockUserPersistence {
        stored: std::sync::Mutex<User>,
        password_enabled: std::sync::atomic::AtomicBool,
        confirmed_address_after_read: std::sync::Mutex<Option<String>>,
    }

    impl MockUserPersistence {
        fn new() -> Self {
            Self {
                stored: std::sync::Mutex::new(User {
                    auth_revision: 0,
                    id: Uuid::new_v4(),
                    username: "testuser".to_string(),
                    email: "testuser@gmail.com".to_string(),
                    email_verified_at: None,
                    password_hash: "secret_hash".to_string(),
                    avatar_url: None,
                    created_at: chrono::Utc::now(),
                }),
                password_enabled: std::sync::atomic::AtomicBool::new(true),
                confirmed_address_after_read: std::sync::Mutex::new(None),
            }
        }

        fn id(&self) -> Uuid {
            self.stored.lock().unwrap().id
        }
    }

    #[async_trait]
    impl UserPersistence for MockUserPersistence {
        async fn login_external_identity(
            &self,
            _: ExternalIdentity<'_>,
        ) -> AppResult<Option<User>> {
            Err(AppError::Internal(
                "External login proof is not supported by this test persistence".into(),
            ))
        }

        async fn canonical_auth_identifier(&self, value: &str) -> AppResult<String> {
            Ok(value.to_lowercase())
        }
        async fn reserve_login_attempt(&self, _: &AuthAttemptKey) -> AppResult<bool> {
            Ok(true)
        }

        async fn clear_login_attempt(&self, _: &AuthAttemptKey) -> AppResult<()> {
            Ok(())
        }

        async fn create_user(
            &self,
            username: &str,
            email: &str,
            password_hash: &str,
        ) -> AppResult<User> {
            Ok(User {
                auth_revision: 0,
                id: Uuid::new_v4(),
                username: username.to_string(),
                email: email.to_string(),
                email_verified_at: None,
                password_hash: password_hash.to_string(),
                avatar_url: None,
                created_at: chrono::Utc::now(),
            })
        }

        async fn get_by_email(&self, email: &str) -> AppResult<Option<User>> {
            if email == "testuser@gmail.com" {
                Ok(Some(User {
                    auth_revision: 0,
                    id: Uuid::new_v4(),
                    username: "testuser".to_string(),
                    email: "testuser@gmail.com".to_string(),
                    email_verified_at: None,
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
                    auth_revision: 0,
                    id: Uuid::new_v4(),
                    username: "testuser".to_string(),
                    email: "testuser@gmail.com".to_string(),
                    email_verified_at: None,
                    password_hash: "secret_hash".to_string(),
                    avatar_url: None,
                    created_at: chrono::Utc::now(),
                }))
            } else {
                Ok(None)
            }
        }

        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<User>> {
            let mut stored = self.stored.lock().unwrap();
            let observed = stored.clone();
            if let Some(confirmed) = self.confirmed_address_after_read.lock().unwrap().take() {
                stored.email = confirmed;
                stored.email_verified_at = Some(chrono::Utc::now());
            }
            Ok(Some(observed))
        }

        async fn notification_preferences(&self, _id: Uuid) -> AppResult<NotificationPreferences> {
            Ok(NotificationPreferences::default())
        }

        async fn set_notification_preferences(
            &self,
            _id: Uuid,
            _preferences: NotificationPreferences,
        ) -> AppResult<()> {
            Ok(())
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
            if !stored.email.eq_ignore_ascii_case(profile.email.as_str()) {
                stored.email_verified_at = None;
            }
            stored.username = profile.username.to_string();
            stored.email = profile.email.to_string();
            stored.avatar_url = profile.avatar_url.cloned();
            Ok(Some(stored.clone()))
        }

        async fn update_profile_display(
            &self,
            _id: Uuid,
            username: &str,
            avatar_url: Option<&AvatarUrl>,
        ) -> AppResult<Option<User>> {
            let mut stored = self.stored.lock().unwrap();
            stored.username = username.to_string();
            stored.avatar_url = avatar_url.cloned();
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

        async fn activate_password(
            &self,
            id: Uuid,
            password_hash: &str,
        ) -> AppResult<Option<User>> {
            let updated = self.update_password_hash(id, password_hash).await?;
            self.password_enabled
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(updated)
        }

        async fn has_login_method(&self, _id: Uuid, method: LoginMethod) -> AppResult<bool> {
            Ok(match method {
                LoginMethod::Password => self
                    .password_enabled
                    .load(std::sync::atomic::Ordering::SeqCst),
                LoginMethod::Google | LoginMethod::Apple => false,
            })
        }
    }

    struct MockUserCredentialsHasher;

    impl UserCredentialsHasher for MockUserCredentialsHasher {
        fn hash_password(&self, password: &str) -> AppResult<String> {
            Ok(format!("{}_hash", password))
        }

        fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool> {
            Ok(hash == format!("{}_hash", password)
                || hash == "secret_hash" && password == "secret")
        }
    }

    #[tokio::test]
    async fn add_user_works() {
        let user_use_cases = UserUseCases::new(
            Arc::new(MockUserCredentialsHasher),
            Arc::new(MockUserPersistence::new()),
            "test-attempt-secret",
        );

        let user = user_use_cases
            .add(
                "testuser",
                "testuser@gmail.com",
                &"testuser_pw".into(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, "testuser@gmail.com");
    }

    fn use_cases(persistence: Arc<MockUserPersistence>) -> UserUseCases {
        UserUseCases::new(
            Arc::new(MockUserCredentialsHasher),
            persistence,
            "test-attempt-secret",
        )
    }

    enum NoSmtpPolicy {
        Deny,
        AllowUnverified,
    }

    fn no_smtp_use_cases(
        persistence: Arc<MockUserPersistence>,
        policy: NoSmtpPolicy,
    ) -> UserUseCases {
        use_cases(persistence).with_email_confirmation(EmailConfirmation {
            registrations: Arc::new(MockRegistrations),
            account_changes: Arc::new(MockAccountChanges::default()),
            codes: Arc::new(RecordingSender::default()),
            config: Arc::new(AppConfig::for_test()),
            transport_available: false,
            allow_unverified_without_smtp: matches!(policy, NoSmtpPolicy::AllowUnverified),
        })
    }

    /// The pending changes an account has asked for, in memory, plus the codes that were mailed.
    ///
    /// It records the *hash* rather than the code, exactly as Postgres does, so the tests below
    /// confirm the same way a browser does: by presenting a code that has to hash to what was
    /// stored.
    struct StoredAccountChange {
        user_id: Uuid,
        kind: AccountChangeKind,
        payload: String,
        confirmation_code_hash: String,
        live: bool,
    }

    #[derive(Default)]
    struct MockAccountChanges {
        stored: std::sync::Mutex<Vec<StoredAccountChange>>,
    }

    #[async_trait]
    impl AccountChangePersistence for MockAccountChanges {
        async fn save_pending_account_change(
            &self,
            pending: PendingAccountChange<'_>,
        ) -> AppResult<()> {
            let kind = pending.change.kind();
            let payload = match pending.change {
                AccountChange::Email(email) => email.to_string(),
                AccountChange::Password(hash) => hash.to_string(),
            };
            let mut stored = self.stored.lock().unwrap();
            stored.retain(|change| !(change.user_id == pending.user_id && change.kind == kind));
            stored.push(StoredAccountChange {
                user_id: pending.user_id,
                kind,
                payload,
                confirmation_code_hash: pending.confirmation_code_hash.to_string(),
                live: pending.expires_at > chrono::Utc::now(),
            });
            Ok(())
        }

        async fn confirm_pending_account_change(
            &self,
            user_id: Uuid,
            kind: AccountChangeKind,
            confirmation_code_hash: &str,
            _: &AuthAttemptKey,
        ) -> AppResult<Option<User>> {
            let mut stored = self.stored.lock().unwrap();
            let found = stored.iter().position(|change| {
                change.user_id == user_id
                    && change.kind == kind
                    && change.confirmation_code_hash == confirmation_code_hash
                    && change.live
            });
            let Some(found) = found else { return Ok(None) };
            let payload = stored.remove(found).payload;

            Ok(Some(User {
                auth_revision: 0,
                id: user_id,
                username: "testuser".to_string(),
                email: match kind {
                    AccountChangeKind::Email => payload.clone(),
                    AccountChangeKind::Password => "testuser@gmail.com".to_string(),
                },
                email_verified_at: (kind == AccountChangeKind::Email).then(chrono::Utc::now),
                password_hash: match kind {
                    AccountChangeKind::Email => "secret_hash".to_string(),
                    AccountChangeKind::Password => payload,
                },
                avatar_url: None,
                created_at: chrono::Utc::now(),
            }))
        }

        async fn list_pending_account_changes(
            &self,
            user_id: Uuid,
        ) -> AppResult<Vec<PendingChange>> {
            Ok(self
                .stored
                .lock()
                .unwrap()
                .iter()
                .filter(|change| change.user_id == user_id && change.live)
                .map(|change| match change.kind {
                    AccountChangeKind::Email => PendingChange::Email {
                        new_email: EmailAddress::from(change.payload.as_str()),
                        expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
                    },
                    AccountChangeKind::Password => PendingChange::Password {
                        expires_at: chrono::Utc::now() + chrono::Duration::minutes(15),
                    },
                })
                .collect())
        }

        async fn discard_pending_account_change(
            &self,
            user_id: Uuid,
            kind: AccountChangeKind,
        ) -> AppResult<()> {
            self.stored
                .lock()
                .unwrap()
                .retain(|change| !(change.user_id == user_id && change.kind == kind));
            Ok(())
        }
    }

    /// Keeps every code instead of mailing it, so a test can present one back.
    #[derive(Default)]
    struct RecordingSender {
        sent: std::sync::Mutex<Vec<(EmailAddress, String, ConfirmationPurpose)>>,
    }

    impl RecordingSender {
        /// The code from the most recent mail, and where it went.
        fn last(&self) -> (EmailAddress, String, ConfirmationPurpose) {
            self.sent
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("a code was mailed")
        }
    }

    #[async_trait]
    impl ConfirmationCodeSender for RecordingSender {
        async fn send_code(
            &self,
            recipient: &EmailAddress,
            code: &str,
            purpose: ConfirmationPurpose,
        ) -> AppResult<()> {
            self.sent
                .lock()
                .unwrap()
                .push((recipient.clone(), code.to_string(), purpose));
            Ok(())
        }
    }

    struct MockRegistrations;

    #[async_trait]
    impl RegistrationPersistence for MockRegistrations {
        async fn save_pending_registration(&self, _: PendingRegistration<'_>) -> AppResult<()> {
            unimplemented!()
        }
        async fn confirm_pending_registration(
            &self,
            _: &EmailAddress,
            _: &str,
            _: &AuthAttemptKey,
        ) -> AppResult<Option<User>> {
            unimplemented!()
        }
    }

    /// Use cases wired the way a real deployment is: codes are mailed and have to come back.
    ///
    /// Explicitly configured to model a deployment with an outbound confirmation transport.
    fn confirming_use_cases(
        persistence: Arc<MockUserPersistence>,
    ) -> (UserUseCases, Arc<MockAccountChanges>, Arc<RecordingSender>) {
        let changes = Arc::new(MockAccountChanges::default());
        let sender = Arc::new(RecordingSender::default());
        let use_cases = UserUseCases::new(
            Arc::new(MockUserCredentialsHasher),
            persistence,
            "test-attempt-secret",
        )
        .with_email_confirmation(EmailConfirmation {
            registrations: Arc::new(MockRegistrations),
            account_changes: changes.clone(),
            codes: sender.clone(),
            config: Arc::new(AppConfig {
                smtp_host: "smtp.example.com".to_string(),
                smtp_from_address: "noreply@example.com".to_string(),
                ..AppConfig::for_test()
            }),
            transport_available: true,
            allow_unverified_without_smtp: false,
        });

        (use_cases, changes, sender)
    }

    #[tokio::test]
    async fn a_new_address_is_mailed_a_code_and_is_not_the_account_s_until_it_comes_back() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        let (use_cases, _, sender) = confirming_use_cases(persistence.clone());

        let saved = use_cases
            .update_profile(
                id,
                ProfileUpdate {
                    username: "renamed",
                    email: &EmailAddress::from("new@example.com"),
                    avatar_url: None,
                },
            )
            .await
            .unwrap();

        // The rename lands now; the address does not.
        assert_eq!(saved.user.username, "renamed");
        assert_eq!(
            persistence.stored.lock().unwrap().email,
            "testuser@gmail.com"
        );

        // The code goes to the *new* address -- proving it is reachable is the whole point.
        let (sent_to, code, purpose) = sender.last();
        assert_eq!(sent_to.as_str(), "new@example.com");
        assert_eq!(purpose, ConfirmationPurpose::EmailChange);
        assert!(matches!(
            saved.address,
            AddressSaveOutcome::ConfirmationPending(PendingChange::Email { .. })
        ));

        let confirmed = use_cases
            .confirm_account_change(id, AccountChangeKind::Email, &code)
            .await
            .unwrap();
        assert_eq!(confirmed.email, "new@example.com");
        assert!(
            use_cases
                .pending_account_changes(id)
                .await
                .unwrap()
                .is_empty(),
            "a spent code leaves nothing to confirm again"
        );
    }

    #[tokio::test]
    async fn a_new_password_is_mailed_a_code_and_the_old_one_stands_until_it_comes_back() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        let (use_cases, _, sender) = confirming_use_cases(persistence.clone());

        let outcome = use_cases
            .change_password(
                id,
                PasswordChange {
                    current: &"secret".into(),
                    new: &"a-longer-secret".into(),
                    confirmation: &"a-longer-secret".into(),
                },
                None,
            )
            .await
            .unwrap();

        assert!(matches!(outcome, PasswordOutcome::ConfirmationSent(_)));
        assert_eq!(
            persistence.stored.lock().unwrap().password_hash,
            "secret_hash",
            "the stored password must still be the old one"
        );

        // This code goes to the address the account already has: the mailbox is what a stolen
        // session does not have.
        let (sent_to, code, purpose) = sender.last();
        assert_eq!(sent_to.as_str(), "testuser@gmail.com");
        assert_eq!(purpose, ConfirmationPurpose::PasswordChange);

        let confirmed = use_cases
            .confirm_account_change(id, AccountChangeKind::Password, &code)
            .await
            .unwrap();
        assert_eq!(confirmed.password_hash, "a-longer-secret_hash");
    }

    #[tokio::test]
    async fn a_code_confirms_only_the_change_it_was_mailed_for() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        let (use_cases, _, sender) = confirming_use_cases(persistence.clone());

        use_cases
            .update_profile(
                id,
                ProfileUpdate {
                    username: "testuser",
                    email: &EmailAddress::from("new@example.com"),
                    avatar_url: None,
                },
            )
            .await
            .unwrap();
        let (_, email_code, _) = sender.last();

        use_cases
            .change_password(
                id,
                PasswordChange {
                    current: &"secret".into(),
                    new: &"a-longer-secret".into(),
                    confirmation: &"a-longer-secret".into(),
                },
                None,
            )
            .await
            .unwrap();

        // Both are outstanding at once, and the address code must not spend the password request.
        assert_eq!(
            use_cases.pending_account_changes(id).await.unwrap().len(),
            2
        );
        let crossed = use_cases
            .confirm_account_change(id, AccountChangeKind::Password, &email_code)
            .await;
        assert!(matches!(crossed, Err(AppError::BadRequest(_))));

        for wrong in ["000000", "12345", "abcdef"] {
            assert!(
                use_cases
                    .confirm_account_change(id, AccountChangeKind::Email, wrong)
                    .await
                    .is_err(),
                "{wrong} should not confirm anything"
            );
        }
    }

    #[tokio::test]
    async fn a_cancelled_request_gives_the_form_back_and_voids_its_code() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        let (use_cases, _, sender) = confirming_use_cases(persistence);

        use_cases
            .update_profile(
                id,
                ProfileUpdate {
                    username: "testuser",
                    email: &EmailAddress::from("new@example.com"),
                    avatar_url: None,
                },
            )
            .await
            .unwrap();
        let (_, code, _) = sender.last();

        use_cases
            .discard_account_change(id, AccountChangeKind::Email)
            .await
            .unwrap();

        assert!(
            use_cases
                .pending_account_changes(id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            use_cases
                .confirm_account_change(id, AccountChangeKind::Email, &code)
                .await
                .is_err(),
            "a cancelled request's code must not still work"
        );
    }

    #[tokio::test]
    async fn updating_a_profile_trims_and_stores_every_field() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();

        let saved = no_smtp_use_cases(persistence, NoSmtpPolicy::AllowUnverified)
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

        assert_eq!(saved.user.username, "renamed");
        assert_eq!(saved.user.email, "new@example.com");
        assert_eq!(
            saved.user.avatar_url.as_deref(),
            Some("https://cdn.example.com/me.png")
        );
        assert!(matches!(
            saved.address,
            AddressSaveOutcome::InstalledUnverified
        ));
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
                None,
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
                None,
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
                    None,
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
            "test-attempt-secret",
        );

        let user = user_use_cases
            .login("testuser@gmail.com", &"secret".into(), None, None)
            .await
            .unwrap();

        assert_eq!(user.username, "testuser");
    }

    #[tokio::test]
    async fn linking_an_external_provider_requires_the_accounts_email() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        let user_use_cases = use_cases(persistence);
        let other_email = EmailAddress::from("someone-else@example.com");

        let result = user_use_cases
            .link_external(
                id,
                ExternalIdentity {
                    provider: LoginMethod::Apple,
                    subject: "apple-subject",
                    email: &other_email,
                    email_verified: true,
                    display_name: None,
                },
            )
            .await;

        assert!(
            matches!(result, Err(AppError::BadRequest(message)) if message.contains("must match"))
        );
    }

    #[tokio::test]
    async fn security_email_identity_application_rejects_unverified_oauth_claims() {
        let persistence = Arc::new(MockUserPersistence::new());
        let user_id = persistence.id();
        let users = use_cases(persistence);
        let email = EmailAddress::from("testuser@gmail.com");
        let identity = ExternalIdentity {
            provider: LoginMethod::Google,
            subject: "unverified-subject",
            email: &email,
            email_verified: false,
            display_name: None,
        };
        assert!(matches!(
            users.register_external(identity).await,
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            users.login_external(identity).await,
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            users.link_external(user_id, identity).await,
            Err(AppError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn security_email_identity_no_smtp_registration_has_no_proof() {
        let users = no_smtp_use_cases(
            Arc::new(MockUserPersistence::new()),
            NoSmtpPolicy::AllowUnverified,
        );
        let created = users
            .register("newuser", "newuser@example.com", &"password".into(), None)
            .await
            .unwrap();
        let RegistrationOutcome::CreatedUnverified(account) = created else {
            panic!("explicit bypass creates only an unverified account");
        };
        assert!(account.email_verified_at.is_none());
    }

    #[tokio::test]
    async fn security_email_identity_default_no_smtp_refuses_registration_and_address_change() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        persistence.stored.lock().unwrap().email_verified_at = Some(chrono::Utc::now());
        let users = no_smtp_use_cases(persistence, NoSmtpPolicy::Deny);

        assert!(matches!(
            users
                .register("newuser", "newuser@example.com", &"password".into(), None)
                .await,
            Err(AppError::BadRequest(message)) if message.contains("SMTP")
        ));

        let saved = users
            .update_profile(
                id,
                ProfileUpdate {
                    username: "renamed",
                    email: &EmailAddress::from("new@example.com"),
                    avatar_url: None,
                },
            )
            .await
            .unwrap();
        assert!(matches!(saved.address, AddressSaveOutcome::Unavailable));
        assert_eq!(saved.user.username, "renamed");
        assert_eq!(saved.user.email, "testuser@gmail.com");
        assert!(saved.user.email_verified_at.is_some());
    }

    #[tokio::test]
    async fn security_email_identity_stale_profile_read_preserves_concurrent_confirmation() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        *persistence.confirmed_address_after_read.lock().unwrap() =
            Some("confirmed@example.com".into());
        let users = no_smtp_use_cases(persistence, NoSmtpPolicy::Deny);
        let saved = users
            .update_profile(
                id,
                ProfileUpdate {
                    username: "renamed",
                    email: &EmailAddress::from("testuser@gmail.com"),
                    avatar_url: None,
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            saved.address,
            AddressSaveOutcome::CurrentAddressChanged
        ));
        assert_eq!(saved.user.username, "renamed");
        assert_eq!(saved.user.email, "confirmed@example.com");
        assert!(saved.user.email_verified_at.is_some());
    }

    #[tokio::test]
    async fn security_email_identity_bypass_address_change_clears_old_proof() {
        let persistence = Arc::new(MockUserPersistence::new());
        let id = persistence.id();
        persistence.stored.lock().unwrap().email_verified_at = Some(chrono::Utc::now());
        let users = no_smtp_use_cases(persistence, NoSmtpPolicy::AllowUnverified);
        let saved = users
            .update_profile(
                id,
                ProfileUpdate {
                    username: "renamed",
                    email: &EmailAddress::from("new@example.com"),
                    avatar_url: None,
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            saved.address,
            AddressSaveOutcome::InstalledUnverified
        ));
        assert_eq!(saved.user.email, "new@example.com");
        assert!(saved.user.email_verified_at.is_none());
    }

    #[tokio::test]
    async fn an_oauth_only_account_can_add_password_login() {
        let persistence = Arc::new(MockUserPersistence::new());
        persistence
            .password_enabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let id = persistence.id();
        let user_use_cases = use_cases(persistence.clone());

        let result = user_use_cases
            .set_password(
                id,
                PasswordSetup {
                    new: &"a-new-password".into(),
                    confirmation: &"a-new-password".into(),
                },
                None,
            )
            .await
            .expect("set password");

        assert!(matches!(result, PasswordOutcome::Changed));
        assert!(
            persistence
                .has_login_method(id, LoginMethod::Password)
                .await
                .unwrap()
        );
        assert_eq!(
            persistence.stored.lock().unwrap().password_hash,
            "a-new-password_hash"
        );
    }
}
