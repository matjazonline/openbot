//! Registration and confirmation-code issue paths.
use super::*;
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
    pub transport_available: bool,
    pub allow_unverified_without_smtp: bool,
}

impl EmailConfirmation {
    /// Mails a fresh code for `change` and records what it will write, returning the request as
    /// the page has to show it.
    ///
    /// The mail is sent *after* the request is stored, so a code that reaches somebody's inbox is
    /// always one this deployment can still honour.
    pub(super) async fn request(
        &self,
        signing_key: &AuthAttemptSecret,
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
                    signing_key,
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

impl UserUseCases {
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
        let password_hash = self
            .hash_password(password.expose_secret(), work_lifetime)
            .await?;
        let code = format!("{:06}", confirmation_number());
        let canonical_email = self
            .persistence
            .canonical_auth_identifier(email.as_str())
            .await?;
        let code_hash = confirmation_code_hash(&self.attempt_secret, &canonical_email, &code);
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
            self.attempt_secret.expose_secret(),
            "registration-code:v1",
            &[normalized.as_bytes()],
        );
        confirmation
            .registrations
            .confirm_pending_registration(
                &email,
                &confirmation_code_hash(&self.attempt_secret, &normalized, code),
                &attempt_key,
            )
            .await?
            .ok_or_else(|| AppError::BadRequest("Invalid or expired confirmation code".into()))
    }
}
