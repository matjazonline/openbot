use super::*;
use crate::adapters::persistence::workflow::knowledge;
use crate::entities::access_level::AccessLevel;
use crate::use_cases::connection_level::ConnectionLevelCommand;

#[tokio::test]
async fn knowledge_run_level_http_step_and_direct_tool_refuse_before_network() {
    for entrance in [Entrance::Http, Entrance::Tool] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let setup = Box::pin(setup(&origin, entrance)).await;
        let p = &setup.fixture.persistence;
        let scope = setup.claim.fence.scope;
        let mut tx = p.pool().begin().await.unwrap();
        knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, AccessLevel::L5)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let failure = runtime(&setup)
            .run(&setup.kind, &setup.claim)
            .await
            .completed_result()
            .unwrap_err();
        assert_eq!(
            failure.failure.code().as_str(),
            crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT
        );
        assert_eq!(failure.failure.class(), FailureClass::Terminal);
        assert_eq!(
            action_counts(&setup.fixture).await,
            ActionCounts {
                dispatches: 0,
                entries: 0,
                receipts: 0
            }
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(40), listener.accept())
                .await
                .is_err()
        );
    }
}
#[tokio::test]
async fn knowledge_run_level_http_current_connection_level_allows_classified_dispatch() {
    let (origin, server) =
        server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let setup = Box::pin(setup(&origin, Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    p.set_http_level(ConnectionLevelCommand {
        company: scope.company,
        actor: setup.fixture.target.actor,
        connection: setup.connection.id,
        expected_revision: setup.connection.revision,
        access_level: AccessLevel::L5,
    })
    .await
    .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, AccessLevel::L5)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        runtime(&setup)
            .run(&setup.kind, &setup.claim)
            .await
            .completed_result()
            .is_ok()
    );
    server.await.unwrap();
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 1
        }
    );
}

#[tokio::test]
async fn knowledge_run_level_remote_possible_marker_stays_uncertain_after_taint() {
    let (origin, server) = server("HTTP/1.1 500 Error\r\nContent-Length: 0\r\n\r\n").await;
    let setup = Box::pin(setup(&origin, Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let runtime = runtime(&setup);
    assert!(
        runtime
            .run(&setup.kind, &setup.claim)
            .await
            .completed_result()
            .is_err()
    );
    server.await.unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, AccessLevel::L5)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let failure = runtime
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "http.unresolved");
    assert_eq!(failure.failure.class(), FailureClass::Retryable);
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 0
        }
    );
    assert_eq!(
        retire(p, &setup.claim, lease(), failure).await,
        Retired::new(
            "waiting",
            Some("reconciliation"),
            "http.unresolved",
            "retryable",
            "unknown",
            1
        )
    );
}
#[tokio::test]
async fn knowledge_run_level_completed_remote_receipt_replays_after_taint_without_reentry() {
    let (origin, server) =
        server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let setup = Box::pin(setup(&origin, Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let runtime = runtime(&setup);
    let original = runtime
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap();
    server.await.unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, AccessLevel::L5)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        runtime
            .run(&setup.kind, &setup.claim)
            .await
            .completed_result()
            .unwrap(),
        original
    );
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 1
        }
    );
}
