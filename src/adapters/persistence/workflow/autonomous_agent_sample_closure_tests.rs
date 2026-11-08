use super::*;
use crate::application::workflow::sample::{SampleReport, SampleRequest, SampleStatus};
use crate::application::workflow::sample_capture::SampleDefinitionCapture;

#[path = "autonomous_agent_sample_bounds_tests.rs"]
mod bounds;

#[path = "agent_history_sample_tests.rs"]
mod history;

fn capture_service(setup: &AgentSetup) -> crate::adapters::persistence::workflow::SampleCapture {
    crate::adapters::persistence::workflow::SampleCapture {
        persistence: setup.fixture.persistence.clone(),
        http: vec![],
        mcp: vec![],
        refused_kinds: &[],
    }
}

fn target(setup: &AgentSetup, graph: &GraphWorkflow) -> DefinitionTarget {
    DefinitionTarget {
        workflow: graph.workflow,
        ..setup.fixture.target
    }
}

fn sample_request(responses: Vec<Value>) -> SampleRequest {
    SampleRequest {
        response_contract: None,
        response_approval_policy: None,
        input: json!({}),
        params: json!({}),
        scripts: serde_json::from_value(Value::Array(responses)).unwrap(),
        agents: vec![],
    }
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct ProductionCounts {
    runs: i64,
    revisions: i64,
    actions: i64,
    checkpoints: i64,
}

async fn production_counts(setup: &AgentSetup) -> ProductionCounts {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_runs WHERE company_id=$1) AS runs,
         (SELECT count(*) FROM agent_revisions WHERE company_id=$1) AS revisions,
         (SELECT count(*) FROM workflow_action_intents WHERE company_id=$1) AS actions,
         (SELECT count(*) FROM task_harness_runs WHERE company_id=$1) AS checkpoints",
    )
    .bind(setup.fixture.target.company.as_uuid())
    .fetch_one(setup.fixture.persistence.pool())
    .await
    .unwrap()
}

async fn draft_agent(
    setup: &AgentSetup,
    key: &AgentKey,
    instructions: String,
    tools: Vec<AgentToolBinding>,
) -> AgentId {
    let agent = AgentPersistence::create(
        &setup.fixture.persistence,
        setup.fixture.target.company.as_uuid(),
        AgentWrite {
            name: key.as_str().to_owned(),
            slug: key.as_str().to_owned(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some(instructions),
            tool_bindings: Some(tools),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentId::new(agent.id)
}

async fn assistant_binding(setup: &AgentSetup, binding: AgentToolBinding, instructions: &str) {
    AgentPersistence::update(
        &setup.fixture.persistence,
        setup.agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some(instructions.into()),
            tool_bindings: Some(vec![binding]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

struct NestedSample {
    setup: Box<AgentSetup>,
    root: GraphWorkflow,
    middle: GraphWorkflow,
    leaf: GraphWorkflow,
    second: AgentId,
    root_binding: AgentToolBinding,
}

async fn nested_sample() -> NestedSample {
    let setup = Box::new(Box::pin(agent_setup("http://127.0.0.1:1")).await);
    let assistant = AgentKey::parse("assistant").unwrap();
    // The same agent's classify view is a leaf even though its run view owns children.
    let leaf = publish_graph(
        &setup,
        model_source(&assistant, ModelStep::Classify),
        json!({}),
    )
    .await;
    let leaf_binding = register_child(&setup, &leaf, "sample_leaf").await;
    let second_key = AgentKey::parse("sample-second").unwrap();
    let second = draft_agent(
        &setup,
        &second_key,
        "Second agent".into(),
        vec![leaf_binding],
    )
    .await;
    setup.fixture.publish_agent(second.as_uuid()).await;
    let middle = publish_graph(
        &setup,
        model_source(&second_key, ModelStep::AgentRun),
        json!({}),
    )
    .await;
    let root_binding = register_child(&setup, &middle, "sample_middle").await;
    assistant_binding(&setup, root_binding.clone(), "Initial sample instructions").await;
    setup.fixture.publish_agent(setup.agent).await;
    let root = publish_graph(
        &setup,
        model_source(&assistant, ModelStep::AgentRun),
        json!({}),
    )
    .await;
    NestedSample {
        setup,
        root,
        middle,
        leaf,
        second,
        root_binding,
    }
}

fn nested_request(f: &NestedSample) -> SampleRequest {
    sample_request(vec![
        json!({"workflow_id":f.root.workflow,"step":"start","responses":[{
            "output":"root", "tools":[{"name":"child","input":{},"output":null}]
        }]}),
        json!({"workflow_id":f.middle.workflow,"step":"start","responses":[{
            "output":"middle", "tools":[{"name":"child","input":{},"output":null}]
        }]}),
        json!({"workflow_id":f.leaf.workflow,"step":"start","responses":[{"output":"support"}]}),
    ])
}

async fn republish_middle(f: &NestedSample) -> VersionId {
    let p = &f.setup.fixture.persistence;
    let fixture = Fixture {
        _db: f.setup.fixture._db.clone(),
        persistence: p.clone(),
        target: target(&f.setup, &f.middle),
    };
    let directory =
        crate::adapters::persistence::workflow::child_publication::ChildPublicationDirectory {
            persistence: p.clone(),
            inner: AgentPublicationDirectory {
                persistence: p.clone(),
                inner: Preflight,
            },
        };
    let service = DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut source = model_source(
        &AgentKey::parse("sample-second").unwrap(),
        ModelStep::AgentRun,
    );
    source["workflow_id"] = json!(f.middle.workflow);
    source["steps"]["start"]["with"]["context"] = json!({"literal":"A later child definition"});
    let mut draft = fixture.save(Some(DraftRevision::new(1).unwrap()));
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
    let mut publication = fixture.publish();
    publication.expected = DraftRevision::new(2).unwrap();
    publication.version = VersionId::new(Uuid::new_v4());
    publication.key = IdempotencyKey::parse("sample-middle-v2").unwrap();
    let version = publication.version;
    service.publish(publication).await.unwrap();
    let registration = p
        .workflow_tools(fixture.target.company, fixture.target.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.name.as_str() == "sample_middle")
        .unwrap();
    p.register_workflow_tool(
        fixture.target.company,
        fixture.target.actor,
        WorkflowToolWrite {
            version,
            expected: Some(registration.revision),
            name: registration.name,
            workflow: f.middle.workflow,
            params: json!({}),
        },
    )
    .await
    .unwrap();
    version
}

fn assert_nested_selection(f: &NestedSample, report: &SampleReport) {
    assert_eq!(
        report.status,
        SampleStatus::Completed,
        "{:?}",
        report.diagnostic
    );
    assert_eq!(report.usage.direct_tool_calls, 2);
    assert_eq!(report.usage.model_calls, 5);
    let agents: std::collections::BTreeSet<_> =
        report.agent_selections.iter().map(|a| a.agent_id).collect();
    assert_eq!(
        agents,
        [AgentId::new(f.setup.agent), f.second]
            .into_iter()
            .collect()
    );
    let workflows: std::collections::BTreeSet<_> = report
        .workflow_selections
        .iter()
        .map(|w| (w.workflow_id, w.version_id))
        .collect();
    assert_eq!(
        workflows,
        [
            (f.root.workflow, VersionId::new(Uuid::nil())),
            (f.middle.workflow, f.middle.version),
            (f.leaf.workflow, f.leaf.version)
        ]
        .into_iter()
        .collect()
    );
}

fn assert_same_sample(report: &SampleReport, replay: &SampleReport) {
    assert_eq!(
        replay.status,
        SampleStatus::Completed,
        "{:?}",
        replay.diagnostic
    );
    assert_eq!(report.configuration_hash, replay.configuration_hash);
    assert_eq!(
        serde_json::to_value(&report.workflow_selections).unwrap(),
        serde_json::to_value(&replay.workflow_selections).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&report.effects).unwrap(),
        serde_json::to_value(&replay.effects).unwrap()
    );
}

async fn capture_reselected_draft(
    f: &NestedSample,
    capture: &impl SampleDefinitionCapture,
    version: VersionId,
) -> SampleReport {
    let stale = capture
        .capture(
            target(&f.setup, &f.root),
            DraftRevision::new(1).unwrap(),
            nested_request(f),
        )
        .await
        .err()
        .expect("fresh draft capture must refuse a stale reviewed child contract");
    assert!(
        stale.to_string().contains("reviewed contract changed"),
        "{stale}"
    );
    let mut binding = f.root_binding.clone();
    binding.id = AgentBindingId::new(Uuid::new_v4());
    let AgentBindingSource::Workflow { version: pin, .. } = &mut binding.source else {
        panic!("the nested fixture owns a workflow binding")
    };
    *pin = version;
    assistant_binding(&f.setup, binding, "Explicitly reselected child contract").await;
    let current = capture
        .capture(
            target(&f.setup, &f.root),
            DraftRevision::new(1).unwrap(),
            nested_request(f),
        )
        .await
        .unwrap();
    crate::application::workflow::sample::execute(current).unwrap()
}

#[tokio::test]
async fn autonomous_agent_samples_nested_aliases_and_model_only_leaf_keep_exact_sealed_configuration()
 {
    let f = nested_sample().await;
    let capture = capture_service(&f.setup);
    let before = production_counts(&f.setup).await;
    let first = capture
        .capture(
            target(&f.setup, &f.root),
            DraftRevision::new(1).unwrap(),
            nested_request(&f),
        )
        .await
        .unwrap();
    let old = capture
        .capture(
            target(&f.setup, &f.root),
            DraftRevision::new(1).unwrap(),
            nested_request(&f),
        )
        .await
        .unwrap();
    let report = crate::application::workflow::sample::execute(first).unwrap();
    assert_nested_selection(&f, &report);
    assert_eq!(before, production_counts(&f.setup).await);

    assistant_binding(
        &f.setup,
        f.root_binding.clone(),
        "Changed active instructions",
    )
    .await;
    f.setup.fixture.publish_agent(f.setup.agent).await;
    let later_child = republish_middle(&f).await;
    assert_ne!(later_child, f.middle.version);
    let after_changes = production_counts(&f.setup).await;
    let replay = crate::application::workflow::sample::execute(old).unwrap();
    assert_same_sample(&report, &replay);
    let current = capture_reselected_draft(&f, &capture, later_child).await;
    assert_eq!(
        current.status,
        SampleStatus::Completed,
        "{:?}",
        current.diagnostic
    );
    assert_ne!(current.configuration_hash, report.configuration_hash);
    assert!(
        current
            .workflow_selections
            .iter()
            .any(|w| w.version_id == later_child)
    );
    assert!(
        !current
            .workflow_selections
            .iter()
            .any(|w| w.version_id == f.middle.version)
    );
    assert_eq!(after_changes, production_counts(&f.setup).await);
    capture
        .authorize_release(target(&f.setup, &f.root))
        .await
        .unwrap();
}
