use std::sync::Arc;

use ai_agents::Agent as _;
use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        agent::{Agent, MAX_AGENT_RUN_TIMEOUT_SECS, MIN_AGENT_RUN_TIMEOUT_SECS},
        channel::{Channel, PUBLIC_PARTICIPANT},
        company::{Company, CompanyChannelDefaults},
        creation::CreationProvenance,
        memory::{MemoryPersistenceMode, MemoryRecallMode, default_memory_max_results},
        user::Viewer,
        value_objects::{AvatarUrl, ModelName, ModelProvider},
    },
    use_cases::{
        channel::{ChannelWrite, SlugKind, check_spam_interlock, validate_slug},
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
#[derive(Debug, Clone)]
pub struct AgentWrite {
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub run_timeout_secs: Option<u32>,
    pub system_prompt: Option<String>,
    /// Short statement of what the agent is for, read by the agent directory tool.
    pub description: Option<String>,
    pub config_json: Option<serde_json::Value>,
    pub memory_enabled: bool,
    pub memory_persistence_mode: MemoryPersistenceMode,
    pub memory_recall_mode: MemoryRecallMode,
    pub memory_max_results: u8,
    pub avatar_url: Option<AvatarUrl>,
    pub created_by: Option<CreationProvenance>,
}

impl Default for AgentWrite {
    fn default() -> Self {
        Self {
            name: String::new(),
            slug: String::new(),
            provider: None,
            model: None,
            run_timeout_secs: None,
            system_prompt: None,
            description: None,
            config_json: None,
            memory_enabled: false,
            memory_persistence_mode: MemoryPersistenceMode::AudienceOnly,
            memory_recall_mode: MemoryRecallMode::Fast,
            memory_max_results: default_memory_max_results(),
            avatar_url: None,
            created_by: None,
        }
    }
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

        if !(1..=20).contains(&self.memory_max_results) {
            return Err(AppError::BadRequest(
                "Memory result limit must be between 1 and 20.".into(),
            ));
        }
        validate_agent_config(self.config_json.as_ref())?;

        Ok(())
    }
}

fn validate_agent_config(config: Option<&serde_json::Value>) -> AppResult<()> {
    let Some(config) = config else { return Ok(()) };
    let object = config
        .as_object()
        .ok_or_else(|| AppError::BadRequest("Agent config must be a JSON object.".into()))?;
    for reserved in ["name", "system_prompt"] {
        if object.contains_key(reserved) {
            return Err(AppError::BadRequest(format!(
                "Agent config path '{reserved}' is reserved; use the typed agent field instead."
            )));
        }
    }
    if let Some(llm) = object.get("llm").and_then(serde_json::Value::as_object) {
        for reserved in ["provider", "model", "api_key"] {
            if llm.contains_key(reserved) {
                return Err(AppError::BadRequest(format!(
                    "Agent config path 'llm.{reserved}' is reserved; use the typed agent or company field instead."
                )));
            }
        }
    }
    fn contains_secret_key(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
                matches!(
                    key.to_ascii_lowercase().as_str(),
                    "api_key" | "apikey" | "api-key"
                ) || contains_secret_key(value)
            }),
            serde_json::Value::Array(values) => values.iter().any(contains_secret_key),
            _ => false,
        }
    }
    if contains_secret_key(config) {
        return Err(AppError::BadRequest(
            "Agent config must not contain API keys or other model credentials.".into(),
        ));
    }
    Ok(())
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

#[async_trait]
pub trait OwnedAgentChannelPersistence: Send + Sync {
    async fn create_owned_agent_channel(
        &self,
        company_id: Uuid,
        agent: AgentWrite,
        channel: ChannelWrite,
    ) -> AppResult<(Agent, Channel)>;

    async fn update_agent_and_owned_address(
        &self,
        agent_id: Uuid,
        write: AgentWrite,
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
    pub channel: Channel,
    pub warnings: Vec<ProvisioningWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpamScanning {
    Available,
    Unavailable,
}

/// Where the personal channel a new agent gets comes from.
///
/// An agent is never created without one, so this is not "should there be a channel" but "who
/// wrote it" -- and the two cases carry different data, which is why it is an enum rather than an
/// `Option<ChannelWrite>` beside a confirmation flag.
#[derive(Debug, Clone)]
pub enum PersonalChannelPlan {
    /// Derived from the company's channel defaults, which is what every caller outside the
    /// Agents workspace's channel step asks for.
    CompanyDefaults,
    /// Written by the caller, on the create form's channel step. Boxed because it dwarfs the
    /// other variant, and this plan travels inside the create future.
    Configured(Box<ConfiguredPersonalChannel>),
}

impl PersonalChannelPlan {
    /// A channel the caller wrote, and its answer to the `@public` spam interlock.
    pub fn configured(write: ChannelWrite, spam_disabled_confirmed: bool) -> Self {
        Self::Configured(Box::new(ConfiguredPersonalChannel {
            write,
            spam_disabled_confirmed,
        }))
    }
}

/// A personal channel as the channel step submitted it.
#[derive(Debug, Clone)]
pub struct ConfiguredPersonalChannel {
    pub write: ChannelWrite,
    /// Whether the `@public`-without-spam-scanning interlock was answered on the step.
    pub spam_disabled_confirmed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PersonalChannelDecision {
    pub channel: ChannelWrite,
    pub warnings: Vec<ProvisioningWarning>,
}

pub(crate) fn personal_channel_write(
    agent: &AgentWrite,
    defaults: &CompanyChannelDefaults,
    spam_scanning: SpamScanning,
) -> PersonalChannelDecision {
    let mut participants = defaults
        .participant_emails
        .as_ref()
        .map(|values| values.iter().map(ToString::to_string).collect::<Vec<_>>());
    let mut warnings = Vec::new();
    if spam_scanning == SpamScanning::Unavailable {
        let had_public = participants.as_mut().is_some_and(|values| {
            let before = values.len();
            values.retain(|value| !value.eq_ignore_ascii_case(PUBLIC_PARTICIPANT));
            before != values.len()
        });
        if had_public {
            warnings.push(ProvisioningWarning {
                code: "public_access_removed".into(),
                message: "The personal channel was created without public access because spam scanning is unavailable.".into(),
            });
        }
    }
    if participants.as_ref().is_some_and(Vec::is_empty) {
        participants = None;
    }

    PersonalChannelDecision {
        channel: ChannelWrite {
            name: agent.name.clone(),
            description: agent.description.clone(),
            slug: agent.slug.clone(),
            participant_emails: participants,
            enabled: true,
            add_3rd_party: defaults.add_3rd_party,
            retrieve_company_memory: defaults.retrieve_company_memory,
            retrieve_agent_memory: defaults.retrieve_agent_memory,
            retrieve_user_memory: defaults.retrieve_user_memory,
            persist_company_memory: defaults.persist_company_memory,
            persist_agent_memory: defaults.persist_agent_memory,
            persist_user_memory: defaults.persist_user_memory,
            ..ChannelWrite::default()
        },
        warnings,
    }
}

/// One library definition as a company agent write: every stored field, with the provenance left
/// for [`AgentUseCases::create_addressable_agent`] to stamp with whoever picked it.
fn library_agent_write(definition: &Agent) -> AgentWrite {
    AgentWrite {
        name: definition.name.clone(),
        slug: definition.slug.clone(),
        provider: definition.provider.clone(),
        model: definition.model.clone(),
        run_timeout_secs: definition.run_timeout_secs,
        system_prompt: definition.system_prompt.clone(),
        description: definition.description.clone(),
        config_json: definition.config_json.clone(),
        memory_enabled: definition.memory_enabled,
        memory_persistence_mode: definition.memory_persistence_mode,
        memory_recall_mode: definition.memory_recall_mode,
        memory_max_results: definition.memory_max_results,
        avatar_url: definition.avatar_url.clone(),
        created_by: None,
    }
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
    owned_persistence: Arc<dyn OwnedAgentChannelPersistence>,
    spam_scanning: SpamScanning,
}

impl AgentUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        agent_persistence: Arc<dyn AgentPersistence>,
        owned_persistence: Arc<dyn OwnedAgentChannelPersistence>,
        spam_scanning: SpamScanning,
    ) -> Self {
        Self {
            company_persistence,
            agent_persistence,
            owned_persistence,
            spam_scanning,
        }
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
        let provider = ModelProvider::canonical(provider);
        let model = ModelName::canonical(model);
        let allowed = connections.iter().any(|connection| {
            connection.provider == provider && connection.models.contains(&model)
        });
        if !allowed {
            return Err(AppError::BadRequest(format!(
                "Model '{model}' is not enabled for provider '{provider}' in this company."
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

    #[instrument(skip(self))]
    pub async fn create_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        write: AgentWrite,
    ) -> AppResult<Agent> {
        Ok(self
            .create_addressable_agent(user_id, company_id, write)
            .await?
            .agent)
    }

    #[instrument(skip(self))]
    pub async fn create_addressable_agent(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        write: AgentWrite,
    ) -> AppResult<ProvisionedAgent> {
        self.create_addressable_agent_with(
            user_id,
            company_id,
            write,
            PersonalChannelPlan::CompanyDefaults,
        )
        .await
    }

    /// [`create_addressable_agent`](Self::create_addressable_agent), told where the personal
    /// channel comes from.
    ///
    /// Both go through one body because the agent and its channel are written in a single
    /// transaction either way: what the plan changes is who filled the channel in, not when it is
    /// created.
    #[instrument(skip(self))]
    pub async fn create_addressable_agent_with(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        write: AgentWrite,
        plan: PersonalChannelPlan,
    ) -> AppResult<ProvisionedAgent> {
        let (company, write) = self
            .prepared_agent_write(user_id, company_id, write)
            .await?;

        info!(
            "Creating agent '{}' ({}) for company {}",
            write.name, write.slug, company_id
        );

        let mut decision = match plan {
            PersonalChannelPlan::CompanyDefaults => {
                personal_channel_write(&write, &company.channel_defaults, self.spam_scanning)
            }
            PersonalChannelPlan::Configured(configured) => {
                self.configured_personal_channel(&write, *configured)?
            }
        };
        decision.channel.created_by = Some(CreationProvenance::user(user_id));
        decision
            .channel
            .normalize_with(crate::use_cases::channel::ActiveAgent::SuppliedByCaller)?;
        let (agent, channel) = self
            .owned_persistence
            .create_owned_agent_channel(company_id, write, decision.channel)
            .await?;
        Ok(ProvisionedAgent {
            agent,
            channel,
            warnings: decision.warnings,
        })
    }

    /// Everything a create checks before it writes anything, answering with the normalized write.
    ///
    /// The create form's channel step calls it so a bad handle, model pair or config JSON is
    /// refused on the agent step rather than after the channel is filled in, and the step renders
    /// the handle in the form it will be stored in.
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
    /// One place for "is this a valid new agent here", so the channel step and the create that
    /// follows it cannot disagree about what will be accepted.
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

    /// The channel the create form's channel step submitted, held to the two rules a personal
    /// channel has that its form cannot enforce.
    ///
    /// Its address follows the agent handle -- [`update_agent_and_owned_address`] keeps them in
    /// step forever after -- so the submitted slug is replaced rather than trusted; and `@public`
    /// on a server without spam scanning needs the confirmation the step showed. The defaults path
    /// strips `@public` and warns instead, because nobody was there to answer.
    ///
    /// [`update_agent_and_owned_address`]: OwnedAgentChannelPersistence::update_agent_and_owned_address
    fn configured_personal_channel(
        &self,
        agent: &AgentWrite,
        configured: ConfiguredPersonalChannel,
    ) -> AppResult<PersonalChannelDecision> {
        let ConfiguredPersonalChannel {
            mut write,
            spam_disabled_confirmed,
        } = configured;

        check_spam_interlock(&write, self.spam_scanning, spam_disabled_confirmed)?;

        write.slug = agent.slug.clone();
        if write.name.trim().is_empty() {
            write.name = agent.name.clone();
        }

        Ok(PersonalChannelDecision {
            channel: write,
            warnings: Vec::new(),
        })
    }

    /// Create one company agent from a global library definition.
    ///
    /// The definition is copied rather than referenced: the company owns what it creates and can
    /// edit it afterwards without the global entry changing under it, and it gets the personal
    /// channel every other created agent gets.
    #[instrument(skip(self))]
    pub async fn create_agent_from_library(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        library_agent_id: Uuid,
    ) -> AppResult<ProvisionedAgent> {
        self.verify_company_manager(user_id, company_id).await?;
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

        let mut provisioned = self
            .create_addressable_agent(user_id, company_id, write)
            .await?;
        warnings.append(&mut provisioned.warnings);
        provisioned.warnings = warnings;
        Ok(provisioned)
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
        self.owned_persistence
            .update_agent_and_owned_address(agent_id, write)
            .await
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
        self.validate_model_selection(company_id, &write).await?;

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

    fn resolve_explicit(
        provider: &ModelProvider,
        model: &ModelName,
        api_key: Option<&str>,
    ) -> AppResult<Self> {
        let provider = provider.as_str().to_string();
        let model = model.as_str().to_string();

        // A company that has not finished configuring its provider is a caller problem, not a
        // server fault -- every sibling check in `generate_system_prompt` says so with a 400.
        let api_key = non_empty(api_key)
            .ok_or_else(|| AppError::BadRequest(format!(
                "API key is missing for provider '{provider}'. Please configure the company model connection."
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
        value_objects::{ChannelSlug, EmailAddress},
    };
    use crate::use_cases::company::CompanyWrite;
    use crate::use_cases::participant::test_support::email_allowlist_policy;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn personal_channel_derivation_copies_defaults_and_safely_removes_public() {
        let agent = AgentWrite {
            name: "Helper".into(),
            slug: "helper".into(),
            description: Some("Helps".into()),
            ..AgentWrite::default()
        };
        let defaults = CompanyChannelDefaults {
            add_3rd_party: false,
            participant_emails: Some(vec!["@public".into(), "partner@example.com".into()]),
            retrieve_company_memory: true,
            retrieve_agent_memory: true,
            retrieve_user_memory: true,
            persist_company_memory: true,
            persist_agent_memory: true,
            persist_user_memory: true,
        };
        let decision = personal_channel_write(&agent, &defaults, SpamScanning::Unavailable);
        assert_eq!(decision.channel.name, "Helper");
        assert_eq!(decision.channel.description.as_deref(), Some("Helps"));
        assert_eq!(decision.channel.slug, "helper");
        assert_eq!(
            decision.channel.participant_emails,
            Some(vec!["partner@example.com".into()])
        );
        assert!(!decision.channel.add_3rd_party);
        assert!(decision.channel.retrieve_company_memory);
        assert!(decision.channel.retrieve_agent_memory);
        assert!(decision.channel.retrieve_user_memory);
        assert!(decision.channel.persist_company_memory);
        assert!(decision.channel.persist_agent_memory);
        assert!(decision.channel.persist_user_memory);
        assert_eq!(decision.warnings.len(), 1);
    }
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
            memory_enabled: false,
            memory_persistence_mode: crate::entities::memory::MemoryPersistenceMode::AudienceOnly,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            id: Uuid::new_v4(),
            company_id: None,
            name: "Timed".into(),
            slug: "timed".into(),
            provider: None,
            model: None,
            run_timeout_secs: Some(45),
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

    #[test]
    fn agent_config_accepts_supplementary_settings_and_rejects_typed_or_secret_paths() {
        for config in [
            json!({"name": "forged"}),
            json!({"system_prompt": "forged"}),
            json!({"llm": {"provider": "other"}}),
            json!({"llm": {"model": "other"}}),
            json!({"llm": {"api_key": "secret"}}),
            json!({"tools": {"nested": {"api-key": "secret"}}}),
        ] {
            let mut write = AgentWrite {
                name: "Configured".into(),
                slug: "configured".into(),
                config_json: Some(config),
                ..AgentWrite::default()
            };
            assert!(write.normalize().is_err());
        }

        let mut allowed = AgentWrite {
            name: "Configured".into(),
            slug: "configured".into(),
            config_json: Some(json!({
                "llm": {"temperature": 0.2, "max_tokens": 512},
                "tools": {"web": {"enabled": true}}
            })),
            ..AgentWrite::default()
        };
        allowed.normalize().unwrap();
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

        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }

        async fn list_company_team_accounts(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
            unimplemented!("this double is not exercised on the team-account path")
        }

        async fn list_model_connections(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
            Ok(vec![crate::entities::company::CompanyModelConnection {
                provider: "openai".into(),
                models: vec!["gpt-4o".into()],
                is_default: true,
                has_api_key: true,
            }])
        }

        async fn model_api_key(
            &self,
            _company_id: Uuid,
            _provider: &crate::entities::value_objects::ModelProvider,
        ) -> AppResult<Option<String>> {
            Ok(Some("test-key".into()))
        }

        async fn replace_model_connections_for_user(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
            _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
        ) -> AppResult<()> {
            unimplemented!("agent use cases read connections, they do not write them")
        }
    }

    struct MockAgentPersistence {
        agents: Mutex<Vec<Agent>>,
    }

    #[async_trait]
    impl AgentPersistence for MockAgentPersistence {
        async fn create(&self, company_id: Uuid, write: AgentWrite) -> AppResult<Agent> {
            let agent = Agent {
                memory_persistence_mode:
                    crate::entities::memory::MemoryPersistenceMode::AudienceOnly,
                memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
                memory_max_results: 5,
                id: Uuid::new_v4(),
                company_id: Some(company_id),
                name: write.name,
                slug: write.slug,
                provider: write.provider,
                model: write.model,
                run_timeout_secs: write.run_timeout_secs,
                system_prompt: write.system_prompt,
                description: write.description,
                config_json: write.config_json,
                memory_enabled: write.memory_enabled,
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
            agent.system_prompt = write.system_prompt;
            agent.description = write.description;
            agent.memory_enabled = write.memory_enabled;
            agent.memory_persistence_mode = write.memory_persistence_mode;
            agent.memory_recall_mode = write.memory_recall_mode;
            agent.memory_max_results = write.memory_max_results;
            agent.config_json = write.config_json;
            agent.avatar_url = write.avatar_url;
            Ok(agent.clone())
        }

        async fn delete(&self, id: Uuid) -> AppResult<()> {
            self.agents.lock().unwrap().retain(|a| a.id != id);
            Ok(())
        }
    }

    #[async_trait]
    impl OwnedAgentChannelPersistence for MockAgentPersistence {
        async fn create_owned_agent_channel(
            &self,
            company_id: Uuid,
            agent: AgentWrite,
            channel: ChannelWrite,
        ) -> AppResult<(Agent, Channel)> {
            let agent = AgentPersistence::create(self, company_id, agent).await?;
            let participant_emails: Option<Vec<EmailAddress>> = channel
                .participant_emails
                .map(|items| items.into_iter().map(Into::into).collect());
            let (access_mode, principal_grants) =
                email_allowlist_policy(company_id, participant_emails.as_deref());
            let channel = Channel {
                id: Uuid::new_v4(),
                company_id,
                owner_agent_id: Some(agent.id),
                name: channel.name,
                description: channel.description,
                slug: channel.slug.into(),
                alias_slugs: channel.alias_slugs.into_iter().map(Into::into).collect(),
                participant_emails,
                access_mode,
                principal_grants,
                agent_ids: Some(vec![agent.id]),
                enabled: channel.enabled,
                add_3rd_party: channel.add_3rd_party,
                retrieve_company_memory: channel.retrieve_company_memory,
                retrieve_agent_memory: channel.retrieve_agent_memory,
                retrieve_user_memory: channel.retrieve_user_memory,
                persist_company_memory: channel.persist_company_memory,
                persist_agent_memory: channel.persist_agent_memory,
                persist_user_memory: channel.persist_user_memory,
                created_by: channel
                    .created_by
                    .unwrap_or_else(CreationProvenance::system),
                created_at: Utc::now(),
            };
            Ok((agent, channel))
        }

        async fn update_agent_and_owned_address(
            &self,
            agent_id: Uuid,
            write: AgentWrite,
        ) -> AppResult<Agent> {
            AgentPersistence::update(self, agent_id, write).await
        }
    }

    /// A company, its owner, and the use cases over mock persistence — what every test about
    /// creating an agent needs before it can say anything about the channel that comes with it.
    struct Fixture {
        owner_id: Uuid,
        company_id: Uuid,
        use_cases: AgentUseCases,
    }

    fn fixture(defaults: CompanyChannelDefaults, spam_scanning: SpamScanning) -> Fixture {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            members: Mutex::new(Vec::new()),
            companies: Mutex::new(vec![Company {
                channel_defaults: defaults,
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
        });
        let agent_persistence = Arc::new(MockAgentPersistence {
            agents: Mutex::new(Vec::new()),
        });

        Fixture {
            owner_id,
            company_id,
            use_cases: AgentUseCases::new(
                company_persistence,
                agent_persistence.clone(),
                agent_persistence,
                spam_scanning,
            ),
        }
    }

    fn support_agent() -> AgentWrite {
        AgentWrite {
            name: "Support Triage".into(),
            slug: "support-triage".into(),
            ..AgentWrite::default()
        }
    }

    /// The channel step writes the personal channel itself, so what it submitted is what gets
    /// stored — except the address, which belongs to the agent handle and is taken from it whatever
    /// the body said.
    #[tokio::test]
    async fn a_configured_personal_channel_is_stored_on_the_agent_handle() {
        let fixture = fixture(CompanyChannelDefaults::default(), SpamScanning::Available);

        let provisioned = fixture
            .use_cases
            .create_addressable_agent_with(
                fixture.owner_id,
                fixture.company_id,
                support_agent(),
                PersonalChannelPlan::configured(
                    ChannelWrite {
                        name: "Support Inbox".into(),
                        description: Some("Where support mail lands".into()),
                        slug: "somewhere-else".into(),
                        alias_slugs: vec!["help".into(), "sales".into()],
                        participant_emails: Some(vec!["partner@example.com".into()]),
                        enabled: true,
                        add_3rd_party: false,
                        retrieve_company_memory: true,
                        persist_user_memory: true,
                        ..ChannelWrite::default()
                    },
                    false,
                ),
            )
            .await
            .expect("the pair is created");

        assert_eq!(provisioned.channel.slug, "support-triage");
        assert_eq!(provisioned.channel.name, "Support Inbox");
        assert_eq!(
            provisioned.channel.description.as_deref(),
            Some("Where support mail lands")
        );
        assert_eq!(
            provisioned.channel.alias_slugs,
            vec![ChannelSlug::from("help"), ChannelSlug::from("sales")]
        );
        assert_eq!(
            provisioned.channel.participant_emails,
            Some(vec!["partner@example.com".into()])
        );
        assert!(!provisioned.channel.add_3rd_party);
        assert!(provisioned.channel.retrieve_company_memory);
        assert!(provisioned.channel.persist_user_memory);
        assert!(!provisioned.channel.retrieve_user_memory);
        assert!(provisioned.warnings.is_empty());
    }

    /// The defaults path strips `@public` and warns, because nobody was there to answer. The
    /// configured path showed the interlock, so it refuses instead — and takes the answer.
    #[tokio::test]
    async fn a_public_configured_channel_is_refused_until_the_interlock_is_answered() {
        let public = |confirmed| {
            PersonalChannelPlan::configured(
                ChannelWrite {
                    name: "Support Triage".into(),
                    slug: "support-triage".into(),
                    participant_emails: Some(vec![PUBLIC_PARTICIPANT.into()]),
                    enabled: true,
                    ..ChannelWrite::default()
                },
                confirmed,
            )
        };

        let fixture = fixture(CompanyChannelDefaults::default(), SpamScanning::Unavailable);
        let refused = fixture
            .use_cases
            .create_addressable_agent_with(
                fixture.owner_id,
                fixture.company_id,
                support_agent(),
                public(false),
            )
            .await;
        assert!(
            matches!(refused, Err(AppError::BadRequest(_))),
            "{refused:?}"
        );

        let provisioned = fixture
            .use_cases
            .create_addressable_agent_with(
                fixture.owner_id,
                fixture.company_id,
                support_agent(),
                public(true),
            )
            .await
            .expect("a confirmed public channel is created");
        assert_eq!(
            provisioned.channel.participant_emails,
            Some(vec![PUBLIC_PARTICIPANT.into()])
        );
        assert!(provisioned.warnings.is_empty());
    }

    /// The split must not change what a create that names no channel produces.
    #[tokio::test]
    async fn an_unconfigured_personal_channel_still_comes_from_the_company_defaults() {
        let fixture = fixture(
            CompanyChannelDefaults {
                add_3rd_party: false,
                participant_emails: Some(vec![
                    PUBLIC_PARTICIPANT.into(),
                    "partner@example.com".into(),
                ]),
                retrieve_agent_memory: true,
                ..CompanyChannelDefaults::default()
            },
            SpamScanning::Unavailable,
        );

        let provisioned = fixture
            .use_cases
            .create_addressable_agent(fixture.owner_id, fixture.company_id, support_agent())
            .await
            .expect("the pair is created");

        assert_eq!(provisioned.channel.slug, "support-triage");
        assert_eq!(provisioned.channel.name, "Support Triage");
        assert_eq!(
            provisioned.channel.participant_emails,
            Some(vec!["partner@example.com".into()])
        );
        assert!(!provisioned.channel.add_3rd_party);
        assert!(provisioned.channel.retrieve_agent_memory);
        assert_eq!(provisioned.warnings.len(), 1);
    }

    /// The channel step checks the agent before it renders, so the two must refuse the same things.
    #[tokio::test]
    async fn validating_a_new_agent_normalizes_the_handle_and_refuses_an_unavailable_model() {
        let fixture = fixture(CompanyChannelDefaults::default(), SpamScanning::Available);

        let validated = fixture
            .use_cases
            .validate_new_agent(
                fixture.owner_id,
                fixture.company_id,
                AgentWrite {
                    name: "  Support Triage  ".into(),
                    slug: "Support Triage".into(),
                    ..AgentWrite::default()
                },
            )
            .await
            .expect("a plain agent validates");
        assert_eq!(validated.name, "Support Triage");
        assert_eq!(validated.slug, "support-triage");

        let refused = fixture
            .use_cases
            .validate_new_agent(
                fixture.owner_id,
                fixture.company_id,
                AgentWrite {
                    provider: Some("anthropic".into()),
                    model: Some("claude-not-enabled".into()),
                    ..support_agent()
                },
            )
            .await;
        assert!(
            matches!(refused, Err(AppError::BadRequest(_))),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn company_owner_agent_crud_flow_works() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            members: Mutex::new(Vec::new()),
            companies: Mutex::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
        });

        let agent_persistence = Arc::new(MockAgentPersistence {
            agents: Mutex::new(Vec::new()),
        });

        let use_cases = AgentUseCases::new(
            company_persistence,
            agent_persistence.clone(),
            agent_persistence,
            SpamScanning::Available,
        );

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
        let config = json!({ "temperature": 0.7 });

        let agent = use_cases
            .create_agent(
                owner_id,
                company_id,
                AgentWrite {
                    name: "Support Bot".to_string(),
                    slug: "support-bot".to_string(),
                    provider: Some("openai".to_string()),
                    model: Some("gpt-4o".to_string()),
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
        let invalid_model = use_cases
            .update_agent(
                owner_id,
                company_id,
                agent.id,
                AgentWrite {
                    name: "Updated Bot".to_string(),
                    slug: "updated-bot".to_string(),
                    provider: Some("anthropic".to_string()),
                    model: Some("claude-3-5-sonnet".to_string()),
                    ..AgentWrite::default()
                },
            )
            .await;
        assert!(invalid_model.is_err());

        let updated = use_cases
            .update_agent(
                owner_id,
                company_id,
                agent.id,
                AgentWrite {
                    name: "Updated Bot".to_string(),
                    slug: "updated-bot".to_string(),
                    provider: Some("openai".to_string()),
                    model: Some("gpt-4o".to_string()),
                    config_json: Some(updated_config.clone()),
                    ..AgentWrite::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Updated Bot");
        assert_eq!(updated.slug, "updated-bot");
        assert_eq!(updated.provider.as_deref(), Some("openai"));
        assert_eq!(updated.model.as_deref(), Some("gpt-4o"));
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
        let company_error = PromptGeneratorLlm::resolve_explicit(
            &ModelProvider::canonical("google"),
            &ModelName::canonical("gemini-2.5-flash"),
            None,
        )
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
            channel_defaults: Default::default(),
            id,
            user_id,
            name: "Acme Corp".to_string(),
            slug: slug.into(),
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
        let use_cases = AgentUseCases::new(
            company_persistence,
            agent_persistence.clone(),
            agent_persistence,
            SpamScanning::Available,
        );

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

    #[tokio::test]
    async fn a_library_pick_is_copied_into_the_company_with_a_model_it_can_run() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            members: Mutex::new(Vec::new()),
            companies: Mutex::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
        });

        // A library definition is one with no company of its own.
        let definition = |name: &str, slug: &str, provider: &str, model: &str| Agent {
            id: Uuid::new_v4(),
            company_id: None,
            name: name.to_string(),
            slug: slug.to_string(),
            provider: Some(provider.to_string()),
            model: Some(model.to_string()),
            run_timeout_secs: Some(90),
            system_prompt: Some("Answer briefly.".into()),
            description: Some("Sorts support mail".into()),
            config_json: None,
            memory_enabled: true,
            memory_persistence_mode: MemoryPersistenceMode::AudienceOnly,
            memory_recall_mode: MemoryRecallMode::Fast,
            memory_max_results: 7,
            avatar_url: None,
            created_by: CreationProvenance::system(),
            created_at: Utc::now(),
        };
        let runnable = definition("Triage", "triage", "openai", "gpt-4o");
        let elsewhere = definition(
            "Billing",
            "billing",
            "anthropic",
            "claude-3-5-sonnet-20241022",
        );
        let agent_persistence = Arc::new(MockAgentPersistence {
            agents: Mutex::new(vec![runnable.clone(), elsewhere.clone()]),
        });
        let use_cases = AgentUseCases::new(
            company_persistence,
            agent_persistence.clone(),
            agent_persistence,
            SpamScanning::Available,
        );

        // The definition is copied field for field, and the company owns the copy.
        let provisioned = use_cases
            .create_agent_from_library(owner_id, company_id, runnable.id)
            .await
            .expect("the owner picks a library agent");
        assert_eq!(provisioned.agent.company_id, Some(company_id));
        assert_ne!(provisioned.agent.id, runnable.id);
        assert_eq!(provisioned.agent.name, "Triage");
        assert_eq!(provisioned.agent.slug, "triage");
        assert_eq!(provisioned.agent.provider.as_deref(), Some("openai"));
        assert_eq!(provisioned.agent.model.as_deref(), Some("gpt-4o"));
        assert_eq!(provisioned.agent.run_timeout_secs, Some(90));
        assert_eq!(
            provisioned.agent.system_prompt.as_deref(),
            Some("Answer briefly.")
        );
        assert!(provisioned.agent.memory_enabled);
        // It gets the personal channel every created agent gets.
        assert_eq!(provisioned.channel.slug.as_str(), "triage");
        assert!(provisioned.warnings.is_empty());

        // A definition published on a model this company has not enabled still gets created --
        // on the company default, and it says so rather than failing the pick.
        let fallback = use_cases
            .create_agent_from_library(owner_id, company_id, elsewhere.id)
            .await
            .expect("a pick whose model is unavailable here");
        assert_eq!(fallback.agent.provider, None);
        assert_eq!(fallback.agent.model, None);
        assert_eq!(
            fallback
                .warnings
                .iter()
                .map(|warning| warning.code.as_str())
                .collect::<Vec<_>>(),
            vec!["library_model_unavailable"]
        );

        // A company agent is not a library definition, so it cannot be picked as one.
        assert!(
            use_cases
                .create_agent_from_library(owner_id, company_id, provisioned.agent.id)
                .await
                .is_err()
        );
    }
}
