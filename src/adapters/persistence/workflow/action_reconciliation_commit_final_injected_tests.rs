//! Each Final position rejects a complete, otherwise-valid ordinary COMMIT.
use super::super::super::commit_tests::injected::{Position, assert_injection_catalog};
use super::*;

async fn exercise_injected(position: Position) {
    assert!(
        matches!(
            position,
            Position::Evidence | Position::Reopen | Position::Audit | Position::Command
        ),
        "Final never fabricates a receipt injection"
    );
    // Retain the genuine sibling/provider chain behind the stock-stack seam.
    let source = Box::pin(source()).await;
    let observed = install(&source).await;
    sqlx::raw_sql(include_str!("action_reconciliation_commit_injected.sql"))
        .execute(source.fixture.persistence().pool())
        .await
        .unwrap();
    sqlx::raw_sql(&position.trigger())
        .execute(source.fixture.persistence().pool())
        .await
        .unwrap();
    let installed = observer::catalog(&source.fixture).await;
    assert_injection_catalog(&observed, &installed, position);
    let mut gate = observer::Gate::hold(&source.fixture).await;
    let before = configure(&source, Case::Positive, &gate).await;
    let (result, observation) = gate
        .run(&source.fixture, &source.command, source.verifier.clone())
        .await;
    let after = snapshot(&source.fixture).await;
    assert!(
        observation.unwrap().is_some(),
        "complete Final proposal reached gate: {result:?}"
    );
    let expected = format!(
        "error returned from database: row3e injected Final {}",
        position.name()
    );
    assert!(
        matches!(result, Err(AppError::Database(ref error)) if error==&expected),
        "{position:?}: {result:?}"
    );
    assert_eq!(
        after, before,
        "{position:?}: every public row rolls back after exact backend quiescence"
    );
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert!(source.verifier.issued.lock().unwrap().is_some());
    assert_eq!(effects(&source.fixture).await, 1);
    assert_eq!(observer::catalog(&source.fixture).await, installed);
    drop(gate);
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_commit_final_injected_evidence() {
    Box::pin(exercise_injected(Position::Evidence)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_final_injected_reopen() {
    Box::pin(exercise_injected(Position::Reopen)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_final_injected_audit() {
    Box::pin(exercise_injected(Position::Audit)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_final_injected_command() {
    Box::pin(exercise_injected(Position::Command)).await;
}
