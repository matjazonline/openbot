use super::*;

async fn history_snapshot(setup: &AgentSetup) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('sessions',(SELECT count(*) FROM agent_history_sessions WHERE company_id=$1),'mappings',(SELECT count(*) FROM workflow_agent_sessions WHERE company_id=$1),'conversations',(SELECT count(*) FROM agent_history_conversations WHERE company_id=$1),'exchanges',(SELECT jsonb_agg(jsonb_build_object('id',id,'state',state,'output',output,'preparation',frozen_preparation) ORDER BY id) FROM agent_history_exchanges WHERE company_id=$1))")
        .bind(setup.fixture.target.company.as_uuid()).fetch_one(setup.fixture.persistence.pool()).await.unwrap()
}

#[tokio::test]
async fn agent_history_samples_sealed_capture_cannot_load_production_history_or_advance_turns() {
    let setup = Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        None,
        false,
        |source, _| {
            source["input_schema"] = json!({"type":"object"});
            source["steps"]["start"]["with"]["context"] = json!({"ref":"/input"});
        },
    ))
    .await;
    let mut llm = scripted_llm(vec![LlmTurn::text("\"PRODUCTION PRIVATE ANSWER\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    assert_eq!(
        Box::pin(
            agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim)
        )
        .await
        .completed_result()
        .unwrap(),
        json!("PRODUCTION PRIVATE ANSWER")
    );
    let counts = production_counts(&setup).await;
    let before = history_snapshot(&setup).await;
    assert_eq!(before["exchanges"].as_array().unwrap().len(), 1);
    assert_eq!(
        before["exchanges"][0]["output"],
        "PRODUCTION PRIVATE ANSWER"
    );
    let make_request = || {
        let mut request = sample_request(vec![json!({"workflow_id":setup.fixture.target.workflow,
            "step":"start","responses":[{"output":"sample accepted"}]})]);
        request.input = json!({"history_session":{"from_run":setup.claim.fence.scope.run},
            "history":[{"id":before["exchanges"][0]["id"]}]});
        request
    };
    for _ in 0..2 {
        let captured = capture_service(&setup)
            .capture(
                setup.fixture.target,
                DraftRevision::new(1).unwrap(),
                make_request(),
            )
            .await
            .unwrap();
        let report = crate::application::workflow::sample::execute(captured).unwrap();
        assert_eq!(
            report.status,
            crate::application::workflow::sample::SampleStatus::Completed,
            "{report:?}"
        );
        assert_eq!(report.output, Some(json!("sample accepted")));
        let history = report.steps[0].history.as_ref().unwrap();
        assert!(history.seed.pairs().is_empty());
        assert_eq!(history.omitted_pairs, 0);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("PRODUCTION PRIVATE ANSWER")
        );
        assert_eq!(production_counts(&setup).await, counts);
        assert_eq!(history_snapshot(&setup).await, before);
    }
    assert_eq!(llm.observed().len(), 1);
    assert_eq!(llm.finish().await.unwrap(), 1);
}
