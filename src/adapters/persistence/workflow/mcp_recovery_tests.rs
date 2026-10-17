use super::*;

#[tokio::test]
async fn workflow_mcp_permit_timeout_retires_safely_then_retries_once() {
    use crate::application::services::mcp_client::McpClient;
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(
            setup
                .client
                .connect(&setup.connection.endpoint, Some(SecretString::from(TOKEN)))
                .await
                .unwrap(),
        );
    }
    let first = claimed(&setup).await;
    let failure =
        tokio::time::timeout(Duration::from_secs(7), setup.runtime().run(&kind(), &first))
            .await
            .unwrap()
            .unwrap_err();
    assert_eq!(code(&failure), "mcp.unavailable");
    assert_eq!(setup.server.count("POST initialize"), 4);
    assert_eq!(setup.server.calls(), 0);
    assert_eq!(action_counts(p).await, counts(0, 0, 0));
    assert_eq!(
        retire(p, &first, lease(), failure).await,
        Retired::new("running", None, "mcp.unavailable", "retryable", "safe", 0)
    );
    for session in held {
        session.close().await.unwrap();
    }
    let second = reclaim(p, first.fence.scope).await;
    assert!(setup.runtime().run(&kind(), &second).await.is_ok());
    assert_eq!(setup.server.calls(), 1);
    assert_eq!(action_counts(p).await, counts(1, 1, 1));
    setup.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn workflow_mcp_post_entry_failures_and_session_expiry_park_without_reinitialize() {
    for call in [Call::Fail, Call::Expired] {
        let setup = Box::pin(mcp_setup(call)).await;
        let p = &setup.fixture.persistence;
        let claim = claimed(&setup).await;
        let runtime = setup.runtime();
        let failure = runtime.run(&kind(), &claim).await.unwrap_err();
        assert_eq!(code(&failure), "mcp.unresolved", "{call:?}");
        assert_eq!(setup.server.calls(), 1);
        assert_eq!(setup.server.count("POST initialize"), 1, "no reinitialize");
        assert_eq!(action_counts(p).await, counts(1, 1, 0));
        let seen = setup.server.log().len();
        assert!(runtime.run(&kind(), &claim).await.is_err());
        assert_eq!(setup.server.log().len(), seen);
        assert_eq!(
            retire(p, &claim, lease(), failure).await,
            Retired::new(
                "waiting",
                Some("reconciliation"),
                "mcp.unresolved",
                "retryable",
                "unknown",
                1
            )
        );
    }
}

#[tokio::test]
async fn workflow_mcp_cancellation_and_dropped_worker_cancel_and_close_without_second_call() {
    // Direct tool: explicit cancellation during tools/call.
    let setup = Box::pin(mcp_setup(Call::Hang)).await;
    let claim = claimed(&setup).await;
    let runtime = Arc::new(setup.runtime());
    let cancel = CancellationToken::new();
    let task = {
        let (runtime, cancel, fence) = (runtime.clone(), cancel.clone(), claim.fence);
        tokio::spawn(async move {
            runtime
                .call_tool(fence, tool_call("model-1"), &cancel)
                .await
        })
    };
    setup.server.state.arrived.notified().await;
    cancel.cancel();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    // rmcp serializes transport sends; the notification is queued behind the in-flight
    // POST. Complete that response after local cancellation, then observe cleanup.
    setup.server.state.release.notify_one();
    setup.server.observed("POST notifications/cancelled").await;
    setup.server.observed("DELETE").await;
    // A restarted owner sees the possible dispatch and never calls again.
    let failure = setup.runtime().run(&kind(), &claim).await.unwrap_err();
    assert_eq!(code(&failure), "mcp.unresolved");
    assert_eq!(setup.server.calls(), 1);

    // Step: the worker drops the handler future (lease loss) during tools/call.
    let setup = Box::pin(mcp_setup(Call::Hang)).await;
    let p = &setup.fixture.persistence;
    let claim = claimed(&setup).await;
    let runtime = Arc::new(setup.runtime());
    let task = {
        let (runtime, claim) = (runtime.clone(), claim.clone());
        tokio::spawn(async move { runtime.run(&kind(), &claim).await })
    };
    setup.server.state.arrived.notified().await;
    task.abort();
    assert!(task.await.is_err());
    setup.server.state.release.notify_one();
    setup.server.observed("POST notifications/cancelled").await;
    setup.server.observed("DELETE").await;
    release(p, &claim, lease(), LeaseReleaseCause::LeaseLost).await;
    assert_eq!(
        retired(p, &claim).await,
        Retired::new(
            "waiting",
            Some("reconciliation"),
            "workflow.interrupted",
            "retryable",
            "unknown",
            1
        )
    );
    assert_eq!(setup.server.calls(), 1);
}

#[tokio::test]
async fn workflow_mcp_stale_worker_during_eager_open_retries_then_calls_once() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    setup.server.hang_list(true);
    let scope = admitted(&setup, "mcp-stale").await;
    let first = claim(p, scope).await;
    let runtime = Arc::new(setup.runtime());
    let task = {
        let (runtime, first) = (runtime.clone(), first.clone());
        tokio::spawn(async move { runtime.run(&kind(), &first).await })
    };
    setup.server.state.arrived.notified().await;
    task.abort();
    assert!(task.await.is_err());
    setup.server.hang_list(false);
    setup.server.state.release.notify_one();
    assert_eq!(action_counts(p).await, counts(0, 0, 0));
    release(p, &first, lease(), LeaseReleaseCause::LeaseLost).await;
    assert_eq!(
        retired(p, &first).await,
        Retired::new(
            "running",
            None,
            "workflow.interrupted",
            "retryable",
            "safe",
            0
        )
    );
    let second = reclaim(p, scope).await;
    assert!(runtime.run(&kind(), &second).await.is_ok());
    assert_eq!(setup.server.calls(), 1);
}

#[tokio::test]
async fn workflow_mcp_revocation_and_rotation_racing_dispatch_never_call() {
    // Revoker vs dispatcher: the approval is revoked while discovery is in flight.
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    setup.server.hang_list(true);
    let scope = admitted(&setup, "mcp-revoke").await;
    let first = claim(p, scope).await;
    let runtime = Arc::new(setup.runtime());
    let task = {
        let (runtime, first) = (runtime.clone(), first.clone());
        tokio::spawn(async move { runtime.run(&kind(), &first).await })
    };
    setup.server.state.arrived.notified().await;
    let current = setup.current().await;
    p.revoke_mcp_operation(
        setup.company(),
        setup.actor(),
        McpOperationRevoke {
            connection: current.id,
            catalog_revision: current.revision,
            name: TypeName::parse(TOOL).unwrap(),
            version: setup.operation,
        },
    )
    .await
    .unwrap();
    setup.server.state.release.notify_one();
    let failure = task.await.unwrap().unwrap_err();
    assert_eq!(
        code(&failure),
        "mcp.unresolved",
        "reservation refused: retried once"
    );
    assert_eq!(setup.server.calls(), 0);
    assert_eq!(action_counts(p).await, counts(0, 0, 0));
    assert_eq!(
        retire(p, &first, lease(), failure).await,
        Retired::new("running", None, "mcp.unresolved", "retryable", "safe", 0)
    );
    setup.server.hang_list(false);
    let second = reclaim(p, scope).await;
    let failure = runtime.run(&kind(), &second).await.unwrap_err();
    assert_eq!(
        code(&failure),
        "mcp.rejected",
        "the next preparation refuses"
    );
    assert_eq!(setup.server.calls(), 0);

    // Credential rotation after the session opened: abort before sending, unknown marker.
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    setup.server.hang_list(true);
    let claim = claimed(&setup).await;
    let runtime = Arc::new(setup.runtime());
    let task = {
        let (runtime, claim) = (runtime.clone(), claim.clone());
        tokio::spawn(async move { runtime.run(&kind(), &claim).await })
    };
    setup.server.state.arrived.notified().await;
    let current = setup.current().await;
    p.replace_mcp_token(
        setup.company().as_uuid(),
        current.id,
        current.revision,
        Some(SecretString::from("rotated-mcp-secret")),
    )
    .await
    .unwrap();
    setup.server.state.release.notify_one();
    let failure = task.await.unwrap().unwrap_err();
    assert_eq!(code(&failure), "mcp.unresolved");
    assert_eq!(setup.server.calls(), 0, "rotated credential: nothing sent");
    assert_eq!(action_counts(p).await, counts(1, 1, 0));
}
