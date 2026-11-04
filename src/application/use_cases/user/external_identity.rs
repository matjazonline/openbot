//! Verified external identity operations.
use super::*;
impl UserUseCases {
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
}
fn require_verified_external_identity(identity: &ExternalIdentity<'_>) -> AppResult<()> {
    if identity.provider == LoginMethod::Password || !identity.email_verified {
        return Err(AppError::BadRequest(
            "An external identity must carry a verified provider email claim.".into(),
        ));
    }
    Ok(())
}
