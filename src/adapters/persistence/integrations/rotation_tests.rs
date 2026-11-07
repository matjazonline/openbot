use super::*;
use crate::adapters::persistence::{
    credentials::CredentialFormat,
    test_support::{wait_until_a_backend_is_blocked, wait_until_backends_are_blocked},
};
#[tokio::test]
async fn vault_rotation_inventory_keysets_reconnect_rows_and_retirement() {
    let f = Fixture::isolated().await;
    for index in 0..101 {
        f.create(&format!("rotation-{index}")).await;
    }
    let account = f
        .vault
        .list(&f.owner, PageRequest::new(None, 1).unwrap())
        .await
        .unwrap()
        .accounts
        .remove(0);
    sqlx::query("UPDATE integration_accounts SET status='reconnect_required',revision=2,authorization_revision=2,credential_generation=2 WHERE id=$1").bind(account.id.as_uuid()).execute(f.pool()).await.unwrap();
    assert!(f.vault.read_usable(&f.owner, account.id).await.is_err());
    let rotating = f.rotating();
    let before = rotating.credential_status().await.unwrap();
    assert_eq!(before.total_rows, 101);
    assert_eq!(before.old_rows, 101);
    let report = rotating.rotate_credentials().await.unwrap();
    assert!(report.complete);
    assert_eq!(report.rotated, 101);
    assert!(report.batches >= 2);
    let account_after = f.vault.get(&f.owner, account.id).await.unwrap();
    assert_eq!(account_after.revision.get(), 2);
    assert_eq!(account_after.authorization_revision.get(), 2);
    let retired = PostgresPersistence::with_credential_cipher(
        f.pool().clone(),
        CredentialCipher::for_test_with_keys(&[(2, [8; 32])], 2),
    );
    assert!(
        retired
            .credential_status()
            .await
            .unwrap()
            .satisfies_required_version(2)
    );
    assert_eq!(rotating.rotate_credentials().await.unwrap().rotated, 0);
    let ready = f
        .vault
        .list(&f.owner, PageRequest::new(None, 50).unwrap())
        .await
        .unwrap()
        .accounts
        .into_iter()
        .find(|a| a.status == AccountStatus::Ready)
        .unwrap();
    assert!(
        PostgresVault::new(retired)
            .read_usable(&f.owner, ready.id)
            .await
            .is_ok()
    );
}
#[tokio::test]
async fn vault_rotation_unavailable_key_blocks_convergence_without_plaintext_fallback() {
    let f = Fixture::isolated().await;
    let account = f.create("unavailable").await;
    let unavailable = PostgresPersistence::with_credential_cipher(
        f.pool().clone(),
        CredentialCipher::for_test_with_keys(&[(2, [8; 32])], 2),
    );
    assert_eq!(
        unavailable
            .credential_status()
            .await
            .unwrap()
            .unavailable_rows,
        1
    );
    assert!(!unavailable.rotate_credentials().await.unwrap().complete);
    assert!(
        PostgresVault::new(unavailable)
            .read_usable(&f.owner, account.id)
            .await
            .is_err()
    );
    assert!(f.rotating().rotate_credentials().await.unwrap().complete);
}
enum Writer {
    Static,
    Managed,
}
async fn racing_writer(writer: Writer) {
    let mut f = Fixture::isolated().await;
    let account = match writer {
        Writer::Static => f.create("racing").await,
        Writer::Managed => {
            f.vault.registry = managed_registry();
            f.vault
                .create(
                    &f.owner,
                    managed_write(&f.vault.registry, "racing", "workspace-a"),
                )
                .await
                .unwrap()
        }
    };
    let mut scan = f.pool().acquire().await.unwrap();
    let old = inventory::page(&mut scan, None, 100)
        .await
        .unwrap()
        .remove(0);
    drop(scan);
    let rotating = f.rotating();
    let rotated = rotating
        .credential_cipher()
        .unwrap()
        .rotate_to_active(&CredentialFormat::Envelope(old.context), &old.envelope)
        .unwrap();
    // The fixture-only trigger pauses the real writer after its update, holding the row.
    sqlx::raw_sql("CREATE FUNCTION pause_vault_writer() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.revision>OLD.revision THEN PERFORM pg_advisory_xact_lock(53943); END IF; RETURN NEW; END $$; CREATE TRIGGER pause_vault_writer AFTER UPDATE ON integration_accounts FOR EACH ROW EXECUTE FUNCTION pause_vault_writer();").execute(f.pool()).await.unwrap();
    let mut barrier = f.pool().acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(53943)")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let vault = f.vault.clone();
    let owner = f.owner.clone();
    let id = account.id;
    let revision = account.revision;
    let writing = tokio::spawn(async move {
        match writer {
            Writer::Static => {
                vault
                    .replace(&owner, id, revision, static_write("racing", "replacement"))
                    .await
            }
            Writer::Managed => {
                vault
                    .write_managed(&owner, id, revision, managed_payload("same-client", 2))
                    .await
            }
        }
    });
    wait_until_a_backend_is_blocked(f.pool()).await;
    let pool = f.pool().clone();
    let stored = old.envelope;
    let rotation = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let changed = inventory::rewrap(&mut tx, id.as_uuid(), &stored, &rotated)
            .await
            .unwrap()
            .rows_affected();
        tx.commit().await.unwrap();
        changed
    });
    wait_until_backends_are_blocked(f.pool(), 2).await;
    sqlx::query("SELECT pg_advisory_unlock(53943)")
        .execute(&mut *barrier)
        .await
        .unwrap();
    drop(barrier);
    let written = writing.await.unwrap().unwrap();
    assert_eq!(rotation.await.unwrap(), 0);
    let before = f.vault.read_usable(&f.owner, id).await.unwrap();
    assert_eq!(before.versions.generation.get(), 2);
    assert_eq!(written.revision.get(), 2);
    assert_eq!(
        written.authorization_revision.get(),
        if written.auth_method == AuthMethod::StaticSecret {
            2
        } else {
            1
        }
    );
    assert!(rotating.rotate_credentials().await.unwrap().complete);
    let mut active = PostgresVault::new(rotating);
    active.registry = f.vault.registry.clone();
    let after = active.read_usable(&f.owner, id).await.unwrap();
    assert!(after.payload.expose_secret() == before.payload.expose_secret());
    assert_eq!(after.versions, before.versions);
}
#[tokio::test]
async fn vault_rotation_real_owner_replacement_defeats_waiting_stale_cas() {
    racing_writer(Writer::Static).await;
}
#[tokio::test]
async fn vault_rotation_real_managed_cache_write_defeats_waiting_stale_cas() {
    racing_writer(Writer::Managed).await;
}
