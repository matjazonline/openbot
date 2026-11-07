use super::*;
#[tokio::test]
async fn google_refresh_absent_cache_uses_the_same_authorized_resolution_path() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let before = f.credential().await;
    let mut payload: Value = serde_json::from_str(before.payload.expose_secret()).unwrap();
    payload["access_token"] = Value::Null;
    payload["expires_at"] = Value::Null;
    payload["generation"] = json!(2);
    f.vault
        .write_managed(
            &f.account.owner,
            f.account.id,
            before.versions.revision,
            SecretString::from(payload.to_string()),
        )
        .await
        .unwrap();
    let result = f.resolve().await.unwrap();
    assert_eq!(result.generation.get(), 3);
    assert_eq!(f.server.calls(), 1);
    assert_eq!(
        f.credential().await.versions.authorization,
        f.account.authorization_revision
    );
}
#[tokio::test]
async fn integration_renewal_generation_drift_cannot_enter_a_cache_update() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let before = f.credential().await;
    let mut payload: Value = serde_json::from_str(before.payload.expose_secret()).unwrap();
    payload["generation"] = json!(99);
    assert_eq!(
        f.vault
            .write_managed(
                &f.account.owner,
                f.account.id,
                before.versions.revision,
                SecretString::from(payload.to_string())
            )
            .await
            .unwrap_err(),
        IntegrationError::Credential
    );
    let after = f.credential().await;
    assert_eq!(before.versions, after.versions);
    assert_eq!(
        before.payload.expose_secret(),
        after.payload.expose_secret()
    );
    assert_eq!(f.server.calls(), 0);
}
#[tokio::test]
async fn integration_renewal_initial_generation_must_match_the_durable_entry() {
    let f = Box::pin(RenewalSetup::new(Utc::now() + ChronoDuration::hours(1))).await;
    let mut payload: Value = serde_json::from_str(
        google_payload("bad-initial", Utc::now() + ChronoDuration::hours(1)).expose_secret(),
    )
    .unwrap();
    payload["generation"] = json!(2);
    let payload = SecretString::from(payload.to_string());
    let entry = f
        .vault
        .registry
        .get(&AppId::parse("gmail").unwrap())
        .unwrap();
    let write = AccountWrite {
        key: AccountKey::parse("bad-initial").unwrap(),
        provider: entry.provider.id.clone(),
        config: entry.config.id.clone(),
        method: entry.provider.method,
        schema: entry.provider.schema.clone(),
        identity: entry.adapter.validate(&payload).unwrap(),
        metadata: AccountMetadata::new("Bad initial".into(), None).unwrap(),
        payload,
    };
    assert_eq!(
        f.vault.create(&f.account.owner, write).await.unwrap_err(),
        IntegrationError::Credential
    );
    assert_eq!(
        f.vault
            .list(&f.account.owner, PageRequest::new(None, 50).unwrap())
            .await
            .unwrap()
            .accounts
            .len(),
        1
    );
}
#[tokio::test]
async fn integration_renewal_shared_app_cannot_change_the_authorizations_protocol() {
    let server = TokenServer::new().await;
    let mut registry = CodeOwnedRegistry::default();
    registry.register(server.google_entry()).unwrap();
    let mut other = server.google_entry();
    other.app.id = AppId::parse("unsafe-alias").unwrap();
    other.renewal = RenewalStrategy::NoRenewal;
    assert_eq!(
        registry.register(other).unwrap_err(),
        IntegrationError::Invalid
    );
    assert_eq!(server.calls(), 0);
}
#[tokio::test]
async fn google_refresh_missing_encryption_key_fails_closed_before_the_endpoint() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let cipher = CredentialCipher::for_test_with_keys(&[(2, [19; 32])], 2);
    let vault = PostgresVault::with_registry(
        PostgresPersistence::with_credential_cipher(
            f.setup.fixture.persistence.pool().clone(),
            cipher,
        ),
        f.vault.registry.clone(),
    );
    let (i, c) = prepared(&f.setup).await;
    assert_eq!(
        vault.resolve(&i, &c).await.unwrap_err(),
        ResolutionError::Integration(IntegrationError::Credential)
    );
    assert_eq!(f.server.calls(), 0);
    assert_eq!(
        f.credential().await.versions.generation,
        CredentialGeneration::INITIAL
    );
}
#[tokio::test]
async fn integration_renewal_api_key_injection_needs_no_refresh_token_or_oauth() {
    let s = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let vault = PostgresVault::new(s.fixture.persistence.clone());
    let app = AppId::parse("static-api-key").unwrap();
    let entry = vault.registry.get(&app).unwrap();
    let payload = SecretString::from("key-without-refresh");
    let account = vault
        .create(
            &owner_for_user(s.fixture.target.actor.user_id()),
            AccountWrite {
                key: AccountKey::parse("api-key").unwrap(),
                provider: entry.provider.id.clone(),
                config: entry.config.id.clone(),
                method: entry.provider.method,
                schema: entry.provider.schema.clone(),
                identity: entry.adapter.validate(&payload).unwrap(),
                metadata: AccountMetadata::new("API key".into(), None).unwrap(),
                payload,
            },
        )
        .await
        .unwrap();
    attach_app(&s, &vault, &account, app).await;
    let (i, c) = prepared(&s).await;
    let result = vault.resolve(&i, &c).await.unwrap();
    let InjectionMethod::ApiKeyHeader(header) = result.injection.method else {
        panic!("API key header required")
    };
    assert!(header.eq_ignore_ascii_case("X-API-Key"));
    assert_eq!(
        result.injection.value.expose_secret(),
        "key-without-refresh"
    );
    assert!(result.expires_at.is_none());
}

#[tokio::test]
async fn google_connection_lifecycle_owner_controls_keep_consent_revision_after_cache_update() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    f.resolve().await.unwrap();
    let current = f.vault.get(&f.account.owner, f.account.id).await.unwrap();
    assert_eq!(current.revision.get(), 2);
    assert_eq!(current.authorization_revision.get(), 1);
    let html = crate::adapters::http::pages::vault::vault_content(
        &AccountPage {
            accounts: vec![current],
            next: None,
        },
        None,
        &f.vault.registry.connectable(),
    );
    assert_eq!(
        html.matches("name=\"expected_revision\" value=\"1\"")
            .count(),
        2
    );
    assert!(html.contains("Company grants require fresh confirmation after reconnecting."));
    for secret in ["renewed-access-sentinel", "refresh-sentinel", "enc:v2:"] {
        assert!(!html.contains(secret));
    }
}
