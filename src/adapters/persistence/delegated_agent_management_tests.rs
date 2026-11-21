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
