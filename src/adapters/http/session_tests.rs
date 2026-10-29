use super::*;
use crate::{
    adapters::persistence::{PostgresPersistence, test_support::own_database},
    use_cases::user::UserPersistence,
};

#[tokio::test]
async fn security_session_signature_and_signed_without_row() {
    let sessions = SessionAuthority::for_test(&AppConfig::for_test());
    let now = Utc::now().timestamp();
    let claims = SessionClaims {
        sub: Uuid::new_v4(),
        jti: SessionId(Uuid::new_v4()),
        rev: 0,
        iat: now,
        exp: now + 60,
    };
    let token = encode(&Header::new(Algorithm::HS256), &claims, &sessions.encoding).unwrap();
    assert!(sessions.decode_session(&token).is_some());
    assert!(sessions.verify(&token).await.unwrap().is_none());
    let other_key = EncodingKey::from_secret(b"another-long-enough-secret-0123456789");
    let forged = encode(&Header::new(Algorithm::HS256), &claims, &other_key).unwrap();
    assert!(sessions.decode_session(&forged).is_none());
    let mut tampered = token.into_bytes();
    tampered[10] = if tampered[10] == b'a' { b'b' } else { b'a' };
    assert!(
        sessions
            .decode_session(std::str::from_utf8(&tampered).unwrap())
            .is_none()
    );
}

#[tokio::test]
async fn security_session_cookie_flags_and_independent_ids() {
    let config = AppConfig {
        secure_cookies: true,
        ..AppConfig::for_test()
    };
    let sessions = SessionAuthority::for_test(&config);
    let user = test_support::account(Uuid::new_v4());
    let first = sessions.cookie(&user).await.unwrap();
    let second = sessions.cookie(&user).await.unwrap();
    assert_ne!(first.value(), second.value());
    assert_eq!(first.http_only(), Some(true));
    assert_eq!(first.secure(), Some(true));
    assert_eq!(first.same_site(), Some(SameSite::Lax));
    assert_eq!(first.max_age(), Some(config.refresh_token_ttl));
    assert_eq!(first.path(), Some("/"));
    assert_eq!(
        sessions
            .verify(first.value())
            .await
            .unwrap()
            .unwrap()
            .user_id,
        user.id
    );
}

#[tokio::test]
async fn security_session_strict_claims_and_purpose() {
    let sessions = SessionAuthority::for_test(&AppConfig::for_test());
    let now = Utc::now().timestamp();
    let base = serde_json::json!({"sub": Uuid::new_v4(), "jti": Uuid::new_v4(), "rev": 0,
        "iat": now, "exp": now + 60});
    for field in ["sub", "jti", "rev", "iat", "exp"] {
        let mut claims = base.clone();
        claims.as_object_mut().unwrap().remove(field);
        let token = encode(&Header::new(Algorithm::HS256), &claims, &sessions.encoding).unwrap();
        assert!(sessions.decode_session(&token).is_none(), "missing {field}");
        assert!(
            sessions.verify(&token).await.unwrap().is_none(),
            "missing {field}"
        );
    }
    for (field, value) in [
        ("iat", now + 60),
        ("exp", now),
        ("exp", now + 366 * 86400),
        ("rev", -1),
    ] {
        let mut claims = base.clone();
        claims[field] = value.into();
        let token = encode(&Header::new(Algorithm::HS256), &claims, &sessions.encoding).unwrap();
        assert!(sessions.decode_session(&token).is_none(), "invalid {field}");
    }
    let token = encode(&Header::new(Algorithm::HS384), &base, &sessions.encoding).unwrap();
    assert!(sessions.decode_session(&token).is_none());
    assert!(
        sessions
            .decode_session(&"a".repeat(MAX_SESSION_TOKEN_BYTES + 1))
            .is_none()
    );
    let oauth = sessions.issue_oauth_state("google", "login", None, "nonce".into());
    let token = sessions.encode_oauth_state(&oauth);
    assert!(sessions.decode_session(&token).is_none());
    assert_eq!(sessions.verify_oauth_state(&token).unwrap().nonce, "nonce");
    let mut headers = HeaderMap::new();
    headers.insert(
        "cookie",
        format!("user_id={}", Uuid::new_v4()).parse().unwrap(),
    );
    assert!(
        sessions
            .user_from_headers(&headers)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn security_session_database_round_trip_absent_revoked_and_expired() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
        .await
        .unwrap();
    let sessions = SessionAuthority::new(&AppConfig::for_test(), persistence.clone());
    let cookie = sessions.cookie(&user).await.unwrap();
    let live = sessions.verify(cookie.value()).await.unwrap().unwrap();
    assert_eq!(live.user_id, user.id);
    let row = sessions.decode_session(cookie.value()).unwrap();
    let stored: (Uuid, i64, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT user_id, auth_revision, issued_at, expires_at, revoked_at FROM user_sessions WHERE id = $1")
        .bind(row.id.0).fetch_one(&database.pool).await.unwrap();
    assert_eq!(stored.0, user.id);
    assert_eq!(stored.1, user.auth_revision);
    assert_eq!(stored.2, row.issued_at);
    assert_eq!(stored.3, row.expires_at);
    assert!(stored.4.is_none());
    sessions.revoke(live).await.unwrap();
    assert!(sessions.verify(cookie.value()).await.unwrap().is_none());
    let absent = sessions.cookie(&user).await.unwrap();
    let row = sessions.decode_session(absent.value()).unwrap();
    sqlx::query("DELETE FROM user_sessions WHERE id = $1")
        .bind(row.id.0)
        .execute(&database.pool)
        .await
        .unwrap();
    assert!(
        sessions.verify(absent.value()).await.unwrap().is_none(),
        "signed token without a row"
    );
    let now = Utc::now().timestamp();
    let expired = SessionClaims {
        sub: user.id,
        jti: SessionId(Uuid::new_v4()),
        rev: user.auth_revision,
        iat: now - 120,
        exp: now - 1,
    };
    let token = encode(&Header::new(Algorithm::HS256), &expired, &sessions.encoding).unwrap();
    let record = record_from_claims(expired).unwrap();
    persistence.create_session(&record).await.unwrap();
    assert!(!persistence.session_is_live(&record).await.unwrap());
    assert!(sessions.verify(&token).await.unwrap().is_none());
    let invalid = SessionRecord {
        expires_at: record.issued_at,
        ..record.clone()
    };
    assert!(
        persistence.create_session(&invalid).await.is_err(),
        "database enforces lifetime"
    );
}

#[tokio::test]
async fn security_session_mismatched_row_revision_and_failed_insert() {
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
        .await
        .unwrap();
    let sessions = SessionAuthority::new(&AppConfig::for_test(), persistence.clone());
    let cookie = sessions.cookie(&user).await.unwrap();
    let mut record = sessions.decode_session(cookie.value()).unwrap();
    record.auth_revision += 1;
    assert!(!persistence.session_is_live(&record).await.unwrap());
    record.auth_revision -= 1;
    record.expires_at += chrono::Duration::seconds(1);
    assert!(!persistence.session_is_live(&record).await.unwrap());
    let stale = User {
        auth_revision: user.auth_revision + 1,
        ..user.clone()
    };
    assert!(matches!(
        sessions.cookie(&stale).await,
        Err(AppError::InvalidCredentials)
    ));
    database.pool.close().await;
    assert!(matches!(
        sessions.cookie(&user).await,
        Err(AppError::ServiceUnavailable(_))
    ));
    assert!(matches!(
        sessions.verify(cookie.value()).await,
        Err(AppError::ServiceUnavailable(_))
    ));
}

#[tokio::test]
async fn security_session_oauth_accounts_persist_before_cookie() {
    use crate::{
        entities::value_objects::EmailAddress,
        use_cases::user::{ExternalIdentity, LoginMethod},
    };
    let Some(database) = own_database().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
    let sessions = SessionAuthority::new(&AppConfig::for_test(), persistence.clone());
    for provider in [LoginMethod::Google, LoginMethod::Apple] {
        let subject = Uuid::new_v4().to_string();
        let email = EmailAddress::from(format!("{subject}@example.test"));
        let user = persistence
            .create_external_user(ExternalIdentity {
                provider,
                subject: &subject,
                email: &email,
                email_verified: true,
                display_name: None,
            })
            .await
            .unwrap();
        let cookie = sessions.cookie(&user).await.unwrap();
        let verified = sessions.verify(cookie.value()).await.unwrap().unwrap();
        assert_eq!(verified.user_id, user.id);
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_sessions WHERE id = $1 AND user_id = $2)",
        )
        .bind(verified.session_id.0)
        .bind(user.id)
        .fetch_one(&database.pool)
        .await
        .unwrap();
        assert!(exists);
    }
}
