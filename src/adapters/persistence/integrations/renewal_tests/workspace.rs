//! Non-Google, workspace identity and strict rotating refresh protocol fixture.
use super::server::TokenServer;
use crate::{
    application::integrations::{private::*, registry::*, renewal::*},
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    workspace: ExternalSubject,
    client: ExternalSubject,
    permissions: Vec<Capability>,
    token: SecretString,
    refresh: SecretString,
    expires_at: DateTime<Utc>,
    generation: CredentialGeneration,
}
fn decode(raw: &SecretString) -> IntegrationResult<Payload> {
    if raw.expose_secret().len() > MAX_CREDENTIAL_BYTES {
        return Err(IntegrationError::Credential);
    }
    let p: Payload =
        serde_json::from_str(raw.expose_secret()).map_err(|_| IntegrationError::Credential)?;
    if p.client.as_str() != "strict-client"
        || p.permissions.len() > MAX_SCOPES
        || p.refresh.expose_secret().is_empty()
        || p.token.expose_secret().is_empty()
    {
        return Err(IntegrationError::Credential);
    }
    Ok(p)
}
fn encode(p: &Payload) -> IntegrationResult<SecretString> {
    let text=json!({"workspace":p.workspace,"client":p.client,"permissions":p.permissions,"token":p.token.expose_secret(),"refresh":p.refresh.expose_secret(),"expires_at":p.expires_at,"generation":p.generation}).to_string();
    if text.len() > MAX_CREDENTIAL_BYTES {
        return Err(IntegrationError::Credential);
    }
    Ok(SecretString::from(text))
}
use serde_json::json;
struct Adapter;
impl CredentialAdapter for Adapter {
    fn policy_identity(&self, _: &AppDefinition) -> IntegrationResult<String> {
        Ok("workspace-strict-bearer/v1".into())
    }
    fn validate_use(&self, raw: &SecretString, app: &AppDefinition) -> IntegrationResult<()> {
        let p = decode(raw)?;
        if app.capabilities.iter().any(|c| !p.permissions.contains(c)) {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
    fn validate(&self, raw: &SecretString) -> IntegrationResult<Option<ExternalIdentity>> {
        let p = decode(raw)?;
        let key = format!(
            "{:x}",
            Sha256::digest(format!("{}/{}", p.workspace.as_str(), p.client.as_str()))
        );
        Ok(Some(ExternalIdentity {
            kind: ExternalIdentityKind::parse("workspace-installation")?,
            subject: p.workspace,
            authorization_key: AuthorizationKey::parse(key)?,
        }))
    }
    fn validate_cache_update(&self, u: CredentialCacheUpdate<'_>) -> IntegrationResult<()> {
        let a = decode(u.previous)?;
        let b = decode(u.replacement)?;
        if a.workspace != b.workspace
            || a.client != b.client
            || a.permissions.into_iter().collect::<BTreeSet<_>>()
                != b.permissions.into_iter().collect::<BTreeSet<_>>()
        {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
    fn injection(&self, raw: &SecretString) -> IntegrationResult<TypedInjection> {
        Ok(TypedInjection {
            method: InjectionMethod::Bearer,
            value: decode(raw)?.token,
        })
    }
}
struct Strategy {
    client: reqwest::Client,
    endpoint: String,
}
#[derive(Deserialize)]
struct Reply {
    workspace: ExternalSubject,
    permissions: Vec<Capability>,
    access_token: SecretString,
    refresh_token: SecretString,
    expires_in: i64,
    token_type: String,
}
#[async_trait]
impl RenewalProvider for Strategy {
    fn session_policy(&self) -> SessionPolicy {
        SessionPolicy::RebuildOnGeneration
    }
    fn revocation_scope(&self) -> RevocationScope {
        RevocationScope::Installation
    }
    fn assess(
        &self,
        raw: &SecretString,
        generation: CredentialGeneration,
        now: DateTime<Utc>,
    ) -> ResolutionResult<RenewalDecision> {
        let p = decode(raw)?;
        if p.generation != generation {
            return Err(IntegrationError::Credential.into());
        }
        if p.expires_at >= now + chrono::Duration::seconds(60) {
            Ok(RenewalDecision::Cached {
                expires_at: p.expires_at,
            })
        } else {
            Ok(RenewalDecision::Request)
        }
    }
    fn cooldown(&self, _: &SecretString, _: DateTime<Utc>) -> IntegrationResult<SecretString> {
        Err(IntegrationError::Denied)
    }
    async fn renew(
        &self,
        raw: &SecretString,
        next: CredentialGeneration,
        work: &RenewalWork,
    ) -> Result<SecretString, RenewalFailure> {
        let mut p = decode(raw).map_err(|_| RenewalFailure::Malformed)?;
        if p.generation.next().ok() != Some(next) {
            return Err(RenewalFailure::Malformed);
        }
        let started = Utc::now();
        let request = self.client.post(&self.endpoint).form(&[
            ("grant_type", "refresh_token"),
            ("client_id", p.client.as_str()),
            ("refresh_token", p.refresh.expose_secret()),
        ]);
        let response = tokio::select! {biased;_=work.cancellation.cancelled()=>return Err(RenewalFailure::Cancelled),result=request.send()=>result.map_err(|_|RenewalFailure::Temporary)?};
        if !response.status().is_success() {
            return Err(RenewalFailure::Temporary);
        }
        if response
            .content_length()
            .is_some_and(|v| v > MAX_CREDENTIAL_BYTES as u64)
        {
            return Err(RenewalFailure::Malformed);
        }
        let body = read_body(response).await?;
        let r: Reply = serde_json::from_slice(&body).map_err(|_| RenewalFailure::Malformed)?;
        if r.workspace != p.workspace
            || r.permissions.into_iter().collect::<BTreeSet<_>>()
                != p.permissions.iter().cloned().collect::<BTreeSet<_>>()
        {
            return Err(RenewalFailure::Reconnect);
        }
        if next != p.generation.next().map_err(|_| RenewalFailure::Malformed)?
            || r.refresh_token.expose_secret().is_empty()
            || r.refresh_token.expose_secret() == p.refresh.expose_secret()
            || !r.token_type.eq_ignore_ascii_case("bearer")
            || !r
                .access_token
                .expose_secret()
                .bytes()
                .all(|b| b.is_ascii_graphic())
            || r.access_token.expose_secret().is_empty()
            || !(61..=86400).contains(&r.expires_in)
        {
            return Err(RenewalFailure::Malformed);
        }
        p.token = r.access_token;
        p.refresh = r.refresh_token;
        p.expires_at = started + chrono::Duration::seconds(r.expires_in);
        p.generation = next;
        if p.expires_at < Utc::now() + chrono::Duration::seconds(60) {
            return Err(RenewalFailure::Malformed);
        }
        encode(&p).map_err(|_| RenewalFailure::Malformed)
    }
}
pub(crate) fn registration(server: &TokenServer) -> Registration {
    let provider = ProviderId::parse("workspace-fixture").unwrap();
    let config = AuthConfigId::parse("workspace-strict").unwrap();
    Registration {
        provider: AuthProvider {
            id: provider.clone(),
            method: AuthMethod::OAuthAuthorizationCode,
            schema: CredentialSchema::parse("workspace-rotating-fixture-v1").unwrap(),
        },
        config: AuthConfig {
            id: config.clone(),
            provider: provider.clone(),
            client_identity: ExternalSubject::parse("strict-client").unwrap(),
            client_secret_reference: None,
        },
        app: AppDefinition {
            id: AppId::parse("workspace-strict").unwrap(),
            provider,
            config,
            destinations: vec![],
            capabilities: vec![Capability::parse("resource.read").unwrap()],
        },
        adapter: Arc::new(Adapter),
        authorization: None,
        renewal: RenewalStrategy::StrictRotating(Arc::new(Strategy {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
            endpoint: server.url.clone(),
        })),
    }
}
pub(crate) fn payload() -> SecretString {
    SecretString::from(json!({"workspace":"no-email-workspace","client":"strict-client","permissions":["resource.read"],"token":"expired-workspace-token","refresh":"single-use-refresh","expires_at":Utc::now()-chrono::Duration::seconds(1),"generation":1}).to_string())
}
pub(crate) fn response() -> serde_json::Value {
    json!({"workspace":"no-email-workspace","permissions":["resource.read"],"access_token":"workspace-renewed-token","refresh_token":"replacement-refresh","expires_in":3600,"token_type":"Bearer"})
}

async fn read_body(mut response: reqwest::Response) -> Result<Vec<u8>, RenewalFailure> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| RenewalFailure::Temporary)?
    {
        if body.len() + chunk.len() > MAX_CREDENTIAL_BYTES {
            return Err(RenewalFailure::Malformed);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
