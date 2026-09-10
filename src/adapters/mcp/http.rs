//! rmcp's stock reqwest adapter buffers unbounded JSON/error bodies. Enforce byte ceilings before
//! parsing either JSON or SSE, and never include remote errors, headers or bodies in diagnostics.
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
use sse_stream::{Sse, SseStream};
use std::{collections::HashMap, io, sync::Arc};

type Error = StreamableHttpError<io::Error>;
#[derive(Clone)]
pub(super) struct BoundedHttp {
    pub client: reqwest::Client,
    pub remaining: Arc<std::sync::atomic::AtomicUsize>,
}
fn failed() -> Error {
    StreamableHttpError::Client(io::Error::other("MCP HTTP operation failed"))
}
impl BoundedHttp {
    fn request(
        &self,
        method: Method,
        uri: &str,
        session: Option<&str>,
        token: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<reqwest::RequestBuilder, Error> {
        let mut request = self
            .client
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
) -> BoxStream<'static, Result<bytes::Bytes, io::Error>> {
    let mut source = response.bytes_stream();
    async_stream::try_stream! {
        while let Some(chunk) = source.next().await {
            let chunk = chunk.map_err(|_| io::Error::other("MCP stream failed"))?;
            remaining.fetch_update(std::sync::atomic::Ordering::SeqCst,std::sync::atomic::Ordering::SeqCst,|value|value.checked_sub(chunk.len())).map_err(|_|io::Error::other("MCP response exceeds limit"))?;
            yield chunk;
        }
    }.boxed()
}
impl StreamableHttpClient for BoundedHttp {
    type Error = io::Error;
    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, Error> {
        let response = self
            .request(Method::POST, &uri, session_id.as_deref(), auth, headers)?
            .json(&message)
            .send()
            .await
            .map_err(|_| failed())?;
        let status = response.status();
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
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .unwrap_or("")
            .trim()
            .to_owned();
        let mut stream = bounded(response, self.remaining.clone());
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
        let response = self
            .request(Method::DELETE, &uri, Some(&session), auth, headers)?
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
