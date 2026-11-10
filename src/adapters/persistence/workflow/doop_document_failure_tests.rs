use super::*;
use crate::application::workflow::{CancelResult, CancelWorkflowRequest, RunRevision};
use crate::services::test_support::{
    LlmTurn, ScriptedExchange, ScriptedResponse, register_scripted_agent_base_url, scripted_llm,
    scripted_scenario,
};

#[tokio::test]
async fn doop_recovery_cancelled_before_create_never_becomes_a_draft() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let scope = d.runtime().admit(input()).await.unwrap();
    let waiting = d.drive(scope, |_, decisions| !decisions.is_empty()).await;
    let cancelled = d
        .engine
        .services()
        .service()
        .cancel(CancelWorkflowRequest {
            company_id: scope.company,
            actor: scope.viewer,
            run_id: scope.run,
            expected_revision: RunRevision(waiting.detail.summary.revision),
            command_key: IdempotencyKey::parse("cancel-document").unwrap(),
        })
        .await
        .unwrap();
    assert!(matches!(cancelled, CancelResult::Applied { .. }));
    let view = d.drive(scope, |_, _| false).await;
    assert_eq!(view.detail.summary.state, "cancelled");
    assert_eq!(d.server.call_names(), vec!["get_profile"]);
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_agent_revoked_write_after_approval_never_enters_remote_frame_creation() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    d.server.set_tool_result(tool_name("get_canvas"), canvas());
    let llm = scripted_llm(vec![
        LlmTurn::tool_call("get_canvas", json!({"canvas_id":"canvas-a"})),
        LlmTurn::tool_call("create_frame", json!({"canvas_id":"canvas-a","name":"Draft","html":"<h1>Draft</h1>","agent_name":"Openbot document editor"})),
        LlmTurn::text(json!({"canvas_id":"canvas-a","status":"failed","summary":"Write authority revoked."}).to_string()),
    ]).await;
    register_scripted_agent_base_url(d.agent, &llm.base_url);
    let scope = d.runtime().admit(input()).await.unwrap();
    d.drive(scope, |_, decisions| {
        !decisions.is_empty()
            && d.server
                .call_names()
                .iter()
                .filter(|n| n.as_str() == "get_canvas")
                .count()
                == 2
    })
    .await;
    for decision in d.runtime().approvals(scope).await {
        d.runtime().approve(scope, decision).await;
    }
    let p = &d.f.persistence;
    let current = connection_on(p, scope.company.as_uuid(), d.connection).await;
    let selected = p
        .reviewed_mcp_operations(
            scope.company,
            scope.viewer,
            McpOperationRead {
                connection: d.connection,
                catalog_revision: current.revision,
                selection: McpOperationSelection::Exact(TypeName::parse("create_frame").unwrap()),
            },
        )
        .await
        .unwrap();
    p.revoke_mcp_operation(
        scope.company,
        scope.viewer,
        McpOperationRevoke {
            connection: d.connection,
            catalog_revision: current.revision,
            name: TypeName::parse("create_frame").unwrap(),
            version: selected.operations[0].version,
        },
    )
    .await
    .unwrap();
    let view = d.drive(scope, |_, _| false).await;
    assert_eq!(view.detail.summary.state, "failed");
    assert!(!d.server.call_names().contains(&"create_frame".into()));
    // Revocation can refuse preparation before consuming the scripted continuation.
    let result = llm.finish().await;
    assert!(
        result.is_ok() || result.as_ref().unwrap_err().contains("unused"),
        "{result:?}"
    );
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_agent_incomplete_provider_stream_cannot_complete_or_duplicate_canvas() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let exchanges = (0..3)
        .map(|_| {
            ScriptedExchange::new(
                |_| Ok(()),
                ScriptedResponse {
                    status: 200,
                    headers: vec![("Content-Type".into(), "text/event-stream".into())],
                    body: "data: {\"choices\":[{\"delta\":{\"content\":\"unfinished".into(),
                    disconnect: false,
                    barrier: None,
                },
            )
        })
        .collect();
    let llm = scripted_scenario(exchanges).await;
    register_scripted_agent_base_url(d.agent, &llm.base_url);
    let view = d
        .drive(d.runtime().admit(input()).await.unwrap(), |_, _| false)
        .await;
    assert_eq!(view.detail.summary.state, "failed");
    assert_eq!(
        d.server.call_names(),
        vec!["get_profile", "create_canvas", "get_canvas"]
    );
    assert_eq!(llm.finish().await.unwrap(), 3);
    d.client.shutdown().await.unwrap();
}
