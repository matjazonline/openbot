use std::env;
use std::time::Duration;

use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tracing::info;

/// Sized for everything `serve` runs, including the default workflow worker slots
/// (`infra::workflow::worker_slots` refuses a pool too small for the configured slots).
pub const DEFAULT_DATABASE_MAX_CONNECTIONS: u32 = 20;

/// The configured pool size, as `init_db` and the workflow slot check both read it.
pub fn max_connections() -> u32 {
    env::var("DATABASE_MAX_CONNECTIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_DATABASE_MAX_CONNECTIONS)
}

/// Close a pooled connection released inside a transaction, so the server rolls it back.
///
/// sqlx 0.8.6 returns a connection to the pool still inside a transaction when a `begin()` future
/// is dropped after `BEGIN` reached the server but before its reply: its cancellation guard rolls
/// back only a transaction it has counted, and it counts one only once that reply arrives. The
/// next user's statements would then run, uncommitted and holding their locks, inside the leaked
/// transaction until some later `COMMIT` on that connection happened to commit them
/// (WF-P08-F029). Any timeout or shutdown that drops a transaction can do it, and the code wraps
/// many transactions in timeouts, so the guard sits on the pool rather than at each `begin`.
///
/// The check is a simple-protocol query, which runs in its own implicit transaction unless an
/// explicit one is still open: only then does the transaction predate the statement. It costs one
/// round trip per release, next to the ping sqlx already makes there.
/// `db::tests::a_cancelled_begin_never_leaks_its_transaction` pins both the sqlx behaviour and
/// this guard; drop the guard only when that test shows sqlx no longer leaks.
pub fn discard_leaked_transactions(options: PgPoolOptions) -> PgPoolOptions {
    options.after_release(|connection, _| {
        Box::pin(async move {
            let leaked = inside_a_transaction(connection).await?;
            if leaked {
                tracing::warn!(
                    "A pooled database connection was released inside a transaction; closing it"
                );
            }
            Ok(!leaked)
        })
    })
}

/// An argument-less `&str` goes over the simple protocol, which the check depends on.
async fn inside_a_transaction(connection: &mut sqlx::PgConnection) -> Result<bool, sqlx::Error> {
    use sqlx::Executor;
    connection
        .fetch_one("SELECT transaction_timestamp() <> statement_timestamp()")
        .await?
        .try_get(0)
}

pub async fn init_db() -> anyhow::Result<PgPool> {
    let database_url = env::var("DATABASE_URL")?;
    let max_connections = max_connections();
    let acquire_timeout_secs = env::var("DATABASE_ACQUIRE_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);

    let pool = discard_leaked_transactions(PgPoolOptions::new())
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(acquire_timeout_secs))
        .connect(&database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    info!("Connected to database and ran migrations!");
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::own_database;
    use sqlx::Connection;

    /// Whether the connection the pool hands out next is inside a transaction.
    async fn next_connection_in_transaction(pool: &PgPool) -> bool {
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql("SELECT transaction_timestamp() <> statement_timestamp()")
            .fetch_one(&mut *connection)
            .await
            .unwrap()
            .get(0)
    }

    /// Drop a `begin` after `BEGIN` reached the server but before its reply arrived, then let
    /// the dropped connection finish returning to the pool.
    async fn cancel_a_begin(pool: &PgPool) {
        let cancelled = tokio::time::timeout(
            Duration::from_millis(100),
            pool.begin_with("BEGIN; SELECT pg_sleep(0.5)"),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "the begin was still waiting for its reply"
        );
        tokio::time::sleep(Duration::from_millis(700)).await;
    }

    /// WF-P08-F029: sqlx alone hands the next user a connection still inside a cancelled
    /// `begin`'s transaction; the guard closes that connection instead. When the first assertion
    /// fails, sqlx has fixed the leak and `discard_leaked_transactions` can go.
    #[tokio::test]
    async fn a_cancelled_begin_never_leaks_its_transaction() {
        let database = own_database().await.expect("pool tests need a database");
        let options = database.pool.connect_options().as_ref().clone();
        let bare = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        cancel_a_begin(&bare).await;
        assert!(
            next_connection_in_transaction(&bare).await,
            "sqlx no longer leaks a cancelled begin; see discard_leaked_transactions"
        );
        bare.close().await;

        let guarded = discard_leaked_transactions(PgPoolOptions::new())
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        // An ordinary release keeps its connection: the check has no false positive.
        let backend = |pool: PgPool| async move {
            let mut connection = pool.acquire().await.unwrap();
            let mut transaction = (*connection).begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *transaction)
                .await
                .unwrap();
            transaction.commit().await.unwrap();
            pid
        };
        let first = backend(guarded.clone()).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(backend(guarded.clone()).await, first);
        cancel_a_begin(&guarded).await;
        assert!(!next_connection_in_transaction(&guarded).await);
        guarded.close().await;
    }
}
