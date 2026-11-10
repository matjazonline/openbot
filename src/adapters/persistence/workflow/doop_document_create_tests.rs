use super::*;

#[tokio::test]
async fn doop_create_input_bounds_and_workspace_override_rejected_before_transport() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    for value in [
        json!({"title":"x","brief":"x","workspace_id":"foreign"}),
        json!({"title":"","brief":"x"}),
        json!({"title":"x".repeat(161),"brief":"x"}),
        json!({"title":"x","brief":"x".repeat(8193)}),
    ] {
        assert!(d.runtime().admit(value).await.is_err());
    }
    assert!(d.server.log().is_empty());
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_create_source_audience_denied_before_any_external_write() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let scope = d.runtime().admit(input()).await.unwrap();
    let mut tx = d.f.persistence.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        scope.company,
        scope.run,
        crate::entities::access_level::AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let view = d.drive(scope, |_, _| false).await;
    assert_eq!(view.detail.summary.state, "failed");
    assert!(
        d.server.log().is_empty(),
        "release authority precedes even profile dispatch"
    );
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_create_bad_receipt_or_read_identity_never_invokes_the_agent() {
    for fault in [
        "empty_id",
        "wrong_workspace",
        "error",
        "wrong_read_id",
        "wrong_read_workspace",
    ] {
        let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
        let mut created = envelope(
            json!({"id":"canvas-a","name":"Synthetic document","url":"/c/canvas-a","workspace_id":"workspace-a"}),
        );
        let mut read = canvas();
        match fault {
            "empty_id" => created["structuredContent"]["id"] = json!(""),
            "wrong_workspace" => created["structuredContent"]["workspace_id"] = json!("foreign"),
            "error" => {
                created =
                    json!({"content":[{"type":"text","text":"Create refused"}],"isError":true})
            }
            "wrong_read_id" => read["structuredContent"]["canvas"]["id"] = json!("foreign"),
            "wrong_read_workspace" => read["structuredContent"]["workspace_id"] = json!("foreign"),
            _ => unreachable!(),
        }
        d.server
            .set_tool_result(tool_name("create_canvas"), created);
        d.server.set_tool_result(tool_name("get_canvas"), read);
        let view = d
            .drive(d.runtime().admit(input()).await.unwrap(), |_, _| false)
            .await;
        assert_eq!(view.detail.summary.state, "failed", "{fault}");
        assert!(
            !view
                .detail
                .executions
                .iter()
                .any(|e| e.step.as_str() == "edit"),
            "{fault}"
        );
        assert_eq!(
            d.server
                .call_names()
                .iter()
                .filter(|n| n.as_str() == "create_canvas")
                .count(),
            1
        );
        d.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn doop_recovery_possible_create_stays_unknown_after_worker_restart_without_reissue() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    d.server.tool_mode(tool_name("create_canvas"), Call::Fail);
    let scope = d.runtime().admit(input()).await.unwrap();
    let unknown = |view: &RunView, _: &[DecisionId]| {
        view.detail.summary.waiting_reason.as_deref() == Some("reconciliation")
    };
    let first = d.drive(scope, unknown).await;
    assert_eq!(first.detail.summary.state, "waiting");
    assert!(
        !first
            .detail
            .executions
            .iter()
            .any(|e| e.step.as_str() == "edit")
    );
    assert_eq!(
        d.server
            .call_names()
            .iter()
            .filter(|n| n.as_str() == "create_canvas")
            .count(),
        1
    );
    d.server.tool_mode(tool_name("create_canvas"), Call::Json);
    let second = d.drive(scope, unknown).await;
    assert_eq!(second.detail.summary.state, "waiting");
    assert_eq!(
        d.server
            .call_names()
            .iter()
            .filter(|n| n.as_str() == "create_canvas")
            .count(),
        1
    );
    assert_eq!(first.detail.actions.len(), second.detail.actions.len());
    d.client.shutdown().await.unwrap();
}
