use super::*;
use crate::entities::{
    company::{Company, CompanyAccess},
    company_member::CompanyMembership,
    value_objects::EmailAddress,
};
use crate::use_cases::company::CompanyWrite;
use chrono::Utc;

use std::sync::Mutex;

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
            id: Uuid::new_v4(),
            company_id: Some(company_id),
            name: write.name,
            slug: write.slug,
            provider: write.provider,
            model: write.model,
            system_prompt: write.system_prompt,
            description: write.description,
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

        agent.name = write.name;
        agent.slug = write.slug;
        agent.provider = write.provider;
        agent.model = write.model;
        agent.system_prompt = write.system_prompt;
        agent.description = write.description;
        agent.avatar_url = write.avatar_url;
        Ok(agent.clone())
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        self.agents.lock().unwrap().retain(|a| a.id != id);
        Ok(())
    }
}

#[async_trait]
impl LibraryAgentCopyPersistence for MockAgentPersistence {
    async fn create_agent_from_library(
        &self,
        company_id: Uuid,
        _library_agent_id: Uuid,
        agent: AgentWrite,
    ) -> AppResult<Agent> {
        AgentPersistence::create(self, company_id, agent).await
    }
}

/// A company, its owner, and the use cases over mock persistence.
struct Fixture {
    owner_id: Uuid,
    company_id: Uuid,
    use_cases: AgentUseCases,
}

fn fixture() -> Fixture {
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
            avatar_url: None,
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
        ),
    }
}

fn support_agent() -> AgentWrite {
    AgentWrite {
        name: "Support Triage".into(),
        slug: "support-triage".into(),
        ..Default::default()
    }
}

/// p08 D4.7: a created agent has no channel; its address is an assistant channel made later.
/// The mock's owned-channel write is unreachable, so reaching it fails the test.
#[tokio::test]
async fn creating_an_agent_creates_only_the_agent() {
    let fixture = fixture();
    let agent = fixture
        .use_cases
        .create_agent(fixture.owner_id, fixture.company_id, support_agent())
        .await
        .expect("the agent is created");
    assert_eq!(agent.slug, "support-triage");
    assert_eq!(agent.company_id, Some(fixture.company_id));
    assert_eq!(agent.provider, None);
}

/// The Simple tab checks the agent before it expands the prompt, so the two must refuse the same
/// things.
#[tokio::test]
async fn validating_a_new_agent_normalizes_the_handle_and_refuses_an_unavailable_model() {
    let fixture = fixture();

    let validated = fixture
        .use_cases
        .validate_new_agent(
            fixture.owner_id,
            fixture.company_id,
            AgentWrite {
                name: "  Support Triage  ".into(),
                slug: "Support Triage".into(),
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
            avatar_url: None,
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
    );

    // 0. Invalid reserved suffix slug rejection test
    let invalid_res = use_cases
        .create_agent(
            owner_id,
            company_id,
            AgentWrite {
                name: "Quiet Bot".to_string(),
                slug: "quiet".to_string(),
                ..Default::default()
            },
        )
        .await;
    assert!(invalid_res.is_err());

    // 1. Owner creates agent

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

    // 2. Non-owner cannot create agent
    let non_owner_id = Uuid::new_v4();
    let err = use_cases
        .create_agent(
            non_owner_id,
            company_id,
            AgentWrite {
                name: "Hacker Bot".to_string(),
                slug: "hacker-bot".to_string(),
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
        avatar_url: None,
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
    );

    let agent = use_cases
        .create_agent(
            owner_id,
            company_id,
            AgentWrite {
                name: "Support Bot".to_string(),
                slug: "support-bot".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("the owner creates the agent");

    let viewer = |user_id: Uuid, email: &str| Viewer {
        user_id,
        email: EmailAddress::from(email),
        email_verified_at: Some(chrono::Utc::now()),
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
            avatar_url: None,
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
        system_prompt: Some("Answer briefly.".into()),
        description: Some("Sorts support mail".into()),
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
    );

    // The definition is copied field for field, and the company owns the copy.
    let provisioned = use_cases
        .create_agent_from_library(
            owner_id,
            company_id,
            runnable.id,
            LibraryCopyModel::FollowDefault,
        )
        .await
        .expect("the owner picks a library agent");
    assert_eq!(provisioned.agent.company_id, Some(company_id));
    assert_ne!(provisioned.agent.id, runnable.id);
    assert_eq!(provisioned.agent.name, "Triage");
    assert_eq!(provisioned.agent.slug, "triage");
    assert_eq!(provisioned.agent.provider.as_deref(), Some("openai"));
    assert_eq!(provisioned.agent.model.as_deref(), Some("gpt-4o"));
    assert_eq!(
        provisioned.agent.system_prompt.as_deref(),
        Some("Answer briefly.")
    );
    assert!(provisioned.warnings.is_empty());

    // A definition published on a model this company has not enabled still gets created --
    // on the company default, and it says so rather than failing the pick.
    let fallback = use_cases
        .create_agent_from_library(
            owner_id,
            company_id,
            elsewhere.id,
            LibraryCopyModel::FollowDefault,
        )
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

    // Onboarding pins a copy that would inherit to the company default (p08 D4.7), so its
    // assistant channel can run it; the pick still says why its own model was dropped.
    let pinned = use_cases
        .create_agent_from_library(
            owner_id,
            company_id,
            elsewhere.id,
            LibraryCopyModel::PinDefault,
        )
        .await
        .expect("a pinned pick");
    assert_eq!(pinned.agent.provider.as_deref(), Some("openai"));
    assert_eq!(pinned.agent.model.as_deref(), Some("gpt-4o"));
    assert_eq!(
        pinned
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        vec!["library_model_unavailable"]
    );
    // A copy published on a model the company runs keeps it.
    let kept = use_cases
        .create_agent_from_library(
            owner_id,
            company_id,
            runnable.id,
            LibraryCopyModel::PinDefault,
        )
        .await
        .expect("a pick on an enabled model");
    assert_eq!(kept.agent.model.as_deref(), Some("gpt-4o"));

    // A company agent is not a library definition, so it cannot be picked as one.
    assert!(
        use_cases
            .create_agent_from_library(
                owner_id,
                company_id,
                provisioned.agent.id,
                LibraryCopyModel::FollowDefault,
            )
            .await
            .is_err()
    );
}
