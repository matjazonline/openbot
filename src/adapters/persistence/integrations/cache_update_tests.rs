use super::*;

#[tokio::test]
async fn vault_storage_cache_updates_preserve_each_providers_authorization_facts() {
    for app in ["workspace-a", "google-fixture"] {
        let mut f = Fixture::shared().await;
        f.vault.registry = if app == "google-fixture" {
            google_registry()
        } else {
            managed_registry()
        };
        let registration = f.vault.registry.get(&AppId::parse(app).unwrap()).unwrap();
        let mut original = if app == "google-fixture" {
            json!({"subject":"google-account","mailbox":"mailbox@gmail.com","client":"google-client-fixture","scopes":["mail.read","mail.send"],"refresh_token":"fixture-refresh","access_token":"fixture-access","expires_at":"2030-01-01T00:00:00Z","generation":1,"refresh_retry_at":null})
        } else {
            json!({"workspace":"workspace-without-email","client":"same-client","permissions":["resource.read","resource.write"],"token":"fixture-managed-secret","generation":1})
        };
        let permission = if app == "google-fixture" {
            "scopes"
        } else {
            "permissions"
        };
        let payload = SecretString::from(original.to_string());
        let write = registered_write(registration, "authorization-facts", payload);
        let account = f.vault.create(&f.owner, write).await.unwrap();
        let mut changed = original.clone();
        changed[permission] = json!(["broader.permission"]);
        changed["generation"] = json!(2);
        let before = f.vault.read_usable(&f.owner, account.id).await.unwrap();
        let ciphertext: String =
            sqlx::query_scalar("SELECT envelope FROM integration_accounts WHERE id=$1")
                .bind(account.id.as_uuid())
                .fetch_one(f.pool())
                .await
                .unwrap();

        assert!(matches!(
            f.vault
                .write_managed(
                    &f.owner,
                    account.id,
                    account.revision,
                    SecretString::from(changed.to_string())
                )
                .await,
            Err(IntegrationError::Denied)
        ));
        let retained = f.vault.read_usable(&f.owner, account.id).await.unwrap();
        assert_eq!(retained.versions, before.versions);
        let retained_ciphertext: String =
            sqlx::query_scalar("SELECT envelope FROM integration_accounts WHERE id=$1")
                .bind(account.id.as_uuid())
                .fetch_one(f.pool())
                .await
                .unwrap();
        assert!(retained_ciphertext == ciphertext);

        assert!(retained.payload.expose_secret() == before.payload.expose_secret());
        original[permission].as_array_mut().unwrap().reverse();
        original["generation"] = json!(2);
        original[if app == "google-fixture" {
            "access_token"
        } else {
            "token"
        }] = json!("refreshed-cache");
        let updated = f
            .vault
            .write_managed(
                &f.owner,
                account.id,
                account.revision,
                SecretString::from(original.to_string()),
            )
            .await
            .unwrap();
        assert_eq!(
            updated.authorization_revision,
            account.authorization_revision
        );
        assert_eq!(updated.revision.get(), 2);
    }
}
#[tokio::test]
async fn vault_storage_sql_required_json_fields_cannot_evaluate_to_null() {
    let mut f = Fixture::shared().await;
    let account = f.create("required-json").await;
    for metadata in [
        "{}",
        r#"{"description":null}"#,
        r#"{"label":null,"description":null}"#,
    ] {
        let mut tx = f.pool().begin().await.unwrap();
        let result = sqlx::query("UPDATE integration_accounts SET metadata=$2 WHERE id=$1")
            .bind(account.id.as_uuid())
            .bind(metadata)
            .execute(&mut *tx)
            .await;
        tx.rollback().await.unwrap();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .as_database_error()
                .is_some_and(|error| error.code().as_deref() == Some("23514"))
        );
    }
    f.vault.registry = managed_registry();
    let managed = f
        .vault
        .create(
            &f.owner,
            managed_write(&f.vault.registry, "managed-required-json", "workspace-a"),
        )
        .await
        .unwrap();
    let mut tx = f.pool().begin().await.unwrap();
    let result=sqlx::query("INSERT INTO integration_accounts(id,owner_namespace,owner_subject,key,provider_id,auth_config_id,auth_method,credential_schema,external_identity,authorization_key,envelope,status,metadata) SELECT $1,owner_namespace,owner_subject,$2,provider_id,auth_config_id,auth_method,credential_schema,jsonb_set(external_identity::jsonb,'{authorization_key}','null'::jsonb)::text,$3,envelope,status,metadata FROM integration_accounts WHERE id=$4")
        .bind(Uuid::new_v4()).bind("invalid-canonical-key").bind(Uuid::new_v4().to_string()).bind(managed.id.as_uuid()).execute(&mut *tx).await;
    tx.rollback().await.unwrap();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .as_database_error()
            .is_some_and(|error| error.code().as_deref() == Some("23514"))
    );
}
