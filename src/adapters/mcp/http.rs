//! rmcp's stock reqwest adapter buffers unbounded JSON/error bodies. Enforce byte ceilings before
//! parsing either JSON or SSE, and never include remote errors, headers or bodies in diagnostics.
use super::preparation::{Control, HttpLifetime};
use crate::application::services::mcp_client::McpPreparationFailure;
#[cfg(test)]
#[path = "body_lifetime_tests.rs"]
mod body_lifetime_tests;
use futures::{StreamExt, stream::BoxStream};
use reqwest::{
    Method, StatusCode,
    header::{HeaderName, HeaderValue},
};
use rmcp::{
    model::ClientJsonRpcMessage,
    transport::streamable_http_client::{
        SseError, StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
    },
};
use secrecy::{ExposeSecret, SecretString};
use sse_stream::{Sse, SseStream};
use std::{collections::HashMap, io, sync::Arc};

pub(super) type Error = StreamableHttpError<HttpFailure>;

#[derive(Debug, thiserror::Error)]
pub(super) enum HttpFailure {
    #[error("MCP service rejected the configured credentials")]
    Authentication,
    #[error("MCP HTTP operation failed")]
    Unavailable,
}
#[derive(Clone)]
pub(super) struct BoundedHttp {
    pub client: reqwest::Client,
    pub remaining: Arc<std::sync::atomic::AtomicUsize>,
    pub control: Control,
    pub lifetime: Arc<HttpLifetime>,
    pub cleanup: Cleanup,
}
#[derive(Clone)]
pub(super) struct Cleanup {
    pub client: reqwest::Client,
    pub endpoint: String,
    pub token: Option<SecretString>,
    pub session: Arc<std::sync::Mutex<Option<SessionIdentity>>>,
}
pub(super) struct SessionIdentity {
    id: String,
    headers: HashMap<HeaderName, HeaderValue>,
}
impl Cleanup {
    pub async fn finish(&self) -> Result<(), Error> {
        let session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(SessionIdentity { id, headers }) = session {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                delete(
                    &self.client,
                    &self.endpoint,
                    &id,
                    self.token.as_ref().map(|t| t.expose_secret().to_owned()),
                    headers,
                ),
            )
            .await
            .map_err(|_| failed())??;
        }
        Ok(())
    }
}
fn failed() -> Error {
    StreamableHttpError::Client(HttpFailure::Unavailable)
}
impl BoundedHttp {
    fn request(
        client: &reqwest::Client,
        method: Method,
        uri: &str,
        session: Option<&str>,
        token: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<reqwest::RequestBuilder, Error> {
        let mut request = client
            .request(method, uri)
            .header("accept", "application/json, text/event-stream");
        if let Some(session) = session {
            request = request.header("mcp-session-id", session);
        }
        if let Some(token) = token {
            let mut value =
                HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| failed())?;
            value.set_sensitive(true);
            request = request.header("authorization", value);
        }
        // Only rmcp's negotiated protocol header is needed. No tenant-supplied headers exist.
        for (name, value) in headers {
            if name == "mcp-protocol-version" {
                request = request.header(name, value);
            }
        }
        Ok(request)
    }
}
fn bounded(
    response: reqwest::Response,
    remaining: Arc<std::sync::atomic::AtomicUsize>,
    control: Control,
    lifetime: Arc<HttpLifetime>,
) -> BoxStream<'static, Result<bytes::Bytes, io::Error>> {
    let mut source = response.bytes_stream();
    async_stream::try_stream! {
        let _lifetime=lifetime;
        while let Some(chunk) = control.run(async {Ok(source.next().await)}).await.map_err(|_|io::Error::other("MCP stream cancelled"))? {
            let chunk = chunk.map_err(|_| io::Error::other("MCP stream failed"))?;
            remaining.try_update(std::sync::atomic::Ordering::SeqCst,std::sync::atomic::Ordering::SeqCst,|value|value.checked_sub(chunk.len())).map_err(|_|io::Error::other("MCP response exceeds limit"))?;
            yield chunk;
        }
    }.boxed()
}
impl StreamableHttpClient for BoundedHttp {
    type Error = HttpFailure;
    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, Error> {
        // Only protocol cancellation is cleanup; initialize/catalog/call always require
        // the real selected-generation authority after every preceding wait.
        if let Some(id) = session_id.as_deref() {
            *self
                .cleanup
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(SessionIdentity {
                id: id.to_owned(),
                headers: headers.clone(),
            });
        }
        let cleanup = matches!(&message, ClientJsonRpcMessage::Notification(n) if matches!(&n.notification,rmcp::model::ClientNotification::CancelledNotification(_)));
        if !cleanup {
            self.control.check().await.map_err(|_| failed())?;
        }
        let request = Self::request(
            &self.client,
            Method::POST,
            &uri,
            session_id.as_deref(),
            auth,
            headers.clone(),
        )?
        .json(&message);
        let response = if cleanup {
            tokio::time::timeout(std::time::Duration::from_secs(5), request.send())
                .await
                .map_err(|_| failed())?
                .map_err(|_| failed())?
        } else {
            self.control
                .run(async {
                    request
                        .send()
                        .await
                        .map_err(|_| McpPreparationFailure::Unavailable)
                })
                .await
                .map_err(|_| failed())?
        };
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(StreamableHttpError::Client(HttpFailure::Authentication));
        }
        if !status.is_success() {
            return Err(failed());
        }
        if matches!(status, StatusCode::ACCEPTED | StatusCode::NO_CONTENT) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        let session = response
            .headers()
            .get("mcp-session-id")
            .map(|v| v.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| failed())?;
        if session.as_ref().is_some_and(|s| s.len() > 1024) {
            return Err(failed());
        }
        if let Some(id) = session.as_deref().or(session_id.as_deref()) {
            *self
                .cleanup
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(SessionIdentity {
                id: id.to_owned(),
                headers,
            });
        }
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .unwrap_or("")
            .trim()
            .to_owned();
        let mut stream = bounded(
            response,
            self.remaining.clone(),
            self.control.clone(),
            self.lifetime.clone(),
        );
        match mime.as_str() {
            "text/event-stream" => Ok(StreamableHttpPostResponse::Sse(
                SseStream::from_bytes_stream(stream).boxed(),
                session,
            )),
            "application/json" => {
                let mut bytes = Vec::new();
                while let Some(chunk) = stream.next().await {
                    bytes.extend_from_slice(&chunk.map_err(|_| failed())?);
                }
                let message = serde_json::from_slice(&bytes).map_err(|_| failed())?;
                Ok(StreamableHttpPostResponse::Json(message, session))
            }
            _ => Err(failed()),
        }
    }
    async fn delete_session(
        &self,
        uri: Arc<str>,
        session: Arc<str>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), Error> {
        delete(&self.client, &uri, &session, auth, headers).await?;
        self.cleanup
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        Ok(())
    }
    async fn get_stream(
        &self,
        _uri: Arc<str>,
        _session: Arc<str>,
        _last: Option<String>,
        _auth: Option<String>,
        _headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, Error> {
        // Optional unsolicited server streams are not needed for explicit bounded refresh/call.
        // POST SSE is fully supported. Avoid reconnecting a background notification stream.
        Err(StreamableHttpError::ServerDoesNotSupportSse)
    }
}

async fn delete(
    client: &reqwest::Client,
    uri: &str,
    session: &str,
    auth: Option<String>,
    headers: HashMap<HeaderName, HeaderValue>,
) -> Result<(), Error> {
    let response = BoundedHttp::request(client, Method::DELETE, uri, Some(session), auth, headers)?
        .send()
        .await
        .map_err(|_| failed())?;
    if response.status().is_success()
        || matches!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND
        )
    {
        Ok(())
    } else {
        Err(failed())
    }
}
