use super::*;

#[tokio::test]
async fn cloudroom_connection_uncertain_reconnect_withholds_new_effect_until_reconciled() {
    let Some(c) = Context::new().await else {
        return;
    };
    let server = Server::start(c.f.persistence.clone()).await;
    let mut first = Peer::initialize(socket(&server, &c).await, &c, 2).await;
    assert!(first.ready().await);
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    assert_eq!(
        first.command(operation.id()).await.mode,
        DeliveryMode::Dispatch
    );
    for evidence in [
        LocalEvidence::Receipt {},
        LocalEvidence::SubmissionIntent {},
        LocalEvidence::Uncertain {},
    ] {
        first
            .observe(agent(first.sequence + 1, operation.id(), evidence))
            .await;
    }
    let sequence = first.sequence;
    let queued =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    drop(first);
    let mut next = Peer::initialize(socket(&server, &c).await, &c, sequence).await;
    assert!(
        !next.ready().await,
        "an ended server wait does not prove native work settled"
    );
    let recovery = next.command(operation.id()).await;
    assert_eq!(recovery.mode, DeliveryMode::Reconcile);
    assert_eq!(
        recovery.operation.outcome(),
        Some(OperationOutcome::Uncertain)
    );
    let attempted: bool = sqlx::query_scalar("SELECT delivery_attempted_at IS NOT NULL FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(queued.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert!(
        !attempted,
        "new effect must remain untouched until current-generation recovery"
    );
    next.observe(
        crate::adapters::persistence::local_agents::protocol_test_support::turn(
            next.sequence + 1,
            operation.id(),
            "turn_ws_recovered",
            TurnState::Completed,
            Some("recovered through actual WebSocket"),
        ),
    )
    .await;
    assert!(next.ready().await);
    assert_eq!(next.command(queued.id()).await.mode, DeliveryMode::Dispatch);
    server.close().await;
}
