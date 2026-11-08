use super::*;
use crate::adapters::persistence::{credentials::CredentialFormat, integrations::inventory};
#[tokio::test]
async fn vault_rotation_competing_cas_cannot_overwrite_a_real_provider_refresh() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let mut connection = f.vault.persistence.pool().acquire().await.unwrap();
    let old = inventory::page(&mut connection, None, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.id == f.account.id.as_uuid())
        .unwrap();
    drop(connection);
    let cipher = CredentialCipher::for_test_with_keys(&[(1, [42; 32]), (2, [23; 32])], 2);
    let rotated = cipher
        .rotate_to_active(
            &CredentialFormat::Envelope(old.context.clone()),
            &old.envelope,
        )
        .unwrap();
    let service = independent_service(&f.vault).await;
    let active = PostgresVault::with_registry(
        PostgresPersistence::with_credential_cipher(
            service.vault.persistence.pool().clone(),
            CredentialCipher::for_test_with_keys(&[(1, [42; 32]), (2, [23; 32])], 2),
        ),
        f.vault.registry.clone(),
    );
    let (i, c) = prepared(&f.setup).await;
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let resolving = active.clone();
    let refresh = tokio::spawn(async move { resolving.resolve(&i, &c).await });
    f.server.wait_calls(1).await;
    let rotating = independent_service(&f.vault).await;
    let name = rotating.application.clone();
    let cas = tokio::spawn(async move {
        let mut tx = rotating.vault.persistence.pool().begin().await.unwrap();
        let changed = inventory::rewrap(&mut tx, old.id, &old.envelope, &rotated)
            .await
            .unwrap()
            .rows_affected();
        tx.commit().await.unwrap();
        changed
    });
    wait_blocked(&f.vault, &name).await;
    drop(held);
    let fresh = refresh.await.unwrap().unwrap();
    assert_eq!(cas.await.unwrap(), 0);
    let saved = active
        .read_usable(&f.account.owner, f.account.id)
        .await
        .unwrap();
    assert_eq!(saved.versions.generation, fresh.generation);
    assert_eq!(
        saved.versions.authorization,
        f.account.authorization_revision
    );
    let payload: Value = serde_json::from_str(saved.payload.expose_secret()).unwrap();
    assert_eq!(payload["access_token"], "renewed-access-sentinel");
    let mut connection = active.persistence.pool().acquire().await.unwrap();
    let entries = inventory::page(&mut connection, None, 100).await.unwrap();
    assert!(entries.iter().all(|row| matches!(
        cipher.classify(
            &CredentialFormat::Envelope(row.context.clone()),
            &row.envelope
        ),
        crate::adapters::persistence::credentials::CredentialState::Active { version: 2 }
    )));
}
