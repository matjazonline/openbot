use super::*;
use crate::application::workflow::worker::WorkflowHandler;

pub(super) async fn approved_create(d: &Document) -> ClaimedWorkflow {
    let scope = d.runtime().admit(input()).await.unwrap();
    let paused = d.drive(scope, |_, decisions| !decisions.is_empty()).await;
    assert_eq!(paused.detail.summary.state, "waiting");
    assert_eq!(d.server.call_names(), vec!["get_profile"]);
    let decisions = d.runtime().approvals(scope).await;
    assert_eq!(decisions.len(), 1);
    d.runtime().approve(scope, decisions[0]).await;
    let activation = job_scope(&d.f.persistence, scope.company, scope.run, "create").await;
    claim(&d.f.persistence, activation).await
}

#[tokio::test]
async fn doop_recovery_before_entry_failure_keeps_create_unentered_and_safe_to_retry() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let claim = approved_create(&d).await;
    d.server.fail_initialize(true);
    let first = Box::pin(d.engine.handler().execute(&kind(), &claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(code(&first), "mcp.unavailable");
    assert!(!d.server.call_names().contains(&"create_canvas".into()));
    d.server.fail_initialize(false);
    // No remote entry happened; retry the still-owned action after restoring availability.
    let second = Box::pin(d.engine.handler().execute(&kind(), &claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(second["structuredContent"]["id"], "canvas-a");
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

#[tokio::test]
async fn doop_recovery_dropped_create_after_remote_acceptance_parks_unknown_and_never_reissues() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let claim = approved_create(&d).await;
    d.server.tool_mode(tool_name("create_canvas"), Call::Hang);
    {
        // Drop the actual handler future after the remote server has accepted the call,
        // just as a worker whose lease/shutdown interrupts external I/O does.
        let kind = kind();
        let pending = Box::pin(d.engine.handler().execute(&kind, &claim));
        tokio::pin!(pending);
        tokio::select! {
            _ = d.server.state.arrived.notified() => {},
            result = &mut pending => panic!("create must be in flight: {result:?}"),
        }
    }
    d.server.state.release.notify_one();
    d.server.observed("POST notifications/cancelled").await;
    d.server.observed("DELETE").await;
    release(
        &d.f.persistence,
        &claim,
        lease(),
        LeaseReleaseCause::LeaseLost,
    )
    .await;
    let state = retired(&d.f.persistence, &claim).await;
    assert_eq!(
        state,
        Retired::new(
            "waiting",
            Some("reconciliation"),
            "workflow.interrupted",
            "retryable",
            "unknown",
            1
        )
    );
    d.server.tool_mode(tool_name("create_canvas"), Call::Json);
    let failure = Box::pin(d.engine.handler().execute(&kind(), &claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(
        code(&failure),
        "mcp.rejected",
        "the old worker fence is no longer dispatch authority"
    );
    assert_eq!(retired(&d.f.persistence, &claim).await, state);
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

#[tokio::test]
async fn doop_recovery_create_call_timeout_after_acceptance_remains_unknown() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    d.server.tool_mode(tool_name("create_canvas"), Call::Hang);
    let scope = d.runtime().admit(input()).await.unwrap();
    let (view, ()) = tokio::join!(
        d.drive(scope, |view, _| view
            .detail
            .summary
            .waiting_reason
            .as_deref()
            == Some("reconciliation")),
        async {
            tokio::time::timeout(Duration::from_secs(15), d.server.state.arrived.notified())
                .await
                .expect("the intended remote boundary must be reached");
            // Let the real adapter's 30-second call timeout fire before releasing the peer.
            tokio::time::sleep(Duration::from_secs(31)).await;
            d.server.state.release.notify_one();
        },
    );
    assert_eq!(view.detail.summary.state, "waiting");
    assert_eq!(
        view.detail.summary.waiting_reason.as_deref(),
        Some("reconciliation")
    );
    assert_eq!(
        d.server
            .call_names()
            .iter()
            .filter(|n| n.as_str() == "create_canvas")
            .count(),
        1
    );
    assert!(
        !view
            .detail
            .executions
            .iter()
            .any(|e| e.step.as_str() == "edit")
    );
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_recovery_discovery_timeout_before_create_allows_one_safe_entry() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let scope = d.runtime().admit(input()).await.unwrap();
    d.drive(scope, |_, decisions| !decisions.is_empty()).await;
    d.server.hang_list(true);
    let (view, ()) = tokio::join!(
        d.drive(scope, |view, _| view
            .detail
            .executions
            .iter()
            .any(|e| e.step.as_str() == "created")),
        async {
            tokio::time::timeout(Duration::from_secs(15), d.server.state.arrived.notified())
                .await
                .expect("the intended remote boundary must be reached");
            tokio::time::sleep(Duration::from_secs(31)).await;
            assert_eq!(
                d.server.call_names(),
                vec!["get_profile"],
                "pre-entry timeout cannot create"
            );
            d.server.hang_list(false);
            d.server.state.release.notify_one();
        },
    );
    assert!(
        view.detail
            .executions
            .iter()
            .any(|e| e.step.as_str() == "created")
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
