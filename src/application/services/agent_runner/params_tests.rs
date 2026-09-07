//! Which provider, model and credential one agent run is entitled to, and what its capability
//! spec says before any harness has seen it.
//!
//! One persistence double and nothing else: every rule here is a pure function of a company row,
//! an agent row and a stored model connection.

use uuid::Uuid;

use super::{ResolvedAgentCapabilities, resolve_agent_capabilities};
use crate::entities::agent::Agent as AgentEntity;
use crate::entities::company::Company;
use crate::entities::harness::{HarnessKind, SubAgentScope};
use crate::entities::skill::{Skill, SkillInstruction};
use crate::entities::value_objects::{ModelName, ModelProvider, ToolId};
use crate::use_cases::skill::{AgentCapabilityReader, StoredAgentCapabilities};

fn company_named(name: &str) -> Company {
    Company {
        channel_defaults: Default::default(),
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        name: name.to_string(),
        slug: "acme".into(),
        enable_llm_spam_guardrail: None,
        avatar_url: None,
        memory_provider: None,
        created_at: chrono::Utc::now(),
    }
}

#[test]
fn an_unsupported_provider_is_refused_before_a_credential_is_touched() {
    let error = ResolvedAgentCapabilities::from_connection(
        None,
        None,
        &ModelProvider::canonical(""),
        &ModelName::canonical("model"),
        "key",
    )
    .expect_err("a blank provider is not supported");
    assert!(error.to_string().contains("Unsupported agent provider"));
}

#[test]
fn an_agent_without_a_model_cannot_be_resolved() {
    let error = ResolvedAgentCapabilities::from_connection(
        Some(&company_named("Test")),
        None,
        &ModelProvider::canonical("google"),
        &ModelName::canonical(""),
        "key",
    )
    .expect_err("a provider alone does not name a model");
    assert!(error.to_string().contains("Agent model is missing"));
}

#[test]
fn a_blank_credential_is_refused_rather_than_passed_on() {
    let error = ResolvedAgentCapabilities::from_connection(
        Some(&company_named("Test")),
        None,
        &ModelProvider::canonical("google"),
        &ModelName::canonical("gemini-2.5-flash"),
        "   ",
    )
    .expect_err("whitespace is not a credential");
    assert!(error.to_string().contains("API key is missing"));
}

/// The spec a company with no agent produces: the company's own name, the default prompt, and
/// nothing granted. What is *absent* is the point -- this phase gives no agent a skill or a tool
/// it did not already have.
#[test]
fn a_company_without_an_agent_resolves_to_a_spec_that_grants_nothing() {
    let company = company_named("Acme Corp");

    let resolved = ResolvedAgentCapabilities::new(Some(&company), None).expect("params resolve");

    assert_eq!(resolved.provider(), "google");
    assert_eq!(resolved.model(), "gemini-2.5-flash");
    assert_eq!(resolved.api_key(), "company-api-key");

    let spec = resolved.spec();
    assert_eq!(spec.harness, HarnessKind::AiAgents);
    assert_eq!(spec.name, "Acme Corp");
    assert_eq!(spec.system_prompt, "You are a helpful assistant.");
    assert!(spec.granted_tools.is_empty());
    assert!(spec.skills.is_empty());
    assert_eq!(spec.sub_agents, SubAgentScope::AllCompanySiblings);
    assert_eq!(
        spec.harness_config,
        crate::entities::harness::HarnessConfig::empty(HarnessKind::AiAgents)
    );
}

/// The agent's own name and prompt win over the company's, and reviewed advanced settings are
/// decoded into the typed capability spec.
#[test]
fn an_agent_supplies_its_own_name_prompt_and_reviewed_configuration() {
    let company = company_named("Acme Corp");
    let mut agent = agent_selecting(None, None);
    agent.name = "Support Agent".into();
    agent.system_prompt = Some("You are a helpful triage assistant.".into());
    agent.config_json = Some(serde_json::json!({
        "version": 1,
        "reasoning": {"mode": "react", "max_iterations": 4}
    }));

    let resolved =
        ResolvedAgentCapabilities::new(Some(&company), Some(&agent)).expect("params resolve");

    assert_eq!(resolved.spec().name, "Support Agent");
    assert_eq!(
        resolved.spec().system_prompt,
        "You are a helpful triage assistant."
    );
    assert_eq!(
        resolved.config(),
        serde_json::json!({
            "version": 1,
            "reasoning": {"mode": "react", "max_iterations": 4}
        })
    );
}

#[test]
fn an_agent_with_a_blank_prompt_falls_back_to_the_default() {
    let company = company_named("Acme Corp");
    let mut agent = agent_selecting(None, None);
    agent.system_prompt = Some("   ".into());

    let resolved =
        ResolvedAgentCapabilities::new(Some(&company), Some(&agent)).expect("params resolve");

    assert_eq!(
        resolved.spec().system_prompt,
        "You are a helpful assistant."
    );
}

#[test]
fn every_supported_provider_resolves_and_nothing_else_does() {
    let company = company_named("Acme Corp");

    for provider in ["google", "openai", "anthropic", "groq", "xai"] {
        let resolved = ResolvedAgentCapabilities::from_connection(
            Some(&company),
            None,
            &ModelProvider::canonical(provider),
            &ModelName::canonical("a-model"),
            "key",
        )
        .unwrap_or_else(|error| panic!("{provider} is supported: {error}"));
        assert_eq!(resolved.provider(), provider);
    }

    let error = ResolvedAgentCapabilities::from_connection(
        Some(&company),
        None,
        &ModelProvider::canonical("unsupported_provider"),
        &ModelName::canonical("model"),
        "key",
    )
    .expect_err("the allow-list is the whole set");
    assert!(
        error
            .to_string()
            .contains("Unsupported agent provider 'unsupported_provider'")
    );
}

/// A company whose model connections and stored credential are stated outright, so each test
/// below says exactly which of them it is exercising.
struct StubCompanyPersistence {
    connections: Vec<crate::entities::company::CompanyModelConnection>,
    api_key: Option<String>,
}

impl StubCompanyPersistence {
    fn with(connections: Vec<crate::entities::company::CompanyModelConnection>) -> Self {
        Self {
            connections,
            api_key: Some("stored-company-key".into()),
        }
    }
}

fn connection(
    provider: &str,
    models: &[&str],
    is_default: bool,
) -> crate::entities::company::CompanyModelConnection {
    crate::entities::company::CompanyModelConnection {
        provider: ModelProvider::canonical(provider),
        models: models.iter().map(ModelName::canonical).collect(),
        is_default,
        has_api_key: true,
    }
}

#[async_trait::async_trait]
impl crate::use_cases::company::CompanyPersistence for StubCompanyPersistence {
    async fn create(
        &self,
        _user_id: Uuid,
        _write: crate::use_cases::company::CompanyWrite,
    ) -> crate::app_error::AppResult<Company> {
        unimplemented!()
    }
    async fn get_by_id(&self, _id: Uuid) -> crate::app_error::AppResult<Option<Company>> {
        unimplemented!()
    }
    async fn get_by_slug(&self, _slug: &str) -> crate::app_error::AppResult<Option<Company>> {
        unimplemented!()
    }
    async fn list_by_user_id(&self, _user_id: Uuid) -> crate::app_error::AppResult<Vec<Company>> {
        unimplemented!()
    }
    async fn update(
        &self,
        _id: Uuid,
        _write: crate::use_cases::company::CompanyWrite,
    ) -> crate::app_error::AppResult<Company> {
        unimplemented!()
    }
    async fn delete(&self, _id: Uuid) -> crate::app_error::AppResult<()> {
        unimplemented!()
    }
    async fn update_for_user(
        &self,
        _user_id: Uuid,
        _id: Uuid,
        _write: crate::use_cases::company::CompanyWrite,
    ) -> crate::app_error::AppResult<Company> {
        unimplemented!()
    }
    async fn delete_for_user(&self, _user_id: Uuid, _id: Uuid) -> crate::app_error::AppResult<()> {
        unimplemented!()
    }
    async fn list_company_team_emails(
        &self,
        _company_id: Uuid,
    ) -> crate::app_error::AppResult<Vec<String>> {
        unimplemented!()
    }
    async fn list_company_team_accounts(
        &self,
        _company_id: Uuid,
    ) -> crate::app_error::AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
        unimplemented!()
    }
    async fn list_model_connections(
        &self,
        _company_id: Uuid,
    ) -> crate::app_error::AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
        Ok(self.connections.clone())
    }
    async fn model_api_key(
        &self,
        _company_id: Uuid,
        _provider: &ModelProvider,
    ) -> crate::app_error::AppResult<Option<String>> {
        Ok(self.api_key.clone())
    }
    async fn replace_model_connections_for_user(
        &self,
        _user_id: Uuid,
        _company_id: Uuid,
        _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
    ) -> crate::app_error::AppResult<()> {
        unimplemented!()
    }
}

fn agent_selecting(provider: Option<&str>, model: Option<&str>) -> AgentEntity {
    AgentEntity {
        memory_enabled: false,
        id: Uuid::new_v4(),
        company_id: None,
        name: "Selector".into(),
        slug: "selector".into(),
        provider: provider.map(str::to_string),
        model: model.map(str::to_string),
        run_timeout_secs: None,
        system_prompt: Some("Answer the question.".into()),
        description: None,
        harness_kind: HarnessKind::default(),
        granted_tool_ids: Vec::new(),
        native_tool_policy: crate::entities::harness::NativeToolPolicy::default(),
        config_json: None,
        memory_persistence_mode: Default::default(),
        memory_recall_mode: Default::default(),
        memory_max_results: crate::entities::memory::default_memory_max_results(),
        avatar_url: None,
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: chrono::Utc::now(),
    }
}

struct StubCapabilityReader {
    snapshot: Option<StoredAgentCapabilities>,
    failure: Option<String>,
}

fn capabilities(agent: AgentEntity) -> StubCapabilityReader {
    StubCapabilityReader {
        snapshot: Some(StoredAgentCapabilities {
            agent,
            skills: Vec::new(),
            sub_agent_scope: SubAgentScope::AllCompanySiblings,
        }),
        failure: None,
    }
}

#[async_trait::async_trait]
impl AgentCapabilityReader for StubCapabilityReader {
    async fn load_for_execution(
        &self,
        execution_company_id: Uuid,
        agent_id: Uuid,
    ) -> crate::app_error::AppResult<Option<StoredAgentCapabilities>> {
        if let Some(message) = &self.failure {
            return Err(crate::app_error::AppError::Database(message.clone()));
        }
        Ok(self
            .snapshot
            .as_ref()
            .filter(|snapshot| {
                snapshot.agent.id == agent_id
                    && snapshot
                        .agent
                        .company_id
                        .is_none_or(|company_id| company_id == execution_company_id)
            })
            .cloned())
    }
}

fn resolving_company() -> Company {
    Company {
        channel_defaults: Default::default(),
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        enable_llm_spam_guardrail: None,
        avatar_url: None,
        memory_provider: None,
        created_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn an_agent_that_selects_nothing_inherits_the_default_connection_and_its_first_model() {
    let persistence = StubCompanyPersistence::with(vec![
        connection("anthropic", &["claude-a"], false),
        connection("openai", &["gpt-first", "gpt-second"], true),
    ]);
    let company = resolving_company();

    let agent = agent_selecting(None, None);
    let resolved = resolve_agent_capabilities(
        &persistence,
        &capabilities(agent.clone()),
        &company,
        agent.id,
    )
    .await
    .expect("the default connection resolves");

    assert_eq!(resolved.provider(), "openai");
    assert_eq!(resolved.model(), "gpt-first");
    // The credential comes from the company's stored connection, never from the agent.
    assert_eq!(resolved.api_key(), "stored-company-key");
}

#[tokio::test]
async fn a_company_with_no_default_connection_cannot_resolve_an_agent() {
    let persistence =
        StubCompanyPersistence::with(vec![connection("openai", &["gpt-first"], false)]);

    let agent = agent_selecting(None, None);
    let error = resolve_agent_capabilities(
        &persistence,
        &capabilities(agent.clone()),
        &resolving_company(),
        agent.id,
    )
    .await
    .expect_err("a company with no default cannot run an agent");

    assert!(
        error
            .to_string()
            .contains("Company default model connection is missing"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn an_agent_cannot_select_a_provider_its_company_has_not_enabled() {
    let persistence =
        StubCompanyPersistence::with(vec![connection("openai", &["gpt-first"], true)]);
    let agent = agent_selecting(Some("anthropic"), Some("claude-a"));

    let error = resolve_agent_capabilities(
        &persistence,
        &capabilities(agent.clone()),
        &resolving_company(),
        agent.id,
    )
    .await
    .expect_err("a provider the company never configured is not usable");

    assert!(
        error
            .to_string()
            .contains("Provider 'anthropic' is not enabled for this company"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn an_agent_cannot_select_a_model_outside_its_providers_allow_list() {
    let persistence =
        StubCompanyPersistence::with(vec![connection("openai", &["gpt-first"], true)]);
    let agent = agent_selecting(Some("openai"), Some("gpt-unlisted"));

    let error = resolve_agent_capabilities(
        &persistence,
        &capabilities(agent.clone()),
        &resolving_company(),
        agent.id,
    )
    .await
    .expect_err("the allow-list is the whole set of models an agent may pick");

    assert!(
        error
            .to_string()
            .contains("Model 'gpt-unlisted' is not enabled for provider 'openai' in this company"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn provider_selection_folds_case_while_model_selection_does_not() {
    let persistence =
        StubCompanyPersistence::with(vec![connection("openai", &["gpt-first"], true)]);
    let company = resolving_company();

    let shouting = agent_selecting(Some("OpenAI"), Some("gpt-first"));
    let resolved = resolve_agent_capabilities(
        &persistence,
        &capabilities(shouting.clone()),
        &company,
        shouting.id,
    )
    .await
    .expect("providers are matched case-insensitively");
    assert_eq!(resolved.provider(), "openai");

    // Model ids are provider-assigned and case-sensitive, so this one is genuinely absent.
    let miscased = agent_selecting(Some("openai"), Some("GPT-First"));
    assert!(
        resolve_agent_capabilities(
            &persistence,
            &capabilities(miscased.clone()),
            &company,
            miscased.id
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn a_connection_without_a_stored_credential_refuses_to_build_provider_params() {
    let persistence = StubCompanyPersistence {
        api_key: None,
        ..StubCompanyPersistence::with(vec![connection("openai", &["gpt-first"], true)])
    };

    let agent = agent_selecting(None, None);
    let error = resolve_agent_capabilities(
        &persistence,
        &capabilities(agent.clone()),
        &resolving_company(),
        agent.id,
    )
    .await
    .expect_err("no credential means no provider call");

    assert!(
        error.to_string().contains("API key is missing"),
        "unexpected error: {error}"
    );
}

fn skill(slug: &str) -> Skill {
    let now = chrono::Utc::now();
    Skill {
        id: Uuid::new_v4(),
        company_id: None,
        slug: slug.into(),
        name: slug.to_string(),
        description: format!("{slug} description"),
        trigger: format!("Use {slug}"),
        instructions: vec![SkillInstruction::Prompt {
            text: format!("Apply {slug}"),
        }],
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn a_spec_carries_its_agents_skills_in_position_order() {
    let persistence =
        StubCompanyPersistence::with(vec![connection("openai", &["gpt-first"], true)]);
    let company = resolving_company();
    let mut agent = agent_selecting(None, None);
    agent.granted_tool_ids = vec![ToolId::from("calculator")];
    let first = skill("first");
    let second = skill("second");
    let reader = StubCapabilityReader {
        snapshot: Some(StoredAgentCapabilities {
            agent: agent.clone(),
            skills: vec![first.clone(), second.clone()],
            sub_agent_scope: SubAgentScope::Restricted(vec![Uuid::new_v4()]),
        }),
        failure: None,
    };

    let resolved = resolve_agent_capabilities(&persistence, &reader, &company, agent.id)
        .await
        .expect("the stored snapshot resolves");

    assert_eq!(resolved.agent_id, agent.id);
    assert_eq!(resolved.spec.skills, vec![first, second]);
    assert_eq!(resolved.spec.granted_tools, [ToolId::from("calculator")]);
    assert!(matches!(
        resolved.spec.sub_agents,
        SubAgentScope::Restricted(_)
    ));
}

#[tokio::test]
async fn a_scope_read_error_fails_the_run_instead_of_becoming_unrestricted() {
    let persistence =
        StubCompanyPersistence::with(vec![connection("openai", &["gpt-first"], true)]);
    let agent = agent_selecting(None, None);
    let reader = StubCapabilityReader {
        snapshot: None,
        failure: Some("scope unavailable".into()),
    };

    let error = resolve_agent_capabilities(&persistence, &reader, &resolving_company(), agent.id)
        .await
        .expect_err("a failed authorization read must fail execution");

    assert!(matches!(error, crate::app_error::AppError::Database(_)));
    assert!(error.to_string().contains("scope unavailable"));
}

#[tokio::test]
async fn an_absent_capability_snapshot_fails_explicitly() {
    let persistence =
        StubCompanyPersistence::with(vec![connection("openai", &["gpt-first"], true)]);
    let agent_id = Uuid::new_v4();
    let reader = StubCapabilityReader {
        snapshot: None,
        failure: None,
    };

    let error = resolve_agent_capabilities(&persistence, &reader, &resolving_company(), agent_id)
        .await
        .expect_err("absence must not become an empty capability");

    assert!(matches!(error, crate::app_error::AppError::NotFound(_)));
    assert!(error.to_string().contains(&agent_id.to_string()));
}
