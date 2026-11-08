//! Real native output is durable locally but deliberately unconsumed at restart.
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    Abort, Gate, Stall, connect,
};

async fn mapping(rig: &NativeRig) -> (SessionId, CloudroomThreadId) {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let session: Option<Uuid> = sqlx::query_scalar(
                "SELECT session_id FROM workflow_local_agent_calls WHERE company_id=$1 AND run_id=$2",
            )
            .bind(rig.s.scope.company.as_uuid()).bind(rig.s.scope.run.as_uuid())
            .fetch_optional(rig.s.fixture.persistence.pool()).await.unwrap();
            if let Some(session) = session {
                let session = SessionId::new(session).unwrap();
                let details = rig.s.fixture.persistence.read_session(
                    rig.s.fixture.target.actor.user_id(), rig.s.scope.company.as_uuid(), session,
                ).await.unwrap();
                if let Some(thread) = details.session.thread_id { return (session, thread); }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }).await.unwrap()
}

async fn assert_unconsumed(s: &Setup, operation: OperationId) {
    let outcome: Option<String> = sqlx::query_scalar(
        "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(operation.as_uuid())
    .fetch_one(s.fixture.persistence.pool())
    .await
    .unwrap();
    assert!(
        outcome.is_none(),
        "server output must remain unconsumed until connector restart"
    );
}

pub(super) async fn initial_native(rig: &mut NativeRig) -> NativeInitial {
    let _injected = Injected::mark();
    let s = &rig.s;
    let when = "NEW.command->>'kind'='create' AND NEW.outcome='succeeded'";
    let mut gate = Gate::arm(
        s,
        "local_agent_operations",
        "UPDATE",
        when,
        Stall::Statement {
            table: "local_agent_operations",
            events: "UPDATE",
            when,
        },
    )
    .await;
    let (mut observer, _) = connect(s).await;
    let workers = Workers::start(&s.fixture.persistence);
    state_timeout(s, "waiting", 30).await;
    let (session, thread) = mapping(rig).await;
    let operation: Uuid = sqlx::query_scalar(
        "SELECT operation_id FROM workflow_local_agent_calls WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(s.scope.run.as_uuid())
    .fetch_one(s.fixture.persistence.pool())
    .await
    .unwrap();
    let operation = OperationId::new(operation).unwrap();
    rig.peer
        .command(
            json!({"command":"terminal","thread":thread,"operation":operation,"text":rig.marker}),
        )
        .await;
    let native = rig.peer.event_timeout("terminal", 150).await;
    let backend = gate.stalled(&mut observer, &[]).await;
    assert_unconsumed(s, operation).await;
    state_timeout(s, "waiting", 10).await;
    rig.peer
        .command(json!({"command":"snapshot","thread":thread}))
        .await;
    let before = rig.peer.event("snapshot").await;
    assert_eq!(before["requests"], 1);
    assert!(
        before["turns"]
            .as_array()
            .unwrap()
            .contains(&native["turn"])
    );
    rig.peer.command(json!({"command":"shutdown"})).await;
    rig.peer.event_timeout("shutdown", 30).await;
    workers.close().await;
    gate.abort_on_release(Abort::Backend(backend)).await;
    gate.drain().await;
    super::super::early::disarm_local_gate(s).await;
    rig.server
        .restart(
            Arc::new(PostgresPersistence::new(
                s.fixture.persistence.pool().clone(),
            )),
            crate::adapters::http::routes::local_machines::public_router()
                .with_state(rig.cases.clone()),
        )
        .await;
    assert_unconsumed(s, operation).await;
    rig.peer.command(json!({"command":"boot"})).await;
    rig.peer.event_timeout("booted", 120).await;
    let recovered_workers = Workers::start(&s.fixture.persistence);
    state_timeout(s, "succeeded", 120).await;
    recovered_workers.close().await;
    assert_recovered(rig, session, &thread, &before).await;
    rig.peer
        .command(json!({"command":"snapshot","thread":thread}))
        .await;
    let after = rig.peer.event("snapshot").await;
    assert_eq!(after["requests"], 1);
    assert_eq!(after["turns"], before["turns"]);
    NativeInitial {
        session,
        thread,
        before,
        after,
    }
}

async fn assert_recovered(
    rig: &mut NativeRig,
    session: SessionId,
    thread: &CloudroomThreadId,
    before: &Value,
) {
    let s = &rig.s;
    let details = s
        .fixture
        .persistence
        .read_session(
            s.fixture.target.actor.user_id(),
            s.scope.company.as_uuid(),
            session,
        )
        .await
        .unwrap();
    assert!(details.recent.entries.iter().any(|operation| {
        operation
            .output
            .as_ref()
            .is_some_and(|output| output.as_str().contains(&rig.marker))
    }));
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2 AND result->>'status'='succeeded'",
    ).bind(s.scope.company.as_uuid()).bind(s.scope.run.as_uuid())
        .fetch_one(s.fixture.persistence.pool()).await.unwrap();
    assert_eq!(
        receipts, 1,
        "replayed native output completes the original workflow once"
    );
    assert_eq!(before["requests"], 1);
    rig.peer
        .command(json!({"command":"ui","thread":thread,"text":rig.marker}))
        .await;
    assert_eq!(
        rig.peer.event_timeout("ui", 120).await["thread"]
            .as_str()
            .unwrap(),
        thread.as_str()
    );
}
