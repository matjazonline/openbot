//! Test-only OAuth extension: installation identity, custom scope vocabulary, strict refresh.
use super::{server::TokenServer, workspace};
use crate::{
    application::integrations::{
        connect::*, gateway::*, private::CredentialAdapter, registry::Registration,
    },
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

const AUTHORIZE: &str = "https://workspace.example/authorize";
const REDIRECT: &str = "https://openbot.example/auth/integrations/workspace-fixture/callback";
struct Authorization {
    adapter: Arc<dyn CredentialAdapter>,
    client: ExternalSubject,
    http: reqwest::Client,
    endpoint: String,
}
pub(crate) fn with_authorization(
    mut registration: Registration,
    server: &TokenServer,
) -> Registration {
    registration.authorization = Some(Arc::new(Authorization {
        adapter: registration.adapter.clone(),
        client: registration.config.client_identity.clone(),
        http: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap(),
        endpoint: server.url.clone(),
    }));
    registration
}
fn scope(capability: &Capability) -> IntegrationResult<&'static str> {
    match capability.as_str() {
        "resource.read" => Ok("workspace:inspect"),
        "resource.write" => Ok("workspace:mutate"),
        _ => Err(IntegrationError::Denied),
    }
}
fn permissions(scopes: &str) -> Result<Vec<Capability>, ConnectStatus> {
    let permissions = scopes
        .split_whitespace()
        .map(|scope| match scope {
            "workspace:inspect" => Capability::parse("resource.read"),
            "workspace:mutate" => Capability::parse("resource.write"),
            _ => Err(IntegrationError::Denied),
        })
        .collect::<IntegrationResult<std::collections::BTreeSet<_>>>()
        .map_err(|_| ConnectStatus::MissingPermissions)?;
    if permissions.len() > MAX_SCOPES {
        return Err(ConnectStatus::MissingPermissions);
    }
    Ok(permissions.into_iter().collect())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    workspace: ExternalSubject,
    scope: String,
    access_token: SecretString,
    refresh_token: Option<SecretString>,
    expires_in: i64,
    token_type: String,
}
impl Authorization {
    async fn fetch(&self, r: &ProviderExchange<'_>) -> Result<SecretString, ConnectStatus> {
        let started = Utc::now();
        let response = self
            .http
            .post(&self.endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", self.client.as_str()),
                ("code", r.code.expose_secret()),
                ("code_verifier", r.verifier.expose_secret()),
                ("redirect_uri", REDIRECT),
            ])
            .send()
            .await
            .map_err(|_| ConnectStatus::ProviderError)?;
        if !response.status().is_success() {
            return Err(ConnectStatus::ProviderError);
        }
        let bytes = workspace::read_body(response)
            .await
            .map_err(|_| ConnectStatus::ProviderError)?;
        let reply: Reply =
            serde_json::from_slice(&bytes).map_err(|_| ConnectStatus::ProviderError)?;
        let refresh = reply
            .refresh_token
            .filter(|value| !value.expose_secret().is_empty())
            .ok_or(ConnectStatus::MissingRefresh)?;
        if !(61..=86400).contains(&reply.expires_in)
            || !reply.token_type.eq_ignore_ascii_case("bearer")
            || reply.access_token.expose_secret().is_empty()
            || !reply
                .access_token
                .expose_secret()
                .bytes()
                .all(|b| b.is_ascii_graphic())
        {
            return Err(ConnectStatus::ProviderError);
        }
        Ok(SecretString::from(json!({"workspace":reply.workspace,"client":self.client,"permissions":permissions(&reply.scope)?,"token":reply.access_token.expose_secret(),"refresh":refresh.expose_secret(),"expires_at":started+chrono::Duration::seconds(reply.expires_in),"generation":1}).to_string()))
    }
}
#[async_trait]
impl AuthorizationProvider for Authorization {
    fn callback_error(&self, error: &str) -> ConnectStatus {
        if error == "access_denied" {
            ConnectStatus::Denied
        } else {
            ConnectStatus::ProviderError
        }
    }
    fn authorization_url(
        &self,
        r: AuthorizationRequest<'_>,
    ) -> IntegrationResult<SanitizedConnectUrl> {
        let scopes = r
            .app
            .capabilities
            .iter()
            .map(scope)
            .collect::<IntegrationResult<Vec<_>>>()?
            .join(" ");
        let mut url = url::Url::parse(AUTHORIZE).map_err(|_| IntegrationError::Invalid)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", self.client.as_str())
            .append_pair("redirect_uri", REDIRECT)
            .append_pair("scope", &scopes)
            .append_pair("state", r.state)
            .append_pair("code_challenge", r.challenge)
            .append_pair("code_challenge_method", "S256");
        SanitizedConnectUrl::from_registered_endpoint(url.into(), AUTHORIZE)
    }
    async fn exchange(&self, r: ProviderExchange<'_>) -> Result<SecretString, ConnectStatus> {
        let remaining = (r.deadline - Utc::now())
            .to_std()
            .map_err(|_| ConnectStatus::Expired)?;
        // Bound and cancel the actual HTTP/body work; do not detach a task holding a code.
        tokio::select! {biased;
            _=r.cancellation.cancelled()=>Err(ConnectStatus::Cancelled),
            result=tokio::time::timeout(remaining,Box::pin(self.fetch(&r)))=>result.map_err(|_|ConnectStatus::Expired)?,
        }
    }
    fn finalize(&self, r: FinalizeAuthorization<'_>) -> Result<AuthorizedAccount, ConnectStatus> {
        let mut candidate: serde_json::Value = serde_json::from_str(r.candidate.expose_secret())
            .map_err(|_| ConnectStatus::ProviderError)?;
        candidate["generation"] = json!(r.generation.get());
        let payload = SecretString::from(candidate.to_string());
        self.adapter
            .validate_use(&payload, r.app)
            .map_err(|_| ConnectStatus::MissingPermissions)?;
        let identity = self
            .adapter
            .validate(&payload)
            .map_err(|_| ConnectStatus::IdentityMismatch)?
            .ok_or(ConnectStatus::IdentityMismatch)?;
        let metadata = AccountMetadata::new(
            "Workspace installation".into(),
            Some("Provider scopes mapped to approved capabilities".into()),
        )
        .map_err(|_| ConnectStatus::ProviderError)?;
        Ok(AuthorizedAccount {
            identity,
            payload,
            metadata,
        })
    }
}
