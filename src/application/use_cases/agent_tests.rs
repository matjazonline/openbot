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
        harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
        ..Default::default()
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
        harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
        ..Default::default()
    };
    inherited.normalize().unwrap();

    let mut invalid = AgentWrite {
        name: "Invalid".into(),
        slug: "invalid".into(),
        run_timeout_secs: Some(MAX_AGENT_RUN_TIMEOUT_SECS + 1),
        harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
        ..Default::default()
    };
    assert!(invalid.normalize().is_err());

    let agent = Agent {
        response_contract: None,
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
        harness_kind: HarnessKind::default(),
        granted_tool_ids: Vec::new(),
        native_tool_policy: NativeToolPolicy::default(),
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
fn agent_config_accepts_only_reviewed_settings_and_rejects_authority_paths() {
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
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        };
        assert!(write.normalize().is_err());
    }

    let mut allowed = AgentWrite {
        name: "Configured".into(),
        slug: "configured".into(),
        config_json: Some(json!({
            "version": 1,
            "reasoning": {"mode": "react", "max_iterations": 4},
            "reflection": {"enabled": "auto", "max_retries": 2},
            "disambiguation": {"enabled": true}
        })),
        harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
        ..Default::default()
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
            response_contract: write.response_contract.0.flatten(),
            memory_persistence_mode: crate::entities::memory::MemoryPersistenceMode::AudienceOnly,
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
            harness_kind: write.harness_kind.expect("use case resolves harness"),
            granted_tool_ids: write.granted_tool_ids,
            native_tool_policy: write.native_tool_policy,
            config_json: write.config_json,
            memory_enabled: write.memory_enabled,
            avatar_url: write.avatar_url,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };
        self.agents.lock().unwrap().push(agent.clone());
        Ok(agent)
    }

    async fn create_library(&self, write: AgentWrite) -> AppResult<Agent> {
        let mut agent = self.create(Uuid::nil(), write).await?;
        agent.company_id = None;
        if let Some(stored) = self
            .agents
            .lock()
            .unwrap()
            .iter_mut()
            .find(|stored| stored.id == agent.id)
        {
            stored.company_id = None;
        }
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

    async fn list_library(&self) -> AppResult<Vec<Agent>> {
        Ok(self
            .agents
            .lock()
            .unwrap()
            .iter()
            .filter(|agent| agent.company_id.is_none())
            .cloned()
            .collect())
    }

    async fn update(&self, id: Uuid, write: AgentWrite) -> AppResult<Agent> {
        let mut list = self.agents.lock().unwrap();
        let agent = list
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| AppError::Internal("Not found".into()))?;

        agent.harness_kind = write.harness_kind.unwrap_or(agent.harness_kind);
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

    async fn create_owned_agent_channel_from_library(
        &self,
        company_id: Uuid,
        _library_agent_id: Uuid,
        agent: AgentWrite,
        channel: ChannelWrite,
    ) -> AppResult<(Agent, Channel)> {
        self.create_owned_agent_channel(company_id, agent, channel)
            .await
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
        harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
        ..Default::default()
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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
            },
        )
        .await;
    assert!(invalid_res.is_err());

    // 1. Owner creates agent with config_json
    let config = json!({
        "version": 1,
        "reasoning": {"mode": "auto", "max_iterations": 5}
    });

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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
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
    let updated_config = json!({
        "version": 1,
        "reflection": {"enabled": "enabled", "max_retries": 1}
    });
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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
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
                harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
                ..Default::default()
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
        response_contract: None,
        id: Uuid::new_v4(),
        company_id: None,
        name: name.to_string(),
        slug: slug.to_string(),
        provider: Some(provider.to_string()),
        model: Some(model.to_string()),
        run_timeout_secs: Some(90),
        system_prompt: Some("Answer briefly.".into()),
        description: Some("Sorts support mail".into()),
        harness_kind: HarnessKind::default(),
        granted_tool_ids: Vec::new(),
        native_tool_policy: NativeToolPolicy::default(),
        config_json: None,
        memory_enabled: true,
        memory_persistence_mode: MemoryPersistenceMode::AudienceOnly,
        memory_recall_mode: MemoryRecallMode::Fast,
        memory_max_results: 7,
        avatar_url: None,
        created_by: CreationProvenance::system(),
        created_at: Utc::now(),
    };
    let mut runnable = definition("Triage", "triage", "openai", "gpt-4o");
    runnable.harness_kind = HarnessKind::Rig;
    runnable.response_contract = Some(
        serde_json::from_value(
            json!({"version":1,"format":"json_schema","schema":{"type":"object"}}),
        )
        .unwrap(),
    );
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
    )
    .with_response_validator(Arc::new(
        crate::adapters::response_schema::JsonResponseValidator,
    ));

    // The definition is copied field for field, and the company owns the copy.
    let provisioned = use_cases
        .create_agent_from_library(owner_id, company_id, runnable.id)
        .await
        .expect("the owner picks a library agent");
    assert_eq!(provisioned.agent.company_id, Some(company_id));
    assert_ne!(provisioned.agent.id, runnable.id);
    assert_eq!(
        provisioned.agent.response_contract,
        runnable.response_contract
    );
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
#[tokio::test]
async fn omitted_creates_use_injected_policy_and_omitted_updates_keep_the_selection() {
    for default in HarnessKind::ALL {
        let fixture = fixture(CompanyChannelDefaults::default(), SpamScanning::Available);
        let use_cases = fixture.use_cases.with_default_agent_harness(default);
        let write = AgentWrite {
            name: "Inherited".into(),
            slug: "inherited".into(),
            ..Default::default()
        };
        let created = use_cases
            .create_agent(fixture.owner_id, fixture.company_id, write.clone())
            .await
            .unwrap();
        assert_eq!(created.harness_kind, default);
        assert!(created.config_json.is_none());
        let other = if default == HarnessKind::Rig {
            HarnessKind::AiAgents
        } else {
            HarnessKind::Rig
        };
        let use_cases = use_cases.with_default_agent_harness(other);
        let updated = use_cases
            .update_agent(fixture.owner_id, fixture.company_id, created.id, write)
            .await
            .unwrap();
        assert_eq!(updated.harness_kind, default);
        let explicit = AgentWrite {
            name: "Explicit".into(),
            slug: "explicit".into(),
            harness_kind: Some(default),
            ..Default::default()
        };
        assert_eq!(
            use_cases
                .create_agent(fixture.owner_id, fixture.company_id, explicit)
                .await
                .unwrap()
                .harness_kind,
            default
        );
    }
}

#[tokio::test]
async fn a_harness_switch_rejects_carried_settings_but_accepts_an_explicit_empty_target() {
    let fixture = fixture(CompanyChannelDefaults::default(), SpamScanning::Available);
    let initial = AgentWrite {
        name: "Switch".into(),
        slug: "switch".into(),
        harness_kind: Some(HarnessKind::AiAgents),
        config_json: Some(
            serde_json::json!({"version":1,"reasoning":{"mode":"chain_of_thought","max_iterations":2}}),
        ),
        ..Default::default()
    };
    let created = fixture
        .use_cases
        .create_agent(fixture.owner_id, fixture.company_id, initial)
        .await
        .unwrap();
    let target = AgentWrite {
        name: "Switch".into(),
        slug: "switch".into(),
        harness_kind: Some(HarnessKind::Rig),
        ..Default::default()
    };
    assert!(
        fixture
            .use_cases
            .update_agent(
                fixture.owner_id,
                fixture.company_id,
                created.id,
                target.clone()
            )
            .await
            .is_err()
    );
    let mut explicit = target;
    explicit.config_json = Some(serde_json::json!({"version":1}));
    let updated = fixture
        .use_cases
        .update_agent(fixture.owner_id, fixture.company_id, created.id, explicit)
        .await
        .unwrap();
    assert_eq!(updated.harness_kind, HarnessKind::Rig);
    assert_eq!(
        HarnessConfig::parse(updated.harness_kind, updated.config_json.as_ref()).unwrap(),
        HarnessConfig::empty(HarnessKind::Rig)
    );
}
