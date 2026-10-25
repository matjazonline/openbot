//! Guarded HTTP transport. Construction is owned by the production action composition.
use crate::{
    app_error::{AppError, AppResult},
    application::workflow::{actions::*, http::*},
    entities::mcp::McpEndpoint,
};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};

/// Operator-owned exact origins. The shared MCP destination policy supplies vetted,
/// pinned DNS, no proxies/redirects/retries and TLS hostname verification.
#[derive(Clone, Default)]
pub struct HttpDestinationPolicy(crate::adapters::mcp::policy::EndpointPolicy);
impl HttpDestinationPolicy {
    pub fn new(private_origins: impl IntoIterator<Item = String>) -> AppResult<Self> {
        let origins = private_origins
            .into_iter()
            .map(|s| HttpOrigin::parse(&s).map(|origin| format!("{}/", origin.as_str())))
            .collect::<AppResult<Vec<_>>>()?;
        Ok(Self(crate::adapters::mcp::policy::EndpointPolicy::new(
            origins,
        )?))
    }
}

pub(crate) struct HttpProvider {
    pub persistence: crate::adapters::persistence::PostgresPersistence,
    pub policy: HttpDestinationPolicy,
}
pub struct HttpRuntime {
    inner: crate::application::workflow::http::HttpActions<
        crate::adapters::persistence::PostgresPersistence,
        crate::adapters::persistence::workflow::action_dispatch::PostgresActionDispatch<
            crate::adapters::persistence::workflow::http::HttpActionAuthority,
            NoLocal,
        >,
        HttpProvider,
    >,
}
/// No same-database effect: remote providers only. Shared by the MCP composition.
pub(crate) struct NoLocal;
#[async_trait]
impl crate::adapters::persistence::workflow::action_dispatch::SqlLocalAction for NoLocal {
    async fn apply(
        &self,
        _: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _: &FrozenAction,
        _: &crate::application::workflow::actions::ApprovalSubject,
    ) -> AppResult<Value> {
        Err(failed())
    }
}
impl HttpRuntime {
    pub fn new(
        persistence: crate::adapters::persistence::PostgresPersistence,
        policy: HttpDestinationPolicy,
        lease: crate::application::workflow::lease::LeasePolicy,
    ) -> Self {
        Self::with_approvals(persistence, policy, lease, None)
    }
    /// `notices` lets a protected action park for review; without them it fails closed.
    pub fn with_approvals(
        persistence: crate::adapters::persistence::PostgresPersistence,
        policy: HttpDestinationPolicy,
        lease: crate::application::workflow::lease::LeasePolicy,
        notices: Option<crate::application::workflow::decisions::DecisionNotices>,
    ) -> Self {
        use crate::adapters::persistence::workflow::{
            action_dispatch::PostgresActionDispatch, http::HttpActionAuthority,
        };
        let mut dispatch =
            PostgresActionDispatch::new(persistence.clone(), HttpActionAuthority, NoLocal);
        if let Some(notices) = notices {
            dispatch = dispatch.with_decision_notices(notices);
        }
        Self {
            inner: HttpActions::new(
                persistence.clone(),
                dispatch,
                HttpProvider {
                    persistence,
                    policy,
                },
                lease,
            ),
        }
    }
    pub async fn call_tool(
        &self,
        fence: crate::application::workflow::lease::WorkflowFence,
        call: HttpToolCall,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        self.inner.call_tool(fence, call, cancel).await
    }
    /// Returns the durable result or protected-action suspension.
    pub async fn run(
        &self,
        kind: &crate::application::workflow::polling::WorkflowStepKind,
        claim: &crate::application::workflow::lease::ClaimedWorkflow,
    ) -> crate::application::workflow::lease::WorkflowHandlerOutcome {
        Box::pin(self.inner.run(kind, claim)).await
    }
}
#[async_trait]
impl crate::application::workflow::worker::WorkflowHandler for HttpRuntime {
    fn supports(&self, kind: &crate::application::workflow::polling::WorkflowStepKind) -> bool {
        self.inner.supports(kind)
    }
    async fn execute(
        &self,
        kind: &crate::application::workflow::polling::WorkflowStepKind,
        claim: &crate::application::workflow::lease::ClaimedWorkflow,
    ) -> crate::application::workflow::lease::WorkflowHandlerOutcome {
        // Bound the external provider/SQL composition at the adapter seam.
        Box::pin(self.inner.execute(kind, claim)).await
    }
}
#[async_trait]
impl RemoteAction for HttpProvider {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, action: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        let request = HttpRequest::parse(&action.request().arguments)?;
        tokio::time::timeout(Duration::from_secs(30), async {
            let (connection, _) = self.persistence.http_for_action(action).await?;
            let client = client(&self.policy, &connection).await?;
            // DNS may have waited: reload revocation and rotated credentials after resolution,
            // immediately before polling the HTTP request. The origin itself is immutable.
            let (connection, secret) = self.persistence.http_for_action(action).await?;
            send(client, &connection, secret.as_ref(), request).await
        })
        .await
        .map_err(|_| failed())?
    }
}

async fn client(
    policy: &HttpDestinationPolicy,
    connection: &HttpConnection,
) -> AppResult<reqwest::Client> {
    let endpoint =
        McpEndpoint::try_from(format!("{}/", connection.origin.as_str())).map_err(|_| failed())?;
    policy.0.client(&endpoint).await.map_err(|_| failed())
}
async fn send(
    client: reqwest::Client,
    connection: &HttpConnection,
    secret: Option<&SecretString>,
    request: HttpRequest,
) -> AppResult<Value> {
    let mut url = url::Url::parse(connection.origin.as_str()).map_err(|_| failed())?;
    let (path, query) = request
        .path
        .as_str()
        .split_once('?')
        .map_or((request.path.as_str(), None), |(path, query)| {
            (path, Some(query))
        });
    url.set_path(path);
    url.set_query(query);
    if url.origin().ascii_serialization() != connection.origin.as_str() {
        return Err(failed());
    }
    let method =
        reqwest::Method::from_bytes(request.method.as_str().as_bytes()).map_err(|_| failed())?;
    let mut outgoing = client.request(method, url);
    for (name, value) in request.headers {
        outgoing = outgoing.header(name, value);
    }
    if let Some(secret) = secret {
        outgoing = outgoing.bearer_auth(secret.expose_secret());
    }
    if let Some(body) = request.body {
        outgoing = outgoing.json(&body);
    }
    let response = outgoing.send().await.map_err(|_| failed())?;
    bounded_response(response, secret).await
}

async fn bounded_response(
    mut response: reqwest::Response,
    secret: Option<&SecretString>,
) -> AppResult<Value> {
    let status = response.status().as_u16();
    // Neither redirects nor remote failure responses establish a safe replay contract.
    if !(200..300).contains(&status)
        || response
            .content_length()
            .is_some_and(|n| n > MAX_HTTP_RESPONSE as u64)
    {
        return Err(failed());
    }
    let mut headers = BTreeMap::new();
    if let Some(value) = response.headers().get("content-type") {
        let value = value.to_str().map_err(|_| failed())?;
        if value.len() > 256 {
            return Err(failed());
        }
        headers.insert(
            "content-type",
            crate::adapters::redaction::redact_text(value, secret),
        );
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| failed())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_HTTP_RESPONSE {
            return Err(failed());
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| failed())?;
    let mut body = serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()));
    if !crate::adapters::redaction::redact_json(&mut body, secret) {
        return Err(failed());
    }
    let result = json!({"status":status,"headers":headers,"body":body});
    if serde_json::to_vec(&result).map_err(|_| failed())?.len() > 65_536 {
        return Err(failed());
    }
    Ok(result)
}

fn failed() -> AppError {
    AppError::BadRequest("HTTP action did not produce an accepted result".into())
}

#[cfg(test)]
mod tests;
