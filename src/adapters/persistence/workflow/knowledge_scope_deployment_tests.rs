//! Scoped retrieval through actual admission, agent checkpoints and scripted provider requests.
use super::*;
use crate::{
    application::{
        knowledge::KnowledgeUploadTarget,
        ports::department::{DepartmentEdit, DepartmentWrite},
    },
    domain::workflow::{TriggerRef, TriggerSource},
    entities::{
        department::{DepartmentId, DepartmentRole},
        transport::PrincipalId,
    },
};
use std::collections::BTreeSet;

struct ScopedDesk {
    unit: DepartmentId,
    channel: Uuid,
}

async fn desk(d: &Deployment, slug: &str, parent: Option<DepartmentId>) -> ScopedDesk {
    let unit = crate::adapters::persistence::test_support::department_use_cases(d.p())
        .create_department(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            DepartmentWrite {
                name: slug.into(),
                slug: slug.into(),
                parent_id: parent,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    let channel = ChannelPersistence::list_by_company_id(d.p(), d.fixture.target.company.as_uuid())
        .await
        .unwrap()
        .into_iter()
        .find(|channel| channel.slug.as_str() == slug)
        .unwrap()
        .id;
    ScopedDesk { unit, channel }
}

async fn deployment(count: usize) -> Deployment {
    let d = Box::pin(Deployment::new(turns(count))).await;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(d.p().pool())
        .await
        .unwrap();
    d
}

async fn channel(d: &Deployment, slug: &str, unit: DepartmentId) -> Uuid {
    crate::adapters::persistence::test_support::create_channel(
        d.p(),
        d.fixture.target.company.as_uuid(),
        ChannelWrite {
            name: slug.into(),
            slug: slug.into(),
            department_id: Some(unit),
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

async fn agent(d: &Deployment, slug: &str) -> Uuid {
    use crate::entities::agent_capability::{AgentBuiltinTool, AgentToolBinding};
    let agent = crate::adapters::persistence::test_support::company_agents(d.p())
        .create(
            d.fixture.target.actor,
            d.fixture.target.company,
            AgentWrite {
                name: slug.into(),
                slug: slug.into(),
                provider: Some(SCRIPTED_PROVIDER.into()),
                model: Some(SCRIPTED_MODEL.into()),
                tool_bindings: Some(vec![AgentToolBinding::builtin(
                    AgentBuiltinTool::SearchKnowledge,
                )]),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .agent
        .id;
    register_scripted_agent_base_url(agent, &d.llm.base_url);
    agent
}

async fn lead(d: &Deployment, unit: DepartmentId, agent: Uuid) {
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(agent)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    crate::adapters::persistence::test_support::department_use_cases(d.p())
        .assign_role(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            unit,
            DepartmentRole::Lead,
            PrincipalId::new(principal),
        )
        .await
        .unwrap();
}

async fn lower_admin(d: &Deployment, units: &[DepartmentId]) -> WorkflowActor {
    let admin = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &d.fixture, "admin",
    )
    .await;
    sqlx::query(
        "UPDATE department_members SET access_level=3 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(admin)
    .execute(d.p().pool())
    .await
    .unwrap();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(admin)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    let departments = crate::adapters::persistence::test_support::department_use_cases(d.p());
    for unit in units {
        departments
            .add_member(
                d.fixture.target.actor.user_id(),
                d.fixture.target.company.as_uuid(),
                *unit,
                PrincipalId::new(principal),
            )
            .await
            .unwrap();
    }
    WorkflowActor::authenticated(admin).unwrap()
}

async fn upload(d: &Deployment, unit: DepartmentId, name: &str, text: &str) {
    d.knowledge
        .upload(
            KnowledgeUploadTarget {
                scope: scope(d, d.fixture.target.actor.user_id()),
                unit,
            },
            vec![file(name, text)],
        )
        .await
        .unwrap();
}

fn source(agents: &[&str]) -> Value {
    let mut steps = serde_json::Map::new();
    for (index, agent) in agents.iter().enumerate() {
        let next = if index + 1 == agents.len() {
            "$end".into()
        } else {
            format!("reader{}", index + 1)
        };
        steps.insert(format!("reader{index}"), json!({"type":"agent.run","with":{
            "agent":{"literal":agent},"context":{"literal":"Search the scoped canary notes."},
            "output_schema":{"literal":{"type":"string","maxLength":128}}},"routes":{"success":next}}));
    }
    json!({"format_version":1,"workflow_id":Uuid::nil().to_string(),"input_schema":{"type":"object"},
        "parameter_schema":true,"output_schema":true,"resources":[],"entry":"reader0","steps":steps,
        "limits":{"max_steps":100,"max_context_bytes":65536}})
}

fn turns(count: usize) -> Vec<LlmTurn> {
    (0..count)
        .flat_map(|_| {
            [
                LlmTurn::tool_call("search_knowledge", json!({"query":"scoped canary notes"})),
                LlmTurn::text("\"done\""),
            ]
        })
        .collect()
}

async fn admit(d: &Deployment, binding: BindingTarget, actor: WorkflowActor) -> RunId {
    let company = d.fixture.target.company;
    let result = d
        .engine
        .services()
        .service()
        .admit(AdmitWorkflowRequest {
            history_session: None,
            company_id: company,
            actor,
            association: RelatedAssociation::Channel(RelatedChannelId::new(d.channel)),
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: crate::entities::correlation::CorrelationId::new(),
            binding_id: binding.binding,
            idempotency_key: IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::InAppOnly,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await
        .unwrap();
    let AdmissionResult::Created(run) = result else {
        panic!("new scoped run expected: {result:?}")
    };
    run
}

async fn execute(d: &Deployment, run: RunId) {
    Box::pin(d.run_until("scoped knowledge run completed", async || {
        let state: String =
            sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
                .bind(d.fixture.target.company.as_uuid())
                .bind(run.as_uuid())
                .fetch_one(d.p().pool())
                .await
                .unwrap();
        assert_ne!(state, "failed", "{}", d.diagnostics().await);
        state == "succeeded"
    }))
    .await;
}

fn assert_passages(request: &Value, expected: &[&str]) {
    let tool = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("actual tool result in provider request");
    let result: Value = serde_json::from_str(tool["content"].as_str().unwrap()).unwrap();
    let found: BTreeSet<_> = result["passages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|passage| {
            assert_eq!(
                passage.as_object().unwrap().len(),
                3,
                "public passage schema remains file/heading/text"
            );
            passage["text"].as_str().unwrap()
        })
        .collect();
    assert_eq!(found, expected.iter().copied().collect());
}

async fn read_units(d: &Deployment, run: RunId) -> BTreeSet<Uuid> {
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT unnest(queried_unit_ids) FROM workflow_knowledge_consumptions WHERE company_id=$1 AND run_id=$2")
        .bind(d.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_all(d.p().pool()).await.unwrap();
    ids.into_iter().collect()
}

#[tokio::test]
async fn knowledge_scope_search_deployment_two_desks_ancestors_lead_level_and_frozen_topology() {
    let mut d = Box::pin(deployment(7)).await;
    let reader = agent(&d, "scope-reader").await;
    let a = desk(&d, "scope-a", None).await;
    let b = desk(&d, "scope-b", None).await;
    let child = desk(&d, "scope-child", Some(a.unit)).await;
    let grandchild = desk(&d, "scope-grandchild", Some(child.unit)).await;
    let root = target(&d, d.fixture.target.actor.user_id()).await.unit;
    let root_desk = channel(&d, "scope-root", root).await;
    for (unit, name, text) in [
        (root, "0_root.md", "ROOT CANARY"),
        (a.unit, "0_a.md", "A CANARY"),
        (b.unit, "0_b.md", "B CANARY"),
        (child.unit, "0_child.md", "CHILD CANARY"),
        (grandchild.unit, "0_grandchild.md", "GRANDCHILD CANARY"),
        (a.unit, "5_private.md", "PRIVATE CANARY"),
    ] {
        upload(&d, unit, name, text).await;
    }
    // An actual admin may have a lower level; the canonical owner must remain level 9.
    let actor = lower_admin(&d, &[a.unit, b.unit, child.unit]).await;
    for (channel, expected) in [
        (a.channel, vec!["ROOT CANARY", "A CANARY"]),
        (b.channel, vec!["ROOT CANARY", "B CANARY"]),
        (root_desk, vec!["ROOT CANARY"]),
        (
            child.channel,
            vec!["ROOT CANARY", "A CANARY", "CHILD CANARY"],
        ),
    ] {
        d.channel = channel;
        let binding = d.bind(source(&["scope-reader"]), None).await;
        let run = admit(&d, binding, actor).await;
        execute(&d, run).await;
        assert_passages(d.llm.observed().last().unwrap(), &expected);
    }
    // Lead entitlements belong to the acting principal, independently of its current desk.
    lead(&d, a.unit, reader).await;
    d.channel = a.channel;
    let binding = d.bind(source(&["scope-reader"]), None).await;
    let led = admit(&d, binding, actor).await;
    execute(&d, led).await;
    assert_passages(
        d.llm.observed().last().unwrap(),
        &["ROOT CANARY", "A CANARY", "CHILD CANARY"],
    );
    Box::pin(check_frozen_topology(&mut d, &a, &b, &child, root, actor)).await;
    d.knowledge.shutdown().await.unwrap();
    assert_eq!(d.llm.finish().await.unwrap(), 14);
}

async fn check_frozen_topology(
    d: &mut Deployment,
    a: &ScopedDesk,
    b: &ScopedDesk,
    child: &ScopedDesk,
    root: DepartmentId,
    actor: WorkflowActor,
) {
    d.channel = a.channel;
    let binding = d.bind(source(&["scope-reader"]), None).await;
    let queued = admit(d, binding, actor).await;
    let before = read_snapshot(d, queued).await;
    let departments = crate::adapters::persistence::test_support::department_use_cases(d.p());
    departments
        .update_department(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            child.unit,
            DepartmentEdit {
                name: Some("scope-child-new".into()),
                slug: Some("scope-child-new".into()),
                parent_id: Some(Some(b.unit)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(read_snapshot(d, queued).await, before);
    execute(d, queued).await;
    let requests = d.llm.observed();
    let request = requests.last().unwrap();
    assert_passages(request, &["ROOT CANARY", "A CANARY", "CHILD CANARY"]);
    let old_units = read_units(d, queued).await;
    assert!(old_units.contains(&child.unit.as_uuid()));
    let system = request["messages"].to_string();
    assert!(system.contains("Memory write scope: unit:scope-a; searchable units for this run: "));
    assert!(system.contains("scope-child"));
    assert!(
        !system.contains("scope-child-new"),
        "host context keeps frozen labels"
    );
    let fresh = admit(d, binding, actor).await;
    execute(d, fresh).await;
    assert_passages(
        d.llm.observed().last().unwrap(),
        &["ROOT CANARY", "A CANARY"],
    );
    assert_eq!(
        read_units(d, fresh).await,
        BTreeSet::from([root.as_uuid(), a.unit.as_uuid()])
    );
}

async fn read_snapshot(d: &Deployment, run: RunId) -> Value {
    sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(scope) ORDER BY scope.acting_principal_id,scope.access,scope.unit_id) FROM workflow_run_scopes AS scope WHERE company_id=$1 AND run_id=$2")
        .bind(d.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(d.p().pool()).await.unwrap()
}

#[tokio::test]
async fn knowledge_scope_search_deployment_keeps_each_acting_agents_units_separate() {
    let mut d = Box::pin(deployment(2)).await;
    let lead_agent = agent(&d, "lead-reader").await;
    agent(&d, "ordinary-reader").await;
    let local = desk(&d, "actor-local", None).await;
    let report = desk(&d, "actor-report", Some(local.unit)).await;
    lead(&d, local.unit, lead_agent).await;
    upload(&d, local.unit, "0_local.md", "LOCAL CANARY").await;
    upload(&d, report.unit, "0_report.md", "REPORT CANARY").await;
    d.channel = local.channel;
    let binding = d
        .bind(source(&["lead-reader", "ordinary-reader"]), None)
        .await;
    let run = admit(&d, binding, d.fixture.target.actor).await;
    execute(&d, run).await;
    let requests = d.llm.observed();
    assert_eq!(requests.len(), 4);
    assert_passages(&requests[1], &["LOCAL CANARY", "REPORT CANARY"]);
    assert_passages(&requests[3], &["LOCAL CANARY"]);
    let proofs: Vec<Vec<Uuid>>=sqlx::query_scalar("SELECT proof.queried_unit_ids FROM workflow_knowledge_consumptions AS proof JOIN workflow_executions AS execution ON (execution.company_id,execution.id)=(proof.company_id,proof.execution_id) WHERE proof.company_id=$1 AND proof.run_id=$2 ORDER BY execution.step_id")
        .bind(d.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_all(d.p().pool()).await.unwrap();
    assert_eq!(proofs.len(), 2);
    assert!(proofs[0].contains(&report.unit.as_uuid()));
    assert!(!proofs[1].contains(&report.unit.as_uuid()));
    d.knowledge.shutdown().await.unwrap();
    assert_eq!(d.llm.finish().await.unwrap(), 4);
}
