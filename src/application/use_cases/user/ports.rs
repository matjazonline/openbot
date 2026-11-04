//! User/account persistence and request contracts.
use super::AuthAttemptKey;
use crate::{
    app_error::{AppError, AppResult},
    entities::{
        notification::NotificationPreferences,
        user::User,
        value_objects::{AvatarUrl, EmailAddress},
    },
};
use async_trait::async_trait;
use secrecy::SecretString;
use uuid::Uuid;
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

    /// Store the first password of an account that had none, recording password as one of its
    /// login methods in the same write. `Ok(None)` means no such user. Required: a default that
    /// only stored the hash would leave the login-method record behind it.
    async fn activate_password(&self, id: Uuid, password_hash: &str) -> AppResult<Option<User>>;

    /// Whether the account can sign in with `method`. Required: it gates credential changes, and a
    /// double must say what it answers rather than inherit "password, always".
    async fn has_login_method(&self, id: Uuid, method: LoginMethod) -> AppResult<bool>;

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
