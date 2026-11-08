//! Clock setup only, in the caller's OwnDatabase. All expiry/retirement uses native guards.
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::{agent_history::AgentExchangeId, storage::*};

pub(super) async fn age(p: &PostgresPersistence, scope: ActivationRequest) {
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::raw_sql(
        "ALTER TABLE workflow_runs DISABLE TRIGGER workflow_run_snapshot_immutable;
        ALTER TABLE agent_history_exchanges DISABLE TRIGGER agent_history_exchange_change",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE workflow_runs SET created_at=created_at-interval '100 days',
        deadline=deadline-interval '100 days',finished_at=finished_at-interval '100 days'
        WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE agent_history_exchanges SET created_at=created_at-interval '100 days',
        activated_at=activated_at-interval '100 days',finished_at=finished_at-interval '100 days'
        WHERE company_id=$1 AND id=$2 AND state IN ('completed','failed','cancelled')",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::raw_sql(
        "SET CONSTRAINTS ALL IMMEDIATE;
        ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable;
        ALTER TABLE agent_history_exchanges ENABLE TRIGGER agent_history_exchange_change",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

pub(super) async fn candidate(
    p: &PostgresPersistence,
    scope: ActivationRequest,
) -> HistoryCandidate {
    let finished_at = sqlx::query_scalar(
        "SELECT finished_at FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    HistoryCandidate {
        company: scope.company,
        finished_at,
        exchange: AgentExchangeId::for_execution(scope.execution),
    }
}

pub(super) async fn expire(p: &PostgresPersistence, scope: ActivationRequest) {
    age(p, scope).await;
    assert!(p.expire_history(candidate(p, scope).await).await.unwrap());
}

pub(super) async fn retire(p: &PostgresPersistence, scope: ActivationRequest) {
    expire(p, scope).await;
    retire_source(p, scope).await;
}

pub(super) async fn retire_source(p: &PostgresPersistence, scope: ActivationRequest) {
    let finished_at =
        sqlx::query_scalar("SELECT finished_at FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let result = p
        .retire_tree(RetentionRoot {
            company: scope.company,
            run: scope.run,
            finished_at,
        })
        .await
        .unwrap();
    assert!(
        result.is_some(),
        "eligible aged source tree must retire after payload expiry"
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(count, 0, "source cascade removes only expired metadata");
}

/// Backdate only an already completed history copy; the live root/continuation deadline is intact.
pub(super) async fn age_completed_copy(p: &PostgresPersistence, scope: ActivationRequest) {
    let before: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT deadline FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::raw_sql(
        "ALTER TABLE agent_history_exchanges DISABLE TRIGGER agent_history_exchange_change",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let changed = sqlx::query("UPDATE agent_history_exchanges SET created_at=created_at-interval '100 days',activated_at=activated_at-interval '100 days',finished_at=finished_at-interval '100 days' WHERE company_id=$1 AND id=$2 AND state='completed' AND payload_retained")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut *tx).await.unwrap().rows_affected();
    assert_eq!(changed, 1);
    sqlx::raw_sql("SET CONSTRAINTS ALL IMMEDIATE; ALTER TABLE agent_history_exchanges ENABLE TRIGGER agent_history_exchange_change")
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let after: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT deadline FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(before, after);
    assert!(
        after > chrono::Utc::now(),
        "genuine human continuation remains before original deadline"
    );
}

/// A cancelled count-prefix's original retry window has elapsed, but its copies are younger than90days.
pub(super) async fn close_retry_window(p: &PostgresPersistence, scope: ActivationRequest) {
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::raw_sql(
        "ALTER TABLE workflow_runs DISABLE TRIGGER workflow_run_snapshot_immutable;
        ALTER TABLE agent_history_exchanges DISABLE TRIGGER agent_history_exchange_change",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE workflow_runs SET created_at=created_at-interval '1 day',deadline=deadline-interval '1 day',finished_at=finished_at-interval '1 day' WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE agent_history_exchanges SET created_at=created_at-interval '1 day',activated_at=activated_at-interval '1 day',finished_at=finished_at-interval '1 day' WHERE company_id=$1 AND id=$2 AND state='cancelled'")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(
        "SET CONSTRAINTS ALL IMMEDIATE;
        ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable;
        ALTER TABLE agent_history_exchanges ENABLE TRIGGER agent_history_exchange_change",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let elapsed_but_recent: bool = sqlx::query_scalar("SELECT run.deadline<clock_timestamp() AND exchange.finished_at>clock_timestamp()-interval '90 days' FROM workflow_runs AS run JOIN agent_history_exchanges AS exchange ON exchange.company_id=run.company_id AND exchange.run_id=run.id WHERE run.company_id=$1 AND run.id=$2 AND exchange.id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_one(p.pool()).await.unwrap();
    assert!(
        elapsed_but_recent,
        "pressure, not90day age, must decide expiry of this cancelled prefix"
    );
}
