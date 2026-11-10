//! Retention of settled memory intents and lifecycle rows (p06 gate A3-1, A3-2; F1, F3).
//!
//! Each sweep is one statement: candidates are taken `FOR UPDATE SKIP LOCKED` in the order of
//! their partial index, so competing sweepers skip each other and never wait, and the delete
//! applies to exactly those rows. Their predicates cannot turn false again: a terminal intent is
//! final, a run past its deadline admits no control retry, and a removed namespace or company id
//! is never reused.
use crate::adapters::persistence::PostgresPersistence;
use crate::app_error::AppResult;
use crate::application::memory::{
    MemoryMaintenanceStore, MemoryRowCounts, SETTLED_INTENT_RETENTION_DAYS,
    SETTLED_LIFECYCLE_RETENTION_DAYS, STUCK_TRANSIENT_RETRIES,
};
use async_trait::async_trait;

/// Settled intents nothing can replay or read: terminal, past the service's idempotency window,
/// and a capture (enqueued once, at exchange acceptance) or a save whose run is retired or
/// terminal with its deadline passed. A live run's parked step reads its receipt through
/// `wait_id`, and a control retry can reopen a failed run before its deadline.
const SETTLED_INTENT: &str = "intent.state IN ('delivered', 'failed', 'expired', 'cancelled') \
     AND intent.created_at < now() - make_interval(days => $1) \
     AND (intent.kind = 'generation' OR NOT EXISTS (SELECT 1 FROM workflow_runs AS run \
          WHERE run.company_id = intent.company_id AND run.id = intent.run_id \
            AND (run.state NOT IN ('succeeded', 'failed', 'cancelled') \
                 OR run.deadline >= now())))";

/// Settled absent lifecycle rows whose owner row is gone, 30 days after they settled.
const SETTLED_LIFECYCLE: &str = "lifecycle.state = 'settled' AND lifecycle.desired = 'absent' \
     AND lifecycle.observed = 'absent' \
     AND lifecycle.updated_at < now() - make_interval(days => $1) \
     AND CASE WHEN lifecycle.resource = 'namespace' \
              THEN NOT EXISTS (SELECT 1 FROM memory_namespaces AS namespace \
                    WHERE namespace.company_id = lifecycle.company_id \
                      AND namespace.id = lifecycle.namespace_id) \
              ELSE NOT EXISTS (SELECT 1 FROM memory_tenants AS tenant \
                    WHERE tenant.company_id = lifecycle.company_id) END";

/// `$1` the retention in days, `$2` the batch.
pub(super) fn delivery_sweep() -> String {
    format!(
        "WITH candidate AS (SELECT intent.company_id, intent.id FROM memory_deliveries AS intent \
           WHERE {SETTLED_INTENT} ORDER BY intent.created_at, intent.id LIMIT $2 \
           FOR UPDATE OF intent SKIP LOCKED) \
         DELETE FROM memory_deliveries AS intent USING candidate \
          WHERE (intent.company_id, intent.id) = (candidate.company_id, candidate.id)"
    )
}

/// `$1` the retention in days, `$2` the batch.
pub(super) fn lifecycle_sweep() -> String {
    format!(
        "WITH candidate AS (SELECT lifecycle.id FROM memory_lifecycles AS lifecycle \
           WHERE {SETTLED_LIFECYCLE} ORDER BY lifecycle.updated_at, lifecycle.id LIMIT $2 \
           FOR UPDATE OF lifecycle SKIP LOCKED) \
         DELETE FROM memory_lifecycles AS lifecycle USING candidate \
          WHERE lifecycle.id = candidate.id"
    )
}

#[async_trait]
impl MemoryMaintenanceStore for PostgresPersistence {
    async fn sweep_settled_deliveries(&self, limit: u16) -> AppResult<u64> {
        Ok(sqlx::query(&delivery_sweep())
            .bind(SETTLED_INTENT_RETENTION_DAYS)
            .bind(i64::from(limit))
            .execute(self.pool())
            .await?
            .rows_affected())
    }

    async fn sweep_settled_lifecycles(&self, limit: u16) -> AppResult<u64> {
        Ok(sqlx::query(&lifecycle_sweep())
            .bind(SETTLED_LIFECYCLE_RETENTION_DAYS)
            .bind(i64::from(limit))
            .execute(self.pool())
            .await?
            .rows_affected())
    }

    async fn memory_row_counts(&self) -> AppResult<MemoryRowCounts> {
        let sql = format!(
            "SELECT \
               (SELECT count(*) FROM memory_deliveries WHERE state = 'pending'), \
               (SELECT count(*) FROM memory_deliveries WHERE state = 'claimed'), \
               (SELECT count(*) FROM memory_lifecycles WHERE state = 'pending'), \
               (SELECT count(*) FROM memory_lifecycles WHERE state = 'claimed'), \
               (SELECT count(*) FROM memory_lifecycles WHERE state = 'failed'), \
               (SELECT count(*) FROM memory_lifecycles \
                 WHERE state IN ('pending', 'claimed') AND transient_retries >= $3), \
               (SELECT count(*) FROM memory_lifecycles \
                 WHERE state = 'pending' AND outcome_code = 'provisioner_mismatch'), \
               (SELECT count(*) FROM memory_lifecycles \
                 WHERE state = 'pending' AND outcome_code = 'tenant_disabled'), \
               (SELECT count(*) FROM memory_lifecycles \
                 WHERE state = 'pending' AND outcome_code = 'tenant_recheck'), \
               (SELECT count(*) FROM memory_deliveries AS intent WHERE {SETTLED_INTENT}), \
               (SELECT count(*) FROM memory_lifecycles AS lifecycle WHERE {})",
            SETTLED_LIFECYCLE.replace("$1", "$2"),
        );
        let row: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(&sql)
            .bind(SETTLED_INTENT_RETENTION_DAYS)
            .bind(SETTLED_LIFECYCLE_RETENTION_DAYS)
            .bind(STUCK_TRANSIENT_RETRIES)
            .fetch_one(self.pool())
            .await?;
        Ok(MemoryRowCounts {
            deliveries_pending: row.0,
            deliveries_claimed: row.1,
            lifecycles_pending: row.2,
            lifecycles_claimed: row.3,
            lifecycles_failed: row.4,
            lifecycles_stuck: row.5,
            lifecycles_provisioner_mismatch: row.6,
            lifecycles_tenant_disabled: row.7,
            lifecycles_tenant_recheck: row.8,
            delivery_backlog: row.9,
            lifecycle_backlog: row.10,
        })
    }
}
