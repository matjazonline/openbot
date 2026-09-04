//! Configuring and reading one company's Resend account.
//!
//! Two audiences, and the split between them is the whole design. A settings page asks
//! [`CompanyResendApiUseCases`] and is answered with [`CompanyResendApiIntegration`], which holds no
//! secret and is scoped to the owner of the company. The runtime -- the webhook route, the
//! inbound decoder, the delivery worker -- asks [`CompanyResendApiAccounts`] and is answered with
//! credentials, by a lookup that names one company and returns nothing else about it.
//!
//! Nothing here falls back to a deployment-wide credential when a company has none configured.
//! That is deliberate: a shared key is a key with which one tenant's mail can be fetched under
//! another tenant's account, and "no integration" has to stay distinguishable from "not yet
//! looked up".

use std::sync::Arc;

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company_resend_api::{
            CompanyResendApiIntegration, CompanyResendApiIntegrationWrite,
            ResendApiAccountCredentials, ResendApiInboundCredentials, decode_signing_secret,
        },
        value_objects::{AuthservId, ResendApiWebhookToken},
    },
};

/// The longest API key and signing secret this application will store.
///
/// Resend's own credentials are far shorter; this is the boundary that stops a settings form from
/// putting an arbitrary blob through envelope encryption and into a row.
pub const MAX_RESEND_API_CREDENTIAL_BYTES: usize = 512;

/// What the runtime needs: credentials by webhook token, and credentials by company.
///
/// Deliberately not the same trait as the owner-scoped writes below. The decoder, the delivery
/// worker and the webhook route are handed exactly these two lookups, so no background task holds
/// a port through which a company's integration could be changed.
#[async_trait]
pub trait CompanyResendApiAccounts: Send + Sync {
    /// The tenant one webhook request belongs to, and the secret it must be proved against.
    ///
    /// A row that is switched off answers `None`: the endpoint stops existing when the
    /// integration is disabled, rather than accepting mail nothing downstream will send for.
    async fn inbound_credentials(
        &self,
        token: &ResendApiWebhookToken,
    ) -> AppResult<Option<ResendApiInboundCredentials>>;

    /// The credentials to act as one company at Resend. `None` when it has no integration or has
    /// switched it off.
    async fn account_credentials(
        &self,
        company_id: Uuid,
    ) -> AppResult<Option<ResendApiAccountCredentials>>;
}

/// One company's Resend row, as its owner changes it.
///
/// Every method that returns a credential lives on [`CompanyResendApiAccounts`] instead, so no
/// projection here can accidentally widen into carrying a secret: the narrow readers there are
/// the only statements in the codebase that read the two encrypted columns back.
#[async_trait]
pub trait CompanyResendApiIntegrationStore: Send + Sync {
    /// The settings view for a company its owner is looking at. `None` when nothing is configured.
    async fn integration_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyResendApiIntegration>>;

    /// Store the company's integration, minting a webhook token on the first write.
    ///
    /// A write that omits a secret keeps the stored one, so an update is refused rather than
    /// half-applied when there is nothing stored to keep.
    async fn upsert_integration_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        write: CompanyResendApiIntegrationWrite,
    ) -> AppResult<CompanyResendApiIntegration>;

    /// Issue a fresh webhook token, invalidating the URL registered at the provider.
    async fn rotate_webhook_token_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<CompanyResendApiIntegration>;

    /// Forget the integration and both of its secrets. `false` when there was none.
    async fn delete_integration_for_user(&self, user_id: Uuid, company_id: Uuid)
    -> AppResult<bool>;
}

/// What a settings form submitted, before it is anything the store will accept.
///
/// Parsing lives here rather than at the HTTP boundary because the same rules decide whether the
/// value is storable at all: a blank secret means "keep", an oversized one is refused, and an
/// `authserv-id` with a space in it could never match a header.
#[derive(Debug)]
pub struct SubmittedResendApiIntegration {
    pub api_key: Option<String>,
    pub signing_secret: Option<String>,
    pub authserv_id: String,
    pub enabled: bool,
}

impl SubmittedResendApiIntegration {
    /// The write this submission means, or the refusal to show above the form.
    pub fn parse(self) -> AppResult<CompanyResendApiIntegrationWrite> {
        Ok(CompanyResendApiIntegrationWrite {
            api_key: parse_secret(self.api_key, "API key")?,
            signing_secret: parse_secret(self.signing_secret, "webhook signing secret")?,
            authserv_id: AuthservId::parse(&self.authserv_id).map_err(AppError::BadRequest)?,
            enabled: self.enabled,
        })
    }
}

/// A submitted secret: blank means keep the stored one, and anything oversized is refused here
/// rather than by a constraint violation two layers down.
fn parse_secret(value: Option<String>, label: &str) -> AppResult<Option<SecretString>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > MAX_RESEND_API_CREDENTIAL_BYTES {
        return Err(AppError::BadRequest(format!(
            "The Resend {label} may be at most {MAX_RESEND_API_CREDENTIAL_BYTES} characters."
        )));
    }
    Ok(Some(SecretString::from(trimmed.to_string())))
}

/// The owner-scoped operations a settings page performs.
pub struct CompanyResendApiUseCases {
    store: Arc<dyn CompanyResendApiIntegrationStore>,
}

impl CompanyResendApiUseCases {
    pub fn new(store: Arc<dyn CompanyResendApiIntegrationStore>) -> Self {
        Self { store }
    }

    #[instrument(skip(self))]
    pub async fn integration(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyResendApiIntegration>> {
        self.store.integration_for_user(user_id, company_id).await
    }

    /// Save what the form submitted. A signing secret that is not a Svix one is refused before it
    /// is stored, so a mistyped credential is a message under the field rather than a webhook
    /// that answers 401 to every delivery and looks like a provider outage.
    #[instrument(skip(self, submitted))]
    pub async fn save(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        submitted: SubmittedResendApiIntegration,
    ) -> AppResult<CompanyResendApiIntegration> {
        let write = submitted.parse()?;
        if let Some(secret) = write.signing_secret.as_ref()
            && !is_svix_signing_secret(secret)
        {
            return Err(AppError::BadRequest(
                "The webhook signing secret is the base64 value Resend shows for the endpoint, \
                 usually starting with whsec_."
                    .into(),
            ));
        }
        let saved = self
            .store
            .upsert_integration_for_user(user_id, company_id, write)
            .await?;
        info!(%company_id, enabled = saved.enabled, "Saved a company's Resend integration");
        Ok(saved)
    }

    #[instrument(skip(self))]
    pub async fn rotate_webhook_token(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<CompanyResendApiIntegration> {
        let rotated = self
            .store
            .rotate_webhook_token_for_user(user_id, company_id)
            .await?;
        // The old URL stops working the moment this commits, so the operator has to re-register
        // the new one. Logged as the deliberate act it is, without the token itself.
        info!(%company_id, "Rotated a company's Resend webhook token");
        Ok(rotated)
    }

    #[instrument(skip(self))]
    pub async fn disconnect(&self, user_id: Uuid, company_id: Uuid) -> AppResult<bool> {
        let removed = self
            .store
            .delete_integration_for_user(user_id, company_id)
            .await?;
        if removed {
            info!(%company_id, "Removed a company's Resend integration");
        }
        Ok(removed)
    }
}

/// Whether a submitted signing secret is one Svix could have issued.
///
/// The same decode the signature check performs, run at the point of entry: a secret that cannot
/// be decoded verifies nothing, and finding that out at save time is the difference between a
/// message under a field and a silent inbound outage.
fn is_svix_signing_secret(secret: &SecretString) -> bool {
    decode_signing_secret(secret.expose_secret()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submitted(api_key: Option<&str>, authserv_id: &str) -> SubmittedResendApiIntegration {
        SubmittedResendApiIntegration {
            api_key: api_key.map(str::to_string),
            signing_secret: None,
            authserv_id: authserv_id.to_string(),
            enabled: true,
        }
    }

    #[test]
    fn a_blank_secret_means_keep_the_stored_one() {
        let write = submitted(Some("   "), "resend.com").parse().unwrap();
        assert!(write.api_key.is_none());
    }

    #[test]
    fn a_submitted_secret_is_trimmed_rather_than_stored_with_its_whitespace() {
        let write = submitted(Some("  re_key  "), "resend.com").parse().unwrap();
        assert_eq!(write.api_key.unwrap().expose_secret(), "re_key");
    }

    #[test]
    fn an_oversized_secret_is_refused_at_the_use_case_rather_than_by_a_constraint() {
        let error = submitted(
            Some(&"k".repeat(MAX_RESEND_API_CREDENTIAL_BYTES + 1)),
            "resend.com",
        )
        .parse()
        .unwrap_err();
        assert!(matches!(error, AppError::BadRequest(_)), "{error:?}");
    }

    #[test]
    fn an_authserv_id_with_a_space_is_refused() {
        let error = submitted(None, "resend.com is").parse().unwrap_err();
        assert!(matches!(error, AppError::BadRequest(_)), "{error:?}");
    }

    #[test]
    fn a_signing_secret_is_recognised_with_and_without_its_prefix() {
        assert!(is_svix_signing_secret(&SecretString::from(
            "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw".to_string()
        )));
        assert!(is_svix_signing_secret(&SecretString::from(
            "MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw".to_string()
        )));
        assert!(!is_svix_signing_secret(&SecretString::from(
            "whsec_not base64".to_string()
        )));
    }
}
