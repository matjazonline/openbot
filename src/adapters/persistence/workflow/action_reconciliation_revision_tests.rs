//! Audit-only truth advances the owner revision without authorizing another entry.
use super::*;
use crate::application::workflow::{CancelCommand, CancelResult};

#[derive(Clone, Copy)]
enum State {
    Waiting,
    Cancelled,
    Expired,
}

fn expected_state(state: State) -> &'static str {
    match state {
        State::Waiting => "waiting",
        State::Cancelled => "cancelled",
        State::Expired => "failed",
    }
}

fn expected_outcome(state: State, truth: SiblingTruth) -> ReconciliationOutcome {
    match (state, truth) {
        (_, SiblingTruth::Unknown) => ReconciliationOutcome::UnknownRecorded,
        (State::Cancelled | State::Expired, SiblingTruth::Receipt) => {
            ReconciliationOutcome::AppliedRecorded { receipt: true }
        }
        (State::Cancelled | State::Expired, SiblingTruth::Final) => {
            ReconciliationOutcome::NotAppliedRecorded
        }
        _ => ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob,
        },
    }
}

fn assert_truth(before: &Value, after: &Value, command: &Value, truth: SiblingTruth) {
    let evidence = after["workflow_action_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == command["evidence_id"])
        .unwrap();
    assert_eq!(evidence["command_id"], command["id"]);
    assert_eq!(evidence["command_key"], command["command_key"]);
    assert_eq!(
        evidence["disposition"],
        match truth {
            SiblingTruth::Unknown => "unknown",
            SiblingTruth::Receipt => "applied",
            SiblingTruth::Final => "final_not_applied",
            SiblingTruth::AppliedNoResult | SiblingTruth::InvalidResult => {
                panic!("audit matrix requires recovered Applied")
            }
        }
    );
    let receipts = after["workflow_action_receipts"].as_array().unwrap();
    let prior_receipts = before["workflow_action_receipts"].as_array().unwrap();
    if !prior_receipts.is_empty() {
        assert_eq!(
            receipts, prior_receipts,
            "durable actual receipt is preserved"
        );
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["result"], good());
    } else if matches!(truth, SiblingTruth::Receipt) {
        assert_eq!(receipts.len(), 1, "recovered Applied requires its receipt");
        assert_eq!(receipts[0]["reconciliation_evidence_id"], evidence["id"]);
        assert_eq!(receipts[0]["result"], good());
    } else {
        assert!(receipts.is_empty(), "Unknown/Final cannot add a receipt");
    }
}

pub(super) fn truth_delta(
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    truth: SiblingTruth,
) {
    let receipt = after["workflow_action_evidence_commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["command_key"] == command.command_key.as_str())
        .unwrap();
    assert_eq!(receipt["command_key"], command.command_key.as_str());
    assert_eq!(receipt["expected_revision"], command.expected_revision.0);
    assert_eq!(receipt["result_revision"], result.revision.0);
    assert_eq!(
        receipt["result_revision"],
        after["workflow_runs"][0]["revision"]
    );
    assert!(result.revision.0 > command.expected_revision.0);
    assert_eq!(receipt["outcome"], json!(result.outcome));
    assert_eq!(
        receipt["evidence_id"],
        json!(result.evidence.unwrap().as_uuid())
    );
    assert!(receipt["scheduled_job_id"].is_null());
    let audit = after["workflow_run_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|audit| audit["sequence"] == receipt["audit_sequence"])
        .unwrap();
    assert_eq!(audit["event_kind"], "action_reconciled");
    assert_eq!(audit["company_id"], json!(command.scope.company.as_uuid()));
    assert_eq!(audit["run_id"], json!(command.scope.run.as_uuid()));
    assert_eq!(
        audit["execution_id"],
        json!(command.scope.execution.as_uuid())
    );
    assert_eq!(audit["actor_id"], json!(command.actor.user_id()));
    assert_truth(before, after, receipt, truth);
    scoped_truth(after, receipt, command);
    let mut normalized = after.clone();
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_commands",
        "workflow_run_events",
    ] {
        let old = before[table].as_array().unwrap();
        let new = after[table].as_array().unwrap();
        assert_eq!(new.len(), old.len() + 1, "one {table} append");
        assert!(old.iter().all(|row| new.contains(row)), "preserved {table}");
        normalized[table] = before[table].clone();
    }
    normalized["workflow_action_receipts"] = before["workflow_action_receipts"].clone();
    normalized["workflow_runs"][0]["revision"] = before["workflow_runs"][0]["revision"].clone();
    assert_eq!(
        &normalized, before,
        "only linked truth/receipt/audit and generated revision change"
    );
}

fn scoped_truth(after: &Value, receipt: &Value, command: &ReconcileActionCommand) {
    let evidence = after["workflow_action_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == receipt["evidence_id"])
        .unwrap();
    for (column, expected) in [
        ("company_id", json!(command.scope.company.as_uuid())),
        ("run_id", json!(command.scope.run.as_uuid())),
        ("execution_id", json!(command.scope.execution.as_uuid())),
        ("invocation_id", json!(command.subject.invocation.as_uuid())),
        (
            "argument_digest",
            json!(command.subject.argument_digest.as_str()),
        ),
        ("dispatch_id", json!(command.marker.as_uuid())),
        ("actor_id", json!(command.actor.user_id())),
    ] {
        assert_eq!(evidence[column], expected, "scoped evidence {column}");
    }
    assert_eq!(evidence["request_digest"], receipt["request_digest"]);
    let entries: Vec<_> = after["workflow_action_remote_entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["invocation_id"] == evidence["invocation_id"])
        .collect();
    let coverage: Vec<_> = after["workflow_action_evidence_coverage"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["evidence_id"] == evidence["id"])
        .collect();
    assert_eq!(
        coverage.len(),
        entries.len(),
        "covers every exact prior entry"
    );
    for covered in coverage {
        assert!(
            entries
                .iter()
                .any(|entry| entry["id"] == covered["remote_entry_id"])
        );
        for column in [
            "company_id",
            "run_id",
            "execution_id",
            "invocation_id",
            "argument_digest",
            "dispatch_id",
        ] {
            assert_eq!(covered[column], evidence[column], "coverage {column}");
        }
    }
}

pub(super) async fn replay_and_stale(
    f: &AdmissionFixture,
    command: &ReconcileActionCommand,
    verifier: Arc<LedgerVerifier>,
    result: &ReconciliationResult,
) {
    let saved = all_tables(f).await;
    let replay = run_command(f, command, verifier.clone()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.revision, result.revision);
    assert_eq!(replay.evidence, result.evidence);
    assert_eq!(replay.outcome, result.outcome);
    assert_eq!(saved, all_tables(f).await);
    let mut stale = command.clone();
    stale.command_key = IdempotencyKey::parse("revision-distinct-stale").unwrap();
    let refusal = run_command(f, &stale, verifier.clone()).await.unwrap();
    assert_eq!(refusal.outcome, ReconciliationOutcome::RevisionConflict);
    assert_eq!(refusal.revision, result.revision);
    assert!(refusal.evidence.is_none());
    let refused = all_tables(f).await;
    let mut normalized = refused.clone();
    let commands = refused["workflow_action_evidence_commands"]
        .as_array()
        .unwrap();
    assert_eq!(
        commands.len(),
        saved["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .len()
            + 1
    );
    let row = commands
        .iter()
        .find(|row| row["command_key"] == stale.command_key.as_str())
        .unwrap();
    assert_eq!(row["result_revision"], result.revision.0);
    assert_eq!(row["expected_revision"], stale.expected_revision.0);
    assert_eq!(
        row["outcome"],
        json!(ReconciliationOutcome::RevisionConflict)
    );
    assert!(row["scheduled_job_id"].is_null());
    assert!(row["evidence_id"].is_null());
    assert!(row["audit_sequence"].is_null());
    assert!(
        saved["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| commands.contains(row)),
        "every prior command is immutable"
    );
    normalized["workflow_action_evidence_commands"] =
        saved["workflow_action_evidence_commands"].clone();
    assert_eq!(
        normalized, saved,
        "refusal has zero revision/fact/audit bump"
    );
    let replay = run_command(f, &stale, verifier).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.revision, result.revision);
    assert_eq!(replay.outcome, refusal.outcome);
    assert_eq!(replay.evidence, refusal.evidence);
    assert_eq!(refused, all_tables(f).await);
}

async fn audit_case(state: State, truth: SiblingTruth) {
    // One genuine unresolved sibling vetoes continuation without inventing history.
    // Box the paired provider/owner fixture to preserve stock 2 MiB test stacks.
    let (f, requests, verifier) = Box::pin(seed_siblings(&[truth, SiblingTruth::Unknown])).await;
    let request = &requests[0];
    let mut command = proof_command(&f, request, &verifier, "revision-truth").await;
    command.marker = ActionRemoteMarkerId::new(scoped_marker(&f, request).await);
    if matches!(state, State::Cancelled) {
        assert!(matches!(
            f.persistence()
                .cancel(CancelCommand {
                    company_id: command.scope.company,
                    run_id: command.scope.run,
                    actor: command.actor,
                    command_key: IdempotencyKey::parse("revision-terminal-owner").unwrap(),
                    expected_revision: command.expected_revision,
                })
                .await
                .unwrap(),
            CancelResult::Applied { .. }
        ));
    } else if matches!(state, State::Expired) {
        // Move only the clock inputs; expire_run owns the real terminal transition.
        sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE company_id=$1 AND id=$2")
            .bind(command.scope.company.as_uuid()).bind(command.scope.run.as_uuid())
            .execute(f.persistence().pool()).await.unwrap();
        assert!(
            f.persistence()
                .expire_run(request.fence.scope)
                .await
                .unwrap()
        );
        assert!(
            !f.persistence()
                .expire_run(request.fence.scope)
                .await
                .unwrap()
        );
    }
    if !matches!(state, State::Waiting) {
        command.expected_revision = f
            .persistence()
            .head(command.scope.company, command.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision;
    }
    let before = all_tables(&f).await;
    assert_eq!(before["workflow_runs"][0]["state"], expected_state(state));
    let result = run_command(&f, &command, verifier.clone()).await.unwrap();
    assert_eq!(result.outcome, expected_outcome(state, truth));
    assert!(!result.replayed);
    truth_delta(&before, &all_tables(&f).await, &command, &result, truth);
    Box::pin(replay_and_stale(&f, &command, verifier, &result)).await;
    let saved = all_tables(&f).await;
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        saved,
        all_tables(&f).await,
        "audit-only truth creates no attempt or debit"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_waiting_truth_is_audit_only() {
    for truth in [
        SiblingTruth::Unknown,
        SiblingTruth::Receipt,
        SiblingTruth::Final,
    ] {
        Box::pin(audit_case(State::Waiting, truth)).await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_cancelled_truth_is_audit_only() {
    for truth in [
        SiblingTruth::Unknown,
        SiblingTruth::Receipt,
        SiblingTruth::Final,
    ] {
        Box::pin(audit_case(State::Cancelled, truth)).await;
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_expired_truth_is_audit_only() {
    for truth in [
        SiblingTruth::Unknown,
        SiblingTruth::Receipt,
        SiblingTruth::Final,
    ] {
        Box::pin(audit_case(State::Expired, truth)).await;
    }
}
