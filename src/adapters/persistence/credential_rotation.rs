use std::{collections::BTreeMap, str::FromStr, time::Instant};

use serde::Serialize;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use super::{
    PostgresPersistence,
    credentials::{CredentialCipher, CredentialContext, CredentialFormat, CredentialState},
};
use crate::{
    app_error::{AppError, AppResult},
    entities::transport::{IntegrationCredentialKind, TransportKind},
};

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

/// The tables that hold a protected secret.
///
/// Status and rotation walk every one of them. A table added here but not to this list is a table
/// whose rows never rotate, and nothing else in the system would notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialTable {
    /// `company_model_connections.api_key` — the legacy `enc:v1` direct-key format.
    ModelConnections,
    /// `integration_credentials.envelope` — `enc:v2`, rotated by rewrapping the data key.
    IntegrationCredentials,
}

impl CredentialTable {
    const ALL: &'static [Self] = &[Self::ModelConnections, Self::IntegrationCredentials];
}

/// Where one credential lives, in the terms its own table keys it by.
///
/// Doubles as the keyset cursor: both tables are scanned in their primary-key order, so the last
/// key of a batch is exactly where the next batch starts.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CredentialKey {
    ModelConnection {
        company_id: Uuid,
        provider: String,
    },
    IntegrationCredential {
        company_id: Uuid,
        installation_id: Uuid,
        credential_kind: String,
    },
}

/// One credential row, with the format its column is written in.
///
/// The format comes from the *table*, never from the stored string: letting a value pick its own
/// reader is how a plaintext row talks its way past a validity check.
#[derive(Debug, Clone)]
struct InventoryRow {
    key: CredentialKey,
    format: CredentialFormat,
    stored: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ModelConnectionRow {
    company_id: Uuid,
    provider: String,
    api_key: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct IntegrationCredentialRow {
    company_id: Uuid,
    installation_id: Uuid,
    credential_kind: String,
    envelope: String,
    transport: String,
}

impl From<ModelConnectionRow> for InventoryRow {
    fn from(row: ModelConnectionRow) -> Self {
        Self {
            key: CredentialKey::ModelConnection {
                company_id: row.company_id,
                provider: row.provider,
            },
            format: CredentialFormat::LegacyDirectKey,
            stored: row.api_key,
        }
    }
}

impl TryFrom<IntegrationCredentialRow> for InventoryRow {
    type Error = AppError;

    /// A transport or credential kind the current build does not know is schema drift, not a bad
    /// credential: both columns are `CHECK`-constrained, so reaching this arm means the database
    /// was edited past what this build understands. Failing the scan is the honest answer.
    fn try_from(row: IntegrationCredentialRow) -> AppResult<Self> {
        let transport = TransportKind::from_str(&row.transport).map_err(|error| {
            AppError::Internal(format!(
                "Invalid integration_installations.transport: {error}"
            ))
        })?;
        let credential_kind =
            IntegrationCredentialKind::from_str(&row.credential_kind).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid integration_credentials.credential_kind: {error}"
                ))
            })?;
        Ok(Self {
            format: CredentialFormat::Envelope(CredentialContext::integration_credential(
                row.company_id,
                row.installation_id,
                transport,
                credential_kind,
            )),
            key: CredentialKey::IntegrationCredential {
                company_id: row.company_id,
                installation_id: row.installation_id,
                credential_kind: row.credential_kind,
            },
            stored: row.envelope,
        })
    }
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
    for table in CredentialTable::ALL {
        rotate_table(connection, cipher, *table, &mut outcome).await?;
    }
    Ok(outcome)
}

/// Walk one table in keyset-cursored batches, rotating what needs it.
///
/// Bounded on purpose: the batch is capped, the cursor is the primary key, and each batch commits
/// on its own, so a rotation over a large table neither loads it into memory nor holds one
/// transaction open across the whole scan.
async fn rotate_table(
    connection: &mut PgConnection,
    cipher: &CredentialCipher,
    table: CredentialTable,
    outcome: &mut RotationPassOutcome,
) -> AppResult<()> {
    let mut cursor = None;
    loop {
        let rows = fetch_credential_batch(connection, table, cursor.as_ref()).await?;
        if rows.is_empty() {
            return Ok(());
        }
        debug_assert!(rows.len() <= usize::try_from(CREDENTIAL_BATCH_SIZE).unwrap_or(usize::MAX));
        cursor = rows.last().map(|row| row.key.clone());
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
    rows: Vec<InventoryRow>,
) -> AppResult<RotationBatchOutcome> {
    let mut outcome = RotationBatchOutcome::default();
    let mut transaction = connection.begin().await.map_err(AppError::from)?;
    for row in rows {
        match cipher.classify(&row.format, &row.stored) {
            CredentialState::Active { .. } => {}
            CredentialState::Old { .. } => {
                let rotated = cipher.rotate_to_active(&row.format, &row.stored)?;
                if store_rotated_credential(&mut transaction, &row, &rotated).await? {
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

/// Compare-and-swap on the stored value, so a rotation that read a row before a normal writer
/// replaced it loses rather than clobbering the newer credential.
async fn store_rotated_credential(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &InventoryRow,
    rotated: &str,
) -> AppResult<bool> {
    let changed = match &row.key {
        CredentialKey::ModelConnection {
            company_id,
            provider,
        } => {
            sqlx::query(
                r#"UPDATE company_model_connections AS connection
               SET api_key = $3, updated_at = CURRENT_TIMESTAMP
               WHERE connection.company_id = $1
                 AND connection.provider = $2
                 AND connection.api_key = $4"#,
            )
            .bind(company_id)
            .bind(provider)
            .bind(rotated)
            .bind(&row.stored)
            .execute(&mut **transaction)
            .await
        }
        CredentialKey::IntegrationCredential {
            company_id,
            installation_id,
            credential_kind,
        } => {
            sqlx::query(
                r#"UPDATE integration_credentials AS credential
               SET envelope = $4, updated_at = CURRENT_TIMESTAMP
               WHERE credential.company_id = $1
                 AND credential.installation_id = $2
                 AND credential.credential_kind = $3
                 AND credential.envelope = $5"#,
            )
            .bind(company_id)
            .bind(installation_id)
            .bind(credential_kind)
            .bind(rotated)
            .bind(&row.stored)
            .execute(&mut **transaction)
            .await
        }
    }
    .map_err(AppError::from)?
    .rows_affected();

    Ok(changed == 1)
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
    for table in CredentialTable::ALL {
        let mut cursor = None;
        loop {
            let rows = fetch_credential_batch(connection, *table, cursor.as_ref()).await?;
            if rows.is_empty() {
                break;
            }
            debug_assert!(
                rows.len() <= usize::try_from(CREDENTIAL_BATCH_SIZE).unwrap_or(usize::MAX)
            );
            cursor = rows.last().map(|row| row.key.clone());
            for row in rows {
                status.total_rows += 1;
                match cipher.classify(&row.format, &row.stored) {
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
    Ok(status)
}

/// One bounded batch of `table`, starting after `cursor` in primary-key order.
///
/// The cursor is the table's own key rather than an offset, so a scan stays O(batch) however far
/// into the table it has walked, and a row inserted behind the cursor is picked up by the next
/// full pass rather than shifting the window.
async fn fetch_credential_batch(
    connection: &mut PgConnection,
    table: CredentialTable,
    cursor: Option<&CredentialKey>,
) -> AppResult<Vec<InventoryRow>> {
    match table {
        CredentialTable::ModelConnections => fetch_model_connection_batch(connection, cursor).await,
        CredentialTable::IntegrationCredentials => {
            fetch_integration_credential_batch(connection, cursor).await
        }
    }
}

async fn fetch_model_connection_batch(
    connection: &mut PgConnection,
    cursor: Option<&CredentialKey>,
) -> AppResult<Vec<InventoryRow>> {
    const COLUMNS: &str = "connection.company_id, connection.provider, connection.api_key";
    let rows = match cursor {
        Some(CredentialKey::ModelConnection {
            company_id,
            provider,
        }) => {
            sqlx::query_as::<_, ModelConnectionRow>(&format!(
                "SELECT {COLUMNS}
                   FROM company_model_connections AS connection
                  WHERE (connection.company_id, connection.provider) > ($1, $2)
                  ORDER BY connection.company_id, connection.provider
                  LIMIT $3"
            ))
            .bind(company_id)
            .bind(provider)
            .bind(CREDENTIAL_BATCH_SIZE)
            .fetch_all(&mut *connection)
            .await
        }
        _ => {
            sqlx::query_as::<_, ModelConnectionRow>(&format!(
                "SELECT {COLUMNS}
                   FROM company_model_connections AS connection
                  ORDER BY connection.company_id, connection.provider
                  LIMIT $1"
            ))
            .bind(CREDENTIAL_BATCH_SIZE)
            .fetch_all(&mut *connection)
            .await
        }
    };
    Ok(rows
        .map_err(AppError::from)?
        .into_iter()
        .map(InventoryRow::from)
        .collect())
}

/// The installation join carries the transport, which is part of every envelope's authenticated
/// context: rotation cannot rewrap a row without knowing which account it belongs to, and reading
/// it from the row rather than assuming a literal is what keeps a second provider correct.
async fn fetch_integration_credential_batch(
    connection: &mut PgConnection,
    cursor: Option<&CredentialKey>,
) -> AppResult<Vec<InventoryRow>> {
    const SELECT: &str = "SELECT credential.company_id, credential.installation_id,
                                 credential.credential_kind, credential.envelope,
                                 installation.transport
                            FROM integration_credentials AS credential
                            JOIN integration_installations AS installation
                              ON installation.company_id = credential.company_id
                             AND installation.id = credential.installation_id";
    const ORDER: &str = "ORDER BY credential.company_id, credential.installation_id,
                                  credential.credential_kind";
    let rows = match cursor {
        Some(CredentialKey::IntegrationCredential {
            company_id,
            installation_id,
            credential_kind,
        }) => {
            sqlx::query_as::<_, IntegrationCredentialRow>(&format!(
                "{SELECT}
                  WHERE (credential.company_id, credential.installation_id,
                         credential.credential_kind) > ($1, $2, $3)
                  {ORDER}
                  LIMIT $4"
            ))
            .bind(company_id)
            .bind(installation_id)
            .bind(credential_kind)
            .bind(CREDENTIAL_BATCH_SIZE)
            .fetch_all(&mut *connection)
            .await
        }
        _ => {
            sqlx::query_as::<_, IntegrationCredentialRow>(&format!("{SELECT} {ORDER} LIMIT $1"))
                .bind(CREDENTIAL_BATCH_SIZE)
                .fetch_all(&mut *connection)
                .await
        }
    };
    rows.map_err(AppError::from)?
        .into_iter()
        .map(InventoryRow::try_from)
        .collect()
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
#[path = "credential_rotation_tests.rs"]
mod tests;
