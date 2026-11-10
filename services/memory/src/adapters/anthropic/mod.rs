//! The Anthropic Messages API provider (D9): raw HTTP `POST {base_url}/v1/messages` with
//! structured output through `output_config.format`, no sampling parameters, no fallbacks and
//! no retries of its own. Error mapping keeps D9's retry rules: only Anthropic's error envelope
//! with a `request-id` is a definite rejection; anything else after the send is ambiguous.
//! Logs carry status codes and error types only, never prompts, output or the key.
use crate::{
    application::ports::{
        GenerationProvider, ProviderError, ProviderRequest, ProviderResponse, ProviderUsage,
        StopReason,
    },
    domain::access_level::AccessLevel,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt, time::Duration};

pub const ANTHROPIC_VERSION: &str = "2026-10-21";
pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// Longer than the D9 lease, so the worker's local deadline, not this, bounds a call (04B).
pub const PROVIDER_TIMEOUT: Duration = Duration::from_secs(120);

/// The API key; never printed.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: String) -> Self {
        Self(key)
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

#[derive(Debug, Clone)]
pub struct AnthropicSettings {
    pub base_url: String,
    pub api_key: ApiKey,
    pub model: String,
    /// `low` by default (D9).
    pub effort: String,
    pub timeout: Duration,
}

impl AnthropicSettings {
    pub fn new(api_key: ApiKey) -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            api_key,
            model: DEFAULT_MODEL.to_owned(),
            effort: "low".to_owned(),
            timeout: PROVIDER_TIMEOUT,
        }
    }
}

pub struct AnthropicProvider {
    settings: AnthropicSettings,
    http: reqwest::Client,
}

/// Retryable Anthropic error types (D9); everything else with an envelope is terminal.
const RETRYABLE: [&str; 3] = ["rate_limit_error", "overloaded_error", "api_error"];

/// The provider error types the service knows (the Messages API's documented set). The type
/// becomes an operation's `outcome_code` and a metric label, so anything else is bounded to
/// `provider_error` (review B1 F2b): a closed label set, always within `^[a-z_]{1,64}$`.
const KNOWN_KINDS: [&str; 9] = [
    "invalid_request_error",
    "authentication_error",
    "permission_error",
    "not_found_error",
    "request_too_large",
    "rate_limit_error",
    "api_error",
    "overloaded_error",
    "billing_error",
];

fn outcome_kind(kind: &str) -> String {
    if KNOWN_KINDS.contains(&kind) {
        kind.to_owned()
    } else {
        "provider_error".to_owned()
    }
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: String,
    error: EnvelopeError,
}

#[derive(Deserialize)]
struct EnvelopeError {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Message {
    stop_reason: Option<String>,
    #[serde(default)]
    content: Vec<Value>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

impl AnthropicProvider {
    /// The model must be `claude-sonnet-5-5` (the user decision, and `thinking:
    /// between_tools` is specific to it), and effort `low`, `medium` or `high`:
    /// `between_tools` is refused above `high`.
    pub fn new(settings: AnthropicSettings) -> Result<Self, String> {
        if settings.model != DEFAULT_MODEL {
            return Err(format!(
                "the generation model must be {DEFAULT_MODEL}; the request shape (thinking \
                 between_tools) is specific to it"
            ));
        }
        if !["low", "medium", "high"].contains(&settings.effort.as_str()) {
            return Err("effort must be low, medium or high with thinking off".into());
        }
        let http = reqwest::Client::builder()
            .timeout(settings.timeout)
            .no_proxy()
            .build()
            .map_err(|_| "the HTTP client could not be built".to_owned())?;
        Ok(Self { settings, http })
    }

    /// The exact request body sent (no temperature, top_p, top_k or fallbacks).
    pub fn body(&self, request: &ProviderRequest) -> Value {
        json!({
            "model": self.settings.model,
            "max_tokens": request.max_output_tokens,
            "system": request.system,
            "messages": [{"role": "user", "content": request.user}],
            // Thinking off: on claude-sonnet-5-5 omitting `thinking` runs adaptive thinking out
            // of `max_tokens`, and `{type: "disabled"}` is a 400; `between_tools` (no other
            // field, effort `high` or below) turns it off for this single, tool-less call.
            "thinking": {"type": "between_tools"},
            "output_config": {
                "effort": self.settings.effort,
                "format": {"type": "json_schema", "schema": request.schema},
            },
        })
    }

    fn classify(status: reqwest::StatusCode, request_id: bool, body: &[u8]) -> ProviderError {
        match serde_json::from_slice::<Envelope>(body) {
            Ok(envelope) if envelope.kind == "error" && request_id => {
                tracing::warn!(status = status.as_u16(), error = %envelope.error.kind, "provider rejected");
                let kind = outcome_kind(&envelope.error.kind);
                if RETRYABLE.contains(&kind.as_str()) {
                    ProviderError::Rejected(kind)
                } else {
                    ProviderError::Terminal(kind)
                }
            }
            _ => {
                tracing::warn!(
                    status = status.as_u16(),
                    "provider answer without its envelope"
                );
                ProviderError::Ambiguous
            }
        }
    }
}

#[async_trait]
impl GenerationProvider for AnthropicProvider {
    /// User decision 2026-11-08 (D9): every level a request is authorized for may be sent.
    fn max_level(&self) -> AccessLevel {
        AccessLevel::MAX
    }

    async fn generate(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let url = format!(
            "{}/v1/messages",
            self.settings.base_url.trim_end_matches('/')
        );
        let sent = self
            .http
            .post(url)
            .header("x-api-key", &self.settings.api_key.0)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&self.body(request))
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            // No connection: nothing was sent.
            Err(error) if error.is_connect() => return Err(ProviderError::NotSent),
            Err(_) => return Err(ProviderError::Ambiguous),
        };
        let status = response.status();
        let request_id = response.headers().contains_key("request-id");
        let Ok(body) = response.bytes().await else {
            return Err(ProviderError::Ambiguous);
        };
        if !status.is_success() {
            return Err(Self::classify(status, request_id, &body));
        }
        let Ok(message) = serde_json::from_slice::<Message>(&body) else {
            return Err(ProviderError::Ambiguous);
        };
        let text = message
            .content
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect::<String>();
        Ok(ProviderResponse {
            stop: match message.stop_reason.as_deref() {
                Some("end_turn") => StopReason::EndTurn,
                Some("max_tokens") => StopReason::MaxTokens,
                Some("refusal") => StopReason::Refusal,
                _ => StopReason::Other,
            },
            text,
            usage: ProviderUsage {
                input_tokens: message.usage.input_tokens,
                cache_creation_input_tokens: message.usage.cache_creation_input_tokens,
                cache_read_input_tokens: message.usage.cache_read_input_tokens,
                output_tokens: message.usage.output_tokens,
            },
        })
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
