//! The `company_resend_api_integrations` row: one company's Resend account.
//!
//! This is the only module that names the two encrypted columns. The settings projection selects
//! neither of them and never will -- that is what makes "the page cannot leak a key" a property of
//! this file rather than a habit spread across the callers.
//!
//! Every statement names `company_id`, and every owner-scoped one joins `companies.user_id` in the
//! same statement rather than checking ownership in a separate round trip: a company id arrives
//! from a URL, and a query that trusts it alone is one guessed UUID away from writing another
//! tenant's credentials.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use tracing::warn;
use uuid::Uuid;

use crate::{
    adapters::persistence::{PostgresPersistence, credentials::CredentialContext},
    app_error::{AppError, AppResult},
    entities::{
        company_resend_api::{
            CompanyResendApiIntegration, CompanyResendApiIntegrationWrite,
            ResendApiAccountCredentials, ResendApiCredentialKind, ResendApiInboundCredentials,
        },
        value_objects::{AuthservId, ResendApiWebhookToken},
    },
    use_cases::company_resend_api::{CompanyResendApiAccounts, CompanyResendApiIntegrationStore},
};

/// The settings projection. No credential column appears here, and none may be added: this list
/// feeds every view of the integration the product renders.
const INTEGRATION_COLUMNS: &str = "\
    integration.company_id, integration.webhook_token, integration.authserv_id, \
    integration.enabled, integration.created_at, integration.updated_at";

#[derive(sqlx::FromRow)]
struct IntegrationRow {
    company_id: Uuid,
    webhook_token: String,
    authserv_id: String,
    enabled: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<IntegrationRow> for CompanyResendApiIntegration {
    fn from(row: IntegrationRow) -> Self {
        Self {
            company_id: row.company_id,
            webhook_token: ResendApiWebhookToken::from(row.webhook_token),
            authserv_id: AuthservId::from(row.authserv_id),
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// What a company already has stored, read under the same lock the write commits through.
struct StoredSecrets {
    webhook_token: String,
    api_key: Option<String>,
    signing_secret: Option<String>,
}

#[async_trait]
impl CompanyResendApiAccounts for PostgresPersistence {
    async fn inbound_credentials(
        &self,
        token: &ResendApiWebhookToken,
    ) -> AppResult<Option<ResendApiInboundCredentials>> {
        let cipher = self.credential_cipher()?;
        let row: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT company_id, signing_secret FROM company_resend_api_integrations \
             WHERE webhook_token = $1 AND enabled",
        )
        .bind(token.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        let Some((company_id, envelope)) = row else {
            return Ok(None);
        };
        // An envelope that will not open is an error rather than "this company has no secret":
        // answering the latter would turn a corrupted row into a 404 on a live endpoint, and the
        // operator would go looking at Resend for a problem that is in this database.
        let signing_secret = cipher
            .open_envelope(
                &context(company_id, ResendApiCredentialKind::SigningSecret),
                &envelope,
            )
            .inspect_err(|error| {
                warn!(
                    %company_id,
                    credential_kind = ResendApiCredentialKind::SigningSecret.as_str(),
                    failure = %error,
                    "stored Resend credential failed to open"
                );
            })?;
        Ok(Some(ResendApiInboundCredentials {
            company_id,
            signing_secret,
        }))
    }

    async fn account_credentials(
        &self,
        company_id: Uuid,
    ) -> AppResult<Option<ResendApiAccountCredentials>> {
        let cipher = self.credential_cipher()?;
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT api_key, authserv_id FROM company_resend_api_integrations \
             WHERE company_id = $1 AND enabled",
        )
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        let Some((envelope, authserv_id)) = row else {
            return Ok(None);
        };
        let api_key = cipher
            .open_envelope(
                &context(company_id, ResendApiCredentialKind::ApiKey),
                &envelope,
            )
            .inspect_err(|error| {
                warn!(
                    %company_id,
                    credential_kind = ResendApiCredentialKind::ApiKey.as_str(),
                    failure = %error,
                    "stored Resend credential failed to open"
                );
            })?;
        Ok(Some(ResendApiAccountCredentials {
            api_key,
            authserv_id: AuthservId::from(authserv_id),
        }))
    }
}

#[async_trait]
impl CompanyResendApiIntegrationStore for PostgresPersistence {
    async fn integration_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyResendApiIntegration>> {
        let row: Option<IntegrationRow> = sqlx::query_as(&format!(
            "SELECT {INTEGRATION_COLUMNS} \
             FROM company_resend_api_integrations AS integration \
             JOIN companies AS company ON company.id = integration.company_id \
             WHERE integration.company_id = $1 AND company.user_id = $2"
        ))
        .bind(company_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(row.map(Into::into))
    }

    async fn upsert_integration_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        write: CompanyResendApiIntegrationWrite,
    ) -> AppResult<CompanyResendApiIntegration> {
        let cipher = self.credential_cipher()?;
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;

        // The ownership check and the row it guards are taken under one lock, so two saves racing
        // on the same company serialize here rather than one of them minting a second token.
        let owned = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM companies WHERE id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(company_id)
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        if owned.is_none() {
            return Err(crate::use_cases::company::company_not_found());
        }

        let stored: Option<(String, String, String)> = sqlx::query_as(
            "SELECT webhook_token, api_key, signing_secret FROM company_resend_api_integrations \
             WHERE company_id = $1 FOR UPDATE",
        )
        .bind(company_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        let stored = stored.map_or(
            StoredSecrets {
                // The token is minted here rather than by a default, so the value the settings
                // page shows and the value the webhook route matches come from one place.
                webhook_token: ResendApiWebhookToken::generate().into_string(),
                api_key: None,
                signing_secret: None,
            },
            |(webhook_token, api_key, signing_secret)| StoredSecrets {
                webhook_token,
                api_key: Some(api_key),
                signing_secret: Some(signing_secret),
            },
        );

        let api_key = envelope_for(
            cipher,
            company_id,
            ResendApiCredentialKind::ApiKey,
            write.api_key.as_ref(),
            stored.api_key,
            "A Resend API key is required to connect this company.",
        )?;
        let signing_secret = envelope_for(
            cipher,
            company_id,
            ResendApiCredentialKind::SigningSecret,
            write.signing_secret.as_ref(),
            stored.signing_secret,
            "A webhook signing secret is required to connect this company.",
        )?;

        let row: IntegrationRow = sqlx::query_as(&format!(
            // Aliased so one column list serves every statement in this module, `RETURNING`
            // included -- two lists is how the settings projection grows a credential column.
            "INSERT INTO company_resend_api_integrations AS integration \
                 (company_id, webhook_token, api_key, signing_secret, authserv_id, enabled) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (company_id) DO UPDATE \
             SET api_key = EXCLUDED.api_key, signing_secret = EXCLUDED.signing_secret, \
                 authserv_id = EXCLUDED.authserv_id, enabled = EXCLUDED.enabled, \
                 updated_at = CURRENT_TIMESTAMP \
             RETURNING {INTEGRATION_COLUMNS}"
        ))
        .bind(company_id)
        .bind(&stored.webhook_token)
        .bind(&api_key)
        .bind(&signing_secret)
        .bind(write.authserv_id.as_str())
        .bind(write.enabled)
        .fetch_one(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        transaction.commit().await.map_err(AppError::from)?;
        Ok(row.into())
    }

    async fn rotate_webhook_token_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<CompanyResendApiIntegration> {
        let row: Option<IntegrationRow> = sqlx::query_as(&format!(
            "UPDATE company_resend_api_integrations AS integration \
             SET webhook_token = $3, updated_at = CURRENT_TIMESTAMP \
             FROM companies AS company \
             WHERE integration.company_id = $1 \
               AND company.id = integration.company_id AND company.user_id = $2 \
             RETURNING {INTEGRATION_COLUMNS}"
        ))
        .bind(company_id)
        .bind(user_id)
        .bind(ResendApiWebhookToken::generate().as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        row.map(Into::into).ok_or_else(|| {
            AppError::NotFound("This company has no Resend integration to rotate.".into())
        })
    }

    async fn delete_integration_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<bool> {
        let deleted = sqlx::query(
            "DELETE FROM company_resend_api_integrations AS integration \
             USING companies AS company \
             WHERE integration.company_id = $1 \
               AND company.id = integration.company_id AND company.user_id = $2",
        )
        .bind(company_id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?
        .rows_affected();
        Ok(deleted > 0)
    }
}

/// The envelope one column is written with: the submitted secret sealed, or the stored envelope
/// kept untouched.
///
/// Keeping the stored envelope rather than resealing it means a save that changes only the
/// `authserv-id` never decrypts a credential at all.
fn envelope_for(
    cipher: &crate::adapters::persistence::credentials::CredentialCipher,
    company_id: Uuid,
    kind: ResendApiCredentialKind,
    submitted: Option<&SecretString>,
    stored: Option<String>,
    missing: &str,
) -> AppResult<String> {
    match submitted {
        Some(secret) => cipher.seal_envelope(&context(company_id, kind), secret),
        None => stored.ok_or_else(|| AppError::BadRequest(missing.to_string())),
    }
}

/// The authenticated context for one credential column, built in exactly one place so a reader and
/// a writer cannot disagree about what the tag covers.
fn context(company_id: Uuid, kind: ResendApiCredentialKind) -> CredentialContext {
    CredentialContext::company_resend_api_credential(company_id, kind)
}

#[cfg(test)]
#[path = "company_resend_api_tests.rs"]
mod tests;
