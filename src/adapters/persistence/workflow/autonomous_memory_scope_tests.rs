use super::*;
use crate::application::memory::MemorySettingsStore;
use crate::ports::channel::ChannelWrite;

async fn own_workflow_tool(setup: &AgentSetup) -> GraphWorkflow {
    let p = &setup.fixture.persistence;
    p.enable_memory(
        setup.fixture.target.company,
        setup.fixture.target.actor,
        0,
        Uuid::new_v4(),
    )
    .await
    .unwrap();
    let mut source: Value =
        serde_json::from_str(&registry::example("memory.load").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["scope"] = json!({"literal":{"unit":"own"}});
    let child = Box::pin(publish_graph_definition(setup, source)).await;
    let binding = register_child(setup, &child, "own_memory").await;
    AgentPersistence::update(
        p,
        setup.agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            tool_bindings: Some(vec![binding]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    setup.fixture.publish_agent(setup.agent).await;
    child
}

async fn effects(setup: &AgentSetup) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'runs',(SELECT count(*) FROM workflow_runs WHERE company_id=$1),
            'jobs',(SELECT count(*) FROM background_tasks WHERE company_id=$1),
            'deliveries',(SELECT count(*) FROM memory_deliveries WHERE company_id=$1))",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .fetch_one(setup.fixture.persistence.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn memory_scope_admission_agent_workflow_tool_own_refuses_before_root_effects() {
    let mut llm = scripted_llm(vec![]).await;
    let setup = Box::pin(agent_setup(&llm.base_url)).await;
    own_workflow_tool(&setup).await;
    let root = root_graph(&setup).await;
    let before = effects(&setup).await;
    let result = admit_graph(&setup, &root, "own-descendant-no-context").await;
    assert!(
        matches!(&result, Err(error) if error.to_string().contains("binding.memory_scope_needs_desk")),
        "root admission must refuse before any effect: {result:?}"
    );
    assert_eq!(effects(&setup).await, before);
    assert!(llm.observed().is_empty());
    assert_eq!(llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn memory_scope_admission_agent_workflow_tool_own_accepts_a_real_desk_context() {
    let mut llm = scripted_llm(vec![]).await;
    let setup = Box::pin(agent_setup(&llm.base_url)).await;
    let child = own_workflow_tool(&setup).await;
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let unit = crate::adapters::persistence::company::root_on(
        &mut p.pool().acquire().await.unwrap(),
        company,
    )
    .await
    .unwrap();
    let desk = crate::adapters::persistence::test_support::create_channel(
        p,
        company.as_uuid(),
        ChannelWrite {
            name: "Scope preflight".into(),
            slug: "scope-preflight".into(),
            enabled: true,
            department_id: Some(unit),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let association = RelatedAssociation::Channel(RelatedChannelId::new(desk.id));
    let graph = Box::pin(publish_graph_definition(
        &setup,
        model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun),
    ))
    .await;
    Box::pin(activate_graph(&setup, &graph, json!({}), association)).await;
    let mut request =
        super::super::autonomous_admission::request(&setup, "own-descendant-desk", Uuid::new_v4())
            .await;
    request.binding_id = graph.binding.binding;
    request.association = association;
    let result = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(request)
        .await
        .unwrap();
    let AdmissionResult::Created(run) = result else {
        panic!("new desk admission")
    };
    let selected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_dependency_manifests
         WHERE company_id=$1 AND root_run_id=$2 AND content LIKE '%' || $3 || '%')",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .bind(child.version.to_string())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(
        selected,
        "the actual workflow-tool descendant was traversed"
    );
    let own: Vec<Uuid> = sqlx::query_scalar(
        "SELECT unit_id FROM workflow_run_scopes WHERE company_id=$1 AND run_id=$2
         AND acting_principal_id IS NULL AND access='write'",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(own, vec![unit.as_uuid()]);
    assert!(llm.observed().is_empty());
    assert_eq!(llm.finish().await.unwrap(), 0);
}
