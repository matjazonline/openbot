use super::*;
use crate::{
    adapters::persistence::credentials::envelope::CredentialContext,
    app_error::{AppError, AppResult},
};
use sqlx::PgConnection;
pub(crate) async fn page(
    db: &mut PgConnection,
    after: Option<Uuid>,
    limit: i64,
) -> AppResult<Vec<super::super::inventory::VaultInventoryEntry>> {
    if !(1..=100).contains(&limit) {
        return Err(AppError::Internal("Invalid connect inventory page".into()));
    }
    let rows:Vec<(Uuid,String,String)>=sqlx::query_as("SELECT id,facts,envelope FROM integration_connect_attempts WHERE envelope IS NOT NULL AND ($1::uuid IS NULL OR id>$1) ORDER BY id LIMIT $2")
        .bind(after).bind(limit).fetch_all(db).await?;
    rows.into_iter()
        .map(|(id, facts, envelope)| {
            if facts.len() > MAX_METADATA_BYTES || envelope.len() > 23000 {
                return Err(AppError::Internal("Invalid connect inventory row".into()));
            }
            let facts: AttemptFacts = serde_json::from_str(&facts)
                .map_err(|_| AppError::Internal("Invalid connect inventory facts".into()))?;
            if facts.id.as_uuid() != id {
                return Err(AppError::Internal(
                    "Invalid connect inventory identity".into(),
                ));
            }
            Ok(super::super::inventory::VaultInventoryEntry {
                id,
                context: CredentialContext::integration_connect(&facts),
                envelope,
            })
        })
        .collect()
}
pub(crate) async fn rewrap(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    old: &str,
    new: &str,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE integration_connect_attempts SET envelope=$2 WHERE id=$1 AND envelope=$3 AND status='pending'").bind(id).bind(new).bind(old).execute(&mut **tx).await
}

#[cfg(test)]
pub(crate) const ISOLATED_TABLE_SQL: &str = "CREATE TEMPORARY TABLE integration_connect_attempts (LIKE public.integration_connect_attempts INCLUDING ALL) ON COMMIT PRESERVE ROWS";
