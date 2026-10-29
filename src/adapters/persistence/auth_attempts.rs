//! Durable, fixed-window authentication budgets shared by every application instance.

use sqlx::{PgPool, Postgres, Transaction};

use crate::{app_error::AppResult, use_cases::user::AuthAttemptKey};

const CLEANUP_BATCH: i64 = 64;

async fn purge_expired(pool: &PgPool, table: &str, key_column: &str) -> AppResult<()> {
    // Table and column come only from the two fixed call sites below, never from a request.
    let sql = format!(
        "DELETE FROM {table} AS budget WHERE budget.{key_column} IN \
         (SELECT {key_column} FROM {table} WHERE window_ends_at <= CURRENT_TIMESTAMP \
          ORDER BY window_ends_at LIMIT $1) \
         AND budget.window_ends_at <= CURRENT_TIMESTAMP"
    );
    sqlx::query(&sql).bind(CLEANUP_BATCH).execute(pool).await?;
    Ok(())
}

pub(super) async fn reserve_code_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    key: &AuthAttemptKey,
) -> AppResult<bool> {
    // The unique key serializes competing claimants. A refused candidate never extends the
    // window; after expiry the first claimant starts one fresh one-hour window.
    let reserved = sqlx::query_scalar::<_, i16>(
        r#"INSERT INTO auth_code_attempts (operation_key, attempts, window_ends_at)
           VALUES ($1, 1, CURRENT_TIMESTAMP + INTERVAL '1 hour')
           ON CONFLICT (operation_key) DO UPDATE SET
               attempts = CASE WHEN auth_code_attempts.window_ends_at <= CURRENT_TIMESTAMP
                               THEN 1 ELSE auth_code_attempts.attempts + 1 END,
               window_ends_at = CASE WHEN auth_code_attempts.window_ends_at <= CURRENT_TIMESTAMP
                                     THEN CURRENT_TIMESTAMP + INTERVAL '1 hour'
                                     ELSE auth_code_attempts.window_ends_at END
           WHERE auth_code_attempts.window_ends_at <= CURRENT_TIMESTAMP
              OR auth_code_attempts.attempts < 5
           RETURNING attempts"#,
    )
    .bind(key.as_bytes().as_slice())
    .fetch_optional(&mut **transaction)
    .await?;
    if reserved.is_none() {
        tracing::info!(
            auth_budget_kind = "code",
            auth_outcome = "exhausted",
            "authentication attempt budget"
        );
    }
    Ok(reserved.is_some())
}

pub(super) async fn clear_code_attempt(
    transaction: &mut Transaction<'_, Postgres>,
    key: &AuthAttemptKey,
) -> AppResult<()> {
    sqlx::query("DELETE FROM auth_code_attempts WHERE operation_key = $1")
        .bind(key.as_bytes().as_slice())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub(super) async fn cleanup_code_attempts(pool: &PgPool) -> AppResult<()> {
    purge_expired(pool, "auth_code_attempts", "operation_key").await
}

pub(super) async fn cleanup_pending_registrations(pool: &PgPool) -> AppResult<()> {
    sqlx::query(
        r#"DELETE FROM pending_user_registrations AS pending
           WHERE pending.email IN (
               SELECT email FROM pending_user_registrations
               WHERE expires_at <= CURRENT_TIMESTAMP ORDER BY expires_at LIMIT $1
           ) AND pending.expires_at <= CURRENT_TIMESTAMP"#,
    )
    .bind(CLEANUP_BATCH)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn cleanup_pending_account_changes(pool: &PgPool) -> AppResult<()> {
    sqlx::query(
        r#"DELETE FROM pending_account_changes AS pending
           WHERE (pending.user_id, pending.kind) IN (
               SELECT user_id, kind FROM pending_account_changes
               WHERE expires_at <= CURRENT_TIMESTAMP ORDER BY expires_at LIMIT $1
           ) AND pending.expires_at <= CURRENT_TIMESTAMP"#,
    )
    .bind(CLEANUP_BATCH)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn reserve_login_attempt(pool: &PgPool, key: &AuthAttemptKey) -> AppResult<bool> {
    purge_expired(pool, "auth_login_attempts", "identity_source_key").await?;
    let reserved = sqlx::query_scalar::<_, i16>(
        r#"INSERT INTO auth_login_attempts (identity_source_key, attempts, window_ends_at)
           VALUES ($1, 1, CURRENT_TIMESTAMP + INTERVAL '2 minutes')
           ON CONFLICT (identity_source_key) DO UPDATE SET
               attempts = CASE WHEN auth_login_attempts.window_ends_at <= CURRENT_TIMESTAMP
                               THEN 1 ELSE auth_login_attempts.attempts + 1 END,
               window_ends_at = CASE WHEN auth_login_attempts.window_ends_at <= CURRENT_TIMESTAMP
                                     THEN CURRENT_TIMESTAMP + INTERVAL '2 minutes'
                                     ELSE auth_login_attempts.window_ends_at END
           WHERE auth_login_attempts.window_ends_at <= CURRENT_TIMESTAMP
              OR auth_login_attempts.attempts < 5
           RETURNING attempts"#,
    )
    .bind(key.as_bytes().as_slice())
    .fetch_optional(pool)
    .await?;
    if reserved.is_none() {
        tracing::info!(
            auth_budget_kind = "login",
            auth_outcome = "exhausted",
            "authentication attempt budget"
        );
    }
    Ok(reserved.is_some())
}

pub(super) async fn clear_login_attempt(pool: &PgPool, key: &AuthAttemptKey) -> AppResult<()> {
    sqlx::query("DELETE FROM auth_login_attempts WHERE identity_source_key = $1")
        .bind(key.as_bytes().as_slice())
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{net::IpAddr, sync::Arc};

    use chrono::{Duration, Utc};
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::*;
    use crate::{
        adapters::persistence::{PostgresPersistence, test_support::own_database},
        app_error::AppError,
        entities::value_objects::EmailAddress,
        infra::config::AppConfig,
        use_cases::user::{
            AccountChange, AccountChangeKind, AccountChangePersistence, ConfirmationCodeSender,
            ConfirmationPurpose, EmailConfirmation, PendingAccountChange, PendingRegistration,
            ProfileUpdate, RegistrationOutcome, RegistrationPersistence, UserCredentialsHasher,
            UserPersistence, UserUseCases, auth_attempt_key,
        },
    };

    const SECRET: &str = "test-attempt-secret";

    #[tokio::test]
    async fn security_email_identity_configured_send_failure_never_bypasses() {
        struct FailingSender;
        #[async_trait::async_trait]
        impl ConfirmationCodeSender for FailingSender {
            async fn send_code(
                &self,
                _: &EmailAddress,
                _: &str,
                _: ConfirmationPurpose,
            ) -> AppResult<()> {
                Err(AppError::ServiceUnavailable("SMTP delivery failed".into()))
            }
        }

        let Some(database) = own_database().await else {
            return;
        };
        let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
        let mut config = AppConfig::for_test();
        config.smtp_host = "smtp.example.com".into();
        config.smtp_from_address = "noreply@example.com".into();
        let config = Arc::new(config);
        let users = UserUseCases::new(
            Arc::new(crate::infra::argon2_password_hasher()),
            persistence.clone(),
            config.jwt_secret.clone(),
        )
        .with_email_confirmation(EmailConfirmation {
            registrations: persistence.clone(),
            account_changes: persistence.clone(),
            codes: Arc::new(FailingSender),
            config,
            transport_available: true,
            allow_unverified_without_smtp: true,
        });

        let suffix = Uuid::new_v4().simple();
        let registration_email = format!("sendfail-{suffix}@example.com");
        let registration_name = format!("sendfail_{suffix}");
        assert!(matches!(
            users
                .register(
                    &registration_name,
                    &registration_email,
                    &SecretString::from("long-password"),
                    None,
                )
                .await,
            Err(AppError::ServiceUnavailable(_))
        ));
        assert!(
            persistence
                .get_by_email(&registration_email)
                .await
                .unwrap()
                .is_none()
        );

        let old_email = format!("old-{suffix}@example.com");
        let user = persistence
            .create_user(&format!("existing_{suffix}"), &old_email, "hash")
            .await
            .unwrap();
        sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(user.id)
            .execute(&database.pool)
            .await
            .unwrap();
        let new_email = EmailAddress::from(format!("new-{suffix}@example.com"));
        assert!(matches!(
            users
                .update_profile(
                    user.id,
                    ProfileUpdate {
                        username: "renamed",
                        email: &new_email,
                        avatar_url: None,
                    },
                )
                .await,
            Err(AppError::ServiceUnavailable(_))
        ));
        let after = persistence.get_by_id(user.id).await.unwrap().unwrap();
        assert_eq!(after.email, old_email);
        assert!(after.email_verified_at.is_some());
    }

    async fn account(persistence: &PostgresPersistence) -> Uuid {
        let name = format!("auth_limit_{}", Uuid::new_v4().simple());
        persistence
            .create_user(&name, &format!("{name}@example.com"), "old-hash")
            .await
            .expect("test account")
            .id
    }

    fn change_key(id: Uuid, kind: AccountChangeKind) -> AuthAttemptKey {
        auth_attempt_key(
            SECRET,
            "account-change-code:v1",
            &[id.as_bytes(), kind.as_str().as_bytes()],
        )
    }

    async fn pending_password(persistence: &PostgresPersistence, id: Uuid, code: &str) {
        persistence
            .save_pending_account_change(PendingAccountChange {
                user_id: id,
                change: AccountChange::Password("new-hash"),
                confirmation_code_hash: code,
                expires_at: Utc::now() + Duration::minutes(15),
            })
            .await
            .expect("pending password change");
    }

    #[tokio::test]
    async fn security_auth_limits_competing_claimants_cannot_exceed_last_code_attempt() {
        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
        let id = account(&persistence).await;
        let key = change_key(id, AccountChangeKind::Password);
        pending_password(&persistence, id, "right").await;

        for _ in 0..4 {
            assert!(
                persistence
                    .confirm_pending_account_change(id, AccountChangeKind::Password, "wrong", &key)
                    .await
                    .expect("wrong candidate")
                    .is_none()
            );
        }
        let (a, b) = tokio::join!(
            persistence.confirm_pending_account_change(
                id,
                AccountChangeKind::Password,
                "wrong",
                &key
            ),
            persistence.confirm_pending_account_change(
                id,
                AccountChangeKind::Password,
                "wrong",
                &key
            ),
        );
        assert!(a.expect("first claimant").is_none());
        assert!(b.expect("second claimant").is_none());
        let attempts: i16 =
            sqlx::query_scalar("SELECT attempts FROM auth_code_attempts WHERE operation_key = $1")
                .bind(key.as_bytes().as_slice())
                .fetch_one(&pool)
                .await
                .expect("one shared counter");
        assert_eq!(attempts, 5);
        assert!(
            persistence
                .confirm_pending_account_change(id, AccountChangeKind::Password, "right", &key)
                .await
                .expect("exhausted correct candidate")
                .is_none()
        );
        assert_eq!(
            persistence
                .get_by_id(id)
                .await
                .unwrap()
                .unwrap()
                .password_hash,
            "old-hash"
        );
    }

    #[tokio::test]
    async fn security_auth_limits_competing_correct_codes_spend_once() {
        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        let persistence = Arc::new(PostgresPersistence::new(pool));
        let id = account(&persistence).await;
        let key = change_key(id, AccountChangeKind::Password);
        pending_password(&persistence, id, "right").await;

        let (a, b) = tokio::join!(
            persistence.confirm_pending_account_change(
                id,
                AccountChangeKind::Password,
                "right",
                &key
            ),
            persistence.confirm_pending_account_change(
                id,
                AccountChangeKind::Password,
                "right",
                &key
            ),
        );
        let won = [a.expect("first claimant"), b.expect("second claimant")]
            .into_iter()
            .filter(Option::is_some)
            .count();
        assert_eq!(won, 1);
        assert_eq!(
            persistence
                .get_by_id(id)
                .await
                .unwrap()
                .unwrap()
                .password_hash,
            "new-hash"
        );
    }

    #[tokio::test]
    async fn security_auth_limits_cancel_and_expired_code_reissue_keep_budget() {
        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        let persistence = PostgresPersistence::new(pool.clone());
        let id = account(&persistence).await;
        let kind = AccountChangeKind::Password;
        let key = change_key(id, kind);
        pending_password(&persistence, id, "first").await;
        for _ in 0..5 {
            assert!(
                persistence
                    .confirm_pending_account_change(id, kind, "wrong", &key)
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        persistence
            .discard_pending_account_change(id, kind)
            .await
            .unwrap();
        pending_password(&persistence, id, "second").await;
        assert!(
            persistence
                .confirm_pending_account_change(id, kind, "second", &key)
                .await
                .unwrap()
                .is_none(),
            "cancel and reissue retain the budget"
        );

        sqlx::query("UPDATE pending_account_changes SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second' WHERE user_id = $1 AND kind = $2")
            .bind(id).bind(kind.as_str()).execute(&pool).await.unwrap();
        pending_password(&persistence, id, "third").await;
        assert!(
            persistence
                .confirm_pending_account_change(id, kind, "third", &key)
                .await
                .unwrap()
                .is_none(),
            "pending-code expiry is shorter than budget expiry"
        );

        sqlx::query("UPDATE auth_code_attempts SET window_ends_at = CURRENT_TIMESTAMP - INTERVAL '1 second' WHERE operation_key = $1")
            .bind(key.as_bytes().as_slice()).execute(&pool).await.unwrap();
        assert!(
            persistence
                .confirm_pending_account_change(id, kind, "third", &key)
                .await
                .unwrap()
                .is_some(),
            "a fresh window permits legitimate recovery"
        );
    }

    #[tokio::test]
    async fn security_auth_limits_registration_code_spends_once_across_instances() {
        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        let first = PostgresPersistence::new(pool.clone());
        let second = PostgresPersistence::new(pool.clone());
        let email = EmailAddress::from(format!("register-{}@example.com", Uuid::new_v4().simple()));
        let name = format!("reg_{}", Uuid::new_v4().simple());
        let normalized = email.trim().to_ascii_lowercase();
        let key = auth_attempt_key(SECRET, "registration-code:v1", &[normalized.as_bytes()]);
        first
            .save_pending_registration(PendingRegistration {
                username: &name,
                email: &email,
                password_hash: "hash",
                confirmation_code_hash: "right",
                expires_at: Utc::now() + Duration::minutes(15),
            })
            .await
            .unwrap();
        let (a, b) = tokio::join!(
            first.confirm_pending_registration(&email, "right", &key),
            second.confirm_pending_registration(&email, "right", &key),
        );
        assert_eq!(
            [a.expect("first instance"), b.expect("second instance")]
                .into_iter()
                .filter(Option::is_some)
                .count(),
            1
        );
        assert!(first.get_by_email(&email).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn security_auth_limits_unicode_registration_aliases_share_budget_and_code_digest() {
        struct CapturedCode(std::sync::Mutex<Option<String>>);
        #[async_trait::async_trait]
        impl ConfirmationCodeSender for CapturedCode {
            async fn send_code(
                &self,
                _: &EmailAddress,
                code: &str,
                _: ConfirmationPurpose,
            ) -> AppResult<()> {
                *self.0.lock().unwrap() = Some(code.to_owned());
                Ok(())
            }
        }

        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
        let mut config = AppConfig::for_test();
        config.smtp_host = "smtp.example.com".into();
        config.smtp_from_address = "noreply@example.com".into();
        let config = Arc::new(config);
        let sender = Arc::new(CapturedCode(std::sync::Mutex::new(None)));
        let cases = UserUseCases::new(
            Arc::new(crate::infra::argon2_password_hasher()),
            persistence.clone(),
            config.jwt_secret.clone(),
        )
        .with_email_confirmation(EmailConfirmation {
            registrations: persistence.clone(),
            account_changes: persistence.clone(),
            codes: sender.clone(),
            config: config.clone(),
            transport_available: true,
            allow_unverified_without_smtp: false,
        });
        let email = format!("Älice_{}@example.com", Uuid::new_v4().simple());
        let alias = email.to_lowercase();
        let name = format!("unicode_{}", Uuid::new_v4().simple());
        assert!(matches!(
            cases
                .register(&name, &email, &SecretString::from("long-password"), None)
                .await
                .unwrap(),
            RegistrationOutcome::ConfirmationSent
        ));
        let code = sender.0.lock().unwrap().clone().expect("mailed code");
        for _ in 0..5 {
            assert!(cases.confirm_registration(&email, "wrong").await.is_err());
        }
        assert!(
            cases.confirm_registration(&alias, &code).await.is_err(),
            "the same citext row cannot gain another budget through Unicode case"
        );
        let canonical = persistence.canonical_auth_identifier(&email).await.unwrap();
        let key = auth_attempt_key(
            &config.jwt_secret,
            "registration-code:v1",
            &[canonical.as_bytes()],
        );
        let attempts: i16 =
            sqlx::query_scalar("SELECT attempts FROM auth_code_attempts WHERE operation_key = $1")
                .bind(key.as_bytes().as_slice())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(attempts, 5);

        sqlx::query("UPDATE auth_code_attempts SET window_ends_at = CURRENT_TIMESTAMP - INTERVAL '1 second' WHERE operation_key = $1")
            .bind(key.as_bytes().as_slice()).execute(&pool).await.unwrap();
        let confirmed = cases.confirm_registration(&alias, &code).await;
        assert!(
            confirmed.is_ok(),
            "case alias confirmation failed: {confirmed:?}"
        );
    }

    #[tokio::test]
    async fn security_auth_limits_cleanup_is_bounded_and_preserves_active_budget() {
        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        sqlx::query(
            r#"INSERT INTO auth_code_attempts (operation_key, attempts, window_ends_at)
               SELECT decode(lpad(to_hex(n), 64, '0'), 'hex'), 1,
                      CURRENT_TIMESTAMP - INTERVAL '1 hour'
               FROM generate_series(1, 65) AS n"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let active_email = EmailAddress::from("expired-pending@example.com");
        let active_key = auth_attempt_key(
            SECRET,
            "registration-code:v1",
            &[active_email.as_str().as_bytes()],
        );
        sqlx::query("INSERT INTO auth_code_attempts (operation_key, attempts, window_ends_at) VALUES ($1, 4, CURRENT_TIMESTAMP + INTERVAL '1 hour')")
            .bind(active_key.as_bytes().as_slice()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO pending_user_registrations (email, username, password_hash, confirmation_code_hash, expires_at) VALUES ($1, 'expired-pending', 'hash', 'code', CURRENT_TIMESTAMP - INTERVAL '1 second')")
            .bind(active_email.as_str()).execute(&pool).await.unwrap();
        cleanup_pending_registrations(&pool).await.unwrap();
        cleanup_code_attempts(&pool).await.unwrap();
        let expired: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM auth_code_attempts WHERE window_ends_at <= CURRENT_TIMESTAMP",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            expired, 1,
            "one cleanup pass deletes no more than 64 expired rows"
        );
        let pending: i64 =
            sqlx::query_scalar("SELECT count(*) FROM pending_user_registrations WHERE email = $1")
                .bind(active_email.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(pending, 0);
        let active: i16 =
            sqlx::query_scalar("SELECT attempts FROM auth_code_attempts WHERE operation_key = $1")
                .bind(active_key.as_bytes().as_slice())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            active, 4,
            "pending expiry does not erase a live operation budget"
        );
        cleanup_code_attempts(&pool).await.unwrap();
        let expired: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM auth_code_attempts WHERE window_ends_at <= CURRENT_TIMESTAMP",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(expired, 0);
    }

    #[tokio::test]
    async fn security_auth_limits_competing_login_reservations_share_the_last_slot() {
        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        let first = PostgresPersistence::new(pool.clone());
        let second = PostgresPersistence::new(pool.clone());
        let identity = format!("missing-{}", Uuid::new_v4().simple());
        let key = auth_attempt_key(
            SECRET,
            "password-login:v1",
            &[identity.as_bytes(), b"192.0.2.10"],
        );
        let other_source = auth_attempt_key(
            SECRET,
            "password-login:v1",
            &[identity.as_bytes(), b"192.0.2.11"],
        );
        for _ in 0..4 {
            assert!(first.reserve_login_attempt(&key).await.unwrap());
        }
        let (a, b) = tokio::join!(
            first.reserve_login_attempt(&key),
            second.reserve_login_attempt(&key),
        );
        assert_eq!(usize::from(a.unwrap()) + usize::from(b.unwrap()), 1);
        assert!(!first.reserve_login_attempt(&key).await.unwrap());
        assert!(
            second.reserve_login_attempt(&other_source).await.unwrap(),
            "a noisy source cannot lock the identity from another source"
        );
    }

    #[tokio::test]
    async fn security_auth_limits_login_counter_is_shared_and_success_resets_it() {
        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        let first = Arc::new(PostgresPersistence::new(pool.clone()));
        let second = Arc::new(PostgresPersistence::new(pool.clone()));
        let name = format!("login_{}", Uuid::new_v4().simple());
        let email = format!("Älice_{name}@example.com");
        let hasher = Arc::new(crate::infra::argon2_password_hasher());
        let hash = hasher.hash_password("correct-password").unwrap();
        first.create_user(&name, &email, &hash).await.unwrap();
        let cases = [
            UserUseCases::new(hasher.clone(), first, SECRET),
            UserUseCases::new(hasher, second, SECRET),
        ];
        let source: IpAddr = "192.0.2.10".parse().unwrap();
        for index in 0..5 {
            assert!(matches!(
                cases[index % 2]
                    .login(&email, &SecretString::from("wrong"), Some(source), None)
                    .await,
                Err(crate::app_error::AppError::InvalidCredentials)
            ));
        }
        assert!(matches!(
            cases[0]
                .login(
                    &email.to_lowercase(),
                    &SecretString::from("correct-password"),
                    Some(source),
                    None,
                )
                .await,
            Err(crate::app_error::AppError::InvalidCredentials)
        ));
        let canonical = email.to_lowercase();
        let key = auth_attempt_key(
            SECRET,
            "password-login:v1",
            &[canonical.as_bytes(), source.to_string().as_bytes()],
        );
        let attempts: i16 = sqlx::query_scalar(
            "SELECT attempts FROM auth_login_attempts WHERE identity_source_key = $1",
        )
        .bind(key.as_bytes().as_slice())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(attempts, 5);

        sqlx::query("UPDATE auth_login_attempts SET window_ends_at = CURRENT_TIMESTAMP - INTERVAL '1 second' WHERE identity_source_key = $1")
            .bind(key.as_bytes().as_slice()).execute(&pool).await.unwrap();
        let recovered = cases[1]
            .login(
                &email.to_ascii_uppercase(),
                &SecretString::from("correct-password"),
                Some(source),
                None,
            )
            .await;
        assert!(recovered.is_ok(), "case alias login failed: {recovered:?}");
        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM auth_login_attempts WHERE identity_source_key = $1",
        )
        .bind(key.as_bytes().as_slice())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0, "successful login clears the shared cooldown");
    }

    #[tokio::test]
    async fn security_auth_limits_public_burst_and_cooldown_recovery() {
        use axum::{
            Extension, Json, Router,
            body::Body,
            extract::connect_info::ConnectInfo,
            http::{Request, StatusCode},
            middleware,
            routing::post,
        };
        use serde::Deserialize;
        use tower::ServiceExt;

        use crate::adapters::http::auth_admission::{self, AuthAdmission, AuthClientSource};
        use crate::use_cases::user::AuthWorkLifetime;

        #[derive(Deserialize)]
        struct LoginInput {
            email: String,
            password: String,
        }

        fn request(peer: &str, email: &str, password: &str) -> Request<Body> {
            let mut request = Request::post("/api/json/user/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"email": email, "password": password}).to_string(),
                ))
                .unwrap();
            request
                .extensions_mut()
                .insert(ConnectInfo(peer.parse::<std::net::SocketAddr>().unwrap()));
            request
        }

        let Some(database) = own_database().await else {
            return;
        };
        let pool = database.pool.clone();
        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
        let hasher = Arc::new(crate::infra::argon2_password_hasher());
        let hash = hasher.hash_password("correct-password").unwrap();
        let first_name = format!("burst_{}", Uuid::new_v4().simple());
        let first_email = format!("{first_name}@example.com");
        let second_name = format!("healthy_{}", Uuid::new_v4().simple());
        let second_email = format!("{second_name}@example.com");
        persistence
            .create_user(&first_name, &first_email, &hash)
            .await
            .unwrap();
        persistence
            .create_user(&second_name, &second_email, &hash)
            .await
            .unwrap();
        let users = Arc::new(UserUseCases::new(hasher, persistence.clone(), SECRET));
        let app = Router::new()
            .route(
                "/api/json/user/login",
                post(
                    move |Extension(source): Extension<AuthClientSource>,
                          Extension(work): Extension<AuthWorkLifetime>,
                          Json(input): Json<LoginInput>| {
                        let users = users.clone();
                        async move {
                            users
                                .login(
                                    &input.email,
                                    &SecretString::from(input.password),
                                    Some(source.0),
                                    Some(work),
                                )
                                .await
                                .map(|_| StatusCode::OK)
                        }
                    },
                ),
            )
            .layer(middleware::from_fn_with_state(
                Arc::new(AuthAdmission::new(vec![])),
                auth_admission::admit,
            ));

        let noisy_peer = "192.0.2.10:1001";
        let healthy_peer = "192.0.2.11:1001";
        for _ in 0..5 {
            assert_eq!(
                app.clone()
                    .oneshot(request(noisy_peer, &first_email, "wrong"))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            app.clone()
                .oneshot(request(noisy_peer, &first_email, "correct-password"))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED,
            "the fixed cooldown blocks the sixth candidate"
        );
        assert_eq!(
            app.clone()
                .oneshot(request(healthy_peer, &second_email, "correct-password"))
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
            "another source and identity remain usable"
        );

        let canonical = persistence
            .canonical_auth_identifier(&first_email)
            .await
            .unwrap();
        let source: IpAddr = "192.0.2.10".parse().unwrap();
        let key = auth_attempt_key(
            SECRET,
            "password-login:v1",
            &[canonical.as_bytes(), source.to_string().as_bytes()],
        );
        sqlx::query("UPDATE auth_login_attempts SET window_ends_at = CURRENT_TIMESTAMP - INTERVAL '1 second' WHERE identity_source_key = $1")
            .bind(key.as_bytes().as_slice()).execute(&pool).await.unwrap();
        assert_eq!(
            app.clone()
                .oneshot(request(noisy_peer, &first_email, "correct-password"))
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
            "the legitimate account recovers after its cooldown"
        );
        for _ in 0..23 {
            assert_eq!(
                app.clone()
                    .oneshot(request(noisy_peer, "missing@example.com", "wrong"))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            app.oneshot(request(noisy_peer, "missing@example.com", "wrong"))
                .await
                .unwrap()
                .status(),
            StatusCode::TOO_MANY_REQUESTS,
            "the public source burst is rejected before another hash"
        );
    }
}
