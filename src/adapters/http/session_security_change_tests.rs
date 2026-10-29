use super::concurrency_tests::{fixture, waiting_on};
use super::*;
use crate::entities::value_objects::EmailAddress;
use crate::use_cases::user::{
    AccountChange, AccountChangePersistence, PasswordChange, PendingAccountChange, ProfileUpdate,
    UserPersistence, UserUseCases, auth_attempt_key,
};

#[tokio::test]
async fn security_session_stolen_cookies_fail_after_password_change_and_new_login_succeeds() {
    let Some((_database, persistence, user)) = fixture().await else {
        return;
    };
    let user = persistence
        .update_password_hash(user.id, "test:old-password")
        .await
        .unwrap()
        .unwrap();
    let users = UserUseCases::new(
        Arc::new(test_support::TestHasher),
        persistence.clone(),
        "attempt-key",
    );
    let sessions = SessionAuthority::new(&AppConfig::for_test(), persistence.clone());
    let current = sessions.cookie(&user).await.unwrap();
    let stolen = sessions.cookie(&user).await.unwrap();
    users
        .change_password(
            user.id,
            PasswordChange {
                current: &"old-password".into(),
                new: &"new-password".into(),
                confirmation: &"new-password".into(),
            },
            None,
        )
        .await
        .unwrap();
    assert!(sessions.verify(current.value()).await.unwrap().is_none());
    assert!(sessions.verify(stolen.value()).await.unwrap().is_none());
    assert!(
        users
            .login(&user.username, &"old-password".into(), None, None)
            .await
            .is_err()
    );
    let fresh = users
        .login(&user.username, &"new-password".into(), None, None)
        .await
        .unwrap();
    assert_eq!(fresh.auth_revision, user.auth_revision + 1);
    let cookie = sessions.cookie(&fresh).await.unwrap();
    assert!(sessions.verify(cookie.value()).await.unwrap().is_some());
}

#[tokio::test]
async fn security_session_all_account_mutation_paths_revoke_atomically() {
    let Some((database, persistence, user)) = fixture().await else {
        return;
    };
    let sessions = SessionAuthority::new(&AppConfig::for_test(), persistence.clone());
    let mut current = user;
    for step in 0..5 {
        let cookie = sessions.cookie(&current).await.unwrap();
        let updated = match step {
            0 => persistence
                .activate_password(current.id, "setup-password")
                .await
                .unwrap()
                .unwrap(),
            1 => persistence
                .update_profile(
                    current.id,
                    ProfileUpdate {
                        username: &current.username,
                        email: &EmailAddress::from(format!("{}@next.test", current.id)),
                        avatar_url: None,
                    },
                )
                .await
                .unwrap()
                .unwrap(),
            2 | 3 => {
                let email = EmailAddress::from(format!("{}@confirmed.test", current.id));
                let change = if step == 2 {
                    AccountChange::Email(&email)
                } else {
                    AccountChange::Password("confirmed-password")
                };
                let kind = change.kind();
                persistence
                    .save_pending_account_change(PendingAccountChange {
                        user_id: current.id,
                        change,
                        confirmation_code_hash: "test-code-hash",
                        expires_at: Utc::now() + chrono::Duration::minutes(10),
                    })
                    .await
                    .unwrap();
                assert!(
                    sessions.verify(cookie.value()).await.unwrap().is_some(),
                    "pending changes preserve sessions"
                );
                let key = auth_attempt_key(
                    "test",
                    "session-confirmation",
                    &[current.id.as_bytes(), kind.as_str().as_bytes()],
                );
                persistence
                    .confirm_pending_account_change(current.id, kind, "test-code-hash", &key)
                    .await
                    .unwrap()
                    .unwrap()
            }
            _ => persistence
                .update_password_hash(current.id, "immediate-password")
                .await
                .unwrap()
                .unwrap(),
        };
        assert_eq!(updated.auth_revision, current.auth_revision + 1);
        assert!(
            sessions.verify(cookie.value()).await.unwrap().is_none(),
            "mutation {step}"
        );
        let live: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM user_sessions WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(current.id)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert_eq!(live, 0);
        current = updated;
    }
}

#[tokio::test]
async fn security_session_rollback_display_edits_and_noops_preserve_sessions() {
    let Some((database, persistence, user)) = fixture().await else {
        return;
    };
    let sessions = SessionAuthority::new(&AppConfig::for_test(), persistence.clone());
    let cookie = sessions.cookie(&user).await.unwrap();
    let mut transaction = database.pool.begin().await.unwrap();
    sqlx::query("UPDATE users SET password_hash = 'rolled-back' WHERE id = $1")
        .bind(user.id)
        .execute(&mut *transaction)
        .await
        .unwrap();
    let revoked: bool =
        sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM user_sessions WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    assert!(revoked, "revocation happens inside the account transaction");
    transaction.rollback().await.unwrap();
    let refreshed = persistence.get_by_id(user.id).await.unwrap().unwrap();
    assert_eq!(refreshed.auth_revision, user.auth_revision);
    assert_eq!(refreshed.password_hash, user.password_hash);
    persistence
        .update_profile_display(user.id, "display-only", None)
        .await
        .unwrap();
    let unchanged = persistence
        .update_profile(
            user.id,
            ProfileUpdate {
                username: "display-only",
                email: &EmailAddress::from(user.email.to_uppercase()),
                avatar_url: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.auth_revision, user.auth_revision);
    assert!(sessions.verify(cookie.value()).await.unwrap().is_some());
    let invalid_revision =
        sqlx::query("UPDATE users SET auth_revision = auth_revision + 1 WHERE id = $1")
            .bind(user.id)
            .execute(&database.pool)
            .await;
    assert!(
        invalid_revision.is_err(),
        "revision cannot advance independently of a security change"
    );
}

#[tokio::test]
async fn security_session_issued_first_is_revoked_by_waiting_password_change() {
    let Some((database, persistence, user)) = fixture().await else {
        return;
    };
    let sessions = SessionAuthority::new(&AppConfig::for_test(), persistence.clone());
    // Capture a signed token, then reinsert its record in an issuance transaction held at commit.
    let cookie = sessions.cookie(&user).await.unwrap();
    let row = sessions.decode_session(cookie.value()).unwrap();
    sqlx::query("DELETE FROM user_sessions WHERE id = $1")
        .bind(row.id.0)
        .execute(&database.pool)
        .await
        .unwrap();
    let mut issuance = database.pool.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *issuance)
        .await
        .unwrap();
    sqlx::query("SELECT auth_revision FROM users WHERE id = $1 FOR UPDATE")
        .bind(user.id)
        .fetch_one(&mut *issuance)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_sessions (id, user_id, auth_revision, issued_at, expires_at) VALUES ($1,$2,$3,$4,$5)")
        .bind(row.id.0).bind(row.user_id).bind(row.auth_revision).bind(row.issued_at).bind(row.expires_at)
        .execute(&mut *issuance).await.unwrap();
    let change = tokio::spawn({
        let persistence = persistence.clone();
        async move {
            persistence
                .update_password_hash(user.id, "competing-password")
                .await
        }
    });
    waiting_on(&database.pool, blocker).await;
    issuance.commit().await.unwrap();
    change.await.unwrap().unwrap().unwrap();
    assert!(sessions.verify(cookie.value()).await.unwrap().is_none());
    let revoked: bool =
        sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM user_sessions WHERE id = $1")
            .bind(row.id.0)
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert!(revoked);
}
