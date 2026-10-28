//! Direct bounded TypeSafe transport. No SDK, retries, model aliases or agent tools.
use crate::application::workflow::jev::*;
use crate::application::workflow::registry::jev::MAX_RESPONSE_BYTES;
use async_trait::async_trait;
use futures::StreamExt;
use secrecy::ExposeSecret;
use std::{sync::Arc, time::Duration};
use tokio::{sync::Semaphore, time::Instant};

pub struct JevHttp {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    slots: Arc<Semaphore>,
    #[cfg(test)]
    capacity_waiting: tokio::sync::Notify,
}
impl JevHttp {
    pub fn new() -> anyhow::Result<Self> {
        Self::build(reqwest::Url::parse("https://api.typesafe.ai/v1/systemone")?)
    }
    fn build(endpoint: reqwest::Url) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .no_proxy()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(30))
                .pool_max_idle_per_host(4)
                .pool_idle_timeout(Duration::from_secs(30))
                .build()?,
            endpoint,
            slots: Arc::new(Semaphore::new(4)),
            #[cfg(test)]
            capacity_waiting: tokio::sync::Notify::new(),
        })
    }
    #[cfg(test)]
    pub(crate) fn loopback(endpoint: &str) -> Self {
        let url = reqwest::Url::parse(endpoint).unwrap();
        assert!(
            url.host_str()
                .and_then(|host| host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok())
                .is_some_and(|ip| ip.is_loopback())
        );
        assert_eq!(url.scheme(), "http");
        assert!(url.username().is_empty() && url.password().is_none());
        Self::build(url).unwrap()
    }
}
struct HttpSlot {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl HttpSlot {
    async fn send(
        &self,
        request: &JevRequest,
        credential: &JevCredential,
    ) -> Result<JevResponse, JevError> {
        if !request.ready() || credential.model != *request.model() {
            return Err(JevError::Invalid);
        }
        let mut authorization = reqwest::header::HeaderValue::from_str(&format!(
            "Bearer {}",
            credential.bearer.expose_secret()
        ))
        .map_err(|_| JevError::Invalid)?;
        authorization.set_sensitive(true);
        let mut outgoing = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::AUTHORIZATION, authorization)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(request.body().to_vec());
        if request.has_images() {
            outgoing = outgoing.header("TypeSafe-Danger-Zone-Images", "true");
        }
        let response = outgoing.send().await.map_err(transport)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(JevError::Http(status, retry_after(response.headers())));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
        {
            return Err(JevError::Protocol);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(transport)?;
            if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
                return Err(JevError::Protocol);
            }
            body.extend_from_slice(&chunk);
        }
        request.validate_response(serde_json::from_slice(&body).map_err(|_| JevError::Protocol)?)
    }
}
fn transport(error: reqwest::Error) -> JevError {
    if error.is_timeout() {
        JevError::Timeout
    } else {
        JevError::Transport
    }
}
fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let text = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let seconds = text.parse::<u64>().ok().or_else(|| {
        chrono::DateTime::parse_from_rfc2822(text)
            .ok()
            .map(|time| (time.timestamp() - chrono::Utc::now().timestamp()).max(0) as u64)
    })?;
    Some(Duration::from_secs(seconds.min(60)))
}
#[async_trait]
impl JevProvider for JevHttp {
    async fn acquire(&self, deadline: Instant) -> Result<Box<dyn JevProviderSlot>, JevError> {
        if Instant::now() >= deadline {
            return Err(JevError::Timeout);
        }
        #[cfg(test)]
        self.capacity_waiting.notify_one();
        let permit = tokio::time::timeout_at(deadline, self.slots.clone().acquire_owned())
            .await
            .map_err(|_| JevError::Timeout)?
            .map_err(|_| JevError::Transport)?;
        Ok(Box::new(HttpSlot {
            client: self.client.clone(),
            endpoint: self.endpoint.clone(),
            _permit: permit,
        }))
    }
}
#[async_trait]
impl JevProviderSlot for HttpSlot {
    async fn evaluate(
        self: Box<Self>,
        request: &JevRequest,
        credential: &JevCredential,
        deadline: Instant,
    ) -> Result<JevResponse, JevError> {
        if Instant::now() >= deadline {
            return Err(JevError::Timeout);
        }
        tokio::time::timeout_at(deadline, self.send(request, credential))
            .await
            .map_err(|_| JevError::Timeout)?
    }
}
#[cfg(test)]
impl JevHttp {
    pub(crate) async fn evaluate(
        &self,
        request: &JevRequest,
        credential: &JevCredential,
        deadline: Instant,
    ) -> Result<JevResponse, JevError> {
        self.acquire(deadline)
            .await?
            .evaluate(request, credential, deadline)
            .await
    }
    pub(crate) async fn occupy_capacity(&self) -> tokio::sync::OwnedSemaphorePermit {
        self.slots.clone().acquire_many_owned(4).await.unwrap()
    }
    pub(crate) async fn wait_for_capacity_request(&self) {
        self.capacity_waiting.notified().await
    }
}
#[cfg(test)]
pub(crate) mod tests;

mod assets;
pub use assets::JevAssetReader;
