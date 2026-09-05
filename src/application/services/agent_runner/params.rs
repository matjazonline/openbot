//! Turning a company, an agent row and a credential into the capability spec a harness runs.
//!
//! Nothing here compiles anything. It resolves *which* provider, model and credential this run is
//! entitled to, and hands the rest of the agent's description over as an
//! [`AgentCapabilitySpec`] -- what the agent is and may do, in terms no runtime owns. Which
//! dialect that becomes is the harness's business.

use uuid::Uuid;

use crate::app_error::{AppError, AppResult};
use crate::entities::agent::Agent as AgentEntity;
use crate::entities::company::Company;
use crate::entities::harness::{
    AgentCapabilitySpec, HarnessConfig, HarnessKind, NativeToolPolicy, SubAgentScope,
};
use crate::entities::value_objects::{ModelName, ModelProvider};
use crate::use_cases::skill::{AgentCapabilityReader, StoredAgentCapabilities};

/// The providers this platform will build an agent for.
///
/// Mirrored by the company model-connection check in `use_cases/company.rs`: a company cannot
/// store a credential for a provider that is not here, and an agent cannot select one.
const SUPPORTED_PROVIDERS: [&str; 4] = ["google", "openai", "anthropic", "groq"];

/// What an agent says it is when nothing has told it. Public because it is a value a reader may
/// need to recognise -- an agent answering with this has no prompt of its own.
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful assistant.";

/// What an agent is called when neither it nor its company has a usable name.
pub const DEFAULT_AGENT_NAME: &str = "agent";

/// One runnable agent: its capability spec, and the credential it runs on.
///
/// The credential is kept beside the spec rather than inside it because it belongs to the
/// *company*, not to the agent: two agents of one company share it, and it must never be stored,
/// serialized or logged alongside the description that is.
#[derive(Clone)]
pub struct ResolvedAgentCapabilities {
    /// Company credential for the selected provider. Never include this value in structured
    /// diagnostics; pass it to `sanitize_text` as the literal to redact from runtime failures.
    pub api_key: String,
    pub spec: Box<AgentCapabilitySpec>,
    /// Stable identity used for authorization and structured diagnostics. Agent names are mutable
    /// operator-authored display text and must never substitute for this value.
    pub agent_id: Uuid,
    /// Platform-native bounds consumed by native tool implementations, never by a harness
    /// compiler.
    pub native_tool_policy: NativeToolPolicy,
}

impl std::fmt::Debug for ResolvedAgentCapabilities {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedAgentCapabilities")
            .field("spec", &self.spec)
            .field("agent_id", &self.agent_id)
            .field("native_tool_policy", &self.native_tool_policy)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

impl ResolvedAgentCapabilities {
    #[cfg(test)]
    pub(crate) fn new(company: Option<&Company>, agent: Option<&AgentEntity>) -> AppResult<Self> {
        let provider = ModelProvider::canonical(
            agent
                .and_then(|agent| agent.provider.as_deref())
                .unwrap_or("google"),
        );
        let model = ModelName::canonical(
            agent
                .and_then(|agent| agent.model.as_deref())
                .unwrap_or("gemini-2.5-flash"),
        );
        Self::from_connection(company, agent, &provider, &model, "company-api-key")
    }

    /// Resolve executable agent settings. Credentials always come from the company's encrypted
    /// model connection; an agent may select a model only for that same provider.
    ///
    /// `provider` and `model` are newtypes rather than two adjacent `&str`: they are the pair
    /// `src/AGENTS.md` names as the classic argument-swap bug, and swapping them here would send
    /// a model name to a provider lookup with a real credential attached.
    #[cfg(test)]
    pub(crate) fn from_connection(
        company: Option<&Company>,
        agent: Option<&AgentEntity>,
        provider: &ModelProvider,
        model: &ModelName,
        api_key: &str,
    ) -> AppResult<Self> {
        Self::from_snapshot_connection(
            company,
            agent,
            Vec::new(),
            SubAgentScope::AllCompanySiblings,
            provider,
            model,
            api_key,
        )
    }

    fn from_snapshot_connection(
        company: Option<&Company>,
        agent: Option<&AgentEntity>,
        skills: Vec<crate::entities::skill::Skill>,
        sub_agents: SubAgentScope,
        provider: &ModelProvider,
        model: &ModelName,
        api_key: &str,
    ) -> AppResult<Self> {
        if !SUPPORTED_PROVIDERS.contains(&provider.as_str()) {
            return Err(AppError::BadRequest(format!(
                "Unsupported agent provider '{}'. Allowed providers are: {}",
                provider,
                SUPPORTED_PROVIDERS.join(", ")
            )));
        }
        if model.as_str().is_empty() {
            return Err(AppError::BadRequest("Agent model is missing".into()));
        }
        let api_key = Some(api_key.trim())
            .filter(|key| !key.is_empty())
            .ok_or_else(|| {
                tracing::warn!(provider = %provider, "API key is missing for provider");
                AppError::BadRequest(format!(
                    "API key is missing for provider '{provider}'. Please configure the company model connection."
                ))
            })?
            .to_string();

        let name = non_blank(agent.map(|agent| agent.name.as_str()))
            .or_else(|| non_blank(company.map(|company| company.name.as_str())))
            .unwrap_or(DEFAULT_AGENT_NAME)
            .to_string();
        let system_prompt = non_blank(agent.and_then(|agent| agent.system_prompt.as_deref()))
            .unwrap_or(DEFAULT_SYSTEM_PROMPT)
            .to_string();

        let harness = agent.map_or_else(HarnessKind::default, |agent| agent.harness_kind);
        let harness_config =
            HarnessConfig::parse(harness, agent.and_then(|agent| agent.config_json.as_ref()))
                .map_err(AppError::BadRequest)?;
        let agent_id = agent.map_or_else(Uuid::nil, |agent| agent.id);
        let native_tool_policy = agent
            .map(|agent| agent.native_tool_policy.clone())
            .unwrap_or_default();
        Ok(Self {
            api_key,
            spec: Box::new(AgentCapabilitySpec {
                harness,
                name,
                system_prompt,
                provider: provider.clone(),
                model: model.clone(),
                provider_base_url: agent.and_then(|agent| {
                    #[cfg(test)]
                    {
                        crate::services::test_support::scripted_agent_base_url(agent.id)
                    }
                    #[cfg(not(test))]
                    {
                        let _ = agent;
                        None
                    }
                }),
                skills,
                granted_tools: agent
                    .map(|agent| agent.granted_tool_ids.clone())
                    .unwrap_or_default(),
                sub_agents,
                harness_config,
            }),
            agent_id,
            native_tool_policy,
        })
    }

    pub fn provider(&self) -> &ModelProvider {
        &self.spec.provider
    }

    pub fn model(&self) -> &ModelName {
        &self.spec.model
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn spec(&self) -> &AgentCapabilitySpec {
        self.spec.as_ref()
    }

    pub fn native_tool_policy(&self) -> &NativeToolPolicy {
        &self.native_tool_policy
    }

    /// Canonical reviewed harness settings, retained for execution diagnostics.
    ///
    /// Deliberately *not* the compiled configuration: server defaults, credentials, and compiled
    /// grants must never be written to a task payload.
    pub fn config(&self) -> serde_json::Value {
        match self.spec.harness_config.to_json() {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(agent_id = %self.agent_id, %error, "Could not encode agent harness config for diagnostics");
                serde_json::Value::Null
            }
        }
    }
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Load exactly the credential selected by this agent, then discard the persistence boundary
/// before constructing the capability spec.
pub async fn resolve_agent_capabilities(
    company_persistence: &dyn crate::use_cases::company::CompanyPersistence,
    capabilities: &dyn AgentCapabilityReader,
    company: &Company,
    agent_id: Uuid,
) -> AppResult<ResolvedAgentCapabilities> {
    let StoredAgentCapabilities {
        agent,
        skills,
        sub_agent_scope,
    } = capabilities
        .load_for_execution(company.id, agent_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent capability snapshot for {agent_id} is missing or outside company {}.",
                company.id
            ))
        })?;
    if agent.id != agent_id
        || agent
            .company_id
            .is_some_and(|owner_company_id| owner_company_id != company.id)
    {
        return Err(AppError::Internal(format!(
            "Agent capability reader returned a snapshot outside the requested execution scope for {agent_id}."
        )));
    }
    let connections = company_persistence
        .list_model_connections(company.id)
        .await?;
    let default = connections
        .iter()
        .find(|connection| connection.is_default)
        .ok_or_else(|| {
            AppError::BadRequest("Company default model connection is missing".into())
        })?;
    let provider = ModelProvider::canonical(
        agent
            .provider
            .as_deref()
            .unwrap_or(default.provider.as_str()),
    );
    let connection = connections
        .iter()
        .find(|connection| connection.provider == provider)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Provider '{provider}' is not enabled for this company"
            ))
        })?;
    let model = ModelName::canonical(
        agent
            .model
            .as_deref()
            .or_else(|| connection.models.first().map(|model| model.as_str()))
            .ok_or_else(|| {
                AppError::BadRequest(format!("Provider '{provider}' has no enabled models"))
            })?,
    );
    if !connection.models.contains(&model) {
        return Err(AppError::BadRequest(format!(
            "Model '{model}' is not enabled for provider '{provider}' in this company"
        )));
    }
    let api_key = company_persistence
        .model_api_key(company.id, &provider)
        .await?;
    ResolvedAgentCapabilities::from_snapshot_connection(
        Some(company),
        Some(&agent),
        skills,
        sub_agent_scope,
        &provider,
        &model,
        api_key.as_deref().unwrap_or_default(),
    )
}

#[cfg(test)]
#[path = "params_tests.rs"]
mod tests;
