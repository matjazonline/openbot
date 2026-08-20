use sqlx::PgPool;

use crate::app_error::AppError;

pub mod agent;
pub mod approval;
pub mod channel;
pub mod company;
pub mod company_invite;
pub mod dashboard;
pub mod task;
#[cfg(test)]
pub mod test_support;
pub mod thread;
pub mod user;

#[derive(Clone)]
pub struct PostgresPersistence {
    pool: PgPool,
}

impl PostgresPersistence {
    pub fn new(pool: PgPool) -> Self {
        PostgresPersistence { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        AppError::Database(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! Guards the schema-wide invariant that every temporal column carries its timezone.
    //!
    //! A `timestamp without time zone` column is written by two clocks that need not agree:
    //! `CURRENT_TIMESTAMP` resolves in the *session's* timezone, while the app binds `Utc::now()`.
    //! sqlx pins its own sessions to UTC, so the app stays self-consistent and the divergence only
    //! shows up through another client — which is exactly what makes it easy to reintroduce.
    //!
    //! Both tests need a live database and no-op without one, so the rest of the suite still runs
    //! with no `DATABASE_URL` set.

    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    /// Stands in for a client that picks up the server's `timezone` setting instead of pinning UTC,
    /// the way `psql` and `fly postgres connect` do.
    const NON_UTC_SESSION_TIMEZONE: &str = "Europe/Ljubljana";

    use crate::adapters::persistence::test_support::test_pool;

    #[tokio::test]
    async fn every_timestamp_column_carries_a_timezone() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let naive: Vec<String> = sqlx::query_scalar(
            r#"SELECT table_name || '.' || column_name
                 FROM information_schema.columns
                WHERE table_schema = 'public'
                  AND data_type = 'timestamp without time zone'
                ORDER BY table_name, ordinal_position"#,
        )
        .fetch_all(&pool)
        .await
        .expect("information_schema is readable");

        assert!(
            naive.is_empty(),
            "these columns are `timestamp without time zone`, so CURRENT_TIMESTAMP writes the \
             session's local wall clock into them and any non-UTC client silently disagrees with \
             the app's own clock: {naive:?}"
        );
    }

    #[tokio::test]
    async fn a_row_defaulted_from_a_non_utc_session_reads_back_as_the_same_instant() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let mut tx = pool
            .begin()
            .await
            .expect("a transaction to roll the probe row back with");
        sqlx::query(&format!("SET LOCAL TIME ZONE '{NON_UTC_SESSION_TIMEZONE}'"))
            .execute(&mut *tx)
            .await
            .expect("the session timezone is settable");

        let probe = Uuid::new_v4();
        let before = Utc::now();
        let created_at: DateTime<Utc> = sqlx::query_scalar(
            r#"INSERT INTO users (id, username, email, password_hash)
               VALUES ($1, $2, $3, 'probe')
               RETURNING created_at"#,
        )
        .bind(probe)
        .bind(format!("tz-probe-{probe}"))
        .bind(format!("tz-probe-{probe}@example.test"))
        .fetch_one(&mut *tx)
        .await
        .expect("the probe row inserts and its created_at decodes as an absolute instant");

        let skew = (created_at - before).num_seconds().abs();
        assert!(
            skew < 60,
            "a DEFAULT written from a {NON_UTC_SESSION_TIMEZONE} session landed {skew}s from the \
             app's clock; a row inserted by hand would stay invisible to the workers that long"
        );

        tx.rollback()
            .await
            .expect("the probe row leaves nothing behind");
    }
}
