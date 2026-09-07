//! The `ai-agents` harness: this platform's one runtime, in this process.
//!
//! [`AiAgentsHarness`] is the only place that knows what an agent configuration looks like to
//! `ai-agents`. It compiles the capability spec into that dialect ([`compile`]), wires the
//! runtime's callbacks back onto the application's ports, runs the turn, and reports what it
//! cost. Everything above it -- dispatch, the task worker, the settings pages -- speaks only the
//! spec.
//!
//! # Stack
//!
//! This module sits at the bottom of the task-worker chain, which has aborted a process by
//! overflowing its thread stack before. Two rules from `src/AGENTS.md` are load-bearing here and
//! their measured effect is recorded on the functions themselves: everything that does not have
//! to be `async` is not, and the two descents into the runtime's own future chain are boxed.

mod approval;
mod classifier;
pub mod compile;
mod hooks;
mod tools;

pub use classifier::AiAgentsTextClassifier;
pub use compile::base_agent_config;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use ai_agents::{Agent, AgentBuilder};
use async_trait::async_trait;
use tracing::{info, warn};

use crate::app_error::{AppError, AppResult};
use crate::entities::{harness::HarnessKind, task::TokenUsage, value_objects::ModelProvider};
use crate::services::harness::{
    AgentExecutionDisposition, AgentExecutionOutput, AgentHarness, AgentRun,
    EXECUTION_DIAGNOSTICS_KEY, sanitize_text,
};

use approval::AiAgentsApprovalShim;
use compile::{CompiledConfig, compile};
use hooks::AiAgentsTraceShim;
use tools::NativeToolShim;

/// xAI's OpenAI-compatible API supports the function-call message protocol used by this runtime.
/// The pinned library's native xAI backend does not forward tools, so selecting it would silently
/// remove an agent's capabilities.
const XAI_OPENAI_COMPATIBLE_BASE_URL: &str = "https://api.x.ai/v1/";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderTransport {
    provider_type: ai_agents::ProviderType,
    base_url: Option<String>,
}

/// Resolve a logical company provider to the transport implementation `ai-agents` should use.
///
/// A supplied URL is the test-only trusted endpoint carried by the compiled capability spec. In
/// production xAI always receives the fixed official endpoint below; companies cannot supply an
/// arbitrary provider URL.
fn provider_transport(
    provider: &ModelProvider,
    configured_base_url: Option<String>,
) -> AppResult<ProviderTransport> {
    if provider.as_str() == "xai" {
        return Ok(ProviderTransport {
            provider_type: ai_agents::ProviderType::OpenAI,
            base_url: configured_base_url
                .or_else(|| Some(XAI_OPENAI_COMPATIBLE_BASE_URL.to_string())),
        });
    }

    let provider_type = std::str::FromStr::from_str(provider.as_str())
        .map_err(|_| AppError::BadRequest(format!("Unsupported LLM provider '{provider}'.")))?;
    Ok(ProviderTransport {
        provider_type,
        base_url: configured_base_url,
    })
}

/// The in-process `ai-agents` runtime.
///
/// Stateless: everything one run needs arrives in its [`AgentRun`], and nothing survives between
/// runs. That is what makes a single registered instance safe to share across every task worker.
#[derive(Debug, Default, Clone, Copy)]
pub struct AiAgentsHarness;

impl AiAgentsHarness {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentHarness for AiAgentsHarness {
    fn kind(&self) -> HarnessKind {
        HarnessKind::AiAgents
    }

    async fn run(&self, run: AgentRun<'_>) -> AppResult<AgentExecutionOutput> {
        let native_tools = run
            .tool_host
            .as_ref()
            .map(|host| host.available())
            .unwrap_or(&[]);
        let compiled = compile(&run.spec, run.api_key, native_tools)?;
        let suspended = Arc::new(AtomicBool::new(false));
        let callback_failure = Arc::new(Mutex::new(None));

        // A capability that vanishes without a log is a support ticket nobody can answer. Fields
        // rather than an interpolated sentence, and never the compiled YAML -- it carries the
        // company's credential until `sanitize_text` has run over it.
        for id in &compiled.refused_tools {
            warn!(
                tool_id = %id,
                agent_id = %run.agent_id,
                "tool grant is not in the platform allowlist and was dropped"
            );
        }
        for id in &compiled.unavailable_tools {
            warn!(
                tool_id = %id,
                agent_id = %run.agent_id,
                "tool grant has no context to run in on this task and was dropped"
            );
        }

        let executor = Executor {
            compiled: &compiled,
            run: &run,
            suspended: suspended.clone(),
            callback_failure: callback_failure.clone(),
        };

        // `build_agent` parses the agent config and wires every tool; it is the deepest point of
        // the whole task chain and the frame that used to tip it over the guard page.
        let agent = Box::pin(executor.build_agent()).await?;

        info!(
            provider = %run.spec.provider,
            model = %run.spec.model,
            prompt_characters = run.full_prompt.chars().count(),
            history_message_count = run.history_message_count,
            "Calling agent runtime"
        );

        // The provider call descends into the `ai_agents` runtime, whose own `async fn` chain is
        // not ours to shrink. Boxing here caps what this side of the boundary contributes to it.
        let response = Box::pin(agent.chat(run.full_prompt)).await;
        if let Some(error) = callback_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err(error);
        }
        let response = response.map_err(|error| AppError::Internal(error.to_string()))?;
        let clean_content = sanitize_text(&response.content, Some(run.api_key));
        let counted = count_tokens(response.metadata.as_ref(), run.full_prompt, &clean_content);

        let clean_meta = response.metadata.as_ref().and_then(|meta| {
            let val = serde_json::to_value(meta).ok()?;
            let sanitized = sanitize_text(&val.to_string(), Some(run.api_key));
            // A failed redaction round-trip must drop metadata, never fall back to the original
            // value that may contain the credential redaction was meant to remove.
            serde_json::from_str(&sanitized).ok()
        });
        let observability_report = agent.observability().and_then(|manager| {
            match serde_json::to_value(manager.generate_report()) {
                Ok(report) => Some(report),
                Err(_) => {
                    // Diagnostics must not turn a completed provider/tool run into a retry: that
                    // could repeat external effects solely because optional telemetry failed.
                    warn!("Could not render the agent observability report; omitting it");
                    None
                }
            }
        });

        let tool_names: Vec<String> = response
            .tool_calls
            .as_ref()
            .map(|calls| calls.iter().map(|call| call.name.clone()).collect())
            .unwrap_or_default();
        info!(
            response_characters = clean_content.chars().count(),
            tool_call_count = tool_names.len(),
            "Agent runtime returned"
        );
        let diagnostics = AgentExecutionDiagnostics {
            // Filled in by the caller, which owns the clock.
            duration_ms: 0,
            prompt_characters: run.full_prompt.chars().count(),
            response_characters: clean_content.chars().count(),
            history_message_count: run.history_message_count,
            token_usage_source: counted.source().to_string(),
            tool_call_count: tool_names.len(),
            tool_names,
        };

        let metadata = attach_execution_diagnostics(clean_meta, &diagnostics);
        let metadata = match observability_report {
            Some(report) => attach_observability_report(metadata, report),
            None => metadata,
        };

        Ok(AgentExecutionOutput {
            content: clean_content,
            token_usage: TokenUsage::new(counted.prompt_tokens, counted.completion_tokens),
            disposition: if suspended.load(Ordering::SeqCst) {
                AgentExecutionDisposition::Suspended
            } else {
                AgentExecutionDisposition::Completed
            },
            metadata,
        })
    }
}

/// One compiled run, and the ports it may reach back through.
///
/// A borrow of both rather than an owned copy: this exists only to give the three build steps a
/// `self` to hang off, and copying a compiled configuration -- prompt, skills and all -- onto the
/// stack at the deepest point of the task chain is exactly what the stack budget forbids.
struct Executor<'a> {
    compiled: &'a CompiledConfig,
    run: &'a AgentRun<'a>,
    suspended: Arc<AtomicBool>,
    callback_failure: Arc<Mutex<Option<AppError>>>,
}

impl Executor<'_> {
    /// Wire the agent the run will use.
    ///
    /// Only the two `auto_configure_*` calls need to be `async`, and they are all this function
    /// keeps. Everything on either side of them is synchronous and lives in the helpers below, so
    /// none of it is part of this future: an `async fn` at the bottom of the task chain pays for
    /// its whole body in stack, and this one used to cost 292 KiB of it before the split -- 174
    /// KiB after.
    async fn build_agent(&self) -> AppResult<ai_agents::RuntimeAgent> {
        let builder = self
            .builder_with_provider()?
            .auto_configure_features()
            .map_err(build_error)?;
        // Both are `ai_agents` futures we cannot slim down, so box them at the boundary rather
        // than carry them inline.
        //
        // MCP stays configured even though the compiler emits no MCP tool entries: it is a no-op
        // on an empty list, and removing it would be a decision about MCP rather than about this
        // adapter.
        let builder = Box::pin(builder.auto_configure_mcp())
            .await
            .map_err(build_error)?;
        let builder = Box::pin(builder.auto_configure_spawner())
            .await
            .map_err(build_error)?;
        self.build_with_tools(builder)
    }

    /// The agent config, the approval handler and the LLM provider -- everything the builder needs
    /// before the runtime's own auto-configuration runs.
    fn builder_with_provider(&self) -> AppResult<AgentBuilder> {
        let mut builder = AgentBuilder::from_yaml(&self.compiled.yaml).map_err(build_error)?;

        if let Some(approvals) = self.run.approvals.clone() {
            builder = builder.approval_handler(Arc::new(AiAgentsApprovalShim {
                approvals,
                suspended: self.suspended.clone(),
                failure: self.callback_failure.clone(),
            }));
        }

        let transport = provider_transport(
            &self.run.spec.provider,
            self.compiled.provider.base_url.clone(),
        )?;
        let mut provider = ai_agents::UnifiedLLMProvider::from_spec_config(
            transport.provider_type,
            self.run.spec.model.as_str(),
            Some(self.run.api_key.to_string()),
            transport.base_url,
            self.compiled.provider.config.clone(),
        )
        .map_err(build_error)?;
        if let Some(choice) = self.compiled.provider.tool_choice.clone() {
            provider = provider.with_tool_choice(choice);
        }
        Ok(builder.llm(Arc::new(provider)))
    }

    /// Our own tools, the trace hooks, and the delivery context the prompt templates read.
    ///
    /// Called after the runtime's auto-configuration, which is what registers the built-in tool
    /// registry these are added to.
    ///
    /// Every tool the host can serve is registered, granted or not, because registration is not a
    /// grant: the runtime builds its declared tool ids from the compiled `tools:` list alone and
    /// denies anything outside it, a skill's tool step included. Registering an ungranted tool
    /// therefore offers the model nothing -- and *not* registering a granted one would be a
    /// grant the runtime accepts and then cannot execute.
    fn build_with_tools(&self, mut builder: AgentBuilder) -> AppResult<ai_agents::RuntimeAgent> {
        if let Some(host) = self.run.tool_host.clone() {
            for declaration in host.available() {
                builder = builder.tool(Arc::new(NativeToolShim::new(
                    declaration.clone(),
                    host.clone(),
                    self.suspended.clone(),
                )));
            }
        }

        // One hook object sees every tool the run reaches for, including the runtime's built-ins
        // and anything behind MCP -- which is why this is a hook rather than logging inside each
        // of our own tools. The builder composes it with the runtime's observability hooks rather
        // than replacing them.
        if let Some(trace) = self.run.trace.clone() {
            builder = builder.hooks(Arc::new(AiAgentsTraceShim::new(trace)));
        }

        let agent = builder.build().map_err(build_error)?;

        let role_str = self
            .run
            .recipient_role
            .map(|role| role.as_str())
            .unwrap_or("to");
        set_context(&agent, "recipient_role", serde_json::json!(role_str))?;
        set_context(&agent, "is_to", serde_json::json!(role_str == "to"))?;
        set_context(&agent, "is_cc", serde_json::json!(role_str == "cc"))?;
        Ok(agent)
    }
}

fn set_context(
    agent: &ai_agents::RuntimeAgent,
    key: &str,
    value: serde_json::Value,
) -> AppResult<()> {
    agent.set_context(key, value).map_err(build_error)
}

/// A runtime that would not build. Internal rather than a bad request: the configuration it
/// refused is one this adapter compiled.
fn build_error(error: impl std::fmt::Display) -> AppError {
    AppError::Internal(format!("Could not build the agent runtime: {error}"))
}

/// What one run did, recorded beside the model's own metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct AgentExecutionDiagnostics {
    duration_ms: u64,
    prompt_characters: usize,
    response_characters: usize,
    history_message_count: usize,
    token_usage_source: String,
    tool_call_count: usize,
    tool_names: Vec<String>,
}

/// Put `diagnostics` beside whatever the provider returned, without losing either.
///
/// A provider that answered with something other than an object gets it nested rather than
/// dropped: it is evidence about a run that behaved oddly, which is exactly when it is wanted.
fn attach_execution_diagnostics(
    metadata: Option<serde_json::Value>,
    diagnostics: &AgentExecutionDiagnostics,
) -> Option<serde_json::Value> {
    let mut metadata = normalize_metadata(metadata);
    metadata.as_object_mut()?.insert(
        EXECUTION_DIAGNOSTICS_KEY.to_string(),
        serde_json::to_value(diagnostics).ok()?,
    );
    Some(metadata)
}

fn attach_observability_report(
    metadata: Option<serde_json::Value>,
    report: serde_json::Value,
) -> Option<serde_json::Value> {
    let mut metadata = normalize_metadata(metadata);
    metadata
        .as_object_mut()?
        .insert("observability".to_string(), report);
    Some(metadata)
}

/// The provider's metadata as an object we can add to. The key this nests a non-object under is
/// already in stored task rows, so it is fixed.
fn normalize_metadata(metadata: Option<serde_json::Value>) -> serde_json::Value {
    match metadata {
        Some(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        Some(value) => serde_json::json!({ "ai_agents_metadata": value }),
        None => serde_json::json!({}),
    }
}

/// A rough token count for text a provider did not count for us.
///
/// Four characters to a token is the usual English approximation. It is only ever a fallback: a
/// count that came from the provider is always preferred, and which of the two was used is
/// recorded on the run.
pub fn estimate_tokens(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        0
    } else {
        trimmed.len().div_ceil(4)
    }
}

/// Token counts for one exchange, and whether they came from the provider or from estimation.
struct CountedTokens {
    prompt_tokens: usize,
    completion_tokens: usize,
    prompt_estimated: bool,
    completion_estimated: bool,
}

impl CountedTokens {
    fn source(&self) -> &'static str {
        match (self.prompt_estimated, self.completion_estimated) {
            (false, false) => "provider",
            (true, true) => "estimated",
            _ => "mixed",
        }
    }
}

/// Read the provider's token accounting, falling back to a character-based estimate per side.
///
/// Providers disagree on where the numbers live (`prompt_tokens`/`input_tokens`, top level or
/// nested under `usage`), so every shape is probed before giving up on a side.
fn count_tokens(
    metadata: Option<&impl serde::Serialize>,
    prompt: &str,
    content: &str,
) -> CountedTokens {
    let parse_val = |v: &serde_json::Value| -> Option<usize> {
        v.as_u64()
            .map(|n| n as usize)
            .or_else(|| v.as_str().and_then(|s| s.parse::<usize>().ok()))
    };
    let read_pair = |value: &serde_json::Value| -> (Option<usize>, Option<usize>) {
        (
            value
                .get("prompt_tokens")
                .or_else(|| value.get("input_tokens"))
                .and_then(parse_val),
            value
                .get("completion_tokens")
                .or_else(|| value.get("output_tokens"))
                .and_then(parse_val),
        )
    };

    let meta = metadata.and_then(|meta| serde_json::to_value(meta).ok());
    let mut prompt_tokens = 0usize;
    let mut completion_tokens = 0usize;
    if let Some(ref meta) = meta {
        let (p, c) = read_pair(meta);
        prompt_tokens = p.unwrap_or(0);
        completion_tokens = c.unwrap_or(0);
        if prompt_tokens == 0
            && completion_tokens == 0
            && let Some(usage) = meta.get("usage")
        {
            let (p, c) = read_pair(usage);
            prompt_tokens = p.unwrap_or(prompt_tokens);
            completion_tokens = c.unwrap_or(completion_tokens);
        }
    }

    let prompt_estimated = prompt_tokens == 0;
    let completion_estimated = completion_tokens == 0;
    if prompt_estimated {
        prompt_tokens = estimate_tokens(prompt);
    }
    if completion_estimated {
        completion_tokens = estimate_tokens(content);
    }

    CountedTokens {
        prompt_tokens,
        completion_tokens,
        prompt_estimated,
        completion_estimated,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
