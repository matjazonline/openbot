use super::*;
use crate::application::agent::{AgentPublicationScope, AgentPublications, PublishedAgentRevision};
use crate::application::workflow::activation::*;
use crate::entities::agent_capability::{AgentBindingId, AgentId, AgentToolAlias};

pub(super) struct OriginHistory {
    pub setup: Box<AgentSetup>,
    pub survivor: RunId,
    pub original: Value,
    pub later: PublishedAgentRevision,
    pub foreign: PublishedAgentRevision,
}

async fn foreign_agent(setup: &AgentSetup) -> PublishedAgentRevision {
    let mut fixture = Fixture::in_database(setup.fixture._db.clone(), "-origin-foreign").await;
    fixture.persistence = setup.fixture.persistence.clone();
    CompanyPersistence::replace_model_connections_for_user(
        &fixture.persistence,
        fixture.target.actor.user_id(),
        fixture.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("test-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let agent = AgentPersistence::create(
        &fixture.persistence,
        fixture.target.company.as_uuid(),
        AgentWrite {
            name: "Foreign origin".into(),
            slug: "foreign-origin".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    fixture.publish_agent(agent.id).await
}

async fn publish_later(setup: &AgentSetup) -> PublishedAgentRevision {
    let p = &setup.fixture.persistence;
    let scope = AgentPublicationScope {
        company: setup.fixture.target.company,
        actor: setup.fixture.target.actor,
        agent: AgentId::new(setup.agent),
    };
    let original = p.active_agent_revision(scope).await.unwrap().unwrap();
    let binding = original.definition.tools[0].binding.clone();
    let mut additional = binding.clone();
    additional.id = AgentBindingId::new(Uuid::new_v4());
    additional.alias = AgentToolAlias::parse("additional.request").unwrap();
    AgentPersistence::update(
        p,
        setup.agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("A later active definition".into()),
            tool_bindings: Some(vec![binding, additional]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    setup.fixture.publish_agent(setup.agent).await
}

async fn survivor(setup: &AgentSetup) -> RunId {
    let p = &setup.fixture.persistence;
    let request =
        super::super::autonomous_admission::request(setup, "origin-surviving-run", Uuid::new_v4())
            .await;
    match WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(request)
        .await
        .unwrap()
    {
        AdmissionResult::Created(run) => run,
        _ => panic!("a distinct logical run must be admitted"),
    }
}

async fn claim(setup: &AgentSetup, run: RunId) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    let execution: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_executions WHERE company_id=$1 AND run_id=$2")
            .bind(setup.fixture.target.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let job: Uuid = sqlx::query_scalar(
        "SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .bind(execution)
    .fetch_one(p.pool())
    .await
    .unwrap();
    p.claim_io(
        ActivationRequest {
            company: setup.fixture.target.company,
            run,
            execution: ExecutionId::new(execution),
            job: WorkflowJobId(job),
        },
        WorkflowWorkerId(Uuid::new_v4()),
        lease(),
    )
    .await
    .unwrap()
    .unwrap()
}

async fn run_both(setup: &AgentSetup, survivor: RunId) {
    let turn = || {
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/messages","body":{"message":"hello"}}),
        )
    };
    let mut llm = scripted_llm(vec![
        turn(),
        LlmTurn::text("\"done\""),
        turn(),
        LlmTurn::text("\"done\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(setup);
    let kind = WorkflowStepKind("agent.run".into());
    assert_eq!(
        Box::pin(runtime.execute(&kind, &setup.claim))
            .await
            .completed_result()
            .unwrap(),
        json!("done")
    );
    let queued = claim(setup, survivor).await;
    assert_eq!(
        Box::pin(runtime.execute(&kind, &queued))
            .await
            .completed_result()
            .unwrap(),
        json!("done")
    );
    for request in llm.observed() {
        assert!(
            !request["tools"].to_string().contains("additional__request"),
            "old selections cannot expand on publication"
        );
    }
    assert_eq!(llm.finish().await.unwrap(), 4);
}

pub(super) async fn populated() -> OriginHistory {
    let (origin, server) = servers(2).await;
    let setup = Box::new(Box::pin(agent_setup(&origin)).await);
    let foreign = foreign_agent(&setup).await;
    // These fixtures normally permit frozen-row fault injection. This proof uses
    // genuine history with the strict native run guard restored as well.
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(setup.fixture.persistence.pool())
        .await
        .unwrap();
    let survivor = survivor(&setup).await;
    let later = publish_later(&setup).await;
    Box::pin(run_both(&setup, survivor)).await;
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.starts_with("POST /messages"))
    );
    let original:Value=sqlx::query_scalar("SELECT to_jsonb(intent) FROM workflow_action_intents AS intent WHERE company_id=$1 AND run_id=$2 AND origin_kind='agent'")
        .bind(setup.fixture.target.company.as_uuid()).bind(setup.claim.fence.scope.run.as_uuid()).fetch_one(setup.fixture.persistence.pool()).await.unwrap();
    let old_binding: Uuid = serde_json::from_value(original["agent_binding_id"].clone()).unwrap();
    assert!(
        later
            .definition
            .tools
            .iter()
            .any(|tool| tool.binding.id.as_uuid() == old_binding)
    );
    OriginHistory {
        setup,
        survivor,
        original,
        later,
        foreign,
    }
}
