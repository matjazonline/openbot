//! Genuine Doop carrier shapes through dispatch, durable checkpoint and provider.
use super::*;

pub(super) fn captured(pdf: bool) -> Value {
    let mut value: Value = serde_json::from_str(if pdf {
        include_str!("../../mcp/fixtures/doop/pdf.json")
    } else {
        include_str!("../../mcp/fixtures/doop/screenshot.json")
    })
    .unwrap();
    // The fixture operation has a customer schema; retain the genuine binary
    // carrier, and omit optional vendor-specific structured output.
    value.as_object_mut().unwrap().remove("structuredContent");
    value["isError"] = json!(false);
    value
}

fn binary(value: &Value, pdf: bool) -> &str {
    if pdf {
        value["content"][0]["resource"]["blob"].as_str().unwrap()
    } else {
        value["content"][0]["data"].as_str().unwrap()
    }
}

#[tokio::test]
async fn doop_binary_agent_actual_provider_checkpoint_raw_receipt_and_replay() {
    for pdf in [false, true] {
        let raw = captured(pdf);
        let server = mcp_server(Call::Json).await;
        server.set_result(raw.clone());
        let (fixture, agent) = Box::pin(published_mcp_agent(&server)).await;
        let p = &fixture.persistence;
        let claim = Box::pin(claim_without_slots(&fixture)).await;
        let mut llm = scripted_llm(vec![
            LlmTurn::tool_call(TOOL, json!({"customer_id":"c-42"})),
            LlmTurn::text("\"Visual inspection was not performed\""),
        ])
        .await;
        register_scripted_agent_base_url(agent.as_uuid(), &llm.base_url);
        let runtime = mcp_agent_runtime(
            p,
            Arc::new(McpRuntime::new(
                p.clone(),
                guarded_client(&server.url),
                lease(),
            )),
        );
        let kind = WorkflowStepKind("agent.run".into());
        let output = Box::pin(runtime.execute(&kind, &claim))
            .await
            .completed_result()
            .unwrap();
        let messages = llm.observed()[1]["messages"].to_string();
        assert!(messages.contains("binaryContentOmitted"));
        assert!(messages.contains("Visual inspection was not performed"));
        assert!(!messages.contains(binary(&raw, pdf)));
        let checkpoint: Value = sqlx::query_scalar(
            "SELECT to_jsonb(checkpoint) FROM task_harness_runs AS checkpoint WHERE company_id=$1",
        )
        .bind(fixture.target.company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert!(checkpoint.to_string().contains("binaryContentOmitted"));
        assert!(!checkpoint.to_string().contains(binary(&raw, pdf)));
        let receipt: Value = sqlx::query_scalar("SELECT result FROM workflow_action_receipts")
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(receipt, raw, "authoritative bounded carrier remains intact");
        assert_eq!(
            Box::pin(runtime.execute(&kind, &claim))
                .await
                .completed_result()
                .unwrap(),
            output
        );
        assert_eq!(server.calls(), 1);
        assert_eq!(llm.finish().await.unwrap(), 2);
    }
}

#[tokio::test]
async fn doop_binary_raw_result_bound_preserves_unknown_and_no_reissue() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let mut raw = captured(true);
    raw["content"] = json!(vec![raw["content"][0].clone(); 7]);
    assert!(raw.to_string().len() > 65_536);
    setup.server.set_result(raw);
    let claim = claimed(&setup).await;
    let runtime = setup.runtime();
    for _ in 0..2 {
        let failure = runtime
            .run(&kind(), &claim)
            .await
            .completed_result()
            .unwrap_err();
        assert_eq!(code(&failure), "mcp.unresolved");
    }
    assert_eq!(setup.server.calls(), 1);
    assert_eq!(
        action_counts(&setup.fixture.persistence).await,
        counts(1, 1, 0)
    );
}
