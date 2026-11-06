use super::*;

fn root_response(workflow: WorkflowId) -> Value {
    json!({"workflow_id":workflow,"step":"start","responses":[{"output":"done"}]})
}

fn dynamic_source(candidates: &[String]) -> Value {
    let mut source = model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun);
    source["parameter_schema"] = json!({"type":"object","properties":{
        "selected":{"type":"string","enum":candidates}},"required":["selected"],"additionalProperties":false});
    source["steps"]["start"]["with"]["agent"] = json!({"ref":"/params/selected"});
    source
}

#[tokio::test]
async fn autonomous_agent_samples_message_steps_keep_metadata_and_generic_message_tools_stay_forbidden()
 {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let result = json!({"status":"accepted","messages":[{
        "destination":{"kind":"reply","id":Uuid::from_u128(700)},
        "message_id":Uuid::from_u128(701),"status":"accepted"}]});
    for kind in ["message.reply", "tool.call"] {
        let mut source: Value =
            serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
        if kind == "tool.call" {
            source["steps"]["start"]["with"]["tool"] = json!({"literal":"message.reply"});
            source["steps"]["start"]["with"]["arguments"] = json!({"literal":{
                "content":{"body":"hello"},"source_message":Uuid::from_u128(5)}});
        }
        let target = DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..setup.fixture.target
        };
        save_source(&setup, target, None, source).await;
        let request = sample_request(vec![json!({"workflow_id":target.workflow,
            "step":"start","responses":[{"output":result}]})]);
        let before = production_counts(&setup).await;
        let captured = capture_service(&setup)
            .capture(target, DraftRevision::new(1).unwrap(), request)
            .await;
        if kind == "tool.call" {
            let error = captured
                .err()
                .expect("messaging remains a STEP, not a generic selectable tool");
            assert!(
                error.to_string().contains("not a selectable tool"),
                "{error}"
            );
            assert_eq!(before, production_counts(&setup).await);
            continue;
        }
        let frozen = captured.unwrap();
        let report = crate::application::workflow::sample::execute(frozen).unwrap();
        assert_eq!(
            report.status,
            SampleStatus::Completed,
            "{kind}: {:?}",
            report.diagnostic
        );
        assert_eq!(report.effects.len(), 1);
        assert!(report.agent_selections.is_empty());
        assert_eq!(before, production_counts(&setup).await);
    }
}

async fn save_source(
    setup: &AgentSetup,
    target: DefinitionTarget,
    expected: Option<DraftRevision>,
    mut source: Value,
) {
    let p = &setup.fixture.persistence;
    let fixture = Fixture {
        _db: setup.fixture._db.clone(),
        persistence: p.clone(),
        target,
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
    source["workflow_id"] = json!(target.workflow);
    let mut draft = fixture.save(expected);
    draft.content.source = source.to_string();
    service.save(draft).await.unwrap();
}

#[tokio::test]
async fn autonomous_agent_samples_draft_cycle_is_refused_without_activation_or_production_effects()
{
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let root = publish_graph(
        &setup,
        model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun),
        json!({}),
    )
    .await;
    let cycle = register_child(&setup, &root, "sample_cycle").await;
    // The active agent still has its old HTTP definition; only its draft is cyclic.
    assistant_binding(&setup, cycle, "Cyclic draft").await;
    let before = production_counts(&setup).await;
    let error = capture_service(&setup)
        .capture(
            target(&setup, &root),
            DraftRevision::new(1).unwrap(),
            sample_request(vec![]),
        )
        .await
        .err()
        .expect("the draft-only closed graph must reject a cycle");
    assert!(error.to_string().contains("cycle"), "{error}");
    assert_eq!(before, production_counts(&setup).await);
}

#[tokio::test]
async fn autonomous_agent_samples_combined_capture_depth_accepts_32_and_refuses_34() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let mut graph = publish_graph(
        &setup,
        model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun),
        json!({}),
    )
    .await;
    for level in 1..=16 {
        let binding = register_child(&setup, &graph, &format!("sample_depth_{level}")).await;
        let key = AgentKey::parse(format!("sample-depth-{level}")).unwrap();
        let agent = draft_agent(&setup, &key, "Depth capture".into(), vec![binding]).await;
        setup.fixture.publish_agent(agent.as_uuid()).await;
        graph = publish_graph(&setup, model_source(&key, ModelStep::AgentRun), json!({})).await;
        if level == 15 {
            let before = production_counts(&setup).await;
            let sealed = capture_service(&setup)
                .capture(
                    target(&setup, &graph),
                    DraftRevision::new(1).unwrap(),
                    sample_request(vec![root_response(graph.workflow)]),
                )
                .await
                .unwrap();
            let report = crate::application::workflow::sample::execute(sealed).unwrap();
            assert_eq!(
                report.status,
                SampleStatus::Completed,
                "{:?}",
                report.diagnostic
            );
            assert_eq!(report.agent_selections.len(), 16);
            assert_eq!(report.workflow_selections.len(), 16);
            assert!(report.effects.is_empty());
            assert_eq!(before, production_counts(&setup).await);
        }
    }
    let before = production_counts(&setup).await;
    let error = capture_service(&setup)
        .capture(
            target(&setup, &graph),
            DraftRevision::new(1).unwrap(),
            sample_request(vec![]),
        )
        .await
        .err()
        .expect("depth34 must be rejected during capture");
    assert!(error.to_string().contains("depth exceeds 32"), "{error}");
    assert_eq!(before, production_counts(&setup).await);
}

#[tokio::test]
async fn autonomous_agent_samples_unique_draft_selections_accept_256_and_refuse_257() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let mut candidates = vec!["assistant".to_owned()];
    for index in 1..255 {
        let key = AgentKey::parse(format!("sample-candidate-{index}")).unwrap();
        draft_agent(&setup, &key, "Draft candidate".into(), vec![]).await;
        candidates.push(key.as_str().to_owned());
    }
    let target = DefinitionTarget {
        workflow: WorkflowId::new(Uuid::new_v4()),
        ..setup.fixture.target
    };
    save_source(&setup, target, None, dynamic_source(&candidates)).await;
    let mut request = sample_request(vec![root_response(target.workflow)]);
    request.params = json!({"selected":"assistant"});
    let before = production_counts(&setup).await;
    let capture = capture_service(&setup);
    let sealed = capture
        .capture(target, DraftRevision::new(1).unwrap(), request.clone())
        .await
        .unwrap();
    let report = crate::application::workflow::sample::execute(sealed).unwrap();
    assert_eq!(
        report.status,
        SampleStatus::Completed,
        "{:?}",
        report.diagnostic
    );
    assert_eq!(report.agent_selections.len(), 255);
    assert_eq!(report.workflow_selections.len(), 1);
    assert!(report.effects.is_empty());
    assert_eq!(before, production_counts(&setup).await);
    let last = AgentKey::parse("sample-candidate-255").unwrap();
    draft_agent(&setup, &last, "Last draft".into(), vec![]).await;
    candidates.push(last.as_str().to_owned());
    save_source(
        &setup,
        target,
        Some(DraftRevision::new(1).unwrap()),
        dynamic_source(&candidates),
    )
    .await;
    let before = production_counts(&setup).await;
    let error = capture
        .capture(target, DraftRevision::new(2).unwrap(), request)
        .await
        .err()
        .expect("the complete256candidate universe plusroot must exceed capacity");
    assert!(
        error
            .to_string()
            .contains("Too many sample dependency selections"),
        "{error}"
    );
    assert_eq!(before, production_counts(&setup).await);
}

#[tokio::test]
async fn autonomous_agent_samples_aggregate_draft_bytes_are_bounded_without_publication() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let mut candidates = vec![];
    for index in 0..16 {
        let key = AgentKey::parse(format!("sample-large-{index}")).unwrap();
        draft_agent(&setup, &key, "x".repeat(15 * 1024), vec![]).await;
        candidates.push(key.as_str().to_owned());
    }
    let target = DefinitionTarget {
        workflow: WorkflowId::new(Uuid::new_v4()),
        ..setup.fixture.target
    };
    save_source(&setup, target, None, dynamic_source(&candidates)).await;
    let mut request = sample_request(vec![root_response(target.workflow)]);
    request.params = json!({"selected":candidates[0]});
    let capture = capture_service(&setup);
    let before = production_counts(&setup).await;
    let sealed = capture
        .capture(target, DraftRevision::new(1).unwrap(), request.clone())
        .await
        .unwrap();
    let report = crate::application::workflow::sample::execute(sealed).unwrap();
    assert_eq!(
        report.status,
        SampleStatus::Completed,
        "{:?}",
        report.diagnostic
    );
    assert_eq!(report.agent_selections.len(), 16);
    assert_eq!(before, production_counts(&setup).await);
    let last = AgentKey::parse("sample-large-16").unwrap();
    draft_agent(&setup, &last, "x".repeat(15 * 1024), vec![]).await;
    candidates.push(last.as_str().to_owned());
    save_source(
        &setup,
        target,
        Some(DraftRevision::new(1).unwrap()),
        dynamic_source(&candidates),
    )
    .await;
    let before = production_counts(&setup).await;
    let error = capture
        .capture(target, DraftRevision::new(2).unwrap(), request)
        .await
        .err()
        .expect("aggregatecanonicaldraftbytes must be limited before execution");
    assert!(error.to_string().contains("256 KiB"), "{error}");
    assert_eq!(before, production_counts(&setup).await);
}
