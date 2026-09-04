//! A scripted model over a real socket, for the tests that need an agent to actually answer.
//!
//! # Why a socket rather than a fake `AgentRunner`
//!
//! [`crate::services::agent_runner::AgentRunner`] is a concrete struct, built inline by
//! `thread::dispatch` and `task_worker`, so there is no trait to swap. What there *is* is an HTTP
//! seam: an agent's `config_json` carries `llm.base_url`, `ensure_config_fields` leaves it alone,
//! and `builder_with_provider` hands it to `ai_agents::UnifiedLLMProvider`. Pointing that at a
//! localhost listener means the real runner, the real prompt assembly, the real `UntrustedFence`
//! and the real tool registry all run -- only the model is scripted.
//!
//! The wire shape is OpenAI's `POST {base_url}/chat/completions`, because the `llm` crate's OpenAI
//! backend is what `ai-agents` reaches for and `mail_agents` permits only
//! `google | openai | anthropic | groq` (so `openai-compatible` is not an option).
//!
//! # Why not [`crate::adapters::memory::test_support`]
//!
//! Those helpers read one request with a single `stream.read()` into a fixed buffer. That is fine
//! for a memory-provider call and wrong here: an agent prompt plus the JSON schema of every
//! registered tool runs to tens of kilobytes and arrives across several segments, so a single read
//! truncates the body and the test sees a parse error instead of a request. This one frames the
//! request properly -- headers, then `Content-Length` bytes -- and bounds what it will accept.

use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

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

/// The same, for an agent that is expected to delegate to a sibling channel.
///
/// Three things stand between a model and an actual hop, and all three default closed:
///
/// * the tool has to be *granted*. `base_agent_config()` carries policy for
///   `outreach_and_await_quorum` but no top-level `tools:` list, and `declared_tool_ids` in the
///   `ai-agents` builder comes from that list -- so an agent without one is offered no tools at
///   all. `docs/inter_channel_agent_communication.md` is where the grant is documented.
/// * a `tool_choice` has to be set. The runtime asks the provider for one, and with `None` it
///   sends neither native tool definitions nor the prompt-protocol instructions, so the grant
///   above reaches the model as nothing at all.
/// * `allowed_target_scope` defaults to `external_only` and `internal_requires_approval` to
///   `true`, so a sibling channel is refused outright and a permitted one still waits for a human.
///
/// The policy lives under `tool_security.tools.<id>.config`, which is where `AgentRunner` reads it
/// from the merged agent config. Repeating it under the `tools:` entry does nothing --
/// `ToolSecurityConfig::enabled` defaults to false, so the `ai-agents` security engine hands every
/// tool a null `custom_config` and never carries these values itself.
///
/// This is the delegating configuration `docs/inter_channel_agent_communication.md` documents,
/// plus the `tool_choice` that doc does not mention.
pub fn delegating_agent_config(base_url: &str) -> Value {
    let mut config = scripted_agent_config(base_url);
    let outreach_tool_config = json!({
        "allowed_target_scope": "same_company_channels",
        "max_targets": 1,
        // The documented pairing: mail to strangers still waits for a human, a hop to
        // a colleague does not.
        "internal_requires_approval": false,
    });
    config["llm"]["tool_choice"] = json!("auto");
    config["tools"] = json!([
        { "name": crate::services::agent_directory_tool::AGENT_DIRECTORY_TOOL_ID },
        { "name": crate::services::outreach_tool::OUTREACH_TOOL_ID },
    ]);
    config["tool_security"] = json!({
        "tools": {
            crate::services::outreach_tool::OUTREACH_TOOL_ID: {
                "config": outreach_tool_config,
            }
        }
    });
    config
}

/// The `config_json` for a fixture agent pointed at a scripted model.
///
/// Only `llm.base_url` matters; provider, model and key are still resolved from the company's
/// model connection the way production does, so the fixture has to create one that agrees with
/// [`SCRIPTED_PROVIDER`] and [`SCRIPTED_MODEL`].
pub fn scripted_agent_config(base_url: &str) -> Value {
    json!({
        "llm": {
            "provider": SCRIPTED_PROVIDER,
            "model": SCRIPTED_MODEL,
            "base_url": base_url,
            "temperature": 0.0,
            "max_tokens": 1024,
        }
    })
}
