use async_trait::async_trait;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    application::session::{SessionCleanup, SessionPersistence, SessionRecord, VerifiedSession},
};

#[async_trait]
impl SessionPersistence for PostgresPersistence {
    async fn create_session(&self, session: &SessionRecord) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SET LOCAL statement_timeout = '3s'")
            .execute(&mut *transaction)
            .await?;
        let revision: Option<i64> =
            sqlx::query_scalar("SELECT auth_revision FROM users WHERE id = $1 FOR UPDATE")
                .bind(session.user_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if revision != Some(session.auth_revision) {
            return Err(AppError::InvalidCredentials);
        }
        sqlx::query(
            "INSERT INTO user_sessions (id, user_id, auth_revision, issued_at, expires_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(session.id.0)
        .bind(session.user_id)
        .bind(session.auth_revision)
        .bind(session.issued_at)
        .bind(session.expires_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn session_is_live(&self, session: &SessionRecord) -> AppResult<bool> {
        sqlx::query_scalar(
            r#"SELECT EXISTS (
                SELECT 1 FROM user_sessions AS session
                JOIN users AS account ON account.id = session.user_id
                WHERE session.id = $1 AND session.user_id = $2
                  AND session.auth_revision = $3 AND account.auth_revision = $3
                  AND session.issued_at = $4 AND session.expires_at = $5
                  AND session.revoked_at IS NULL AND session.expires_at > statement_timestamp()
            )"#,
        )
        .bind(session.id.0)
        .bind(session.user_id)
        .bind(session.auth_revision)
        .bind(session.issued_at)
        .bind(session.expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)
    }

    async fn revoke_session(&self, session: VerifiedSession) -> AppResult<()> {
        sqlx::query(
            "UPDATE user_sessions SET revoked_at = clock_timestamp() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(session.session_id.0)
        .bind(session.user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn cleanup_sessions(&self) -> AppResult<SessionCleanup> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SET LOCAL statement_timeout = '3s'")
            .execute(&mut *transaction)
            .await?;
        let removed = sqlx::query(
            r#"WITH expired AS (
                SELECT id FROM user_sessions WHERE expires_at <= statement_timestamp()
                ORDER BY expires_at, id LIMIT 100 FOR UPDATE SKIP LOCKED
            ), revoked AS (
                SELECT id FROM user_sessions WHERE revoked_at IS NOT NULL
                ORDER BY revoked_at, id LIMIT 100 FOR UPDATE SKIP LOCKED
            )
            DELETE FROM user_sessions AS session USING (
                SELECT id FROM expired UNION SELECT id FROM revoked
            ) AS candidate WHERE session.id = candidate.id"#,
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let backlog = sqlx::query_scalar(
            r#"SELECT count(*) FROM (
                SELECT id FROM (
                    (SELECT id FROM user_sessions WHERE expires_at <= statement_timestamp()
                     ORDER BY expires_at, id LIMIT 1001)
                    UNION ALL
                    (SELECT id FROM user_sessions WHERE revoked_at IS NOT NULL
                     ORDER BY revoked_at, id LIMIT 1001)
                ) AS candidates GROUP BY id LIMIT 1001
            ) AS bounded_backlog"#,
        )
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(SessionCleanup { removed, backlog })
    }
}
