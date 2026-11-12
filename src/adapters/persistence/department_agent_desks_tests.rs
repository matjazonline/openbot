//! Company-agent creation against PostgreSQL (01-departments p03, 01DEP-P03-D1..D3):
//! `agent_default_desk_` for what one creation commits or refuses, its placement and its guards,
//! and `agent_default_desk_competing_` for creators racing one slug.
use super::*;
use crate::adapters::persistence::company_agent::{CompanyAgentService, default_desk_key};
use crate::adapters::persistence::test_support::provisioning_templates;
use crate::application::ports::channel::{ChannelOwnershipWrite, ChannelPersistence, ChannelWrite};
use crate::entities::participant::PrincipalCapability;
use crate::use_cases::agent::{
    AgentPersistence, AgentPlacement, AgentWrite, CompanyAgents, CreatedAgent,
};
use crate::use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence};

fn service(f: &Departments) -> CompanyAgentService {
    CompanyAgentService::new(f.persistence.clone(), provisioning_templates())
}

fn owner(f: &Departments) -> WorkflowActor {
    WorkflowActor::authenticated(f.owner).unwrap()
}

/// The company's model connection: OpenAI, with its key (the schema holds no connection without
/// one).
async fn connect(f: &Departments) {
    CompanyPersistence::replace_model_connections_for_user(
        &f.persistence,
        f.owner,
        f.company,
        vec![CompanyModelConnectionWrite {
            provider: "openai".into(),
            api_key: Some("test-key".into()),
            models: vec!["gpt-4o-mini".into(), "gpt-4o".into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
}

fn agent(slug: &str) -> AgentWrite {
    AgentWrite {
        name: format!("Agent {slug}"),
        slug: slug.into(),
        system_prompt: Some("Help the team.".into()),
        ..AgentWrite::default()
    }
}

async fn create(
    f: &Departments,
    write: AgentWrite,
    placement: Option<AgentPlacement>,
) -> AppResult<CreatedAgent> {
    service(f)
        .create(owner(f), CompanyId::new(f.company), write, placement)
        .await
}

/// Everything a creation writes in the company, counted.
async fn written(f: &Departments) -> serde_json::Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
           'agents', (SELECT count(*) FROM agents WHERE company_id = $1), \
           'principals', (SELECT count(*) FROM principals WHERE company_id = $1 AND kind = 'agent'), \
           'revisions', (SELECT count(*) FROM agent_revisions WHERE company_id = $1), \
           'desks', (SELECT count(*) FROM channels WHERE company_id = $1), \
           'workflows', (SELECT count(*) FROM workflow_definitions \
             WHERE company_id = $1 AND template_origin IS NOT NULL), \
           'bindings', (SELECT count(*) FROM workflow_bindings \
             WHERE company_id = $1 AND channel_id IS NOT NULL AND active), \
           'commands', (SELECT count(*) FROM workflow_provisioning_commands WHERE company_id = $1), \
           'members', (SELECT count(*) FROM department_members WHERE company_id = $1), \
           'roles', (SELECT count(*) FROM department_roles WHERE company_id = $1), \
           'skills', (SELECT count(*) FROM skills WHERE company_id = $1))",
    )
    .bind(f.company)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

fn grew(before: &serde_json::Value, after: &serde_json::Value) -> serde_json::Value {
    serde_json::Value::Object(
        before
            .as_object()
            .unwrap()
            .iter()
            .map(|(kind, count)| {
                (
                    kind.clone(),
                    json!(after[kind].as_i64().unwrap() - count.as_i64().unwrap()),
                )
            })
            .filter(|(_, grown)| grown != &json!(0))
            .collect(),
    )
}

const ONE_AGENT: &str = r#"{"agents": 1, "principals": 1, "revisions": 1, "desks": 1,
    "workflows": 1, "bindings": 1, "commands": 1}"#;

fn one_agent() -> serde_json::Value {
    serde_json::from_str(ONE_AGENT).unwrap()
}

/// The desk at `desk`: its primary address, name, access mode, owning department and mode.
async fn desk_row(f: &Departments, desk: Uuid) -> (String, String, String, Option<Uuid>, bool) {
    sqlx::query_as(
        "SELECT slug.slug::text, channel.name, channel.access_mode, channel.department_id, \
                channel.members_only \
           FROM channels AS channel \
           JOIN channel_slugs AS slug ON slug.channel_id = channel.id AND slug.is_primary \
          WHERE channel.company_id = $1 AND channel.id = $2",
    )
    .bind(f.company)
    .bind(desk)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

async fn principal_of(f: &Departments, agent: Uuid) -> PrincipalId {
    PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
            .bind(f.company)
            .bind(agent)
            .fetch_one(f.pool())
            .await
            .unwrap(),
    )
}

/// 03A.1/2: one creation commits the agent, its principal, one published version, its default
/// desk at its slug -- a plain team desk -- the desk's autonomous workflow and active binding, and
/// the `agent:{id}` command that records which desk it was. An omitted model is pinned to the
/// keyed default connection's first model (D3).
#[tokio::test]
async fn agent_default_desk_creation_commits_the_agent_with_its_desk() {
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let before = written(&f).await;
    let created = create(&f, agent("helper"), None).await.unwrap();
    assert_eq!(grew(&before, &written(&f).await), one_agent());
    assert_eq!(
        (
            created.agent.provider.as_deref(),
            created.agent.model.as_deref()
        ),
        (Some("openai"), Some("gpt-4o-mini"))
    );
    assert_eq!(
        desk_row(&f, created.desk_id).await,
        (
            "helper".into(),
            "Agent helper".into(),
            "team".into(),
            None,
            false
        )
    );
    let (command, actor): (serde_json::Value, Uuid) = sqlx::query_as(
        "SELECT result, actor_id FROM workflow_provisioning_commands \
         WHERE company_id = $1 AND command_key = $2",
    )
    .bind(f.company)
    .bind(default_desk_key(created.agent.id))
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(actor, f.owner);
    assert_eq!(command["channel"]["id"], json!(created.desk_id.to_string()));
    assert_eq!(command["agent"]["id"], json!(created.agent.id.to_string()));
    assert_eq!(command["reply_mode"], json!("autonomous"));
    let binding_channel: Uuid = sqlx::query_scalar(
        "SELECT channel_id FROM workflow_bindings WHERE company_id = $1 AND id = $2 AND active",
    )
    .bind(f.company)
    .bind(Uuid::parse_str(command["binding_id"].as_str().unwrap()).unwrap())
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(binding_channel, created.desk_id);

    // An explicit model the keyed connection enables is kept.
    let explicit = create(
        &f,
        AgentWrite {
            provider: Some("openai".into()),
            model: Some("gpt-4o".into()),
            ..agent("explicit")
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(explicit.agent.model.as_deref(), Some("gpt-4o"));
}

/// 03A.2, D3: with no model connection (none exists without a key) every creation is refused
/// with the worded error and writes nothing; so is an explicit model the connections lack.
#[tokio::test]
async fn agent_default_desk_refuses_without_a_keyed_model() {
    let Some(f) = Departments::new().await else {
        return;
    };
    sqlx::query("DELETE FROM company_model_connections WHERE company_id = $1")
        .bind(f.company)
        .execute(f.pool())
        .await
        .unwrap();
    let before = written(&f).await;
    let message = "Add a model connection with an API key before creating an agent: its desk answers on an explicit model.";
    assert!(matches!(
        create(&f, agent("helper"), None).await,
        Err(AppError::BadRequest(refused)) if refused == message
    ));
    assert!(matches!(
        create(
            &f,
            AgentWrite {
                provider: Some("openai".into()),
                model: Some("gpt-4o".into()),
                ..agent("helper")
            },
            None
        )
        .await,
        Err(AppError::BadRequest(refused)) if refused == message
    ));
    connect(&f).await;
    assert!(matches!(
        create(
            &f,
            AgentWrite {
                provider: Some("openai".into()),
                model: Some("gpt-9".into()),
                ..agent("helper")
            },
            None
        )
        .await,
        Err(AppError::BadRequest(refused)) if refused.contains("'gpt-9'")
    ));
    assert_eq!(grew(&before, &written(&f).await), json!({}));
}

/// 03A.2, F5: a taken address (a desk already has the agent's slug) and a taken agent slug are
/// conflicts, and nothing of the refused creation survives.
#[tokio::test]
async fn agent_default_desk_refuses_a_taken_address_or_slug() {
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    ChannelPersistence::create(
        &f.persistence,
        f.company,
        ChannelWrite {
            name: "Taken".into(),
            slug: "taken".into(),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    create(&f, agent("helper"), None).await.unwrap();
    let before = written(&f).await;
    let taken_address = create(&f, agent("taken"), None).await;
    assert!(
        matches!(&taken_address, Err(AppError::Conflict(message)) if message.contains("'taken'")),
        "{taken_address:?}"
    );
    let taken_slug = create(&f, agent("helper"), None).await;
    assert!(
        matches!(&taken_slug, Err(AppError::Conflict(_))),
        "{taken_slug:?}"
    );
    assert_eq!(grew(&before, &written(&f).await), json!({}));
}

/// 03A.2: a publication prerequisite the company has not met -- a library skill that needs a
/// company tool no binding supplies -- refuses the copy, skills and principal included.
#[tokio::test]
async fn agent_default_desk_refuses_a_library_copy_it_cannot_publish() {
    use crate::domain::workflow::TypeName;
    use crate::use_cases::skill::{SkillManagementPersistence, SkillWrite};
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let skill = SkillManagementPersistence::create_library(
        &f.persistence,
        SkillWrite {
            slug: format!("needs-lookup-{}", Uuid::new_v4().simple()),
            name: "Needs lookup".into(),
            description: "Requires a company tool".into(),
            instructions: "Use support.lookup.".into(),
            required_tools: Some(vec![TypeName::parse("support.lookup").unwrap()]),
            files: None,
            created_by: None,
        },
    )
    .await
    .unwrap();
    let library = AgentPersistence::create_library(
        &f.persistence,
        AgentWrite {
            skill_ids: vec![skill.id],
            ..agent(&format!("portable-{}", Uuid::new_v4().simple()))
        },
    )
    .await
    .unwrap();
    let before = written(&f).await;
    let refused = service(&f)
        .copy_from_library(
            owner(&f),
            CompanyId::new(f.company),
            library.id,
            agent("portable"),
            None,
        )
        .await;
    assert!(
        matches!(&refused, Err(AppError::BadRequest(_))),
        "{refused:?}"
    );
    assert_eq!(grew(&before, &written(&f).await), json!({}));
}

/// 03A.2: a library definition has no company and no desk; its copy gets one like any agent.
#[tokio::test]
async fn agent_default_desk_library_definitions_have_none_and_copies_do() {
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let slug = format!("library-{}", Uuid::new_v4().simple());
    let before = written(&f).await;
    let library = AgentPersistence::create_library(&f.persistence, agent(&slug))
        .await
        .unwrap();
    let library_desks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_provisioning_commands WHERE command_key = $1",
    )
    .bind(default_desk_key(library.id))
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(library_desks, 0);
    assert_eq!(grew(&before, &written(&f).await), json!({}));
    let copy = service(&f)
        .copy_from_library(
            owner(&f),
            CompanyId::new(f.company),
            library.id,
            agent("copied"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(grew(&before, &written(&f).await), one_agent());
    assert_eq!(desk_row(&f, copy.desk_id).await.0, "copied");
}

/// 03C.0 with C002: an agent created as a department's lead is a member and its lead, and the
/// derived grants reach its own department's members-only desk, its parent's and its child's.
#[tokio::test]
async fn agent_default_desk_places_the_agent_and_its_lead_role_with_grants() {
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let parent = f.create("Operations", None).await;
    let department = f.create("Support", Some(parent.id)).await;
    let child = f.create("Escalations", Some(department.id)).await;
    let mut desks = Vec::new();
    for (slug, owner) in [
        ("ops-room", parent.id),
        ("support-room", department.id),
        ("escalation-room", child.id),
    ] {
        desks.push(
            ChannelPersistence::create(
                &f.persistence,
                f.company,
                ChannelWrite {
                    name: slug.into(),
                    slug: slug.into(),
                    enabled: true,
                    ownership: Some(ChannelOwnershipWrite {
                        department_id: Some(Some(owner)),
                        members_only: Some(true),
                    }),
                    ..ChannelWrite::default()
                },
            )
            .await
            .unwrap()
            .id,
        );
    }
    let created = create(
        &f,
        agent("lead"),
        Some(AgentPlacement {
            department: department.id,
            role: Some(DepartmentRole::Lead),
        }),
    )
    .await
    .unwrap();
    let principal = principal_of(&f, created.agent.id).await;
    assert_eq!(f.members(department.id).await, vec![principal]);
    assert_eq!(
        f.roles(department.id)
            .await
            .into_iter()
            .map(|holder| (holder.role, holder.principal_id))
            .collect::<Vec<_>>(),
        vec![(DepartmentRole::Lead, principal)]
    );
    for desk in desks {
        let mut grants: Vec<String> = sqlx::query_scalar(
            "SELECT capability FROM channel_principal_grants \
             WHERE company_id = $1 AND channel_id = $2 AND principal_id = $3 \
               AND provenance = 'department_membership' ORDER BY capability",
        )
        .bind(f.company)
        .bind(desk)
        .bind(principal.as_uuid())
        .fetch_all(f.pool())
        .await
        .unwrap();
        grants.sort();
        assert_eq!(
            grants,
            vec![
                PrincipalCapability::Participate.as_str().to_owned(),
                PrincipalCapability::View.as_str().to_owned()
            ],
            "desk {desk}"
        );
    }
    // The agent's own desk stays a plain team desk; membership does not choose its owner.
    assert_eq!(desk_row(&f, created.desk_id).await.3, None);

    // A department of another company, or none, refuses the whole creation.
    let before = written(&f).await;
    let foreign = DepartmentId::random();
    assert!(matches!(
        create(
            &f,
            agent("stray"),
            Some(AgentPlacement {
                department: foreign,
                role: None
            })
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(grew(&before, &written(&f).await), json!({}));
}

/// 03A.3: an agent with a default desk is guarded as before -- its published version refuses
/// deletion -- and the desk and its binding stay.
#[tokio::test]
async fn agent_default_desk_deletion_guards_are_unchanged() {
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let created = create(&f, agent("helper"), None).await.unwrap();
    let before = written(&f).await;
    assert!(matches!(
        AgentPersistence::delete(&f.persistence, created.agent.id).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(grew(&before, &written(&f).await), json!({}));
    assert_eq!(desk_row(&f, created.desk_id).await.0, "helper");
}

/// Two creators of one slug, the second waiting on the company write lock the first holds: one
/// agent and one desk, and the loser is a conflict that wrote nothing.
#[tokio::test]
async fn agent_default_desk_competing_creators_of_one_slug_make_one_agent() {
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let before = written(&f).await;
    let mut hold = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *hold)
        .await
        .unwrap();
    let first = tokio::spawn({
        let service = service(&f);
        let (actor, company) = (owner(&f), CompanyId::new(f.company));
        async move { service.create(actor, company, agent("helper"), None).await }
    });
    let second = tokio::spawn({
        let service = service(&f);
        let (actor, company) = (owner(&f), CompanyId::new(f.company));
        async move { service.create(actor, company, agent("helper"), None).await }
    });
    wait_until_backends_are_blocked(f.pool(), 2).await;
    hold.commit().await.unwrap();
    let outcomes = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, Err(AppError::Conflict(_)))),
        "{outcomes:?}"
    );
    assert_eq!(grew(&before, &written(&f).await), one_agent());
}

/// 01DEP-P03-B1-F1: an agent created as a department's lead takes the gates of every desk its
/// grants reach in one sorted call. With an admission holding the parent's desk gate -- the
/// lower key, as inbound routing into both desks would take it first -- the creator waits there
/// holding no other gate, so a second admission into the department's own desk goes through
/// rather than closing a cycle; when the first admission commits, the creation completes.
#[tokio::test]
async fn agent_default_desk_competing_lead_creation_takes_its_desk_gates_once() {
    use crate::adapters::persistence::channel_gate::{
        ChannelGateAccess, ChannelGateKey, acquire_channel_gate_on,
    };
    use crate::adapters::persistence::department_desks::set_desk_ownership_on;
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let parent = f.create("Operations", None).await;
    let department = f.create("Support", Some(parent.id)).await;
    let members_only = |department: DepartmentId| ChannelOwnershipWrite {
        department_id: Some(Some(department)),
        members_only: Some(true),
    };
    let mut desks = Vec::new();
    for slug in ["room-a", "room-b"] {
        desks.push(
            ChannelPersistence::create(
                &f.persistence,
                f.company,
                ChannelWrite {
                    name: slug.into(),
                    slug: slug.into(),
                    enabled: true,
                    ownership: Some(members_only(parent.id)),
                    ..ChannelWrite::default()
                },
            )
            .await
            .unwrap()
            .id,
        );
    }
    desks.sort_unstable();
    let (parent_desk, department_desk) = (desks[0], desks[1]);
    let mut moved = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *moved)
        .await
        .unwrap();
    set_desk_ownership_on(
        &mut moved,
        CompanyId::new(f.company),
        department_desk,
        &members_only(department.id),
    )
    .await
    .unwrap();
    moved.commit().await.unwrap();

    let mut admission = f.pool().begin().await.unwrap();
    acquire_channel_gate_on(
        &mut admission,
        ChannelGateKey::new(f.company, parent_desk),
        ChannelGateAccess::Admit,
    )
    .await
    .unwrap();
    let creation = tokio::spawn({
        let service = service(&f);
        let (actor, company) = (owner(&f), CompanyId::new(f.company));
        let placement = Some(AgentPlacement {
            department: department.id,
            role: Some(DepartmentRole::Lead),
        });
        async move {
            service
                .create(actor, company, agent("lead"), placement)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 1).await;
    let mut second = f.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '500ms'")
        .execute(&mut *second)
        .await
        .unwrap();
    acquire_channel_gate_on(
        &mut second,
        ChannelGateKey::new(f.company, department_desk),
        ChannelGateAccess::Admit,
    )
    .await
    .expect("the waiting creator holds no gate of the department's desk");
    second.rollback().await.unwrap();
    admission.commit().await.unwrap();
    creation
        .await
        .unwrap()
        .expect("the lead is created once the admission commits");
}
