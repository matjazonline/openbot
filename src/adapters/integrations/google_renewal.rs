//! Google's reusable refresh protocol, distinct from strict rotating providers.
use super::{
    google_authorization::opaque_token,
    google_credentials::{GoogleCredentialAdapter, canonical_scopes},
    google_http::{GoogleHttp, GoogleHttpError},
};
use crate::{
    application::integrations::{
        private::{CredentialAdapter, CredentialCacheUpdate},
        renewal::*,
    },
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::sync::Arc;

pub(crate) struct GoogleRenewal {
    pub credentials: Arc<GoogleCredentialAdapter>,
    pub client_secret: SecretString,
    pub http: Arc<GoogleHttp>,
}
#[derive(Deserialize)]
struct Token {
    access_token: SecretString,
    refresh_token: Option<SecretString>,
    expires_in: i64,
    token_type: String,
    scope: Option<String>,
}
#[async_trait]
impl RenewalProvider for GoogleRenewal {
    fn session_policy(&self) -> SessionPolicy {
        SessionPolicy::RebuildOnGeneration
    }
    fn revocation_scope(&self) -> RevocationScope {
        RevocationScope::CombinedProject
    }
    fn assess(
        &self,
        raw: &SecretString,
        generation: CredentialGeneration,
        now: DateTime<Utc>,
    ) -> ResolutionResult<RenewalDecision> {
        let p = self.credentials.decode(raw)?;
        if p.generation != generation {
            return Err(IntegrationError::Credential.into());
        }
        if let Some(retry) = p.refresh_retry_at.filter(|r| *r > now) {
            if retry > now + chrono::Duration::seconds(60) {
                return Err(IntegrationError::Credential.into());
            }
            return Ok(RenewalDecision::Cooldown(retry));
        }
        if let Some(expires_at) = p
            .expires_at
            .filter(|e| *e >= now + chrono::Duration::seconds(60))
        {
            return Ok(RenewalDecision::Cached { expires_at });
        }
        Ok(RenewalDecision::Request)
    }
    fn cooldown(&self, raw: &SecretString, now: DateTime<Utc>) -> IntegrationResult<SecretString> {
        let mut p = self.credentials.decode(raw)?;
        p.refresh_retry_at = Some(now + chrono::Duration::seconds(30));
        p.encode()
    }
    async fn renew(
        &self,
        raw: &SecretString,
        next: CredentialGeneration,
        work: &RenewalWork,
    ) -> Result<SecretString, RenewalFailure> {
        let mut p = self
            .credentials
            .decode(raw)
            .map_err(|_| RenewalFailure::Malformed)?;
        if p.generation.next().ok() != Some(next) {
            return Err(RenewalFailure::Malformed);
        }
        let started = Utc::now();
        let response = self
            .http
            .token(
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", self.credentials.client.as_str()),
                    ("client_secret", self.client_secret.expose_secret()),
                    ("refresh_token", p.refresh_token.expose_secret()),
                ],
                work.deadline,
                &work.cancellation,
            )
            .await
            .map_err(http_error)?;
        if !(200..300).contains(&response.status) {
            return Err(provider_error(response.status, &response.body));
        }
        let token: Token = serde_json::from_str(response.body.expose_secret())
            .map_err(|_| RenewalFailure::Malformed)?;
        if !token.token_type.eq_ignore_ascii_case("bearer")
            || !opaque_token(&token.access_token)
            || token
                .refresh_token
                .as_ref()
                .is_some_and(|t| !opaque_token(t))
            || !(61..=86400).contains(&token.expires_in)
        {
            return Err(RenewalFailure::Malformed);
        }
        let expires = started + chrono::Duration::seconds(token.expires_in);
        if expires < Utc::now() + chrono::Duration::seconds(60) {
            return Err(RenewalFailure::Malformed);
        }
        if let Some(scope) = token.scope {
            p.scopes = canonical_scopes(
                scope
                    .split_whitespace()
                    .map(Scope::parse)
                    .collect::<IntegrationResult<_>>()
                    .map_err(|_| RenewalFailure::Malformed)?,
            )
            .map_err(|_| RenewalFailure::Malformed)?;
        }
        p.access_token = Some(token.access_token);
        p.expires_at = Some(expires);
        p.generation = next;
        p.refresh_retry_at = None;
        if let Some(refresh) = token.refresh_token {
            p.refresh_token = refresh;
        }
        let replacement = p.encode().map_err(|_| RenewalFailure::Malformed)?;
        self.credentials
            .validate_cache_update(CredentialCacheUpdate {
                previous: raw,
                replacement: &replacement,
            })
            .map_err(|_| RenewalFailure::Reconnect)?;
        Ok(replacement)
    }
}
fn http_error(error: GoogleHttpError) -> RenewalFailure {
    match error {
        GoogleHttpError::Cancelled => RenewalFailure::Cancelled,
        GoogleHttpError::Unavailable => RenewalFailure::Temporary,
        GoogleHttpError::Malformed => RenewalFailure::Malformed,
    }
}
fn provider_error(status: u16, raw: &SecretString) -> RenewalFailure {
    let error = serde_json::from_str::<serde_json::Value>(raw.expose_secret()).ok();
    match error
        .as_ref()
        .and_then(|v| v.get("error"))
        .and_then(|v| v.as_str())
    {
        Some("invalid_grant") => RenewalFailure::Reconnect,
        Some("invalid_client" | "unauthorized_client") => RenewalFailure::Configuration,
        _ if status == 429 || (500..600).contains(&status) => RenewalFailure::Temporary,
        _ => RenewalFailure::Malformed,
    }
}
