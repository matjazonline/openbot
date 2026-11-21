//! Real agent authoring and construction under delegated unit ownership.
use super::*;
use crate::adapters::persistence::company_agent::CompanyAgentService;
use crate::application::agent::*;
use crate::entities::agent_capability::{AgentBuiltinTool, AgentId, AgentToolAlias};
use crate::use_cases::agent::{
    AgentPersistence, AgentPlacement, AgentUseCases, AgentWrite, CompanyAgents, CreatedAgent,
    SpamScanning,
};
use crate::use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence};
use crate::use_cases::skill::{AgentCapabilityReader, SkillManagementPersistence, SkillWrite};

struct ManagedAgents {
    _database: crate::adapters::persistence::test_support::OwnDatabase,
    d: Delegated,
    cases: AgentUseCases,
    service: Arc<CompanyAgentService>,
}

fn write(slug: &str) -> AgentWrite {
    AgentWrite {
        name: slug.into(),
        slug: slug.into(),
        provider: Some("openai".into()),
        model: Some("gpt-4o-mini".into()),
        system_prompt: Some("Answer carefully.".into()),
        ..Default::default()
    }
}

impl ManagedAgents {
    async fn new() -> Option<Self> {
        let database = own_database().await?;
        let mut d = Delegated::on(database.pool.clone()).await;
        d.f.persistence = d.f.persistence.with_app_domain("agents.test");
        let p = Arc::new(d.f.persistence.clone());
        CompanyPersistence::replace_model_connections_for_user(
            p.as_ref(),
            d.f.owner,
            d.f.company,
            vec![CompanyModelConnectionWrite {
                provider: "openai".into(),
                api_key: Some("fixture-key".into()),
                models: vec!["gpt-4o-mini".into()],
                is_default: true,
            }],
        )
        .await
        .unwrap();
        let service = Arc::new(CompanyAgentService::new(
            p.as_ref().clone(),
            provisioning_templates(),
        ));
        let cases = AgentUseCases::new(p.clone(), p.clone(), p, SpamScanning::Available)
            .with_company_agents(service.clone());
        Some(Self {
            _database: database,
            d,
            cases,
            service,
        })
    }

    fn p(&self) -> &PostgresPersistence {
        &self.d.f.persistence
    }
    fn actor(&self) -> WorkflowActor {
        self.d.scope(self.d.f.member).actor
    }
    fn scope(&self, actor: WorkflowActor, agent: Uuid) -> AgentPublicationScope {
        AgentPublicationScope {
            company: self.d.company(),
            actor,
            agent: AgentId::new(agent),
        }
    }
    async fn create(&self, slug: &str, unit: DepartmentId) -> CreatedAgent {
        self.cases
            .create_agent(
                self.actor().user_id(),
                self.d.f.company,
                write(slug),
                Some(AgentPlacement {
                    department: unit,
                    role: None,
                }),
            )
            .await
            .unwrap()
    }
    async fn changes(&self, actor: WorkflowActor, agent: Uuid) -> SaveCompanyAgent {
        let state = self
            .p()
            .load_capabilities(self.d.f.company, agent)
            .await
            .unwrap()
            .unwrap();
        SaveCompanyAgent {
            scope: self.scope(actor, agent),
            expected_revision: state.agent.draft_revision,
            tools: None,
            write: AgentWrite {
                name: state.agent.name,
                slug: state.agent.slug,
                provider: state.agent.provider,
                model: state.agent.model,
                system_prompt: Some("Updated within the unit.".into()),
                skill_ids: state.skills.into_iter().map(|skill| skill.id).collect(),
                ..Default::default()
            },
        }
    }
}

#[tokio::test]
async fn delegated_admin_agent_create_edit_skills_tools_publish_and_delete_need_no_desk_view() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let remote = m.d.remote().await;
    let desk = m.d.f.desk_of(remote.id).await;
    assert_no_desk_view(&m, desk).await;
    let created = m.create("delegated-author", remote.id).await;
    let initial = m
        .p()
        .active_agent_revision(m.scope(m.actor(), created.agent.id))
        .await
        .unwrap()
        .unwrap();
    let skill = SkillManagementPersistence::create_company(
        m.p(),
        m.d.f.company,
        m.p().test_root_unit(m.d.f.company).await,
        SkillWrite {
            slug: "accuracy".into(),
            name: "Accuracy".into(),
            description: "Check the answer".into(),
            instructions: "Use exact, supported answers.".into(),
            files: None,
            required_tools: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
    let mut command = m.changes(m.actor(), created.agent.id).await;
    command.write.skill_ids = vec![skill.id];
    let updated = m
        .cases
        .update_agent(
            m.actor().user_id(),
            m.d.f.company,
            CompanyAgentUpdate {
                agent: command.scope.agent,
                expected_revision: command.expected_revision,
                write: command.write,
                tools: Some(vec![AgentToolAssignment {
                    binding: None,
                    alias: AgentToolAlias::parse("unit.send").unwrap(),
                    choice: AgentToolChoice::Builtin {
                        tool: AgentBuiltinTool::MessageSend,
                    },
                }]),
            },
        )
        .await
        .unwrap();
    let published = m
        .p()
        .publish_agent(PublishAgent {
            scope: m.scope(m.actor(), created.agent.id),
            expected_draft_revision: updated.draft_revision,
            expected_active_revision: Some(initial.id),
            key: AgentPublicationKey::parse("unit-publish").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(published.definition.skills[0].skill.skill_id, skill.id);
    assert_eq!(published.definition.tools.len(), 1);
    m.p()
        .revoke_agent_grant(
            m.scope(m.actor(), created.agent.id),
            AgentGrantIdentity::Binding(published.definition.tools[0].binding.id),
        )
        .await
        .unwrap();
    assert!(
        matches!(
            m.cases
                .delete_agent(m.actor().user_id(), m.d.f.company, created.agent.id)
                .await,
            Err(AppError::Conflict(_))
        ),
        "retained capability history still prevents deletion"
    );
    let deletable = m.create("deletable-unit-agent", remote.id).await;
    m.cases
        .delete_agent(m.actor().user_id(), m.d.f.company, deletable.agent.id)
        .await
        .unwrap();
    assert!(
        AgentPersistence::get_by_id(m.p(), deletable.agent.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn delegated_admin_agent_library_copy_uses_managed_placement_and_root_when_unplaced() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let library = AgentPersistence::create_library(m.p(), write("portable"))
        .await
        .unwrap();
    let copied = m
        .cases
        .create_agent_from_library(
            m.actor().user_id(),
            m.d.f.company,
            library.id,
            Some(AgentPlacement {
                department: m.d.unit.id,
                role: None,
            }),
        )
        .await
        .unwrap();
    assert!(
        m.p()
            .active_agent_revision(m.scope(m.actor(), copied.agent.id))
            .await
            .unwrap()
            .is_some()
    );
    hidden(
        m.service
            .copy_from_library(
                m.actor(),
                m.d.company(),
                library.id,
                write("sibling-copy"),
                Some(AgentPlacement {
                    department: m.d.sibling.id,
                    role: None,
                }),
            )
            .await,
    );
    assert_root_boundaries(&m, library.id).await;
}

#[tokio::test]
async fn delegated_admin_agent_managed_lists_and_any_placement_share_the_same_predicate() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let own = m.create("owned", m.d.unit.id).await;
    let other = m
        .service
        .create(
            m.d.scope(m.d.f.owner).actor,
            m.d.company(),
            write("sibling"),
            Some(AgentPlacement {
                department: m.d.sibling.id,
                role: None,
            }),
        )
        .await
        .unwrap();
    let list = m
        .cases
        .list_company_agents(m.actor().user_id(), m.d.f.company)
        .await
        .unwrap();
    assert!(list.iter().any(|agent| agent.id == own.agent.id));
    assert!(!list.iter().any(|agent| agent.id == other.agent.id));
    let directory = m
        .p()
        .agent_directory(
            AgentCompanyScope {
                company: m.d.company(),
                actor: m.actor(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        directory
            .items
            .iter()
            .map(|entry| entry.id.as_uuid())
            .collect::<Vec<_>>(),
        vec![own.agent.id]
    );
    assert!(
        m.p()
            .agent_directory_entry(
                AgentCompanyScope {
                    company: m.d.company(),
                    actor: m.actor()
                },
                AgentKey::parse("sibling").unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
    share_with_unit(&m, other.principal_id).await;
    assert!(
        m.p()
            .preview_agent(m.scope(m.actor(), other.agent.id))
            .await
            .map(drop)
            .is_ok()
    );
    assert!(
        m.p()
            .preview_agent(m.scope(m.d.scope(m.d.sibling_lead).actor, other.agent.id))
            .await
            .map(drop)
            .is_ok()
    );
    assert!(
        m.cases
            .list_company_agents(m.actor().user_id(), m.d.f.company)
            .await
            .unwrap()
            .iter()
            .any(|agent| agent.id == other.agent.id)
    );
}

#[tokio::test]
async fn delegated_admin_agent_persistence_refuses_sibling_foreign_agent_and_library_bypasses() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let own = m.create("owned", m.d.unit.id).await;
    let sibling = m.d.scope(m.d.sibling_lead).actor;
    let command = m.changes(sibling, own.agent.id).await;
    hidden(m.p().save_company_agent(command).await);
    hidden(
        m.p()
            .delete_company_agent(m.scope(sibling, own.agent.id))
            .await,
    );
    hidden(
        m.p()
            .preview_agent(m.scope(sibling, own.agent.id))
            .await
            .map(drop),
    );
    hidden(
        m.p()
            .delete_company_agent(AgentPublicationScope {
                company: CompanyId::new(m.d.f.foreign_company),
                ..m.scope(m.actor(), own.agent.id)
            })
            .await,
    );
    hidden(
        m.p()
            .delete_company_agent(m.scope(
                WorkflowActor::authenticated(own.agent.id).unwrap(),
                own.agent.id,
            ))
            .await,
    );
    hidden(
        AgentPersistence::update_library_agent(m.p(), own.agent.id, write("library-bypass")).await,
    );
    hidden(AgentPersistence::delete_library_agent(m.p(), own.agent.id).await);
    hidden(
        m.service
            .create(
                m.actor(),
                CompanyId::new(m.d.f.foreign_company),
                write("foreign"),
                Some(AgentPlacement {
                    department: m.d.unit.id,
                    role: None,
                }),
            )
            .await,
    );
    assert!(
        AgentPersistence::get_by_id(m.p(), own.agent.id)
            .await
            .unwrap()
            .is_some()
    );
}

#[path = "delegated_agent_construction_tests.rs"]
mod construction;
#[path = "delegated_agent_race_tests.rs"]
mod races;

async fn assert_no_desk_view(m: &ManagedAgents, desk: Uuid) {
    let authorizer = crate::application::workflow::LifecycleAuthorizer::new(
        m.p().clone(),
        m.p().clone(),
        m.p().clone(),
    );
    assert!(matches!(
        crate::application::workflow::WorkflowAuthorization::authorize(
            &authorizer,
            m.d.company(),
            m.actor(),
            crate::application::workflow::RelatedAssociation::Channel(
                crate::application::workflow::RelatedChannelId::new(desk)
            ),
            crate::application::workflow::WorkflowOperation::Inspect
        )
        .await,
        Err(AppError::NotFound(_))
    ));
}

async fn assert_root_boundaries(m: &ManagedAgents, library: Uuid) {
    hidden(
        m.service
            .create(m.actor(), m.d.company(), write("unplaced"), None)
            .await,
    );
    hidden(
        m.cases
            .create_agent_from_library(m.actor().user_id(), m.d.f.company, library, None)
            .await,
    );
    hidden(
        m.service
            .create(
                m.actor(),
                m.d.company(),
                write("own-unit-lead"),
                Some(AgentPlacement {
                    department: m.d.unit.id,
                    role: Some(DepartmentRole::Lead),
                }),
            )
            .await,
    );
    let unplaced = m
        .service
        .create(
            m.d.scope(m.d.f.owner).actor,
            m.d.company(),
            write("root-agent"),
            None,
        )
        .await
        .unwrap();
    hidden(
        m.p()
            .preview_agent(m.scope(m.actor(), unplaced.agent.id))
            .await
            .map(drop),
    );
}

async fn share_with_unit(m: &ManagedAgents, principal: PrincipalId) {
    m.d.f
        .use_cases
        .add_member(m.d.f.owner, m.d.f.company, m.d.unit.id, principal)
        .await
        .unwrap();
}

/// Plan07 01B.4: a move names the agents whose active revision uses the skill.
#[tokio::test]
async fn unit_resource_move_lists_the_agents_whose_active_revision_uses_a_skill() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let created = m.create("unit-skill-user", m.d.unit.id).await;
    let initial = m
        .p()
        .active_agent_revision(m.scope(m.actor(), created.agent.id))
        .await
        .unwrap()
        .unwrap();
    let skill = SkillManagementPersistence::create_company(
        m.p(),
        m.d.f.company,
        m.d.unit.id.as_uuid(),
        SkillWrite {
            slug: "sales-policy".into(),
            name: "Sales policy".into(),
            description: "Quote the price list".into(),
            instructions: "Quote only the current price list.".into(),
            files: None,
            required_tools: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
    let mut command = m.changes(m.actor(), created.agent.id).await;
    command.write.skill_ids = vec![skill.id];
    let updated = m
        .cases
        .update_agent(
            m.actor().user_id(),
            m.d.f.company,
            CompanyAgentUpdate {
                agent: command.scope.agent,
                expected_revision: command.expected_revision,
                write: command.write,
                tools: None,
            },
        )
        .await
        .unwrap();
    m.p()
        .publish_agent(PublishAgent {
            scope: m.scope(m.actor(), created.agent.id),
            expected_draft_revision: updated.draft_revision,
            expected_active_revision: Some(initial.id),
            key: AgentPublicationKey::parse("unit-skill-publish").unwrap(),
        })
        .await
        .unwrap();
    let moved = SkillManagementPersistence::move_company(
        m.p(),
        m.d.f.owner,
        m.d.f.company,
        skill.id,
        m.d.child.id.as_uuid(),
    )
    .await
    .unwrap();
    assert_eq!((moved.from, moved.to), (m.d.unit.id, m.d.child.id));
    assert_eq!(moved.more_uses, 0);
    assert_eq!(
        moved
            .uses
            .iter()
            .map(|used| (used.kind, used.id, used.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(
            crate::application::ports::unit_resources::ResourceUseKind::Agent,
            created.agent.id,
            "unit-skill-user"
        )]
    );
}

/// Plan07: a library agent's skills are copied into the new agent's placement unit, or the root
/// when it has none.
#[tokio::test]
async fn unit_resource_library_agent_copies_skills_into_its_placement_or_the_root() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let library_skill = SkillManagementPersistence::create_library(
        m.p(),
        SkillWrite {
            slug: "portable-policy".into(),
            name: "Portable policy".into(),
            description: "Shared guidance".into(),
            instructions: "Follow the shared guidance.".into(),
            files: None,
            required_tools: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
    let mut library_write = write("portable-skilled");
    library_write.skill_ids = vec![library_skill.id];
    let library = AgentPersistence::create_library(m.p(), library_write)
        .await
        .unwrap();
    let skill_units = |agent: Uuid| {
        let pool = m.d.f.pool().clone();
        async move {
            sqlx::query_scalar::<_, Option<Uuid>>(
                "SELECT skill.unit_id FROM agent_skills AS link JOIN skills AS skill \
                 ON skill.id = link.skill_id WHERE link.agent_id = $1",
            )
            .bind(agent)
            .fetch_all(&pool)
            .await
            .unwrap()
        }
    };
    let placed = m
        .cases
        .create_agent_from_library(
            m.actor().user_id(),
            m.d.f.company,
            library.id,
            Some(AgentPlacement {
                department: m.d.unit.id,
                role: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        skill_units(placed.agent.id).await,
        vec![Some(m.d.unit.id.as_uuid())]
    );
    // The owner's own default is the root, so placing into Sales shows the placement decides.
    let mut owner_placed_write = write("portable-owner-placed");
    owner_placed_write.skill_ids = vec![library_skill.id];
    let owner_placed_library = AgentPersistence::create_library(m.p(), owner_placed_write)
        .await
        .unwrap();
    let owner_placed = m
        .cases
        .create_agent_from_library(
            m.d.f.owner,
            m.d.f.company,
            owner_placed_library.id,
            Some(AgentPlacement {
                department: m.d.unit.id,
                role: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        skill_units(owner_placed.agent.id).await,
        vec![Some(m.d.unit.id.as_uuid())]
    );
    let mut unplaced_write = write("portable-unplaced");
    unplaced_write.skill_ids = vec![library_skill.id];
    let unplaced_library = AgentPersistence::create_library(m.p(), unplaced_write)
        .await
        .unwrap();
    let unplaced = m
        .cases
        .create_agent_from_library(m.d.f.owner, m.d.f.company, unplaced_library.id, None)
        .await
        .unwrap();
    let root = m.p().test_root_unit(m.d.f.company).await;
    assert_eq!(skill_units(unplaced.agent.id).await, vec![Some(root)]);
}

/// B1-F5: uses in units the mover does not manage are counted, not named. With decision d an
/// agent can only publish what it reaches, so the unmanaged use is an agent re-placed elsewhere
/// after publishing (its active revision still uses the skill).
#[tokio::test]
async fn unit_resource_move_counts_but_does_not_name_unmanaged_uses() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let owner = WorkflowActor::authenticated(m.d.f.owner).unwrap();
    let created = m
        .cases
        .create_agent(
            m.d.f.owner,
            m.d.f.company,
            write("sales-then-support"),
            Some(AgentPlacement {
                department: m.d.unit.id,
                role: None,
            }),
        )
        .await
        .unwrap();
    let initial = m
        .p()
        .active_agent_revision(m.scope(owner, created.agent.id))
        .await
        .unwrap()
        .unwrap();
    let skill = SkillManagementPersistence::create_company(
        m.p(),
        m.d.f.company,
        m.d.unit.id.as_uuid(),
        SkillWrite {
            slug: "shared-sales-policy".into(),
            name: "Shared sales policy".into(),
            description: "Quote the price list".into(),
            instructions: "Quote only the current price list.".into(),
            files: None,
            required_tools: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
    let mut command = m.changes(owner, created.agent.id).await;
    command.write.skill_ids = vec![skill.id];
    let updated = m
        .cases
        .update_agent(
            m.d.f.owner,
            m.d.f.company,
            CompanyAgentUpdate {
                agent: command.scope.agent,
                expected_revision: command.expected_revision,
                write: command.write,
                tools: None,
            },
        )
        .await
        .unwrap();
    m.p()
        .publish_agent(PublishAgent {
            scope: m.scope(owner, created.agent.id),
            expected_draft_revision: updated.draft_revision,
            expected_active_revision: Some(initial.id),
            key: AgentPublicationKey::parse("sales-publish").unwrap(),
        })
        .await
        .unwrap();
    // The owner re-places the agent outside Sales; the Sales lead no longer manages it.
    sqlx::query(
        "WITH agent AS (SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 AND kind='agent') \
         DELETE FROM department_members USING agent WHERE company_id=$1 AND department_id=$3 \
         AND principal_id=agent.id",
    )
    .bind(m.d.f.company)
    .bind(created.agent.id)
    .bind(m.d.unit.id.as_uuid())
    .execute(m.d.f.pool())
    .await
    .unwrap();
    let elsewhere = m.p().test_unit(m.d.f.company, "elsewhere").await;
    sqlx::query(
        "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
         SELECT $1, $3, 'lead', id FROM principals WHERE company_id=$1 AND agent_id=$2 AND kind='agent'",
    )
    .bind(m.d.f.company)
    .bind(created.agent.id)
    .bind(elsewhere)
    .execute(m.d.f.pool())
    .await
    .unwrap();
    let moved = SkillManagementPersistence::move_company(
        m.p(),
        m.actor().user_id(),
        m.d.f.company,
        skill.id,
        m.d.child.id.as_uuid(),
    )
    .await
    .unwrap();
    assert!(moved.uses.is_empty(), "{moved:?}");
    assert_eq!(moved.more_uses, 1);
}

/// Plan07 01B.1 with decision d: an agent is offered only skills and connections in its reach.
#[tokio::test]
async fn unit_resource_selection_offers_follow_the_agent_reach() {
    use crate::application::agent::{AgentAuthoringReads, AgentToolKind};
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let created = m.create("sales-agent", m.d.unit.id).await;
    let root = m.p().test_root_unit(m.d.f.company).await;
    let skill = |slug: &'static str| SkillWrite {
        slug: slug.into(),
        name: slug.into(),
        description: "Unit guidance".into(),
        instructions: "Follow the unit's guidance.".into(),
        files: None,
        required_tools: None,
        created_by: None,
    };
    let mut reachable = Vec::new();
    for (unit, slug, offered) in [
        (root, "root-guide", true),
        (m.d.unit.id.as_uuid(), "sales-guide", true),
        (m.d.sibling.id.as_uuid(), "support-guide", false),
    ] {
        let created =
            SkillManagementPersistence::create_company(m.p(), m.d.f.company, unit, skill(slug))
                .await
                .unwrap();
        if offered {
            reachable.push(created.id);
        }
    }
    reachable.sort();
    let skills = crate::use_cases::skill::SkillUseCases::new(
        Arc::new(m.p().clone()),
        Arc::new(m.p().clone()),
        Arc::new(m.p().clone()),
    );
    let mut offered: Vec<Uuid> = skills
        .list_authoring_page(
            m.d.f.owner,
            m.d.f.company,
            Some(created.agent.id),
            crate::use_cases::skill::SkillPageRequest {
                before: None,
                limit: 50,
            },
        )
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|skill| skill.id)
        .collect();
    offered.sort();
    assert_eq!(offered, reachable, "the owner authoring a Sales agent");
    // Connections: one in Sales is offered, one in Support is not.
    let mut ids = Vec::new();
    for (slug, unit) in [("sales-crm", m.d.unit.id), ("support-crm", m.d.sibling.id)] {
        let connection = crate::use_cases::mcp::McpPersistence::create_mcp_connection(
            m.p(),
            m.d.f.company,
            unit.as_uuid(),
            crate::use_cases::mcp::McpConnectionWrite {
                slug: slug.into(),
                endpoint: format!("https://{slug}.example.test/mcp")
                    .try_into()
                    .unwrap(),
                enabled: true,
                auth: crate::entities::mcp::McpAuth::None,
                discovered_tools: vec![],
                tool_grants: vec![],
            },
        )
        .await
        .unwrap();
        ids.push(connection.id);
    }
    let page = m
        .p()
        .tool_connections(
            m.scope(
                WorkflowActor::authenticated(m.d.f.owner).unwrap(),
                created.agent.id,
            ),
            AgentToolKind::Mcp,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        page.items.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![ids[0]]
    );
    // A named connection's tool page and a new draft choice follow the same reach (B2-F6).
    use crate::application::workflow::http::{HttpConnectionWrite, HttpConnections, HttpOrigin};
    let owner = WorkflowActor::authenticated(m.d.f.owner).unwrap();
    let company = crate::domain::workflow::CompanyId::new(m.d.f.company);
    let operation = crate::application::workflow::http::HttpOperation {
        tool: crate::application::workflow::publication::ToolSnapshot {
            workflow: None,
            company_id: company,
            contract: crate::application::workflow::registry::ToolContract {
                name: crate::domain::workflow::TypeName::parse("unit.lookup").unwrap(),
                connection: None,
                input_schema: serde_json::json!({"type":"object","properties":{"method":{"type":"string"},"path":{"type":"string"}},"required":["method","path"],"additionalProperties":false}),
                output_schema: serde_json::json!(true),
            },
            policy: crate::application::agent::ApprovedActionPolicy {
                capability: crate::domain::workflow::TypeName::parse("http.request").unwrap(),
                policy_revision: 1,
                effect: crate::application::agent::ActionEffect::Read,
                recovery: crate::application::agent::ActionRecovery::Reconcile,
            },
        },
        approval_required: false,
    };
    let mut http = Vec::new();
    for (origin, unit) in [
        ("https://sales.example.test", m.d.unit.id),
        ("https://support.example.test", m.d.sibling.id),
    ] {
        let connection = m
            .p()
            .create_http(
                company,
                owner,
                Some(unit),
                HttpConnectionWrite {
                    origin: HttpOrigin::parse(origin).unwrap(),
                    bearer: false,
                },
            )
            .await
            .unwrap();
        m.p()
            .register_http_operation(
                company,
                owner,
                connection.id,
                connection.revision,
                operation.clone(),
            )
            .await
            .unwrap();
        http.push(connection.id);
    }
    let options = |connection: Uuid| {
        m.p().tool_options(AgentToolPageRequest {
            scope: m.scope(owner, created.agent.id),
            kind: AgentToolKind::Http,
            connection: Some(connection),
            after: None,
        })
    };
    assert_eq!(options(http[0]).await.unwrap().items.len(), 1);
    assert!(matches!(options(http[1]).await, Err(AppError::NotFound(_))));
    let m = &m;
    let choose = |connection: Uuid| async move {
        let command = m.changes(owner, created.agent.id).await;
        m.p()
            .save_company_agent(SaveCompanyAgent {
                tools: Some(vec![AgentToolAssignment {
                    binding: None,
                    alias: AgentToolAlias::parse("unit.lookup").unwrap(),
                    choice: AgentToolChoice::Http {
                        connection,
                        operation: crate::domain::workflow::TypeName::parse("unit.lookup").unwrap(),
                    },
                }]),
                ..command
            })
            .await
    };
    assert!(choose(http[1]).await.is_err(), "a sibling's operation");
    choose(http[0]).await.unwrap();
}

fn model(provider: &str, model: &str, key: &str, is_default: bool) -> CompanyModelConnectionWrite {
    CompanyModelConnectionWrite {
        provider: provider.into(),
        api_key: Some(key.into()),
        models: vec![model.into()],
        is_default,
    }
}

fn unpinned(slug: &str) -> AgentWrite {
    AgentWrite {
        provider: None,
        model: None,
        ..write(slug)
    }
}

impl ManagedAgents {
    async fn connection(&self, provider: &str) -> crate::entities::company::CompanyModelConnection {
        CompanyPersistence::list_model_connections(self.p(), self.d.f.company)
            .await
            .unwrap()
            .into_iter()
            .find(|connection| connection.provider.as_str() == provider)
            .unwrap()
    }
    async fn create_as(
        &self,
        user: Uuid,
        write: AgentWrite,
        unit: Option<DepartmentId>,
    ) -> AppResult<CreatedAgent> {
        self.cases
            .create_agent(
                user,
                self.d.f.company,
                write,
                unit.map(|department| AgentPlacement {
                    department,
                    role: None,
                }),
            )
            .await
    }
}

/// Plan07 02A.2/02A.3: an agent naming no model pins the nearest default from its department up
/// (the root's when unplaced or when no department above has one); a later default change leaves
/// it pinned; an explicit model outside its department's reach is refused.
#[tokio::test]
async fn unit_model_default_cascade_pins_the_nearest_default_and_stays_frozen() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let lead = m.d.f.member;
    CompanyPersistence::replace_unit_model_connections_for_user(
        m.p(),
        lead,
        m.d.f.company,
        m.d.unit.id.as_uuid(),
        vec![model("anthropic", "claude-sales", "sales-key", true)],
    )
    .await
    .unwrap();
    let pinned = |created: &CreatedAgent| {
        (
            created.agent.provider.clone().unwrap(),
            created.agent.model.clone().unwrap(),
        )
    };
    let field = m
        .create_as(lead, unpinned("field-agent"), Some(m.d.child.id))
        .await
        .unwrap();
    assert_eq!(pinned(&field), ("anthropic".into(), "claude-sales".into()));
    // A library copy naming no model pins its department's default inside the creating
    // transaction too.
    let definition = m
        .cases
        .create_library_agent(
            &crate::entities::user::Caller::operator(m.d.f.owner),
            unpinned("library-helper"),
        )
        .await
        .unwrap();
    let copied = m
        .cases
        .create_agent_from_library(
            lead,
            m.d.f.company,
            definition.id,
            Some(AgentPlacement {
                department: m.d.child.id,
                role: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        (
            copied.agent.provider.as_deref(),
            copied.agent.model.as_deref()
        ),
        (Some("anthropic"), Some("claude-sales"))
    );
    let rooted = m
        .create_as(m.d.f.owner, unpinned("rooted-agent"), None)
        .await
        .unwrap();
    assert_eq!(pinned(&rooted), ("openai".into(), "gpt-4o-mini".into()));
    let support = m
        .create_as(m.d.f.owner, unpinned("support-agent"), Some(m.d.sibling.id))
        .await
        .unwrap();
    assert_eq!(pinned(&support), ("openai".into(), "gpt-4o-mini".into()));
    // Sales makes another provider its default: the Field agent keeps its pinned model, a new one
    // pins the new default.
    CompanyPersistence::replace_unit_model_connections_for_user(
        m.p(),
        lead,
        m.d.f.company,
        m.d.unit.id.as_uuid(),
        vec![
            CompanyModelConnectionWrite {
                api_key: None,
                ..model("anthropic", "claude-sales", "", false)
            },
            model("groq", "llama-sales", "groq-key", true),
        ],
    )
    .await
    .unwrap();
    let kept = m
        .p()
        .load_capabilities(m.d.f.company, field.agent.id)
        .await
        .unwrap()
        .unwrap()
        .agent;
    assert_eq!(
        (kept.provider.as_deref(), kept.model.as_deref()),
        (Some("anthropic"), Some("claude-sales"))
    );
    let later = m
        .create_as(lead, unpinned("field-later"), Some(m.d.child.id))
        .await
        .unwrap();
    assert_eq!(pinned(&later), ("groq".into(), "llama-sales".into()));
    // Support cannot reach Sales's providers.
    let refused = m
        .create_as(
            m.d.f.owner,
            AgentWrite {
                provider: Some("anthropic".into()),
                model: Some("claude-sales".into()),
                ..write("support-explicit")
            },
            Some(m.d.sibling.id),
        )
        .await;
    assert!(
        matches!(&refused, Err(AppError::BadRequest(message)) if message.contains("outside this agent's department")),
        "{refused:?}"
    );
}

/// Plan07 02A.1 with decisions a and g: a unit's managers replace only its own connections; a
/// provider another unit owns is a conflict; the root keeps exactly one default, so its default
/// cannot be moved away; a moved connection lands non-default; a department owning one cannot be
/// deleted until it moves; the unit key is a real foreign key.
#[tokio::test]
async fn unit_model_default_authority_moves_and_department_delete() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let (company, owner, lead) = (m.d.f.company, m.d.f.owner, m.d.f.member);
    let sales = m.d.unit.id.as_uuid();
    let m = &m;
    let replace = |user: Uuid, unit: Option<Uuid>, rows: Vec<CompanyModelConnectionWrite>| async move {
        match unit {
            Some(unit) => {
                CompanyPersistence::replace_unit_model_connections_for_user(
                    m.p(),
                    user,
                    company,
                    unit,
                    rows,
                )
                .await
            }
            None => {
                CompanyPersistence::replace_model_connections_for_user(m.p(), user, company, rows)
                    .await
            }
        }
    };
    let root_rows = || vec![model("openai", "gpt-4o-mini", "fixture-key", true)];
    // The Sales lead does not manage the root; the Support lead does not manage Sales.
    assert!(matches!(
        replace(lead, None, root_rows()).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        replace(
            m.d.sibling_lead,
            Some(sales),
            vec![model("groq", "g", "k", true)]
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    // The root's provider is not Sales's to rewrite.
    let conflict = replace(
        lead,
        Some(sales),
        vec![model("openai", "gpt-x", "other-key", false)],
    )
    .await;
    assert!(
        matches!(&conflict, Err(AppError::Conflict(message)) if message.contains("openai")),
        "{conflict:?}"
    );
    // At most one default per department.
    let cases = crate::use_cases::company::CompanyUseCases::new(Arc::new(m.p().clone()));
    assert!(matches!(
        cases
            .replace_unit_model_connections(
                lead,
                company,
                sales,
                vec![
                    model("groq", "g", "k", true),
                    model("anthropic", "a", "k", true)
                ],
            )
            .await,
        Err(AppError::BadRequest(_))
    ));
    replace(
        lead,
        Some(sales),
        vec![model("anthropic", "claude-sales", "sales-key", true)],
    )
    .await
    .unwrap();
    // The root's default cannot leave it while the root keeps another connection.
    replace(
        owner,
        None,
        vec![
            CompanyModelConnectionWrite {
                api_key: None,
                ..model("openai", "gpt-4o-mini", "", true)
            },
            model("google", "gemini-root", "google-key", false),
        ],
    )
    .await
    .unwrap();
    let openai = m.connection("openai").await;
    assert!(matches!(
        m.p()
            .move_model_connection(owner, company, openai.id, sales)
            .await,
        Err(AppError::BadRequest(_))
    ));
    // Moved into the root, which has a default, Sales's default lands non-default.
    let anthropic = m.connection("anthropic").await;
    let root = m.p().test_root_unit(company).await;
    let moved = m
        .p()
        .move_model_connection(owner, company, anthropic.id, root)
        .await
        .unwrap();
    assert_eq!(moved.to.as_uuid(), root);
    let anthropic = m.connection("anthropic").await;
    assert_eq!(anthropic.unit_id, root);
    assert!(!anthropic.is_default);
    assert!(m.connection("openai").await.is_default);
    // A department owning a connection is not deleted until it moves.
    replace(
        lead,
        Some(m.d.child.id.as_uuid()),
        vec![model("groq", "g", "field-key", false)],
    )
    .await
    .unwrap();
    // Its own desk goes first (Plan06 decision 9); then only the unit's resources hold it.
    m.d.f.drop_desk(m.d.child.id).await;
    let refused =
        m.d.f
            .use_cases
            .delete_department(owner, company, m.d.child.id)
            .await;
    assert!(
        matches!(&refused, Err(AppError::Conflict(message)) if message.contains("model connection groq")),
        "{refused:?}"
    );
    let foreign = sqlx::query(
        "UPDATE company_model_connections SET unit_id = gen_random_uuid() \
         WHERE company_id = $1 AND provider = 'groq'",
    )
    .bind(company)
    .execute(m.p().pool())
    .await
    .unwrap_err();
    assert_eq!(
        foreign.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
}

/// Plan07 02A with decision d: an agent's model connection must stay in its reach; moved to a
/// sibling, the agent's next publication is refused naming it, and the move lists the agent.
#[tokio::test]
async fn unit_model_default_reach_refuses_publication_after_a_move() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let (company, owner, lead) = (m.d.f.company, m.d.f.owner, m.d.f.member);
    CompanyPersistence::replace_unit_model_connections_for_user(
        m.p(),
        lead,
        company,
        m.d.unit.id.as_uuid(),
        vec![model("anthropic", "claude-sales", "sales-key", true)],
    )
    .await
    .unwrap();
    let created = m
        .create_as(lead, unpinned("sales-agent"), Some(m.d.unit.id))
        .await
        .unwrap();
    let anthropic = m.connection("anthropic").await;
    let moved = m
        .p()
        .move_model_connection(owner, company, anthropic.id, m.d.sibling.id.as_uuid())
        .await
        .unwrap();
    assert!(
        moved.uses.iter().any(|used| used.id == created.agent.id),
        "{moved:?}"
    );
    let active = m
        .p()
        .active_agent_revision(m.scope(m.actor(), created.agent.id))
        .await
        .unwrap()
        .unwrap();
    let refused = m
        .p()
        .publish_agent(PublishAgent {
            scope: m.scope(m.actor(), created.agent.id),
            expected_draft_revision: created.agent.draft_revision,
            expected_active_revision: Some(active.id),
            key: AgentPublicationKey::parse("model-moved").unwrap(),
        })
        .await;
    assert!(
        matches!(&refused, Err(AppError::ResourceOutsideUnit(refusal))
            if refusal.code() == "publication.resource_outside_unit"
                && refusal.resource == anthropic.id),
        "{:?}",
        refused.err()
    );
}

/// Plan07 G1 r2 D1: two leads adding the same provider to their sibling units at once — one is
/// saved, the other is a conflict, and the saved key is never overwritten.
#[tokio::test]
async fn unit_model_default_competing_units_add_one_provider() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let company = m.d.f.company;
    let m = &m;
    let add = |user: Uuid, unit: Uuid, key: &'static str| async move {
        CompanyPersistence::replace_unit_model_connections_for_user(
            m.p(),
            user,
            company,
            unit,
            vec![model("groq", "llama", key, true)],
        )
        .await
    };
    let (sales, support) = tokio::join!(
        add(m.d.f.member, m.d.unit.id.as_uuid(), "sales-key"),
        add(m.d.sibling_lead, m.d.sibling.id.as_uuid(), "support-key")
    );
    let won = match (&sales, &support) {
        (Ok(()), Err(AppError::Conflict(_))) => m.d.unit.id.as_uuid(),
        (Err(AppError::Conflict(_)), Ok(())) => m.d.sibling.id.as_uuid(),
        other => panic!("{other:?}"),
    };
    let groq = m.connection("groq").await;
    assert_eq!(groq.unit_id, won);
    let key = CompanyPersistence::model_api_key(m.p(), company, &"groq".into())
        .await
        .unwrap();
    let expected = if won == m.d.unit.id.as_uuid() {
        "sales-key"
    } else {
        "support-key"
    };
    assert_eq!(key.as_deref(), Some(expected));
}

/// Plan07 02A.1/02A.2 with G1 r2 D1 (B1-F1): a move needs both units managed; the root's only
/// connection may leave it, after which an unplaced creation finds no default anywhere and is
/// refused; a connection moved into a root without a default becomes the company default.
#[tokio::test]
async fn unit_model_default_moves_and_an_empty_root() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let (company, owner, lead) = (m.d.f.company, m.d.f.owner, m.d.f.member);
    let sales = m.d.unit.id.as_uuid();
    let support = m.d.sibling.id.as_uuid();
    let root = m.p().test_root_unit(company).await;
    CompanyPersistence::replace_unit_model_connections_for_user(
        m.p(),
        lead,
        company,
        sales,
        vec![model("anthropic", "claude-sales", "sales-key", true)],
    )
    .await
    .unwrap();
    let (openai, anthropic) = (
        m.connection("openai").await,
        m.connection("anthropic").await,
    );
    // The Sales lead manages neither the root nor Support.
    for (id, target) in [(openai.id, sales), (anthropic.id, support)] {
        assert!(matches!(
            m.p().move_model_connection(lead, company, id, target).await,
            Err(AppError::NotFound(_))
        ));
    }
    // The root's only connection may leave it; then no department up from the root has a
    // default.
    m.p()
        .move_model_connection(owner, company, openai.id, support)
        .await
        .unwrap();
    let refused = m.create_as(owner, unpinned("nowhere"), None).await;
    assert!(
        matches!(&refused, Err(AppError::BadRequest(message)) if message.contains("Add a model connection")),
        "{refused:?}"
    );
    // Moved into the default-less root, Sales's default becomes the company default.
    m.p()
        .move_model_connection(owner, company, anthropic.id, root)
        .await
        .unwrap();
    let anthropic = m.connection("anthropic").await;
    assert!(anthropic.is_default && anthropic.in_root, "{anthropic:?}");
    let rooted = m.create_as(owner, unpinned("rooted"), None).await.unwrap();
    assert_eq!(rooted.agent.provider.as_deref(), Some("anthropic"));
}

/// Plan07 decision a (B1-F1): a root admin and a root lead replace the root's connections, a
/// plain member gets not-found; a department's own default beats the one above it; an existing
/// agent is offered only connections in its reach; a library copy whose model is outside its
/// department falls back to the department's default.
#[tokio::test]
async fn unit_model_default_root_managers_own_defaults_offers_and_library_fallback() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let (company, owner, lead) = (m.d.f.company, m.d.f.owner, m.d.f.member);
    let root = m.p().test_root_unit(company).await;
    let root_lead = m.p().test_unit_lead(company, root).await;
    let (plain, _) = super::super::teammate(m.p().pool(), company, "member").await;
    let rows = |key: &'static str| vec![model("openai", "gpt-4o-mini", key, true)];
    for manager in [m.d.f.admin, root_lead] {
        CompanyPersistence::replace_model_connections_for_user(
            m.p(),
            manager,
            company,
            rows("rotated-key"),
        )
        .await
        .unwrap();
    }
    assert!(matches!(
        CompanyPersistence::replace_model_connections_for_user(m.p(), plain, company, rows("x"))
            .await,
        Err(AppError::NotFound(_))
    ));
    CompanyPersistence::replace_unit_model_connections_for_user(
        m.p(),
        lead,
        company,
        m.d.unit.id.as_uuid(),
        vec![model("anthropic", "claude-sales", "sales-key", true)],
    )
    .await
    .unwrap();
    CompanyPersistence::replace_unit_model_connections_for_user(
        m.p(),
        lead,
        company,
        m.d.child.id.as_uuid(),
        vec![model("groq", "llama-field", "field-key", true)],
    )
    .await
    .unwrap();
    let field = m
        .create_as(lead, unpinned("field-own"), Some(m.d.child.id))
        .await
        .unwrap();
    assert_eq!(field.agent.provider.as_deref(), Some("groq"));
    let support = m
        .create_as(owner, unpinned("support-agent"), Some(m.d.sibling.id))
        .await
        .unwrap();
    let cases = &m.cases;
    let offered = |agent: Uuid| async move {
        cases
            .list_company_model_connections(owner, company, Some(agent))
            .await
            .unwrap()
            .into_iter()
            .map(|connection| connection.provider.as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        offered(support.agent.id).await,
        ["openai".to_owned()].into()
    );
    assert_eq!(
        offered(field.agent.id).await,
        [
            "anthropic".to_owned(),
            "groq".to_owned(),
            "openai".to_owned()
        ]
        .into()
    );
    // A library agent published on Sales's model, copied into Support, pins Support's default.
    let definition = m
        .cases
        .create_library_agent(
            &crate::entities::user::Caller::operator(owner),
            AgentWrite {
                provider: Some("anthropic".into()),
                model: Some("claude-sales".into()),
                ..write("library-sales")
            },
        )
        .await
        .unwrap();
    let copied = m
        .cases
        .create_agent_from_library(
            owner,
            company,
            definition.id,
            Some(AgentPlacement {
                department: m.d.sibling.id,
                role: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(copied.agent.provider.as_deref(), Some("openai"));
}
