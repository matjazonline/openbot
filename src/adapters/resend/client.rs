//! The Resend HTTP surface: one shared client, bounded responses, and typed outcomes.
//!
//! Every failure this module can produce is a value the caller can act on. That is the whole
//! reason the trait exists: a delivery worker has to tell a definite refusal from an ambiguous one
//! and an inbound decoder has to tell "this mail will never be readable" from "ask again in a
//! minute", and a stringly-typed error erases both distinctions at the one seam that needs them.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::{
    adapters::protocols::email::parser::MAX_INBOUND_MESSAGE_BYTES,
    infra::config::{MAX_RESEND_RETRY_AFTER_SECS, ResendOutboundConfig},
};

/// The largest JSON body this adapter will read from the API. Generous for a retrieve response
/// carrying an HTML body inline, and far below the raw-MIME cap that governs the CDN download.
pub const MAX_RESEND_JSON_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Connecting is not transferring: a provider that has not answered the TCP handshake in this long
/// is down, and waiting the full request timeout for that verdict wastes a worker slot.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What the Resend API said, in the terms a caller has to decide with.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResendError {
    /// The provider refused, and will refuse the identical request again: a malformed address, an
    /// unverified sending domain, a revoked key, an email id that does not exist.
    #[error("resend refused the request ({status}): {detail}")]
    Refused { status: u16, detail: String },
    #[error("resend is rate limiting this deployment: {detail}")]
    RateLimited {
        retry_after: Option<Duration>,
        detail: String,
    },
    /// A 5xx, a connection failure or a timeout. The request may or may not have been acted on.
    #[error("resend is unavailable: {detail}")]
    Unavailable { detail: String },
    #[error("resend returned more than the {limit} bytes this adapter will read")]
    TooLarge { limit: usize },
    #[error("resend returned a body this build cannot read: {detail}")]
    Malformed { detail: String },
}

impl ResendError {
    /// Whether asking again could plausibly answer differently. Definite refusals and oversized or
    /// malformed bodies will read the same way on the fifth attempt.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::RateLimited { .. } | Self::Unavailable { .. })
    }
}

/// One outbound mail, in Resend's own shape.
///
/// `text` only: [`EmailRenderer`](crate::adapters::protocols::email::EmailRenderer) freezes a
/// plain-text body and nothing downstream of it composes HTML, so offering an `html` field here
/// would be a parameter no caller can fill.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResendSendRequest {
    pub from: String,
    pub to: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<String>,
    pub subject: String,
    pub text: String,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub headers: std::collections::BTreeMap<String, String>,
    /// Replayed unchanged across every attempt at one delivery, which is what makes retrying an
    /// ambiguous send safe rather than a duplicate.
    #[serde(skip)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ResendSendResponse {
    pub id: String,
}

/// A received mail as the retrieve endpoint reports it.
///
/// Only the fields this adapter uses are named. `html`, `text` and the parsed header map are
/// deliberately absent: the raw MIME is downloaded and parsed by the same code the SMTP listener
/// uses, so the body, the headers and the attachments all come from one representation rather than
/// from two that can disagree.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReceivedEmail {
    pub id: String,
    pub from: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub received_for: Vec<String>,
    pub raw: Option<RawReference>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RawReference {
    pub download_url: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// The Resend calls this deployment makes.
///
/// A trait so the delivery transport and the inbound decoder can both be tested against a fake
/// that answers with a chosen [`ResendError`]; the repository carries no HTTP-mocking crate and
/// hand-written doubles in `test_support` are what `src/AGENTS.md` asks for.
#[async_trait]
pub trait ResendApi: Send + Sync {
    async fn send_email(
        &self,
        request: &ResendSendRequest,
    ) -> Result<ResendSendResponse, ResendError>;

    async fn retrieve_received(&self, email_id: &str) -> Result<ReceivedEmail, ResendError>;

    /// The raw MIME behind a signed CDN URL, bounded by [`MAX_INBOUND_MESSAGE_BYTES`].
    async fn download_raw(&self, url: &str) -> Result<Vec<u8>, ResendError>;
}

/// The one process-wide Resend client. `reqwest::Client` owns the connection pool and the TLS
/// configuration, so building one per request would discard both.
pub struct ReqwestResendClient {
    client: Client,
    base_url: String,
    api_key: SecretString,
}

impl ReqwestResendClient {
    pub fn from_config(config: &ResendOutboundConfig) -> Result<Self, ResendError> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(CONNECT_TIMEOUT.min(config.request_timeout))
                .timeout(config.request_timeout)
                .build()
                .map_err(|error| ResendError::Unavailable {
                    detail: error.to_string(),
                })?,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            api_key: config.api_key.clone(),
        })
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.base_url))
            .bearer_auth(self.api_key.expose_secret())
    }
}

#[async_trait]
impl ResendApi for ReqwestResendClient {
    async fn send_email(
        &self,
        request: &ResendSendRequest,
    ) -> Result<ResendSendResponse, ResendError> {
        let mut builder = self.request(Method::POST, "/emails").json(request);
        if let Some(key) = request.idempotency_key.as_deref() {
            builder = builder.header("Idempotency-Key", key);
        }
        let response = builder.send().await.map_err(classify_transport)?;
        let bytes = bounded_body(response, MAX_RESEND_JSON_RESPONSE_BYTES).await?;
        serde_json::from_slice(&bytes).map_err(|error| ResendError::Malformed {
            detail: error.to_string(),
        })
    }

    async fn retrieve_received(&self, email_id: &str) -> Result<ReceivedEmail, ResendError> {
        // The id goes into a path segment, so anything that is not one is refused here rather than
        // sent as a request that could address something else.
        if !is_path_segment(email_id) {
            return Err(ResendError::Refused {
                status: StatusCode::BAD_REQUEST.as_u16(),
                detail: "a received-email id must be a plain identifier".to_string(),
            });
        }
        let response = self
            .request(Method::GET, &format!("/emails/receiving/{email_id}"))
            .send()
            .await
            .map_err(classify_transport)?;
        let bytes = bounded_body(response, MAX_RESEND_JSON_RESPONSE_BYTES).await?;
        serde_json::from_slice(&bytes).map_err(|error| ResendError::Malformed {
            detail: error.to_string(),
        })
    }

    async fn download_raw(&self, url: &str) -> Result<Vec<u8>, ResendError> {
        // The URL comes from an authenticated API response, but it is still a value from outside:
        // an http:// or file:// download_url would send a signed request somewhere unintended.
        let parsed = url::Url::parse(url).map_err(|error| ResendError::Malformed {
            detail: error.to_string(),
        })?;
        if parsed.scheme() != "https" {
            return Err(ResendError::Malformed {
                detail: "a raw-mail download URL must be https".to_string(),
            });
        }
        // Unauthenticated on purpose: the URL is signed, and attaching the API key would hand this
        // deployment's credential to whatever host the response named.
        let response = self
            .client
            .get(parsed)
            .send()
            .await
            .map_err(classify_transport)?;
        bounded_body(response, MAX_INBOUND_MESSAGE_BYTES).await
    }
}

/// Read a response under a hard byte cap, refusing an oversized declared body before transferring
/// it and an oversized chunked body as it arrives.
async fn bounded_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, ResendError> {
    let status = response.status();
    if !status.is_success() {
        let retry_after = retry_after_of(&response);
        // Bounded, because the failure detail of a 500 that returns an HTML error page must not be
        // the thing that allocates. The body is the provider's own words about the refusal.
        let detail = response
            .chunk()
            .await
            .ok()
            .flatten()
            .map(|chunk| String::from_utf8_lossy(&chunk[..chunk.len().min(512)]).into_owned())
            .unwrap_or_default();
        return Err(classify_status(status, retry_after, detail));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(ResendError::TooLarge { limit });
    }
    let capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0)
        .min(limit);
    let mut bytes = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.map_err(classify_transport)? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(ResendError::TooLarge { limit });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn retry_after_of(response: &reqwest::Response) -> Option<Duration> {
    let seconds: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(
        seconds.min(MAX_RESEND_RETRY_AFTER_SECS),
    ))
}

/// Map one HTTP status onto the decision it forces.
///
/// `408` and `429` sit with the transient failures rather than with the other 4xx: both say "not
/// now" rather than "not ever", and treating them as definite refusals would dead-letter mail over
/// a burst of traffic.
pub fn classify_status(
    status: StatusCode,
    retry_after: Option<Duration>,
    detail: String,
) -> ResendError {
    match status {
        StatusCode::TOO_MANY_REQUESTS => ResendError::RateLimited {
            retry_after,
            detail,
        },
        StatusCode::REQUEST_TIMEOUT => ResendError::Unavailable { detail },
        _ if status.is_client_error() => ResendError::Refused {
            status: status.as_u16(),
            detail,
        },
        _ => ResendError::Unavailable { detail },
    }
}

pub fn classify_transport(error: reqwest::Error) -> ResendError {
    // A body this adapter refused to read is not the provider being unavailable, and retrying it
    // would produce the same oversized response.
    if error.is_decode() {
        return ResendError::Malformed {
            detail: error.to_string(),
        };
    }
    ResendError::Unavailable {
        detail: error.to_string(),
    }
}

fn is_path_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_limit_is_transient_and_a_refusal_is_not() {
        assert!(classify_status(StatusCode::TOO_MANY_REQUESTS, None, String::new()).is_transient());
        assert!(classify_status(StatusCode::REQUEST_TIMEOUT, None, String::new()).is_transient());
        assert!(
            classify_status(StatusCode::INTERNAL_SERVER_ERROR, None, String::new()).is_transient()
        );
        assert!(
            !classify_status(StatusCode::UNPROCESSABLE_ENTITY, None, String::new()).is_transient()
        );
        assert!(!classify_status(StatusCode::NOT_FOUND, None, String::new()).is_transient());
        assert!(!ResendError::TooLarge { limit: 1 }.is_transient());
    }

    #[test]
    fn an_email_id_that_is_not_a_path_segment_is_refused_without_a_request() {
        assert!(is_path_segment("56761188-7520-42d8-8898-ff6fc54ce618"));
        assert!(!is_path_segment("../emails"));
        assert!(!is_path_segment("a/b"));
        assert!(!is_path_segment(""));
        assert!(!is_path_segment(&"a".repeat(129)));
    }

    #[test]
    fn a_send_request_serializes_without_its_idempotency_key_or_empty_fields() {
        let request = ResendSendRequest {
            from: "Support <support@acme.example>".to_string(),
            to: vec!["someone@example.com".to_string()],
            cc: Vec::new(),
            subject: "Re: hello".to_string(),
            text: "hi".to_string(),
            headers: std::collections::BTreeMap::new(),
            idempotency_key: Some("<delivery-abc@example>".to_string()),
        };
        let json = serde_json::to_value(&request).expect("the request serializes");
        assert_eq!(json["from"], "Support <support@acme.example>");
        assert_eq!(json["to"][0], "someone@example.com");
        // The key travels as a header, and empty collections are omitted rather than sent as [].
        assert!(json.get("idempotency_key").is_none());
        assert!(json.get("cc").is_none());
        assert!(json.get("headers").is_none());
    }
}
