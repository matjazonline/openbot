//! Message delivery subjects through the 04.7 service (04.11-D3 §H, D4 §N-§R).
//!
//! A subject is one destination delivery of the invocation that is `outcome_unknown`. Locks,
//! after company/principal, run and the frozen-intent restore: action `FOR NO KEY UPDATE`, then
//! the subject parent (`FOR SHARE` to snapshot, `FOR UPDATE` to settle). Settlement never
//! reopens a workflow job, writes a receipt or completes a step: the consumer and the effect
//! wait own progression, so message outcomes never reach the shared continuation.
use super::*;
use crate::adapters::persistence::delivery::evidence::{
    UnknownSettlement, settle_unknown_delivery_on,
};
use crate::entities::transport::DeliveryCancellationReason;

#[derive(Clone, Copy)]
pub(super) enum Lock {
    Snapshot,
    Settle,
}

#[derive(sqlx::FromRow)]
struct SubjectRow {
    part: Uuid,
    parts: i64,
    recipient: Option<String>,
    message_id: String,
    content_digest: String,
    transport: String,
    source_binding: Option<Uuid>,
    attempt_count: i32,
    max_attempts: i32,
    request_started_at: Option<DateTime<Utc>>,
    cancellation: Option<String>,
    status: String,
}

/// The subject delivery with its current status, or `None` when the invocation has no such
/// destination delivery.
pub(super) async fn subject_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    lock: Lock,
) -> AppResult<Option<(DeliverySubject, String)>> {
    let ReconciliationTarget::MessageDelivery { ordinal, delivery } = command.target else {
        return Err(invalid());
    };
    let action: Option<Uuid> = sqlx::query_scalar(
        "SELECT invocation_id FROM workflow_message_actions WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 \
           AND invocation_id=$4 AND argument_digest=$5 AND dispatch_id=$6 FOR NO KEY UPDATE",
    )
    .bind(command.scope.company.as_uuid())
    .bind(command.scope.run.as_uuid())
    .bind(command.scope.execution.as_uuid())
    .bind(command.subject.invocation.as_uuid())
    .bind(command.subject.argument_digest.as_str())
    .bind(command.marker.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    if action.is_none() {
        return Ok(None);
    }
    let lock = match lock {
        Lock::Snapshot => "FOR SHARE OF delivery",
        Lock::Settle => "FOR UPDATE OF delivery",
    };
    let row: Option<SubjectRow> = sqlx::query_as(&format!(
        "SELECT (SELECT part.id FROM message_delivery_parts AS part WHERE part.delivery_id=delivery.id ORDER BY part.part_index LIMIT 1) AS part, \
                (SELECT count(*) FROM message_delivery_parts AS part WHERE part.delivery_id=delivery.id) AS parts, \
                target.recipient_address AS recipient, metadata.rfc_message_id AS message_id, \
                (SELECT part.content_digest FROM message_delivery_parts AS part WHERE part.delivery_id=delivery.id ORDER BY part.part_index LIMIT 1) AS content_digest, \
                delivery.transport, delivery.source_binding_id AS source_binding, delivery.attempt_count, delivery.max_attempts, \
                (SELECT part.request_started_at FROM message_delivery_parts AS part WHERE part.delivery_id=delivery.id ORDER BY part.part_index LIMIT 1) AS request_started_at, \
                delivery.cancellation_reason AS cancellation, delivery.status \
           FROM workflow_message_destinations AS target \
           JOIN message_deliveries AS delivery ON delivery.company_id=target.company_id AND delivery.id=target.delivery_id \
           JOIN email_message_metadata AS metadata ON metadata.company_id=target.company_id AND metadata.message_id=target.message_id \
          WHERE target.company_id=$1 AND target.invocation_id=$2 AND target.ordinal=$3 AND target.delivery_id=$4 {lock}"
    ))
    .bind(command.scope.company.as_uuid())
    .bind(command.subject.invocation.as_uuid())
    .bind(i16::from(ordinal))
    .bind(delivery)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    // Email renders exactly one part; anything else is not a subject this path can attest.
    let (Some(recipient), Some(source_binding), Some(request_started_at), 1) = (
        row.recipient,
        row.source_binding,
        row.request_started_at,
        row.parts,
    ) else {
        return Ok(None);
    };
    Ok(Some((
        DeliverySubject {
            ordinal,
            delivery,
            part: row.part,
            recipient,
            message_id: row.message_id,
            content_digest: row.content_digest,
            transport: row.transport,
            source_binding,
            attempt_count: u32::try_from(row.attempt_count).map_err(|_| invalid())?,
            max_attempts: u32::try_from(row.max_attempts).map_err(|_| invalid())?,
            request_started_at,
            cancellation: row.cancellation,
        },
        row.status,
    )))
}

/// Settlement's re-read under the parent `FOR UPDATE`: the current coverage of a subject that
/// is still `outcome_unknown`, or `None` (stale: refused with no grant).
pub(super) async fn current_coverage_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    action: &FrozenAction,
) -> AppResult<Option<ActionCoverageDigest>> {
    let Some((subject, status)) = subject_on(tx, command, Lock::Settle).await? else {
        return Ok(None);
    };
    if status != "outcome_unknown" {
        return Ok(None);
    }
    ReconciliationSnapshot::delivery_coverage(action, &command.subject, command.marker, &subject)
        .map(Some)
}

/// The effective disposition, applied to the one subject delivery.
pub(super) async fn truth_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    snapshot: &ReconciliationSnapshot,
    evidence: &VerifiedEvidence,
    id: ActionEvidenceId,
) -> AppResult<ReconciliationOutcome> {
    let subject = snapshot.delivery.as_ref().ok_or_else(invalid)?;
    match evidence.disposition() {
        EffectiveEvidenceDisposition::Unknown => Ok(ReconciliationOutcome::UnknownRecorded),
        EffectiveEvidenceDisposition::Applied => {
            let Some(AppliedEvidenceRequest::Delivery { accepted_at }) = evidence.applied_request()
            else {
                return Err(invalid());
            };
            // Applied never downgrades and is recorded on live and terminal runs alike.
            settle_unknown_delivery_on(
                tx,
                subject.delivery,
                subject.part,
                UnknownSettlement::Delivered {
                    evidence: id.as_uuid(),
                },
            )
            .await?;
            // Finality breach (D3 §G row 20, D4 §Q): a consumed not-applied proof for this
            // delivery observed after the attested acceptance was wrong. Existing orientation:
            // the breached proof is `evidence_id`, the new applied evidence contradicts it.
            sqlx::query(
                "INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,contradictory_evidence_id,reason) \
                 SELECT proof.company_id,proof.run_id,proof.execution_id,proof.invocation_id,proof.argument_digest,proof.dispatch_id,proof.id,$3,'finality_breach' \
                   FROM workflow_message_delivery_reopens AS reopen \
                   JOIN workflow_action_evidence AS proof ON proof.company_id=reopen.company_id AND proof.id=reopen.evidence_id \
                  WHERE reopen.company_id=$1 AND reopen.delivery_id=$2 AND proof.observed_at>$4 ORDER BY proof.id",
            )
            .bind(command.scope.company.as_uuid())
            .bind(subject.delivery)
            .bind(id.as_uuid())
            .bind(accepted_at)
            .execute(&mut **tx)
            .await?;
            Ok(if conflicted_on(tx, command).await? {
                ReconciliationOutcome::EvidenceConflict
            } else {
                ReconciliationOutcome::AppliedRecorded { receipt: false }
            })
        }
        EffectiveEvidenceDisposition::FinalNotApplied => {
            // The proof stays recorded either way; a conflicted execution grants nothing.
            if conflicted_on(tx, command).await? {
                return Ok(ReconciliationOutcome::Blocked {
                    reason: ReconciliationBlockedReason::EvidenceConflict,
                });
            }
            let (state, live): (String, bool) = sqlx::query_as(
                "SELECT state, state IN ('queued','running','waiting') AND deadline>clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2",
            )
            .bind(command.scope.company.as_uuid())
            .bind(command.scope.run.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
            let settlement = if live
                && subject.cancellation.is_none()
                && subject.attempt_count < subject.max_attempts
            {
                let reopen = Uuid::new_v4();
                sqlx::query(
                    "INSERT INTO workflow_message_delivery_reopens(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,evidence_id,ordinal,delivery_id,attempt_count,valid_until) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
                )
                .bind(command.scope.company.as_uuid())
                .bind(command.scope.run.as_uuid())
                .bind(command.scope.execution.as_uuid())
                .bind(command.subject.invocation.as_uuid())
                .bind(command.subject.argument_digest.as_str())
                .bind(command.marker.as_uuid())
                .bind(reopen)
                .bind(id.as_uuid())
                .bind(i16::from(subject.ordinal))
                .bind(subject.delivery)
                .bind(i32::try_from(subject.attempt_count).map_err(|_| invalid())?)
                .bind(evidence.valid_until())
                .execute(&mut **tx)
                .await?;
                UnknownSettlement::Reopened { reopen }
            } else if !live || subject.cancellation.is_some() {
                let reason = match subject.cancellation.as_deref() {
                    Some(reason) => reason.parse().map_err(|_| invalid())?,
                    None if state == "cancelled" => DeliveryCancellationReason::WorkflowCancelled,
                    None => DeliveryCancellationReason::WorkflowExpired,
                };
                UnknownSettlement::Superseded {
                    evidence: id.as_uuid(),
                    reason,
                }
            } else {
                UnknownSettlement::Exhausted {
                    evidence: id.as_uuid(),
                }
            };
            settle_unknown_delivery_on(tx, subject.delivery, subject.part, settlement).await?;
            Ok(match settlement {
                UnknownSettlement::Reopened { .. } => ReconciliationOutcome::DeliveryReopened,
                _ => ReconciliationOutcome::NotAppliedRecorded,
            })
        }
    }
}

async fn conflicted_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
) -> AppResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT workflow_action_has_evidence_conflict($1,$2)")
            .bind(command.scope.company.as_uuid())
            .bind(command.scope.execution.as_uuid())
            .fetch_one(&mut **tx)
            .await?,
    )
}
