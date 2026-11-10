//! Early cuts use the actual admission transaction and command-write boundary.
use super::*;
use crate::adapters::http::routes::local_machine_connection::tests::CommandWriteGate;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    Abort, Gate, Stall, connect,
};

pub(super) async fn disarm_local_gate(s: &Setup) {
    sqlx::raw_sql("DROP TRIGGER aa_lease_stall ON local_agent_operations; DROP TRIGGER lease_gate_arm ON local_agent_operations; DROP FUNCTION lease_gate(); DROP FUNCTION lease_gate_uncancellable(); DROP TABLE lease_gate_abort")
        .execute(s.fixture.persistence.pool()).await.unwrap();
}

pub(super) async fn terminal(s: &Setup) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            let value: String =
                sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
                    .bind(s.scope.company.as_uuid())
                    .bind(s.scope.run.as_uuid())
                    .fetch_one(s.fixture.persistence.pool())
                    .await
                    .unwrap();
            if matches!(value.as_str(), "succeeded" | "failed" | "cancelled") {
                return value;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}

async fn assert_reconstructed(s: &Setup, peer: &mut Peer) -> String {
    let workers = Workers::start(&s.fixture.persistence);
    let result = terminal(s).await;
    workers.close().await;
    let snapshot = peer.snapshot().await;
    let calls = snapshot["calls"].as_array().unwrap();
    assert!(
        calls.len() <= 1,
        "recovery cannot blindly repeat a native mutation"
    );
    assert_eq!(calls.len(), usize::from(result == "succeeded"));
    let receipts: Vec<Value> = sqlx::query_scalar(
        "SELECT result FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(s.scope.run.as_uuid())
    .fetch_all(s.fixture.persistence.pool())
    .await
    .unwrap();
    assert!(
        receipts.len() <= 1,
        "a recovered invocation publishes at most once"
    );
    if result == "succeeded" {
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["status"], "succeeded");
    } else {
        assert!(
            receipts
                .iter()
                .all(|receipt| receipt["status"] != "succeeded")
        );
    }
    result
}

#[tokio::test]
async fn cloudroom_e2e_crash_before_admission_commit_reconstructs_workers_server_and_peer() {
    let _injected = Injected::mark();
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let mut gate = Gate::arm(
        &s,
        "local_agent_operations",
        "INSERT",
        "NEW.command->>'kind' = 'create'",
        Stall::Statement {
            table: "local_agent_operations",
            events: "INSERT",
            when: "NEW.command->>'kind' = 'create'",
        },
    )
    .await;
    let (mut observer, _) = connect(&s).await;
    let mut server = TestTlsServer::start(Arc::new(p.clone())).await;
    let mut peer = Peer::new(&s, &server, "none");
    peer.event("started").await;
    let workers = Workers::start(p);
    let backend = gate.stalled(&mut observer, &[]).await;
    assert_eq!(peer.snapshot().await["calls"].as_array().unwrap().len(), 0);
    workers.crash().await;
    gate.abort_on_release(Abort::Backend(backend)).await;
    gate.drain().await;
    disarm_local_gate(&s).await;
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND command->>'kind'='create'",
    ).bind(s.scope.company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        rows, 0,
        "the interrupted admission did not commit an operation"
    );
    peer.kill().await;
    server
        .restart(
            Arc::new(PostgresPersistence::new(p.pool().clone())),
            axum::Router::new(),
        )
        .await;
    peer.reopen();
    peer.event("started").await;
    assert_reconstructed(&s, &mut peer).await;
    peer.close().await;
    server.close().await;
}

#[tokio::test]
async fn cloudroom_e2e_crash_after_admission_before_peer_connect_dispatches_once() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    parked(&s).await;
    let mut server = TestTlsServer::start(Arc::new(p.clone())).await;
    let mut peer = Peer::new(&s, &server, "after_admission");
    peer.crashed("after_admission").await;
    let marker: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT delivery_attempted_at FROM local_agent_operations WHERE company_id=$1 AND command->>'kind'='create'",
    ).bind(s.scope.company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(
        marker.is_none(),
        "no connection existed to attempt the admitted command"
    );
    server
        .restart(
            Arc::new(PostgresPersistence::new(p.pool().clone())),
            axum::Router::new(),
        )
        .await;
    peer.reopen();
    peer.event("started").await;
    assert_eq!(assert_reconstructed(&s, &mut peer).await, "succeeded");
    peer.close().await;
    server.close().await;
}

#[tokio::test]
async fn cloudroom_e2e_committed_delivery_marker_before_socket_never_blindly_dispatches() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    parked(&s).await;
    let id: Uuid = sqlx::query_scalar(
        "SELECT operation_id FROM workflow_local_agent_calls WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(s.scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let gate = Arc::new(CommandWriteGate {
        operation: OperationId::new(id).unwrap(),
        reached: Notify::new(),
    });
    let mut server =
        TestTlsServer::with_gate(Arc::new(p.clone()), axum::Router::new(), Some(gate.clone()))
            .await;
    let mut peer = Peer::new(&s, &server, "none");
    peer.event("started").await;
    tokio::time::timeout(std::time::Duration::from_secs(15), gate.reached.notified())
        .await
        .unwrap();
    let marked: bool = sqlx::query_scalar(
        "SELECT delivery_attempted_at IS NOT NULL FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    ).bind(s.scope.company.as_uuid()).bind(id).fetch_one(p.pool()).await.unwrap();
    assert!(
        marked,
        "the real first-delivery transaction committed before the socket cut"
    );
    assert_eq!(peer.snapshot().await["calls"].as_array().unwrap().len(), 0);
    peer.kill().await;
    server
        .restart(
            Arc::new(PostgresPersistence::new(p.pool().clone())),
            axum::Router::new(),
        )
        .await;
    peer.reopen();
    peer.event("started").await;
    assert_ne!(assert_reconstructed(&s, &mut peer).await, "succeeded");
    assert_eq!(peer.snapshot().await["calls"].as_array().unwrap().len(), 0);
    peer.close().await;
    server.close().await;
}
