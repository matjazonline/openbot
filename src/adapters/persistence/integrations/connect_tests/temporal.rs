use super::*;
#[derive(Clone, Copy)]
enum SessionBound {
    Expires,
    Valid,
}
async fn waiting_finalizer(bound: SessionBound) {
    let lifetime = match bound {
        SessionBound::Expires => time::Duration::seconds(2),
        SessionBound::Valid => time::Duration::hours(24),
    };
    let f = Box::pin(Fixture::with_lifetime(lifetime)).await;
    let start = f.begin(None).await;
    let permit = f.script.gate.clone().acquire_owned().await.unwrap();
    let worker = f.spawn_finish(&start, CancellationToken::new());
    f.script.wait_calls(1).await;
    let expiry: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT expires_at FROM user_sessions WHERE id=$1")
            .bind(f.session.session_id.0)
            .fetch_one(f.pool())
            .await
            .unwrap();
    let mut held = f.pool().begin().await.unwrap();
    sqlx::query("SELECT owner_subject FROM integration_owner_states WHERE owner_namespace=$1 AND owner_subject=$2 FOR UPDATE").bind(f.owner.owner().namespace.as_str()).bind(f.owner.owner().subject.as_str()).fetch_one(&mut *held).await.unwrap();
    drop(permit);
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(f.pool(), 1).await;
    if matches!(bound, SessionBound::Expires) {
        super::bounds::await_time(f.pool(), expiry).await;
    }
    held.rollback().await.unwrap();
    let done = worker.await.unwrap();
    match bound {
        SessionBound::Expires => {
            assert!(done.is_err());
            assert!(
                f.vault
                    .list(f.owner.owner(), PageRequest::new(None, 50).unwrap())
                    .await
                    .unwrap()
                    .accounts
                    .is_empty()
            );
        }
        SessionBound::Valid => assert_eq!(done.unwrap().status, ConnectStatus::Connected),
    }
}
#[tokio::test]
async fn google_vault_callback_session_expiry_during_module_lock_denies() {
    Box::pin(waiting_finalizer(SessionBound::Expires)).await;
}
#[tokio::test]
async fn google_vault_callback_session_valid_during_module_lock_succeeds() {
    Box::pin(waiting_finalizer(SessionBound::Valid)).await;
}

#[tokio::test]
async fn google_vault_callback_expiry_cancels_account_write_wait_without_partial_account() {
    let f = Box::pin(Fixture::new()).await;
    let original = f.begin(None).await;
    let mut facts = f
        .service
        .codec
        .verify(&f.callback(&original).state)
        .unwrap();
    facts.id = ConnectAttemptId::new(Uuid::new_v4());
    facts.nonce = ConnectNonce::parse(Uuid::new_v4().to_string()).unwrap();
    facts.expires_at = Utc::now() + chrono::Duration::seconds(2);
    f.vault
        .start(&facts, SecretString::from("v".repeat(64)))
        .await
        .unwrap();
    let callback = ConnectCallback {
        provider: ProviderId::parse("google").unwrap(),
        state: f.service.codec.sign(&facts).unwrap(),
        code: Some(SecretString::from("code-sentinel")),
        error: None,
    };
    let mut held = f.pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE integration_accounts IN SHARE MODE")
        .execute(&mut *held)
        .await
        .unwrap();
    let service = f.service.clone();
    let owner = f.owner.clone();
    let worker = tokio::spawn(async move {
        service
            .finish_connect(&owner, callback, work(CancellationToken::new()))
            .await
    });
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(f.pool(), 1).await;
    let result = tokio::time::timeout(std::time::Duration::from_millis(2500), worker)
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_err());
    held.rollback().await.unwrap();
    assert!(
        f.vault
            .list(f.owner.owner(), PageRequest::new(None, 50).unwrap())
            .await
            .unwrap()
            .accounts
            .is_empty()
    );
}
