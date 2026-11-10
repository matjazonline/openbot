//! Real workflow-tool completion retains raw child output, omits model bytes.
use super::*;
use crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome;

fn binary_child() -> Value {
    let mut raw: Value =
        serde_json::from_str(include_str!("../../mcp/fixtures/doop/pdf.json")).unwrap();
    raw["isError"] = json!(false);
    raw["content"] = json!(vec![raw["content"][0].clone(); 7]);
    let value = json!({"nested":[raw]});
    assert!(value.to_string().len() > 65_536);
    assert!(value.to_string().len() < 131_072);
    let mut child = echo_child();
    child["output_schema"] = json!({"type":"object","properties":{"nested":{"type":"array"}},"required":["nested"],"additionalProperties":false});
    child["steps"]["answer"]["with"]["value"] = json!({"literal":value});
    child["steps"]["answer"]["with"]["output_schema"] = json!({"literal":child["output_schema"]});
    child
}

#[tokio::test]
async fn doop_binary_child_actual_completion_provider_checkpoint_and_replay() {
    let source = binary_child();
    let raw = source["steps"]["answer"]["with"]["value"]["literal"].clone();
    let bytes = raw["nested"][0]["content"][0]["resource"]["blob"]
        .as_str()
        .unwrap();
    let setup = Box::pin(ToolSetup::new(source)).await;
    let p = setup.persistence();
    let claim = setup.claim("binary-child").await;
    let scope = claim.fence.scope;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"question":"inspect"})),
        LlmTurn::text("\"Visual inspection was not performed\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = setup.runtime();
    assert!(matches!(
        Box::pin(runtime.execute(&kind(), &claim)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let child = setup.finish_child(scope).await;
    let committed: Value =
        sqlx::query_scalar("SELECT committed_output FROM workflow_executions WHERE id=$1")
            .bind(child.execution.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(committed, raw);
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    ));
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let output = Box::pin(runtime.execute(&kind(), &continued))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(output, json!("Visual inspection was not performed"));
    let messages = llm.observed()[1]["messages"].to_string();
    assert!(messages.contains("binaryContentOmitted"));
    assert!(!messages.contains(bytes));
    let saved = saved_result(p, scope).await.to_string();
    assert!(saved.contains("binaryContentOmitted"));
    assert!(!saved.contains(bytes));
    assert_eq!(
        Box::pin(runtime.execute(&kind(), &continued))
            .await
            .completed_result()
            .unwrap(),
        output
    );
    assert_eq!(call_parked(p, scope).await.children, 1);
    assert_eq!(llm.finish().await.unwrap(), 2);
}
