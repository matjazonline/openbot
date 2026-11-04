//! Profile and credential changes.
use super::*;
impl UserUseCases {
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
            .request(
                &self.attempt_secret,
                id,
                AccountChange::Email(&email),
                &email,
            )
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
            .request(
                &self.attempt_secret,
                id,
                AccountChange::Password(&hash),
                &account_email,
            )
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
            .request(
                &self.attempt_secret,
                id,
                AccountChange::Password(&hash),
                &account_email,
            )
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
            self.attempt_secret.expose_secret(),
            "account-change-code:v1",
            &[id.as_bytes(), kind.as_str().as_bytes()],
        );

        confirmation
            .account_changes
            .confirm_pending_account_change(
                id,
                kind,
                &account_change_code_hash(&self.attempt_secret, id, kind, code),
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
    pub(super) async fn account(&self, id: Uuid) -> AppResult<User> {
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
