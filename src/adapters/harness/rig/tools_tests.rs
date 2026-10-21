use super::*;
use serde_json::json;

fn correlation() -> ToolCorrelationId {
    ToolCorrelationId::parse("call-1").unwrap()
}

#[tokio::test]
async fn an_ungranted_tool_never_executes_and_leaves_the_bridge_usable() {
    let bridge = ToolBridge::new();
    assert!(bridge.supported_ids().is_empty());
    for id in [
        "calculator",
        "create_agent_channel",
        "outreach_and_await_quorum",
    ] {
        let invocation = bridge
            .invoke(&ToolId::from(id), &correlation(), json!({}))
            .await
            .unwrap();
        assert!(!invocation.success, "{id}");
    }
    assert_eq!(bridge.stop_reason().await, None);
}

#[tokio::test]
async fn the_invocation_budget_latches_the_bridge() {
    let bridge = ToolBridge::new();
    let id = ToolId::from("unknown");
    for _ in 0..MAX_TOOL_INVOCATIONS {
        bridge.invoke(&id, &correlation(), json!({})).await.unwrap();
    }
    assert!(bridge.invoke(&id, &correlation(), json!({})).await.is_err());
    assert_eq!(bridge.stop_reason().await, Some(ToolStopReason::Budget));
    assert!(bridge.invoke(&id, &correlation(), json!({})).await.is_err());
}
