use super::*;
use crate::application::workflow::{
    lease::FencedWorkflowResult,
    waits::{WaitProgress, WorkflowWaits},
};
use crate::entities::agent_capability::DependencyManifestId;

struct ChildGraph {
    agent: AgentId,
    revision: crate::application::agent::PublishedAgentRevision,
    workflow: GraphWorkflow,
    leaf: GraphWorkflow,
}

async fn child_graph(setup: &AgentSetup) -> ChildGraph {
    let leaf = publish_graph(
        setup,
        model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::Classify),
        json!({}),
    )
    .await;
    let binding = register_child(setup, &leaf, "agent.leaf").await;
    let agent = AgentPersistence::create(
        &setup.fixture.persistence,
        setup.fixture.target.company.as_uuid(),
        AgentWrite {
            name: "Child only".into(),
            slug: "child-only".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some(
                "Original child instructions selected before first execution".into(),
            ),
            tool_bindings: Some(vec![binding]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let revision = setup.fixture.publish_agent(agent.id).await;
    let workflow = publish_graph(
        setup,
        model_source(&AgentKey::parse("child-only").unwrap(), ModelStep::AgentRun),
        json!({}),
    )
    .await;
    ChildGraph {
        agent: AgentId::new(agent.id),
        revision,
        workflow,
        leaf,
    }
}

async fn root_graph(setup: &AgentSetup, child: &ChildGraph) -> GraphWorkflow {
    let binding = register_child(setup, &child.workflow, "agent.child").await;
    AgentPersistence::update(
        &setup.fixture.persistence,
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
    publish_graph(
        setup,
        model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun),
        json!({}),
    )
    .await
}

async fn repin_child(setup: &AgentSetup, child: &ChildGraph) -> VersionId {
    let p = &setup.fixture.persistence;
    let fixture = Fixture {
        _db: setup.fixture._db.clone(),
        persistence: p.clone(),
        target: DefinitionTarget {
            workflow: child.workflow.workflow,
            ..setup.fixture.target
        },
    };
    let service = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: Preflight,
        },
    );
    let mut draft = fixture.save(Some(DraftRevision::new(1).unwrap()));
    let mut source = model_source(&AgentKey::parse("child-only").unwrap(), ModelStep::AgentRun);
    source["workflow_id"] = json!(child.workflow.workflow.to_string());
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.expected = DraftRevision::new(2).unwrap();
    publication.version = VersionId::new(Uuid::new_v4());
    publication.key = IdempotencyKey::parse("child-repin-v2").unwrap();
    let version = publication.version;
    service.publish(publication).await.unwrap();
    let current = p
        .workflow_tools(fixture.target.company, fixture.target.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|tool| tool.name.as_str() == "agent.child")
        .unwrap();
    p.register_workflow_tool(
        fixture.target.company,
        fixture.target.actor,
        WorkflowToolWrite {
            name: current.name,
            workflow: child.workflow.workflow,
            version,
            params: json!({}),
            expected: Some(current.revision),
        },
    )
    .await
    .unwrap();
    version
}

async fn actual_child(setup: &AgentSetup, parent: RunId) -> RunId {
    let child: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2")
            .bind(setup.fixture.target.company.as_uuid())
            .bind(parent.as_uuid())
            .fetch_one(setup.fixture.persistence.pool())
            .await
            .unwrap();
    RunId::new(child)
}

#[tokio::test]
async fn autonomous_agent_children_first_tool_child_keeps_root_selected_agent_and_workflow_after_publication_and_repin()
 {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let child = child_graph(&setup).await;
    let root = root_graph(&setup, &child).await;
    let p = &setup.fixture.persistence;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(p.pool())
        .await
        .unwrap();
    let AdmissionResult::Created(run) = admit_graph(&setup, &root, "child-before-publish")
        .await
        .unwrap()
    else {
        panic!("new root")
    };
    let claim = claim_run(&setup, run).await;
    let root_manifest = p
        .prepare_agent(
            &claim,
            crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(
                60,
            ))
            .unwrap(),
        )
        .await
        .unwrap()
        .expect_ready()
        .prepared
        .selection()
        .manifest_id();
    sqlx::query(
        "UPDATE agents SET system_prompt='Later child behavior' WHERE company_id=$1 AND id=$2",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .bind(child.agent.as_uuid())
    .execute(p.pool())
    .await
    .unwrap();
    let new_agent = setup.fixture.publish_agent(child.agent.as_uuid()).await;
    let new_version = repin_child(&setup, &child).await;
    assert_ne!(new_agent.id, child.revision.id);
    assert_ne!(new_version, child.workflow.version);
    let root_llm = scripted_llm(vec![
        LlmTurn::tool_call("child", json!({})),
        LlmTurn::text("\"support\""),
        LlmTurn::text("\"parent complete\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &root_llm.base_url);
    assert!(matches!(
        Box::pin(agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &claim))
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let child_run = actual_child(&setup, run).await;
    let child_claim = claim_run(&setup, child_run).await;
    assert_child_selection(&setup, &child, &child_claim, root_manifest).await;
    let continued = finish_and_resume(&setup, &claim, &child_claim, &child).await;
    assert_eq!(
        Box::pin(agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &continued))
            .await
            .completed_result()
            .unwrap(),
        json!("parent complete")
    );
    assert_eq!(root_llm.finish().await.unwrap(), 3);
    let children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(children, 1);
}

async fn assert_child_selection(
    setup: &AgentSetup,
    child: &ChildGraph,
    claim: &ClaimedWorkflow,
    root_manifest: DependencyManifestId,
) {
    let p = &setup.fixture.persistence;
    let selected = p
        .prepare_agent(
            claim,
            crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(
                60,
            ))
            .unwrap(),
        )
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(selected.prepared.selection().manifest_id(), root_manifest);
    assert_eq!(
        selected.prepared.selection().revision().id,
        child.revision.id
    );
    assert_eq!(
        selected.prepared.selection().agent().instructions,
        child.revision.definition.instructions
    );
    let child_version: Uuid =
        sqlx::query_scalar("SELECT version_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(setup.fixture.target.company.as_uuid())
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(child_version, child.workflow.version.as_uuid());
}

async fn finish_and_resume(
    setup: &AgentSetup,
    parent: &ClaimedWorkflow,
    claim: &ClaimedWorkflow,
    child: &ChildGraph,
) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call("child", json!({})),
        LlmTurn::text("\"old child complete\""),
    ])
    .await;
    register_scripted_agent_base_url(child.agent.as_uuid(), &llm.base_url);
    assert!(matches!(
        Box::pin(agent_runtime(setup).execute(&WorkflowStepKind("agent.run".into()), claim)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let leaf = actual_child(setup, claim.fence.scope.run).await;
    let leaf_claim = claim_run(setup, leaf).await;
    let leaf_prepared = p
        .prepare_agent(
            &leaf_claim,
            crate::application::workflow::lease::LeasePolicy::new(std::time::Duration::from_secs(
                60,
            ))
            .unwrap(),
        )
        .await
        .unwrap()
        .expect_ready();
    let root_manifest: Uuid = sqlx::query_scalar(
        "SELECT manifest_id FROM run_dependency_manifests WHERE company_id=$1 AND run_id=$2",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .bind(parent.fence.scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        leaf_prepared.prepared.selection().manifest_id().as_uuid(),
        root_manifest
    );
    let leaf_version: Uuid =
        sqlx::query_scalar("SELECT version_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(setup.fixture.target.company.as_uuid())
            .bind(leaf.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(leaf_version, child.leaf.version.as_uuid());
    complete_model(setup, &leaf_claim, ModelStep::Classify).await;
    let continued = resume_once(setup, claim).await;
    complete_model(setup, &continued, ModelStep::AgentRun).await;
    assert_eq!(llm.finish().await.unwrap(), 2);
    resume_once(setup, parent).await
}

async fn complete_model(setup: &AgentSetup, claim: &ClaimedWorkflow, kind: ModelStep) {
    let output =
        Box::pin(agent_runtime(setup).execute(&WorkflowStepKind(kind.kind().into()), claim))
            .await
            .completed_result()
            .unwrap();
    setup
        .fixture
        .persistence
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .unwrap();
}

async fn resume_once(setup: &AgentSetup, parent: &ClaimedWorkflow) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    let (one, two) = tokio::join!(
        p.resume_wait(parent.fence.scope),
        p.resume_wait(parent.fence.scope)
    );
    assert_eq!(
        [one.unwrap(), two.unwrap()]
            .iter()
            .filter(|outcome| matches!(outcome, WaitProgress::Continued))
            .count(),
        1
    );
    p.claim_io(
        parent.fence.scope,
        WorkflowWorkerId(Uuid::new_v4()),
        lease(),
    )
    .await
    .unwrap()
    .unwrap()
}

#[tokio::test]
async fn autonomous_agent_views_child_call_and_dependency_versions_use_root_manifest() {
    use crate::application::workflow::run_views::{CallRecordKind, RunReadScope, RunViews};
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let child = child_graph(&setup).await;
    let root = root_graph(&setup, &child).await;
    let p = &setup.fixture.persistence;
    let AdmissionResult::Created(run) = admit_graph(&setup, &root, "views-root-before-repin")
        .await
        .unwrap()
    else {
        panic!("root admitted")
    };
    let claim = claim_run(&setup, run).await;
    sqlx::query(
        "UPDATE agents SET system_prompt='later child instructions' WHERE company_id=$1 AND id=$2",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .bind(child.agent.as_uuid())
    .execute(p.pool())
    .await
    .unwrap();
    let new_revision = setup.fixture.publish_agent(child.agent.as_uuid()).await;
    let new_version = repin_child(&setup, &child).await;
    assert_ne!(new_revision.id, child.revision.id);
    assert_ne!(new_version, child.workflow.version);
    let llm = scripted_llm(vec![LlmTurn::tool_call("child", json!({}))]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    assert!(matches!(
        Box::pin(agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &claim))
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    assert_eq!(llm.finish().await.unwrap(), 1);
    let scope = RunReadScope {
        company: setup.fixture.target.company,
        run,
        viewer: setup.fixture.target.actor,
    };
    let root_view = p.run_view(scope).await.unwrap().unwrap();
    let linked = &root_view.children[0];
    let version = linked.dependency.as_ref().unwrap();
    assert_eq!(version.workflow_id, child.workflow.workflow);
    assert_eq!(version.version_id, child.workflow.version);
    let child_view = p
        .run_view(RunReadScope {
            run: linked.run,
            ..scope
        })
        .await
        .unwrap()
        .unwrap();
    let dependencies = root_view.dependencies.as_ref().unwrap();
    assert_eq!(child_view.dependencies.as_ref().unwrap(), dependencies);
    assert_eq!(dependencies.root, run);
    assert!(
        dependencies
            .agents
            .iter()
            .any(|agent| agent.agent_id == child.agent && agent.revision_id == child.revision.id)
    );
    assert!(dependencies.workflows.contains(version));
    assert!(
        !dependencies
            .workflows
            .iter()
            .any(|workflow| workflow.version_id == new_version)
    );
    let origin = &root_view.calls.items[0];
    assert_eq!(origin.kind, CallRecordKind::ChildToolCall);
    assert_eq!(Some(origin.record), linked.tool_call);
    assert_eq!(origin.origin.agent.as_uuid(), setup.agent);
    assert!(
        matches!(&origin.tool.target, crate::application::agent::AgentToolChoice::Workflow { workflow, version, .. }
        if *workflow == child.workflow.workflow && *version == child.workflow.version)
    );
    let step = p
        .step_view(scope, claim.fence.scope.execution.as_uuid())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(step.calls, root_view.calls);
}
