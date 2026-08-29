//! Transport machinery every HTTP memory provider shares.
//!
//! The bounds here — response byte cap, request body cap, identifier charset, status
//! classification — are the ones that keep a provider outage or a hostile response from becoming
//! an unbounded allocation or a leaked credential. They live in one place so two adapters cannot
//! drift apart on them.

use std::{
    future::Future,
    time::{Duration, Instant},
};

use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use crate::{
    entities::memory::{
        MAX_MEMORY_PROVIDER_BASE_URL_BYTES, MAX_MEMORY_PROVIDER_CONNECT_SECONDS,
        MAX_MEMORY_PROVIDER_CREDENTIAL_BYTES, MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES,
        MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES, MAX_MEMORY_PROVIDER_REQUEST_SECONDS,
        MAX_MEMORY_PROVIDER_RESPONSE_BYTES, MAX_MEMORY_RETURNED_ROWS,
        MAX_MEMORY_TARGET_COLLECTIONS, MemoryChunk, MemoryProviderError, MemoryRecallMode,
    },
    services::runtime_metrics::MemoryProviderActivity,
};

/// A bearer-authenticated JSON client with the deadlines `MemoryRecallMode` selects between.
pub struct MemoryHttpClient {
    client: Client,
    base_url: String,
    api_key: SecretString,
    fast_timeout: Duration,
    thinking_timeout: Duration,
    activity: MemoryProviderActivity,
}

impl MemoryHttpClient {
    /// Returns an error rather than panicking: a provider whose configuration cannot be honoured
    /// must leave the registry empty, not take the process down at boot. Boot-time validation of
    /// the environment already happened in `infra::config`.
    pub fn new(
        base_url: impl Into<String>,
        api_key: SecretString,
        fast_timeout: Duration,
        thinking_timeout: Duration,
    ) -> Result<Self, MemoryProviderError> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        if base_url.is_empty()
            || base_url.len() > MAX_MEMORY_PROVIDER_BASE_URL_BYTES
            || api_key.expose_secret().is_empty()
            || api_key.expose_secret().len() > MAX_MEMORY_PROVIDER_CREDENTIAL_BYTES
            || fast_timeout.is_zero()
            || thinking_timeout < fast_timeout
            || fast_timeout > Duration::from_secs(MAX_MEMORY_PROVIDER_REQUEST_SECONDS)
            || thinking_timeout > Duration::from_secs(MAX_MEMORY_PROVIDER_REQUEST_SECONDS)
        {
            return Err(MemoryProviderError::Unavailable);
        }
        Ok(Self {
            client: Client::builder()
                .connect_timeout(
                    fast_timeout.min(Duration::from_secs(MAX_MEMORY_PROVIDER_CONNECT_SECONDS)),
                )
                .build()
                .map_err(|_| MemoryProviderError::Unavailable)?,
            base_url,
            api_key,
            fast_timeout,
            thinking_timeout,
            activity: MemoryProviderActivity::default(),
        })
    }

    /// Report every call into the runtime metric sample this machine writes each ten seconds.
    /// The handle is shared across providers: the panel is a machine-level aggregate.
    pub fn with_activity(mut self, activity: MemoryProviderActivity) -> Self {
        self.activity = activity;
        self
    }

    pub fn fast_timeout(&self) -> Duration {
        self.fast_timeout
    }

    pub fn timeout_for(&self, mode: MemoryRecallMode) -> Duration {
        match mode {
            MemoryRecallMode::Fast => self.fast_timeout,
            MemoryRecallMode::Thinking => self.thinking_timeout,
        }
    }

    /// Time one provider call and tally its outcome. Wraps the whole exchange — connect, transfer
    /// and decode — because that is the latency the caller waits for.
    pub async fn measured<T>(
        &self,
        call: impl Future<Output = Result<T, MemoryProviderError>>,
    ) -> Result<T, MemoryProviderError> {
        let started = Instant::now();
        let outcome = call.await;
        self.activity.record(started.elapsed(), outcome.is_ok());
        outcome
    }

    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.base_url))
            .bearer_auth(self.api_key.expose_secret())
    }

    /// Read a JSON body under a hard byte cap. The `Content-Length` precheck rejects an oversized
    /// declared body without transferring it; the streaming loop covers a chunked body that
    /// declares nothing.
    pub async fn json_response(
        mut response: reqwest::Response,
    ) -> Result<Value, MemoryProviderError> {
        let status = response.status();
        if !status.is_success() {
            return Err(classify_status(status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MEMORY_PROVIDER_RESPONSE_BYTES as u64)
        {
            return Err(MemoryProviderError::ResponseTooLarge);
        }

        let capacity = response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0)
            .min(MAX_MEMORY_PROVIDER_RESPONSE_BYTES);
        let mut bytes = Vec::with_capacity(capacity);
        while let Some(chunk) = response.chunk().await.map_err(classify_transport)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_MEMORY_PROVIDER_RESPONSE_BYTES {
                return Err(MemoryProviderError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| MemoryProviderError::MalformedResponse)
    }

    pub fn bounded_json(value: &Value) -> Result<Vec<u8>, MemoryProviderError> {
        let bytes =
            serde_json::to_vec(value).map_err(|_| MemoryProviderError::MalformedResponse)?;
        if bytes.len() > MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES {
            return Err(MemoryProviderError::RequestTooLarge);
        }
        Ok(bytes)
    }
}

/// Identifiers we send are database ids, collection names and composed namespaces. Some providers
/// put them straight into a URL path segment, so the charset is a path-injection guard and not a
/// size bound — it gets its own error rather than being folded into `RequestTooLarge`.
pub fn validate_identifier(value: &str) -> Result<(), MemoryProviderError> {
    if value.is_empty() || value.len() > MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES {
        return Err(MemoryProviderError::RequestTooLarge);
    }
    if value.bytes().any(
        |byte| !matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'),
    ) {
        return Err(MemoryProviderError::InvalidIdentifier);
    }
    Ok(())
}

pub fn classify_status(status: StatusCode) -> MemoryProviderError {
    match status {
        // 402 is the hosted plans' insufficient-credit response. It is an account failure the
        // operator has to resolve, not a transient one, so it must not land in the retryable set;
        // reporting it as authentication is what puts the retry affordance in front of them.
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::PAYMENT_REQUIRED => {
            MemoryProviderError::Authentication
        }
        StatusCode::TOO_MANY_REQUESTS => MemoryProviderError::RateLimited,
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => MemoryProviderError::Timeout,
        StatusCode::NOT_FOUND => MemoryProviderError::NotFound,
        status if status.is_server_error() => MemoryProviderError::Unavailable,
        _ => MemoryProviderError::RejectedItem,
    }
}

pub fn classify_transport(error: reqwest::Error) -> MemoryProviderError {
    if error.is_timeout() {
        MemoryProviderError::Timeout
    } else {
        MemoryProviderError::Unavailable
    }
}

/// The bounds a recall request has to satisfy before any call is made.
pub fn validate_recall_bounds(
    scope_count: usize,
    max_results: u8,
) -> Result<(), MemoryProviderError> {
    if scope_count > MAX_MEMORY_TARGET_COLLECTIONS {
        return Err(MemoryProviderError::TooManyTargets);
    }
    if !(1..=MAX_MEMORY_RETURNED_ROWS).contains(&(max_results as usize)) {
        return Err(MemoryProviderError::TooManyResults);
    }
    Ok(())
}

/// One row a provider returned, before scope weighting is applied.
pub struct ScoredMemoryChunk {
    pub chunk: MemoryChunk,
    /// The provider's own relevance score, when it reports one.
    pub score: Option<f64>,
}

/// Everything one scope's call returned, in the order that provider ranked it.
pub struct ScopeRecallResults {
    pub weight: f32,
    pub rows: Vec<ScoredMemoryChunk>,
}

/// Rank rows from several single-scope calls into the one list the caller asked for.
///
/// A provider that takes a weighted collection map ranks across scopes itself. A provider whose
/// only partition is the namespace cannot, so `ResolvedMemoryScope::weight` has to be applied
/// here instead — otherwise the scope precedence the channel configured would silently stop
/// meaning anything.
pub fn merge_scope_results(
    scopes: Vec<ScopeRecallResults>,
    max_results: u8,
) -> Result<Vec<MemoryChunk>, MemoryProviderError> {
    let mut ranked = Vec::new();
    for scope in scopes {
        if scope.rows.len() > MAX_MEMORY_RETURNED_ROWS {
            return Err(MemoryProviderError::TooManyResults);
        }
        for (position, row) in scope.rows.into_iter().enumerate() {
            // A provider that reports no score still reports an order; keep it by deriving a
            // descending value from the row's position rather than flattening the scope to zero.
            let relevance = row.score.unwrap_or_else(|| 1.0 / (1.0 + position as f64));
            ranked.push((relevance * f64::from(scope.weight), row.chunk));
        }
    }
    ranked.sort_by(|(left, _), (right, _)| {
        right.partial_cmp(left).unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked.truncate(max_results as usize);
    Ok(ranked.into_iter().map(|(_, chunk)| chunk).collect())
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
