use super::{test_support::*, *};

#[path = "observation_failure_tests.rs"]
mod failures;

#[tokio::test]
async fn cloudroom_observations_lost_ack_replay_uses_current_fence_and_same_payload() {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    let receipt = agent(3, operation.id(), LocalEvidence::Receipt {});
    assert_eq!(
        c.f.persistence
            .accept_observation(c.fence, &receipt)
            .await
            .unwrap(),
        3
    );
    let (next, welcome) = c.reconnect(c.journal, 3).await;
    assert!(!welcome.recovery_required);
    assert_eq!(welcome.acknowledged_sequence, 3);
    assert!(!c.f.persistence.finish_reconciliation(next).await.unwrap());
    assert!(matches!(
        c.f.persistence.accept_observation(c.fence, &receipt).await,
        Err(AppError::InvalidCredentials)
    ));
    assert_eq!(
        c.f.persistence
            .accept_observation(next, &receipt)
            .await
            .unwrap(),
        3
    );
    assert!(c.f.persistence.finish_reconciliation(next).await.unwrap());
    let changed = agent(3, operation.id(), LocalEvidence::Uncertain {});
    assert!(matches!(
        c.f.persistence.accept_observation(next, &changed).await,
        Err(AppError::Conflict(_))
    ));
    let gap = agent(5, operation.id(), LocalEvidence::SubmissionIntent {});
    assert!(matches!(
        c.f.persistence.accept_observation(next, &gap).await,
        Err(AppError::Conflict(_))
    ));
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_agent_observations WHERE company_id=$1 AND sequence=3",
    )
    .bind(c.f.company)
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(rows, 1);
    let cursor: i64 = sqlx::query_scalar(
        "SELECT acknowledged_sequence FROM local_machines WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(c.paired.receipt.machine.id.as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(cursor, 3);
}

#[tokio::test]
async fn cloudroom_observations_lost_journal_or_cursor_cannot_release_recovery_barrier() {
    let Some(c) = Context::new().await else {
        return;
    };
    let (lost, welcome) = c.reconnect(Uuid::new_v4(), 2).await;
    assert!(welcome.recovery_required);
    assert!(matches!(
        c.f.persistence.finish_reconciliation(lost).await,
        Err(AppError::InvalidCredentials)
    ));
    assert!(matches!(
        c.f.persistence.pending_commands(lost).await,
        Err(AppError::InvalidCredentials)
    ));
    let (behind, welcome) = c.reconnect(c.journal, 1).await;
    assert!(welcome.recovery_required);
    assert!(matches!(
        c.f.persistence.finish_reconciliation(behind).await,
        Err(AppError::InvalidCredentials)
    ));
    let (recovered, welcome) = c.reconnect(c.journal, 2).await;
    assert!(!welcome.recovery_required);
    assert!(
        c.f.persistence
            .finish_reconciliation(recovered)
            .await
            .unwrap()
    );
    c.f.persistence.release_connection(c.fence).await.unwrap();
    assert!(c.f.persistence.renew_connection(recovered).await.unwrap());
}

#[tokio::test]
async fn cloudroom_observations_matching_turn_result_is_durable_and_human_turn_cannot_replace_it() {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    assert!(
        c.f.persistence
            .accept_observation(
                c.fence,
                &turn(
                    3,
                    operation.id(),
                    "turn_owned",
                    TurnState::Completed,
                    Some("too early")
                )
            )
            .await
            .is_err()
    );
    c.f.persistence
        .accept_observation(
            c.fence,
            &agent(3, operation.id(), LocalEvidence::Receipt {}),
        )
        .await
        .unwrap();
    c.f.persistence
        .accept_observation(
            c.fence,
            &agent(4, operation.id(), LocalEvidence::SubmissionIntent {}),
        )
        .await
        .unwrap();
    c.f.persistence
        .accept_observation(
            c.fence,
            &turn(5, operation.id(), "turn_owned", TurnState::Running, None),
        )
        .await
        .unwrap();
    assert!(
        c.f.persistence
            .accept_observation(
                c.fence,
                &turn(
                    6,
                    operation.id(),
                    "turn_human",
                    TurnState::Completed,
                    Some("human output")
                )
            )
            .await
            .is_err()
    );
    let result = turn(
        6,
        operation.id(),
        "turn_owned",
        TurnState::Completed,
        Some("owned output"),
    );
    assert_eq!(
        c.f.persistence
            .accept_observation(c.fence, &result)
            .await
            .unwrap(),
        6
    );
    assert_eq!(
        c.f.persistence
            .accept_observation(c.fence, &result)
            .await
            .unwrap(),
        6
    );
    let stored: (String, String) = sqlx::query_as(
        "SELECT outcome,output FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(stored, ("succeeded".to_owned(), "owned output".to_owned()));
    c.f.persistence
        .accept_observation(
            c.fence,
            &turn(
                7,
                operation.id(),
                "turn_owned",
                TurnState::Completed,
                Some("late edit"),
            ),
        )
        .await
        .unwrap();
    let output: String = sqlx::query_scalar(
        "SELECT output FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(output, "owned output");
}
