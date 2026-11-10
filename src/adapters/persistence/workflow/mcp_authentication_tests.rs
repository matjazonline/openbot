use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn doop_connection_authentication_before_entry_retires_terminal_without_any_effect() {
    for method in ["initialize", "tools/list"] {
        for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
            let setup = Box::pin(mcp_setup(Call::Json)).await;
            let p = &setup.fixture.persistence;
            setup.server.refuse(method, status);
            let claim = claimed(&setup).await;
            let failure = setup
                .runtime()
                .run(&kind(), &claim)
                .await
                .completed_result()
                .unwrap_err();
            assert_eq!(code(&failure), "mcp.authentication_rejected");
            assert!(!format!("{failure:?}").contains(TOKEN));
            assert!(!format!("{failure:?}").contains("remote-private-diagnostic"));
            assert_eq!(setup.server.calls(), 0);
            assert_eq!(action_counts(p).await, counts(0, 0, 0));
            assert_eq!(
                retire(p, &claim, lease(), failure).await,
                Retired::new(
                    "failed",
                    None,
                    "mcp.authentication_rejected",
                    "terminal",
                    "safe",
                    0
                )
            );
        }
    }
}

#[tokio::test]
async fn doop_connection_authentication_after_entry_stays_unknown_and_never_reissues() {
    for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
        let setup = Box::pin(mcp_setup(Call::Json)).await;
        let p = &setup.fixture.persistence;
        setup.server.refuse("tools/call", status);
        let claim = claimed(&setup).await;
        let runtime = setup.runtime();
        let failure = runtime
            .run(&kind(), &claim)
            .await
            .completed_result()
            .unwrap_err();
        assert_eq!(code(&failure), "mcp.unresolved");
        assert_eq!(setup.server.calls(), 1);
        assert_eq!(action_counts(p).await, counts(1, 1, 0));
        let seen = setup.server.log().len();
        assert!(
            runtime
                .run(&kind(), &claim)
                .await
                .completed_result()
                .is_err()
        );
        assert_eq!(
            setup.server.log().len(),
            seen,
            "possible effect must not reopen the provider"
        );
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
