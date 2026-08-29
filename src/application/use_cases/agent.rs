use std::sync::Arc;

use ai_agents::Agent as _;
use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        agent::{Agent, MAX_AGENT_RUN_TIMEOUT_SECS, MIN_AGENT_RUN_TIMEOUT_SECS},
        creation::CreationProvenance,
        user::Viewer,
        value_objects::AvatarUrl,
    },
    use_cases::{
        channel::{SlugKind, validate_slug},
        company::{CompanyPersistence, managed_company},
    },
};

/// An agent belonging to another company is reported exactly like a missing one, so an id probe
/// cannot tell a foreign agent from a nonexistent one. See [`managed_company`].
pub fn agent_not_found() -> AppError {
    AppError::NotFound("Agent not found in this company.".into())
}

/// Everything one agent write sets, so create and update cannot drift apart and so a caller
/// cannot transpose two same-typed arguments in a nine-parameter list.
///
/// Values reach persistence already normalized — see [`AgentWrite::normalize`]. Mirrors
/// [`crate::use_cases::channel::ChannelWrite`].
#[derive(Debug, Clone, Default)]
pub struct AgentWrite {
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub run_timeout_secs: Option<u32>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    /// Short statement of what the agent is for, read by the agent directory tool.
    pub description: Option<String>,
    pub config_json: Option<serde_json::Value>,
    pub avatar_url: Option<AvatarUrl>,
    pub created_by: Option<CreationProvenance>,
}

impl AgentWrite {
    /// Trim the fields that have canonical forms and drop the blanks. Runs once, in the use case,
    /// so create and update store the same shape.
    pub(crate) fn normalize(&mut self) -> AppResult<()> {
        self.name = self.name.trim().to_string();
        self.slug = self.slug.trim().to_lowercase().replace(' ', "-");

        if self.name.is_empty() || self.slug.is_empty() {
            return Err(AppError::BadRequest(
                "The agent name and slug cannot be empty.".into(),
            ));
        }
        validate_slug(&self.slug, SlugKind::AgentSlug)?;

        if let Some(seconds) = self.run_timeout_secs
            && !(MIN_AGENT_RUN_TIMEOUT_SECS..=MAX_AGENT_RUN_TIMEOUT_SECS).contains(&seconds)
        {
            return Err(AppError::BadRequest(format!(
                "Agent run timeout must be between {MIN_AGENT_RUN_TIMEOUT_SECS} and {MAX_AGENT_RUN_TIMEOUT_SECS} seconds."
            )));
        }

        for field in [
            &mut self.provider,
            &mut self.model,
            &mut self.api_key,
            &mut self.system_prompt,
            &mut self.description,
        ] {
            if let Some(value) = field.as_mut() {
                *value = value.trim().to_string();
                if value.is_empty() {
                    *field = None;
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
pub trait AgentPersistence: Send + Sync {
    async fn create(&self, company_id: Uuid, write: AgentWrite) -> AppResult<Agent>;

    async fn create_library(&self, _write: AgentWrite) -> AppResult<Agent> {
        Err(AppError::Internal(
            "Agent library persistence is unavailable.".into(),
        ))
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Agent>>;

    async fn get_by_company_slug_and_agent_slug(
        &self,
        company_slug: &str,
        agent_slug: &str,
    ) -> AppResult<Option<Agent>>;

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Agent>>;

    async fn list_library(&self) -> AppResult<Vec<Agent>> {
        Ok(Vec::new())
    }

    async fn update(&self, id: Uuid, write: AgentWrite) -> AppResult<Agent>;

    async fn delete(&self, id: Uuid) -> AppResult<()>;
}

#[derive(Debug, Clone, Default)]
pub struct SelectableAgents {
    pub company_agents: Vec<Agent>,
    pub library_agents: Vec<Agent>,
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

    async fn verify_company_manager(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        managed_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn create_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        mut write: AgentWrite,
    ) -> AppResult<Agent> {
        self.verify_company_manager(user_id, company_id).await?;
        write.created_by = Some(CreationProvenance::user(user_id));
        write.normalize()?;

        info!(
            "Creating agent '{}' ({}) for company {}",
            write.name, write.slug, company_id
        );

        self.agent_persistence.create(company_id, write).await
    }

    #[instrument(skip(self))]
    pub async fn list_company_agents(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Agent>> {
        self.verify_company_manager(user_id, company_id).await?;
        self.agent_persistence.list_by_company_id(company_id).await
    }

    /// Definitions a company owner or admin may assign to a channel, grouped by ownership.
    pub async fn list_selectable_agents(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<SelectableAgents> {
        self.verify_company_manager(user_id, company_id).await?;
        Ok(SelectableAgents {
            company_agents: self
                .agent_persistence
                .list_by_company_id(company_id)
                .await?,
            library_agents: self.agent_persistence.list_library().await?,
        })
    }

    pub async fn list_assignable_agents(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Agent>> {
        let selectable = self.list_selectable_agents(user_id, company_id).await?;
        Ok(selectable
            .library_agents
            .into_iter()
            .chain(selectable.company_agents)
            .collect())
    }

    pub async fn list_library_agents(&self) -> AppResult<Vec<Agent>> {
        self.agent_persistence.list_library().await
    }

    pub async fn get_library_agent(&self, agent_id: Uuid) -> AppResult<Option<Agent>> {
        Ok(self
            .agent_persistence
            .get_by_id(agent_id)
            .await?
            .filter(Agent::is_library))
    }

    pub async fn create_library_agent(&self, mut write: AgentWrite) -> AppResult<Agent> {
        write.created_by = Some(CreationProvenance::system());
        write.normalize()?;
        self.agent_persistence.create_library(write).await
    }

    pub async fn update_library_agent(
        &self,
        agent_id: Uuid,
        mut write: AgentWrite,
    ) -> AppResult<Agent> {
        self.get_library_agent(agent_id)
            .await?
            .ok_or_else(agent_not_found)?;
        write.normalize()?;
        self.agent_persistence.update(agent_id, write).await
    }

    pub async fn delete_library_agent(&self, agent_id: Uuid) -> AppResult<()> {
        self.get_library_agent(agent_id)
            .await?
            .ok_or_else(agent_not_found)?;
        self.agent_persistence.delete(agent_id).await
    }

    #[instrument(skip(self))]
    pub async fn get_company_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<Agent>> {
        self.verify_company_manager(user_id, company_id).await?;
        let agent = self.agent_persistence.get_by_id(agent_id).await?;
        if let Some(ref ag) = agent {
            if ag.company_id != Some(company_id) {
                return Ok(None);
            }
        }
        Ok(agent)
    }

    /// One agent, if this viewer is on the company's team.
    ///
    /// The read counterpart of [`AgentUseCases::get_company_agent`]: that one is for the pages
    /// that *configure* agents and stays owner-or-admin, this one is what a rendered thread names as
    /// its responder, and an invited member reads that just as the owner does. Which channels the
    /// member may read at all is `Channel::viewer_access`'s question, asked before this one.
    #[instrument(skip(self))]
    pub async fn get_readable_agent(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<Agent>> {
        let is_team = self
            .company_persistence
            .company_access(viewer.user_id, company_id)
            .await?
            .is_some_and(|access| access.membership.is_team());
        if !is_team {
            return Ok(None);
        }

        Ok(self
            .agent_persistence
            .get_by_id(agent_id)
            .await?
            .filter(|agent| agent.company_id.is_none() || agent.company_id == Some(company_id)))
    }

    #[instrument(skip(self))]
    pub async fn update_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agent_id: Uuid,
        mut write: AgentWrite,
    ) -> AppResult<Agent> {
        self.verify_company_manager(user_id, company_id).await?;

        let agent = self
            .agent_persistence
            .get_by_id(agent_id)
            .await?
            .ok_or_else(agent_not_found)?;

        if agent.company_id != Some(company_id) {
            return Err(agent_not_found());
        }

        write.normalize()?;

        info!(
            "Updating agent {} for company {}: {} ({})",
            agent_id, company_id, write.name, write.slug
        );

        self.agent_persistence.update(agent_id, write).await
    }

    #[instrument(skip(self))]
    pub async fn delete_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_manager(user_id, company_id).await?;

        let agent = self
            .agent_persistence
            .get_by_id(agent_id)
            .await?
            .ok_or_else(agent_not_found)?;

        if agent.company_id != Some(company_id) {
            return Err(agent_not_found());
        }

        info!("Deleting agent {} for company {}", agent_id, company_id);
        self.agent_persistence.delete(agent_id).await
    }

    #[instrument(skip(self))]
    /// Expand a plain-language description of an agent into a full system prompt, using the
    /// company's own LLM credentials.
    pub async fn generate_system_prompt(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        instructions: &str,
        provider_override: Option<&str>,
        model_override: Option<&str>,
        api_key_override: Option<&str>,
    ) -> AppResult<String> {
        self.verify_company_manager(user_id, company_id).await?;

        let company = self
            .company_persistence
            .get_by_id(company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Company not found.".into()))?;

        let llm = PromptGeneratorLlm::resolve(
            &company,
            provider_override,
            model_override,
            api_key_override,
        )?;

        generate_prompt_with(llm, instructions).await
    }

    /// Generate a library prompt from explicit operator settings (falling back to environment
    /// credentials), since a global definition has no company settings to inherit from.
    pub async fn generate_library_system_prompt(
        &self,
        instructions: &str,
        provider: Option<&str>,
        model: Option<&str>,
        api_key: Option<&str>,
    ) -> AppResult<String> {
        let llm = PromptGeneratorLlm::resolve_global(provider, model, api_key)?;
        generate_prompt_with(llm, instructions).await
    }
}

async fn generate_prompt_with(llm: PromptGeneratorLlm, instructions: &str) -> AppResult<String> {
    let agent = llm.build_agent()?;
    info!(
        "Calling generate_system_prompt AI model | provider: '{}', model: '{}'",
        llm.provider, llm.model
    );
    let response = agent
        .chat(instructions)
        .await
        .map_err(|e| AppError::Internal(format!("AI generation call failed: {e}")))?;

    // Models like to wrap the prompt in a code fence despite being told not to.
    Ok(response
        .content
        .trim()
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string())
}

const PROMPT_GENERATOR_SYSTEM_PROMPT: &str = "\
You are an expert AI prompt engineer specializing in crafting system prompts for autonomous AI agents.
Your task is to generate a comprehensive, clear, structured, and production-ready system prompt based on the user's instructions.

Guidelines:
- Define a clear role and primary objective for the agent.
- Outline specific instructions, guidelines, constraints, and tone of communication.
- Keep the prompt structured, concise, and unambiguous.
- Output ONLY the system prompt text itself. Do NOT include any intro/outro explanations, conversational filler, or markdown code blocks (```).";

/// The model that writes system prompts, resolved from explicit operator input or company-owned
/// settings. Deployment credentials are never shared with a company.
struct PromptGeneratorLlm {
    provider: String,
    model: String,
    api_key: String,
}

impl PromptGeneratorLlm {
    fn resolve_global(
        provider_override: Option<&str>,
        model_override: Option<&str>,
        api_key_override: Option<&str>,
    ) -> AppResult<Self> {
        let provider = non_empty(provider_override)
            .map(str::to_lowercase)
            .ok_or_else(|| {
                AppError::BadRequest(
                    "A provider is required to generate a global library prompt.".into(),
                )
            })?;
        let model = non_empty(model_override)
            .unwrap_or_else(|| default_model_for(&provider))
            .to_string();
        let api_key = non_empty(api_key_override)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "An API key is required to generate a prompt with provider '{provider}'."
                ))
            })?
            .to_string();
        Ok(Self {
            provider,
            model,
            api_key,
        })
    }

    fn resolve(
        company: &crate::entities::company::Company,
        provider_override: Option<&str>,
        model_override: Option<&str>,
        api_key_override: Option<&str>,
    ) -> AppResult<Self> {
        let provider = non_empty(provider_override)
            .or_else(|| non_empty(company.provider.as_deref()))
            .map(|provider| provider.to_lowercase())
            .ok_or_else(|| {
                AppError::Internal(
                    "LLM provider is missing. Please configure one in company settings or in the form."
                        .into(),
                )
            })?;

        let model = non_empty(model_override)
            .or_else(|| non_empty(company.model.as_deref()))
            .unwrap_or_else(|| default_model_for(&provider))
            .to_string();

        let api_key = non_empty(api_key_override)
            .or_else(|| non_empty(company.api_key.as_deref()))
            .ok_or_else(|| AppError::Internal(format!(
                "API key is missing for provider '{}'. Please configure an API key in company settings or in the form.",
                provider
            )))?
            .to_string();

        Ok(Self {
            provider,
            model,
            api_key,
        })
    }

    fn build_agent(&self) -> AppResult<ai_agents::RuntimeAgent> {
        // The system prompt goes in as a YAML block scalar, so every line needs indenting.
        let indented_system_prompt = PROMPT_GENERATOR_SYSTEM_PROMPT
            .lines()
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("  {}", line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let config_yaml = format!(
            "name: prompt_generator\nsystem_prompt: |\n{}\nllm:\n  provider: {}\n  model: {}\n  api_key: {}",
            indented_system_prompt, self.provider, self.model, self.api_key
        );

        let builder = ai_agents::AgentBuilder::from_yaml(&config_yaml).map_err(|e| {
            AppError::Internal(format!("Failed to parse agent builder config: {e}"))
        })?;

        let provider_type = std::str::FromStr::from_str(&self.provider).map_err(|_| {
            AppError::BadRequest(format!("Unsupported LLM provider '{}'.", self.provider))
        })?;
        let provider = ai_agents::UnifiedLLMProvider::new(
            provider_type,
            self.model.clone(),
            Some(self.api_key.clone()),
            None,
        )
        .map_err(|e| AppError::Internal(format!("Failed to initialize LLM provider: {e}")))?;
        let builder = builder.llm(Arc::new(provider));

        builder
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build AI agent: {e}")))
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn default_model_for(provider: &str) -> &'static str {
    match provider {
        "openai" => "gpt-4o",
        "anthropic" => "claude-3-5-sonnet-20241022",
        "groq" => "llama-3.3-70b-versatile",
        // Providers without a tailored default use the Google model default; unsupported
        // providers are rejected when the prompt generator is built.
        _ => "gemini-2.5-flash",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        company::{Company, CompanyAccess},
        company_member::CompanyMembership,
        value_objects::EmailAddress,
    };
    use crate::use_cases::company::CompanyWrite;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Mutex;

    #[test]
    fn agent_timeout_is_optional_bounded_and_overrides_the_global_default() {
        let mut inherited = AgentWrite {
            name: "Inherited".into(),
            slug: "inherited".into(),
            ..AgentWrite::default()
        };
        inherited.normalize().unwrap();

        let mut invalid = AgentWrite {
            name: "Invalid".into(),
            slug: "invalid".into(),
            run_timeout_secs: Some(MAX_AGENT_RUN_TIMEOUT_SECS + 1),
            ..AgentWrite::default()
        };
        assert!(invalid.normalize().is_err());

        let agent = Agent {
            id: Uuid::new_v4(),
            company_id: None,
            name: "Timed".into(),
            slug: "timed".into(),
            provider: None,
            model: None,
            run_timeout_secs: Some(45),
            api_key: None,
            system_prompt: None,
            description: None,
            config_json: None,
            avatar_url: None,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };
        assert_eq!(
            agent.run_timeout(std::time::Duration::from_secs(300)),
            std::time::Duration::from_secs(45)
        );
    }

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
        /// Accepted memberships, as `(user_id, company_id, access)`.
        members: Mutex<Vec<(Uuid, Uuid, CompanyMembership)>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
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

        async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.user_id == user_id)
                .cloned()
                .collect())
        }

        async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
            let companies = self.companies.lock().unwrap();
            let members = self.members.lock().unwrap();
            Ok(companies
                .iter()
                .filter_map(|company| {
                    let membership = if company.user_id == user_id {
                        CompanyMembership::Owner
                    } else if let Some((_, _, membership)) =
                        members.iter().find(|(member_id, company_id, _)| {
                            *member_id == user_id && *company_id == company.id
                        })
                    {
                        *membership
                    } else {
                        return None;
                    };
                    Some(CompanyAccess {
                        company: company.clone(),
                        membership,
                    })
                })
                .collect())
        }

        async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }

        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
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
        async fn create(&self, company_id: Uuid, write: AgentWrite) -> AppResult<Agent> {
            let agent = Agent {
                id: Uuid::new_v4(),
                company_id: Some(company_id),
                name: write.name,
                slug: write.slug,
                provider: write.provider,
                model: write.model,
                run_timeout_secs: write.run_timeout_secs,
                api_key: write.api_key,
                system_prompt: write.system_prompt,
                description: write.description,
                config_json: write.config_json,
                avatar_url: write.avatar_url,
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
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
                .filter(|a| a.company_id == Some(company_id))
                .cloned()
                .collect())
        }

        async fn update(&self, id: Uuid, write: AgentWrite) -> AppResult<Agent> {
            let mut list = self.agents.lock().unwrap();
            let agent = list
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;

            agent.name = write.name;
            agent.slug = write.slug;
            agent.provider = write.provider;
            agent.model = write.model;
            agent.api_key = write.api_key;
            agent.system_prompt = write.system_prompt;
            agent.description = write.description;
            agent.config_json = write.config_json;
            agent.avatar_url = write.avatar_url;
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
            members: Mutex::new(Vec::new()),
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
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
                AgentWrite {
                    name: "Quiet Bot".to_string(),
                    slug: "quiet".to_string(),
                    ..AgentWrite::default()
                },
            )
            .await;
        assert!(invalid_res.is_err());

        // 1. Owner creates agent with config_json
        let config = json!({ "temperature": 0.7, "system_prompt": "Hello" });

        let agent = use_cases
            .create_agent(
                owner_id,
                company_id,
                AgentWrite {
                    name: "Support Bot".to_string(),
                    slug: "support-bot".to_string(),
                    provider: Some("openai".to_string()),
                    model: Some("gpt-4o".to_string()),
                    api_key: Some("key_123".to_string()),
                    system_prompt: Some("Prompt".to_string()),
                    config_json: Some(config.clone()),
                    ..AgentWrite::default()
                },
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
                AgentWrite {
                    name: "Hacker Bot".to_string(),
                    slug: "hacker-bot".to_string(),
                    ..AgentWrite::default()
                },
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
                AgentWrite {
                    name: "Updated Bot".to_string(),
                    slug: "updated-bot".to_string(),
                    provider: Some("anthropic".to_string()),
                    model: Some("claude-3-5-sonnet".to_string()),
                    config_json: Some(updated_config.clone()),
                    ..AgentWrite::default()
                },
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
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("  {}", line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let config_yaml = format!(
            "name: prompt_generator\nsystem_prompt: |\n{}\nllm:\n  provider: {}\n  model: {}\n  api_key: {}",
            indented_system_prompt, "google", "gemini-2.5-flash", "test_key"
        );

        assert!(ai_agents::AgentBuilder::from_yaml(&config_yaml).is_ok());
    }

    #[test]
    fn prompt_generation_requires_explicitly_owned_credentials() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: Some("openai".to_string()),
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };

        let company_error = PromptGeneratorLlm::resolve(&company, None, None, None)
            .err()
            .expect("a company without its own key must be rejected");
        assert!(company_error.to_string().contains("API key is missing"));

        let global_provider_error = PromptGeneratorLlm::resolve_global(None, None, Some("key"))
            .err()
            .expect("a global prompt must name its provider");
        assert!(
            global_provider_error
                .to_string()
                .contains("provider is required")
        );

        let global_key_error = PromptGeneratorLlm::resolve_global(Some("openai"), None, None)
            .err()
            .expect("a global prompt must supply its own key");
        assert!(global_key_error.to_string().contains("API key is required"));
    }

    /// Members may read agents used by their inbox, while only owners and admins may configure
    /// them. Both paths must still stop at the company's edge.
    #[tokio::test]
    async fn members_read_agents_and_admins_manage_them() {
        let owner_id = Uuid::new_v4();
        let admin_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        let stranger_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let other_company_id = Uuid::new_v4();

        let company = |id: Uuid, user_id: Uuid, slug: &str| Company {
            id,
            user_id,
            name: "Acme Corp".to_string(),
            slug: slug.into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![
                company(company_id, owner_id, "acme"),
                company(other_company_id, Uuid::new_v4(), "other"),
            ]),
            members: Mutex::new(vec![
                (admin_id, company_id, CompanyMembership::Admin),
                (member_id, company_id, CompanyMembership::Member),
            ]),
        });
        let agent_persistence = Arc::new(MockAgentPersistence {
            agents: Mutex::new(Vec::new()),
        });
        let use_cases = AgentUseCases::new(company_persistence, agent_persistence);

        let agent = use_cases
            .create_agent(
                owner_id,
                company_id,
                AgentWrite {
                    name: "Support Bot".to_string(),
                    slug: "support-bot".to_string(),
                    ..AgentWrite::default()
                },
            )
            .await
            .expect("the owner creates the agent");

        let viewer = |user_id: Uuid, email: &str| Viewer {
            user_id,
            email: EmailAddress::from(email),
        };

        // The member reads it, exactly as the owner does.
        assert_eq!(
            use_cases
                .get_readable_agent(
                    &viewer(member_id, "member@example.com"),
                    company_id,
                    agent.id
                )
                .await
                .expect("a lookup")
                .map(|found| found.id),
            Some(agent.id)
        );
        assert_eq!(
            use_cases
                .get_readable_agent(&viewer(owner_id, "owner@example.com"), company_id, agent.id)
                .await
                .expect("a lookup")
                .map(|found| found.id),
            Some(agent.id)
        );

        // A stranger to the company does not.
        assert!(
            use_cases
                .get_readable_agent(
                    &viewer(stranger_id, "stranger@example.com"),
                    company_id,
                    agent.id
                )
                .await
                .expect("a lookup")
                .is_none()
        );

        // Nor does the member reach it through a company they are nothing to.
        assert!(
            use_cases
                .get_readable_agent(
                    &viewer(member_id, "member@example.com"),
                    other_company_id,
                    agent.id
                )
                .await
                .expect("a lookup")
                .is_none()
        );

        // Reading is all an ordinary membership grants.
        assert!(
            use_cases
                .list_company_agents(member_id, company_id)
                .await
                .is_err()
        );

        // An admin can list, create, update and delete company agents.
        assert_eq!(
            use_cases
                .list_company_agents(admin_id, company_id)
                .await
                .expect("an admin lists agents")
                .len(),
            1
        );
        let managed = use_cases
            .create_agent(
                admin_id,
                company_id,
                AgentWrite {
                    name: "Admin Bot".into(),
                    slug: "admin-bot".into(),
                    ..AgentWrite::default()
                },
            )
            .await
            .expect("an admin creates an agent");
        let managed = use_cases
            .update_agent(
                admin_id,
                company_id,
                managed.id,
                AgentWrite {
                    name: "Managed Bot".into(),
                    slug: "managed-bot".into(),
                    ..AgentWrite::default()
                },
            )
            .await
            .expect("an admin updates an agent");
        assert_eq!(managed.name, "Managed Bot");
        use_cases
            .delete_agent(admin_id, company_id, managed.id)
            .await
            .expect("an admin deletes an agent");
    }
}
