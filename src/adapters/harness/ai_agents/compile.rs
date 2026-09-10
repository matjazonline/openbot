//! Turning an [`AgentCapabilitySpec`] into the YAML `AgentBuilder::from_yaml` accepts.
//!
//! This is the only part of this adapter that is new rather than moved, and it is the platform's
//! single enforcement point for what an agent may call: the runtime builds its declared tool ids
//! from the compiled `tools:` list alone and denies every invocation outside it -- a model's and a
//! skill step's alike. Nothing else can reach an ungranted tool, so nothing else has to check.
//!
//! Everything here is synchronous and free-standing. `src/AGENTS.md` prefers a sync helper over
//! another `async` level, it makes the allowlist filter a pure and mock-free test, and this sits
//! at the deepest point of the frame the stack budget is measured on.

use serde_json::{Value, json};

use crate::app_error::{AppError, AppResult};
use crate::entities::{
    harness::{
        AgentCapabilitySpec, AiAgentsAdvancedConfigV1, AiAgentsReasoningMode,
        AiAgentsReflectionMode,
    },
    skill::{Skill, SkillInstruction},
    tool_catalogue::{CatalogueTool, GrantFilter, ToolSource, retain_grantable},
    value_objects::ToolId,
};
use crate::services::harness::NativeToolDeclaration;
use crate::services::prompt_fence::UNTRUSTED_INPUT_SYSTEM_PROMPT;

/// The runtime context block, resolved by the runtime's own `context:` sources at each turn.
///
/// It is appended to the agent's own prompt rather than configured separately because the
/// template variables in it only bind against the `context:` block that
/// [`base_agent_config`] declares -- separating the two produces a prompt full of unrendered
/// `{{ ... }}`.
const BASE_CONTEXT_SYSTEM_PROMPT: &str = "Runtime context:\n\
- Current local date: {{ context.time.date }}\n\
- Current local time: {{ context.time.time }}\n\
- Agent name: {{ context.agent_info.name }}\n\
- Recipient role: {{ context.recipient_role }}\n\
- Primary recipient: {{ context.is_to }}\n\
- CC recipient: {{ context.is_cc }}";

/// A compiled agent configuration, and what the compiler refused on the way.
///
/// The two refusal lists are separate because they mean different things to whoever reads the
/// log: one is a grant this platform will never honour, the other is a grant this particular run
/// could not serve.
pub struct CompiledConfig {
    pub yaml: String,
    /// The provider settings read back out of the compiled `llm:` block, so the builder does not
    /// have to re-parse the YAML it was just handed.
    pub provider: ProviderSettings,
    /// Grants the platform allowlist refused. Returned rather than dropped: a capability that
    /// vanishes without a log is a support ticket nobody can answer.
    pub refused_tools: Vec<ToolId>,
    /// Native grants this run has no context to serve -- an outreach grant on a run with no
    /// durable task, say. Declaring them anyway would offer the model a tool that fails on use.
    pub unavailable_tools: Vec<ToolId>,
}

/// What the LLM provider is built with, named so the three cannot be reordered at the call site.
pub struct ProviderSettings {
    pub config: ai_agents::llm::LLMConfig,
    pub base_url: Option<String>,
    pub tool_choice: Option<ai_agents::ToolChoice>,
}

/// Compile `spec` into runnable YAML.
///
/// The order is load-bearing: start from a fresh server-owned base, map only reviewed typed
/// settings, build the effective grant from typed capability fields, then stamp credentials and
/// model selection last. No persisted JSON is merged into the runtime document.
pub fn compile(
    spec: &AgentCapabilitySpec,
    api_key: &str,
    native_tools: &[NativeToolDeclaration],
) -> AppResult<CompiledConfig> {
    for id in spec.required_tool_ids() {
        if CatalogueTool::get(&id).is_some_and(|tool| {
            !tool.supports_harness(crate::entities::harness::HarnessKind::AiAgents)
        }) {
            return Err(AppError::BadRequest(format!(
                "Tool '{id}' is unsupported by this harness."
            )));
        }
    }
    let mut config = base_agent_config();
    let advanced = spec.harness_config.ai_agents().ok_or_else(|| {
        AppError::BadRequest("The stored config does not match the ai-agents harness.".into())
    })?;
    apply_advanced_config(&mut config, advanced);

    let grant = grant_for(spec, native_tools);
    if let Some(map) = config.as_object_mut() {
        // Removed rather than emitted empty when nothing survived: an agent that grants no tools
        // must compile to the document it compiled to before there was a grant list at all.
        // Only the `Simple(String)` form is emitted -- we grant no MCP entries.
        match grant.granted.is_empty() {
            true => map.remove("tools"),
            false => map.insert(
                "tools".to_string(),
                Value::Array(grant.granted.iter().map(|id| json!(id.as_str())).collect()),
            ),
        };
    }
    // Only emitted when the spec has skills of its own, keeping the default document minimal.
    if !spec.skills.is_empty() {
        config["skills"] = Value::Array(spec.skills.iter().map(compile_skill).collect());
    }

    ensure_config_fields(
        &mut config,
        spec.provider.as_str(),
        spec.model.as_str(),
        api_key,
        &spec.system_prompt,
        &spec.name,
    );
    append_base_context_prompt(&mut config);

    let mut provider = provider_settings(&config)?;
    if let Some(base_url) = &spec.provider_base_url {
        provider.base_url = Some(base_url.clone());
    }
    let yaml = serde_yaml::to_string(&config)
        .map_err(|error| AppError::Internal(format!("Agent configuration is not YAML: {error}")))?;

    Ok(CompiledConfig {
        yaml,
        provider,
        refused_tools: grant.refused,
        unavailable_tools: grant.unavailable,
    })
}

/// What survived the platform allowlist, and the two ways a grant can fail to.
struct ToolGrant {
    granted: Vec<ToolId>,
    refused: Vec<ToolId>,
    unavailable: Vec<ToolId>,
}

/// Everything this agent asked for, filtered down to what it may actually be given.
///
/// Three sources, in the order they are declared so the compiled list reads the same way twice:
/// the agent's own configuration, its explicit grants, and the tools its skills invoke.
///
/// The union with skill tools is not optional. The runtime checks a skill's tool step against the
/// same declared scope as a model-initiated call, so a skill naming a tool absent from `tools:`
/// dies mid-run on "not available in the current scope" rather than at compile time.
fn grant_for(spec: &AgentCapabilitySpec, native_tools: &[NativeToolDeclaration]) -> ToolGrant {
    let wanted = spec.required_tool_ids();

    let GrantFilter { granted, refused } = retain_grantable(&wanted);

    // A native tool is grantable in general and servable only by a run that was given its
    // context. Splitting the two keeps "this platform will never allow it" apart from "this run
    // cannot do it": different problems, with different fixes, and only one of them is a
    // misconfiguration.
    let servable = |id: &ToolId| match CatalogueTool::get(id) {
        Some(tool) if tool.source == ToolSource::Native => {
            native_tools.iter().any(|declaration| declaration.id == *id)
        }
        // Built-ins come from the runtime's own registry, which every run has.
        _ => true,
    };
    let (granted, unavailable): (Vec<ToolId>, Vec<ToolId>) =
        granted.into_iter().partition(servable);

    ToolGrant {
        granted,
        refused,
        unavailable,
    }
}

/// One skill, as the inline `SkillDefinition` the runtime deserializes.
///
/// `SkillDefinition` and `SkillStep` are both `#[serde(deny_unknown_fields)]`, so one stray key
/// here is a hard parse failure at `AgentBuilder::from_yaml` rather than an ignored extra. The
/// three optional sections -- reasoning, reflection, disambiguation -- are omitted rather than
/// written as null for the same reason.
fn compile_skill(skill: &Skill) -> Value {
    json!({
        "id": skill.slug.as_str(),
        "description": skill.description,
        "trigger": skill.trigger,
        "steps": skill.instructions.iter().map(compile_step).collect::<Vec<_>>(),
    })
}

/// One instruction, as a `SkillStep`.
///
/// `SkillStep` is untagged, so the *key name* is the discriminator: a map carrying `prompt` is a
/// prompt step and a map carrying `tool` is a tool step. Emitting both, or neither, deserializes
/// as an unhelpful untagged error -- which is why `Skill::validate` refuses such a skill at write
/// time and why nothing here has to decide what a malformed one means.
fn compile_step(instruction: &SkillInstruction) -> Value {
    match instruction {
        // `llm` is omitted: one provider is registered per run, so naming it would only be a
        // second place for the model choice to disagree with itself.
        SkillInstruction::Prompt { text } => json!({ "prompt": text }),
        SkillInstruction::Tool {
            tool,
            args,
            output_as,
        } => {
            let mut step = serde_json::Map::new();
            step.insert("tool".to_string(), json!(tool.as_str()));
            if let Some(args) = args {
                step.insert("args".to_string(), args.clone());
            }
            if let Some(output_as) = output_as {
                step.insert("output_as".to_string(), json!(output_as));
            }
            Value::Object(step)
        }
    }
}

/// Whether observability is on for this deployment.
///
/// Read from the environment directly rather than through `AppConfig`, because it is a debugging
/// switch for this adapter rather than a setting anything else consults.
fn ai_agents_observability_enabled() -> bool {
    std::env::var("ENABLE_AI_AGENTS_OBSERVABILITY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(false)
}

/// The server-owned half of every agent configuration: what may be observed, what needs a human,
/// what each tool is bounded by, and which runtime facts the prompt can read.
///
/// Persisted configuration is never merged over this document. The compiler maps reviewed typed
/// settings into selected fields while credentials, approval, ceilings, context, and observability
/// remain server-owned.
pub fn base_agent_config() -> Value {
    base_agent_config_with_observability(ai_agents_observability_enabled())
}

fn base_agent_config_with_observability(observability_enabled: bool) -> Value {
    json!({
        "observability": {
            "enabled": observability_enabled,
            "privacy": {
                "include_prompts": false,
                "include_responses": false,
                "include_tool_args": false,
                "include_tool_outputs": false,
                "hash_inputs": true
            },
            "export": {
                "write_report": false,
                "write_raw_events": false
            }
        },
        "hitl": {
            "default_timeout_seconds": 86400,
            "on_timeout": "reject",
            "tools": {
                "create_agent_channel": {
                    "require_approval": true,
                    "approval_context": [
                        "name",
                        "slug",
                        "description",
                        "granted_tool_ids",
                        "skill_slugs"
                    ]
                },
                "outreach_and_await_quorum": {
                    "require_approval": true,
                    "approval_context": [
                        "target_emails",
                        "completion_threshold_percent",
                        "timeout_hours",
                        "subject"
                    ]
                }
            }
        },
        "tool_security": {
            "tools": {
                "create_agent_channel": {
                    "timeout_ms": 10000,
                    "max_output_chars": 2000
                },
                "outreach_and_await_quorum": {
                    "timeout_ms": 10000,
                    "max_output_chars": 4000,
                    "config": {
                        "max_targets": crate::services::outreach_tool::DEFAULT_MAX_TARGETS,
                        "default_timeout_hours": crate::services::outreach_tool::DEFAULT_TIMEOUT_HOURS,
                        "max_timeout_hours": crate::services::outreach_tool::MAX_TIMEOUT_HOURS,
                        "allowed_target_scope": crate::services::outreach_tool::DEFAULT_ALLOWED_TARGET_SCOPE,
                        "internal_requires_approval": true
                    }
                },
                "list_company_agents": {
                    "timeout_ms": 5000,
                    "max_output_chars": 4000,
                    "config": {
                        "max_results": crate::services::agent_directory_tool::DEFAULT_DIRECTORY_MAX_RESULTS
                    }
                }
            }
        },
        "context": {
            "time": {
                "type": "builtin",
                "source": "datetime",
                "refresh": "per_turn"
            },
            "session": {
                "type": "builtin",
                "source": "session"
            },
            "agent_info": {
                "type": "builtin",
                "source": "agent"
            },
            "recipient_role": {
                "type": "runtime",
                "required": true
            },
            "is_to": {
                "type": "runtime",
                "required": true
            },
            "is_cc": {
                "type": "runtime",
                "required": true
            }
        }
    })
}

fn apply_advanced_config(config: &mut Value, advanced: &AiAgentsAdvancedConfigV1) {
    if let Some(reasoning) = &advanced.reasoning {
        let mode = match reasoning.mode {
            AiAgentsReasoningMode::None => "none",
            AiAgentsReasoningMode::ChainOfThought => "cot",
            AiAgentsReasoningMode::React => "react",
            AiAgentsReasoningMode::Auto => "auto",
        };
        let mut value = json!({
            "mode": mode,
            "output": "hidden",
            "max_iterations": reasoning.max_iterations
        });
        if reasoning.mode == AiAgentsReasoningMode::Auto {
            value["judge_llm"] = json!("default");
        }
        config["reasoning"] = value;
    }
    if let Some(reflection) = &advanced.reflection {
        let enabled = match reflection.enabled {
            AiAgentsReflectionMode::Disabled => "disabled",
            AiAgentsReflectionMode::Enabled => "enabled",
            AiAgentsReflectionMode::Auto => "auto",
        };
        let mut value = json!({
            "enabled": enabled,
            "max_retries": reflection.max_retries
        });
        if reflection.enabled != AiAgentsReflectionMode::Disabled {
            value["evaluator_llm"] = json!("default");
        }
        config["reflection"] = value;
    }
    if let Some(disambiguation) = &advanced.disambiguation {
        config["disambiguation"] = if disambiguation.enabled {
            json!({"enabled": true, "detection": {"llm": "default"}})
        } else {
            json!({"enabled": false})
        };
    }
}

/// Whether this config grants the model any tools.
///
/// The `tools:` list is the grant `ai-agents` builds `declared_tool_ids` from; `tool_security`
/// only carries policy for tools that are granted elsewhere, so it is deliberately not consulted.
fn declares_tools(config: &serde_json::Map<String, Value>) -> bool {
    config
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
}

/// Stamp the fields the server owns, and default the two the runtime gets wrong on its own.
///
/// The two fallbacks are required rather than optional, and are the agent's own name and prompt
/// as [`ResolvedAgentCapabilities`] resolved them. There is deliberately no last-resort literal here:
/// what an unnamed agent is called and what a promptless one says are decided once, in
/// `services::agent_runner::params`, and a second copy in this file is how the two drift.
///
/// [`ResolvedAgentCapabilities`]: crate::services::agent_runner::ResolvedAgentCapabilities
pub fn ensure_config_fields(
    config: &mut Value,
    provider: &str,
    model: &str,
    api_key: &str,
    fallback_system_prompt: &str,
    fallback_name: &str,
) {
    if !config.is_object() {
        *config = json!({});
    }

    if let Value::Object(map) = config {
        // Read before the `llm` entry is borrowed below: a grant in one half of the config decides
        // a default in the other.
        let grants_tools = declares_tools(map);

        let has_valid_name = map
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        if !has_valid_name {
            map.insert("name".to_string(), json!(fallback_name));
        }

        let has_valid_sys_prompt = map
            .get("system_prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        if !has_valid_sys_prompt {
            map.insert("system_prompt".to_string(), json!(fallback_system_prompt));
        }

        let llm_val = map.entry("llm").or_insert_with(|| json!({}));
        if !llm_val.is_object() {
            *llm_val = json!({});
        }

        if let Value::Object(llm_map) = llm_val {
            if !llm_map.contains_key("max_tokens") {
                // Override ai-agents' small implicit 2,048-token limit, which can
                // otherwise stop long email responses in the middle of a sentence.
                llm_map.insert("max_tokens".to_string(), json!(8192));
            }

            // A granted tool the model is never told about is not a grant. The runtime asks the
            // provider for a tool choice, and with none it sends neither native tool definitions
            // nor the prompt-protocol instructions -- so a config that lists `tools:` and omits
            // `tool_choice` runs with no tools at all, silently. Defaulting it here covers every
            // authoring path, and an explicit choice (`none` included) is left alone. Now that the
            // grant comes from a picker rather than by hand, that failure mode would otherwise be
            // one checkbox away for every user.
            if grants_tools && !llm_map.contains_key("tool_choice") {
                llm_map.insert("tool_choice".to_string(), json!("auto"));
            }

            llm_map.insert("provider".to_string(), json!(provider));
            llm_map.insert("model".to_string(), json!(model));
            llm_map.insert("api_key".to_string(), json!(api_key));
        }
    }
}

/// Append the untrusted-input convention and the runtime context block to the agent's own prompt.
///
/// Idempotent: a config that already carries the block is left alone, so compiling twice does not
/// stack two copies of it.
pub fn append_base_context_prompt(config: &mut Value) {
    let Some(system_prompt) = config.get("system_prompt").and_then(Value::as_str) else {
        return;
    };

    if system_prompt.contains(BASE_CONTEXT_SYSTEM_PROMPT) {
        return;
    }

    config["system_prompt"] = json!(full_system_prompt(system_prompt));
}

/// The system prompt an agent actually runs with: its own, then the convention for reading fenced
/// untrusted input, then the runtime context block.
fn full_system_prompt(agent_prompt: &str) -> String {
    format!("{agent_prompt}\n\n{UNTRUSTED_INPUT_SYSTEM_PROMPT}\n\n{BASE_CONTEXT_SYSTEM_PROMPT}")
}

/// Read the provider settings back out of the compiled `llm:` block.
///
/// The credential is deliberately removed from `extra` before it is handed to the provider
/// factory: it is passed separately, and leaving a second copy in a free-form map is how it ends
/// up in a debug rendering of the provider.
fn provider_settings(config: &Value) -> AppResult<ProviderSettings> {
    let llm = config.get("llm").cloned().ok_or_else(|| {
        AppError::Internal("Agent configuration is missing the llm section".to_string())
    })?;
    let spec: ai_agents::LLMConfig = serde_json::from_value(llm).map_err(|error| {
        AppError::BadRequest(format!("Agent llm configuration is not valid: {error}"))
    })?;
    let base_url = spec.base_url.clone();
    let tool_choice = spec.tool_choice.clone();
    let mut extra = spec.extra;
    extra.remove("api_key");

    Ok(ProviderSettings {
        config: ai_agents::llm::LLMConfig {
            temperature: Some(spec.temperature),
            max_tokens: Some(spec.max_tokens),
            top_p: spec.top_p,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop_sequences: None,
            timeout_seconds: spec.timeout_seconds,
            reasoning: spec.reasoning,
            reasoning_effort: spec.reasoning_effort,
            reasoning_budget_tokens: spec.reasoning_budget_tokens,
            extra,
        },
        base_url,
        tool_choice,
    })
}

#[cfg(test)]
#[path = "compile_tests.rs"]
mod tests;
