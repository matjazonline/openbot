//! Cancelling deliveries: recording the intent, and settling a delivery that carries it.
//!
//! A cancellation is two facts that can land at different times. The *intent* is written by
//! whoever cancels -- a stopped task, a removed channel assignment -- under the parent row's lock,
//! in the transaction that makes the cancellation true. A delivery nobody holds is settled there
//! and then. A delivery a worker holds keeps its lease: that worker's next fenced write finds the
//! intent, settles the row with what its parts actually prove, and releases the lease.
//!
//! Nothing here reaches past a live execution, so a provider result already on its way is recorded
//! rather than overwritten; and nothing clears the intent afterwards, so no retry, release or lease
//! sweep can hand a cancelled delivery back to the queue. `message_deliveries_cancellation_check`
//! is the backstop for both: a cancelled row can never be claimable.

use std::str::FromStr;

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{claimable_statuses_sql, queue::own_row_fence_sql, sql_status_list};
use crate::{
    app_error::{AppError, AppResult},
    entities::transport::{
        DeliveryCancellationReason, DeliveryId, DeliveryPartStatus, DeliveryStatus, FailureClass,
        cancelled_parent_status,
    },
    transport::{DeliveryOutcome, ExecutionLease, PartTransition},
};

/// What recording a cancellation did to one set of deliveries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CancelledDeliveries {
    /// Deliveries nobody held, settled as superseded: known never to have been sent.
    pub superseded: u64,
    /// Deliveries in flight or already ambiguous. The intent is recorded and no further part will
    /// start, but a provider may already hold what was sent.
    pub potentially_sent: Vec<DeliveryId>,
}

/// The detail a superseded row carries, per cause. Plain ASCII with no quote, because
/// [`cancellation_detail_sql`] splices it into a statement.
pub(crate) const fn cancellation_detail(reason: DeliveryCancellationReason) -> &'static str {
    match reason {
        DeliveryCancellationReason::ChannelAgentRemoved => {
            "The agent that produced this delivery was removed from its channel"
        }
        DeliveryCancellationReason::TaskStopped => {
            "The task that produced this delivery was stopped"
        }
    }
}

/// [`cancellation_detail`] as a SQL `CASE` over a row's stored reason, for set-based statements
/// that settle rows cancelled for different causes at once.
fn cancellation_detail_sql(reason_column: &str) -> String {
    let arms = DeliveryCancellationReason::ALL
        .iter()
        .map(|reason| {
            format!(
                "WHEN '{}' THEN '{}'",
                reason.as_str(),
                cancellation_detail(*reason)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("CASE {reason_column} {arms} END")
}

/// Record a cancellation on every live delivery these tasks produced, whatever channel it goes out
/// through.
///
/// Locks the parents first, in id order -- the same parent-before-part order every fenced worker
/// write takes -- so a worker racing this either commits its `begin_part` before the lock is taken
/// (and the row is reported as potentially sent) or finds the intent once it is released.
pub(crate) async fn cancel_task_deliveries_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    task_ids: &[Uuid],
    reason: DeliveryCancellationReason,
) -> AppResult<CancelledDeliveries> {
    if task_ids.is_empty() {
        return Ok(CancelledDeliveries::default());
    }
    let claimable = claimable_statuses_sql();
    let in_flight =
        sql_status_list([DeliveryStatus::Sending, DeliveryStatus::OutcomeUnknown].iter());
    let locked: Vec<Uuid> = sqlx::query_scalar(&format!(
        r#"SELECT id FROM message_deliveries
            WHERE company_id = $1 AND task_id = ANY($2)
              AND status IN ({claimable}, {in_flight})
              AND cancellation_requested_at IS NULL
            ORDER BY id
              FOR UPDATE"#
    ))
    .bind(company_id)
    .bind(task_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if locked.is_empty() {
        return Ok(CancelledDeliveries::default());
    }

    // Nobody holds these, so they are settled now. The attempt count stays what it is: it is the
    // number of provider calls this delivery actually cost, and a cancellation cost none.
    let superseded: Vec<Uuid> = sqlx::query_scalar(&format!(
        r#"UPDATE message_deliveries
              SET status = '{dead}',
                  last_error_class = '{superseded}', last_error_detail = $3,
                  cancellation_requested_at = CURRENT_TIMESTAMP, cancellation_reason = $2,
                  updated_at = CURRENT_TIMESTAMP
            WHERE id = ANY($1) AND status IN ({claimable})
        RETURNING id"#,
        dead = DeliveryStatus::DeadLetter.as_str(),
        superseded = FailureClass::Superseded.as_str(),
    ))
    .bind(&locked)
    .bind(reason.as_str())
    .bind(cancellation_detail(reason))
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    bury_unsent_parts_on(tx, &superseded).await?;

    // A worker holds these, or a provider may. Only the intent is written: the lease, the
    // execution id and every provider outcome stay exactly as the worker left them.
    let potentially_sent: Vec<Uuid> = sqlx::query_scalar(&format!(
        r#"UPDATE message_deliveries
              SET cancellation_requested_at = CURRENT_TIMESTAMP, cancellation_reason = $2,
                  updated_at = CURRENT_TIMESTAMP
            WHERE id = ANY($1) AND status IN ({in_flight})
        RETURNING id"#
    ))
    .bind(&locked)
    .bind(reason.as_str())
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;

    Ok(CancelledDeliveries {
        superseded: superseded.len() as u64,
        potentially_sent: potentially_sent.into_iter().map(DeliveryId::new).collect(),
    })
}

/// Mark dead every part of these cancelled deliveries that never reached a provider.
///
/// Only `prepared` and `retryable` parts: a delivered part is the record of what was sent, and a
/// part whose request started is exactly the uncertainty a cancellation must not erase. Acts only
/// on parents that already carry the intent, which is where the detail comes from.
pub(crate) async fn bury_unsent_parts_on(
    tx: &mut Transaction<'_, Postgres>,
    delivery_ids: &[Uuid],
) -> AppResult<()> {
    if delivery_ids.is_empty() {
        return Ok(());
    }
    let unsent = [DeliveryPartStatus::Prepared, DeliveryPartStatus::Retryable]
        .iter()
        .map(|status| format!("'{}'", status.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    sqlx::query(&format!(
        r#"UPDATE message_delivery_parts AS part
              SET status = '{dead}',
                  last_error_class = '{superseded}',
                  last_error_detail = {detail},
                  updated_at = CURRENT_TIMESTAMP
             FROM message_deliveries AS delivery
            WHERE part.delivery_id = delivery.id
              AND delivery.id = ANY($1)
              AND delivery.cancellation_requested_at IS NOT NULL
              AND part.status IN ({unsent})"#,
        dead = DeliveryPartStatus::Dead.as_str(),
        superseded = FailureClass::Superseded.as_str(),
        detail = cancellation_detail_sql("delivery.cancellation_reason"),
    ))
    .bind(delivery_ids)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Whether the execution making a fenced write still holds its delivery, and whether somebody has
/// cancelled it in the meantime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeldDelivery {
    /// This execution no longer holds the row.
    Lost,
    /// Held, and nobody has asked for it not to go out.
    Live,
    /// Held, and cancelled: settle it instead of making the requested transition.
    Cancelled(DeliveryCancellationReason),
}

/// Lock the parent this execution holds and read its cancellation intent.
///
/// Every fenced worker write starts here, which is what serializes it against
/// [`cancel_task_deliveries_on`]: whichever takes the parent's row lock first decides the order,
/// and the other sees its result.
pub(crate) async fn lock_held_delivery_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<DeliveryId>,
) -> AppResult<HeldDelivery> {
    let row: Option<(Option<String>,)> = sqlx::query_as(&format!(
        "SELECT cancellation_reason FROM message_deliveries WHERE {fence} FOR UPDATE",
        fence = own_row_fence_sql(),
    ))
    .bind(fence.row.as_uuid())
    .bind(fence.execution.as_uuid())
    .bind(fence.owner.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    match row {
        None => Ok(HeldDelivery::Lost),
        Some((None,)) => Ok(HeldDelivery::Live),
        Some((Some(reason),)) => DeliveryCancellationReason::from_str(&reason)
            .map(HeldDelivery::Cancelled)
            .map_err(|error| {
                AppError::Internal(format!(
                    "message_deliveries.cancellation_reason holds a value this build cannot read: {error}"
                ))
            }),
    }
}

/// Settle a cancelled delivery this execution holds, and release its lease.
///
/// `last_part` is the provider result this same transaction has just recorded, when there is one;
/// an ambiguous result keeps its own classification on the parent rather than being filed as a
/// cancellation.
pub(crate) async fn settle_cancelled_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<DeliveryId>,
    reason: DeliveryCancellationReason,
    last_part: Option<&PartTransition>,
) -> AppResult<DeliveryOutcome> {
    bury_unsent_parts_on(tx, &[fence.row.as_uuid()]).await?;
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM message_delivery_parts WHERE delivery_id = $1 ORDER BY part_index",
    )
    .bind(fence.row.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let parts = stored
        .iter()
        .map(|status| {
            DeliveryPartStatus::from_str(status)
                .map_err(|error| AppError::Internal(error.to_string()))
        })
        .collect::<AppResult<Vec<_>>>()?;

    let target = cancelled_parent_status(&parts);
    let (class, detail) = match target {
        DeliveryStatus::DeadLetter => (
            Some(FailureClass::Superseded),
            Some(cancellation_detail(reason).to_string()),
        ),
        DeliveryStatus::OutcomeUnknown => (
            last_part.and_then(|transition| transition.class),
            last_part
                .and_then(|transition| transition.detail.as_ref())
                .map(|detail| detail.as_str().to_string()),
        ),
        _ => (None, None),
    };
    // A `NULL` class on an ambiguous settle keeps whatever classification the row already had.
    let result = sqlx::query(&format!(
        r#"UPDATE message_deliveries
              SET status = $4,
                  last_error_class = CASE WHEN $4 = '{delivered}' THEN NULL
                                          ELSE COALESCE($5, last_error_class) END,
                  last_error_detail = CASE WHEN $4 = '{delivered}' THEN NULL
                                           WHEN $5 IS NULL THEN last_error_detail
                                           ELSE $6 END,
                  delivered_at = CASE WHEN $4 = '{delivered}'
                                      THEN CURRENT_TIMESTAMP ELSE NULL END,
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
            WHERE {fence}"#,
        delivered = DeliveryStatus::Delivered.as_str(),
        fence = own_row_fence_sql(),
    ))
    .bind(fence.row.as_uuid())
    .bind(fence.execution.as_uuid())
    .bind(fence.owner.as_uuid())
    .bind(target.as_str())
    .bind(class.map(FailureClass::as_str))
    .bind(detail)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if result.rows_affected() != 1 {
        return Ok(DeliveryOutcome::LeaseLost);
    }
    Ok(DeliveryOutcome::Cancelled(target))
}

/// Settle every stranded delivery that carries a cancellation and holds no ambiguous part.
///
/// Called by the lease sweep after it has classified the stranded parts, and before it charges the
/// rest a retry: a cancelled delivery a worker abandoned is finished here, never handed back.
pub(crate) async fn settle_stranded_cancellations_on(
    tx: &mut Transaction<'_, Postgres>,
    stranded: &[Uuid],
    expired_predicate: &str,
) -> AppResult<u64> {
    let cancelled: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM message_deliveries
          WHERE id = ANY($1) AND cancellation_requested_at IS NOT NULL",
    )
    .bind(stranded)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if cancelled.is_empty() {
        return Ok(0);
    }
    bury_unsent_parts_on(tx, &cancelled).await?;
    let result = sqlx::query(&format!(
        r#"UPDATE message_deliveries
              SET status = '{dead}',
                  attempt_count = LEAST(attempt_count + 1, max_attempts),
                  last_error_class = '{superseded}',
                  last_error_detail = {detail},
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
            WHERE id = ANY($1) AND {expired_predicate}"#,
        dead = DeliveryStatus::DeadLetter.as_str(),
        superseded = FailureClass::Superseded.as_str(),
        detail = cancellation_detail_sql("cancellation_reason"),
    ))
    .bind(&cancelled)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_has_a_detail_the_sql_case_can_carry() {
        for reason in DeliveryCancellationReason::ALL {
            let detail = cancellation_detail(*reason);
            assert!(!detail.contains('\''), "{detail}");
            assert!(detail.len() <= 512);
        }
        let case = cancellation_detail_sql("delivery.cancellation_reason");
        for reason in DeliveryCancellationReason::ALL {
            assert!(case.contains(reason.as_str()));
        }
    }
}
