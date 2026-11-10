//! Shared bounded HTTP transport for owned connections and private integration execution.
use crate::{
    app_error::{AppError, AppResult},
    application::workflow::http::*,
    entities::mcp::McpEndpoint,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::collections::BTreeMap;

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

    /// Refuses every request to `service_url`'s origin (the memory service, phase 05 E5).
    pub fn reserving(self, service_url: &str) -> AppResult<Self> {
        self.0.reserving(service_url).map(Self)
    }
}

pub(crate) async fn client(
    policy: &HttpDestinationPolicy,
    origin: &HttpOrigin,
) -> AppResult<reqwest::Client> {
    let endpoint = McpEndpoint::try_from(format!("{}/", origin.as_str())).map_err(|_| failed())?;
    policy.0.client(&endpoint).await.map_err(|_| failed())
}
pub(crate) fn request_url(origin: &HttpOrigin, request: &HttpRequest) -> AppResult<url::Url> {
    let mut url = url::Url::parse(origin.as_str()).map_err(|_| failed())?;
    let (path, query) = request
        .path
        .as_str()
        .split_once('?')
        .map_or((request.path.as_str(), None), |(path, query)| {
            (path, Some(query))
        });
    url.set_path(path);
    url.set_query(query);
    if url.origin().ascii_serialization() != origin.as_str() {
        return Err(failed());
    }
    Ok(url)
}

pub(crate) enum HttpAuthentication<'a> {
    None,
    Bearer(&'a SecretString),
    Header(&'a str, &'a SecretString),
}
impl HttpAuthentication<'_> {
    fn secret(&self) -> Option<&SecretString> {
        match self {
            Self::None => None,
            Self::Bearer(value) | Self::Header(_, value) => Some(value),
        }
    }
}

pub(crate) async fn send(
    client: reqwest::Client,
    connection: &HttpConnection,
    secret: Option<&SecretString>,
    request: HttpRequest,
) -> AppResult<Value> {
    let authentication = secret.map_or(HttpAuthentication::None, HttpAuthentication::Bearer);
    send_authenticated(client, &connection.origin, authentication, request).await
}

pub(crate) async fn send_authenticated(
    client: reqwest::Client,
    origin: &HttpOrigin,
    authentication: HttpAuthentication<'_>,
    request: HttpRequest,
) -> AppResult<Value> {
    let url = request_url(origin, &request)?;
    let method =
        reqwest::Method::from_bytes(request.method.as_str().as_bytes()).map_err(|_| failed())?;
    let mut outgoing = client.request(method, url);
    for (name, value) in request.headers {
        outgoing = outgoing.header(name, value);
    }
    outgoing = match &authentication {
        HttpAuthentication::None => outgoing,
        HttpAuthentication::Bearer(secret) => outgoing.bearer_auth(secret.expose_secret()),
        HttpAuthentication::Header(name, secret) => outgoing.header(*name, secret.expose_secret()),
    };
    if let Some(body) = request.body {
        outgoing = outgoing.json(&body);
    }
    let response = outgoing.send().await.map_err(|_| failed())?;
    bounded_response(response, authentication.secret()).await
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
