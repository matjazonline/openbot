use super::*;
#[tokio::test]
async fn vault_storage_roundtrip_replace_disconnect_delete_and_redaction() {
    let f = Fixture::shared().await;
    let account = f.create("roundtrip").await;
    let record = f.vault.read_usable(&f.owner, account.id).await.unwrap();
    assert!(record.payload.expose_secret() == "fixture-secret");
    let mut write = static_write("roundtrip", "replacement-secret");
    write.metadata = AccountMetadata::new("Renamed".into(), Some("Description".into())).unwrap();
    let replaced = f
        .vault
        .replace(&f.owner, account.id, account.revision, write)
        .await
        .unwrap();
    assert_eq!(replaced.revision.get(), 2);
    assert_eq!(replaced.authorization_revision.get(), 2);
    let record = f.vault.read_usable(&f.owner, account.id).await.unwrap();
    assert!(record.payload.expose_secret() == "replacement-secret");
    assert_eq!(record.versions.generation.get(), 2);
    assert_eq!(
        f.vault
            .get(&f.owner, account.id)
            .await
            .unwrap()
            .metadata
            .label(),
        "Renamed"
    );
    let serialized = serde_json::to_string(
        &f.vault
            .list(&f.owner, PageRequest::new(None, 50).unwrap())
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        !serialized.contains("replacement-secret")
            && !serialized.contains("fixture-secret")
            && !serialized.contains("enc:v2")
            && !serialized.contains("envelope")
    );
    assert!(matches!(
        f.vault
            .replace(
                &f.owner,
                account.id,
                account.revision,
                static_write("roundtrip", "stale")
            )
            .await,
        Err(IntegrationError::Conflict)
    ));
    let disconnected = f
        .vault
        .disconnect(&f.owner, account.id, replaced.authorization_revision)
        .await
        .unwrap();
    assert_eq!(disconnected.status, AccountStatus::Disconnected);
    assert!(f.vault.read_usable(&f.owner, account.id).await.is_err());
    let empty: bool =
        sqlx::query_scalar("SELECT envelope IS NULL FROM integration_accounts WHERE id=$1")
            .bind(account.id.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert!(empty);
    f.vault
        .delete(&f.owner, account.id, disconnected.authorization_revision)
        .await
        .unwrap();
    assert!(matches!(
        f.vault.get(&f.owner, account.id).await,
        Err(IntegrationError::NotFound)
    ));
}
#[tokio::test]
async fn vault_storage_owner_namespaces_keys_and_competing_replacements() {
    let f = Fixture::shared().await;
    let account = f.create("same-key").await;
    let foreign = AccountOwner {
        namespace: OwnerNamespace::parse("another-client").unwrap(),
        subject: f.owner.subject.clone(),
    };
    assert!(matches!(
        f.vault.get(&foreign, account.id).await,
        Err(IntegrationError::NotFound)
    ));
    assert!(f.vault.read_usable(&foreign, account.id).await.is_err());
    assert!(
        f.vault
            .create(&foreign, static_write("same-key", "different"))
            .await
            .is_ok()
    );
    assert!(matches!(
        f.vault
            .create(&f.owner, static_write("same-key", "duplicate"))
            .await,
        Err(IntegrationError::Conflict)
    ));
    let barrier = tokio::sync::Barrier::new(2);
    let left = async {
        barrier.wait().await;
        f.vault
            .replace(
                &f.owner,
                account.id,
                account.revision,
                static_write("same-key", "left"),
            )
            .await
    };
    let right = async {
        barrier.wait().await;
        f.vault
            .replace(
                &f.owner,
                account.id,
                account.revision,
                static_write("same-key", "right"),
            )
            .await
    };
    let (left, right) = tokio::join!(left, right);
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(
        matches!(left, Err(IntegrationError::Conflict))
            || matches!(right, Err(IntegrationError::Conflict))
    );
}
#[tokio::test]
async fn vault_storage_cap_serializes_real_same_owner_creators() {
    let f = Fixture::shared().await;
    for index in 0..127 {
        f.create(&format!("account-{index}")).await;
    }
    let other = PostgresVault::new(f.vault.persistence.clone());
    let barrier = tokio::sync::Barrier::new(2);
    let left = async {
        barrier.wait().await;
        f.vault
            .create(&f.owner, static_write("last-left", "secret"))
            .await
    };
    let right = async {
        barrier.wait().await;
        other
            .create(&f.owner, static_write("last-right", "secret"))
            .await
    };
    let (left, right) = tokio::join!(left, right);
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(
        matches!(left, Err(IntegrationError::Limit))
            || matches!(right, Err(IntegrationError::Limit))
    );
    let mut cursor = None;
    let mut ids = std::collections::HashSet::new();
    loop {
        let page = f
            .vault
            .list(&f.owner, PageRequest::new(cursor, 50).unwrap())
            .await
            .unwrap();
        assert!(page.accounts.len() <= 50);
        for account in page.accounts {
            assert!(ids.insert(account.id));
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 128);
}
#[tokio::test]
async fn vault_storage_managed_identity_ignores_config_aliases_and_static_edits_deny() {
    let mut f = Fixture::shared().await;
    f.vault.registry = managed_registry();
    let account = f
        .vault
        .create(
            &f.owner,
            managed_write(&f.vault.registry, "managed", "workspace-a"),
        )
        .await
        .unwrap();
    let identity = serde_json::to_value(&account.external_identity).unwrap();
    assert!(identity.get("email").is_none());
    assert_eq!(identity["subject"], "workspace-without-email");
    assert!(matches!(
        f.vault
            .create(
                &f.owner,
                managed_write(&f.vault.registry, "alias", "workspace-b")
            )
            .await,
        Err(IntegrationError::Conflict)
    ));
    assert!(
        f.vault
            .create(
                &f.owner,
                managed_write(&f.vault.registry, "another-client", "workspace-other")
            )
            .await
            .is_ok()
    );
    assert!(
        f.vault
            .replace(
                &f.owner,
                account.id,
                account.revision,
                static_write("managed", "generic")
            )
            .await
            .is_err()
    );
    let updated = f
        .vault
        .write_managed(
            &f.owner,
            account.id,
            account.revision,
            managed_payload("same-client", 2),
        )
        .await
        .unwrap();
    assert_eq!(updated.revision.get(), 2);
    assert_eq!(
        updated.authorization_revision,
        account.authorization_revision
    );
    assert_eq!(
        f.vault
            .read_usable(&f.owner, account.id)
            .await
            .unwrap()
            .versions
            .generation
            .get(),
        2
    );
    assert!(matches!(
        f.vault
            .write_managed(
                &f.owner,
                account.id,
                account.revision,
                managed_payload("same-client", 3)
            )
            .await,
        Err(IntegrationError::Conflict)
    ));
}
#[tokio::test]
async fn vault_storage_constraints_substitution_and_malformed_encrypted_json_fail_closed() {
    let f = Fixture::shared().await;
    let account = f.create("constraints").await;
    for statement in [
        "UPDATE integration_accounts SET revision=0 WHERE id=$1",
        "UPDATE integration_accounts SET credential_schema='unknown' WHERE id=$1",
        "UPDATE integration_accounts SET metadata='{\"label\":42}' WHERE id=$1",
        "UPDATE integration_accounts SET status='disconnected' WHERE id=$1",
        "UPDATE integration_accounts SET key='substituted' WHERE id=$1",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(account.id.as_uuid())
                .execute(f.pool())
                .await
                .is_err()
        );
    }
    let raw = SecretString::from(r#" {"token":42,"sentinel":"private"}"#);
    let envelope = f
        .vault
        .persistence
        .credential_cipher()
        .unwrap()
        .seal_envelope(&CredentialContext::integration_account(&account), &raw)
        .unwrap();
    sqlx::query("UPDATE integration_accounts SET envelope=$2 WHERE id=$1")
        .bind(account.id.as_uuid())
        .bind(envelope)
        .execute(f.pool())
        .await
        .unwrap();
    let error = f.vault.read_usable(&f.owner, account.id).await.unwrap_err();
    assert!(!format!("{error:?} {error}").contains("private"));
    assert!(f.vault.get(&f.owner, account.id).await.is_ok());
    let mut w = static_write("constraints", "new");
    w.config = AuthConfigId::parse("unregistered").unwrap();
    assert!(
        f.vault
            .replace(&f.owner, account.id, account.revision, w)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn vault_storage_envelope_identity_and_context_specific_bounds() {
    let f = Fixture::shared().await;
    let account = f.create("context").await;
    let cipher = f.vault.persistence.credential_cipher().unwrap();
    let secret = SecretString::from("x".repeat(MAX_CREDENTIAL_BYTES));
    let context = CredentialContext::integration_account(&account);
    let envelope = cipher.seal_envelope(&context, &secret).unwrap();
    assert!(cipher.open_envelope(&context, &envelope).is_ok());
    assert!(
        cipher
            .seal_envelope(
                &context,
                &SecretString::from("x".repeat(MAX_CREDENTIAL_BYTES + 1))
            )
            .is_err()
    );
    assert!(
        cipher
            .seal_envelope(
                &CredentialContext::workflow_http_credential(Uuid::new_v4(), Uuid::new_v4()),
                &SecretString::from("x".repeat(4097))
            )
            .is_err()
    );
    for field in 0..7 {
        let mut moved = account.clone();
        match field {
            0 => moved.owner.namespace = OwnerNamespace::parse("other").unwrap(),
            1 => moved.owner.subject = OwnerSubject::parse("other").unwrap(),
            2 => moved.id = AccountId::new(Uuid::new_v4()),
            3 => moved.provider_id = ProviderId::parse("other").unwrap(),
            4 => moved.auth_config_id = AuthConfigId::parse("other").unwrap(),
            5 => moved.auth_method = AuthMethod::OAuthAuthorizationCode,
            _ => moved.credential_schema = CredentialSchema::parse("other").unwrap(),
        };
        assert!(
            cipher
                .open_envelope(&CredentialContext::integration_account(&moved), &envelope)
                .is_err()
        );
    }
}
#[test]
fn vault_storage_host_consumers_do_not_read_module_tables() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in [
        "src/adapters/http/routes",
        "src/adapters/persistence/workflow",
    ] {
        fn scan(dir: &std::path::Path) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    scan(&path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    assert!(
                        !source.contains("FROM integration_accounts")
                            && !source.contains("JOIN integration_accounts")
                            && !source.contains("UPDATE integration_accounts"),
                        "{} bypasses account port",
                        path.display()
                    );
                }
            }
        }
        scan(&root.join(path));
    }
}
