use super::{
    google_credentials::{
        GoogleCredentialAdapter, GooglePayload, VerifiedMailbox, required_scopes,
    },
    google_http::{GoogleHttp, failure},
};
use crate::{
    application::integrations::{connect::*, private::CredentialAdapter, *},
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::sync::Arc;
pub(crate) struct GoogleAuthorization {
    pub credentials: Arc<GoogleCredentialAdapter>,
    pub client_secret: SecretString,
    pub redirect: url::Url,
    pub http: Arc<GoogleHttp>,
}
#[derive(Deserialize)]
struct Token {
    access_token: SecretString,
    refresh_token: Option<SecretString>,
    token_type: String,
    expires_in: i64,
    scope: String,
}
#[derive(Deserialize)]
struct Profile {
    sub: ExternalSubject,
    email: VerifiedMailbox,
    email_verified: bool,
}
#[derive(Deserialize)]
struct Candidate {
    subject: ExternalSubject,
    mailbox: VerifiedMailbox,
    scopes: Vec<Scope>,
    access_token: SecretString,
    refresh_token: Option<SecretString>,
    expires_at: DateTime<Utc>,
}
pub(super) fn opaque_token(token: &SecretString) -> bool {
    let value = token.expose_secret();
    !value.is_empty()
        && value.len() <= MAX_CREDENTIAL_BYTES
        && value.bytes().all(|b| b.is_ascii_graphic())
}
#[async_trait]
impl AuthorizationProvider for GoogleAuthorization {
    fn callback_error(&self, error: &str) -> ConnectStatus {
        match error {
            "access_denied" => ConnectStatus::Denied,
            "admin_policy_enforced" | "access_blocked" => ConnectStatus::AdminBlocked,
            _ => ConnectStatus::ProviderError,
        }
    }

    fn authorization_url(
        &self,
        r: AuthorizationRequest<'_>,
    ) -> IntegrationResult<SanitizedConnectUrl> {
        let mut scopes = vec!["openid", "email"];
        scopes.extend(required_scopes(r.app)?);
        let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .map_err(|_| IntegrationError::Invalid)?;
        // Both paths are explicit owner consent to obtain a refresh grant.
        url.query_pairs_mut().extend_pairs([
            ("client_id", self.credentials.client.as_str()),
            ("redirect_uri", self.redirect.as_str()),
            ("response_type", "code"),
            ("scope", scopes.join(" ").as_str()),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", r.state),
            ("code_challenge", r.challenge),
            ("code_challenge_method", "S256"),
        ]);
        SanitizedConnectUrl::from_registered_endpoint(
            url.into(),
            "https://accounts.google.com/o/oauth2/v2/auth",
        )
    }
    async fn exchange(&self, r: ProviderExchange<'_>) -> Result<SecretString, ConnectStatus> {
        let started = Utc::now();
        let reply = self
            .http
            .token(
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", self.credentials.client.as_str()),
                    ("client_secret", self.client_secret.expose_secret()),
                    ("redirect_uri", self.redirect.as_str()),
                    ("code", r.code.expose_secret()),
                    ("code_verifier", r.verifier.expose_secret()),
                ],
                r.deadline,
                r.cancellation,
            )
            .await?;
        if !(200..300).contains(&reply.status) {
            return Err(failure(&reply));
        }
        let token: Token = serde_json::from_str(reply.body.expose_secret())
            .map_err(|_| ConnectStatus::ProviderError)?;
        if !token.token_type.eq_ignore_ascii_case("bearer")
            || !opaque_token(&token.access_token)
            || token
                .refresh_token
                .as_ref()
                .is_some_and(|t| !opaque_token(t))
            || !(61..=86400).contains(&token.expires_in)
        {
            return Err(ConnectStatus::ProviderError);
        }
        let expires_at = started + chrono::Duration::seconds(token.expires_in);
        let scopes = super::google_credentials::canonical_scopes(
            token
                .scope
                .split_whitespace()
                .map(Scope::parse)
                .collect::<IntegrationResult<_>>()
                .map_err(|_| ConnectStatus::MissingPermissions)?,
        )
        .map_err(|_| ConnectStatus::MissingPermissions)?;
        if scopes.len() > MAX_SCOPES
            || !scopes.iter().any(|s| s.as_str() == "openid")
            || !scopes.iter().any(|s| {
                matches!(
                    s.as_str(),
                    "email" | "https://www.googleapis.com/auth/userinfo.email"
                )
            })
            || required_scopes(r.app)
                .map_err(|_| ConnectStatus::MissingPermissions)?
                .iter()
                .any(|s| !scopes.iter().any(|g| g.as_str() == *s))
        {
            return Err(ConnectStatus::MissingPermissions);
        }
        let reply = self
            .http
            .identity(
                token.access_token.expose_secret(),
                r.deadline,
                r.cancellation,
            )
            .await?;
        if !(200..300).contains(&reply.status) {
            return Err(failure(&reply));
        }
        let profile: Profile = serde_json::from_str(reply.body.expose_secret())
            .map_err(|_| ConnectStatus::IdentityMismatch)?;
        if !profile.email_verified || expires_at < Utc::now() + chrono::Duration::seconds(60) {
            return Err(ConnectStatus::IdentityMismatch);
        }
        Ok(SecretString::from(serde_json::json!({"subject":profile.sub,"mailbox":profile.email.as_str(),"scopes":scopes,"access_token":token.access_token.expose_secret(),"refresh_token":token.refresh_token.as_ref().map(|s|s.expose_secret()),"expires_at":expires_at}).to_string()))
    }
    fn finalize(&self, r: FinalizeAuthorization<'_>) -> Result<AuthorizedAccount, ConnectStatus> {
        if r.candidate.expose_secret().len() > MAX_CREDENTIAL_BYTES {
            return Err(ConnectStatus::ProviderError);
        }
        let c: Candidate = serde_json::from_str(r.candidate.expose_secret())
            .map_err(|_| ConnectStatus::ProviderError)?;
        if r.previous
            .is_some_and(|p| p.versions.generation.next().ok() != Some(r.generation))
        {
            return Err(ConnectStatus::ProviderError);
        }
        let previous = r
            .previous
            .map(|p| self.credentials.decode(&p.payload))
            .transpose()
            .map_err(|_| ConnectStatus::IdentityMismatch)?;
        if previous
            .as_ref()
            .is_some_and(|p| p.subject != c.subject || p.client != self.credentials.client)
        {
            return Err(ConnectStatus::IdentityMismatch);
        }
        let refresh = match c.refresh_token {
            Some(value) => value,
            None => {
                if r.previous
                    .is_none_or(|p| p.account.status != AccountStatus::Ready)
                {
                    return Err(ConnectStatus::MissingRefresh);
                }
                previous.ok_or(ConnectStatus::MissingRefresh)?.refresh_token
            }
        };
        if c.expires_at < Utc::now() + chrono::Duration::seconds(60) {
            return Err(ConnectStatus::ProviderError);
        }
        let p = GooglePayload {
            subject: c.subject,
            client: self.credentials.client.clone(),
            mailbox: c.mailbox,
            scopes: c.scopes,
            refresh_token: refresh,
            access_token: Some(c.access_token),
            expires_at: Some(c.expires_at),
            generation: r.generation,
            refresh_retry_at: None,
        };
        let payload = p.encode().map_err(|_| ConnectStatus::ProviderError)?;
        self.credentials
            .validate_use(&payload, r.app)
            .map_err(|_| ConnectStatus::MissingPermissions)?;
        let identity = self
            .credentials
            .validate(&payload)
            .map_err(|_| ConnectStatus::IdentityMismatch)?
            .ok_or(ConnectStatus::IdentityMismatch)?;
        let metadata = AccountMetadata::new(
            p.mailbox.as_str().into(),
            Some(format!(
                "Granted capabilities: {}",
                r.app
                    .capabilities
                    .iter()
                    .map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        )
        .map_err(|_| ConnectStatus::ProviderError)?;
        Ok(AuthorizedAccount {
            identity,
            payload,
            metadata,
        })
    }
}

pub(crate) fn registration(
    client: ExternalSubject,
    secret: SecretString,
    redirect: url::Url,
    http: Arc<GoogleHttp>,
) -> IntegrationResult<crate::application::integrations::registry::Registration> {
    use crate::application::integrations::registry::Registration;
    use crate::domain::integrations::authority::ApprovedDestination;
    let provider = ProviderId::parse("google")?;
    let config = AuthConfigId::parse("google-personal")?;
    let credentials = Arc::new(GoogleCredentialAdapter { client });
    let authorization = Arc::new(GoogleAuthorization {
        credentials: credentials.clone(),
        client_secret: secret.clone(),
        redirect,
        http: http.clone(),
    });
    Ok(Registration {
        provider: AuthProvider {
            id: provider.clone(),
            method: AuthMethod::OAuthAuthorizationCode,
            schema: CredentialSchema::parse("google-v1")?,
        },
        config: AuthConfig {
            id: config.clone(),
            provider: provider.clone(),
            client_identity: credentials.client.clone(),
            client_secret_reference: Some(ClientSecretReference::parse(
                "google-oauth-client-secret",
            )?),
        },
        app: AppDefinition {
            id: AppId::parse("gmail")?,
            provider,
            config,
            destinations: vec![ApprovedDestination::parse("https://gmail.googleapis.com")?],
            transport:
                crate::application::integrations::destinations::AppTransportPolicy::HttpPaths {
                    prefixes: vec![
                        crate::application::integrations::destinations::ApiPathPrefix::parse(
                            "/gmail/v1/",
                        )?,
                    ],
                },
            capabilities: vec![
                Capability::parse("gmail.read")?,
                Capability::parse("gmail.send")?,
            ],
        },
        renewal: crate::application::integrations::renewal::RenewalStrategy::ReusableRefresh(
            Arc::new(super::google_renewal::GoogleRenewal {
                credentials: credentials.clone(),
                client_secret: secret,
                http,
            }),
        ),
        adapter: credentials,
        authorization: Some(authorization),
    })
}
