//! A scripted model over a real socket, for the tests that need an agent to actually answer.
//!
//! # Why a socket rather than a stub harness
//!
//! A stub in place of the harness would swap out the request assembly, the tool grant and the
//! provider protocol, which is most of what these tests exist to check. What the real harness
//! offers instead is an HTTP seam: this test module registers a trusted provider endpoint for a
//! fixture agent id, and connection resolution carries it separately from agent configuration.
//! Pointing that at a localhost listener means everything but the model runs for real.
//!
//! `scripted_llm` is the short OpenAI Chat Completions API. Request-checked scenarios use the
//! same supervised listener; `provider` supplies native Gemini, Anthropic, xAI Responses, and
//! OpenAI/Groq wire fixtures. Unsupported protocols fail explicitly.
//!
//! An agent prompt plus the JSON schema of every registered tool runs to tens of kilobytes and
//! arrives across several segments, so a single `stream.read()` into a fixed buffer would truncate
//! the body and the test would see a parse error instead of a request. This listener frames the
//! request properly -- headers, then `Content-Length` bytes -- and bounds what it will accept.

use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::app_error::AppResult;
use crate::entities::{
    channel::Channel,
    value_objects::{ChannelSlug, CompanySlug},
};

static SCRIPTED_AGENT_BASE_URLS: OnceLock<Mutex<HashMap<Uuid, String>>> = OnceLock::new();

static SCRIPTED_ORIGINS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Registration belongs to the listener, so an arbitrary loopback URL is not enough.
fn register_origin(base_url: &str) {
    SCRIPTED_ORIGINS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(
            url::Url::parse(base_url)
                .unwrap()
                .origin()
                .ascii_serialization(),
        );
}

fn unregister_origin(base_url: &str) {
    SCRIPTED_ORIGINS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .remove(
            &url::Url::parse(base_url)
                .unwrap()
                .origin()
                .ascii_serialization(),
        );
}

/// Fail before DNS, authentication or any public connection in model-backed unit tests.
pub(crate) fn require_scripted_endpoint(endpoint: Option<&str>) -> Result<(), &'static str> {
    let url = endpoint
        .and_then(|value| url::Url::parse(value).ok())
        .ok_or("model fixture endpoint is not registered")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !SCRIPTED_ORIGINS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .contains(&url.origin().ascii_serialization())
    {
        return Err("model fixture endpoint is not registered");
    }
    Ok(())
}

/// Register the trusted provider endpoint for one fixture agent.
pub fn register_scripted_agent_base_url(agent_id: Uuid, base_url: &str) {
    require_scripted_endpoint(Some(base_url)).expect("active fixture listener");
    SCRIPTED_AGENT_BASE_URLS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("scripted endpoint registry lock")
        .insert(agent_id, base_url.to_string());
}

pub(crate) fn scripted_agent_base_url(agent_id: Uuid) -> Option<String> {
    SCRIPTED_AGENT_BASE_URLS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("scripted endpoint registry lock")
        .get(&agent_id)
        .cloned()
}
use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};

/// The largest request body the double will read before giving up.
///
/// A prompt is bounded upstream, so anything past this is a runaway rather than a big test.
const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;

/// One turn a scripted model takes, in the order the test lists them.
pub enum LlmTurn {
    /// A plain assistant answer, which ends the agent's tool loop.
    Text(String),
    /// A call the agent runtime will execute before asking the model again -- so a turn of this
    /// kind must be followed by another turn.
    ToolCall { name: String, arguments: Value },
}

impl LlmTurn {
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text(content.into())
    }

    pub fn tool_call(name: impl Into<String>, arguments: Value) -> Self {
        Self::ToolCall {
            name: name.into(),
            arguments,
        }
    }

    /// The chat-completions response body for this turn.
    ///
    /// `arguments` is a JSON *string* rather than an object: that is what the wire format says and
    /// what `llm`'s `FunctionCall` deserializes, and getting it wrong looks like an empty tool
    /// call rather than a parse failure.
    fn into_body(self, index: usize) -> String {
        let (message, finish_reason) = match self {
            Self::Text(content) => (json!({ "role": "assistant", "content": content }), "stop"),
            Self::ToolCall { name, arguments } => (
                json!({
                    "role": "assistant",
                    "content": Value::Null,
                    "tool_calls": [{
                        "id": format!("call_{index}"),
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments.to_string(),
                        },
                    }],
                }),
                "tool_calls",
            ),
        };
        json!({
            "id": format!("chatcmpl-scripted-{index}"),
            "object": "chat.completion",
            "created": 0,
            "model": SCRIPTED_MODEL,
            "choices": [{ "index": 0, "message": message, "finish_reason": finish_reason }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 },
        })
        .to_string()
    }
}

/// The model name the fixture agent and the scripted responses agree on.
pub const SCRIPTED_MODEL: &str = "gpt-4o-mini";

/// The provider the fixture agent selects. Only the base URL makes it local.
pub const SCRIPTED_PROVIDER: &str = "openai";

pub(crate) mod provider;
mod scenario;
pub use scenario::{
    ScriptedExchange, ScriptedLlm, ScriptedRequest, ScriptedResponse, scripted_scenario,
};

/// Simple Chat Completions shorthand over the same supervised scenario server.
pub async fn scripted_llm(turns: Vec<LlmTurn>) -> ScriptedLlm {
    scripted_scenario(
        turns
            .into_iter()
            .enumerate()
            .map(|(index, turn)| {
                ScriptedExchange::new(
                    |request| {
                        if request.method != "POST" || request.path != "/chat/completions" {
                            return Err("Chat Completions method/path");
                        }
                        if request.body["model"] != SCRIPTED_MODEL {
                            return Err("scripted model");
                        }
                        Ok(())
                    },
                    ScriptedResponse::json_body(turn.into_body(index)),
                )
            })
            .collect(),
    )
    .await
}

// -------------------------------------------------------------------------------------------
// Shared doubles
// -------------------------------------------------------------------------------------------

/// A [`ChannelPersistence`] that answers only the two lookups a directory question needs.
///
/// Shared rather than re-declared per file: `src/AGENTS.md` puts hand-written mocks in one
/// `test_support` module, and the internal-delegation policy, the directory tool and the harness
/// approvals all ask the same two questions of a channel list.
///
/// The unanswered methods are `unimplemented!()` on purpose. A stub that returned a plausible
/// empty value would let a test exercise a path nobody wrote a fixture for and pass.
pub struct ChannelDirectoryStub {
    pub channels: Vec<Channel>,
    lookup_error: Option<String>,
}

impl ChannelDirectoryStub {
    pub fn new(channels: Vec<Channel>) -> Self {
        Self {
            channels,
            lookup_error: None,
        }
    }

    pub fn failing_lookup(mut self, message: impl Into<String>) -> Self {
        self.lookup_error = Some(message.into());
        self
    }
}

#[async_trait::async_trait]
impl ChannelPersistence for ChannelDirectoryStub {
    async fn create(&self, _company_id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
        unimplemented!()
    }

    async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Channel>> {
        unimplemented!()
    }

    async fn get_by_company_slug_and_channel_slug(
        &self,
        _company_slug: &CompanySlug,
        channel_slug: &ChannelSlug,
    ) -> AppResult<Option<Channel>> {
        if let Some(message) = self.lookup_error.as_ref() {
            return Err(crate::app_error::AppError::Database(message.clone()));
        }
        Ok(self
            .channels
            .iter()
            .find(|channel| &channel.slug == channel_slug)
            .cloned())
    }

    async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
        if let Some(message) = self.lookup_error.as_ref() {
            return Err(crate::app_error::AppError::Database(message.clone()));
        }
        Ok(self.channels.clone())
    }

    async fn update(
        &self,
        _request: crate::use_cases::channel::ChannelUpdate,
    ) -> AppResult<crate::entities::channel::Channel> {
        unimplemented!()
    }

    async fn delete(&self, _id: Uuid) -> AppResult<()> {
        unimplemented!()
    }
}
