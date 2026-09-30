//! Held actual provider responses contradict trusted finality without rewriting history.
use super::*;
use crate::application::workflow::{CancelCommand, CancelResult};

#[path = "action_reconciliation_live_boundary_tests.rs"]
mod live_boundary_tests;

struct HeldTruth {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
    entry: Option<RemoteEntry>,
    entry_id: Uuid,
    marker: Uuid,
    result: Value,
}

async fn held_truth() -> HeldTruth {
    // Box established admission and provider seams to retain stock 2 MiB stacks.
    let (f, request) = Box::pin(setup()).await;
    let verifier = ledger(&f, &request).await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("original entered request");
    };
    let entry = writer.enter_remote(*reserved).await.unwrap();
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: false,
            lose: false,
        },
    );
    let result = Box::pin(provider.invoke(&entry.action, &entry.provider))
        .await
        .unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&f).await, 1);
    assert_eq!(counts(&f).await, (0, 0, 0));
    park(&f, &request).await;
    HeldTruth {
        fixture: f,
        request,
        verifier,
        entry_id: entry.entry,
        marker: entry.marker,
        entry: Some(entry),
        result,
    }
}

async fn schedule_final(h: &HeldTruth, key: &str) -> ReconciliationResult {
    // An explicitly faulty trusted verifier models the breach this boundary must
    // retain. The real provider effect and response remain unchanged and provable.
    let verifier = Arc::new(ForcedVerifier {
        inner: h.verifier.clone(),
        final_not_applied: true,
        future: false,
    });
    let result = Box::pin(reconcile_with(
        &h.fixture, &h.request, h.marker, verifier, key,
    ))
    .await
    .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let state = all_tables(&h.fixture).await;
    assert_eq!(state["background_tasks"].as_array().unwrap().len(), 1);
    assert_eq!(state["background_tasks"][0]["status"], "pending");
    assert_eq!(state["workflow_runs"][0]["state"], "running");
    result
}

async fn consume_and_park(h: &mut HeldTruth) {
    new_claim(&h.fixture, &mut h.request).await;
    let provider = LedgerProvider::new(&h.fixture, Delivery::Pending);
    assert!(matches!(
        Box::pin(invoke(&h.fixture, &h.request, &provider)).await,
        Err(AppError::Timeout(_))
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(&h.fixture).await, (2, 1));
    park(&h.fixture, &h.request).await;
    let state = all_tables(&h.fixture).await;
    assert_eq!(state["background_tasks"][0]["status"], "failed");
    assert_eq!(state["workflow_runs"][0]["state"], "waiting");
    assert_eq!(effects(&h.fixture).await, 1);
    Box::pin(schedule_final(h, "cover-consumed-entry")).await;
}

fn unchanged_tables(before: &Value, after: &Value, changed: &[&str]) {
    for (table, rows) in before.as_object().unwrap() {
        if !changed.contains(&table.as_str()) {
            assert_eq!(after[table], *rows, "preserved public table {table}");
        }
    }
    assert_eq!(
        before.as_object().unwrap().len(),
        after.as_object().unwrap().len()
    );
}

fn state_witnesses(before: &Value, after: &Value) {
    let prior = before["workflow_action_state_witnesses"]
        .as_array()
        .unwrap();
    let current = after["workflow_action_state_witnesses"].as_array().unwrap();
    assert!(
        prior.iter().all(|row| current.contains(row)),
        "immutable prior state witnesses"
    );
    let added: Vec<_> = current.iter().filter(|row| !prior.contains(row)).collect();
    if before["workflow_runs"][0]["state"] == "running" {
        assert_eq!(added.len(), 1, "actual running-to-waiting owner witness");
        assert_eq!(added[0]["initial_state"], "running");
        assert!(added[0]["initial_waiting_reason"].is_null());
        assert_eq!(added[0]["run_id"], before["workflow_runs"][0]["id"]);
        assert_eq!(
            added[0]["company_id"],
            before["workflow_runs"][0]["company_id"]
        );
    } else {
        assert!(
            added.is_empty(),
            "terminal truth invents no state transition witness"
        );
    }
}

fn retained_truth(before: &Value, after: &Value, entry: Uuid, result: &Value) {
    unchanged_tables(
        before,
        after,
        &[
            "workflow_runs",
            "background_tasks",
            "workflow_action_receipts",
            "workflow_action_actual_receipt_observations",
            "workflow_action_evidence_conflicts",
            "workflow_action_effect_states",
            "workflow_action_state_witnesses",
        ],
    );
    state_witnesses(before, after);
    let receipt = &after["workflow_action_receipts"][0];
    assert_eq!(
        after["workflow_action_receipts"].as_array().unwrap().len(),
        1
    );
    assert_eq!(receipt["result"], *result);
    assert_eq!(receipt["remote_entry_id"], json!(entry));
    assert!(receipt["reconciliation_evidence_id"].is_null());
    let observations = after["workflow_action_actual_receipt_observations"]
        .as_array()
        .unwrap();
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0]["remote_entry_id"], json!(entry));
    assert_eq!(observations[0]["result"], *result);
    let evidence = before["workflow_action_evidence"].as_array().unwrap();
    let conflicts = after["workflow_action_evidence_conflicts"]
        .as_array()
        .unwrap();
    assert_eq!(conflicts.len(), evidence.len());
    for proof in evidence {
        assert_eq!(proof["disposition"], "final_not_applied");
        let conflict = conflicts
            .iter()
            .find(|c| c["evidence_id"] == proof["id"])
            .unwrap();
        assert_eq!(conflict["reason"], "finality_breach");
        assert_eq!(conflict["remote_entry_id"], json!(entry));
        assert_eq!(conflict["actual_observation_id"], observations[0]["id"]);
        for field in [
            "company_id",
            "run_id",
            "execution_id",
            "invocation_id",
            "argument_digest",
            "dispatch_id",
        ] {
            assert_eq!(
                conflict[field], proof[field],
                "exact conflict provenance {field}"
            );
            assert_eq!(
                receipt[field], proof[field],
                "exact receipt provenance {field}"
            );
        }
        assert!(
            before["workflow_action_evidence_coverage"]
                .as_array()
                .unwrap()
                .iter()
                .any(|coverage| {
                    coverage["evidence_id"] == proof["id"]
                        && coverage["remote_entry_id"] == json!(entry)
                })
        );
    }
    assert!(
        after["workflow_runs"][0]["revision"].as_i64().unwrap()
            > before["workflow_runs"][0]["revision"].as_i64().unwrap()
    );
    assert_eq!(after["workflow_executions"], before["workflow_executions"]);
}

fn same_job(before: &Value, after: &Value) {
    assert_eq!(after["background_tasks"].as_array().unwrap().len(), 1);
    let mut job = after["background_tasks"][0].clone();
    job["status"] = before["background_tasks"][0]["status"].clone();
    job["updated_at"] = before["background_tasks"][0]["updated_at"].clone();
    assert_eq!(
        job, before["background_tasks"][0],
        "same job, retry accounting and lease fields"
    );
}

async fn inert(h: &HeldTruth, saved: &Value) {
    assert!(!retry_safe(&h.fixture, &h.request).await);
    let (a, b) = tokio::join!(
        h.fixture
            .persistence()
            .claim_io(h.request.fence.scope, worker(), policy()),
        h.fixture
            .persistence()
            .claim_io(h.request.fence.scope, worker(), policy())
    );
    assert!(a.unwrap().is_none() && b.unwrap().is_none());
    let provider = LedgerProvider::new(&h.fixture, Delivery::Pending);
    let outcome = Box::pin(invoke(&h.fixture, &h.request, &provider)).await;
    assert!(matches!(
        outcome,
        Err(_) | Ok(RemoteDispatchObservation::Interrupted)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(
        h.fixture
            .persistence()
            .complete_io(FencedWorkflowResult {
                fence: h.request.fence,
                output: h.result.clone(),
            })
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        *saved,
        all_tables(&h.fixture).await,
        "conflict cannot claim, poll or complete"
    );
    assert_eq!(effects(&h.fixture).await, 1);
}

async fn pending_case(consumed: bool) {
    let mut h = Box::pin(held_truth()).await;
    Box::pin(schedule_final(&h, "first-final")).await;
    if consumed {
        Box::pin(consume_and_park(&mut h)).await;
    }
    let before = all_tables(&h.fixture).await;
    assert_eq!(
        before["task_attempts"].as_array().unwrap().len(),
        if consumed { 2 } else { 1 }
    );
    assert_eq!(
        entry_consumptions(&h.fixture).await,
        if consumed { (2, 1) } else { (1, 0) }
    );
    let entry_id = h.entry_id;
    assert!(matches!(
        adapter(&h.fixture, 0)
            .finish_remote(h.entry.take().unwrap(), h.result.clone())
            .await
            .unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    let after = all_tables(&h.fixture).await;
    retained_truth(&before, &after, entry_id, &h.result);
    same_job(&before, &after);
    assert_eq!(after["background_tasks"][0]["status"], "failed");
    assert_eq!(after["workflow_runs"][0]["state"], "waiting");
    assert_eq!(
        after["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );
    assert_eq!(before["workflow_runs"].as_array().unwrap().len(), 1);
    let mut runs = after["workflow_runs"].clone();
    for field in ["state", "waiting_reason", "revision"] {
        runs[0][field] = before["workflow_runs"][0][field].clone();
    }
    assert_eq!(
        runs, before["workflow_runs"],
        "same run inventory, identity, lineage, budgets and deadline"
    );
    assert_eq!(projection(&h.fixture).await, ("committed".into(), true));
    Box::pin(inert(&h, &after)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_late_truth_pending_before_consumption() {
    Box::pin(pending_case(false)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_late_truth_pending_after_consumption() {
    Box::pin(pending_case(true)).await;
}

async fn terminal_case(cancelled: bool) {
    let mut h = Box::pin(held_truth()).await;
    Box::pin(schedule_final(&h, "terminal-final")).await;
    if cancelled {
        let c = command(&h.fixture, &h.request, h.marker).await;
        assert!(matches!(
            h.fixture
                .persistence()
                .cancel(CancelCommand {
                    company_id: c.scope.company,
                    run_id: c.scope.run,
                    actor: c.actor,
                    expected_revision: c.expected_revision,
                    command_key: IdempotencyKey::parse("terminal-cancel").unwrap(),
                })
                .await
                .unwrap(),
            CancelResult::Applied { .. }
        ));
    } else {
        sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE company_id=$1 AND id=$2")
            .bind(h.request.scope().company.as_uuid()).bind(h.request.scope().run.as_uuid())
            .execute(h.fixture.persistence().pool()).await.unwrap();
        assert!(
            h.fixture
                .persistence()
                .expire_run(h.request.fence.scope)
                .await
                .unwrap()
        );
    }
    let before = all_tables(&h.fixture).await;
    assert_eq!(
        before["workflow_runs"][0]["state"],
        if cancelled { "cancelled" } else { "failed" }
    );
    assert!(matches!(
        adapter(&h.fixture, 0)
            .finish_remote(h.entry.take().unwrap(), h.result.clone())
            .await
            .unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    let after = all_tables(&h.fixture).await;
    retained_truth(&before, &after, h.entry_id, &h.result);
    assert_eq!(before["background_tasks"], after["background_tasks"]);
    let mut run = after["workflow_runs"][0].clone();
    run["revision"] = before["workflow_runs"][0]["revision"].clone();
    assert_eq!(
        run, before["workflow_runs"][0],
        "terminal state, reason and lineage stay intact"
    );
    assert_eq!(entry_consumptions(&h.fixture).await, (1, 0));
    assert_eq!(projection(&h.fixture).await, ("committed".into(), true));
    Box::pin(inert(&h, &after)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_late_truth_cancelled_retains_conflicting_actual_receipt() {
    Box::pin(terminal_case(true)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_late_truth_expired_retains_conflicting_actual_receipt() {
    Box::pin(terminal_case(false)).await;
}
