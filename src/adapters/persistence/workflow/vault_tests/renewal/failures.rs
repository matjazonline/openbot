use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn google_refresh_transient_failure_has_shared_bounded_cooldown_without_consent_change() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    *f.server.state.status.lock().unwrap() = StatusCode::TOO_MANY_REQUESTS;
    *f.server.state.reply.lock().unwrap() =
        Reply::Json(json!({"error":"rate_limit_exceeded","private":"raw-provider-error-sentinel"}));
    assert!(matches!(
        f.resolve().await,
        Err(ResolutionError::Retry(RetryReason::Cooldown, _))
    ));
    let saved = f.credential().await;
    assert_eq!(
        saved.versions.authorization,
        f.account.authorization_revision
    );
    assert_eq!(saved.versions.generation, CredentialGeneration::INITIAL);
    assert_eq!(saved.versions.revision.get(), 2);
    let value: Value = serde_json::from_str(saved.payload.expose_secret()).unwrap();
    assert!(
        !saved
            .payload
            .expose_secret()
            .contains("raw-provider-error-sentinel")
    );
    let retry = value["refresh_retry_at"]
        .as_str()
        .unwrap()
        .parse::<chrono::DateTime<Utc>>()
        .unwrap();
    assert!(retry > Utc::now() && retry <= Utc::now() + ChronoDuration::seconds(60));
    let (i, c) = prepared(&f.setup).await;
    let next = f.separate_service().await;
    assert!(matches!(
        next.vault.resolve(&i, &c).await,
        Err(ResolutionError::Retry(RetryReason::Cooldown, Some(_)))
    ));
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn google_refresh_invalid_client_is_configuration_and_does_not_revoke_the_owner() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    *f.server.state.status.lock().unwrap() = StatusCode::BAD_REQUEST;
    *f.server.state.reply.lock().unwrap() =
        Reply::Json(json!({"error":"invalid_client","description":"client-secret-sentinel"}));
    assert_eq!(
        f.resolve().await.unwrap_err(),
        ResolutionError::Configuration
    );
    let saved = f.credential().await;
    assert_eq!(saved.account.status, AccountStatus::Ready);
    assert_eq!(saved.versions.revision, f.account.revision);
    assert_eq!(
        saved.versions.authorization,
        f.account.authorization_revision
    );
}
#[tokio::test]
async fn google_refresh_invalid_grant_locally_requires_fresh_consent_and_erases_tokens() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let (old, context) = prepared(&f.setup).await;
    *f.server.state.status.lock().unwrap() = StatusCode::BAD_REQUEST;
    *f.server.state.reply.lock().unwrap() = Reply::Json(json!({"error":"invalid_grant"}));
    assert_eq!(
        f.resolve().await.unwrap_err(),
        ResolutionError::ReconnectRequired
    );
    let current = f.vault.get(&f.account.owner, f.account.id).await.unwrap();
    assert_eq!(current.status, AccountStatus::ReconnectRequired);
    assert!(current.authorization_revision.get() > f.account.authorization_revision.get());
    assert!(
        f.vault
            .read_usable(&f.account.owner, f.account.id)
            .await
            .is_err()
    );
    let grants = f
        .vault
        .grants(&f.account.owner, f.account.id, None)
        .await
        .unwrap();
    assert!(grants.grants.iter().all(|g| !g.enabled));
    assert!(f.vault.resolve(&old, &context).await.is_err());
    assert_eq!(f.server.calls(), 1);
}
#[derive(Clone, Copy)]
enum BadReply {
    Malformed,
    Oversized,
    Short,
    Header,
    Lifetime,
    Type,
}
impl BadReply {
    fn plan(self) -> Reply {
        let mut value =
            json!({"access_token":"new-access-sentinel","expires_in":3600,"token_type":"Bearer"});
        match self {
            Self::Malformed => return Reply::Raw("{invalid-json".into()),
            Self::Oversized => value["access_token"] = json!("x".repeat(MAX_CREDENTIAL_BYTES + 1)),
            Self::Short => value["expires_in"] = json!(60),
            Self::Header => value["access_token"] = json!("secret\r\nInjected: yes"),
            Self::Lifetime => value["expires_in"] = json!(86401),
            Self::Type => value["token_type"] = json!("MAC"),
        }
        Reply::Json(value)
    }
}
#[tokio::test]
async fn google_refresh_malformed_oversized_short_or_unsafe_tokens_do_not_partially_write() {
    for case in [
        BadReply::Malformed,
        BadReply::Oversized,
        BadReply::Short,
        BadReply::Header,
        BadReply::Lifetime,
        BadReply::Type,
    ] {
        let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
        let before = f.credential().await;
        *f.server.state.reply.lock().unwrap() = case.plan();
        assert_eq!(
            f.resolve().await.unwrap_err(),
            ResolutionError::Integration(IntegrationError::Credential)
        );
        let after = f.credential().await;
        assert_eq!(before.versions, after.versions);
        assert_eq!(
            before.payload.expose_secret(),
            after.payload.expose_secret()
        );
        assert_eq!(f.server.calls(), 1);
    }
}
#[tokio::test]
async fn google_refresh_scope_drift_requires_reconnect_without_silently_upgrading_grants() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    *f.server.state.reply.lock().unwrap() = Reply::Json(
        json!({"access_token":"changed-access","expires_in":3600,"token_type":"Bearer","scope":"openid email https://www.googleapis.com/auth/drive"}),
    );
    assert_eq!(
        f.resolve().await.unwrap_err(),
        ResolutionError::ReconnectRequired
    );
    assert_eq!(
        f.vault
            .get(&f.account.owner, f.account.id)
            .await
            .unwrap()
            .status,
        AccountStatus::ReconnectRequired
    );
}
#[tokio::test]
async fn google_refresh_lost_response_releases_lock_and_persists_shared_backoff() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    *f.server.state.reply.lock().unwrap() = Reply::Lost;
    assert!(matches!(
        f.resolve().await,
        Err(ResolutionError::Retry(RetryReason::Cooldown, _))
    ));
    let current = f.credential().await;
    f.vault
        .disconnect(
            &current.account.owner,
            current.account.id,
            current.versions.authorization,
        )
        .await
        .unwrap();
    assert_eq!(f.server.calls(), 1);
}
#[tokio::test]
async fn google_refresh_actual_cancellation_rolls_back_and_keeps_no_detached_renewal() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let before = f.credential().await;
    let (i, c) = prepared(&f.setup).await;
    let cancellation = c.cancellation.clone();
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let worker = tokio::spawn(async move { service.vault.resolve(&i, &c).await });
    f.server.wait_calls(1).await;
    cancellation.cancel();
    assert_eq!(
        worker.await.unwrap().unwrap_err(),
        ResolutionError::Cancelled
    );
    let after = f.credential().await;
    assert_eq!(before.versions, after.versions);
    drop(held);
    f.vault
        .disconnect(
            &after.account.owner,
            after.account.id,
            after.versions.authorization,
        )
        .await
        .unwrap();
    assert_eq!(f.server.calls(), 1);
}
