use crate::adapters::persistence::task::TaskPersistence;
use crate::domain::monitoring::{AiExecutionMetrics, MonitoringService};
use crate::entities::agent::Agent as AgentEntity;
use crate::entities::approval::ApprovalStatus;
use crate::entities::channel::Channel;
use crate::entities::company::Company;
use crate::entities::message::{Message, MessageRole};
use crate::entities::task::TokenUsage;
use crate::entities::value_objects::{ChannelSlug, CompanySlug, EmailAddress};
use crate::services::agent_directory_tool::{AgentDirectoryContext, ListCompanyAgentsTool};
use crate::services::outreach_tool::{
    OUTREACH_TOOL_ID, OutreachAndAwaitQuorumTool, OutreachToolContext,
};
use crate::use_cases::approval::ApprovalUseCases;
use crate::use_cases::{
    agent::AgentPersistence,
    channel::{ChannelPersistence, InternalTargetOutcome, resolve_internal_target},
    thread::RecipientRole,
};
use ai_agents::{Agent, AgentBuilder};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};
use tracing::{info, warn};
use uuid::Uuid;

static URL_KEY_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)([\?&](?:api_key|apikey|key|access_token|token|secret|secret_key|private_key|app_key|app_secret|auth|authorization|password|bearer)=)([^&\s"'`<>\)]+)"#).unwrap()
});

static JSON_KEY_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)("(?:api_key|apikey|access_token|token|secret|secret_key|private_key|app_secret|password)"\s*:\s*")([^"]+)(")"#).unwrap()
});

static YAML_KEY_QUOTED_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\b(?:api_key|apikey|access_token|token|secret|secret_key|private_key|app_secret|password)\s*:\s*')([^']+)(')"#).unwrap()
});

static YAML_KEY_UNQUOTED_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\b(?:api_key|apikey|access_token|token|secret|secret_key|private_key|app_secret|password)\s*:\s*)([^\s\n"']+)"#).unwrap()
});

pub fn estimate_tokens(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        0
    } else {
        (trimmed.len() + 3) / 4
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AgentExecutionOutput {
    pub content: String,
    pub token_usage: TokenUsage,
    pub disposition: AgentExecutionDisposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionDisposition {
    Completed,
    Suspended,
}

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

fn attach_execution_diagnostics(
    metadata: Option<serde_json::Value>,
    diagnostics: &AgentExecutionDiagnostics,
) -> Option<serde_json::Value> {
    let mut metadata = match metadata {
        Some(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        Some(value) => serde_json::json!({ "ai_agents_metadata": value }),
        None => serde_json::json!({}),
    };
    metadata.as_object_mut()?.insert(
        "execution_diagnostics".to_string(),
        serde_json::to_value(diagnostics).ok()?,
    );
    Some(metadata)
}

fn attach_observability_report(
    metadata: Option<serde_json::Value>,
    report: serde_json::Value,
) -> Option<serde_json::Value> {
    let mut metadata = match metadata {
        Some(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        Some(value) => serde_json::json!({ "ai_agents_metadata": value }),
        None => serde_json::json!({}),
    };
    metadata
        .as_object_mut()?
        .insert("observability".to_string(), report);
    Some(metadata)
}

pub fn sanitize_text(input: &str, secret_key: Option<&str>) -> String {
    let mut result = URL_KEY_REGEX
        .replace_all(input, "${1}[REDACTED]")
        .into_owned();
    result = JSON_KEY_REGEX
        .replace_all(&result, "${1}[REDACTED]${3}")
        .into_owned();
    result = YAML_KEY_QUOTED_REGEX
        .replace_all(&result, "${1}[REDACTED]${3}")
        .into_owned();
    result = YAML_KEY_UNQUOTED_REGEX
        .replace_all(&result, "${1}[REDACTED]")
        .into_owned();

    if let Some(key) = secret_key {
        let trimmed = key.trim();
        if trimmed.len() >= 4 {
            result = result.replace(trimmed, "[REDACTED]");
        }
    }

    result
}

fn ai_agents_observability_enabled() -> bool {
    std::env::var("ENABLE_AI_AGENTS_OBSERVABILITY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(false)
}

const BASE_CONTEXT_SYSTEM_PROMPT: &str = "Runtime context:\n\
- Current local date: {{ context.time.date }}\n\
- Current local time: {{ context.time.time }}\n\
- Agent name: {{ context.agent_info.name }}\n\
- Recipient role: {{ context.recipient_role }}\n\
- Primary recipient: {{ context.is_to }}\n\
- CC recipient: {{ context.is_cc }}";

pub fn base_agent_config() -> serde_json::Value {
    base_agent_config_with_observability(ai_agents_observability_enabled())
}

fn base_agent_config_with_observability(observability_enabled: bool) -> serde_json::Value {
    serde_json::json!({
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
                "outreach_and_await_quorum": {
                    "timeout_ms": 10000,
                    "max_output_chars": 4000,
                    "config": {
                        "max_targets": 50,
                        "default_timeout_hours": 96,
                        "max_timeout_hours": 720,
                        "allowed_target_scope": "external_only",
                        "internal_requires_approval": true
                    }
                },
                "list_company_agents": {
                    "timeout_ms": 5000,
                    "max_output_chars": 4000,
                    "config": {
                        "max_results": 50
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedAgentParams {
    provider: String,
    model: String,
    api_key: String,
    config: serde_json::Value,
}

impl ResolvedAgentParams {
    /// Resolves LLM execution parameters by combining Company, Channel, and Agent entities.
    /// Company values are overridden by Channel and Channel values are overridden by Agent.
    pub fn new(
        company: Option<&Company>,
        channel: Option<&Channel>,
        agent: Option<&AgentEntity>,
    ) -> anyhow::Result<Self> {
        let mut config = base_agent_config();

        if let Some(ch_cfg) = channel.and_then(|w| w.channel_config.as_ref()) {
            if config.is_object() && ch_cfg.is_object() {
                merge_json(&mut config, ch_cfg);
            } else {
                config = ch_cfg.clone();
            }
        }

        if let Some(agent_cfg) = agent.and_then(|a| a.config_json.as_ref()) {
            if config.is_object() && agent_cfg.is_object() {
                merge_json(&mut config, agent_cfg);
            } else {
                config = agent_cfg.clone();
            }
        }

        let wf_llm = config.get("llm");

        let provider = agent
            .and_then(|a| a.provider.as_deref())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                channel
                    .and_then(|w| w.provider.as_deref())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                company
                    .and_then(|c| c.provider.as_deref())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                wf_llm
                    .and_then(|llm| llm.get("provider"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .map(|s| s.to_lowercase())
            .ok_or_else(|| anyhow::anyhow!("Agent provider is missing"))?;

        if !matches!(
            provider.as_str(),
            "google" | "openai" | "anthropic" | "groq"
        ) {
            anyhow::bail!(
                "Unsupported agent provider '{}'. Allowed providers are: google, openai, anthropic, groq",
                provider
            );
        }

        let model = agent
            .and_then(|a| a.model.as_deref())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                channel
                    .and_then(|w| w.model.as_deref())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                company
                    .and_then(|c| c.model.as_deref())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                wf_llm
                    .and_then(|llm| llm.get("model"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Agent model is missing"))?;

        let api_key = agent
            .and_then(|a| a.api_key.as_deref())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                channel
                    .and_then(|w| w.api_key.as_deref())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                company
                    .and_then(|c| c.api_key.as_deref())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                wf_llm
                    .and_then(|llm| llm.get("api_key"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .map(|s| s.to_string())
            .ok_or_else(|| {
                tracing::warn!("API key is missing for provider '{}'", provider);
                anyhow::anyhow!(
                    "API key is missing for provider '{}'. Please configure an API key in channel or company settings.",
                    provider
                )
            })?;

        let fallback_sys_prompt = agent.and_then(|a| a.system_prompt.as_deref());
        let fallback_name = agent
            .map(|a| a.name.as_str())
            .or_else(|| channel.map(|w| w.name.as_str()))
            .or_else(|| company.map(|c| c.name.as_str()));
        ensure_config_fields(
            &mut config,
            &provider,
            &model,
            &api_key,
            fallback_sys_prompt,
            fallback_name,
        );
        append_base_context_prompt(&mut config);

        Ok(Self {
            provider,
            model,
            api_key,
            config,
        })
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn config(&self) -> &serde_json::Value {
        &self.config
    }

    pub fn into_tuple(self) -> (String, String, String, serde_json::Value) {
        (self.provider, self.model, self.api_key, self.config)
    }
}

fn append_base_context_prompt(config: &mut serde_json::Value) {
    let Some(system_prompt) = config
        .get("system_prompt")
        .and_then(serde_json::Value::as_str)
    else {
        return;
    };

    config["system_prompt"] =
        serde_json::json!(format!("{system_prompt}\n\n{BASE_CONTEXT_SYSTEM_PROMPT}"));
}

pub fn merge_json(base: &mut serde_json::Value, override_val: &serde_json::Value) {
    match (base, override_val) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(override_map)) => {
            for (k, v) in override_map {
                if let Some(base_val) = base_map.get_mut(k) {
                    merge_json(base_val, v);
                } else {
                    base_map.insert(k.clone(), v.clone());
                }
            }
        }
        (base_slot, override_val) => {
            *base_slot = override_val.clone();
        }
    }
}

pub fn ensure_config_fields(
    config: &mut serde_json::Value,
    provider: &str,
    model: &str,
    api_key: &str,
    fallback_system_prompt: Option<&str>,
    fallback_name: Option<&str>,
) {
    if !config.is_object() {
        *config = serde_json::json!({});
    }

    if let serde_json::Value::Object(map) = config {
        let has_name = map
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .is_some();

        if !has_name {
            let name_str = fallback_name
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("agent");
            map.insert("name".to_string(), serde_json::json!(name_str));
        }

        let has_system_prompt = map
            .get("system_prompt")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .is_some();

        if !has_system_prompt {
            let sys_prompt = fallback_system_prompt
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("You are a helpful assistant.");
            map.insert("system_prompt".to_string(), serde_json::json!(sys_prompt));
        }

        let llm_val = map.entry("llm").or_insert_with(|| serde_json::json!({}));
        if !llm_val.is_object() {
            *llm_val = serde_json::json!({});
        }

        if let serde_json::Value::Object(llm_map) = llm_val {
            if !llm_map.contains_key("max_tokens") {
                // Override ai-agents' small implicit 2,048-token limit, which can
                // otherwise stop long email responses in the middle of a sentence.
                llm_map.insert("max_tokens".to_string(), serde_json::json!(8192));
            }

            if llm_map
                .get("provider")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .is_none()
            {
                llm_map.insert("provider".to_string(), serde_json::json!(provider));
            }

            if llm_map
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .is_none()
            {
                llm_map.insert("model".to_string(), serde_json::json!(model));
            }

            if llm_map
                .get("api_key")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .is_none()
            {
                llm_map.insert("api_key".to_string(), serde_json::json!(api_key));
            }
        }
    }
}

fn provider_config_from_agent_config(
    config: &serde_json::Value,
) -> anyhow::Result<(
    ai_agents::llm::LLMConfig,
    Option<String>,
    Option<ai_agents::ToolChoice>,
)> {
    let llm = config
        .get("llm")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Agent configuration is missing the llm section"))?;
    let spec: ai_agents::LLMConfig = serde_json::from_value(llm)?;
    let base_url = spec.base_url.clone();
    let tool_choice = spec.tool_choice.clone();
    let mut extra = spec.extra;
    extra.remove("api_key");
    let provider_config = ai_agents::llm::LLMConfig {
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
    };

    Ok((provider_config, base_url, tool_choice))
}

#[derive(Clone, Debug)]
pub struct ApprovalContext {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub channel_slug: ChannelSlug,
    pub company_slug: CompanySlug,
    pub thread_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub approver_email: EmailAddress,
}

/// Everything needed to decide whether one outreach call is purely internal, and whether that
/// earns it a pass on human approval.
///
/// Approval is keyed by tool ID, so the outreach tool alone cannot distinguish "ask a colleague"
/// from "mail a stranger". This carries the resolved answer instead: the recipients are classified
/// against the channel directory, not taken on the model's word.
#[derive(Clone)]
pub struct InternalDelegationPolicy {
    pub channel_persistence: Arc<dyn ChannelPersistence>,
    pub app_domain_name: String,
    pub company_id: Uuid,
    pub source_channel_id: Uuid,
    /// When false, a call whose recipients are *all* same-company agent channels skips the human.
    /// Defaults to true, so behaviour is unchanged until an operator opts in.
    pub requires_approval: bool,
}

/// Read `tool_security.tools.outreach_and_await_quorum.config.internal_requires_approval`.
///
/// Absent, malformed, or non-boolean all mean `true`: this gates outbound mail, so anything other
/// than an explicit `false` fails closed.
fn internal_requires_approval(config: &serde_json::Value) -> bool {
    config
        .get("tool_security")
        .and_then(|v| v.get("tools"))
        .and_then(|v| v.get(OUTREACH_TOOL_ID))
        .and_then(|v| v.get("config"))
        .and_then(|v| v.get("internal_requires_approval"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

pub struct AgentApprovalHandler {
    pub approval_use_cases: Arc<ApprovalUseCases>,
    pub context: ApprovalContext,
    pub suspended: Arc<AtomicBool>,
    /// `None` when the run has no outreach tool, so nothing can be auto-approved.
    pub delegation: Option<InternalDelegationPolicy>,
}

impl InternalDelegationPolicy {
    /// Whether this trigger is an outreach call whose every recipient is a callable same-company
    /// agent channel, and policy lets such a call skip the human.
    ///
    /// Every uncertain case answers `false`. An unresolvable recipient, a lookup failure, or a
    /// recipient that is not a channel all fall through to the human rather than past the gate.
    async fn approves_without_human(&self, trigger: &ai_agents::hitl::ApprovalTrigger) -> bool {
        if self.requires_approval {
            return false;
        }
        let ai_agents::hitl::ApprovalTrigger::Tool { name, args } = trigger else {
            return false;
        };
        if name != OUTREACH_TOOL_ID {
            return false;
        }
        let Some(targets) = args.get("target_emails").and_then(|v| v.as_array()) else {
            return false;
        };
        // An empty list is not "all internal"; it is a malformed call.
        if targets.is_empty() {
            return false;
        }

        for target in targets {
            let Some(email) = target.as_str() else {
                return false;
            };
            let outcome = resolve_internal_target(
                &email.trim().to_lowercase(),
                &self.app_domain_name,
                self.company_id,
                self.source_channel_id,
                self.channel_persistence.as_ref(),
            )
            .await;
            match outcome {
                Ok(InternalTargetOutcome::Callable(_)) => {}
                Ok(_) => return false,
                Err(error) => {
                    warn!(
                        "Could not classify outreach recipient while deciding approval, \
                         falling back to human approval: {}",
                        error
                    );
                    return false;
                }
            }
        }
        true
    }
}

#[async_trait::async_trait]
impl ai_agents::hitl::ApprovalHandler for AgentApprovalHandler {
    async fn request_approval(
        &self,
        req: ai_agents::hitl::ApprovalRequest,
    ) -> ai_agents::hitl::ApprovalResult {
        // Ahead of the approver check on purpose: delegating to a colleague needs no approver, and
        // a coordinator channel with no configured participant must still be able to do it.
        if let Some(policy) = self.delegation.as_ref()
            && policy.approves_without_human(&req.trigger).await
        {
            info!("Outreach targets only same-company agent channels; approval not required");
            return ai_agents::hitl::ApprovalResult::Approved;
        }

        if self.context.approver_email.trim().is_empty() {
            return ai_agents::hitl::ApprovalResult::rejected_with_reason(
                "No channel participant or company team member is configured to approve this action.",
            );
        }
        let step_raw = match &req.trigger {
            ai_agents::hitl::ApprovalTrigger::Tool { name, args } => {
                format!(
                    "tool:{}:{}",
                    name,
                    serde_json::to_string(args).unwrap_or_default()
                )
            }
            ai_agents::hitl::ApprovalTrigger::Condition { name, matched } => {
                format!("condition:{}:{}", name, matched)
            }
            ai_agents::hitl::ApprovalTrigger::State { from, to } => {
                format!("state:{:?}:{}", from, to)
            }
        };

        let thread_str = self
            .context
            .thread_id
            .map(|t| t.to_string())
            .unwrap_or_default();
        let task_str = self
            .context
            .task_id
            .map(|task| task.to_string())
            .unwrap_or_default();
        let step_key = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}:{}", task_str, thread_str, step_raw).as_bytes())
        );

        // Check if DB already has approval or rejection
        if let Ok(Some(status)) = self
            .approval_use_cases
            .check_step_approval(
                self.context.company_id,
                self.context.channel_id,
                self.context.thread_id,
                &step_key,
            )
            .await
        {
            match status {
                ApprovalStatus::Approved => return ai_agents::hitl::ApprovalResult::Approved,
                ApprovalStatus::Rejected => {
                    return ai_agents::hitl::ApprovalResult::rejected_with_reason(
                        "Approval previously rejected by human",
                    );
                }
                _ => {}
            }
        }

        // New HITL Step: Create DB record & send email with links
        let action_title = match &req.trigger {
            ai_agents::hitl::ApprovalTrigger::Tool { name, .. } => {
                format!("Tool Execution: {}", name)
            }
            ai_agents::hitl::ApprovalTrigger::Condition { name, .. } => {
                format!("Condition Approval: {}", name)
            }
            ai_agents::hitl::ApprovalTrigger::State { to, .. } => {
                format!("State Transition: {}", to)
            }
        };

        let action_summary = if !req.message.is_empty() {
            req.message.clone()
        } else {
            step_raw.clone()
        };

        let payload = serde_json::json!({
            "trigger": req.trigger,
            "context": req.context
        });

        let res = self
            .approval_use_cases
            .create_and_send_approval_request(
                self.context.company_id,
                self.context.channel_id,
                &self.context.channel_name,
                &self.context.channel_slug,
                &self.context.company_slug,
                self.context.thread_id,
                self.context.task_id,
                &step_key,
                &self.context.approver_email,
                req.trigger.trigger_type(),
                &action_title,
                &action_summary,
                payload,
            )
            .await;

        match res {
            Ok(_) => {
                self.suspended.store(true, Ordering::SeqCst);
                ai_agents::hitl::ApprovalResult::rejected_with_reason(
                    "Approval requested via email link; task paused waiting for human decision.",
                )
            }
            Err(e) => ai_agents::hitl::ApprovalResult::rejected_with_reason(e.to_string()),
        }
    }
}

use crate::infra::config::AppConfig;

pub struct AgentRunner<'a> {
    prompt: &'a str,
    history: &'a [Message],
    params: &'a ResolvedAgentParams,
    approval_use_cases: Option<Arc<ApprovalUseCases>>,
    approval_context: Option<ApprovalContext>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    app_config: Option<Arc<AppConfig>>,
    company: Option<Company>,
    company_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    skip_spam_guardrail: bool,
    recipient_role: Option<RecipientRole>,
    upstream_pipeline_context: Option<String>,
    task_persistence: Option<Arc<dyn TaskPersistence>>,
    channel_persistence: Option<Arc<dyn ChannelPersistence>>,
    agent_persistence: Option<Arc<dyn AgentPersistence>>,
    outreach_context: Option<OutreachToolContext>,
}

impl<'a> AgentRunner<'a> {
    pub fn new(prompt: &'a str, params: &'a ResolvedAgentParams) -> Self {
        Self {
            prompt,
            history: &[],
            params,
            approval_use_cases: None,
            approval_context: None,
            monitoring: None,
            app_config: None,
            company: None,
            company_id: None,
            channel_id: None,
            agent_id: None,
            skip_spam_guardrail: false,
            recipient_role: None,
            upstream_pipeline_context: None,
            task_persistence: None,
            channel_persistence: None,
            agent_persistence: None,
            outreach_context: None,
        }
    }

    pub fn config(mut self, config: Option<Arc<AppConfig>>) -> Self {
        self.app_config = config;
        self
    }

    pub fn company(mut self, company: Option<Company>) -> Self {
        self.company = company;
        self
    }

    pub fn history(mut self, history: &'a [Message]) -> Self {
        self.history = history;
        self
    }

    pub fn params(mut self, params: &'a ResolvedAgentParams) -> Self {
        self.params = params;
        self
    }

    pub fn approval_use_cases(mut self, use_cases: Option<Arc<ApprovalUseCases>>) -> Self {
        self.approval_use_cases = use_cases;
        self
    }

    pub fn approval_context(mut self, ctx: Option<ApprovalContext>) -> Self {
        self.approval_context = ctx;
        self
    }

    pub fn monitoring(mut self, monitoring: Option<Arc<dyn MonitoringService>>) -> Self {
        self.monitoring = monitoring;
        self
    }

    pub fn ids(
        mut self,
        company_id: Option<Uuid>,
        channel_id: Option<Uuid>,
        agent_id: Option<Uuid>,
    ) -> Self {
        self.company_id = company_id;
        self.channel_id = channel_id;
        self.agent_id = agent_id;
        self
    }

    pub fn skip_spam_guardrail(mut self, skip: bool) -> Self {
        self.skip_spam_guardrail = skip;
        self
    }

    pub fn recipient_role(mut self, role: Option<RecipientRole>) -> Self {
        self.recipient_role = role;
        self
    }

    pub fn upstream_pipeline_context(mut self, ctx: Option<String>) -> Self {
        self.upstream_pipeline_context = ctx;
        self
    }

    pub fn outreach_tool(
        mut self,
        persistence: Arc<dyn TaskPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        context: OutreachToolContext,
    ) -> Self {
        self.task_persistence = Some(persistence);
        self.channel_persistence = Some(channel_persistence);
        self.outreach_context = Some(context);
        self
    }

    /// Let the agent discover which sibling channels it may call.
    ///
    /// Separate from [`AgentRunner::outreach_tool`] because the directory is a read, not a send:
    /// an agent can be given the address book without being given the ability to write to it.
    pub fn agent_directory(mut self, agent_persistence: Arc<dyn AgentPersistence>) -> Self {
        self.agent_persistence = Some(agent_persistence);
        self
    }

    pub async fn execute(self) -> anyhow::Result<AgentExecutionOutput> {
        let start_time = std::time::Instant::now();
        let history_message_count = self.history.len();
        info!(
            "Executing AI Agent with prompt length {} and history count {}",
            self.prompt.len(),
            self.history.len()
        );

        let raw_full_prompt = self.compose_prompt();
        let key = &self.params.api_key;

        // Stage 3: Optional LLM Spam & Guardrail Evaluation (skipped for trusted participants)
        if !self.skip_spam_guardrail
            && let Some(ref cfg) = self.app_config
        {
            crate::services::llm_guardrail::LlmSpamGuardrail::evaluate(
                cfg,
                self.company.as_ref(),
                self.monitoring.as_ref(),
                &raw_full_prompt,
                &self.params.provider,
                &self.params.model,
                key,
            )
            .await?;
        }

        let mut config = self.params.config.clone();
        ensure_config_fields(
            &mut config,
            &self.params.provider,
            &self.params.model,
            key,
            None,
            None,
        );
        let config_yaml = serde_yaml::to_string(&config).unwrap_or_default();
        let (provider_config, base_url, tool_choice) = provider_config_from_agent_config(&config)?;

        let full_prompt = sanitize_text(&raw_full_prompt, Some(key));
        info!("Full prompt context length: {}", full_prompt.len());
        if !config_yaml.is_empty() {
            info!(
                "Running agent with channel config YAML:\n{}",
                sanitize_text(&config_yaml, Some(key))
            );
        }

        let task = AgentTask {
            config_yaml,
            provider_name: self.params.provider.clone(),
            model_name: self.params.model.clone(),
            api_key: key.clone(),
            provider_config,
            base_url,
            tool_choice,
            approval: self
                .approval_use_cases
                .clone()
                .zip(self.approval_context.clone()),
            outreach: self
                .task_persistence
                .clone()
                .zip(self.channel_persistence.clone())
                .zip(self.outreach_context.clone())
                .map(|((tasks, channels), context)| (tasks, channels, context)),
            agent_persistence: self.agent_persistence.clone(),
            recipient_role: self.recipient_role,
            full_prompt,
            history_message_count,
            internal_requires_approval: internal_requires_approval(&config),
            suspended: Arc::new(AtomicBool::new(false)),
        };

        // Run on its own task: provider futures are deep, and this keeps the caller's stack shallow.
        let task_result = tokio::spawn(task.run()).await;
        let duration_ms = start_time.elapsed().as_millis() as u64;

        match task_result {
            Ok(Ok(mut output)) => {
                // Wall-clock time is only known here, after the task has been awaited.
                if let Some(diagnostics) = output
                    .metadata
                    .as_mut()
                    .and_then(|meta| meta.get_mut("execution_diagnostics"))
                    .and_then(|value| value.as_object_mut())
                {
                    diagnostics.insert("duration_ms".to_string(), serde_json::json!(duration_ms));
                }
                self.record_execution(duration_ms, Some(&output.token_usage), None);
                Ok(output)
            }
            Ok(Err(err)) => {
                let err_msg = sanitize_text(&err.to_string(), Some(key));
                tracing::warn!("AI Agent execution failed ({err_msg})");
                self.record_execution(duration_ms, None, Some(err_msg.clone()));
                Err(anyhow::anyhow!("{err_msg}"))
            }
            Err(join_err) => {
                let err_msg = sanitize_text(&join_err.to_string(), Some(key));
                tracing::warn!("AI Agent task panicked or was cancelled ({err_msg})");
                self.record_execution(
                    duration_ms,
                    None,
                    Some(format!("Panicked or cancelled: {}", err_msg)),
                );
                Err(anyhow::anyhow!("Task failed: {err_msg}"))
            }
        }
    }

    /// Assemble what the model sees: delivery context, upstream pipeline output, conversation
    /// history, then the message itself.
    fn compose_prompt(&self) -> String {
        let delivery_ctx = match self.recipient_role {
            Some(RecipientRole::To) => {
                "[Delivery Context: Email received via TO field (Primary Target)]\n"
            }
            Some(RecipientRole::Cc) => {
                "[Delivery Context: Email received via CC field (Secondary / FYI Target)]\n"
            }
            None => "",
        };

        let pipeline_ctx = self
            .upstream_pipeline_context
            .as_deref()
            .map(str::trim)
            .filter(|upstream| !upstream.is_empty())
            .map(|upstream| {
                format!("[Upstream Pipeline Context from Prior Step Agents]:\n{upstream}\n\n")
            })
            .unwrap_or_default();

        let mut history_str = String::new();
        if !self.history.is_empty() {
            history_str.push_str("Conversation History:\n");
            for msg in self.history {
                let role_label = match msg.role {
                    MessageRole::Human => "User",
                    MessageRole::Agent => "Agent",
                    MessageRole::System => "System",
                };
                history_str.push_str(&format!(
                    "[{} ({})]: {}\n",
                    role_label, msg.sender, msg.clean_text_body
                ));
            }
            history_str.push_str("\nLatest Inbound Message:\n");
        }

        format!(
            "{}{}{}{}",
            delivery_ctx, pipeline_ctx, history_str, self.prompt
        )
    }

    fn record_execution(
        &self,
        duration_ms: u64,
        token_usage: Option<&TokenUsage>,
        error_type: Option<String>,
    ) {
        let Some(ref monitoring) = self.monitoring else {
            return;
        };
        monitoring.record_ai_execution(&AiExecutionMetrics {
            company_id: self.company_id,
            channel_id: self.channel_id,
            agent_id: self.agent_id,
            provider: self.params.provider.clone(),
            model: self.params.model.clone(),
            prompt_tokens: token_usage.map_or(0, |t| t.prompt_tokens as usize),
            completion_tokens: token_usage.map_or(0, |t| t.completion_tokens as usize),
            total_tokens: token_usage.map_or(0, |t| t.total_tokens as usize),
            duration_ms,
            success: error_type.is_none(),
            error_type,
        });
    }
}

/// A configured agent run, owned by the spawned task.
struct AgentTask {
    config_yaml: String,
    provider_name: String,
    model_name: String,
    api_key: String,
    provider_config: ai_agents::llm::LLMConfig,
    base_url: Option<String>,
    tool_choice: Option<ai_agents::ToolChoice>,
    approval: Option<(Arc<ApprovalUseCases>, ApprovalContext)>,
    outreach: Option<(
        Arc<dyn TaskPersistence>,
        Arc<dyn ChannelPersistence>,
        OutreachToolContext,
    )>,
    /// Present when the run may list its sibling agent channels.
    agent_persistence: Option<Arc<dyn AgentPersistence>>,
    recipient_role: Option<RecipientRole>,
    full_prompt: String,
    history_message_count: usize,
    /// Tool policy for internal delegation, read from the merged agent config.
    internal_requires_approval: bool,
    /// Set by the approval handler or outreach tool when the run parks awaiting a human/other agent.
    suspended: Arc<AtomicBool>,
}

impl AgentTask {
    async fn run(self) -> anyhow::Result<AgentExecutionOutput> {
        let agent = self.build_agent().await?;

        let safe_prompt_log = sanitize_text(&self.full_prompt, Some(&self.api_key));
        info!(
            "Calling agent.chat | provider: '{}', model: '{}', api key set: '{}', prompt: '{}'",
            self.provider_name,
            self.model_name,
            !self.api_key.is_empty(),
            safe_prompt_log
        );

        let response = agent.chat(&self.full_prompt).await?;
        info!(
            "{}",
            sanitize_text(&format!("{:?}", response), Some(&self.api_key))
        );

        let clean_content = sanitize_text(&response.content, Some(&self.api_key));
        let counted = count_tokens(
            response.metadata.as_ref(),
            &self.full_prompt,
            &clean_content,
        );

        let clean_meta = response.metadata.as_ref().and_then(|meta| {
            let val = serde_json::to_value(meta).ok()?;
            let sanitized = sanitize_text(&val.to_string(), Some(&self.api_key));
            serde_json::from_str(&sanitized).ok().or(Some(val))
        });
        let observability_report = agent
            .observability()
            .map(|manager| serde_json::to_value(manager.generate_report()))
            .transpose()?;

        let tool_names: Vec<String> = response
            .tool_calls
            .as_ref()
            .map(|calls| calls.iter().map(|call| call.name.clone()).collect())
            .unwrap_or_default();
        let diagnostics = AgentExecutionDiagnostics {
            // Filled in by the caller, which owns the clock.
            duration_ms: 0,
            prompt_characters: self.full_prompt.chars().count(),
            response_characters: clean_content.chars().count(),
            history_message_count: self.history_message_count,
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
            disposition: if self.suspended.load(Ordering::SeqCst) {
                AgentExecutionDisposition::Suspended
            } else {
                AgentExecutionDisposition::Completed
            },
            metadata,
        })
    }

    async fn build_agent(&self) -> anyhow::Result<ai_agents::RuntimeAgent> {
        let mut builder = AgentBuilder::from_yaml(&self.config_yaml)?;

        if let Some((use_cases, context)) = self.approval.clone() {
            let delegation =
                self.outreach
                    .as_ref()
                    .map(|(_, channels, outreach)| InternalDelegationPolicy {
                        channel_persistence: channels.clone(),
                        app_domain_name: outreach.app_domain_name.clone(),
                        company_id: outreach.company_id,
                        source_channel_id: outreach.channel_id,
                        requires_approval: self.internal_requires_approval,
                    });
            builder = builder.approval_handler(Arc::new(AgentApprovalHandler {
                approval_use_cases: use_cases,
                context,
                suspended: self.suspended.clone(),
                delegation,
            }));
        }

        // An unrecognized provider name falls back to whatever the environment can configure.
        if let Ok(provider_type) = std::str::FromStr::from_str(&self.provider_name) {
            let mut provider = ai_agents::UnifiedLLMProvider::from_spec_config(
                provider_type,
                &self.model_name,
                Some(self.api_key.clone()),
                self.base_url.clone(),
                self.provider_config.clone(),
            )?;
            if let Some(choice) = self.tool_choice.clone() {
                provider = provider.with_tool_choice(choice);
            }
            builder = builder.llm(std::sync::Arc::new(provider));
        } else {
            builder = builder.auto_configure_llms()?;
        }

        let mut builder = builder
            .auto_configure_features()?
            .auto_configure_mcp()
            .await?
            .auto_configure_spawner()
            .await?;
        if let Some((task_persistence, channel_persistence, context)) = self.outreach.clone() {
            if let Some(agent_persistence) = self.agent_persistence.clone() {
                builder = builder.tool(Arc::new(ListCompanyAgentsTool::new(
                    channel_persistence.clone(),
                    agent_persistence,
                    AgentDirectoryContext {
                        company_id: context.company_id,
                        company_slug: context.company_slug.clone(),
                        source_channel_id: context.channel_id,
                        app_domain_name: context.app_domain_name.clone(),
                    },
                )));
            }
            builder = builder.tool(Arc::new(OutreachAndAwaitQuorumTool::new(
                task_persistence,
                channel_persistence,
                context,
                self.suspended.clone(),
            )));
        }

        let agent = builder.build()?;

        let role_str = self.recipient_role.map(|r| r.as_str()).unwrap_or("to");
        agent.set_context("recipient_role", serde_json::json!(role_str))?;
        agent.set_context("is_to", serde_json::json!(role_str == "to"))?;
        agent.set_context("is_cc", serde_json::json!(role_str == "cc"))?;
        Ok(agent)
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
mod tests {
    use super::*;
    use crate::entities::channel::Channel;

    #[tokio::test]
    async fn test_agent_runner_returns_error_when_provider_missing() -> anyhow::Result<()> {
        let result = ResolvedAgentParams::new(None, None, None);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("Agent provider is missing"));
        Ok(())
    }

    #[tokio::test]
    async fn test_agent_runner_returns_error_when_model_missing() -> anyhow::Result<()> {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test".into(),
            api_key: Some("key".to_string()),
            provider: Some("google".to_string()),
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };
        let result = ResolvedAgentParams::new(Some(&company), None, None);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("Agent model is missing"));
        Ok(())
    }

    #[tokio::test]
    async fn test_agent_runner_returns_error_when_api_key_missing() -> anyhow::Result<()> {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test".into(),
            api_key: None,
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };
        let result = ResolvedAgentParams::new(Some(&company), None, None);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("API key is missing"));
        Ok(())
    }

    #[tokio::test]
    async fn test_agent_runner_accepts_runtime_api_key_in_config() -> anyhow::Result<()> {
        let custom_config = serde_json::json!({
            "name": "CustomKeyAgent",
            "system_prompt": "You are a test assistant.",
            "llm": {
                "provider": "google",
                "model": "gemini-2.5-flash",
                "api_key": "custom_runtime_test_key"
            }
        });
        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: Some(custom_config),
            created_at: chrono::Utc::now(),
        };
        let params = ResolvedAgentParams::new(None, Some(&channel), None)?;

        let result = AgentRunner::new("Hello world", &params).execute().await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_agent_runner_accepts_api_key_provider_and_model() -> anyhow::Result<()> {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test".into(),
            api_key: Some("runtime_key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };
        let params = ResolvedAgentParams::new(Some(&company), None, None)?;
        let result = AgentRunner::new("Hello world", &params).execute().await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_agent_runner_uses_entity_model_override() -> anyhow::Result<()> {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test".into(),
            api_key: Some("fake_key_123".to_string()),
            provider: Some("google".to_string()),
            model: Some("invalid-custom-model-xyz".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };
        let params = ResolvedAgentParams::new(Some(&company), None, None)?;
        let result = AgentRunner::new("Hello world", &params).execute().await;
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_sanitize_text_hides_keys_in_urls_configs_and_messages() {
        let raw_url = "Fetched https://api.service.com/v1/data?key=12345SECRET&other=val and https://other.org/ep?api_key=98765SECRET";
        let sanitized_url = sanitize_text(raw_url, Some("12345SECRET"));
        assert!(!sanitized_url.contains("12345SECRET"));
        assert!(!sanitized_url.contains("98765SECRET"));
        assert!(sanitized_url.contains("key=[REDACTED]"));
        assert!(sanitized_url.contains("api_key=[REDACTED]"));

        let json_config = r#"{"llm": {"provider": "openai", "api_key": "my_secret_key_123"}}"#;
        let sanitized_json = sanitize_text(json_config, Some("my_secret_key_123"));
        assert!(!sanitized_json.contains("my_secret_key_123"));
        assert!(sanitized_json.contains(r#""api_key": "[REDACTED]""#));

        let yaml_config = "llm:\n  provider: google\n  api_key: secret_abc_123\n";
        let sanitized_yaml = sanitize_text(yaml_config, Some("secret_abc_123"));
        assert!(!sanitized_yaml.contains("secret_abc_123"));
        assert!(sanitized_yaml.contains("api_key: [REDACTED]"));

        let msg_with_secret = "The secret token is secret_abc_123.";
        let sanitized_msg = sanitize_text(msg_with_secret, Some("secret_abc_123"));
        assert_eq!(sanitized_msg, "The secret token is [REDACTED].");

        let err_with_url_key = "Failed to fetch URL https://api.service.com/v1/exec?api_key=SECRET_API_KEY_999: HTTP 500 Internal Server Error";
        let sanitized_err = sanitize_text(err_with_url_key, Some("SECRET_API_KEY_999"));
        assert!(!sanitized_err.contains("SECRET_API_KEY_999"));
        assert!(sanitized_err.contains("api_key=[REDACTED]"));

        let err_with_raw_key = "Authentication failed for key sk-proj-SECRET123456789";
        let sanitized_raw_err = sanitize_text(err_with_raw_key, Some("sk-proj-SECRET123456789"));
        assert!(!sanitized_raw_err.contains("sk-proj-SECRET123456789"));
        assert_eq!(
            sanitized_raw_err,
            "Authentication failed for key [REDACTED]"
        );
    }

    #[test]
    fn test_resolve_agent_params_company_only() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: Some("company-api-key".to_string()),
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), None, None).unwrap();
        assert_eq!(resolved.provider(), "google");
        assert_eq!(resolved.model(), "gemini-2.5-flash");
        assert_eq!(resolved.api_key(), "company-api-key");
        let mut expected = base_agent_config();
        merge_json(
            &mut expected,
            &serde_json::json!({
                "name": "Acme Corp",
                "system_prompt": format!(
                    "You are a helpful assistant.\n\n{BASE_CONTEXT_SYSTEM_PROMPT}"
                ),
                "llm": {
                    "provider": "google",
                    "model": "gemini-2.5-flash",
                    "api_key": "company-api-key",
                    "max_tokens": 8192
                }
            }),
        );
        assert_eq!(resolved.config(), &expected);
    }

    #[test]
    fn test_resolve_agent_params_channel_overrides_company() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: Some("company-api-key".to_string()),
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Channel".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: Some("channel-api-key".to_string()),
            provider: Some("openai".to_string()),
            model: None, // Should keep company's model
            participant_emails: None,
            agent_ids: None,
            channel_config: Some(serde_json::json!({
                "system_prompt": "Channel prompt",
                "temperature": 0.2
            })),
            created_at: chrono::Utc::now(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), Some(&channel), None).unwrap();
        assert_eq!(resolved.provider(), "openai"); // channel overridden
        assert_eq!(resolved.model(), "gemini-2.5-flash"); // Kept company
        assert_eq!(resolved.api_key(), "channel-api-key"); // channel overridden
        let mut expected = base_agent_config();
        merge_json(
            &mut expected,
            &serde_json::json!({
                "name": "Support Channel",
                "system_prompt": format!("Channel prompt\n\n{BASE_CONTEXT_SYSTEM_PROMPT}"),
                "temperature": 0.2,
                "llm": {
                    "provider": "openai",
                    "model": "gemini-2.5-flash",
                    "api_key": "channel-api-key",
                    "max_tokens": 8192
                }
            }),
        );
        assert_eq!(resolved.config(), &expected);
    }

    #[test]
    fn test_resolve_agent_params_agent_overrides_channel_and_company() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: Some("company-api-key".to_string()),
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Channel".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: Some("channel-api-key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            participant_emails: None,
            agent_ids: None,
            channel_config: Some(serde_json::json!({
                "system_prompt": "Channel prompt",
                "temperature": 0.2,
                "channel_only_field": true
            })),
            created_at: chrono::Utc::now(),
        };

        let agent = AgentEntity {
            id: Uuid::new_v4(),
            company_id: Some(company.id),
            name: "Tech Agent".to_string(),
            slug: "tech-agent".to_string(),
            provider: Some("anthropic".to_string()),
            model: Some("claude-3-5-sonnet".to_string()),
            api_key: Some("agent-api-key".to_string()),
            system_prompt: None,
            description: None,
            config_json: Some(serde_json::json!({
                "system_prompt": "Agent prompt",
                "temperature": 0.7
            })),
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let resolved =
            ResolvedAgentParams::new(Some(&company), Some(&channel), Some(&agent)).unwrap();
        assert_eq!(resolved.provider(), "anthropic"); // Agent overridden
        assert_eq!(resolved.model(), "claude-3-5-sonnet"); // Agent overridden
        assert_eq!(resolved.api_key(), "agent-api-key"); // Agent overridden
        let mut expected = base_agent_config();
        merge_json(
            &mut expected,
            &serde_json::json!({
                "name": "Tech Agent",
                "system_prompt": format!("Agent prompt\n\n{BASE_CONTEXT_SYSTEM_PROMPT}"),
                "temperature": 0.7,
                "channel_only_field": true,
                "llm": {
                    "provider": "anthropic",
                    "model": "claude-3-5-sonnet",
                    "api_key": "agent-api-key",
                    "max_tokens": 8192
                }
            }),
        );
        assert_eq!(resolved.config(), &expected);

        let (p, m, k, c) = resolved.into_tuple();
        assert_eq!(p, "anthropic");
        assert_eq!(m, "claude-3-5-sonnet");
        assert_eq!(k, "agent-api-key");
        assert!(c.is_object());
    }

    #[test]
    fn test_resolve_agent_params_handles_empty_or_whitespace_strings() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: Some("  ".to_string()),
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Channel".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: Some("".to_string()),
            provider: Some("   ".to_string()),
            model: Some("".to_string()),
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            created_at: chrono::Utc::now(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), Some(&channel), None);
        assert!(resolved.is_err());
        assert!(
            resolved
                .unwrap_err()
                .to_string()
                .contains("API key is missing")
        );
    }

    #[test]
    fn test_resolve_agent_params_validates_provider() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: Some("company-key".to_string()),
            provider: Some("unsupported_provider".to_string()),
            model: Some("some-model".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let err = ResolvedAgentParams::new(Some(&company), None, None).unwrap_err();
        assert!(
            err.to_string()
                .contains("Unsupported agent provider 'unsupported_provider'")
        );

        let company_groq = Company {
            provider: Some("groq".to_string()),
            ..company
        };
        let resolved = ResolvedAgentParams::new(Some(&company_groq), None, None).unwrap();
        assert_eq!(resolved.provider(), "groq");
    }

    #[test]
    fn test_resolve_agent_params_populates_missing_config_llm_fields() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: Some("company-key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Channel".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: Some(serde_json::json!({
                "llm": {
                    "provider": "openai"
                    // model and api_key missing in llm block
                }
            })),
            created_at: chrono::Utc::now(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), Some(&channel), None).unwrap();
        let config_llm = resolved.config().get("llm").unwrap().clone();
        assert_eq!(config_llm.get("provider").unwrap(), "openai");
        assert_eq!(config_llm.get("model").unwrap(), "gpt-4o");
        assert_eq!(config_llm.get("api_key").unwrap(), "company-key");
    }

    #[test]
    fn test_resolve_agent_params_uses_agent_system_prompt_when_config_system_prompt_empty() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: Some("company-key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let agent = AgentEntity {
            id: Uuid::new_v4(),
            company_id: Some(company.id),
            name: "Support Agent".to_string(),
            slug: "support-agent".to_string(),
            provider: None,
            model: None,
            api_key: None,
            system_prompt: Some("You are a helpful triage assistant.".to_string()),
            description: None,
            config_json: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), None, Some(&agent)).unwrap();
        let cfg = resolved.config();
        assert_eq!(
            cfg.get("system_prompt").unwrap().as_str().unwrap(),
            format!("You are a helpful triage assistant.\n\n{BASE_CONTEXT_SYSTEM_PROMPT}")
        );
    }

    #[test]
    fn test_resolve_agent_params_defaults_system_prompt_when_empty() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: Some("company-key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), None, None).unwrap();
        let cfg = resolved.config();
        assert_eq!(
            cfg.get("system_prompt").unwrap().as_str().unwrap(),
            format!("You are a helpful assistant.\n\n{BASE_CONTEXT_SYSTEM_PROMPT}")
        );
    }

    #[test]
    fn test_ensure_config_fields_populates_missing_keys() {
        let mut config = serde_json::json!({
            "temperature": 0.5
        });

        ensure_config_fields(
            &mut config,
            "openai",
            "gpt-4o",
            "sk-test-123",
            Some("Custom prompt"),
            Some("Custom Agent"),
        );

        assert_eq!(
            config.get("name").unwrap().as_str().unwrap(),
            "Custom Agent"
        );
        assert_eq!(
            config.get("system_prompt").unwrap().as_str().unwrap(),
            "Custom prompt"
        );
        let llm = config.get("llm").unwrap();
        assert_eq!(llm.get("provider").unwrap().as_str().unwrap(), "openai");
        assert_eq!(llm.get("model").unwrap().as_str().unwrap(), "gpt-4o");
        assert_eq!(llm.get("api_key").unwrap().as_str().unwrap(), "sk-test-123");
        assert_eq!(llm.get("max_tokens").unwrap().as_u64().unwrap(), 8192);
        assert_eq!(config.get("temperature").unwrap().as_f64().unwrap(), 0.5);
    }

    #[test]
    fn test_ensure_config_fields_preserves_max_tokens_override() {
        let mut config = serde_json::json!({
            "llm": {
                "max_tokens": 4096
            }
        });

        ensure_config_fields(&mut config, "openai", "gpt-4o", "sk-test-123", None, None);

        assert_eq!(config["llm"]["max_tokens"].as_u64().unwrap(), 4096);
    }

    #[test]
    fn test_provider_config_preserves_agent_llm_settings() {
        let config = serde_json::json!({
            "llm": {
                "provider": "google",
                "model": "gemini-2.5-flash",
                "api_key": "runtime-key",
                "temperature": 0.25,
                "max_tokens": 8192,
                "top_p": 0.8,
                "base_url": "https://example.test",
                "timeout_seconds": 45,
                "reasoning": true,
                "reasoning_budget_tokens": 1024
            }
        });

        let (provider_config, base_url, tool_choice) =
            provider_config_from_agent_config(&config).unwrap();

        assert_eq!(provider_config.temperature, Some(0.25));
        assert_eq!(provider_config.max_tokens, Some(8192));
        assert_eq!(provider_config.top_p, Some(0.8));
        assert_eq!(provider_config.timeout_seconds, Some(45));
        assert_eq!(provider_config.reasoning, Some(true));
        assert_eq!(provider_config.reasoning_budget_tokens, Some(1024));
        assert!(!provider_config.extra.contains_key("api_key"));
        assert_eq!(base_url.as_deref(), Some("https://example.test"));
        assert!(tool_choice.is_none());
    }

    #[test]
    fn test_base_agent_config_enables_private_in_memory_observability() {
        let config = base_agent_config_with_observability(true);

        assert_eq!(config["observability"]["enabled"], true);
        assert_eq!(config["observability"]["privacy"]["include_prompts"], false);
        assert_eq!(
            config["observability"]["privacy"]["include_responses"],
            false
        );
        assert_eq!(
            config["observability"]["privacy"]["include_tool_args"],
            false
        );
        assert_eq!(
            config["observability"]["privacy"]["include_tool_outputs"],
            false
        );
        assert_eq!(config["observability"]["export"]["write_report"], false);
        assert_eq!(config["observability"]["export"]["write_raw_events"], false);
    }

    #[test]
    fn test_base_agent_config_declares_delivery_context() {
        let config = base_agent_config_with_observability(false);

        for key in ["recipient_role", "is_to", "is_cc"] {
            assert_eq!(config["context"][key]["type"], "runtime");
            assert_eq!(config["context"][key]["required"], true);
        }
    }

    #[test]
    fn test_base_context_prompt_exposes_basic_runtime_information() {
        for variable in [
            "{{ context.time.date }}",
            "{{ context.time.time }}",
            "{{ context.agent_info.name }}",
            "{{ context.recipient_role }}",
            "{{ context.is_to }}",
            "{{ context.is_cc }}",
        ] {
            assert!(BASE_CONTEXT_SYSTEM_PROMPT.contains(variable));
        }
        assert!(!BASE_CONTEXT_SYSTEM_PROMPT.contains("context.session"));
    }

    #[test]
    fn test_agent_config_can_override_base_observability() {
        let mut config = base_agent_config_with_observability(true);
        merge_json(
            &mut config,
            &serde_json::json!({
                "observability": {
                    "enabled": false
                }
            }),
        );

        assert_eq!(config["observability"]["enabled"], false);
        assert_eq!(config["observability"]["privacy"]["include_prompts"], false);
    }

    #[test]
    fn test_attach_execution_diagnostics_preserves_ai_metadata() {
        let diagnostics = AgentExecutionDiagnostics {
            duration_ms: 123,
            prompt_characters: 1000,
            response_characters: 500,
            history_message_count: 3,
            token_usage_source: "estimated".to_string(),
            tool_call_count: 1,
            tool_names: vec!["search".to_string()],
        };

        let metadata = attach_execution_diagnostics(
            Some(serde_json::json!({ "reasoning": { "iterations": 1 } })),
            &diagnostics,
        )
        .unwrap();

        assert_eq!(metadata["reasoning"]["iterations"], 1);
        assert_eq!(metadata["execution_diagnostics"]["duration_ms"], 123);
        assert_eq!(
            metadata["execution_diagnostics"]["token_usage_source"],
            "estimated"
        );
        assert_eq!(metadata["execution_diagnostics"]["tool_names"][0], "search");
    }

    #[test]
    fn test_attach_observability_report_preserves_existing_metadata() {
        let metadata = attach_observability_report(
            Some(serde_json::json!({
                "execution_diagnostics": { "duration_ms": 123 }
            })),
            serde_json::json!({
                "summary": {
                    "total_events": 2,
                    "total_llm_calls": 1
                }
            }),
        )
        .unwrap();

        assert_eq!(metadata["execution_diagnostics"]["duration_ms"], 123);
        assert_eq!(metadata["observability"]["summary"]["total_events"], 2);
        assert_eq!(metadata["observability"]["summary"]["total_llm_calls"], 1);
    }

    #[test]
    fn test_estimate_tokens_and_token_usage() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("   "), 0);
        assert_eq!(estimate_tokens("Hello world"), 3); // 11 chars -> (11+3)/4 = 3

        let usage = TokenUsage::new(100, 50);
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn test_recipient_role_context_setting() {
        let config_yaml = r#"
name: TestAgent
system_prompt: Hello
"#;
        let mut builder = AgentBuilder::from_yaml(config_yaml).unwrap();
        let provider = ai_agents::UnifiedLLMProvider::new(
            ai_agents::LLMProviderType::OpenAI,
            "gpt-4o".to_string(),
            Some("test_key".to_string()),
            None,
        )
        .unwrap();
        builder = builder.llm(std::sync::Arc::new(provider));
        let agent = builder.build().unwrap();

        for (role, expected_role, expected_to, expected_cc) in [
            (Some(RecipientRole::To), "to", true, false),
            (Some(RecipientRole::Cc), "cc", false, true),
            (None, "to", true, false),
        ] {
            let role_str = role.map(|r| r.as_str()).unwrap_or("to");
            let is_to = role_str == "to";
            let is_cc = role_str == "cc";

            agent
                .set_context("recipient_role", serde_json::json!(role_str))
                .unwrap();
            agent
                .set_context("is_to", serde_json::json!(is_to))
                .unwrap();
            agent
                .set_context("is_cc", serde_json::json!(is_cc))
                .unwrap();

            let context = agent.get_context();
            assert_eq!(context["recipient_role"], serde_json::json!(expected_role));
            assert_eq!(context["is_to"], serde_json::json!(expected_to));
            assert_eq!(context["is_cc"], serde_json::json!(expected_cc));
            assert_eq!(role_str, expected_role);
            assert_eq!(is_to, expected_to);
            assert_eq!(is_cc, expected_cc);
        }
    }

    // --- internal delegation approval policy -------------------------------------------------

    struct DirectoryStub {
        channels: Vec<Channel>,
    }

    #[async_trait::async_trait]
    impl ChannelPersistence for DirectoryStub {
        async fn create(
            &self,
            _company_id: Uuid,
            _write: crate::use_cases::channel::ChannelWrite,
        ) -> crate::app_error::AppResult<Channel> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> crate::app_error::AppResult<Option<Channel>> {
            unimplemented!()
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &CompanySlug,
            channel_slug: &ChannelSlug,
        ) -> crate::app_error::AppResult<Option<Channel>> {
            Ok(self
                .channels
                .iter()
                .find(|c| &c.slug == channel_slug)
                .cloned())
        }
        async fn list_by_company_id(
            &self,
            _company_id: Uuid,
        ) -> crate::app_error::AppResult<Vec<Channel>> {
            Ok(self.channels.clone())
        }
        async fn update(
            &self,
            _id: Uuid,
            _write: crate::use_cases::channel::ChannelWrite,
        ) -> crate::app_error::AppResult<Channel> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> crate::app_error::AppResult<()> {
            unimplemented!()
        }
    }

    fn agent_channel(company_id: Uuid, slug: &str) -> Channel {
        Channel {
            id: Uuid::new_v4(),
            company_id,
            name: slug.to_string(),
            slug: slug.into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: Some(vec![Uuid::new_v4()]),
            channel_config: None,
            enabled: true,
            add_3rd_party: true,
            created_at: chrono::Utc::now(),
        }
    }

    fn policy(
        channels: Vec<Channel>,
        company_id: Uuid,
        requires_approval: bool,
    ) -> InternalDelegationPolicy {
        InternalDelegationPolicy {
            channel_persistence: Arc::new(DirectoryStub { channels }),
            app_domain_name: "mailagents.example".to_string(),
            company_id,
            source_channel_id: Uuid::new_v4(),
            requires_approval,
        }
    }

    fn outreach_trigger(targets: &[&str]) -> ai_agents::hitl::ApprovalTrigger {
        ai_agents::hitl::ApprovalTrigger::Tool {
            name: OUTREACH_TOOL_ID.to_string(),
            args: serde_json::json!({ "target_emails": targets }),
        }
    }

    #[tokio::test]
    async fn an_all_internal_call_skips_the_human_when_policy_allows() {
        let company_id = Uuid::new_v4();
        let policy = policy(
            vec![agent_channel(company_id, "billing")],
            company_id,
            false,
        );
        assert!(
            policy
                .approves_without_human(&outreach_trigger(&["billing@acme.mailagents.example"]))
                .await
        );
    }

    #[tokio::test]
    async fn an_external_recipient_still_requires_the_human() {
        let company_id = Uuid::new_v4();
        let policy = policy(
            vec![agent_channel(company_id, "billing")],
            company_id,
            false,
        );
        assert!(
            !policy
                .approves_without_human(&outreach_trigger(&["stranger@supplier.example"]))
                .await
        );
    }

    /// The case that justifies deciding per call instead of per tool: one stranger in the list
    /// must pull the whole call back under approval.
    #[tokio::test]
    async fn a_mixed_call_requires_the_human() {
        let company_id = Uuid::new_v4();
        let policy = policy(
            vec![agent_channel(company_id, "billing")],
            company_id,
            false,
        );
        assert!(
            !policy
                .approves_without_human(&outreach_trigger(&[
                    "billing@acme.mailagents.example",
                    "stranger@supplier.example",
                ]))
                .await
        );
    }

    #[tokio::test]
    async fn a_platform_address_with_no_such_channel_requires_the_human() {
        let company_id = Uuid::new_v4();
        let policy = policy(
            vec![agent_channel(company_id, "billing")],
            company_id,
            false,
        );
        assert!(
            !policy
                .approves_without_human(&outreach_trigger(&["ghost@acme.mailagents.example"]))
                .await
        );
    }

    #[tokio::test]
    async fn the_default_policy_never_skips_the_human() {
        let company_id = Uuid::new_v4();
        let policy = policy(vec![agent_channel(company_id, "billing")], company_id, true);
        assert!(
            !policy
                .approves_without_human(&outreach_trigger(&["billing@acme.mailagents.example"]))
                .await
        );
    }

    #[tokio::test]
    async fn an_empty_or_malformed_target_list_requires_the_human() {
        let company_id = Uuid::new_v4();
        let policy = policy(
            vec![agent_channel(company_id, "billing")],
            company_id,
            false,
        );
        assert!(!policy.approves_without_human(&outreach_trigger(&[])).await);
        assert!(
            !policy
                .approves_without_human(&ai_agents::hitl::ApprovalTrigger::Tool {
                    name: OUTREACH_TOOL_ID.to_string(),
                    args: serde_json::json!({}),
                })
                .await
        );
    }

    #[test]
    fn the_approval_flag_fails_closed_unless_explicitly_false() {
        let explicit = serde_json::json!({
            "tool_security": { "tools": { OUTREACH_TOOL_ID: {
                "config": { "internal_requires_approval": false } } } }
        });
        assert!(!internal_requires_approval(&explicit));

        assert!(internal_requires_approval(&serde_json::json!({})));
        let wrong_type = serde_json::json!({
            "tool_security": { "tools": { OUTREACH_TOOL_ID: {
                "config": { "internal_requires_approval": "false" } } } }
        });
        assert!(internal_requires_approval(&wrong_type));
    }
}
