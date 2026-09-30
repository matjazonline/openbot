//! Real terminal owners compete with settlement while the first owns the run lock.
use super::*;
use crate::application::workflow::{CancelCommand, CancelResult};
#[path = "action_reconciliation_control_serialization_lock_support.rs"]
pub(super) mod contention;
use contention::race;

#[derive(Clone, Copy, Debug)]
enum Truth {
    Applied,
    Final,
    Unknown,
}
#[derive(Clone, Copy, Debug)]
enum Terminal {
    Cancel,
    Expire,
}
#[derive(Clone, Copy, Debug)]
enum First {
    Command,
    Terminal,
}
#[derive(Clone, Copy)]
struct Variant {
    truth: Truth,
    owner: Terminal,
    first: First,
}
#[derive(Debug)]
enum TerminalResult {
    Cancel(CancelResult),
    Expire(bool),
}
struct History {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    ledger: Arc<LedgerVerifier>,
}

async fn terminal(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    command: &ReconcileActionCommand,
    owner: Terminal,
) -> TerminalResult {
    match owner {
        Terminal::Cancel => TerminalResult::Cancel(
            f.persistence()
                .cancel(CancelCommand {
                    company_id: command.scope.company,
                    run_id: command.scope.run,
                    actor: command.actor,
                    command_key: IdempotencyKey::parse("competing-cancel").unwrap(),
                    expected_revision: command.expected_revision,
                })
                .await
                .unwrap(),
        ),
        Terminal::Expire => TerminalResult::Expire(
            f.persistence()
                .expire_run(request.fence.scope)
                .await
                .unwrap(),
        ),
    }
}

async fn history(truth: Truth) -> History {
    let (f, request) = Box::pin(setup()).await;
    resources(&f).await;
    let verifier = ledger(&f, &request).await;
    let delivery = match truth {
        Truth::Applied => Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
        Truth::Final | Truth::Unknown => Delivery::Pending,
    };
    let provider = LedgerProvider::new(&f, delivery);
    assert!(Box::pin(invoke(&f, &request, &provider)).await.is_err());
    park(&f, &request).await;
    if matches!(truth, Truth::Final) {
        barrier(&f, &request).await;
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    History {
        fixture: f,
        request,
        ledger: verifier,
    }
}

fn preserved_history(before: &Value, after: &Value) {
    for table in [
        "task_attempts",
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
        "workflow_action_reconciliations",
        "workflow_action_evidence_consumptions",
        "workflow_executions",
        "fixture_evidence_ledger",
        "fixture_provider_operations",
        "fixture_provider_effects",
    ] {
        assert_eq!(
            before[table], after[table],
            "immutable/external history {table}"
        );
    }
    assert_eq!(after["background_tasks"].as_array().unwrap().len(), 1);
    assert_eq!(
        after["background_tasks"][0]["id"],
        before["background_tasks"][0]["id"]
    );
    assert_eq!(
        after["background_tasks"][0]["retry_count"],
        before["background_tasks"][0]["retry_count"]
    );
}

fn truth_outcome(truth: Truth) -> ReconciliationOutcome {
    match truth {
        Truth::Applied => ReconciliationOutcome::AppliedRecorded { receipt: true },
        Truth::Final => ReconciliationOutcome::NotAppliedRecorded,
        Truth::Unknown => ReconciliationOutcome::UnknownRecorded,
    }
}

async fn assert_result(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    terminal_result: TerminalResult,
    variant: Variant,
) {
    linked_result(&all_tables(f).await, command, result);
    let Variant {
        truth,
        owner,
        first,
    } = variant;
    match first {
        First::Command => {
            let expected = match (truth, owner) {
                (Truth::Unknown, _) => ReconciliationOutcome::UnknownRecorded,
                (_, Terminal::Expire) => ReconciliationOutcome::Blocked {
                    reason: ReconciliationBlockedReason::IneligibleJob,
                },
                (_, Terminal::Cancel) => ReconciliationOutcome::Scheduled {
                    receipt_only: matches!(truth, Truth::Applied),
                },
            };
            assert_eq!(result.outcome, expected);
            assert!(result.evidence.is_some());
            match terminal_result {
                TerminalResult::Cancel(CancelResult::RevisionConflict { current_revision }) => {
                    assert_eq!(current_revision, result.revision);
                    assert!(matches!(
                        f.persistence()
                            .cancel(CancelCommand {
                                company_id: command.scope.company,
                                run_id: command.scope.run,
                                actor: command.actor,
                                command_key: IdempotencyKey::parse("fresh-cancel").unwrap(),
                                expected_revision: current_revision,
                            })
                            .await
                            .unwrap(),
                        CancelResult::Applied { .. }
                    ));
                }
                TerminalResult::Expire(true) => {}
                other => panic!("unexpected command-first terminal result: {other:?}"),
            }
        }
        First::Terminal => {
            assert!(matches!(
                terminal_result,
                TerminalResult::Cancel(CancelResult::Applied { .. }) | TerminalResult::Expire(true)
            ));
            assert_eq!(
                result.outcome,
                ReconciliationOutcome::Blocked {
                    reason: ReconciliationBlockedReason::StaleSnapshot
                }
            );
            assert!(result.evidence.is_none());
        }
    }
    let before = all_tables(f).await;
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(all_tables(f).await, before, "terminal claim is inert");
}

async fn fresh_terminal_truth(
    f: &AdmissionFixture,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    truth: Truth,
    ledger: Arc<LedgerVerifier>,
) {
    let before = all_tables(f).await;
    assert!(
        before["workflow_action_evidence"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let verifier = Arc::new(ObservedVerifier::new(ledger));
    let mut fresh = command.clone();
    fresh.command_key = IdempotencyKey::parse("fresh-terminal-truth").unwrap();
    fresh.expected_revision = result.revision;
    let attached = Box::pin(execute(f, &fresh, verifier)).await.unwrap();
    assert_eq!(attached.outcome, truth_outcome(truth));
    assert!(attached.evidence.is_some());
    let after = all_tables(f).await;
    linked_result(&after, &fresh, &attached);
    assert_eq!(after["workflow_runs"][0]["revision"], attached.revision.0);
    for table in [
        "background_tasks",
        "workflow_executions",
        "task_attempts",
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
    ] {
        assert_eq!(
            before[table], after[table],
            "terminal truth is audit-only: {table}"
        );
    }
    let mut run = after["workflow_runs"].clone();
    run[0]["revision"] = before["workflow_runs"][0]["revision"].clone();
    assert_eq!(run, before["workflow_runs"]);
}

fn linked_result(after: &Value, command: &ReconcileActionCommand, result: &ReconciliationResult) {
    let commands = after["workflow_action_evidence_commands"]
        .as_array()
        .unwrap();
    let stored = commands
        .iter()
        .find(|row| row["command_key"] == command.command_key.as_str())
        .unwrap();
    assert_eq!(stored["expected_revision"], command.expected_revision.0);
    assert_eq!(stored["result_revision"], result.revision.0);
    assert_eq!(stored["outcome"], json!(result.outcome));
    let Some(evidence_id) = result.evidence else {
        assert!(stored["evidence_id"].is_null());
        assert!(stored["scheduled_job_id"].is_null());
        assert!(stored["audit_sequence"].is_null());
        return;
    };
    assert!(result.revision.0 > command.expected_revision.0);
    assert_eq!(stored["evidence_id"], json!(evidence_id.as_uuid()));
    let evidence = after["workflow_action_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == stored["evidence_id"])
        .unwrap();
    assert_eq!(evidence["command_id"], stored["id"]);
    assert_eq!(evidence["command_key"], stored["command_key"]);
    assert_eq!(evidence["request_digest"], stored["request_digest"]);
    scoped_evidence(after, command, evidence);
    let audit = after["workflow_run_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sequence"] == stored["audit_sequence"])
        .unwrap();
    assert_eq!(audit["event_kind"], "action_reconciled");
    for column in ["company_id", "run_id", "execution_id", "actor_id"] {
        assert_eq!(
            audit[column], evidence[column],
            "exact actor audit {column}"
        );
    }
    if matches!(result.outcome, ReconciliationOutcome::Scheduled { .. }) {
        assert_eq!(
            stored["scheduled_job_id"],
            after["background_tasks"][0]["id"]
        );
    } else {
        assert!(stored["scheduled_job_id"].is_null());
    }
}

fn scoped_evidence(after: &Value, command: &ReconcileActionCommand, evidence: &Value) {
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
        assert_eq!(evidence[column], expected, "exact scoped evidence {column}");
    }
    let coverage = after["workflow_action_evidence_coverage"]
        .as_array()
        .unwrap();
    assert_eq!(coverage.len(), 1);
    assert_eq!(coverage[0]["evidence_id"], evidence["id"]);
    let entry = &after["workflow_action_remote_entries"][0];
    assert_eq!(coverage[0]["remote_entry_id"], entry["id"]);
    for column in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
        "dispatch_id",
    ] {
        assert_eq!(
            coverage[0][column], evidence[column],
            "exact coverage {column}"
        );
        assert_eq!(
            entry[column], evidence[column],
            "exact prior entry {column}"
        );
    }
}

async fn case(truth: Truth, owner: Terminal, first: First) {
    let History {
        fixture: f,
        request,
        ledger,
    } = Box::pin(history(truth)).await;
    if matches!(owner, Terminal::Expire) {
        sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE company_id=$1 AND id=$2")
            .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
            .execute(f.persistence().pool()).await.unwrap();
    }
    let verifier = Arc::new(ObservedVerifier {
        ledger: ledger.clone(),
        calls: AtomicUsize::new(0),
        pause: Some((Arc::new(Notify::new()), Arc::new(Notify::new()))),
    });
    let command = verified_command(&f, &request, &verifier, "competing-truth").await;
    let before = all_tables(&f).await;
    let (result, terminal) =
        Box::pin(race(&f, &request, &command, verifier.clone(), owner, first)).await;
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    Box::pin(assert_result(
        &f,
        &request,
        &command,
        &result,
        terminal,
        Variant {
            truth,
            owner,
            first,
        },
    ))
    .await;
    if matches!(first, First::Terminal) {
        Box::pin(fresh_terminal_truth(&f, &command, &result, truth, ledger)).await;
    }
    let after = all_tables(&f).await;
    preserved_history(&before, &after);
    assert_eq!(
        after["workflow_runs"][0]["state"],
        match owner {
            Terminal::Cancel => "cancelled",
            Terminal::Expire => "failed",
        }
    );
    assert_eq!(
        effects(&f).await,
        i64::from(matches!(truth, Truth::Applied))
    );
    assert_eq!(
        after["workflow_action_receipts"].as_array().unwrap().len(),
        usize::from(matches!(truth, Truth::Applied))
    );
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    let evidence = &after["workflow_action_evidence"][0];
    assert_eq!(
        evidence["disposition"],
        match truth {
            Truth::Applied => "applied",
            Truth::Final => "final_not_applied",
            Truth::Unknown => "unknown",
        }
    );
    if matches!(truth, Truth::Applied) {
        let receipt = &after["workflow_action_receipts"][0];
        assert_eq!(receipt["result"], good());
        assert_eq!(receipt["reconciliation_evidence_id"], evidence["id"]);
        assert!(receipt["remote_entry_id"].is_null());
    }
    Box::pin(terminal_dispatch_inert(&f, &request, &after)).await;
}

async fn terminal_dispatch_inert(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    after: &Value,
) {
    let provider = LedgerProvider::new(f, Delivery::Pending);
    let _terminal_observation = Box::pin(invoke(f, request, &provider)).await;
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        0,
        "terminal dispatch never polls provider"
    );
    assert_eq!(
        all_tables(f).await,
        *after,
        "terminal dispatch adds no entry or history"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_control_serialization_command_before_cancel() {
    for truth in [Truth::Applied, Truth::Final, Truth::Unknown] {
        Box::pin(case(truth, Terminal::Cancel, First::Command)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_control_serialization_cancel_before_command() {
    for truth in [Truth::Applied, Truth::Final, Truth::Unknown] {
        Box::pin(case(truth, Terminal::Cancel, First::Terminal)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_control_serialization_command_before_expire() {
    for truth in [Truth::Applied, Truth::Final, Truth::Unknown] {
        Box::pin(case(truth, Terminal::Expire, First::Command)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_control_serialization_expire_before_command() {
    for truth in [Truth::Applied, Truth::Final, Truth::Unknown] {
        Box::pin(case(truth, Terminal::Expire, First::Terminal)).await;
    }
}
