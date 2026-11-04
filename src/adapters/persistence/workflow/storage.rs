//! Bounded global maintenance. All mutation stays on one owned connection/transaction.
use super::*;
use crate::application::workflow::storage::*;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

async fn begin(pool: &sqlx::PgPool) -> AppResult<Transaction<'_, Postgres>> {
    let mut tx = pool.begin().await?;
    for setting in [
        "SET LOCAL statement_timeout='5s'",
        "SET LOCAL lock_timeout='500ms'",
        "SET LOCAL idle_in_transaction_session_timeout='5s'",
    ] {
        sqlx::query(setting).execute(&mut *tx).await?;
    }
    Ok(tx)
}
fn sweep_limit(limit: u16) -> AppResult<i64> {
    if !(1..=1024).contains(&limit) {
        return Err(AppError::BadRequest(
            "Invalid workflow storage sweep limit".into(),
        ));
    }
    Ok(i64::from(limit))
}
#[async_trait]
impl WorkflowStorageMaintenance for PostgresPersistence {
    async fn sweep_obsolete_witnesses(&self, limit: u16) -> AppResult<u64> {
        let limit = sweep_limit(limit)?;
        let mut tx = begin(&self.pool).await?;
        let mut count = 0;
        for (table, key) in [
            (
                "workflow_action_state_witnesses",
                "company_id,run_id,transaction_id",
            ),
            (
                "workflow_action_schedule_witnesses",
                "company_id,job_id,retired_attempt,transaction_id",
            ),
            (
                "workflow_action_claim_retirement_witnesses",
                "company_id,command_key,transaction_id",
            ),
        ] {
            let sql = format!("WITH obsolete AS MATERIALIZED (SELECT {key} FROM {table}
                WHERE transaction_id<>pg_current_xact_id() ORDER BY {key} LIMIT $1 FOR UPDATE SKIP LOCKED)
                DELETE FROM {table} AS witness USING obsolete WHERE ({})=({})",
                key.split(',').map(|k| format!("witness.{k}")).collect::<Vec<_>>().join(","),
                key.split(',').map(|k| format!("obsolete.{k}")).collect::<Vec<_>>().join(","));
            count += sqlx::query(&sql)
                .bind(limit)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        }
        tx.commit().await?;
        Ok(count)
    }
    async fn retention_page(&self, after: Option<RetentionRoot>) -> AppResult<RetentionPage> {
        let mut tx = begin(&self.pool).await?;
        let rows: Vec<(DateTime<Utc>,Uuid,Uuid)> = sqlx::query_as(
            "SELECT finished_at,company_id,id FROM workflow_runs WHERE parent_run_id IS NULL
             AND state IN ('succeeded','failed','cancelled') AND finished_at<clock_timestamp()-interval '90 days'
             AND ($1::timestamptz IS NULL OR (finished_at,company_id,id)>($1,$2,$3))
             ORDER BY finished_at,company_id,id LIMIT $4")
            .bind(after.map(|r| r.finished_at)).bind(after.map(|r| r.company.as_uuid()))
            .bind(after.map(|r| r.run.as_uuid())).bind(RETENTION_PAGE_SIZE as i64)
            .fetch_all(&mut *tx).await?;
        tx.commit().await?;
        let roots: Vec<_> = rows
            .into_iter()
            .map(|(finished_at, company, run)| RetentionRoot {
                finished_at,
                company: CompanyId::new(company),
                run: RunId::new(run),
            })
            .collect();
        let next = (roots.len() == RETENTION_PAGE_SIZE).then(|| *roots.last().expect("full page"));
        Ok(RetentionPage { roots, next })
    }
    async fn retire_tree(&self, root: RetentionRoot) -> AppResult<Option<RetiredTree>> {
        let mut tx = begin(&self.pool).await?;
        let result = super::storage_retention::retire_on(&mut tx, root).await?;
        tx.commit().await?;
        Ok(result)
    }
    async fn sweep_unused_bundles(&self, limit: u16) -> AppResult<u64> {
        let limit = sweep_limit(limit)?;
        let mut tx = begin(&self.pool).await?;
        let count = sqlx::query("WITH unused AS MATERIALIZED (
            SELECT blob.company_id,blob.digest FROM workflow_bundles AS blob
            WHERE NOT EXISTS(SELECT 1 FROM workflow_runs AS run WHERE run.company_id=blob.company_id AND run.bundle_digest=blob.digest)
            ORDER BY blob.company_id,blob.digest LIMIT $1 FOR UPDATE OF blob SKIP LOCKED)
            DELETE FROM workflow_bundles AS blob USING unused WHERE blob.company_id=unused.company_id AND blob.digest=unused.digest
            AND NOT EXISTS(SELECT 1 FROM workflow_runs AS run WHERE run.company_id=blob.company_id AND run.bundle_digest=blob.digest)")
            .bind(limit).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        Ok(count)
    }
    async fn oldest_terminal_root(&self) -> AppResult<Option<DateTime<Utc>>> {
        let mut tx = begin(&self.pool).await?;
        let oldest = sqlx::query_scalar("SELECT min(finished_at) FROM workflow_runs WHERE parent_run_id IS NULL AND state IN ('succeeded','failed','cancelled')")
            .fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(oldest)
    }
}
