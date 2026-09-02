//! Database-backed tests for the credential inventory.
//!
//! Every test here works against session-local `CREATE TEMPORARY TABLE` shadows of the credential
//! tables. Status and rotation sweep whole tables by design, and the suite shares one database, so
//! a fixture in the real tables would rotate rows belonging to a test running beside it.

use secrecy::{ExposeSecret, SecretString};
use sqlx::Executor;

use super::*;
use crate::adapters::persistence::test_support::test_pool;

async fn isolated_connection() -> Option<PgConnection> {
    let pool = test_pool().await?;
    let options = pool.connect_options();
    let mut connection = PgConnection::connect_with(&options)
        .await
        .expect("an isolated credential-rotation test connection");
    // Session-local tables shadow the real ones for this connection only. The fixtures here
    // share one test database with every other test, and both rotation and status sweep whole
    // tables by design, so a real-table fixture would rotate rows belonging to a test running
    // beside it.
    for statement in [
        r#"CREATE TEMPORARY TABLE company_model_connections (
               company_id UUID NOT NULL,
               provider TEXT NOT NULL,
               api_key TEXT NOT NULL,
               updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
               PRIMARY KEY (company_id, provider)
           ) ON COMMIT PRESERVE ROWS"#,
        r#"CREATE TEMPORARY TABLE integration_installations (
               id UUID NOT NULL,
               company_id UUID NOT NULL,
               transport TEXT NOT NULL,
               PRIMARY KEY (company_id, id)
           ) ON COMMIT PRESERVE ROWS"#,
        r#"CREATE TEMPORARY TABLE integration_credentials (
               company_id UUID NOT NULL,
               installation_id UUID NOT NULL,
               credential_kind TEXT NOT NULL,
               envelope TEXT NOT NULL,
               updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
               PRIMARY KEY (company_id, installation_id, credential_kind)
           ) ON COMMIT PRESERVE ROWS"#,
    ] {
        connection
            .execute(statement)
            .await
            .expect("a session-local credential table");
    }
    Some(connection)
}

/// One installation plus one sealed bot token, in the session-local tables.
async fn insert_integration_credential(
    connection: &mut PgConnection,
    cipher: &CredentialCipher,
    company_id: Uuid,
    secret: &str,
) -> (Uuid, CredentialContext) {
    let installation_id = Uuid::new_v4();
    let context = CredentialContext::integration_credential(
        company_id,
        installation_id,
        TransportKind::Slack,
        IntegrationCredentialKind::BotAccessToken,
    );
    sqlx::query(
        r#"INSERT INTO integration_installations (id, company_id, transport)
           VALUES ($1, $2, 'slack')"#,
    )
    .bind(installation_id)
    .bind(company_id)
    .execute(&mut *connection)
    .await
    .expect("the installation fixture is insertable");
    sqlx::query(
        r#"INSERT INTO integration_credentials
               (company_id, installation_id, credential_kind, envelope)
           VALUES ($1, $2, 'bot_access_token', $3)"#,
    )
    .bind(company_id)
    .bind(installation_id)
    .bind(
        cipher
            .seal_envelope(&context, &SecretString::from(secret.to_string()))
            .unwrap(),
    )
    .execute(&mut *connection)
    .await
    .expect("the integration credential fixture is insertable");
    (installation_id, context)
}

async fn insert_credential(
    connection: &mut PgConnection,
    company_id: Uuid,
    provider: &str,
    api_key: &str,
) {
    sqlx::query(
        r#"INSERT INTO company_model_connections (company_id, provider, api_key)
           VALUES ($1, $2, $3)"#,
    )
    .bind(company_id)
    .bind(provider)
    .bind(api_key)
    .execute(connection)
    .await
    .expect("the credential fixture is insertable");
}

fn rotation_ciphers() -> (CredentialCipher, CredentialCipher) {
    let keys = [(1, [42_u8; 32]), (2, [84_u8; 32])];
    (
        CredentialCipher::for_test_with_keys(&keys, 1),
        CredentialCipher::for_test_with_keys(&keys, 2),
    )
}

async fn simulated_rotation_command(
    connection: &mut PgConnection,
    cipher: &CredentialCipher,
) -> AppResult<CredentialRotationReport> {
    if !try_acquire_rotation_lock(connection).await? {
        return Err(AppError::Conflict(
            "Another credential rotation currently owns the database lock".into(),
        ));
    }
    // Keep the ownership window open long enough for the competing command to reach the lock.
    sqlx::query("SELECT pg_sleep(0.05)")
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
    let result = rotate_locked(connection, cipher).await;
    release_rotation_lock(connection).await?;
    result
}

#[tokio::test]
async fn two_competing_rotation_commands_have_exactly_one_owner() {
    let Some(mut first) = isolated_connection().await else {
        return;
    };
    let Some(mut second) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    insert_credential(
        &mut first,
        Uuid::new_v4(),
        "openai",
        &old_writer.encrypt("first").unwrap(),
    )
    .await;
    insert_credential(
        &mut second,
        Uuid::new_v4(),
        "openai",
        &old_writer.encrypt("second").unwrap(),
    )
    .await;

    let (first_result, second_result) = tokio::join!(
        simulated_rotation_command(&mut first, &rotator),
        simulated_rotation_command(&mut second, &rotator)
    );

    let outcomes = [first_result, second_result];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(AppError::Conflict(_))))
            .count(),
        1
    );
}

#[tokio::test]
async fn stale_rotation_ciphertext_cannot_clobber_a_normal_write() {
    let Some(mut connection) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    let company_id = Uuid::new_v4();
    insert_credential(
        &mut connection,
        company_id,
        "openai",
        &old_writer.encrypt("before-race").unwrap(),
    )
    .await;
    let stale_rows =
        fetch_credential_batch(&mut connection, CredentialTable::ModelConnections, None)
            .await
            .unwrap();
    let replacement = rotator.encrypt("normal-writer-wins").unwrap();
    sqlx::query(
        r#"UPDATE company_model_connections
           SET api_key = $3
           WHERE company_id = $1 AND provider = $2"#,
    )
    .bind(company_id)
    .bind("openai")
    .bind(&replacement)
    .execute(&mut connection)
    .await
    .unwrap();

    let outcome = rotate_credential_batch(&mut connection, &rotator, stale_rows)
        .await
        .unwrap();
    let stored: String =
        sqlx::query_scalar("SELECT api_key FROM company_model_connections WHERE company_id = $1")
            .bind(company_id)
            .fetch_one(&mut connection)
            .await
            .unwrap();

    assert_eq!(outcome.rotated, 0);
    assert_eq!(outcome.cas_conflicts, 1);
    assert_eq!(stored, replacement);
}

#[tokio::test]
async fn a_late_old_writer_is_found_by_the_next_complete_pass() {
    let Some(mut connection) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    insert_credential(
        &mut connection,
        Uuid::new_v4(),
        "openai",
        &old_writer.encrypt("first").unwrap(),
    )
    .await;
    let first_pass = run_rotation_pass(&mut connection, &rotator).await.unwrap();
    assert_eq!(first_pass.rotated, 1);

    insert_credential(
        &mut connection,
        Uuid::new_v4(),
        "openai",
        &old_writer.encrypt("late").unwrap(),
    )
    .await;
    let report = rotate_locked(&mut connection, &rotator).await.unwrap();

    assert!(report.complete);
    assert_eq!(report.rotated, 1);
    assert_eq!(report.final_status.active_rows, 2);
    assert_eq!(report.final_status.old_rows, 0);
}

#[tokio::test]
async fn batches_are_bounded_and_a_second_rotation_is_idempotent() {
    let Some(mut connection) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    for index in 0..205_u16 {
        insert_credential(
            &mut connection,
            Uuid::from_u128(u128::from(index) + 1),
            "openai",
            &old_writer.encrypt("secret").unwrap(),
        )
        .await;
    }

    let first_batch =
        fetch_credential_batch(&mut connection, CredentialTable::ModelConnections, None)
            .await
            .unwrap();
    assert_eq!(first_batch.len(), CREDENTIAL_BATCH_SIZE as usize);
    let first = rotate_locked(&mut connection, &rotator).await.unwrap();
    let second = rotate_locked(&mut connection, &rotator).await.unwrap();

    assert!(first.complete);
    assert_eq!(first.rotated, 205);
    assert!(first.batches >= 6);
    assert!(second.complete);
    assert_eq!(second.rotated, 0);
    assert_eq!(second.cas_conflicts, 0);
}

#[tokio::test]
async fn status_and_rotation_reject_malformed_and_unavailable_rows() {
    let Some(mut connection) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    let unavailable =
        old_writer
            .encrypt("unavailable")
            .unwrap()
            .replacen("enc:v1:1:", "enc:v1:3:", 1);
    insert_credential(&mut connection, Uuid::new_v4(), "openai", "plaintext").await;
    insert_credential(&mut connection, Uuid::new_v4(), "openai", &unavailable).await;

    let status = credential_status_on_connection(&mut connection, &rotator)
        .await
        .unwrap();
    let report = rotate_locked(&mut connection, &rotator).await.unwrap();

    assert_eq!(status.malformed_rows, 1);
    assert_eq!(status.unavailable_rows, 1);
    assert_eq!(status.unavailable_versions.get(&3), Some(&1));
    assert!(!status.is_valid());
    assert!(!report.complete);
    assert_eq!(report.invalid_rows, 2);
}

/// The inventory is not one table. A rotation that converges on model connections while
/// leaving every Slack token on a retired key is not a completed rotation, and the report
/// would say it was.
#[tokio::test]
async fn rotation_covers_integration_credentials_and_only_rewraps_them() {
    let Some(mut connection) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    let company_id = Uuid::new_v4();
    insert_credential(
        &mut connection,
        company_id,
        "openai",
        &old_writer.encrypt("model-key").unwrap(),
    )
    .await;
    let (installation_id, context) =
        insert_integration_credential(&mut connection, &old_writer, company_id, "xoxb-token").await;
    let before: String = stored_envelope(&mut connection, company_id, installation_id).await;

    let status = credential_status_on_connection(&mut connection, &rotator)
        .await
        .unwrap();
    let report = rotate_locked(&mut connection, &rotator).await.unwrap();
    let after: String = stored_envelope(&mut connection, company_id, installation_id).await;

    assert_eq!(status.total_rows, 2, "both tables are inventoried");
    assert_eq!(status.old_rows, 2);
    assert!(report.complete);
    assert_eq!(report.rotated, 2);
    assert_eq!(report.final_status.active_rows, 2);

    // Rewrapped, not re-encrypted: the payload field is byte-identical and the token still
    // opens under the new key.
    let payload = |envelope: &str| envelope.split(':').next_back().unwrap().to_string();
    assert_ne!(after, before);
    assert_eq!(payload(&after), payload(&before));
    assert_eq!(
        rotator
            .open_envelope(&context, &after)
            .unwrap()
            .expose_secret(),
        "xoxb-token"
    );
}

/// A tampered envelope must classify as malformed rather than rewrap cleanly into the new key
/// version, which is why rotation opens the payload instead of only unwrapping the data key.
#[tokio::test]
async fn a_tampered_integration_envelope_blocks_convergence() {
    let Some(mut connection) = isolated_connection().await else {
        return;
    };
    let (old_writer, rotator) = rotation_ciphers();
    let company_id = Uuid::new_v4();
    let (installation_id, _) =
        insert_integration_credential(&mut connection, &old_writer, company_id, "xoxb-token").await;
    let intact = stored_envelope(&mut connection, company_id, installation_id).await;
    let mut fields: Vec<&str> = intact.split(':').collect();
    let corrupted_payload = format!("{}A", fields.pop().unwrap());
    fields.push(&corrupted_payload);
    sqlx::query("UPDATE integration_credentials SET envelope = $1 WHERE company_id = $2")
        .bind(fields.join(":"))
        .bind(company_id)
        .execute(&mut connection)
        .await
        .unwrap();

    let status = credential_status_on_connection(&mut connection, &rotator)
        .await
        .unwrap();
    let report = rotate_locked(&mut connection, &rotator).await.unwrap();

    assert_eq!(status.malformed_rows, 1);
    assert!(!status.is_valid());
    assert!(!report.complete);
    assert_eq!(report.rotated, 0);
}

async fn stored_envelope(
    connection: &mut PgConnection,
    company_id: Uuid,
    installation_id: Uuid,
) -> String {
    sqlx::query_scalar(
        r#"SELECT envelope FROM integration_credentials
           WHERE company_id = $1 AND installation_id = $2"#,
    )
    .bind(company_id)
    .bind(installation_id)
    .fetch_one(connection)
    .await
    .expect("the integration credential is readable")
}
