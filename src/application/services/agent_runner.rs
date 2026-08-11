use crate::entities::approval::ApprovalStatus;
use crate::entities::message::{Message, MessageRole};
use crate::use_cases::approval::ApprovalUseCases;
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
                format!("tool:{}:{}", name, serde_json::to_string(args).unwrap_or_default())
            }
            ai_agents::hitl::ApprovalTrigger::Condition { name, matched } => {
                format!("condition:{}:{}", name, matched)
            }
            ai_agents::hitl::ApprovalTrigger::State { from, to } => {
                format!("state:{:?}:{}", from, to)
            }
        };

        let thread_str = self.context.thread_id.map(|t| t.to_string()).unwrap_or_default();
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
                    )
                }
                _ => {}
            }
        }

        // New HITL Step: Create DB record & send email with links
        let action_title = match &req.trigger {
            ai_agents::hitl::ApprovalTrigger::Tool { name, .. } => format!("Tool Execution: {}", name),
            ai_agents::hitl::ApprovalTrigger::Condition { name, .. } => {
                format!("Condition Approval: {}", name)
            }
            ai_agents::hitl::ApprovalTrigger::State { to, .. } => format!("State Transition: {}", to),
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

pub struct AgentRunner<'a> {
    prompt: &'a str,
    history: &'a [Message],
    workflow_config: Option<&'a serde_json::Value>,
    api_key: Option<&'a str>,
    provider: Option<&'a str>,
    model: Option<&'a str>,
    approval_use_cases: Option<Arc<ApprovalUseCases>>,
    approval_context: Option<ApprovalContext>,
}

impl<'a> AgentRunner<'a> {
    pub fn new(prompt: &'a str) -> Self {
        Self {
            prompt,
            history: &[],
            workflow_config: None,
            api_key: None,
            provider: None,
            model: None,
            approval_use_cases: None,
            approval_context: None,
        }
    }

    pub fn history(mut self, history: &'a [Message]) -> Self {
        self.history = history;
        self
    }

    pub fn workflow_config(mut self, config: Option<&'a serde_json::Value>) -> Self {
        self.workflow_config = config;
        self
    }

    pub fn api_key(mut self, key: Option<&'a str>) -> Self {
        self.api_key = key;
        self
    }

    pub fn provider(mut self, provider: Option<&'a str>) -> Self {
        self.provider = provider;
        self
    }

    pub fn model(mut self, model: Option<&'a str>) -> Self {
        self.model = model;
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

    fn default_config() -> serde_json::Value {
        serde_json::json!({
            "name": "MinimalAgent",
            "system_prompt": "You are a helpful assistant.",
            "llm": {
              "provider": "google",
              "model": "gemini-2.5-flash",
              "api_key": null
            }
        })
    }

    pub async fn execute(self) -> anyhow::Result<String> {
        info!(
            "Executing AI Agent with prompt length {} and history count {}",
            self.prompt.len(),
            self.history.len()
        );

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

        let raw_full_prompt = format!("{}{}", history_str, self.prompt);

        // Resolution order:
        // 1. Runtime entity overrides (`api_key`, `provider`, `model`)
        // 2. Explicit `llm.*` in workflow config
        // 3. Environment variables matching provider (for api_key)
        // 4. System defaults
        let provider_clean = self.provider.map(|s| s.trim()).filter(|s| !s.is_empty());
        let model_clean = self.model.map(|s| s.trim()).filter(|s| !s.is_empty());
        let api_key_clean = self.api_key.map(|s| s.trim()).filter(|s| !s.is_empty());

        let wf_llm = self.workflow_config.and_then(|c| c.get("llm"));

        let provider_name = provider_clean
            .or_else(|| {
                wf_llm
                    .and_then(|llm| llm.get("provider"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("google")
            .to_lowercase();

        let model_name = model_clean
            .or_else(|| {
                wf_llm
                    .and_then(|llm| llm.get("model"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("gemini-2.5-flash")
            .to_string();

        let resolved_api_key = api_key_clean
            .map(|s| s.to_string())
            .or_else(|| {
                wf_llm
                    .and_then(|llm| llm.get("api_key"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                let env_keys = match provider_name.as_str() {
                    "google" | "gemini" => vec!["GEMINI_API_KEY", "GOOGLE_API_KEY"],
                    "openai" => vec!["OPENAI_API_KEY"],
                    "anthropic" => vec!["ANTHROPIC_API_KEY"],
                    "groq" => vec!["GROQ_API_KEY"],
                    "mistral" => vec!["MISTRAL_API_KEY"],
                    _ => vec!["LLM_API_KEY", "API_KEY"],
                };
                env_keys
                    .into_iter()
                    .find_map(|var| std::env::var(var).ok().filter(|s| !s.trim().is_empty()))
            });

        // Check beforehand if api_key exists
        let key = match resolved_api_key {
            Some(k) => k,
            None => {
                tracing::warn!("API key is missing for provider '{}'", provider_name);
                return Err(anyhow::anyhow!(
                    "API key is missing for provider '{}'. Please configure an API key in workflow or company settings.",
                    provider_name
                ));
            }
        };

        // Construct final configuration with resolved provider, model, and api_key
        let mut final_config = self
            .workflow_config
            .cloned()
            .unwrap_or_else(Self::default_config);

        if !final_config.is_object() {
            final_config = serde_json::json!({});
        }

        if final_config.get("name").is_none() {
            final_config["name"] = serde_json::json!("MinimalAgent");
        }
        if final_config.get("system_prompt").is_none() {
            final_config["system_prompt"] = serde_json::json!("You are a helpful assistant.");
        }

        let llm_obj = final_config
            .as_object_mut()
            .unwrap()
            .entry("llm")
            .or_insert_with(|| serde_json::json!({}));

        if let Some(llm_map) = llm_obj.as_object_mut() {
            llm_map.insert("provider".to_string(), serde_json::json!(provider_name));
            llm_map.insert("model".to_string(), serde_json::json!(model_name));
            llm_map.insert("api_key".to_string(), serde_json::json!(key));
        }

        let config_yaml = serde_yaml::to_string(&final_config).unwrap_or_default();

        let full_prompt = sanitize_text(&raw_full_prompt, Some(&key));
        info!("Full prompt context length: {}", full_prompt.len());

        if !config_yaml.is_empty() {
            info!(
                "Running agent with workflow config YAML:\n{}",
                sanitize_text(&config_yaml, Some(&key))
            );
        }

        // Spawn on a separate Tokio task to prevent stack frame overflow on the caller thread
        let key_for_task = key.clone();
        let approval_use_cases = self.approval_use_cases.clone();
        let approval_context = self.approval_context.clone();

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
            Ok::<String, anyhow::Error>(clean_content)
        })
        .await;

        match task_result {
            Ok(Ok(content)) => Ok(content),
            Ok(Err(err)) => {
                let err_msg = sanitize_text(&err.to_string(), Some(&key));
                tracing::warn!("AI Agent execution failed ({err_msg})");
                Err(anyhow::anyhow!("{err_msg}"))
            }
            Err(join_err) => {
                let err_msg = sanitize_text(&join_err.to_string(), Some(&key));
                tracing::warn!("AI Agent task panicked or was cancelled ({err_msg})");
                Err(anyhow::anyhow!("Task failed: {err_msg}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_runner_returns_error_when_api_key_missing() -> anyhow::Result<()> {
        let result = AgentRunner::new("Hello world").execute().await;
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

        let result = AgentRunner::new("Hello world")
            .workflow_config(Some(&custom_config))
            .execute()
            .await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_agent_runner_accepts_api_key_provider_and_model() -> anyhow::Result<()> {
        let result = AgentRunner::new("Hello world")
            .api_key(Some("runtime_key"))
            .provider(Some("openai"))
            .model(Some("gpt-4o"))
            .execute()
            .await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_agent_runner_uses_entity_model_override() -> anyhow::Result<()> {
        let result = AgentRunner::new("Hello world")
            .api_key(Some("fake_key_123"))
            .model(Some("invalid-custom-model-xyz"))
            .execute()
            .await;
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
}
