//! Turning a company, an agent row and a credential into the capability spec a harness runs.
//!
//! Nothing here compiles anything. It resolves *which* provider, model and credential this run is
//! entitled to, and hands the rest of the agent's description over as an
//! [`AgentCapabilitySpec`] -- what the agent is and may do, in terms no runtime owns. Which
//! dialect that becomes is the harness's business.

use anyhow::anyhow;

use crate::entities::agent::Agent as AgentEntity;
use crate::entities::company::Company;
use crate::entities::harness::{AgentCapabilitySpec, HarnessKind, SubAgentScope};
use crate::entities::value_objects::{ModelName, ModelProvider};

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
#[derive(Debug, Clone)]
pub struct ResolvedAgentParams {
    api_key: String,
    spec: AgentCapabilitySpec,
}

impl ResolvedAgentParams {
    #[cfg(test)]
    pub(crate) fn new(
        company: Option<&Company>,
        agent: Option<&AgentEntity>,
    ) -> anyhow::Result<Self> {
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
    pub(crate) fn from_connection(
        company: Option<&Company>,
        agent: Option<&AgentEntity>,
        provider: &ModelProvider,
        model: &ModelName,
        api_key: &str,
    ) -> anyhow::Result<Self> {
        if !SUPPORTED_PROVIDERS.contains(&provider.as_str()) {
            anyhow::bail!(
                "Unsupported agent provider '{}'. Allowed providers are: {}",
                provider,
                SUPPORTED_PROVIDERS.join(", ")
            );
        }
        if model.as_str().is_empty() {
            return Err(anyhow!("Agent model is missing"));
        }
        let api_key = Some(api_key.trim())
            .filter(|key| !key.is_empty())
            .ok_or_else(|| {
                tracing::warn!(provider = %provider, "API key is missing for provider");
                anyhow!(
                    "API key is missing for provider '{provider}'. Please configure the company model connection."
                )
            })?
            .to_string();

        let name = non_blank(agent.map(|agent| agent.name.as_str()))
            .or_else(|| non_blank(company.map(|company| company.name.as_str())))
            .unwrap_or(DEFAULT_AGENT_NAME)
            .to_string();
        let system_prompt = non_blank(agent.and_then(|agent| agent.system_prompt.as_deref()))
            .unwrap_or(DEFAULT_SYSTEM_PROMPT)
            .to_string();

        Ok(Self {
            api_key,
            spec: AgentCapabilitySpec {
                // Phase 5 reads this from the agent row. Until then every agent runs on the one
                // harness this deployment has, which is also what `HarnessKind::default()` means.
                harness: HarnessKind::default(),
                name,
                system_prompt,
                provider: provider.clone(),
                model: model.clone(),
                // Phases 4 and 5 fill these from storage. Empty here is not a placeholder: it is
                // what every agent has today, and it is why this phase changes no agent's output.
                skills: Vec::new(),
                granted_tools: Vec::new(),
                sub_agents: SubAgentScope::AllCompanySiblings,
                extra_config: agent
                    .and_then(|agent| agent.config_json.clone())
                    .unwrap_or_else(|| serde_json::json!({})),
            },
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
        &self.spec
    }

    /// The agent's own configuration, as an operator wrote it.
    ///
    /// Deliberately *not* the compiled configuration: the harness's own defaults, the credential
    /// and the compiled tool grant are not this agent's settings, and two of the three must never
    /// be written to a task payload.
    pub fn config(&self) -> &serde_json::Value {
        &self.spec.extra_config
    }
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Load exactly the credential selected by this agent, then discard the persistence boundary
/// before constructing the capability spec.
pub async fn resolve_agent_params(
    persistence: &dyn crate::use_cases::company::CompanyPersistence,
    company: &Company,
    agent: Option<&AgentEntity>,
) -> anyhow::Result<ResolvedAgentParams> {
    let connections = persistence.list_model_connections(company.id).await?;
    let default = connections
        .iter()
        .find(|connection| connection.is_default)
        .ok_or_else(|| anyhow!("Company default model connection is missing"))?;
    let provider = ModelProvider::canonical(
        agent
            .and_then(|agent| agent.provider.as_deref())
            .unwrap_or(default.provider.as_str()),
    );
    let connection = connections
        .iter()
        .find(|connection| connection.provider == provider)
        .ok_or_else(|| anyhow!("Provider '{provider}' is not enabled for this company"))?;
    let model = ModelName::canonical(
        agent
            .and_then(|agent| agent.model.as_deref())
            .or_else(|| connection.models.first().map(|model| model.as_str()))
            .ok_or_else(|| anyhow!("Provider '{provider}' has no enabled models"))?,
    );
    if !connection.models.contains(&model) {
        anyhow::bail!("Model '{model}' is not enabled for provider '{provider}' in this company");
    }
    let api_key = persistence.model_api_key(company.id, &provider).await?;
    ResolvedAgentParams::from_connection(
        Some(company),
        agent,
        &provider,
        &model,
        api_key.as_deref().unwrap_or_default(),
    )
}

#[cfg(test)]
#[path = "params_tests.rs"]
mod tests;
