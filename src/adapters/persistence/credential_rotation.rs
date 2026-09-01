use std::{collections::BTreeMap, time::Instant};

use serde::Serialize;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use super::{
    PostgresPersistence,
    credentials::{CredentialCipher, CredentialState},
};
use crate::app_error::{AppError, AppResult};

const CREDENTIAL_BATCH_SIZE: i64 = 100;
const CREDENTIAL_ROTATION_LOCK_ID: i64 = 0x4352_4544_524f_5441;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CredentialStatus {
    pub active_version: u32,
    pub available_versions: Vec<u32>,
    pub total_rows: u64,
    pub active_rows: u64,
    pub old_rows: u64,
    pub malformed_rows: u64,
    pub unavailable_rows: u64,
    pub versions: BTreeMap<u32, u64>,
    pub unavailable_versions: BTreeMap<u32, u64>,
}

impl CredentialStatus {
    pub fn is_valid(&self) -> bool {
        self.malformed_rows == 0 && self.unavailable_rows == 0
    }

    pub fn satisfies_required_version(&self, required_version: u32) -> bool {
        self.is_valid()
            && self.available_versions.contains(&required_version)
            && self
                .versions
                .iter()
                .all(|(version, count)| *version == required_version || *count == 0)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CredentialRotationReport {
    pub target_version: u32,
    pub scanned: u64,
    pub rotated: u64,
    pub cas_conflicts: u64,
    pub invalid_rows: u64,
    pub malformed_rows: u64,
    pub unavailable_rows: u64,
    pub batches: u64,
    pub duration_ms: u64,
    pub complete: bool,
    pub final_status: CredentialStatus,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct CredentialRow {
    company_id: Uuid,
    provider: String,
    api_key: String,
}

#[derive(Debug, Clone)]
struct CredentialCursor {
    company_id: Uuid,
    provider: String,
}

#[derive(Debug, Default)]
struct RotationTotals {
    scanned: u64,
    rotated: u64,
    cas_conflicts: u64,
    malformed_rows: u64,
    unavailable_rows: u64,
    batches: u64,
}

#[derive(Debug, Default)]
struct RotationPassOutcome {
    scanned: u64,
    rotated: u64,
    cas_conflicts: u64,
    malformed_rows: u64,
    unavailable_rows: u64,
    batches: u64,
}

#[derive(Debug, Default)]
struct RotationBatchOutcome {
    rotated: u64,
    cas_conflicts: u64,
    malformed_rows: u64,
    unavailable_rows: u64,
}

impl RotationTotals {
    fn add_pass(&mut self, pass: &RotationPassOutcome) {
        self.scanned += pass.scanned;
        self.rotated += pass.rotated;
        self.cas_conflicts += pass.cas_conflicts;
        self.malformed_rows += pass.malformed_rows;
        self.unavailable_rows += pass.unavailable_rows;
        self.batches += pass.batches;
    }
}

impl PostgresPersistence {
    pub async fn credential_status(&self) -> AppResult<CredentialStatus> {
        let cipher = self.credential_cipher()?;
        let mut connection = self.pool.acquire().await.map_err(AppError::from)?;
        credential_status_on_connection(&mut connection, cipher).await
    }

    pub async fn rotate_credentials(&self) -> AppResult<CredentialRotationReport> {
        let cipher = self.credential_cipher()?;
        let mut connection = self.pool.acquire().await.map_err(AppError::from)?;
        if !try_acquire_rotation_lock(&mut connection).await? {
            return Err(AppError::Conflict(
                "Another credential rotation currently owns the database lock".into(),
            ));
        }

        let result = rotate_locked(&mut connection, cipher).await;
        let unlock_result = release_rotation_lock(&mut connection).await;
        match (result, unlock_result) {
            (result, Ok(())) => result,
            (Err(error), Err(_)) => {
                connection.close().await.ok();
                Err(error)
            }
            (Ok(_), Err(error)) => {
                connection.close().await.ok();
                Err(error)
            }
        }
    }

    fn credential_cipher(&self) -> AppResult<&CredentialCipher> {
        self.credential_cipher
            .as_deref()
            .ok_or_else(|| AppError::Internal("Credential encryption is not configured".into()))
    }
}

async fn rotate_locked(
    connection: &mut PgConnection,
    cipher: &CredentialCipher,
) -> AppResult<CredentialRotationReport> {
    let started = Instant::now();
    let mut totals = RotationTotals::default();

    loop {
        let pass = run_rotation_pass(connection, cipher).await?;
        let pass_changed_rows = pass.rotated > 0;
        let pass_has_invalid_rows = pass.malformed_rows > 0 || pass.unavailable_rows > 0;
        totals.add_pass(&pass);

        if pass_changed_rows && !pass_has_invalid_rows {
            continue;
        }

        let final_status = credential_status_on_connection(connection, cipher).await?;
        let complete = final_status.is_valid() && final_status.old_rows == 0;
        if complete || pass_has_invalid_rows {
            return Ok(rotation_report(
                started,
                totals,
                complete,
                final_status,
                cipher,
            ));
        }
    }
}

fn rotation_report(
    started: Instant,
    totals: RotationTotals,
    complete: bool,
    final_status: CredentialStatus,
    cipher: &CredentialCipher,
) -> CredentialRotationReport {
    let invalid_rows = final_status.malformed_rows + final_status.unavailable_rows;
    CredentialRotationReport {
        target_version: cipher.active_version(),
        scanned: totals.scanned,
        rotated: totals.rotated,
        cas_conflicts: totals.cas_conflicts,
        invalid_rows,
        malformed_rows: final_status.malformed_rows,
        unavailable_rows: final_status.unavailable_rows,
        batches: totals.batches,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        complete,
        final_status,
    }
}

async fn run_rotation_pass(
    connection: &mut PgConnection,
    cipher: &CredentialCipher,
) -> AppResult<RotationPassOutcome> {
    let mut outcome = RotationPassOutcome::default();
    let mut cursor = None;

    loop {
        let rows = fetch_credential_batch(connection, cursor.as_ref()).await?;
        if rows.is_empty() {
            return Ok(outcome);
        }
        debug_assert!(rows.len() <= usize::try_from(CREDENTIAL_BATCH_SIZE).unwrap_or(usize::MAX));
        cursor = rows.last().map(|row| CredentialCursor {
            company_id: row.company_id,
            provider: row.provider.clone(),
        });
        outcome.batches += 1;
        outcome.scanned += rows.len() as u64;
        let batch = rotate_credential_batch(connection, cipher, rows).await?;
        outcome.rotated += batch.rotated;
        outcome.cas_conflicts += batch.cas_conflicts;
        outcome.malformed_rows += batch.malformed_rows;
        outcome.unavailable_rows += batch.unavailable_rows;
    }
}

async fn rotate_credential_batch(
    connection: &mut PgConnection,
    cipher: &CredentialCipher,
    rows: Vec<CredentialRow>,
) -> AppResult<RotationBatchOutcome> {
    let mut outcome = RotationBatchOutcome::default();
    let mut transaction = connection.begin().await.map_err(AppError::from)?;
    for row in rows {
        match cipher.inspect(&row.api_key) {
            CredentialState::Active { .. } => {}
            CredentialState::Old { .. } => {
                let plaintext = cipher.decrypt_envelope(&row.api_key)?;
                let encrypted = cipher.encrypt(&plaintext)?;
                let changed = sqlx::query(
                    r#"UPDATE company_model_connections AS connection
                       SET api_key = $3, updated_at = CURRENT_TIMESTAMP
                       WHERE connection.company_id = $1
                         AND connection.provider = $2
                         AND connection.api_key = $4"#,
                )
                .bind(row.company_id)
                .bind(&row.provider)
                .bind(encrypted)
                .bind(&row.api_key)
                .execute(&mut *transaction)
                .await
                .map_err(AppError::from)?
                .rows_affected();
                if changed == 1 {
                    outcome.rotated += 1;
                } else {
                    outcome.cas_conflicts += 1;
                }
            }
            CredentialState::Unavailable { .. } => outcome.unavailable_rows += 1,
            CredentialState::Malformed => outcome.malformed_rows += 1,
        }
    }
    transaction.commit().await.map_err(AppError::from)?;
    Ok(outcome)
}

async fn credential_status_on_connection(
    connection: &mut PgConnection,
    cipher: &CredentialCipher,
) -> AppResult<CredentialStatus> {
    let mut status = CredentialStatus {
        active_version: cipher.active_version(),
        available_versions: cipher.available_versions(),
        total_rows: 0,
        active_rows: 0,
        old_rows: 0,
        malformed_rows: 0,
        unavailable_rows: 0,
        versions: BTreeMap::new(),
        unavailable_versions: BTreeMap::new(),
    };
    let mut cursor = None;
    loop {
        let rows = fetch_credential_batch(connection, cursor.as_ref()).await?;
        if rows.is_empty() {
            return Ok(status);
        }
        debug_assert!(rows.len() <= usize::try_from(CREDENTIAL_BATCH_SIZE).unwrap_or(usize::MAX));
        cursor = rows.last().map(|row| CredentialCursor {
            company_id: row.company_id,
            provider: row.provider.clone(),
        });
        for row in rows {
            status.total_rows += 1;
            match cipher.inspect(&row.api_key) {
                CredentialState::Active { version } => {
                    status.active_rows += 1;
                    *status.versions.entry(version).or_default() += 1;
                }
                CredentialState::Old { version } => {
                    status.old_rows += 1;
                    *status.versions.entry(version).or_default() += 1;
                }
                CredentialState::Unavailable { version } => {
                    status.unavailable_rows += 1;
                    *status.versions.entry(version).or_default() += 1;
                    *status.unavailable_versions.entry(version).or_default() += 1;
                }
                CredentialState::Malformed => status.malformed_rows += 1,
            }
        }
    }
}

async fn fetch_credential_batch(
    connection: &mut PgConnection,
    cursor: Option<&CredentialCursor>,
) -> AppResult<Vec<CredentialRow>> {
    let rows = match cursor {
        Some(cursor) => {
            sqlx::query_as::<_, CredentialRow>(
                r#"SELECT connection.company_id, connection.provider, connection.api_key
               FROM company_model_connections AS connection
               WHERE (connection.company_id, connection.provider) > ($1, $2)
               ORDER BY connection.company_id, connection.provider
               LIMIT $3"#,
            )
            .bind(cursor.company_id)
            .bind(&cursor.provider)
            .bind(CREDENTIAL_BATCH_SIZE)
            .fetch_all(&mut *connection)
            .await
        }
        None => {
            sqlx::query_as::<_, CredentialRow>(
                r#"SELECT connection.company_id, connection.provider, connection.api_key
               FROM company_model_connections AS connection
               ORDER BY connection.company_id, connection.provider
               LIMIT $1"#,
            )
            .bind(CREDENTIAL_BATCH_SIZE)
            .fetch_all(&mut *connection)
            .await
        }
    };
    rows.map_err(AppError::from)
}

async fn try_acquire_rotation_lock(connection: &mut PgConnection) -> AppResult<bool> {
    sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(CREDENTIAL_ROTATION_LOCK_ID)
        .fetch_one(connection)
        .await
        .map_err(AppError::from)
}

async fn release_rotation_lock(connection: &mut PgConnection) -> AppResult<()> {
    let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
        .bind(CREDENTIAL_ROTATION_LOCK_ID)
        .fetch_one(connection)
        .await
        .map_err(AppError::from)?;
    if !released {
        return Err(AppError::Internal(
            "Credential rotation database lock was lost before release".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::Executor;

    use super::*;
    use crate::adapters::persistence::test_support::test_pool;

    async fn isolated_connection() -> Option<PgConnection> {
        let pool = test_pool().await?;
        let options = pool.connect_options();
        let mut connection = PgConnection::connect_with(&options)
            .await
            .expect("an isolated credential-rotation test connection");
        connection
            .execute(
                r#"CREATE TEMPORARY TABLE company_model_connections (
                       company_id UUID NOT NULL,
                       provider TEXT NOT NULL,
                       api_key TEXT NOT NULL,
                       updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                       PRIMARY KEY (company_id, provider)
                   ) ON COMMIT PRESERVE ROWS"#,
            )
            .await
            .expect("a session-local credential table");
        Some(connection)
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
        let stale_rows = fetch_credential_batch(&mut connection, None).await.unwrap();
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
        let stored: String = sqlx::query_scalar(
            "SELECT api_key FROM company_model_connections WHERE company_id = $1",
        )
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

        let first_batch = fetch_credential_batch(&mut connection, None).await.unwrap();
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
}
