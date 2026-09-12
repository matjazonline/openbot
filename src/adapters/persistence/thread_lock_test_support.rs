//! Force real row-lock contention without adding coordination hooks to production code.

use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use uuid::Uuid;

use super::PostgresPersistence;

pub(super) async fn contender(pool: &PgPool, name: &str) -> PostgresPersistence {
    let options = pool
        .connect_options()
        .as_ref()
        .clone()
        .application_name(name);
    PostgresPersistence::new(
        PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap(),
    )
}

pub(super) async fn hold_first_thread(
    pool: &PgPool,
    thread_ids: &[Uuid],
) -> Transaction<'static, Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM threads WHERE id = ANY($1) ORDER BY id LIMIT 1 FOR UPDATE")
        .bind(thread_ids)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx
}

pub(super) async fn release_when_both_wait(
    blocker: Transaction<'_, Postgres>,
    pool: &PgPool,
    names: &[&str],
) {
    wait_until_blocked(pool, names).await;
    blocker.rollback().await.unwrap();
}

pub(super) async fn wait_until_blocked(pool: &PgPool, names: &[&str]) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() \
                 AND application_name = ANY($1) AND wait_event_type = 'Lock' \
                 AND cardinality(pg_blocking_pids(pid)) > 0",
            )
            .bind(names)
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting == names.len() as i64 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the named operations must reach real PostgreSQL lock contention");
}
