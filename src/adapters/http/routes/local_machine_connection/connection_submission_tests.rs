use super::*;

#[tokio::test]
async fn cloudroom_submission_actual_websocket_accepts_thread_before_native_turn() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let mut peer = Peer::initialize(socket(&server, &c).await, &c, 2).await;
    assert!(peer.ready().await);
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    peer.command(operation.id()).await;
    for evidence in [
        LocalEvidence::Receipt {},
        LocalEvidence::SubmissionIntent {},
        LocalEvidence::Submission {
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
            state: SubmissionState::Queued,
        },
    ] {
        peer.observe(agent(peer.sequence + 1, operation.id(), evidence))
            .await;
    }
    let outcome: Option<String> = sqlx::query_scalar(
        "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(
        outcome, None,
        "known queued thread is not a native completion"
    );
    peer.observe(
        crate::adapters::persistence::local_agents::protocol_test_support::turn(
            peer.sequence + 1,
            operation.id(),
            "turn_ws_submission",
            TurnState::Completed,
            Some("matching result"),
        ),
    )
    .await;
    let outcome: String = sqlx::query_scalar(
        "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(outcome, "succeeded");
    server.close().await;
}
