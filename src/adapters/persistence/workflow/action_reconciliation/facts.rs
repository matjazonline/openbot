//! Append-only evidence and recovered receipt share the command transaction.
use super::*;

pub(super) async fn insert_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    snapshot: &ReconciliationSnapshot,
    evidence: &VerifiedEvidence,
    id: ActionEvidenceId,
    command_id: Uuid,
) -> AppResult<()> {
    let registration = evidence.registration();
    let disposition = match evidence.disposition() {
        EffectiveEvidenceDisposition::Applied => "applied",
        EffectiveEvidenceDisposition::FinalNotApplied => "final_not_applied",
        EffectiveEvidenceDisposition::Unknown => "unknown",
    };
    let already_final: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=$1 AND invocation_id=$2 AND coverage_digest=$3 AND grant_eligible)")
        .bind(command.scope.company.as_uuid()).bind(command.subject.invocation.as_uuid())
        .bind(snapshot.coverage_digest.as_str()).fetch_one(&mut **tx).await?;
    let attribution = match evidence.applied_request() {
        Some(AppliedEvidenceRequest::RemoteEntry(_)) => Some("remote_entry"),
        Some(AppliedEvidenceRequest::MarkerReservation) => Some("marker_reservation"),
        Some(AppliedEvidenceRequest::Unattributed) => Some("unattributed"),
        None => None,
    };
    let applied_entry = match evidence.applied_request() {
        Some(AppliedEvidenceRequest::RemoteEntry(entry)) => Some(entry.as_uuid()),
        _ => None,
    };
    sqlx::query("INSERT INTO workflow_action_evidence(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,actor_id,command_id,command_key,request_digest,coverage_digest,disposition,registration,verifier_version,provider,authoritative_reference,operation_signature,observed_at,verified_at,valid_until,grant_eligible,diagnostic,applied_request,applied_remote_entry_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25)")
        .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid())
        .bind(command.subject.invocation.as_uuid()).bind(command.subject.argument_digest.as_str()).bind(command.marker.as_uuid())
        .bind(id.as_uuid()).bind(command.actor.user_id()).bind(command_id).bind(command.command_key.as_str())
        .bind(evidence.request_digest().as_str()).bind(evidence.coverage_digest().as_str()).bind(disposition)
        .bind(registration.map(|r| r.id().as_str())).bind(registration.map(|r| r.version().as_str()))
        .bind(registration.map(|r| r.provider().as_str())).bind(evidence.authoritative_reference().map(|r| r.as_str()))
        .bind(registration.map(|r| r.operation().as_str())).bind(evidence.observed_at()).bind(evidence.verified_at())
        .bind(evidence.valid_until()).bind(disposition == "final_not_applied" && !already_final)
        .bind(evidence.diagnostic()).bind(attribution).bind(applied_entry).execute(&mut **tx).await?;
    for entry in &snapshot.entries {
        sqlx::query("INSERT INTO workflow_action_evidence_coverage(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid())
            .bind(command.subject.invocation.as_uuid()).bind(command.subject.argument_digest.as_str()).bind(command.marker.as_uuid())
            .bind(id.as_uuid()).bind(entry.id.as_uuid()).execute(&mut **tx).await?;
    }
    Ok(())
}

pub(super) async fn truth_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    evidence: &VerifiedEvidence,
    id: ActionEvidenceId,
) -> AppResult<ReconciliationOutcome> {
    let receipt: Option<Value> = sqlx::query_scalar(
        "SELECT result FROM workflow_action_receipts WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(command.scope.company.as_uuid())
    .bind(command.subject.invocation.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    let differs = match evidence.disposition() {
        EffectiveEvidenceDisposition::FinalNotApplied => receipt.is_some(),
        EffectiveEvidenceDisposition::Applied => receipt
            .as_ref()
            .zip(evidence.recovered_result())
            .is_some_and(|(old, new)| old != new),
        EffectiveEvidenceDisposition::Unknown => false,
    };
    if differs {
        disagreement_on(tx, command, id).await?;
    }
    if evidence.disposition() == EffectiveEvidenceDisposition::FinalNotApplied {
        prior_applied_conflicts_on(tx, command, id).await?;
    }
    // Positive truth without a recovered receipt must detect the same old-request
    // contradictions as receipt insertion. A later new request remains legitimate.
    if evidence.disposition() == EffectiveEvidenceDisposition::Applied {
        if receipt.is_some() || evidence.recovered_result().is_none() {
            applied_conflicts_on(tx, command, id).await?;
        }
        if receipt.is_none()
            && let Some(result) = evidence.recovered_result()
        {
            sqlx::query("INSERT INTO workflow_action_receipts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,reconciliation_evidence_id,result) VALUES($1,$2,$3,$4,$5,$6,'remote',$7,$8)")
                .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid())
                .bind(command.subject.invocation.as_uuid()).bind(command.subject.argument_digest.as_str()).bind(command.marker.as_uuid())
                .bind(id.as_uuid()).bind(result).execute(&mut **tx).await?;
        }
    }
    let conflicted: bool =
        sqlx::query_scalar("SELECT workflow_action_has_evidence_conflict($1,$2)")
            .bind(command.scope.company.as_uuid())
            .bind(command.scope.execution.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
    if conflicted {
        return Ok(ReconciliationOutcome::EvidenceConflict);
    }
    Ok(match evidence.disposition() {
        EffectiveEvidenceDisposition::Unknown => ReconciliationOutcome::UnknownRecorded,
        EffectiveEvidenceDisposition::FinalNotApplied => ReconciliationOutcome::NotAppliedRecorded,
        EffectiveEvidenceDisposition::Applied => ReconciliationOutcome::AppliedRecorded {
            receipt: receipt.is_some() || evidence.recovered_result().is_some(),
        },
    })
}

async fn prior_applied_conflicts_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    id: ActionEvidenceId,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,contradictory_evidence_id,reason) SELECT applied.company_id,applied.run_id,applied.execution_id,applied.invocation_id,applied.argument_digest,applied.dispatch_id,$3,applied.id,'evidence_disagreement' FROM workflow_action_evidence AS applied WHERE applied.company_id=$1 AND applied.invocation_id=$2 AND applied.disposition='applied'")
        .bind(command.scope.company.as_uuid()).bind(command.subject.invocation.as_uuid()).bind(id.as_uuid())
        .execute(&mut **tx).await?;
    Ok(())
}

async fn applied_conflicts_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    id: ActionEvidenceId,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,contradictory_evidence_id,reason) SELECT proof.company_id,proof.run_id,proof.execution_id,proof.invocation_id,proof.argument_digest,proof.dispatch_id,proof.id,applied.id,CASE WHEN applied.applied_request='unattributed' THEN 'unattributed_applied' ELSE 'finality_breach' END FROM workflow_action_evidence AS proof JOIN workflow_action_evidence AS applied ON applied.company_id=proof.company_id AND applied.invocation_id=proof.invocation_id WHERE applied.company_id=$1 AND applied.id=$2 AND proof.disposition='final_not_applied' AND (applied.applied_request='unattributed' OR (applied.applied_request='remote_entry' AND EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=proof.company_id AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=applied.applied_remote_entry_id)) OR (applied.applied_request='marker_reservation' AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=proof.company_id AND coverage.evidence_id=proof.id)))")
        .bind(command.scope.company.as_uuid()).bind(id.as_uuid()).execute(&mut **tx).await?;
    Ok(())
}

async fn disagreement_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    id: ActionEvidenceId,
) -> AppResult<()> {
    sqlx::query("INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,contradictory_evidence_id,remote_entry_id,reason) SELECT receipt.company_id,receipt.run_id,receipt.execution_id,receipt.invocation_id,receipt.argument_digest,receipt.dispatch_id,$3,receipt.reconciliation_evidence_id,receipt.remote_entry_id,'evidence_disagreement' FROM workflow_action_receipts AS receipt WHERE receipt.company_id=$1 AND receipt.invocation_id=$2")
        .bind(command.scope.company.as_uuid()).bind(command.subject.invocation.as_uuid()).bind(id.as_uuid()).execute(&mut **tx).await?;
    Ok(())
}
