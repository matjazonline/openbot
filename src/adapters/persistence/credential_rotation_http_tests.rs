use super::*;

#[tokio::test]
async fn workflow_http_rotation_inventory_preserves_context_and_is_idempotent() {
    let Some(mut connection) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    let company = Uuid::new_v4();
    let id = Uuid::new_v4();
    let context = CredentialContext::workflow_http_credential(company, id);
    let sealed = old_writer
        .seal_envelope(&context, &SecretString::from("http-token"))
        .unwrap();
    sqlx::query(
        "INSERT INTO workflow_http_credentials(company_id,connection_id,envelope) VALUES($1,$2,$3)",
    )
    .bind(company)
    .bind(id)
    .bind(&sealed)
    .execute(&mut connection)
    .await
    .unwrap();
    let before = credential_status_on_connection(&mut connection, &rotator)
        .await
        .unwrap();
    assert_eq!(
        (before.total_rows, before.old_rows, before.active_rows),
        (1, 1, 0)
    );
    let report = rotate_locked(&mut connection, &rotator).await.unwrap();
    assert!(report.complete);
    assert_eq!(report.rotated, 1);
    let stored: String = sqlx::query_scalar(
        "SELECT envelope FROM workflow_http_credentials WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(company)
    .bind(id)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_ne!(sealed, stored);
    assert_eq!(
        rotator
            .open_envelope(&context, &stored)
            .unwrap()
            .expose_secret(),
        "http-token"
    );
    for foreign in [
        CredentialContext::workflow_http_credential(Uuid::new_v4(), id),
        CredentialContext::workflow_http_credential(company, Uuid::new_v4()),
        CredentialContext::company_mcp_credential(company, id),
    ] {
        assert!(rotator.open_envelope(&foreign, &stored).is_err());
    }
    let after = credential_status_on_connection(&mut connection, &rotator)
        .await
        .unwrap();
    assert_eq!(
        (after.total_rows, after.old_rows, after.active_rows),
        (1, 0, 1)
    );
    assert!(after.satisfies_required_version(2));
    let second = rotate_locked(&mut connection, &rotator).await.unwrap();
    assert!(second.complete);
    assert_eq!(second.rotated, 0);
}

#[tokio::test]
async fn workflow_http_rotation_stale_ciphertext_cannot_overwrite_normal_writer() {
    let Some(mut connection) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    let company = Uuid::new_v4();
    let id = Uuid::new_v4();
    let context = CredentialContext::workflow_http_credential(company, id);
    let sealed = old_writer
        .seal_envelope(&context, &SecretString::from("old-token"))
        .unwrap();
    sqlx::query(
        "INSERT INTO workflow_http_credentials(company_id,connection_id,envelope) VALUES($1,$2,$3)",
    )
    .bind(company)
    .bind(id)
    .bind(&sealed)
    .execute(&mut connection)
    .await
    .unwrap();
    let stale = fetch_credential_batch(
        &mut connection,
        CredentialTable::WorkflowHttpCredentials,
        None,
    )
    .await
    .unwrap();
    assert_eq!(stale.len(), 1);
    let replacement = rotator
        .seal_envelope(&context, &SecretString::from("normal-writer-wins"))
        .unwrap();
    sqlx::query(
        "UPDATE workflow_http_credentials SET envelope=$3 WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(company)
    .bind(id)
    .bind(&replacement)
    .execute(&mut connection)
    .await
    .unwrap();
    let outcome = rotate_credential_batch(&mut connection, &rotator, stale)
        .await
        .unwrap();
    assert_eq!(outcome.rotated, 0);
    assert_eq!(outcome.cas_conflicts, 1);
    let stored: String = sqlx::query_scalar(
        "SELECT envelope FROM workflow_http_credentials WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(company)
    .bind(id)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(stored, replacement);
}
