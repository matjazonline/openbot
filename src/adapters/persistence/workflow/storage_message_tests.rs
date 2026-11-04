use super::*;
use crate::application::workflow::storage::{RetentionRoot, WorkflowStorageMaintenance};

#[tokio::test]
async fn run_retention_pending_message_consumption_keeps_terminal_tree() {
    let setup = Box::pin(send(&[To::User])).await;
    parked(&setup).await;
    cancel(&setup, "retention-message").await;
    let scope = setup.scope();
    assert_eq!(action_state(setup.pool(), scope.run).await, "pending");
    let finished_at = sqlx::query_scalar(
        "UPDATE workflow_runs SET created_at=clock_timestamp()-interval '100 days',
        finished_at=clock_timestamp()-interval '91 days' WHERE id=$1 RETURNING finished_at",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap();
    let root = RetentionRoot {
        finished_at,
        company: scope.company,
        run: scope.run,
    };
    assert_eq!(
        setup.fixture.persistence.retire_tree(root).await.unwrap(),
        None
    );
}
