//! A scripted model over a real socket, for the tests that need an agent to actually answer.
//!
//! # Why a socket rather than a stub harness
//!
//! There is a trait to swap now -- [`StubHarness`] below is it -- and for a test about *dispatch*
//! that is the right double. It is the wrong one for a test about what the agent actually does:
//! swapping the harness out swaps out the compiler, the tool grant, the skill steps and the
//! runtime, which is most of what these tests exist to check.
//!
//! What the real harness offers instead is an HTTP seam: this test module registers a trusted
//! provider endpoint for a fixture agent id, and connection resolution carries it separately from
//! agent configuration. Pointing that at a localhost listener means the real runner,
//! the real prompt assembly, the real `UntrustedFence`, the real compiled configuration and the
//! real tool registry all run -- only the model is scripted.
//!
//! The wire shape is OpenAI's `POST {base_url}/chat/completions`, because the `llm` crate's OpenAI
//! backend is what `ai-agents` reaches for and `mail_agents` permits only
//! `google | openai | anthropic | groq | xai` (so `openai-compatible` is not an option).
//!
//! # Why not [`crate::adapters::memory::test_support`]
//!
//! Those helpers read one request with a single `stream.read()` into a fixed buffer. That is fine
//! for a memory-provider call and wrong here: an agent prompt plus the JSON schema of every
//! registered tool runs to tens of kilobytes and arrives across several segments, so a single read
//! truncates the body and the test sees a parse error instead of a request. This one frames the
//! request properly -- headers, then `Content-Length` bytes -- and bounds what it will accept.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use chrono::Utc;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use uuid::Uuid;

use crate::app_error::AppResult;
use crate::entities::{
    channel::{Channel, ChannelAccessMode},
    creation::CreationProvenance,
    harness::HarnessKind,
    task::TokenUsage,
    value_objects::{ChannelSlug, CompanySlug},
};
use crate::services::harness::{
    AgentExecutionDisposition, AgentExecutionOutput, AgentHarness, AgentRun,
};

static SCRIPTED_AGENT_BASE_URLS: OnceLock<Mutex<HashMap<Uuid, String>>> = OnceLock::new();

/// Register the trusted provider endpoint for one fixture agent.
pub fn register_scripted_agent_base_url(agent_id: Uuid, base_url: &str) {
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

/// What a scripted model server hands back to the test.
pub struct ScriptedLlm {
    /// Where the fixture agent should point. Always ends in `/`.
    pub base_url: String,
    /// Every request body that arrived, in order.
    pub requests: mpsc::UnboundedReceiver<Value>,
}

impl ScriptedLlm {
    /// The requests that have arrived so far, drained.
    ///
    /// Tests assert on the count as well as the content: the server answers exactly as many
    /// requests as it was given turns, so an unexpected extra model call -- a guardrail, a runtime
    /// auto-configuration step -- shows up as a refused connection and a failing count rather than
    /// as a hang.
    pub fn observed(&mut self) -> Vec<Value> {
        let mut requests = Vec::new();
        while let Ok(request) = self.requests.try_recv() {
            requests.push(request);
        }
        requests
    }
}

/// Answer `turns.len()` chat completions in order, capturing each request body.
///
/// The base URL ends in `/` deliberately: `llm` builds its endpoint with
/// `base_url.join("chat/completions")`, and `Url::join` replaces the last path segment of a URL
/// that does not.
pub async fn scripted_llm(turns: Vec<LlmTurn>) -> ScriptedLlm {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, requests) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        for (index, turn) in turns.into_iter().enumerate() {
            // One connection per turn: the responses below close the connection, so a client
            // making N calls opens N sockets.
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            match read_request_body(&mut stream).await {
                Some(body) => {
                    let _ = sender.send(body);
                    let _ = stream
                        .write_all(http_response(&turn.into_body(index)).as_bytes())
                        .await;
                }
                // A framing failure is reported as a 400 rather than a dropped connection, so the
                // agent's error names the double instead of a reset.
                None => {
                    let _ = stream
                        .write_all(
                            http_response(r#"{"error":{"message":"scripted llm could not frame the request"}}"#)
                                .as_bytes(),
                        )
                        .await;
                }
            }
        }
    });

    ScriptedLlm {
        base_url: format!("http://{address}/"),
        requests,
    }
}

/// Read one HTTP request and return its JSON body.
///
/// Headers first, then exactly `Content-Length` more bytes. Returns `None` for anything this
/// double will not stand behind: a peer that closed early, a body past [`MAX_REQUEST_BYTES`], a
/// missing length, or a body that is not JSON.
async fn read_request_body(stream: &mut TcpStream) -> Option<Value> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 16 * 1024];

    let header_end = loop {
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return None;
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let length = content_length(&buffer[..header_end])?;
    if length > MAX_REQUEST_BYTES {
        return None;
    }
    while buffer.len() < header_end + length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    serde_json::from_slice(&buffer[header_end..header_end + length]).ok()
}

/// The declared body length, from a case-insensitive `Content-Length` header.
fn content_length(headers: &[u8]) -> Option<usize> {
    std::str::from_utf8(headers)
        .ok()?
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim())
        })?
        .parse()
        .ok()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn http_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
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

    async fn update(&self, _id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
        unimplemented!()
    }

    async fn delete(&self, _id: Uuid) -> AppResult<()> {
        unimplemented!()
    }
}

/// A channel an agent answers on: enough of a [`Channel`] for a directory or delegation lookup to
/// classify it as a callable sibling.
pub fn agent_channel(company_id: Uuid, slug: &str) -> Channel {
    Channel {
        owner_agent_id: None,
        id: Uuid::new_v4(),
        company_id,
        name: slug.to_string(),
        description: None,
        slug: slug.into(),
        alias_slugs: Vec::new(),
        participant_emails: None,
        access_mode: ChannelAccessMode::Team,
        principal_grants: Vec::new(),
        agent_ids: Some(vec![Uuid::new_v4()]),
        enabled: true,
        add_3rd_party: true,
        retrieve_company_memory: false,
        retrieve_agent_memory: false,
        retrieve_user_memory: false,
        persist_company_memory: false,
        persist_agent_memory: false,
        persist_user_memory: false,
        created_by: CreationProvenance::system(),
        created_at: Utc::now(),
    }
}

/// An [`AgentHarness`] that answers without running anything.
///
/// For the tests that need a harness to *exist* -- registry wiring, dispatch resolution -- rather
/// than one that thinks. Tests that need a real run register a trusted fixture endpoint with
/// [`register_scripted_agent_base_url`] and drive the actual harness.
pub struct StubHarness {
    kind: HarnessKind,
    reply: String,
    disposition: AgentExecutionDisposition,
    /// When set, the run fails with this message instead of answering.
    failure: Option<String>,
}

impl StubHarness {
    /// A harness of `kind` that completes with a fixed reply.
    pub fn new(kind: HarnessKind) -> Self {
        Self {
            kind,
            reply: "stub reply".to_string(),
            disposition: AgentExecutionDisposition::Completed,
            failure: None,
        }
    }

    /// A harness whose run fails, for the caller-side error paths -- recording the failure,
    /// redacting it, and refusing to commit a reply.
    pub fn failing(mut self, message: impl Into<String>) -> Self {
        self.failure = Some(message.into());
        self
    }

    pub fn replying(mut self, reply: impl Into<String>) -> Self {
        self.reply = reply.into();
        self
    }

    /// A harness that parks instead of answering, for the caller-side suspension paths.
    pub fn suspending(mut self) -> Self {
        self.disposition = AgentExecutionDisposition::Suspended;
        self
    }
}

#[async_trait::async_trait]
impl AgentHarness for StubHarness {
    fn kind(&self) -> HarnessKind {
        self.kind
    }

    async fn run(&self, _run: AgentRun<'_>) -> AppResult<AgentExecutionOutput> {
        if let Some(failure) = self.failure.as_ref() {
            return Err(crate::app_error::AppError::Internal(failure.clone()));
        }
        Ok(AgentExecutionOutput {
            content: self.reply.clone(),
            token_usage: TokenUsage::default(),
            disposition: self.disposition,
            metadata: None,
        })
    }
}
