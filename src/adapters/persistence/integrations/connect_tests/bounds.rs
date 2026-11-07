use super::*;
#[tokio::test]
async fn integration_connect_cap_has_real_competing_creators() {
    let f = Box::pin(Fixture::new()).await;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..9 {
        let service = f.service.clone();
        let owner = f.owner.clone();
        tasks.spawn(async move { service.begin_connect(&owner, command(None)).await });
    }
    let mut accepted = 0;
    let mut limited = 0;
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            Ok(_) => accepted += 1,
            Err(IntegrationError::Limit) => limited += 1,
            Err(error) => panic!("unexpected sanitized error {error:?}"),
        }
    }
    assert_eq!(accepted, 8);
    assert_eq!(limited, 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM integration_connect_attempts")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(count, 8);
}
#[tokio::test]
async fn integration_connect_retains_only_bounded_terminal_outcomes() {
    let f = Box::pin(Fixture::new()).await;
    for _ in 0..40 {
        let start = f.begin(None).await;
        let cancelled = f
            .service
            .cancel_connect(&f.owner, start.attempt.id)
            .await
            .unwrap();
        assert_eq!(cancelled.status, ConnectStatus::Cancelled);
    }
    #[derive(sqlx::FromRow)]
    struct RetentionCounts {
        total: i64,
        material: i64,
    }
    let counts: RetentionCounts = sqlx::query_as(
        "SELECT count(*) AS total,count(envelope) AS material FROM integration_connect_attempts",
    )
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(counts.total, 32);
    assert_eq!(counts.material, 0);
}
#[tokio::test]
async fn integration_connect_expiry_erases_verifier_and_cannot_complete() {
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let mut facts = f.service.codec.verify(&f.callback(&start).state).unwrap();
    facts.id = ConnectAttemptId::new(Uuid::new_v4());
    facts.nonce = ConnectNonce::parse(Uuid::new_v4().to_string()).unwrap();
    facts.expires_at = Utc::now() + chrono::Duration::milliseconds(400);
    let attempt = f
        .vault
        .start(&facts, SecretString::from("v".repeat(64)))
        .await
        .unwrap();
    await_time(f.pool(), facts.expires_at).await;
    assert_eq!(
        f.service
            .get_connect_attempt(&f.owner, attempt.id)
            .await
            .unwrap()
            .status,
        ConnectStatus::Expired
    );
    let envelope: Option<String> =
        sqlx::query_scalar("SELECT envelope FROM integration_connect_attempts WHERE id=$1")
            .bind(attempt.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert!(envelope.is_none());
    assert!(f.vault.consume(&facts).await.is_err());
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn integration_connect_wrong_browser_provider_purpose_signature_and_apps_deny() {
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let mut c = f.callback(&start);
    c.provider = ProviderId::parse("foreign").unwrap();
    assert!(
        f.service
            .finish_connect(&f.owner, c, work(CancellationToken::new()))
            .await
            .is_err()
    );
    let fake = VerifiedOwner::for_browser(
        f.owner.owner().clone(),
        ConnectBrowser {
            session: ConnectSessionReference::parse(Uuid::new_v4().to_string()).unwrap(),
            binding: BrowserBinding::parse("different-browser").unwrap(),
        },
    );
    assert!(
        f.service
            .finish_connect(&fake, f.callback(&start), work(CancellationToken::new()))
            .await
            .is_err()
    );
    let mut c = f.callback(&start);
    let state =
        f.sessions
            .issue_oauth_state("google", "login", Some(f.session.user_id), "nonce".into());
    c.state = f.sessions.encode_oauth_state(&state);
    assert!(
        f.service
            .finish_connect(&f.owner, c, work(CancellationToken::new()))
            .await
            .is_err()
    );
    let mut c = f.callback(&start);
    c.state.push('x');
    assert!(
        f.service
            .finish_connect(&f.owner, c, work(CancellationToken::new()))
            .await
            .is_err()
    );
    let mut bad = command(None);
    bad.app = AppId::parse("unregistered").unwrap();
    assert!(f.service.begin_connect(&f.owner, bad).await.is_err());
    let mut bad = command(None);
    bad.config = AuthConfigId::parse("foreign").unwrap();
    assert!(f.service.begin_connect(&f.owner, bad).await.is_err());
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn integration_connect_ciphertext_context_substitution_fails_and_consumes_material() {
    let f = Box::pin(Fixture::new()).await;
    let a = f.begin(None).await;
    let b = f.begin(None).await;
    sqlx::query("UPDATE integration_connect_attempts SET envelope=(SELECT envelope FROM integration_connect_attempts WHERE id=$1) WHERE id=$2").bind(a.attempt.id.as_uuid()).bind(b.attempt.id.as_uuid()).execute(f.pool()).await.unwrap();
    assert!(
        f.service
            .finish_connect(&f.owner, f.callback(&b), work(CancellationToken::new()))
            .await
            .is_err()
    );
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
    let envelope: Option<String> =
        sqlx::query_scalar("SELECT envelope FROM integration_connect_attempts WHERE id=$1")
            .bind(b.attempt.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert!(envelope.is_none());
    assert_eq!(f.finish(&a).await.status, ConnectStatus::Connected);
}
pub(super) async fn await_time(pool: &sqlx::PgPool, at: chrono::DateTime<Utc>) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1")
                .bind(at)
                .fetch_one(pool)
                .await
                .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn vault_rotation_connect_attempt_competes_with_consumption_without_resurrecting_pkce() {
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let p = PostgresPersistence::with_credential_cipher(
        f.pool().clone(),
        CredentialCipher::for_test_with_keys(&[(1, [7; 32]), (2, [8; 32])], 2),
    );
    let vault = PostgresVault::with_registry(p.clone(), f.vault.registry.clone());
    let service = f.service_on(vault);
    let mut held = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM integration_connect_attempts WHERE id=$1 FOR UPDATE")
        .bind(start.attempt.id.as_uuid())
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let owner = f.owner.clone();
    let callback = f.callback(&start);
    let worker = tokio::spawn(async move {
        service
            .finish_connect(&owner, callback, work(CancellationToken::new()))
            .await
    });
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(f.pool(), 1).await;
    let rotator = tokio::spawn(async move { p.rotate_credentials().await });
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(f.pool(), 2).await;
    held.rollback().await.unwrap();
    assert_eq!(
        worker.await.unwrap().unwrap().status,
        ConnectStatus::Connected
    );
    assert!(rotator.await.unwrap().unwrap().complete);
    let envelope: Option<String> =
        sqlx::query_scalar("SELECT envelope FROM integration_connect_attempts WHERE id=$1")
            .bind(start.attempt.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert!(envelope.is_none());
}

#[tokio::test]
async fn integration_connect_required_host_cannot_skip_atomic_finalizer() {
    struct Skip;
    #[async_trait::async_trait]
    impl ConnectAuthority for Skip {
        async fn complete(
            &self,
            _: &VerifiedOwner,
            _: &dyn ConnectFinalizer,
        ) -> IntegrationResult<()> {
            Ok(())
        }
    }
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let service = ConnectService {
        registry: f.service.registry.clone(),
        store: f.service.store.clone(),
        codec: f.service.codec.clone(),
        authority: Arc::new(Skip),
    };
    assert!(
        service
            .finish_connect(&f.owner, f.callback(&start), work(CancellationToken::new()))
            .await
            .is_err()
    );
    assert!(
        f.vault
            .list(f.owner.owner(), PageRequest::new(None, 50).unwrap())
            .await
            .unwrap()
            .accounts
            .is_empty()
    );
    assert_eq!(
        f.service
            .get_connect_attempt(&f.owner, start.attempt.id)
            .await
            .unwrap()
            .status,
        ConnectStatus::Processing
    );
}
#[tokio::test]
async fn integration_connect_static_api_key_uses_registered_account_lifecycle_without_redirect() {
    let f = Box::pin(Fixture::new()).await;
    let account = f
        .gateway()
        .create_static_account(
            &f.owner,
            StaticAccountInput {
                key: AccountKey::parse("api-key").unwrap(),
                app: AppId::parse("static-api-key").unwrap(),
                metadata: AccountMetadata::new("Static API key".into(), None).unwrap(),
                value: SecretString::from(r#"{"api_key":"static-sentinel"}"#),
            },
        )
        .await
        .unwrap();
    assert_eq!(account.auth_method, AuthMethod::StaticSecret);
    assert_eq!(account.status, AccountStatus::Ready);
    let mut bad = command(None);
    bad.app = AppId::parse("static-api-key").unwrap();
    assert!(f.service.begin_connect(&f.owner, bad).await.is_err());
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn integration_connect_unavailable_key_cannot_exchange_or_restore_verifier() {
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let p = PostgresPersistence::with_credential_cipher(
        f.pool().clone(),
        CredentialCipher::for_test_with_keys(&[(2, [8; 32])], 2),
    );
    let service = f.service_on(PostgresVault::with_registry(p, f.vault.registry.clone()));
    assert!(
        service
            .finish_connect(&f.owner, f.callback(&start), work(CancellationToken::new()))
            .await
            .is_err()
    );
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
    let envelope: Option<String> =
        sqlx::query_scalar("SELECT envelope FROM integration_connect_attempts WHERE id=$1")
            .bind(start.attempt.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert!(envelope.is_none());
}

#[tokio::test]
async fn integration_connect_verifier_context_binds_owner_provider_config_app_and_purpose() {
    use crate::adapters::persistence::credentials::envelope::CredentialContext;
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let facts = f.service.codec.verify(&f.callback(&start).state).unwrap();
    let envelope: String =
        sqlx::query_scalar("SELECT envelope FROM integration_connect_attempts WHERE id=$1")
            .bind(start.attempt.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    let mut substitutions = Vec::new();
    let mut changed = facts.clone();
    changed.owner.subject = OwnerSubject::parse("foreign").unwrap();
    substitutions.push(changed);
    let mut changed = facts.clone();
    changed.provider = ProviderId::parse("foreign").unwrap();
    substitutions.push(changed);
    let mut changed = facts.clone();
    changed.config = AuthConfigId::parse("foreign").unwrap();
    substitutions.push(changed);
    let mut changed = facts.clone();
    changed.app = AppId::parse("foreign").unwrap();
    substitutions.push(changed);
    let mut changed = facts.clone();
    changed.reconnect = Some(ReconnectTarget {
        account: AccountId::new(Uuid::new_v4()),
        authorization: AuthorizationRevision::INITIAL,
    });
    substitutions.push(changed);
    for changed in substitutions {
        assert!(
            f.vault
                .persistence
                .credential_cipher()
                .unwrap()
                .open_envelope(&CredentialContext::integration_connect(&changed), &envelope)
                .is_err()
        );
    }
}
