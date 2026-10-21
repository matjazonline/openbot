//! Production inbound dispatch, shared human policy, real PostgreSQL, scripted model only.
use super::*;
use crate::adapters::persistence::test_support::own_database;
use crate::services::{
    harness::runs::RunCheckpoint,
    test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
};
use serde_json::Value;

async fn checkpoint(fx: &Fixture, task_id: Uuid) -> RunCheckpoint {
    let value: Value =
        sqlx::query_scalar("SELECT checkpoint FROM task_harness_runs WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn provider_deadline_cancels_the_claim_and_recovery_keeps_the_unknown_reservation() {
    use crate::entities::task::{TaskFailure, TaskFailureOutcome, TaskStopReason};
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let (arrived_tx, arrived_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let mut blocked = ScriptedResponse::turn(LlmTurn::text("This response must not commit."), 0);
    blocked.barrier = Some((arrived_tx, release_rx));
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(|_| Ok(()), blocked),
        ScriptedExchange::new(
            |_| Ok(()),
            ScriptedResponse::turn(LlmTurn::text("Recovered within the original budget."), 1),
        ),
    ])
    .await;
    let fx = fixture(pool, Some(&llm.base_url)).await;
    sqlx::query("UPDATE agents SET run_timeout_secs = 1 WHERE company_id = $1")
        .bind(fx.company.id)
        .execute(&fx.pool)
        .await
        .unwrap();
    let ingest = fx
        .threads
        .ingest_test_email(inbound(
            &fx.owner_email,
            &fx.address(&fx.channel_a),
            "<rig-timeout@example.test>",
            "Answer once",
        ))
        .await
        .unwrap();
    let task_id = ingest.task_ids[0];
    let lease = fx.claim(task_id).await;
    let execute = fx.threads.execute_claimed_agent_task_and_dispatch(
        &ingest,
        ReplyDelivery::Send,
        lease,
        ingest.correlation_id().unwrap(),
    );
    let (arrived, outcome) = tokio::join!(
        tokio::time::timeout(std::time::Duration::from_secs(5), arrived_rx),
        execute
    );
    arrived.unwrap().unwrap();
    assert!(matches!(
        outcome,
        Err(crate::app_error::AppError::Timeout(_))
    ));
    let interrupted = checkpoint(&fx, task_id).await;
    super::model_usage_assertions::assert_projection(&fx, &interrupted).await;
    assert_eq!(interrupted.reservations.len(), 1);
    assert!(interrupted.turns.is_empty());
    assert!(interrupted.invocations.is_empty());
    release_tx.send(()).unwrap();
    fx.persistence
        .mark_task_failed(TaskFailure {
            lease,
            error: "fixture timeout",
            next_run_at: Utc::now(),
            outcome: TaskFailureOutcome::Retry,
            reason: TaskStopReason::TimedOut,
        })
        .await
        .unwrap();
    let lease = fx.claim(task_id).await;
    let result = fx
        .threads
        .execute_claimed_agent_task_and_dispatch(
            &ingest,
            ReplyDelivery::Send,
            lease,
            ingest.correlation_id().unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(result, DispatchOutcome::Replied(_)));
    assert_eq!(llm.finish().await, Ok(2));
    let recovered = checkpoint(&fx, task_id).await;
    super::model_usage_assertions::assert_projection(&fx, &recovered).await;
    assert_eq!(recovered.run_id, interrupted.run_id);
    assert_eq!(recovered.reservations.len(), 2);
    assert_eq!(recovered.turns.len(), 1);
    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .unwrap();
}
