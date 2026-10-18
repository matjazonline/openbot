//! Leaving `outcome_unknown` on verified evidence: one decision, one place (04.11-D3 §H.4).
//!
//! The caller is the reconciliation settlement transaction, which already holds company, run,
//! action and the parent `FOR UPDATE`. Every transition here is a parent status UPDATE, so the
//! workflow receipt event trigger records it; the provenance guards accept exactly these exits.
//! The parent is written before its part, because the part guard reads the parent's new state.

use sqlx::PgConnection;
use uuid::Uuid;

use super::cancellation::cancellation_detail;
use crate::{
    app_error::{AppError, AppResult},
    entities::transport::{DeliveryCancellationReason, FailureClass},
};

/// What verified evidence settles an outcome-unknown delivery to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnknownSettlement {
    /// Applied: the message was accepted; the provider key stays the predicted Message-ID.
    Delivered { evidence: Uuid },
    /// Final not-applied on a live run with attempts left: only this delivery is queued again.
    Reopened { reopen: Uuid },
    /// Final not-applied on a terminal run or under a cancellation intent: known not sent.
    Superseded {
        evidence: Uuid,
        reason: DeliveryCancellationReason,
    },
    /// Final not-applied with no attempt left: known not sent, nothing resends it.
    Exhausted { evidence: Uuid },
}

pub(crate) async fn settle_unknown_delivery_on(
    db: &mut PgConnection,
    delivery: Uuid,
    part: Uuid,
    settlement: UnknownSettlement,
) -> AppResult<()> {
    let (parent, part_sql) = match settlement {
        UnknownSettlement::Delivered { evidence } => (
            sqlx::query(
                "UPDATE message_deliveries SET status='delivered', delivered_at=CURRENT_TIMESTAMP, \
                   last_error_class=NULL, last_error_detail=NULL, settled_evidence_id=$2, updated_at=CURRENT_TIMESTAMP \
                 WHERE id=$1 AND status='outcome_unknown'",
            )
            .bind(delivery)
            .bind(evidence)
            .execute(&mut *db)
            .await,
            "UPDATE message_delivery_parts SET status='delivered', delivered_at=CURRENT_TIMESTAMP, \
               last_error_class=NULL, last_error_detail=NULL, updated_at=CURRENT_TIMESTAMP \
             WHERE id=$1 AND delivery_id=$2 AND status='outcome_unknown'",
        ),
        UnknownSettlement::Reopened { reopen } => (
            sqlx::query(
                "UPDATE message_deliveries SET status='retryable', available_at=CURRENT_TIMESTAMP, \
                   last_error_class=$3, last_error_detail='Verified not applied; queued again', reopen_id=$2, \
                   updated_at=CURRENT_TIMESTAMP \
                 WHERE id=$1 AND status='outcome_unknown' AND cancellation_requested_at IS NULL AND attempt_count<max_attempts",
            )
            .bind(delivery)
            .bind(reopen)
            .bind(FailureClass::ReconciledNotApplied.as_str())
            .execute(&mut *db)
            .await,
            "UPDATE message_delivery_parts SET status='retryable', \
               last_error_class='reconciled_not_applied', last_error_detail='Verified not applied; queued again', \
               updated_at=CURRENT_TIMESTAMP \
             WHERE id=$1 AND delivery_id=$2 AND status='outcome_unknown'",
        ),
        UnknownSettlement::Superseded { evidence, reason } => (
            sqlx::query(
                "UPDATE message_deliveries SET status='dead_letter', last_error_class=$3, last_error_detail=$4, \
                   settled_evidence_id=$2, cancellation_reason=COALESCE(cancellation_reason,$5), \
                   cancellation_requested_at=COALESCE(cancellation_requested_at,CURRENT_TIMESTAMP), \
                   updated_at=CURRENT_TIMESTAMP \
                 WHERE id=$1 AND status='outcome_unknown'",
            )
            .bind(delivery)
            .bind(evidence)
            .bind(FailureClass::Superseded.as_str())
            .bind(cancellation_detail(reason))
            .bind(reason.as_str())
            .execute(&mut *db)
            .await,
            "UPDATE message_delivery_parts SET status='dead', last_error_class='superseded', \
               last_error_detail='Verified not applied; the workflow no longer wants it', updated_at=CURRENT_TIMESTAMP \
             WHERE id=$1 AND delivery_id=$2 AND status='outcome_unknown'",
        ),
        UnknownSettlement::Exhausted { evidence } => (
            sqlx::query(
                "UPDATE message_deliveries SET status='dead_letter', last_error_class=$3, \
                   last_error_detail='Verified not applied; no attempt left', settled_evidence_id=$2, \
                   updated_at=CURRENT_TIMESTAMP \
                 WHERE id=$1 AND status='outcome_unknown'",
            )
            .bind(delivery)
            .bind(evidence)
            .bind(FailureClass::NotAppliedExhausted.as_str())
            .execute(&mut *db)
            .await,
            "UPDATE message_delivery_parts SET status='dead', last_error_class='not_applied_exhausted', \
               last_error_detail='Verified not applied; no attempt left', updated_at=CURRENT_TIMESTAMP \
             WHERE id=$1 AND delivery_id=$2 AND status='outcome_unknown'",
        ),
    };
    let parent = parent.map_err(AppError::from)?;
    let part = sqlx::query(part_sql)
        .bind(part)
        .bind(delivery)
        .execute(&mut *db)
        .await
        .map_err(AppError::from)?;
    if parent.rows_affected() != 1 || part.rows_affected() != 1 {
        // Rolled back by the caller: the subject left `outcome_unknown` under another owner.
        return Err(AppError::Conflict(
            "Workflow message delivery is no longer outcome unknown".into(),
        ));
    }
    Ok(())
}
