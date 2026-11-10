//! Per-namespace client credentials as the credential-rotation inventory walks them (E1).
use crate::adapters::persistence::credentials::envelope::CredentialContext;
use crate::app_error::{AppError, AppResult};
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

pub(crate) struct MemoryCredentialEntry {
    pub company_id: Uuid,
    pub namespace_id: Uuid,
    pub context: CredentialContext,
    pub envelope: String,
}

#[derive(sqlx::FromRow)]
struct Row {
    company_id: Uuid,
    id: Uuid,
    credential_envelope: String,
}

/// One keyset page in primary-key order, so the last key of a page is where the next starts.
pub(crate) async fn page(
    connection: &mut PgConnection,
    after: Option<(Uuid, Uuid)>,
    limit: i64,
) -> AppResult<Vec<MemoryCredentialEntry>> {
    if !(1..=100).contains(&limit) {
        return Err(AppError::Internal("Invalid memory credential page".into()));
    }
    let (company, namespace) = after.unzip();
    let rows = sqlx::query_as::<_, Row>(
        "SELECT namespace.company_id, namespace.id, namespace.credential_envelope \
         FROM memory_namespaces AS namespace \
         WHERE namespace.credential_envelope IS NOT NULL \
           AND ($1::uuid IS NULL OR (namespace.company_id, namespace.id) > ($1, $2)) \
         ORDER BY namespace.company_id, namespace.id LIMIT $3",
    )
    .bind(company)
    .bind(namespace)
    .bind(limit)
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| MemoryCredentialEntry {
            company_id: row.company_id,
            namespace_id: row.id,
            context: CredentialContext::memory_client_credential(row.company_id, row.id),
            envelope: row.credential_envelope,
        })
        .collect())
}

/// Compare-and-swap, so a rotation never clobbers a credential reissued after it read the row.
pub(crate) async fn rewrap(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    namespace_id: Uuid,
    old: &str,
    new: &str,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "UPDATE memory_namespaces AS namespace SET credential_envelope = $3 \
         WHERE namespace.company_id = $1 AND namespace.id = $2 AND namespace.credential_envelope = $4",
    )
    .bind(company_id)
    .bind(namespace_id)
    .bind(new)
    .bind(old)
    .execute(&mut **tx)
    .await
}
