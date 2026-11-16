//! Bounded global maintenance. All mutation stays on one owned connection/transaction.
use super::*;
use crate::application::workflow::storage::*;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

pub(super) async fn begin(pool: &sqlx::PgPool) -> AppResult<Transaction<'_, Postgres>> {
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
    async fn sweep_notification_origins(
        &self,
        after: Option<NotificationOriginCursor>,
        limit: u16,
    ) -> AppResult<NotificationOriginSweep> {
        #[derive(sqlx::FromRow)]
        struct ScannedOrigin {
            company_id: Uuid,
            origin_id: Uuid,
            deleted: bool,
        }
        let limit = sweep_limit(limit)?;
        let mut tx = begin(&self.pool).await?;
        // Bound discovery before reference checks and locking. Advancing over the
        // whole scanned page also makes progress when every row is retained or busy.
        let mut query = sqlx::QueryBuilder::<Postgres>::new(
            "WITH page AS MATERIALIZED (SELECT company_id,id FROM workflow_notification_origins WHERE notice_id IS NULL",
        );
        if let Some(after) = after {
            query
                .push(" AND (company_id,id)>(")
                .push_bind(after.company.as_uuid())
                .push(",")
                .push_bind(after.origin)
                .push(")");
        }
        query.push(" ORDER BY company_id,id LIMIT ").push_bind(limit).push(") , \
            locked AS MATERIALIZED (SELECT origin.company_id,origin.id FROM workflow_notification_origins AS origin \
                JOIN page ON page.company_id=origin.company_id AND page.id=origin.id \
                ORDER BY origin.company_id,origin.id FOR UPDATE OF origin SKIP LOCKED), \
            removed AS (DELETE FROM workflow_notification_origins AS origin USING locked \
                WHERE origin.company_id=locked.company_id AND origin.id=locked.id AND origin.notice_id IS NULL \
                  AND NOT EXISTS(SELECT 1 FROM workflow_escalation_notice_targets AS target WHERE target.company_id=origin.company_id AND target.notice_id=origin.id) \
                  AND NOT EXISTS(SELECT 1 FROM workflow_message_notification_origins AS message WHERE message.company_id=origin.company_id AND message.origin_id=origin.id) \
                  AND NOT EXISTS(SELECT 1 FROM workflow_run_notification_origins AS run WHERE run.company_id=origin.company_id AND run.origin_id=origin.id) \
                RETURNING origin.company_id,origin.id) \
            SELECT page.company_id,page.id AS origin_id,removed.id IS NOT NULL AS deleted \
            FROM page LEFT JOIN removed ON removed.company_id=page.company_id AND removed.id=page.id \
            ORDER BY page.company_id,page.id");
        let scanned: Vec<ScannedOrigin> = query.build_query_as().fetch_all(&mut *tx).await?;
        let next = scanned
            .last()
            .filter(|_| scanned.len() == limit as usize)
            .map(|last| NotificationOriginCursor {
                company: CompanyId::new(last.company_id),
                origin: last.origin_id,
            });
        let deleted = scanned.iter().filter(|origin| origin.deleted).count() as u64;
        tx.commit().await?;
        Ok(NotificationOriginSweep { deleted, next })
    }

    async fn history_page(&self, after: Option<HistoryCandidate>) -> AppResult<HistoryPage> {
        super::agent_history_retention::page(&self.pool, after).await
    }
    async fn expire_history(&self, candidate: HistoryCandidate) -> AppResult<bool> {
        super::agent_history_retention::expire(&self.pool, candidate).await
    }
    async fn history_scope_page(&self, after: Option<CompanyId>) -> AppResult<Vec<CompanyId>> {
        super::agent_history_retention::scope_page(&self.pool, after).await
    }
    async fn prune_history(&self, company: CompanyId, limit: u16) -> AppResult<u64> {
        super::agent_history_retention::prune(&self.pool, company, limit).await
    }
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
