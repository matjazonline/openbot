//! Verification is external; only exact, current authority is settled here.
use super::*;
use serde_json::json;

pub(super) async fn settle<A: SqlReconciliationResources>(
    adapter: &PostgresActionReconciliation<A>,
    command: &ReconcileActionCommand,
    snapshot: &ReconciliationSnapshot,
    evidence: VerifiedEvidence,
) -> AppResult<ReconciliationResult> {
    snapshot.validate(command)?;
    let digest = command.request_digest()?;
    validate_token(command, snapshot, &evidence, &digest)?;
    let mut tx = adapter.persistence.pool().begin().await?;
    sqlx::query(
        "SELECT set_config('lock_timeout','5s',true),set_config('statement_timeout','5s',true)",
    )
    .execute(&mut *tx)
    .await?;
    authority::authorize_company(&mut tx, command.scope.company, command.actor).await?;
    let head = head_on(&mut tx, command).await?;
    association::authorize_association(
        &mut tx,
        command.scope.company,
        command.actor,
        head.association,
    )
    .await?;
    let frozen = restore_on(&mut tx, command).await?;
    let resource = adapter
        .resources
        .lock_resource(&mut tx, command.actor, &frozen)
        .await?;
    validate_resource(frozen.action.request(), &resource)?;
    let marker_time = marker_on(&mut tx, command).await?;
    if let Some(result) = replay_on(&mut tx, command, &digest, head.revision).await? {
        tx.commit().await?;
        return Ok(result);
    }
    let refusal = refusal_on(
        &mut tx,
        command,
        snapshot,
        &evidence,
        &head,
        &frozen.action,
        marker_time,
    )
    .await?;
    if let Some(reason) = refusal {
        let outcome = ReconciliationOutcome::Blocked { reason };
        record_refusal_on(&mut tx, command, &digest, head.revision, &outcome).await?;
        tx.commit().await?;
        return Ok(refusal_result(outcome, head.revision));
    }
    let id = ActionEvidenceId::new(Uuid::new_v4());
    let command_id = Uuid::new_v4();
    facts::insert_on(&mut tx, command, snapshot, &evidence, id, command_id).await?;
    let outcome = facts::truth_on(&mut tx, command, &evidence, id).await?;
    let (outcome, scheduled_job) = continue_on(&mut tx, command, &head, outcome).await?;
    let audit: i64 = sqlx::query_scalar("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT $1,$2,COALESCE(MAX(sequence),0)+1,'action_reconciled',$3,$4 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 RETURNING sequence")
        .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).bind(command.actor.user_id())
        .bind(command.scope.execution.as_uuid()).fetch_one(&mut *tx).await?;
    let final_revision = head_on(&mut tx, command).await?.revision;
    sqlx::query("INSERT INTO workflow_action_evidence_commands(company_id,command_key,id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,actor_id,request_digest,expected_revision,result_revision,outcome,evidence_id,audit_sequence,scheduled_job_id,previous_state,previous_waiting_reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).bind(command_id)
        .bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid()).bind(command.subject.invocation.as_uuid())
        .bind(command.subject.argument_digest.as_str()).bind(command.marker.as_uuid()).bind(command.actor.user_id())
        .bind(digest.as_str()).bind(revision(command.expected_revision.0)?).bind(revision(final_revision.0)?)
        .bind(json!(outcome)).bind(id.as_uuid()).bind(audit).bind(scheduled_job)
        .bind(state_name(head.state)).bind(waiting_reason(head.state)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ReconciliationResult {
        outcome,
        revision: final_revision,
        evidence: Some(id),
        replayed: false,
    })
}

fn validate_token(
    command: &ReconcileActionCommand,
    snapshot: &ReconciliationSnapshot,
    evidence: &VerifiedEvidence,
    digest: &ReconciliationRequestDigest,
) -> AppResult<()> {
    if evidence.request_digest() != digest
        || evidence.coverage_digest() != &snapshot.coverage_digest
        || evidence.observed_at() < snapshot.marker_created_at
        || evidence.verified_at() < snapshot.database_now
        || evidence.verified_at() > snapshot.database_now + chrono::Duration::seconds(5)
        || evidence.observed_at() > evidence.verified_at()
        || snapshot
            .entries
            .iter()
            .any(|entry| entry.created_at > evidence.observed_at())
    {
        return Err(invalid());
    }
    match (&command.input, evidence.registration()) {
        (EvidenceInput::UnknownNote { .. }, None)
            if evidence.disposition() == EffectiveEvidenceDisposition::Unknown => {}
        (EvidenceInput::VerifiedReference { registration, .. }, Some(installed))
            if registration == installed.id()
                && installed.matches_snapshot(snapshot)?
                && evidence.authoritative_reference().is_some()
                && evidence.valid_until() > evidence.verified_at()
                && evidence.valid_until()
                    <= evidence.verified_at() + chrono::Duration::hours(24) => {}
        _ => return Err(invalid()),
    }
    Ok(())
}

async fn refusal_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    snapshot: &ReconciliationSnapshot,
    evidence: &VerifiedEvidence,
    head: &RunHead,
    action: &FrozenAction,
    marker_time: DateTime<Utc>,
) -> AppResult<Option<ReconciliationBlockedReason>> {
    if overflow_on(tx, command).await? {
        return Ok(Some(ReconciliationBlockedReason::BoundExceeded));
    }
    let entries = entries_on(tx, command).await?;
    let coverage = ReconciliationSnapshot::coverage(
        action,
        &command.subject,
        command.marker,
        marker_time,
        &entries,
    )?;
    if head.revision != snapshot.revision
        || head.revision != command.expected_revision
        || coverage != snapshot.coverage_digest
        || action.saved_operation() != snapshot.action.saved_operation()
    {
        return Ok(Some(ReconciliationBlockedReason::StaleSnapshot));
    }
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await?;
    if evidence.verified_at() > now {
        return Err(invalid());
    }
    Ok(
        (evidence.registration().is_some() && evidence.valid_until() <= now)
            .then_some(ReconciliationBlockedReason::ExpiredProof),
    )
}

async fn continue_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReconcileActionCommand,
    head: &RunHead,
    outcome: ReconciliationOutcome,
) -> AppResult<(ReconciliationOutcome, Option<Uuid>)> {
    if !matches!(
        outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: true }
            | ReconciliationOutcome::NotAppliedRecorded
    ) {
        return Ok((outcome, None));
    }
    if head.state != RunState::Waiting(WaitingReason::Reconciliation) {
        return Ok((outcome, None));
    }
    let job: Option<Uuid> = sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2 AND queue_kind='workflow' AND status='failed'")
        .bind(command.scope.company.as_uuid()).bind(command.scope.execution.as_uuid()).fetch_optional(&mut **tx).await?;
    let Some(job) = job else {
        return Ok((
            ReconciliationOutcome::Blocked {
                reason: ReconciliationBlockedReason::IneligibleJob,
            },
            None,
        ));
    };
    let eligible: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
            .bind(command.scope.company.as_uuid())
            .bind(command.scope.run.as_uuid())
            .bind(job)
            .fetch_one(&mut **tx)
            .await?;
    if !eligible {
        return Ok((
            ReconciliationOutcome::Blocked {
                reason: ReconciliationBlockedReason::IneligibleJob,
            },
            None,
        ));
    }
    let receipt_only: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM workflow_action_dispatches AS marker WHERE marker.company_id=$1 AND marker.execution_id=$2 AND NOT EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt WHERE receipt.company_id=marker.company_id AND receipt.invocation_id=marker.invocation_id))")
        .bind(command.scope.company.as_uuid()).bind(command.scope.execution.as_uuid()).fetch_one(&mut **tx).await?;
    sqlx::query("UPDATE background_tasks SET status='pending',run_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(command.scope.company.as_uuid()).bind(job).execute(&mut **tx).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid()).execute(&mut **tx).await?;
    Ok((ReconciliationOutcome::Scheduled { receipt_only }, Some(job)))
}

fn state_name(state: RunState) -> &'static str {
    match state {
        RunState::Queued => "queued",
        RunState::Running => "running",
        RunState::Waiting(_) => "waiting",
        RunState::Succeeded => "succeeded",
        RunState::Failed => "failed",
        RunState::Cancelled => "cancelled",
    }
}

fn waiting_reason(state: RunState) -> Option<&'static str> {
    match state {
        RunState::Waiting(reason) => Some(match reason {
            WaitingReason::Decision => "decision",
            WaitingReason::Event => "event",
            WaitingReason::Timer => "timer",
            WaitingReason::ChildRun => "child_run",
            WaitingReason::Effect => "effect",
            WaitingReason::Reconciliation => "reconciliation",
        }),
        _ => None,
    }
}
