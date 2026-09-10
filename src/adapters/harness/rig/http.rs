//! Bounded completion transport. Provider errors never include URLs, headers, or bodies.
use bytes::Bytes;
use rig::http_client::{
    self, HttpClientExt, LazyBody, MultipartForm, Request, Response, StreamingResponse,
};
use secrecy::{ExposeSecret, SecretString};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Default)]
pub(super) struct ProviderHttp {
    client: Option<reqwest::Client>,
    google_key: Option<SecretString>,
}
impl ProviderHttp {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client: Some(client),
            google_key: None,
        }
    }
    pub fn google(client: reqwest::Client, secret: &SecretString) -> Self {
        Self {
            client: Some(client),
            google_key: Some(secret.clone()),
        }
    }
    fn prepare<T: Into<Bytes>>(
        &self,
        request: Request<T>,
    ) -> http_client::Result<reqwest::Request> {
        let (parts, body) = request.into_parts();
        let mut url =
            url::Url::parse(&parts.uri.to_string()).map_err(|_| failure("invalid provider URL"))?;
        #[cfg(test)]
        crate::services::test_support::require_scripted_endpoint(Some(url.as_str()))
            .map_err(failure)?;
        let mut headers = parts.headers;
        if let Some(key) = &self.google_key {
            // Rig's Gemini extension puts a key in the URL. Use Google's supported auth header
            // instead, so URL-bearing transport diagnostics cannot expose tenant credentials.
            let query: Vec<_> = url
                .query_pairs()
                .filter(|(name, _)| name != "key")
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect();
            url.set_query(None);
            if !query.is_empty() {
                url.query_pairs_mut().extend_pairs(query);
            }
            let mut value = http_client::HeaderValue::from_str(key.expose_secret())
                .map_err(|_| failure("invalid provider credentials"))?;
            value.set_sensitive(true);
            headers.insert("x-goog-api-key", value);
        }
        for name in ["authorization", "x-api-key"] {
            if let Some(value) = headers.get_mut(name) {
                value.set_sensitive(true);
            }
        }
        self.client
            .as_ref()
            .ok_or_else(|| failure("provider transport not configured"))?
            .request(parts.method, url)
            .headers(headers)
            .body(body.into())
            .build()
            .map_err(|_| failure("invalid provider request"))
    }
}

impl HttpClientExt for ProviderHttp {
    fn send<T, U>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        let usage = super::usage::UsageProtocol::for_path(request.uri().path());
        let request = self.prepare(request);
        let client = self.client.clone();
        async move {
            let response = client
                .ok_or_else(|| failure("provider transport not configured"))?
                .execute(request?)
                .await
                .map_err(transport_error)?;
            if !response.status().is_success() {
                return Err(http_client::Error::InvalidStatusCode(response.status()));
            }
            let mut builder = Response::builder().status(response.status());
            *builder
                .headers_mut()
                .ok_or_else(|| failure("invalid provider response"))? = response.headers().clone();
            let body: LazyBody<U> = Box::pin(async move {
                let body = usage
                    .normalize(bounded_body(response).await?)
                    .map_err(|_| failure("invalid provider accounting"))?;
                if body.len() > MAX_RESPONSE_BYTES {
                    return Err(failure("provider response exceeds byte budget"));
                }
                Ok(U::from(body))
            });
            builder
                .body(body)
                .map_err(|_| failure("invalid provider response"))
        }
    }

    // Keep the trait's explicitly static future independent of the borrowed transport.
    #[allow(clippy::manual_async_fn)]
    fn send_multipart<U>(
        &self,
        _: Request<MultipartForm>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        async { Err(failure("multipart is not enabled for agent completions")) }
    }

    async fn send_streaming<T>(&self, _: Request<T>) -> http_client::Result<StreamingResponse>
    where
        T: Into<Bytes> + Send,
    {
        Err(failure("streaming is not enabled for agent completions"))
    }
}

async fn bounded_body(mut response: reqwest::Response) -> http_client::Result<Bytes> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(failure("provider response exceeds byte budget"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(failure("provider response exceeds byte budget"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}
fn failure(message: &'static str) -> http_client::Error {
    http_client::Error::Instance(std::io::Error::other(message).into())
}

impl std::fmt::Debug for ProviderHttp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProviderHttp { credentials: [REDACTED] }")
    }
}

/// Preserve timeout classification without retaining a reqwest error's URL or credential context.
#[derive(Debug, thiserror::Error)]
#[error("provider transport timed out")]
pub(super) struct ProviderTimeout;

fn transport_error(error: reqwest::Error) -> http_client::Error {
    if error.is_timeout() {
        http_client::Error::Instance(Box::new(ProviderTimeout))
    } else {
        failure("provider transport failed")
    }
}
