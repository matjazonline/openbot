use super::*;
use crate::application::ports::channel::ChannelOwnershipWrite;
use crate::application::ports::department::DepartmentWrite;
use crate::application::workflow::DeliveryPolicy;
use crate::domain::workflow::{TriggerRef, TriggerSource};
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::transport::PrincipalId;

#[path = "role_model_runtime_tests.rs"]
mod model_only;

#[path = "role_child_runtime_tests.rs"]
mod children;

#[path = "role_refusal_runtime_tests.rs"]
mod refusals;

#[path = "role_runtime_race_support.rs"]
mod races;

struct RoleAgent {
    id: Uuid,
    principal: PrincipalId,
    revision: Uuid,
}

struct RoleDesk {
    id: Uuid,
    department: DepartmentId,
    slug: String,
}

async fn agent(d: &Deployment, slug: &str) -> RoleAgent {
    let agent = crate::adapters::persistence::test_support::company_agents(d.p())
        .create(
            d.fixture.target.actor,
            d.fixture.target.company,
            AgentWrite {
                name: slug.into(),
                slug: slug.into(),
                system_prompt: Some(format!("ROLE_INSTRUCTIONS_{slug}")),
                tool_bindings: Some(vec![
                    crate::entities::agent_capability::AgentToolBinding::builtin(
                        crate::entities::agent_capability::AgentBuiltinTool::ListCompanyAgents,
                    ),
                ]),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .agent;
    register_scripted_agent_base_url(agent.id, &d.llm.base_url);
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(agent.id)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    let revision: Uuid = sqlx::query_scalar(
        "SELECT revision_id FROM agent_active_revisions WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(agent.id)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    RoleAgent {
        id: agent.id,
        principal: PrincipalId::new(principal),
        revision,
    }
}

async fn desk(d: &Deployment, slug: &str, agent: &RoleAgent) -> RoleDesk {
    let departments = crate::adapters::persistence::test_support::department_use_cases(d.p());
    let department = departments
        .create_department(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            DepartmentWrite {
                name: slug.into(),
                slug: slug.into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    departments
        .assign_role(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            department.id,
            DepartmentRole::Lead,
            agent.principal,
        )
        .await
        .unwrap();
    let channel = ChannelPersistence::create(
        d.p(),
        d.fixture.target.company.as_uuid(),
        ChannelWrite {
            name: slug.into(),
            slug: format!("{slug}-intake"),
            enabled: true,
            participant_emails: Some(vec![CUSTOMER.into()]),
            ownership: Some(ChannelOwnershipWrite {
                department_id: Some(Some(department.id)),
                members_only: Some(false),
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    RoleDesk {
        id: channel.id,
        department: department.id,
        slug: channel.slug.to_string(),
    }
}

fn source(kind: &str) -> Value {
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object"});
    let inputs = source["steps"]["start"]["with"].as_object_mut().unwrap();
    inputs.remove("agent");
    inputs.insert("role".into(), json!({"literal":"lead"}));
    source
}

fn request(
    binding: BindingTarget,
    channel: Uuid,
    key: &str,
    trigger: Uuid,
) -> AdmitWorkflowRequest {
    AdmitWorkflowRequest {
        history_session: None,
        company_id: binding.company,
        actor: binding.actor,
        association: RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        trigger: TriggerRef::new(
            binding.company,
            TriggerId::new(trigger),
            TriggerSource::Manual,
        )
        .unwrap(),
        correlation_id: crate::entities::correlation::CorrelationId::new(),
        binding_id: binding.binding,
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        input: json!({}),
        rerun_of: None,
        delivery_policy: DeliveryPolicy::InAppOnly,
        expected_binding_revision: None,
        admission_claim: None,
    }
}

async fn admit(
    d: &Deployment,
    binding: BindingTarget,
    desk: &RoleDesk,
    key: &str,
    trigger: Uuid,
) -> RunId {
    match d
        .engine
        .services()
        .service()
        .admit(request(binding, desk.id, key, trigger))
        .await
        .unwrap()
    {
        AdmissionResult::Created(run) => run,
        other => panic!("new role run expected: {other:?}"),
    }
}

async fn settled(d: &Deployment, run: RunId) -> bool {
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    assert_ne!(state, "failed", "{}", d.diagnostics().await);
    state == "succeeded"
}

async fn bind_same_version(d: &Deployment, first: BindingTarget, desk: &RoleDesk) -> BindingTarget {
    let previous = d
        .p()
        .binding(first.company, first.binding)
        .await
        .unwrap()
        .unwrap();
    let target = BindingTarget {
        binding: WorkflowBindingId::new(Uuid::new_v4()),
        ..first
    };
    let configured = d
        .engine
        .services()
        .bindings()
        .configure(ConfigureBinding {
            escalation: None,
            target,
            expected: None,
            association: RelatedAssociation::Channel(RelatedChannelId::new(desk.id)),
            version: previous
                .configuration
                .bundle()
                .compiled()
                .graph()
                .definition()
                .version_id,
            params: json!({}),
            resources: BTreeMap::new(),
            run_as: None,
            message_trigger: Some(MessageTrigger::Any),
        })
        .await
        .unwrap();
    d.engine
        .services()
        .bindings()
        .activate(SetBindingActivity {
            target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    target
}

async fn assert_selection(d: &Deployment, run: RunId, agent: &RoleAgent) {
    #[derive(sqlx::FromRow)]
    struct Selected {
        agent_id: Uuid,
        agent_revision_id: Uuid,
    }
    let selected: Selected = sqlx::query_as("SELECT agent_id,agent_revision_id FROM workflow_run_role_bindings WHERE company_id=$1 AND run_id=$2")
        .bind(d.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    assert_eq!(selected.agent_id, agent.id);
    assert_eq!(selected.agent_revision_id, agent.revision);
    let actual: Uuid = sqlx::query_scalar(
        "SELECT agent_id FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(actual, agent.id);
}

fn system_text(request: &Value) -> String {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "system")
        .flat_map(|message| message["content"].as_array().cloned().unwrap_or_default())
        .map(|part| part["text"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[tokio::test]
async fn workflow_role_admission_two_desks_execute_frozen_holders_and_revisions_then_new_messages_resolve_again()
 {
    let mut d = Deployment::new(vec![
        LlmTurn::text(r#""first""#),
        LlmTurn::text(r#""second""#),
        LlmTurn::text(r#""new holder""#),
    ])
    .await;
    let a = agent(&d, "first").await;
    let b = agent(&d, "second").await;
    let first_desk = desk(&d, "first-team", &a).await;
    let second_desk = desk(&d, "second-team", &b).await;
    d.channel = first_desk.id;
    let first = d.bind(source("agent.run"), Some(MessageTrigger::Any)).await;
    let second = bind_same_version(&d, first, &second_desk).await;
    let trigger = Uuid::new_v4();
    let queued = races::admit_while_reassigning(&d, first, &first_desk, b.principal, trigger).await;
    sqlx::query("UPDATE agents SET system_prompt='ROLE_INSTRUCTIONS_first_UPDATED' WHERE company_id=$1 AND id=$2")
        .bind(first.company.as_uuid()).bind(a.id).execute(d.p().pool()).await.unwrap();
    let replacement_revision = d.fixture.publish_agent(a.id).await.id;
    assert_ne!(replacement_revision.as_uuid(), a.revision);
    execute_original(&mut d, queued, &a).await;
    let replay = d
        .engine
        .services()
        .service()
        .admit(request(first, first_desk.id, "queued-old", trigger))
        .await
        .unwrap();
    assert_eq!(replay, AdmissionResult::Replayed(queued));
    let other = admit(&d, second, &second_desk, "other-desk", Uuid::new_v4()).await;
    Box::pin(d.run_until("the other desk's holder completed", async || {
        settled(&d, other).await
    }))
    .await;
    assert_selection(&d, other, &b).await;
    d.email(RawInboundPayload {
        headers: Some("Message-ID: <new-role@test>\n".into()),
        to: d.address(&first_desk.slug),
        from: CUSTOMER.into(),
        subject: Some("new role message".into()),
        text: Some("who answers?".into()),
        ..Default::default()
    })
    .await;
    Box::pin(
        d.run_until("a subsequent message resolved the new holder", async || {
            d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='succeeded'")
                .await
                == 3
        }),
    )
    .await;
    let latest: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_runs WHERE company_id=$1 ORDER BY created_at DESC,id DESC LIMIT 1",
    )
    .bind(first.company.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_selection(&d, RunId::new(latest), &b).await;
    let requests = d.llm.observed();
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert!(system_text(&request).contains("ROLE_INSTRUCTIONS_second"));
    }
    assert_eq!(d.llm.finish().await.unwrap(), 3);
}

async fn execute_original(d: &mut Deployment, queued: RunId, a: &RoleAgent) {
    Box::pin(
        d.run_until("the queued original role agent completed", async || {
            settled(d, queued).await
        }),
    )
    .await;
    assert_selection(d, queued, a).await;
    let requests = d.llm.observed();
    assert_eq!(requests.len(), 1);
    let system = system_text(&requests[0]);
    assert!(system.contains("ROLE_INSTRUCTIONS_first"), "{system}");
    assert!(
        !system.contains("ROLE_INSTRUCTIONS_first_UPDATED"),
        "{system}"
    );
    assert!(!system.contains("ROLE_INSTRUCTIONS_second"), "{system}");
    let tools = requests[0]["tools"]
        .as_array()
        .expect("agent.run retains its frozen tools");
    assert!(!tools.is_empty());
}
