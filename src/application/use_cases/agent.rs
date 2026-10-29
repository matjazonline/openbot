use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        agent::{Agent, MAX_AGENT_SKILLS},
        company::Company,
        creation::CreationProvenance,
        user::Viewer,
        value_objects::{AvatarUrl, ModelName, ModelProvider},
    },
    services::harness::{ClassificationRequest, TextClassifier},
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
    pub system_prompt: Option<String>,
    /// Short statement of what the agent is for, read by the agent directory tool.
    pub description: Option<String>,
    /// Ordered workflow tool grants (06-D5). `None` keeps the stored list on update.
    pub workflow_tools: Option<Vec<crate::domain::workflow::TypeName>>,
    pub skill_ids: Vec<Uuid>,
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

        for field in [
            &mut self.provider,
            &mut self.model,
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

        validate_workflow_names(self.workflow_tools.as_deref(), "workflow tools")?;
        deduplicate(&mut self.skill_ids);
        if self.skill_ids.len() > MAX_AGENT_SKILLS {
            return Err(AppError::BadRequest(format!(
                "An agent may carry at most {MAX_AGENT_SKILLS} skills."
            )));
        }

        Ok(())
    }
}

fn deduplicate<T: PartialEq>(values: &mut Vec<T>) {
    let mut index = 0;
    while index < values.len() {
        if values[..index].contains(&values[index]) {
            values.remove(index);
        } else {
            index += 1;
        }
    }
}

#[async_trait]
pub trait AgentPersistence: Send + Sync {
    async fn create(&self, company_id: Uuid, write: AgentWrite) -> AppResult<Agent>;

    async fn create_library(&self, write: AgentWrite) -> AppResult<Agent>;

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Agent>>;

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Agent>>;

    async fn list_library(&self) -> AppResult<Vec<Agent>>;

    async fn update(&self, id: Uuid, write: AgentWrite) -> AppResult<Agent>;

    async fn delete(&self, id: Uuid) -> AppResult<()>;
}

#[async_trait]
pub trait LibraryAgentCopyPersistence: Send + Sync {
    /// A company agent copied from a library definition: its skills and workflow tool grants
    /// come with it, in one transaction. Agent only: no channel (P08-D4 §D4.7).
    async fn create_agent_from_library(
        &self,
        company_id: Uuid,
        library_agent_id: Uuid,
        agent: AgentWrite,
    ) -> AppResult<Agent>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ProvisioningWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProvisionedAgent {
    pub agent: Agent,
    pub warnings: Vec<ProvisioningWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpamScanning {
    Available,
    Unavailable,
}

/// Whether one of `connections` enables `model` for `provider`, both compared canonically: the
/// model-selection rule agent writes and assistant provisioning (p08 D2.3) share.
pub(crate) fn model_allowed(
    connections: &[crate::entities::company::CompanyModelConnection],
    provider: &str,
    model: &str,
) -> bool {
    let provider = ModelProvider::canonical(provider);
    let model = ModelName::canonical(model);
    connections
        .iter()
        .any(|connection| connection.provider == provider && connection.models.contains(&model))
}

/// The model an agent pinned to the company default runs on: the default connection's first
/// model, when the connection has a key. A workflow runs an agent only on an explicit model.
pub fn pinned_default_model(
    connections: &[crate::entities::company::CompanyModelConnection],
) -> Option<(String, String)> {
    let connection = connections
        .iter()
        .find(|connection| connection.is_default && connection.has_api_key)?;
    let model = connection.models.first()?;
    Some((
        connection.provider.as_str().to_owned(),
        model.as_str().to_owned(),
    ))
}

/// What a library copy that names no usable model runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryCopyModel {
    /// The company default, followed as it changes, unless the definition grants workflow tools:
    /// those copies are pinned to it.
    FollowDefault,
    /// Pinned to the company default: onboarding gives each copy an assistant channel (p08 D4.7).
    PinDefault,
}

/// One library definition as a company agent write: every stored field, with the provenance left
/// for [`AgentUseCases::create_agent_from_library`] to stamp with whoever picked it. Its workflow
/// tool grants are copied with its skills, in the creating transaction.
fn library_agent_write(definition: &Agent) -> AgentWrite {
    AgentWrite {
        workflow_tools: None,
        name: definition.name.clone(),
        slug: definition.slug.clone(),
        provider: definition.provider.clone(),
        model: definition.model.clone(),
        system_prompt: definition.system_prompt.clone(),
        description: definition.description.clone(),
        skill_ids: Vec::new(),
        avatar_url: definition.avatar_url.clone(),
        created_by: None,
    }
}

#[derive(Clone)]
pub struct AgentUseCases {
    company_persistence: Arc<dyn CompanyPersistence>,
    agent_persistence: Arc<dyn AgentPersistence>,
    library_copy_persistence: Arc<dyn LibraryAgentCopyPersistence>,
    /// What writes a system prompt when an operator asks for one to be generated.
    ///
    /// Optional because every other use case here works without a model: a deployment that has
    /// not configured one still manages agents. The one path that needs it refuses rather than
    /// degrading.
    prompt_classifier: Option<Arc<dyn TextClassifier>>,
}

impl AgentUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        agent_persistence: Arc<dyn AgentPersistence>,
        library_copy_persistence: Arc<dyn LibraryAgentCopyPersistence>,
    ) -> Self {
        Self {
            company_persistence,
            agent_persistence,
            library_copy_persistence,
            prompt_classifier: None,
        }
    }

    pub fn with_prompt_classifier(mut self, classifier: Arc<dyn TextClassifier>) -> Self {
        self.prompt_classifier = Some(classifier);
        self
    }

    /// The model that writes system prompts, or the reason there is none.
    fn prompt_classifier(&self) -> AppResult<&Arc<dyn TextClassifier>> {
        self.prompt_classifier.as_ref().ok_or_else(|| {
            AppError::Internal(
                "No agent harness is configured to generate a system prompt.".to_string(),
            )
        })
    }

    async fn verify_company_manager(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        managed_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    async fn validate_model_selection(
        &self,
        company_id: Uuid,
        write: &AgentWrite,
    ) -> AppResult<()> {
        let (Some(provider), Some(model)) = (write.provider.as_deref(), write.model.as_deref())
        else {
            if write.provider.is_some() || write.model.is_some() {
                return Err(AppError::BadRequest(
                    "Select both a provider and one of its enabled models, or inherit the company default."
                        .into(),
                ));
            }
            return Ok(());
        };
        let connections = self
            .company_persistence
            .list_model_connections(company_id)
            .await?;
        if !model_allowed(&connections, provider, model) {
            return Err(AppError::BadRequest(format!(
                "Model '{}' is not enabled for provider '{}' in this company.",
                ModelName::canonical(model),
                ModelProvider::canonical(provider)
            )));
        }
        Ok(())
    }

    pub async fn list_company_model_connections(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
        self.verify_company_manager(user_id, company_id).await?;
        self.company_persistence
            .list_model_connections(company_id)
            .await
    }

    /// Create a company agent, and only the agent: its address is an assistant channel the agent
    /// page's "Create assistant channel" action makes (P08-D4 §D4.7).
    #[instrument(skip_all)]
    pub async fn create_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        write: AgentWrite,
    ) -> AppResult<Agent> {
        let (_, write) = self
            .prepared_agent_write(user_id, company_id, write)
            .await?;
        info!(%company_id, "Creating agent");
        self.agent_persistence.create(company_id, write).await
    }

    /// Everything a create checks before it writes anything, answering with the normalized write.
    ///
    /// The Simple create path calls it so a bad handle or model pair is refused before a system
    /// prompt is generated for it.
    pub async fn validate_new_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        write: AgentWrite,
    ) -> AppResult<AgentWrite> {
        let (_, write) = self
            .prepared_agent_write(user_id, company_id, write)
            .await?;
        Ok(write)
    }

    /// The company a new agent belongs to, and the write itself normalized and checked.
    ///
    /// One place for "is this a valid new agent here", so the Simple path's check and the create
    /// that follows it cannot disagree about what will be accepted.
    async fn prepared_agent_write(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        mut write: AgentWrite,
    ) -> AppResult<(Company, AgentWrite)> {
        let company =
            managed_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        write.created_by = Some(CreationProvenance::user(user_id));
        write.normalize()?;
        self.validate_model_selection(company_id, &write).await?;
        Ok((company, write))
    }

    /// Create one company agent from a global library definition.
    ///
    /// The definition is copied rather than referenced: the company owns what it creates and can
    /// edit it afterwards without the global entry changing under it. Agent only, like every
    /// create (P08-D4 §D4.7).
    #[instrument(skip_all)]
    pub async fn create_agent_from_library(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        library_agent_id: Uuid,
        copy_model: LibraryCopyModel,
    ) -> AppResult<ProvisionedAgent> {
        managed_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        let definition = self
            .get_library_agent(library_agent_id)
            .await?
            .ok_or_else(agent_not_found)?;
        let mut write = library_agent_write(&definition);

        // A library definition names a model the company may not have enabled, and the picker
        // never showed one. Fall back to the company default rather than refusing the pick, and
        // say so.
        let mut warnings = Vec::new();
        if write.provider.is_some()
            && self
                .validate_model_selection(company_id, &write)
                .await
                .is_err()
        {
            write.provider = None;
            write.model = None;
            warnings.push(ProvisioningWarning {
                code: "library_model_unavailable".into(),
                message: format!(
                    "'{}' was created on the company's default model: the model it is published with is not enabled here.",
                    definition.name
                ),
            });
        }

        if copy_model == LibraryCopyModel::PinDefault && write.provider.is_none() {
            let connections = self
                .company_persistence
                .list_model_connections(company_id)
                .await?;
            if let Some((provider, model)) = pinned_default_model(&connections) {
                write.provider = Some(provider);
                write.model = Some(model);
            }
        }

        write.created_by = Some(CreationProvenance::user(user_id));
        write.normalize()?;
        self.validate_model_selection(company_id, &write).await?;
        let inherited = write.provider.is_none();
        let agent = self
            .library_copy_persistence
            .create_agent_from_library(company_id, library_agent_id, write)
            .await?;
        if inherited && let (Some(provider), Some(model)) = (&agent.provider, &agent.model) {
            warnings.push(ProvisioningWarning {
                code: "library_model_pinned".into(),
                message: format!(
                    "'{}' runs workflows, which need an explicit model, so it was created on the company's default: {provider} {model}.",
                    definition.name
                ),
            });
        }
        Ok(ProvisionedAgent { agent, warnings })
    }

    #[instrument(skip_all)]
    pub async fn list_company_agents(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Agent>> {
        self.verify_company_manager(user_id, company_id).await?;
        self.agent_persistence.list_by_company_id(company_id).await
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

    #[instrument(skip_all)]
    pub async fn get_company_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<Agent>> {
        self.verify_company_manager(user_id, company_id).await?;
        let agent = self.agent_persistence.get_by_id(agent_id).await?;
        if let Some(ref ag) = agent
            && ag.company_id != Some(company_id)
        {
            return Ok(None);
        }
        Ok(agent)
    }

    /// One agent, if this viewer is on the company's team.
    ///
    /// The read counterpart of [`AgentUseCases::get_company_agent`]: that one is for the pages
    /// that *configure* agents and stays owner-or-admin, this one is what a rendered thread names as
    /// its responder, and an invited member reads that just as the owner does. Which channels the
    /// member may read at all is `Channel::viewer_access`'s question, asked before this one.
    #[instrument(skip_all)]
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

    #[instrument(skip_all)]
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
        self.validate_model_selection(company_id, &write).await?;

        info!(%agent_id, %company_id, "Updating agent");

        self.agent_persistence.update(agent_id, write).await
    }

    #[instrument(skip_all)]
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

    #[instrument(skip_all)]
    /// Expand a plain-language description of an agent into a full system prompt, using the
    /// company's own LLM credentials.
    pub async fn generate_system_prompt(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        instructions: &str,
        provider_override: Option<&str>,
        model_override: Option<&str>,
    ) -> AppResult<String> {
        self.verify_company_manager(user_id, company_id).await?;

        let connections = self
            .company_persistence
            .list_model_connections(company_id)
            .await?;
        let default = connections
            .iter()
            .find(|connection| connection.is_default)
            .ok_or_else(|| {
                AppError::BadRequest("Configure a default company model provider first.".into())
            })?;
        let provider = ModelProvider::canonical(
            non_empty(provider_override).unwrap_or(default.provider.as_str()),
        );
        let connection = connections
            .iter()
            .find(|connection| connection.provider == provider)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Provider '{provider}' is not enabled for this company."
                ))
            })?;
        let model = ModelName::canonical(
            non_empty(model_override)
                .or_else(|| connection.models.first().map(|model| model.as_ref()))
                .ok_or_else(|| {
                    AppError::BadRequest("Configure at least one model first.".into())
                })?,
        );
        if !connection.models.contains(&model) {
            return Err(AppError::BadRequest(format!(
                "Model '{model}' is not enabled for provider '{provider}' in this company."
            )));
        }
        let api_key = self
            .company_persistence
            .model_api_key(company_id, &provider)
            .await?;
        let llm = PromptGeneratorLlm::resolve_explicit(&provider, &model, api_key.as_deref())?;

        generate_prompt_with(self.prompt_classifier()?.as_ref(), llm, instructions).await
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
        generate_prompt_with(self.prompt_classifier()?.as_ref(), llm, instructions).await
    }
}

async fn generate_prompt_with(
    classifier: &dyn TextClassifier,
    llm: PromptGeneratorLlm,
    instructions: &str,
) -> AppResult<String> {
    info!("Calling prompt generator");
    let response = classifier
        .complete(ClassificationRequest {
            system_prompt: PROMPT_GENERATOR_SYSTEM_PROMPT,
            user_prompt: instructions,
            provider: &llm.provider,
            model: &llm.model,
            api_key: &llm.api_key,
        })
        .await
        .map_err(|e| AppError::Internal(format!("AI generation call failed: {e}")))?;

    // Models like to wrap the prompt in a code fence despite being told not to.
    Ok(response
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
    provider: ModelProvider,
    model: ModelName,
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
        let model = ModelName::canonical(
            non_empty(model_override)
                .unwrap_or_else(|| crate::model_providers::default_model_for(&provider)),
        );
        let api_key = non_empty(api_key_override)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "An API key is required to generate a prompt with provider '{provider}'."
                ))
            })?
            .to_string();
        Ok(Self {
            provider: ModelProvider::canonical(&provider),
            model,
            api_key,
        })
    }

    fn resolve_explicit(
        provider: &ModelProvider,
        model: &ModelName,
        api_key: Option<&str>,
    ) -> AppResult<Self> {
        // A company that has not finished configuring its provider is a caller problem, not a
        // server fault -- every sibling check in `generate_system_prompt` says so with a 400.
        let api_key = non_empty(api_key)
            .ok_or_else(|| AppError::BadRequest(format!(
                "API key is missing for provider '{provider}'. Please configure the company model connection."
            )))?
            .to_string();

        Ok(Self {
            provider: provider.clone(),
            model: model.clone(),
            api_key,
        })
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;

/// Workflow names are already `TypeName`s; bound and de-duplicate like the SQL shape check.
pub(crate) fn validate_workflow_names(
    names: Option<&[crate::domain::workflow::TypeName]>,
    label: &str,
) -> AppResult<()> {
    let Some(names) = names else {
        return Ok(());
    };
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    if names.len() > 32 || unique.len() != names.len() {
        return Err(AppError::BadRequest(format!(
            "At most 32 distinct {label} are allowed."
        )));
    }
    Ok(())
}
