use std::sync::Arc;

use ai_agents::Agent as _;
use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::agent::Agent,
    use_cases::{channel::validate_slug, company::CompanyPersistence},
};

#[async_trait]
pub trait AgentPersistence: Send + Sync {
    async fn create(
        &self,
        company_id: Uuid,
        name: &str,
        slug: &str,
        provider: Option<&str>,
        model: Option<&str>,
        api_key: Option<&str>,
        system_prompt: Option<&str>,
        config_json: Option<serde_json::Value>,
    ) -> AppResult<Agent>;

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Agent>>;

    async fn get_by_company_slug_and_agent_slug(
        &self,
        company_slug: &str,
        agent_slug: &str,
    ) -> AppResult<Option<Agent>>;

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Agent>>;

    async fn update(
        &self,
        id: Uuid,
        name: &str,
        slug: &str,
        provider: Option<&str>,
        model: Option<&str>,
        api_key: Option<&str>,
        system_prompt: Option<&str>,
        config_json: Option<serde_json::Value>,
    ) -> AppResult<Agent>;

    async fn delete(&self, id: Uuid) -> AppResult<()>;
}

#[derive(Clone)]
pub struct AgentUseCases {
    company_persistence: Arc<dyn CompanyPersistence>,
    agent_persistence: Arc<dyn AgentPersistence>,
}

impl AgentUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        agent_persistence: Arc<dyn AgentPersistence>,
    ) -> Self {
        Self {
            company_persistence,
            agent_persistence,
        }
    }

    async fn verify_company_owner(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        let company = self
            .company_persistence
            .get_by_id(company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Company not found.".into()))?;

        if company.user_id != user_id {
            return Err(AppError::Internal(
                "Unauthorized: only the company owner can manage agents.".into(),
            ));
        }

        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn create_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        name: &str,
        slug: &str,
        provider: Option<&str>,
        model: Option<&str>,
        api_key: Option<&str>,
        system_prompt: Option<&str>,
        config_json: Option<serde_json::Value>,
    ) -> AppResult<Agent> {
        self.verify_company_owner(user_id, company_id).await?;

        let name_trimmed = name.trim();
        let slug_clean = slug.trim().to_lowercase().replace(' ', "-");

        if name_trimmed.is_empty() || slug_clean.is_empty() {
            return Err(AppError::Internal(
                "Agent name and slug cannot be empty.".into(),
            ));
        }

        validate_slug(&slug_clean)?;

        let provider_clean = provider.map(|s| s.trim()).filter(|s| !s.is_empty());
        let model_clean = model.map(|s| s.trim()).filter(|s| !s.is_empty());
        let api_key_clean = api_key.map(|s| s.trim()).filter(|s| !s.is_empty());
        let system_prompt_clean = system_prompt.map(|s| s.trim()).filter(|s| !s.is_empty());

        info!(
            "Creating agent '{}' ({}) for company {}",
            name_trimmed, slug_clean, company_id
        );

        self.agent_persistence
            .create(
                company_id,
                name_trimmed,
                &slug_clean,
                provider_clean,
                model_clean,
                api_key_clean,
                system_prompt_clean,
                config_json,
            )
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_company_agents(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Agent>> {
        self.verify_company_owner(user_id, company_id).await?;
        self.agent_persistence.list_by_company_id(company_id).await
    }

    #[instrument(skip(self))]
    pub async fn get_company_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<Agent>> {
        self.verify_company_owner(user_id, company_id).await?;
        let agent = self.agent_persistence.get_by_id(agent_id).await?;
        if let Some(ref ag) = agent {
            if ag.company_id != company_id {
                return Ok(None);
            }
        }
        Ok(agent)
    }

    #[instrument(skip(self))]
    pub async fn update_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agent_id: Uuid,
        name: &str,
        slug: &str,
        provider: Option<&str>,
        model: Option<&str>,
        api_key: Option<&str>,
        system_prompt: Option<&str>,
        config_json: Option<serde_json::Value>,
    ) -> AppResult<Agent> {
        self.verify_company_owner(user_id, company_id).await?;

        let agent = self
            .agent_persistence
            .get_by_id(agent_id)
            .await?
            .ok_or_else(|| AppError::Internal("Agent not found.".into()))?;

        if agent.company_id != company_id {
            return Err(AppError::Internal(
                "Agent does not belong to this company.".into(),
            ));
        }

        let name_trimmed = name.trim();
        let slug_clean = slug.trim().to_lowercase().replace(' ', "-");

        if name_trimmed.is_empty() || slug_clean.is_empty() {
            return Err(AppError::Internal(
                "Agent name and slug cannot be empty.".into(),
            ));
        }

        validate_slug(&slug_clean)?;

        let provider_clean = provider.map(|s| s.trim()).filter(|s| !s.is_empty());
        let model_clean = model.map(|s| s.trim()).filter(|s| !s.is_empty());
        let api_key_clean = api_key.map(|s| s.trim()).filter(|s| !s.is_empty());
        let system_prompt_clean = system_prompt.map(|s| s.trim()).filter(|s| !s.is_empty());

        info!(
            "Updating agent {} for company {}: {} ({})",
            agent_id, company_id, name_trimmed, slug_clean
        );

        self.agent_persistence
            .update(
                agent_id,
                name_trimmed,
                &slug_clean,
                provider_clean,
                model_clean,
                api_key_clean,
                system_prompt_clean,
                config_json,
            )
            .await
    }

    #[instrument(skip(self))]
    pub async fn delete_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_owner(user_id, company_id).await?;

        let agent = self
            .agent_persistence
            .get_by_id(agent_id)
            .await?
            .ok_or_else(|| AppError::Internal("Agent not found.".into()))?;

        if agent.company_id != company_id {
            return Err(AppError::Internal(
                "Agent does not belong to this company.".into(),
            ));
        }

        info!(
            "Deleting agent {} for company {}",
            agent_id, company_id
        );
        self.agent_persistence.delete(agent_id).await
    }

    #[instrument(skip(self))]
    pub async fn generate_system_prompt(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        instructions: &str,
        provider_override: Option<&str>,
        model_override: Option<&str>,
        api_key_override: Option<&str>,
    ) -> AppResult<String> {
        self.verify_company_owner(user_id, company_id).await?;

        let company = self
            .company_persistence
            .get_by_id(company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Company not found.".into()))?;

        let provider_opt = provider_override
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| company.provider.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()));

        let provider = provider_opt
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| {
                if std::env::var("OPENAI_API_KEY").ok().filter(|s| !s.trim().is_empty()).is_some() {
                    "openai".to_string()
                } else if std::env::var("ANTHROPIC_API_KEY").ok().filter(|s| !s.trim().is_empty()).is_some() {
                    "anthropic".to_string()
                } else if std::env::var("GROQ_API_KEY").ok().filter(|s| !s.trim().is_empty()).is_some() {
                    "groq".to_string()
                } else {
                    "google".to_string()
                }
            });

        let default_model = match provider.as_str() {
            "google" | "gemini" => "gemini-2.5-flash",
            "openai" => "gpt-4o",
            "anthropic" => "claude-3-5-sonnet-20241022",
            "groq" => "llama-3.3-70b-versatile",
            _ => "gemini-2.5-flash",
        };

        let model = model_override
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| company.model.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()))
            .unwrap_or(default_model)
            .to_string();

        let env_key = match provider.as_str() {
            "google" | "gemini" => std::env::var("GEMINI_API_KEY").ok().or_else(|| std::env::var("GOOGLE_API_KEY").ok()),
            "openai" => std::env::var("OPENAI_API_KEY").ok(),
            "anthropic" => std::env::var("ANTHROPIC_API_KEY").ok(),
            "groq" => std::env::var("GROQ_API_KEY").ok(),
            _ => std::env::var("LLM_API_KEY").ok().or_else(|| std::env::var("API_KEY").ok()),
        };

        let api_key = api_key_override
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| company.api_key.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()))
            .or_else(|| env_key.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()))
            .ok_or_else(|| AppError::Internal(format!(
                "API key is missing for provider '{}'. Please configure an API key in company settings or in the form.",
                provider
            )))?
            .to_string();

        let prepared_system_prompt = "\
You are an expert AI prompt engineer specializing in crafting system prompts for autonomous AI agents.
Your task is to generate a comprehensive, clear, structured, and production-ready system prompt based on the user's instructions.

Guidelines:
- Define a clear role and primary objective for the agent.
- Outline specific instructions, guidelines, constraints, and tone of communication.
- Keep the prompt structured, concise, and unambiguous.
- Output ONLY the system prompt text itself. Do NOT include any intro/outro explanations, conversational filler, or markdown code blocks (```).";

        let indented_system_prompt = prepared_system_prompt
            .lines()
            .map(|line| if line.is_empty() { String::new() } else { format!("  {}", line) })
            .collect::<Vec<_>>()
            .join("\n");

        let config_yaml = format!(
            "name: prompt_generator\nsystem_prompt: |\n{}\nllm:\n  provider: {}\n  model: {}\n  api_key: {}",
            indented_system_prompt, provider, model, api_key
        );

        let mut builder = ai_agents::AgentBuilder::from_yaml(&config_yaml)
            .map_err(|e| AppError::Internal(format!("Failed to parse agent builder config: {e}")))?;

        if let Ok(provider_type) = std::str::FromStr::from_str(&provider) {
            let unified_provider = ai_agents::UnifiedLLMProvider::new(
                provider_type,
                model.clone(),
                Some(api_key.clone()),
                None,
            )
            .map_err(|e| AppError::Internal(format!("Failed to initialize LLM provider: {e}")))?;
            builder = builder.llm(Arc::new(unified_provider));
        } else {
            builder = builder
                .auto_configure_llms()
                .map_err(|e| AppError::Internal(format!("Failed to configure LLMs: {e}")))?;
        }

        let agent = builder
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build AI agent: {e}")))?;

        info!(
            "Calling generate_system_prompt AI model | provider: '{}', model: '{}'",
            provider, model
        );

        let response = agent
            .chat(instructions)
            .await
            .map_err(|e| AppError::Internal(format!("AI generation call failed: {e}")))?;

        let generated = response
            .content
            .trim()
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string();

        Ok(generated)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use chrono::Utc;
    use serde_json::json;
    use crate::entities::company::Company;
    use super::*;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>, _enable_llm_spam_guardrail: Option<bool>) -> AppResult<Company> {
            unimplemented!()
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
                .cloned())
        }

        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug.eq_ignore_ascii_case(slug))
                .cloned())
        }

        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }

        async fn update(&self, _id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>, _enable_llm_spam_guardrail: Option<bool>) -> AppResult<Company> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }

        async fn is_company_team_member(&self, _company_id: Uuid, _email: &str) -> AppResult<bool> {
            Ok(true)
        }

        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }
    }

    struct MockAgentPersistence {
        agents: Mutex<Vec<Agent>>,
    }

    #[async_trait]
    impl AgentPersistence for MockAgentPersistence {
        async fn create(
            &self,
            company_id: Uuid,
            name: &str,
            slug: &str,
            provider: Option<&str>,
            model: Option<&str>,
            api_key: Option<&str>,
            system_prompt: Option<&str>,
            config_json: Option<serde_json::Value>,
        ) -> AppResult<Agent> {
            let agent = Agent {
                id: Uuid::new_v4(),
                company_id,
                name: name.to_string(),
                slug: slug.to_string(),
                provider: provider.map(|s| s.to_string()),
                model: model.map(|s| s.to_string()),
                api_key: api_key.map(|s| s.to_string()),
                system_prompt: system_prompt.map(|s| s.to_string()),
                config_json,
                created_at: Utc::now().naive_utc(),
            };
            self.agents.lock().unwrap().push(agent.clone());
            Ok(agent)
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Agent>> {
            Ok(self
                .agents
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.id == id)
                .cloned())
        }

        async fn get_by_company_slug_and_agent_slug(
            &self,
            _company_slug: &str,
            agent_slug: &str,
        ) -> AppResult<Option<Agent>> {
            Ok(self
                .agents
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.slug.eq_ignore_ascii_case(agent_slug))
                .cloned())
        }

        async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Agent>> {
            Ok(self
                .agents
                .lock()
                .unwrap()
                .iter()
                .filter(|a| a.company_id == company_id)
                .cloned()
                .collect())
        }

        async fn update(
            &self,
            id: Uuid,
            name: &str,
            slug: &str,
            provider: Option<&str>,
            model: Option<&str>,
            api_key: Option<&str>,
            system_prompt: Option<&str>,
            config_json: Option<serde_json::Value>,
        ) -> AppResult<Agent> {
            let mut list = self.agents.lock().unwrap();
            let agent = list
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;

            agent.name = name.to_string();
            agent.slug = slug.to_string();
            agent.provider = provider.map(|s| s.to_string());
            agent.model = model.map(|s| s.to_string());
            agent.api_key = api_key.map(|s| s.to_string());
            agent.system_prompt = system_prompt.map(|s| s.to_string());
            agent.config_json = config_json;
            Ok(agent.clone())
        }

        async fn delete(&self, id: Uuid) -> AppResult<()> {
            self.agents.lock().unwrap().retain(|a| a.id != id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn company_owner_agent_crud_flow_works() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let agent_persistence = Arc::new(MockAgentPersistence {
            agents: Mutex::new(Vec::new()),
        });

        let use_cases = AgentUseCases::new(company_persistence, agent_persistence);

        // 0. Invalid reserved suffix slug rejection test
        let invalid_res = use_cases
            .create_agent(
                owner_id,
                company_id,
                "Quiet Bot",
                "quiet",
                None,
                None,
                None,
                None,
                None,
            )
            .await;
        assert!(invalid_res.is_err());

        // 1. Owner creates agent with config_json
        let config = json!({ "temperature": 0.7, "system_prompt": "Hello" });

        let agent = use_cases
            .create_agent(
                owner_id,
                company_id,
                "Support Bot",
                "support-bot",
                Some("openai"),
                Some("gpt-4o"),
                Some("key_123"),
                Some("Prompt"),
                Some(config.clone()),
            )
            .await
            .unwrap();

        assert_eq!(agent.name, "Support Bot");
        assert_eq!(agent.slug, "support-bot");
        assert_eq!(agent.provider.as_deref(), Some("openai"));
        assert_eq!(agent.model.as_deref(), Some("gpt-4o"));
        assert_eq!(agent.api_key.as_deref(), Some("key_123"));
        assert_eq!(agent.system_prompt.as_deref(), Some("Prompt"));
        assert_eq!(agent.config_json, Some(config));

        // 2. Non-owner cannot create agent
        let non_owner_id = Uuid::new_v4();
        let err = use_cases
            .create_agent(
                non_owner_id,
                company_id,
                "Hacker Bot",
                "hacker-bot",
                None,
                None,
                None,
                None,
                None,
            )
            .await;
        assert!(err.is_err());

        // 3. List agents for company
        let list = use_cases
            .list_company_agents(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        // 4. Update agent
        let updated_config = json!({ "temperature": 0.2 });
        let updated = use_cases
            .update_agent(
                owner_id,
                company_id,
                agent.id,
                "Updated Bot",
                "updated-bot",
                Some("anthropic"),
                Some("claude-3-5-sonnet"),
                None,
                None,
                Some(updated_config.clone()),
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Updated Bot");
        assert_eq!(updated.slug, "updated-bot");
        assert_eq!(updated.provider.as_deref(), Some("anthropic"));
        assert_eq!(updated.model.as_deref(), Some("claude-3-5-sonnet"));
        assert_eq!(updated.api_key, None);
        assert_eq!(updated.system_prompt, None);
        assert_eq!(updated.config_json, Some(updated_config));

        // 5. Delete agent
        use_cases
            .delete_agent(owner_id, company_id, agent.id)
            .await
            .unwrap();

        let list_after = use_cases
            .list_company_agents(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(list_after.len(), 0);
    }

    #[test]
    fn test_generate_system_prompt_yaml_serialization_validity() {
        let prepared_system_prompt = "\
You are an expert AI prompt engineer specializing in crafting system prompts for autonomous AI agents.
Your task is to generate a comprehensive, clear, structured, and production-ready system prompt based on the user's instructions.

Guidelines:
- Define a clear role and primary objective for the agent.
- Output ONLY the system prompt text itself.";

        let indented_system_prompt = prepared_system_prompt
            .lines()
            .map(|line| if line.is_empty() { String::new() } else { format!("  {}", line) })
            .collect::<Vec<_>>()
            .join("\n");

        let config_yaml = format!(
            "name: prompt_generator\nsystem_prompt: |\n{}\nllm:\n  provider: {}\n  model: {}\n  api_key: {}",
            indented_system_prompt, "google", "gemini-2.5-flash", "test_key"
        );

        assert!(ai_agents::AgentBuilder::from_yaml(&config_yaml).is_ok());
    }
}
