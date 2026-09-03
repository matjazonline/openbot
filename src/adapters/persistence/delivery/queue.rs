//! The delivery queue protocol in SQL: one atomic claim, and one fenced write per transition.
//!
//! Every statement below names the claiming execution in its `WHERE` clause. That is the whole
//! protocol: a run whose lease was reaped and re-claimed by someone else finds every write it
//! attempts affecting zero rows, and reports `LeaseLost` rather than overwriting the replacement
//! execution's result.
//!
//! The parent delivery is the only leased object. Part transitions join back to the parent and
//! re-check that same execution, so a part is never independently claimable -- which is what keeps
//! a multi-part send from growing a second ownership state machine for one provider call.

use std::{str::FromStr, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{
    DeliveryDb, attempt_failed_set, claimable_statuses_sql,
    enqueue::{insert_standalone_delivery_on, load_parts_on},
    qualified_delivery_columns, retry_delay_sql, sql_status_list,
};
use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::transport::{
        DeliveryId, DeliveryPartId, DeliveryPartStatus, DeliveryStatus, FailureClass,
        aggregate_parent_status,
    },
    transport::{
        ClaimedDelivery, DeliveryCreation, DeliveryFailure, DeliveryOutcome, DeliveryQueue,
        DeliveryReaping, Disposition, ExecutionLease, NewStandaloneDelivery, PartResult,
        PartTransition, StandaloneDeliveryEnqueuer, WorkerId,
    },
};

#[async_trait]
impl StandaloneDeliveryEnqueuer for PostgresPersistence {
    async fn enqueue_standalone_delivery(
        &self,
        delivery: NewStandaloneDelivery,
    ) -> AppResult<DeliveryCreation> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let created = insert_standalone_delivery_on(&mut tx, &delivery).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(created)
    }
}

#[async_trait]
impl DeliveryQueue for PostgresPersistence {
    async fn claim_deliveries(
        &self,
        owner: WorkerId,
        lease_for: Duration,
        limit: i64,
    ) -> AppResult<Vec<ClaimedDelivery>> {
        let lease_seconds = i64::try_from(lease_for.as_secs()).unwrap_or(i64::MAX);
        let claimable = claimable_statuses_sql();
        let sending = DeliveryStatus::Sending.as_str();
        let delivered = DeliveryStatus::Delivered.as_str();

        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        // One `UPDATE ... FROM (SELECT ... FOR UPDATE SKIP LOCKED)`, ordered by `(available_at,
        // id)` so two claimants never contend for the same row and neither starves. Global rather
        // than tenant-scoped, because that is what a worker is; fairness across tenants is the
        // scheduling decision the worker makes with what it is given.
        //
        // The dependency arm is what stops a mirror overtaking the root it threads under: a row
        // whose predecessor is not yet delivered is simply not claimable. A predecessor that has
        // gone terminal is not skipped for ever either -- `reap_expired_deliveries` dead-letters
        // its descendants with a typed causal reason.
        let rows = sqlx::query_as::<_, DeliveryDb>(&format!(
            r#"WITH claimable AS (
                   SELECT delivery.id
                     FROM message_deliveries AS delivery
                    WHERE delivery.status IN ({claimable})
                      AND delivery.available_at <= CURRENT_TIMESTAMP
                      AND (
                          delivery.depends_on_delivery_id IS NULL
                          OR EXISTS (
                              SELECT 1 FROM message_deliveries AS dependency
                               WHERE dependency.id = delivery.depends_on_delivery_id
                                 AND dependency.status = '{delivered}'
                          )
                      )
                    ORDER BY delivery.available_at, delivery.id
                      FOR UPDATE SKIP LOCKED
                    LIMIT $1
               )
               UPDATE message_deliveries AS delivery
                  SET status = '{sending}',
                      execution_id = gen_random_uuid(),
                      owner_worker_id = $2,
                      locked_at = CURRENT_TIMESTAMP,
                      lock_expires_at = CURRENT_TIMESTAMP + make_interval(secs => $3::double precision),
                      updated_at = CURRENT_TIMESTAMP
                 FROM claimable
                WHERE delivery.id = claimable.id
             RETURNING {returning}"#,
            returning = qualified_delivery_columns("delivery"),
        ))
        .bind(limit)
        .bind(owner.as_uuid())
        .bind(lease_seconds as f64)
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::from)?;

        let mut claimed = Vec::with_capacity(rows.len());
        for row in rows {
            let lease = row.lease().ok_or_else(|| {
                AppError::Internal(format!(
                    "Delivery {} was claimed but carries no lease",
                    row.id
                ))
            })?;
            let parts = load_parts_on(&mut tx, lease.row).await?;
            claimed.push(ClaimedDelivery {
                lease,
                record: row.record()?,
                parts: parts
                    .iter()
                    .map(super::PartDb::stored)
                    .collect::<AppResult<Vec<_>>>()?,
            });
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok(claimed)
    }

    async fn renew_delivery_lease(
        &self,
        fence: &ExecutionLease<DeliveryId>,
        until: DateTime<Utc>,
    ) -> AppResult<bool> {
        let sending = DeliveryStatus::Sending.as_str();
        let result = sqlx::query(&format!(
            r#"UPDATE message_deliveries
                  SET lock_expires_at = $4, updated_at = CURRENT_TIMESTAMP
                WHERE id = $1 AND status = '{sending}'
                  AND execution_id = $2 AND owner_worker_id = $3
                  AND lock_expires_at > CURRENT_TIMESTAMP"#
        ))
        .bind(fence.row.as_uuid())
        .bind(fence.execution.as_uuid())
        .bind(fence.owner.as_uuid())
        .bind(until)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn begin_part(
        &self,
        fence: &ExecutionLease<DeliveryId>,
        part_id: DeliveryPartId,
    ) -> AppResult<DeliveryOutcome> {
        let sending_part = DeliveryPartStatus::Sending.as_str();
        let claimable_part =
            sql_part_list(&[DeliveryPartStatus::Prepared, DeliveryPartStatus::Retryable]);
        // Joined to the parent and fenced on its execution: the part has no lease of its own, so
        // "may I send this?" is answered entirely by whether this run still owns the delivery.
        let result = sqlx::query(&format!(
            r#"UPDATE message_delivery_parts AS part
                  SET status = '{sending_part}',
                      attempt_count = part.attempt_count + 1,
                      request_started_at = CURRENT_TIMESTAMP,
                      updated_at = CURRENT_TIMESTAMP
                 FROM message_deliveries AS delivery
                WHERE part.id = $1 AND part.delivery_id = delivery.id
                  AND part.status IN ({claimable_part})
                  AND {live_fence}"#,
            live_fence = live_fence_sql(),
        ))
        .bind(part_id.as_uuid())
        .bind(fence.row.as_uuid())
        .bind(fence.execution.as_uuid())
        .bind(fence.owner.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        if result.rows_affected() == 1 {
            return Ok(DeliveryOutcome::Applied(DeliveryStatus::Sending));
        }
        Ok(DeliveryOutcome::LeaseLost)
    }

    async fn complete_part(&self, result: PartResult<'_>) -> AppResult<DeliveryOutcome> {
        let transition = PartTransition::of(result.outcome);
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        let updated = sqlx::query(&format!(
            r#"UPDATE message_delivery_parts AS part
                  SET status = $5,
                      provider_message_key = COALESCE($6, part.provider_message_key),
                      last_error_class = $7,
                      last_error_detail = $8,
                      delivered_at = CASE WHEN $5 = '{delivered_part}'
                                          THEN CURRENT_TIMESTAMP ELSE NULL END,
                      updated_at = CURRENT_TIMESTAMP
                 FROM message_deliveries AS delivery
                WHERE part.id = $1 AND part.delivery_id = delivery.id
                  AND {live_fence}"#,
            delivered_part = DeliveryPartStatus::Delivered.as_str(),
            live_fence = live_fence_sql(),
        ))
        .bind(result.part_id.as_uuid())
        .bind(result.fence.row.as_uuid())
        .bind(result.fence.execution.as_uuid())
        .bind(result.fence.owner.as_uuid())
        .bind(transition.status.as_str())
        .bind(
            transition
                .provider_key
                .as_ref()
                .map(|key| key.as_str().to_string()),
        )
        .bind(transition.class.map(|class| class.as_str()))
        .bind(transition.detail.as_ref().map(|detail| detail.as_str()))
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        if updated.rows_affected() != 1 {
            // Either this run no longer owns the delivery, or the part is not one of its own.
            // Rolled back rather than partially applied: the replacement execution owns the
            // outcome now, and this run's provider call is that execution's problem to reconcile.
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(DeliveryOutcome::LeaseLost);
        }

        let outcome = aggregate_parent_on(&mut tx, result.fence, &transition).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(outcome)
    }

    async fn fail_delivery(&self, failure: DeliveryFailure<'_>) -> AppResult<DeliveryOutcome> {
        let sql = match failure.disposition {
            // A payload that will not decode, or a dependency that can never land, comes out the
            // same way on the fifth attempt as on the first. Going terminal now rather than after
            // five backoffs is what keeps a poison row from occupying a claim slot for an hour.
            Disposition::Terminal => format!(
                r#"UPDATE message_deliveries
                      SET status = '{dead}',
                          attempt_count = LEAST(attempt_count + 1, max_attempts),
                          last_error_class = $4, last_error_detail = $5,
                          execution_id = NULL, owner_worker_id = NULL,
                          locked_at = NULL, lock_expires_at = NULL,
                          updated_at = CURRENT_TIMESTAMP
                    WHERE {fence}
                RETURNING status"#,
                dead = DeliveryStatus::DeadLetter.as_str(),
                fence = own_row_fence_sql(),
            ),
            // Whether this lands in `retryable` or in `dead_letter` depends on the attempt count
            // the statement itself increments, so the status comes back from `RETURNING` rather
            // than being predicted here and read back in a second round trip.
            Disposition::Retry => format!(
                "UPDATE message_deliveries {set} WHERE {fence} RETURNING status",
                set = attempt_failed_set("$4", "$5"),
                fence = own_row_fence_sql(),
            ),
        };

        let settled: Option<String> = sqlx::query_scalar(&sql)
            .bind(failure.fence.row.as_uuid())
            .bind(failure.fence.execution.as_uuid())
            .bind(failure.fence.owner.as_uuid())
            .bind(failure.class.as_str())
            .bind(failure.detail.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;

        let Some(settled) = settled else {
            return Ok(DeliveryOutcome::LeaseLost);
        };
        Ok(DeliveryOutcome::Applied(
            DeliveryStatus::from_str(&settled)
                .map_err(|error| AppError::Internal(error.to_string()))?,
        ))
    }

    async fn release_delivery(
        &self,
        fence: &ExecutionLease<DeliveryId>,
    ) -> AppResult<DeliveryOutcome> {
        // No attempt is charged and no backoff applied: nothing was sent, so the honest state is
        // "claimable again, now". A shutdown that instead let the lease lapse would strand this
        // row for a full lease period *and* charge it for the privilege.
        //
        // Valid only while no provider request has started, which the caller guarantees by
        // reaching this before `begin_part` -- and `begin_part` is what stamps
        // `request_started_at`, so a part in `sending` is by construction a request that went out.
        // The delivery worker's shutdown path awaits such a call and records its real outcome
        // instead of releasing.
        let result = sqlx::query(&format!(
            r#"UPDATE message_deliveries
                  SET status = '{pending}',
                      execution_id = NULL, owner_worker_id = NULL,
                      locked_at = NULL, lock_expires_at = NULL,
                      updated_at = CURRENT_TIMESTAMP
                WHERE {fence}"#,
            pending = DeliveryStatus::Pending.as_str(),
            fence = own_row_fence_sql(),
        ))
        .bind(fence.row.as_uuid())
        .bind(fence.execution.as_uuid())
        .bind(fence.owner.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        if result.rows_affected() != 1 {
            return Ok(DeliveryOutcome::LeaseLost);
        }
        Ok(DeliveryOutcome::Applied(DeliveryStatus::Pending))
    }

    async fn reap_expired_deliveries(&self) -> AppResult<DeliveryReaping> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let leases_expired = reap_expired_leases_on(&mut tx).await?;
        let dependencies_orphaned = orphan_stuck_dependents_on(&mut tx).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(DeliveryReaping {
            leases_expired,
            dependencies_orphaned,
        })
    }
}

/// Re-derive the parent's status from its parts, still fenced on the same execution.
///
/// The rule itself is [`aggregate_parent_status`], in the domain, applied to the statuses this
/// transaction has just written. Deliberately read-then-decide-then-write rather than expressed as
/// a SQL `CASE`: "a parent is delivered only when every part is, one ambiguous part holds it, one
/// dead part poisons it" is a domain decision with unit tests, and a second copy of it in SQL is
/// how the two answer differently for a three-part send.
async fn aggregate_parent_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<DeliveryId>,
    transition: &PartTransition,
) -> AppResult<DeliveryOutcome> {
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM message_delivery_parts WHERE delivery_id = $1 ORDER BY part_index",
    )
    .bind(fence.row.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;

    let statuses = stored
        .iter()
        .map(|status| {
            DeliveryPartStatus::from_str(status)
                .map_err(|error| AppError::Internal(error.to_string()))
        })
        .collect::<AppResult<Vec<_>>>()?;

    // The delivery keeps its lease while this run can still make progress: a part already in
    // flight, or a part that succeeded with others still to send. Settling here instead would
    // release the claim in the middle of a multi-part send, and the next claimant would resume it
    // as if the run had crashed.
    let more_to_send = statuses.iter().any(|status| status.is_unfinished());
    if statuses.contains(&DeliveryPartStatus::Sending)
        || (transition.status == DeliveryPartStatus::Delivered && more_to_send)
    {
        return Ok(DeliveryOutcome::Applied(DeliveryStatus::Sending));
    }

    let target = aggregate_parent_status(&statuses);
    let applied = match target {
        DeliveryStatus::Retryable => retry_parent_on(tx, fence, transition).await?,
        _ => settle_parent_on(tx, fence, target, transition).await?,
    };
    if !applied {
        return Ok(DeliveryOutcome::LeaseLost);
    }
    Ok(DeliveryOutcome::Applied(target))
}

/// The parent has unfinished parts and its lease is done: count the attempt, back off, come back.
///
/// A provider that named its own deadline overrides the computed backoff and is not charged an
/// attempt -- being rate-limited is not a failure of this delivery, and spending its retry budget
/// on a busy hour leaves nothing for a real fault.
async fn retry_parent_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<DeliveryId>,
    transition: &PartTransition,
) -> AppResult<bool> {
    let class = transition.class.map(FailureClass::as_str);
    let detail = transition.detail.as_ref().map(|detail| detail.as_str());

    let sql = match (transition.consumes_attempt, transition.retry_after) {
        (true, _) => format!(
            "UPDATE message_deliveries {set} WHERE {fence}",
            set = attempt_failed_set("$4", "$5"),
            fence = own_row_fence_sql(),
        ),
        (false, retry_after) => {
            // Either the provider named a deadline, or nothing about this attempt was chargeable.
            // Both come back without spending an attempt; the delay is the provider's when it gave
            // one and the ordinary backoff otherwise.
            let delay = match retry_after {
                Some(_) => "make_interval(secs => $6::double precision)".to_string(),
                None => retry_delay_sql("attempt_count"),
            };
            format!(
                r#"UPDATE message_deliveries
                      SET status = '{retryable}',
                          last_error_class = $4, last_error_detail = $5,
                          available_at = CURRENT_TIMESTAMP + {delay},
                          execution_id = NULL, owner_worker_id = NULL,
                          locked_at = NULL, lock_expires_at = NULL,
                          updated_at = CURRENT_TIMESTAMP
                    WHERE {fence}"#,
                retryable = DeliveryStatus::Retryable.as_str(),
                fence = own_row_fence_sql(),
            )
        }
    };

    let mut query = sqlx::query(&sql)
        .bind(fence.row.as_uuid())
        .bind(fence.execution.as_uuid())
        .bind(fence.owner.as_uuid())
        .bind(class)
        .bind(detail);
    if !transition.consumes_attempt
        && let Some(retry_after) = transition.retry_after
    {
        query = query.bind(retry_after.as_secs_f64());
    }
    Ok(query
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?
        .rows_affected()
        == 1)
}

/// The parent has reached a terminal state: delivered, ambiguous, or poisoned.
async fn settle_parent_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<DeliveryId>,
    target: DeliveryStatus,
    transition: &PartTransition,
) -> AppResult<bool> {
    let delivered = DeliveryStatus::Delivered.as_str();
    // An attempt is charged for every terminal outcome except success, so a dead letter's attempt
    // ledger reads as the number of provider calls it actually cost.
    let attempt = if transition.consumes_attempt {
        "LEAST(attempt_count + 1, max_attempts)"
    } else {
        "attempt_count"
    };
    let result = sqlx::query(&format!(
        r#"UPDATE message_deliveries
              SET status = $4,
                  attempt_count = {attempt},
                  last_error_class = $5, last_error_detail = $6,
                  delivered_at = CASE WHEN $4 = '{delivered}'
                                      THEN CURRENT_TIMESTAMP ELSE NULL END,
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
            WHERE {fence}"#,
        fence = own_row_fence_sql(),
    ))
    .bind(fence.row.as_uuid())
    .bind(fence.execution.as_uuid())
    .bind(fence.owner.as_uuid())
    .bind(target.as_str())
    .bind(transition.class.map(FailureClass::as_str))
    .bind(transition.detail.as_ref().map(|detail| detail.as_str()))
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(result.rows_affected() == 1)
}

/// End every claim whose lease ran out, charging each an attempt.
///
/// No worker guard: the lease is expired, so by definition no run still holds it. The parts are
/// settled first, and the distinction there is the one that matters -- a part whose request had
/// already started may be held by the provider, so it becomes `outcome_unknown` and is never
/// re-sent, while one that never reached the provider is plainly retryable.
async fn reap_expired_leases_on(tx: &mut Transaction<'_, Postgres>) -> AppResult<u64> {
    let expired = format!(
        r#"status = '{sending}'
           AND (execution_id IS NULL OR owner_worker_id IS NULL OR locked_at IS NULL
                OR lock_expires_at IS NULL OR lock_expires_at <= locked_at
                OR lock_expires_at <= CURRENT_TIMESTAMP)"#,
        sending = DeliveryStatus::Sending.as_str(),
    );

    let stranded: Vec<Uuid> = sqlx::query_scalar(&format!(
        "SELECT id FROM message_deliveries WHERE {expired}"
    ))
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if stranded.is_empty() {
        return Ok(0);
    }

    sqlx::query(&format!(
        r#"UPDATE message_delivery_parts
              SET status = CASE WHEN request_started_at IS NULL
                                THEN '{retryable_part}' ELSE '{unknown_part}' END,
                  last_error_class = '{lease_expired}',
                  last_error_detail = 'The delivery lease expired without a result',
                  updated_at = CURRENT_TIMESTAMP
            WHERE delivery_id = ANY($1) AND status = '{sending_part}'"#,
        retryable_part = DeliveryPartStatus::Retryable.as_str(),
        unknown_part = DeliveryPartStatus::OutcomeUnknown.as_str(),
        sending_part = DeliveryPartStatus::Sending.as_str(),
        lease_expired = FailureClass::LeaseExpired.as_str(),
    ))
    .bind(&stranded)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;

    // A delivery holding an ambiguous part must not come back as claimable, or the sweep would be
    // the very thing that resends a part the provider already accepted.
    let unknown = sqlx::query(&format!(
        r#"UPDATE message_deliveries
              SET status = '{unknown}',
                  attempt_count = LEAST(attempt_count + 1, max_attempts),
                  last_error_class = '{lease_expired}',
                  last_error_detail = 'The lease expired after the provider request had started',
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
            WHERE id = ANY($1)
              AND EXISTS (
                  SELECT 1 FROM message_delivery_parts AS part
                   WHERE part.delivery_id = message_deliveries.id
                     AND part.status = '{unknown_part}'
              )"#,
        unknown = DeliveryStatus::OutcomeUnknown.as_str(),
        unknown_part = DeliveryPartStatus::OutcomeUnknown.as_str(),
        lease_expired = FailureClass::LeaseExpired.as_str(),
    ))
    .bind(&stranded)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;

    let retried = sqlx::query(&format!(
        "UPDATE message_deliveries {set} WHERE id = ANY($1) AND {expired}",
        set = attempt_failed_set(
            &format!("'{}'", FailureClass::LeaseExpired.as_str()),
            "'The delivery lease expired without a result'",
        ),
    ))
    .bind(&stranded)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;

    Ok(unknown.rows_affected() + retried.rows_affected())
}

/// Dead-letter the descendants of a dependency that can never be delivered.
///
/// Without this a mirror whose root post was poisoned waits for a predecessor that will never
/// arrive, forever: it is excluded from every claim and nothing ever changes its status, so it is
/// invisible to the stuck-work census as well. The reason is typed
/// ([`FailureClass::DependencyFailed`]) rather than a sentence, so an operator can tell a delivery
/// that failed from one that was never given a chance.
async fn orphan_stuck_dependents_on(tx: &mut Transaction<'_, Postgres>) -> AppResult<u64> {
    let claimable = claimable_statuses_sql();
    let terminal = sql_status_list(
        DeliveryStatus::ALL
            .iter()
            .filter(|status| status.needs_attention()),
    );
    let result = sqlx::query(&format!(
        r#"UPDATE message_deliveries AS dependent
              SET status = '{dead}',
                  attempt_count = max_attempts,
                  last_error_class = '{dependency_failed}',
                  last_error_detail = 'The delivery this one threads under can never be delivered',
                  updated_at = CURRENT_TIMESTAMP
            WHERE dependent.status IN ({claimable})
              AND EXISTS (
                  SELECT 1 FROM message_deliveries AS dependency
                   WHERE dependency.id = dependent.depends_on_delivery_id
                     AND dependency.status IN ({terminal})
              )"#,
        dead = DeliveryStatus::DeadLetter.as_str(),
        dependency_failed = FailureClass::DependencyFailed.as_str(),
    ))
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(result.rows_affected())
}

/// The parent-side half of a part transition's `WHERE` clause: this delivery, this execution, this
/// worker, and a lease that has not lapsed.
///
/// Written once because it appears in every part statement, and because dropping any one of the
/// four turns the fence into a suggestion. Reads `$2` as the delivery, `$3` as the execution and
/// `$4` as the worker; `$1` is the part the statement is about.
fn live_fence_sql() -> String {
    format!(
        r#"delivery.id = $2 AND delivery.status = '{sending}'
           AND delivery.execution_id = $3 AND delivery.owner_worker_id = $4
           AND delivery.lock_expires_at > CURRENT_TIMESTAMP"#,
        sending = DeliveryStatus::Sending.as_str(),
    )
}

/// The same fence for a statement whose subject *is* the delivery: binds `$1`/`$2`/`$3`.
///
/// The lease expiry is deliberately not checked here. A run that spent longer in a provider call
/// than its lease allowed still has to be able to record what the provider said, and the reaper
/// cannot have taken the row while this execution id is still on it. Checking expiry would throw
/// away exactly the outcome that matters most.
fn own_row_fence_sql() -> String {
    format!(
        r#"id = $1 AND status = '{sending}' AND execution_id = $2 AND owner_worker_id = $3"#,
        sending = DeliveryStatus::Sending.as_str(),
    )
}

fn sql_part_list(statuses: &[DeliveryPartStatus]) -> String {
    statuses
        .iter()
        .map(|status| format!("'{}'", status.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}
