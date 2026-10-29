use super::*;
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        test_support::{OwnDatabase, own_database},
    },
    use_cases::user::UserPersistence,
};

pub(super) async fn fixture() -> Option<(OwnDatabase, Arc<PostgresPersistence>, User)> {
    let database = own_database().await?;
    let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(&suffix, &format!("{suffix}@example.test"), "old-hash")
        .await
        .unwrap();
    Some((database, persistence, user))
}

/// Wait for an actual PostgreSQL lock claimant, rather than assuming a spawned task reached SQL.
pub(super) async fn waiting_on(pool: &sqlx::PgPool, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid)))")
                .bind(blocker).fetch_one(pool).await.unwrap();
            if waiting { return; }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }).await.expect("competing statement waits on held account lock");
}

#[tokio::test]
async fn security_session_creation_fences_waiting_authentication() {
    let Some((database, persistence, user)) = fixture().await else {
        return;
    };
    let sessions = Arc::new(SessionAuthority::new(&AppConfig::for_test(), persistence));
    let earlier = sessions.cookie(&user).await.unwrap();
    let mut change = database.pool.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *change)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET password_hash = 'new-hash' WHERE id = $1")
        .bind(user.id)
        .execute(&mut *change)
        .await
        .unwrap();
    let claimant = tokio::spawn({
        let sessions = sessions.clone();
        let user = user.clone();
        async move { sessions.cookie(&user).await }
    });
    waiting_on(&database.pool, blocker).await;
    change.commit().await.unwrap();
    assert!(matches!(
        claimant.await.unwrap(),
        Err(AppError::InvalidCredentials)
    ));
    assert!(sessions.verify(earlier.value()).await.unwrap().is_none());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_sessions WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "stale authentication cannot persist a session");
}

#[tokio::test]
async fn security_session_cleanup_is_bounded_and_competing_cleaners_preserve_live_rows() {
    let Some((database, persistence, user)) = fixture().await else {
        return;
    };
    sqlx::query(
        r#"INSERT INTO user_sessions (id, user_id, auth_revision, issued_at, expires_at)
        SELECT gen_random_uuid(), $1, 0, CURRENT_TIMESTAMP - INTERVAL '2 days',
               CURRENT_TIMESTAMP - INTERVAL '1 day' FROM generate_series(1, 1700)"#,
    )
    .bind(user.id)
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query(r#"INSERT INTO user_sessions (id, user_id, auth_revision, issued_at, expires_at, revoked_at)
        SELECT gen_random_uuid(), $1, 0, CURRENT_TIMESTAMP - INTERVAL '1 day',
               CURRENT_TIMESTAMP + INTERVAL '1 day', CURRENT_TIMESTAMP FROM generate_series(1, 300)"#)
        .bind(user.id).execute(&database.pool).await.unwrap();
    let sessions = SessionAuthority::new(&AppConfig::for_test(), persistence.clone());
    let live = sessions.cookie(&user).await.unwrap();
    let mut locked = database.pool.begin().await.unwrap();
    for query in [
        "SELECT id FROM user_sessions WHERE expires_at <= statement_timestamp() ORDER BY expires_at, id LIMIT 1 FOR UPDATE",
        "SELECT id FROM user_sessions WHERE revoked_at IS NOT NULL ORDER BY revoked_at, id LIMIT 1 FOR UPDATE",
    ] {
        sqlx::query(query).fetch_one(&mut *locked).await.unwrap();
    }
    // Either missing SKIP LOCKED would block behind a held earliest candidate. The deadline is
    // below the adapter's 3s SQL timeout, so a passing result proves it skipped both locks.
    let first = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        persistence.cleanup_sessions(),
    )
    .await
    .expect("cleanup skips held eligible rows")
    .unwrap();
    locked.rollback().await.unwrap();
    assert_eq!(first.removed, 200);
    assert_eq!(first.backlog, 1001, "backlog saturates above 1000");
    let (left, right) = tokio::join!(
        persistence.cleanup_sessions(),
        persistence.cleanup_sessions()
    );
    let (left, right) = (left.unwrap(), right.unwrap());
    assert!(left.removed <= 200 && right.removed <= 200);
    assert_eq!(left.removed + right.removed, 400);
    let mut removed = first.removed + left.removed + right.removed;
    while removed < 2000 {
        let batch = persistence.cleanup_sessions().await.unwrap();
        assert!(batch.removed > 0 && batch.removed <= 200);
        assert!(batch.backlog <= 1001);
        removed += batch.removed;
    }
    assert_eq!(removed, 2000, "each eligible row deleted exactly once");
    let final_batch = persistence.cleanup_sessions().await.unwrap();
    assert_eq!(final_batch.removed, 0);
    assert_eq!(final_batch.backlog, 0);
    assert!(sessions.verify(live.value()).await.unwrap().is_some());
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM user_sessions")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert_eq!(remaining, 1);
}
