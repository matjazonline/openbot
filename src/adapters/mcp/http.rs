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
    pub pending_call: Arc<std::sync::Mutex<Option<rmcp::model::RequestId>>>,
    pub gate: Arc<tokio::sync::Mutex<()>>,
    #[cfg(test)]
    pub registration_probe: Option<Arc<super::registration_test_support::RegistrationProbe>>,
}
#[derive(Clone)]
pub(super) struct SessionIdentity {
    id: Option<String>,
    headers: HashMap<HeaderName, HeaderValue>,
}
impl Cleanup {
    fn record_protocol(&self, id: Option<&str>, headers: &HashMap<HeaderName, HeaderValue>) {
        *self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(SessionIdentity {
            id: id.map(str::to_owned),
            headers: headers.clone(),
        });
    }

    pub async fn cancel_pending(&self) -> Result<(), Error> {
        let _gate = self.gate.lock().await;
        self.cancel_locked().await
    }

    async fn cancel_locked(&self) -> Result<(), Error> {
        let pending = self
            .pending_call
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let Some(request_id) = pending else {
            return Ok(());
        };
        let identity = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(identity) = identity else {
            return Ok(());
        };
        let notification =
            rmcp::model::CancelledNotification::new(rmcp::model::CancelledNotificationParam {
                request_id,
                reason: Some("Local execution cancelled".into()),
            });
        let message = ClientJsonRpcMessage::notification(notification.into());
        // This fixed cleanup message must survive an already-stopped SDK. It cannot admit an effect.
        let request = BoundedHttp::request(
            &self.client,
            Method::POST,
            &self.endpoint,
            identity.id.as_deref(),
            self.token.as_ref().map(|t| t.expose_secret().to_owned()),
            identity.headers,
        )?
        .json(&message);
        let response = tokio::time::timeout(std::time::Duration::from_secs(3), request.send())
            .await
            .map_err(|_| failed())?
            .map_err(|_| failed())?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(failed())
        }
    }

    pub async fn finish(&self) -> Result<(), Error> {
        // Gate, cancellation and fallback DELETE share the original total cleanup budget.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let _gate = self.gate.lock().await;
            let _ = self.cancel_locked().await;
            let session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            if let Some(SessionIdentity {
                id: Some(id),
                headers,
            }) = session
            {
                delete(
                    &self.client,
                    &self.endpoint,
                    &id,
                    self.token.as_ref().map(|t| t.expose_secret().to_owned()),
                    headers,
                )
                .await?;
            }
            Ok(())
        })
        .await
        .map_err(|_| failed())?
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
        self.cleanup
            .record_protocol(session_id.as_deref(), &headers);
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
        self.cleanup
            .record_protocol(session.as_deref().or(session_id.as_deref()), &headers);
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
        #[cfg(test)]
        if let Some(probe) = &self.cleanup.registration_probe {
            probe.deleting(self.cleanup.gate.try_lock().is_err());
        }
        let _gate = self.cleanup.gate.lock().await;
        let _ = self.cleanup.cancel_locked().await;
        #[cfg(test)]
        if let Some(probe) = &self.cleanup.registration_probe {
            probe.cleanup_claimed.notify_one();
        }
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
