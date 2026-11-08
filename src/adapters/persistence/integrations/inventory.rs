use super::{
    super::credentials::envelope::CredentialContext,
    rows::{AccountRow, COLUMNS},
};
use crate::app_error::{AppError, AppResult};
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;
pub(crate) struct VaultInventoryEntry {
    pub id: Uuid,
    pub context: CredentialContext,
    pub envelope: String,
}
#[derive(sqlx::FromRow)]
struct InventoryDb {
    #[sqlx(flatten)]
    account: AccountRow,
    envelope: String,
}
pub(crate) async fn page(
    connection: &mut PgConnection,
    after: Option<Uuid>,
    limit: i64,
) -> AppResult<Vec<VaultInventoryEntry>> {
    if !(1..=100).contains(&limit) {
        return Err(AppError::Internal("Invalid vault inventory page".into()));
    }
    let rows=sqlx::query_as::<_,InventoryDb>(&format!("SELECT {COLUMNS},envelope FROM integration_accounts WHERE envelope IS NOT NULL AND ($1::uuid IS NULL OR id>$1) ORDER BY id LIMIT $2"))
        .bind(after).bind(limit).fetch_all(connection).await.map_err(|_|AppError::Internal("Vault inventory unavailable".into()))?;
    rows.into_iter()
        .map(|row| {
            let account = row
                .account
                .decode()
                .map_err(|_| AppError::Internal("Invalid vault inventory metadata".into()))?;
            if row.envelope.len() > 23000 {
                return Err(AppError::Internal(
                    "Invalid vault inventory envelope".into(),
                ));
            }
            Ok(VaultInventoryEntry {
                id: account.id.as_uuid(),
                context: CredentialContext::integration_account(&account),
                envelope: row.envelope,
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
    sqlx::query("UPDATE integration_accounts SET envelope=$2 WHERE id=$1 AND envelope=$3")
        .bind(id)
        .bind(new)
        .bind(old)
        .execute(&mut **tx)
        .await
}

#[cfg(test)]
pub(crate) const ISOLATED_TABLE_SQL: &str = "CREATE TEMPORARY TABLE integration_accounts (LIKE public.integration_accounts INCLUDING ALL) ON COMMIT PRESERVE ROWS";
