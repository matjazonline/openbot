use crate::domain::monitoring::{AiExecutionMetrics, MonitoringService};
use crate::entities::agent::Agent as AgentEntity;
use crate::entities::approval::ApprovalStatus;
use crate::entities::company::Company;
use crate::entities::message::{Message, MessageRole};
use crate::entities::task::TokenUsage;
use crate::entities::workflow::Workflow;
use crate::use_cases::approval::ApprovalUseCases;
use crate::use_cases::thread::RecipientRole;
use ai_agents::{Agent, AgentBuilder};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::{Arc, LazyLock};
use tracing::info;
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedAgentParams {
    provider: String,
    model: String,
    api_key: String,
    config: serde_json::Value,
}

impl ResolvedAgentParams {
    /// Resolves LLM execution parameters by combining Company, Workflow, and Agent entities.
    /// Company values are overridden by Workflow and Workflow values are overridden by Agent.
    pub fn new(
        company: Option<&Company>,
        workflow: Option<&Workflow>,
        agent: Option<&AgentEntity>,
    ) -> anyhow::Result<Self> {
        let mut config = workflow.and_then(|w| w.workflow_config.clone());

        if let Some(agent_cfg) = agent.and_then(|a| a.config_json.as_ref()) {
            match config.as_mut() {
                Some(base_cfg) => {
                    if base_cfg.is_object() && agent_cfg.is_object() {
                        merge_json(base_cfg, agent_cfg);
                    } else {
                        config = Some(agent_cfg.clone());
                    }
                }
                None => {
                    config = Some(agent_cfg.clone());
                }
            }
        }

        let wf_llm = config.as_ref().and_then(|c| c.get("llm"));

        let provider = agent
            .and_then(|a| a.provider.as_deref())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                workflow
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
                workflow
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
                workflow
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
                    "API key is missing for provider '{}'. Please configure an API key in workflow or company settings.",
                    provider
                )
            })?;

        let mut config = config.unwrap_or_else(|| serde_json::json!({}));
        let fallback_sys_prompt = agent.and_then(|a| a.system_prompt.as_deref());
        let fallback_name = agent
            .map(|a| a.name.as_str())
            .or_else(|| workflow.map(|w| w.name.as_str()))
            .or_else(|| company.map(|c| c.name.as_str()));
        ensure_config_fields(
            &mut config,
            &provider,
            &model,
            &api_key,
            fallback_sys_prompt,
            fallback_name,
        );

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

#[derive(Clone, Debug)]
pub struct ApprovalContext {
    pub company_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub workflow_slug: String,
    pub company_slug: String,
    pub thread_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub approver_email: String,
}

pub struct AgentApprovalHandler {
    pub approval_use_cases: Arc<ApprovalUseCases>,
    pub context: ApprovalContext,
}

#[async_trait::async_trait]
impl ai_agents::hitl::ApprovalHandler for AgentApprovalHandler {
    async fn request_approval(
        &self,
        req: ai_agents::hitl::ApprovalRequest,
    ) -> ai_agents::hitl::ApprovalResult {
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
        let step_key = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}", thread_str, step_raw).as_bytes())
        );

        // Check if DB already has approval or rejection
        if let Ok(Some(status)) = self
            .approval_use_cases
            .check_step_approval(self.context.thread_id, &step_key)
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
                self.context.workflow_id,
                &self.context.workflow_name,
                &self.context.workflow_slug,
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
            Ok(_) => ai_agents::hitl::ApprovalResult::rejected_with_reason(
                "Approval requested via email link; task paused waiting for human decision.",
            ),
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
    workflow_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    skip_spam_guardrail: bool,
    recipient_role: Option<RecipientRole>,
    upstream_pipeline_context: Option<String>,
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
            workflow_id: None,
            agent_id: None,
            skip_spam_guardrail: false,
            recipient_role: None,
            upstream_pipeline_context: None,
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
        workflow_id: Option<Uuid>,
        agent_id: Option<Uuid>,
    ) -> Self {
        self.company_id = company_id;
        self.workflow_id = workflow_id;
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

    pub async fn execute(self) -> anyhow::Result<AgentExecutionOutput> {
        let start_time = std::time::Instant::now();
        info!(
            "Executing AI Agent with prompt length {} and history count {}",
            self.prompt.len(),
            self.history.len()
        );

        let delivery_ctx = match self.recipient_role {
            Some(RecipientRole::To) => "[Delivery Context: Email received via TO field (Primary Target)]\n",
            Some(RecipientRole::Cc) => "[Delivery Context: Email received via CC field (Secondary / FYI Target)]\n",
            None => "",
        };

        let pipeline_ctx_str = if let Some(ref upstream) = self.upstream_pipeline_context {
            if !upstream.trim().is_empty() {
                format!("[Upstream Pipeline Context from Prior Step Agents]:\n{}\n\n", upstream.trim())
            } else {
                String::new()
            }
        } else {
            String::new()
        };

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

        let raw_full_prompt = format!("{}{}{}{}", delivery_ctx, pipeline_ctx_str, history_str, self.prompt);

        let provider_name = &self.params.provider;
        let model_name = &self.params.model;
        let key = &self.params.api_key;

        // Stage 3: Optional LLM Spam & Guardrail Evaluation (skipped for trusted participants)
        if !self.skip_spam_guardrail {
            if let Some(ref cfg) = self.app_config {
                crate::services::llm_guardrail::LlmSpamGuardrail::evaluate(
                    cfg,
                    self.company.as_ref(),
                    self.monitoring.as_ref(),
                    &raw_full_prompt,
                    provider_name,
                    model_name,
                    key,
                )
                .await?;
            }
        }

        let mut config = self.params.config.clone();
        ensure_config_fields(&mut config, provider_name, model_name, key, None, None);

        let role_str = self.recipient_role.map(|r| r.as_str()).unwrap_or("to");
        let is_to_str = if role_str == "to" { "true" } else { "false" };
        let is_cc_str = if role_str == "cc" { "true" } else { "false" };

        if let Some(sp) = config.get_mut("system_prompt") {
            if let Some(s) = sp.as_str() {
                let replaced = s
                    .replace("{{recipient_role}}", role_str)
                    .replace("{{is_to}}", is_to_str)
                    .replace("{{is_cc}}", is_cc_str);
                *sp = serde_json::json!(replaced);
            }
        }

        let config_yaml = serde_yaml::to_string(&config).unwrap_or_default();

        let full_prompt = sanitize_text(&raw_full_prompt, Some(key));
        info!("Full prompt context length: {}", full_prompt.len());

        if !config_yaml.is_empty() {
            info!(
                "Running agent with workflow config YAML:\n{}",
                sanitize_text(&config_yaml, Some(key))
            );
        }

        // Spawn on a separate Tokio task to prevent stack frame overflow on the caller thread
        let key_for_task = key.clone();
        let provider_name = provider_name.to_string();
        let model_name = model_name.to_string();
        let approval_use_cases = self.approval_use_cases.clone();
        let approval_context = self.approval_context.clone();
        let recipient_role = self.recipient_role;

        let task_result = tokio::spawn(async move {
            let mut builder = AgentBuilder::from_yaml(&config_yaml)?;

            if let (Some(use_cases), Some(ctx)) = (approval_use_cases, approval_context) {
                let handler = Arc::new(AgentApprovalHandler {
                    approval_use_cases: use_cases,
                    context: ctx,
                });
                builder = builder.approval_handler(handler);
            }

            if let Ok(provider_type) = std::str::FromStr::from_str(&provider_name) {
                let provider = ai_agents::UnifiedLLMProvider::new(
                    provider_type,
                    model_name.clone(),
                    Some(key_for_task.clone()),
                    None,
                )?;
                builder = builder.llm(std::sync::Arc::new(provider));
            } else {
                builder = builder.auto_configure_llms()?;
            }

            let agent = builder
                .auto_configure_features()?
                .auto_configure_mcp()
                .await?
                .auto_configure_spawner()
                .await?
                .build()?;

            let role_str = recipient_role.map(|r| r.as_str()).unwrap_or("to");
            let is_to = role_str == "to";
            let is_cc = role_str == "cc";

            agent.set_context("recipient_role", serde_json::json!(role_str))?;
            agent.set_context("is_to", serde_json::json!(is_to))?;
            agent.set_context("is_cc", serde_json::json!(is_cc))?;

            let safe_prompt_log = sanitize_text(&full_prompt, Some(&key_for_task));
            info!(
                "Calling agent.chat | provider: '{}', model: '{}', api key set: '{}', prompt: '{}'",
                provider_name,
                model_name,
                !key_for_task.is_empty(),
                safe_prompt_log
            );

            let response = agent.chat(&full_prompt).await?;
            let safe_response_log = sanitize_text(&format!("{:?}", response), Some(&key_for_task));
            info!("{}", safe_response_log);

            let clean_content = sanitize_text(&response.content, Some(&key_for_task));

            let mut prompt_tokens = 0;
            let mut completion_tokens = 0;

            if let Some(ref meta) = response.metadata {
                let parse_val = |v: &serde_json::Value| -> Option<usize> {
                    v.as_u64()
                        .map(|n| n as usize)
                        .or_else(|| v.as_str().and_then(|s| s.parse::<usize>().ok()))
                };

                if let Some(p) = meta.get("prompt_tokens").or_else(|| meta.get("input_tokens")).and_then(parse_val) {
                    prompt_tokens = p;
                }
                if let Some(c) = meta.get("completion_tokens").or_else(|| meta.get("output_tokens")).and_then(parse_val) {
                    completion_tokens = c;
                }

                if prompt_tokens == 0 && completion_tokens == 0 {
                    if let Some(usage) = meta.get("usage") {
                        if let Some(p) = usage.get("prompt_tokens").or_else(|| usage.get("input_tokens")).and_then(parse_val) {
                            prompt_tokens = p;
                        }
                        if let Some(c) = usage.get("completion_tokens").or_else(|| usage.get("output_tokens")).and_then(parse_val) {
                            completion_tokens = c;
                        }
                    }
                }
            }

            if prompt_tokens == 0 {
                prompt_tokens = estimate_tokens(&full_prompt);
            }
            if completion_tokens == 0 {
                completion_tokens = estimate_tokens(&clean_content);
            }

            let token_usage = TokenUsage::new(prompt_tokens, completion_tokens);

            Ok::<AgentExecutionOutput, anyhow::Error>(AgentExecutionOutput {
                content: clean_content,
                token_usage,
            })
        })
        .await;

        let duration_ms = start_time.elapsed().as_millis() as u64;

        match task_result {
            Ok(Ok(output)) => {
                if let Some(ref m) = self.monitoring {
                    m.record_ai_execution(&AiExecutionMetrics {
                        company_id: self.company_id,
                        workflow_id: self.workflow_id,
                        agent_id: self.agent_id,
                        provider: self.params.provider.clone(),
                        model: self.params.model.clone(),
                        prompt_tokens: output.token_usage.prompt_tokens as usize,
                        completion_tokens: output.token_usage.completion_tokens as usize,
                        total_tokens: output.token_usage.total_tokens as usize,
                        duration_ms,
                        success: true,
                        error_type: None,
                    });
                }
                Ok(output)
            }
            Ok(Err(err)) => {
                let err_msg = sanitize_text(&err.to_string(), Some(&key));
                tracing::warn!("AI Agent execution failed ({err_msg})");
                if let Some(ref m) = self.monitoring {
                    m.record_ai_execution(&AiExecutionMetrics {
                        company_id: self.company_id,
                        workflow_id: self.workflow_id,
                        agent_id: self.agent_id,
                        provider: self.params.provider.clone(),
                        model: self.params.model.clone(),
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                        duration_ms,
                        success: false,
                        error_type: Some(err_msg.clone()),
                    });
                }
                Err(anyhow::anyhow!("{err_msg}"))
            }
            Err(join_err) => {
                let err_msg = sanitize_text(&join_err.to_string(), Some(&key));
                tracing::warn!("AI Agent task panicked or was cancelled ({err_msg})");
                if let Some(ref m) = self.monitoring {
                    m.record_ai_execution(&AiExecutionMetrics {
                        company_id: self.company_id,
                        workflow_id: self.workflow_id,
                        agent_id: self.agent_id,
                        provider: self.params.provider.clone(),
                        model: self.params.model.clone(),
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                        duration_ms,
                        success: false,
                        error_type: Some(format!("Panicked or cancelled: {}", err_msg)),
                    });
                }
                Err(anyhow::anyhow!("Task failed: {err_msg}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            slug: "test".to_string(),
            api_key: Some("key".to_string()),
            provider: Some("google".to_string()),
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
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
            slug: "test".to_string(),
            api_key: None,
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
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
        let workflow = Workflow {
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test".to_string(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            workflow_config: Some(custom_config),
            created_at: chrono::Utc::now().naive_utc(),
        };
        let params = ResolvedAgentParams::new(None, Some(&workflow), None)?;

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
            slug: "test".to_string(),
            api_key: Some("runtime_key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
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
            slug: "test".to_string(),
            api_key: Some("fake_key_123".to_string()),
            provider: Some("google".to_string()),
            model: Some("invalid-custom-model-xyz".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
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
            slug: "acme".to_string(),
            api_key: Some("company-api-key".to_string()),
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), None, None).unwrap();
        assert_eq!(resolved.provider(), "google");
        assert_eq!(resolved.model(), "gemini-2.5-flash");
        assert_eq!(resolved.api_key(), "company-api-key");
        assert_eq!(
            resolved.config(),
            &serde_json::json!({
                "name": "Acme Corp",
                "system_prompt": "You are a helpful assistant.",
                "llm": {
                    "provider": "google",
                    "model": "gemini-2.5-flash",
                    "api_key": "company-api-key"
                }
            })
        );
    }

    #[test]
    fn test_resolve_agent_params_workflow_overrides_company() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".to_string(),
            api_key: Some("company-api-key".to_string()),
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let workflow = Workflow {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Workflow".to_string(),
            slug: "support".to_string(),
            api_key: Some("workflow-api-key".to_string()),
            provider: Some("openai".to_string()),
            model: None, // Should keep company's model
            participant_emails: None,
            agent_ids: None,
            workflow_config: Some(serde_json::json!({
                "system_prompt": "Workflow prompt",
                "temperature": 0.2
            })),
            created_at: chrono::Utc::now().naive_utc(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), Some(&workflow), None).unwrap();
        assert_eq!(resolved.provider(), "openai"); // Workflow overridden
        assert_eq!(resolved.model(), "gemini-2.5-flash"); // Kept company
        assert_eq!(resolved.api_key(), "workflow-api-key"); // Workflow overridden
        assert_eq!(
            resolved.config(),
            &serde_json::json!({
                "name": "Support Workflow",
                "system_prompt": "Workflow prompt",
                "temperature": 0.2,
                "llm": {
                    "provider": "openai",
                    "model": "gemini-2.5-flash",
                    "api_key": "workflow-api-key"
                }
            })
        );
    }

    #[test]
    fn test_resolve_agent_params_agent_overrides_workflow_and_company() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".to_string(),
            api_key: Some("company-api-key".to_string()),
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let workflow = Workflow {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Workflow".to_string(),
            slug: "support".to_string(),
            api_key: Some("workflow-api-key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            participant_emails: None,
            agent_ids: None,
            workflow_config: Some(serde_json::json!({
                "system_prompt": "Workflow prompt",
                "temperature": 0.2,
                "workflow_only_field": true
            })),
            created_at: chrono::Utc::now().naive_utc(),
        };

        let agent = AgentEntity {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Tech Agent".to_string(),
            slug: "tech-agent".to_string(),
            provider: Some("anthropic".to_string()),
            model: Some("claude-3-5-sonnet".to_string()),
            api_key: Some("agent-api-key".to_string()),
            system_prompt: None,
            config_json: Some(serde_json::json!({
                "system_prompt": "Agent prompt",
                "temperature": 0.7
            })),
            created_at: chrono::Utc::now().naive_utc(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), Some(&workflow), Some(&agent)).unwrap();
        assert_eq!(resolved.provider(), "anthropic"); // Agent overridden
        assert_eq!(resolved.model(), "claude-3-5-sonnet"); // Agent overridden
        assert_eq!(resolved.api_key(), "agent-api-key"); // Agent overridden
        assert_eq!(
            resolved.config(),
            &serde_json::json!({
                "name": "Tech Agent",
                "system_prompt": "Agent prompt", // Agent overridden
                "temperature": 0.7,             // Agent overridden
                "workflow_only_field": true,     // Merged from Workflow
                "llm": {
                    "provider": "anthropic",
                    "model": "claude-3-5-sonnet",
                    "api_key": "agent-api-key"
                }
            })
        );

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
            slug: "acme".to_string(),
            api_key: Some("  ".to_string()),
            provider: Some("google".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let workflow = Workflow {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Workflow".to_string(),
            slug: "support".to_string(),
            api_key: Some("".to_string()),
            provider: Some("   ".to_string()),
            model: Some("".to_string()),
            participant_emails: None,
            agent_ids: None,
            workflow_config: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), Some(&workflow), None);
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
            slug: "acme".to_string(),
            api_key: Some("company-key".to_string()),
            provider: Some("unsupported_provider".to_string()),
            model: Some("some-model".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
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
            slug: "acme".to_string(),
            api_key: Some("company-key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let workflow = Workflow {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Workflow".to_string(),
            slug: "support".to_string(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            workflow_config: Some(serde_json::json!({
                "llm": {
                    "provider": "openai"
                    // model and api_key missing in llm block
                }
            })),
            created_at: chrono::Utc::now().naive_utc(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), Some(&workflow), None).unwrap();
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
            slug: "acme".to_string(),
            api_key: Some("company-key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let agent = AgentEntity {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Agent".to_string(),
            slug: "support-agent".to_string(),
            provider: None,
            model: None,
            api_key: None,
            system_prompt: Some("You are a helpful triage assistant.".to_string()),
            config_json: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), None, Some(&agent)).unwrap();
        let cfg = resolved.config();
        assert_eq!(
            cfg.get("system_prompt").unwrap().as_str().unwrap(),
            "You are a helpful triage assistant."
        );
    }

    #[test]
    fn test_resolve_agent_params_defaults_system_prompt_when_empty() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".to_string(),
            api_key: Some("company-key".to_string()),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            enable_llm_spam_guardrail: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let resolved = ResolvedAgentParams::new(Some(&company), None, None).unwrap();
        let cfg = resolved.config();
        assert_eq!(
            cfg.get("system_prompt").unwrap().as_str().unwrap(),
            "You are a helpful assistant."
        );
    }

    #[test]
    fn test_ensure_config_fields_populates_missing_keys() {
        let mut config = serde_json::json!({
            "temperature": 0.5
        });

        ensure_config_fields(&mut config, "openai", "gpt-4o", "sk-test-123", Some("Custom prompt"), Some("Custom Agent"));

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
        assert_eq!(config.get("temperature").unwrap().as_f64().unwrap(), 0.5);
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
        ).unwrap();
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

            agent.set_context("recipient_role", serde_json::json!(role_str)).unwrap();
            agent.set_context("is_to", serde_json::json!(is_to)).unwrap();
            agent.set_context("is_cc", serde_json::json!(is_cc)).unwrap();

            assert_eq!(role_str, expected_role);
            assert_eq!(is_to, expected_to);
            assert_eq!(is_cc, expected_cc);
        }
    }
}
