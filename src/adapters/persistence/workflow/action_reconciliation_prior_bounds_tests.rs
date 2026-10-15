//! One invocation's genuine claim/retire/reopen history at the prior-entry cap.
use super::*;

const FIXTURE_ATTEMPTS: i32 = 132;

#[path = "action_reconciliation_prior_bounds_support.rs"]
mod support;
use support::{PausedVerifier, park_live, pending_loss};

#[path = "action_reconciliation_prior_bounds_controls.rs"]
mod controls;

async fn prior_fixture() -> (AdmissionFixture, ActionDispatchRequest) {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer","minimum":1,"maximum":1000}},"required":["value"]});
    source["steps"]["start"]["with"]["max_tokens"] = json!({"ref":"/input/value"});
    // Supported limits are frozen before admission; nothing replenishes a debit.
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":10,"repetitions":132});
    let (f, scope) = fixture_source(source).await;
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        attempts, 0,
        "fixture allowance precedes the very first claim"
    );
    sqlx::query("UPDATE background_tasks SET max_retries=$2 WHERE id=$1")
        .bind(scope.job.0)
        .bind(FIXTURE_ATTEMPTS)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    // The existing owner creates the claim and immutable action provenance.
    Box::pin(setup_action_on_fixture(
        f,
        scope,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await
}

async fn schedule_prior(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    verifier: &Arc<LedgerVerifier>,
    number: usize,
) -> ReconciliationResult {
    barrier(f, request).await;
    assert!(!delayed_apply(f, request, &good()).await);
    let key = format!("prior-final-{number}");
    let before = all_tables(f).await;
    let command = proof_command(f, request, verifier, &key).await;
    let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
    let ReconciliationPreparation::Snapshot(snapshot) = reader.snapshot(&command).await.unwrap()
    else {
        panic!("{number} prior entries must fit without truncation")
    };
    assert_eq!(snapshot.entries.len(), number);
    let result = run_command(f, &command, verifier.clone()).await.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let coverage: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_evidence_coverage WHERE evidence_id=$1",
    )
    .bind(result.evidence.unwrap().as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(coverage, number as i64);
    let after = all_tables(f).await;
    preserved_history(&before, &after);
    assert_eq!(
        after["background_tasks"][0]["max_retries"],
        FIXTURE_ATTEMPTS
    );
    assert!(
        retry_safe(f, request).await,
        "SQL schedule safety at {number}"
    );
    result
}

async fn prior_history(
    f: &AdmissionFixture,
    request: &mut ActionDispatchRequest,
    verifier: &Arc<LedgerVerifier>,
) -> Option<ReconciliationResult> {
    let pending = LedgerProvider::new(f, Delivery::Pending);
    for number in 1..=128 {
        Box::pin(pending_loss(f, request, &pending)).await?;
        assert_eq!(pending.calls.load(Ordering::SeqCst), number);
        assert_eq!(
            entry_consumptions(f).await,
            (number as i64, number as i64 - 1)
        );
        Box::pin(park_live(f, request)).await?;
        // Box each phase so 128 owner cycles do not enlarge the poll frame.
        let result = Box::pin(schedule_prior(f, request, verifier, number)).await;
        if number == 128 {
            return Some(result);
        }
        new_claim(f, request).await;
    }
    unreachable!()
}

async fn available(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    excluded: Option<Uuid>,
) -> Option<Uuid> {
    sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2,$3)")
        .bind(request.scope().company.as_uuid())
        .bind(request.subject.invocation.as_uuid())
        .bind(excluded)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

/// While verification is paused, the 8,256-row proof coverage history is compared by digest
/// (`digested_tables`): the same rows decide equality, without decoding them inside the budget.
const PAUSE_DIGESTED: [&str; 1] = ["workflow_action_evidence_coverage"];

/// Tables the 129th provider entry must leave as the claim left them.
const ENTRY_PRESERVES: [&str; 6] = [
    "workflow_action_intents",
    "workflow_action_dispatches",
    "workflow_action_evidence",
    "workflow_action_evidence_coverage",
    "workflow_budget_receipts",
    "workflow_root_budget_usage",
];

/// The scheduled 128-entry premise, settled before the paused command starts (DEC-011): the
/// command only reads until settlement, so the competitor need not re-observe it while paused.
async fn scheduled_128(f: &AdmissionFixture, request: &ActionDispatchRequest, proof: Uuid) {
    let scheduled = all_tables(f).await;
    assert_eq!(entry_consumptions(f).await, (128, 127));
    assert_eq!(available(f, request, None).await, Some(proof));
    let old = scheduled["workflow_action_remote_entries"][0]["id"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    for excluded in [old, Uuid::new_v4()] {
        assert_eq!(available(f, request, Some(excluded)).await, None);
    }
    assert_eq!(all_tables(f).await, scheduled);
}

/// The genuine competing 128-to-129 transition, and only that, while verification is paused:
/// the claim and entry are observed through exactly the relations the assertions compare.
async fn consume_128(
    f: &AdmissionFixture,
    request: &mut ActionDispatchRequest,
    proof: Uuid,
    started: std::time::Instant,
) -> Option<()> {
    let phase = |label: &'static str, fence: WorkflowFence| {
        lease_phase(f, fence, "prior_bounds", label, started)
    };
    phase("competitor_started", request.fence).await;
    assert_eq!(entry_consumptions(f).await, (128, 127));
    new_claim(f, request).await;
    phase("claimed_129", request.fence).await;
    renew_fixture_lease(f, request.fence).await;
    let mut observed = ENTRY_PRESERVES.to_vec();
    observed.push("task_attempts");
    let claimed = digested_tables(f, Some(&observed), &PAUSE_DIGESTED).await;
    phase("claimed_snapshot", request.fence).await;
    assert_eq!(claimed["task_attempts"].as_array().unwrap().len(), 129);
    let pending = LedgerProvider::new(f, Delivery::Pending);
    Box::pin(support::observe_pending(f, request, &pending)).await?;
    phase("consumed_129", request.fence).await;
    assert_eq!(pending.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(f).await, (129, 128));
    let consumed: Uuid = sqlx::query_scalar(
        "SELECT remote_entry_id FROM workflow_action_evidence_consumptions WHERE evidence_id=$1",
    )
    .bind(proof)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(available(f, request, Some(consumed)).await, Some(proof));
    assert_eq!(available(f, request, None).await, None);
    assert_eq!(effects(f).await, 0);
    let mut observed = ENTRY_PRESERVES.to_vec();
    observed.push("background_tasks");
    let after = digested_tables(f, Some(&observed), &PAUSE_DIGESTED).await;
    phase("consumed_snapshot", request.fence).await;
    for table in ENTRY_PRESERVES {
        assert_eq!(
            after[table], claimed[table],
            "provider entry preserves {table}"
        );
    }
    assert_eq!(
        after["background_tasks"][0]["max_retries"],
        FIXTURE_ATTEMPTS
    );
    assert_eq!(
        after["workflow_action_dispatches"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    Some(())
}

fn only_refusal(
    before: &Value,
    mut after: Value,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
) {
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::BoundExceeded
        }
    );
    assert_eq!(
        json!(result.revision.0),
        before["workflow_runs"][0]["revision"]
    );
    assert_eq!(result.evidence, None);
    assert!(!result.replayed);
    let matches = |row: &Value| {
        row["company_id"] == json!(command.scope.company.as_uuid())
            && row["command_key"] == json!(command.command_key.as_str())
    };
    let prior = before["workflow_action_evidence_commands"]
        .as_array()
        .unwrap();
    assert!(!prior.iter().any(matches), "refusal command must be new");
    let commands = after["workflow_action_evidence_commands"]
        .as_array_mut()
        .unwrap();
    assert_eq!(commands.len(), prior.len() + 1, "exactly one new receipt");
    assert_eq!(commands.iter().filter(|row| matches(row)).count(), 1);
    let index = commands.iter().position(matches).unwrap();
    let receipt = &commands[index];
    let id: Uuid = receipt["id"].as_str().unwrap().parse().unwrap();
    assert!(!id.is_nil());
    assert!(prior.iter().all(|row| row["id"] != receipt["id"]));
    for (column, expected) in [
        ("run_id", json!(command.scope.run.as_uuid())),
        ("execution_id", json!(command.scope.execution.as_uuid())),
        ("invocation_id", json!(command.subject.invocation.as_uuid())),
        (
            "argument_digest",
            json!(command.subject.argument_digest.as_str()),
        ),
        ("dispatch_id", json!(command.marker.as_uuid())),
        ("actor_id", json!(command.actor.user_id())),
        (
            "request_digest",
            json!(command.request_digest().unwrap().as_str()),
        ),
        ("expected_revision", json!(command.expected_revision.0)),
        ("result_revision", json!(result.revision.0)),
        ("evidence_id", Value::Null),
        ("audit_sequence", Value::Null),
    ] {
        assert_eq!(receipt[column], expected, "refusal {column}");
    }
    let outcome: ReconciliationOutcome =
        serde_json::from_value(receipt["outcome"].clone()).unwrap();
    assert_eq!(outcome, result.outcome);
    commands.remove(index);
    assert_eq!(after, *before, "only the exact refusal receipt may change");
}

async fn refuse_129(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    verifier: &Arc<LedgerVerifier>,
) {
    assert!(!retry_safe(f, request).await);
    assert_eq!(projection(f).await, ("needs_reconciliation".into(), false));
    let parked = all_tables(f).await;
    assert_eq!(parked["background_tasks"][0]["retry_count"], 129);
    assert_eq!(parked["task_attempts"].as_array().unwrap().len(), 129);
    assert!(
        parked["task_attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["workflow_retry_safety"] == "unknown")
    );
    assert_eq!(available(f, request, None).await, None);
    let old: Uuid = parked["workflow_action_remote_entries"][0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(available(f, request, Some(old)).await, None);
    assert_eq!(available(f, request, Some(Uuid::new_v4())).await, None);
    let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
    let fresh = proof_command(f, request, verifier, "129-prior-snapshot").await;
    let before = all_tables(f).await;
    let ReconciliationPreparation::Recorded { result, .. } = reader.snapshot(&fresh).await.unwrap()
    else {
        panic!("129 entries must refuse before provider verification")
    };
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::BoundExceeded
        }
    );
    only_refusal(&before, all_tables(f).await, &fresh, &result);
    let before_recovery = all_tables(f).await;
    assert!(
        f.persistence()
            .settled()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let provider = LedgerProvider::new(f, Delivery::Pending);
    let refused = invoke(f, request, &provider)
        .await
        .err()
        .expect("a refusal, not a dispatch");
    assert!(
        matches!(&refused, AppError::Conflict(message) if message == "Workflow lifecycle revision or command conflict"),
        "expected refusal: {refused:?}"
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(effects(f).await, 0);
    assert_eq!(
        all_tables(f).await,
        before_recovery,
        "SQL/Rust recovery cannot create attempts, debits, entries or refunds"
    );
    let after = all_tables(f).await;
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_consumptions",
        "workflow_action_remote_entries",
        "task_attempts",
        "workflow_budget_receipts",
        "workflow_root_budget_usage",
        "background_tasks",
        "workflow_runs",
    ] {
        assert_eq!(
            after[table], parked[table],
            "129-prior refusal preserves {table}"
        );
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses() {
    Box::pin(support::fresh_proof(prove_prior_bound))
        .await
        .expect("a complete boundary proof is required");
}

async fn prove_prior_bound() -> Option<()> {
    // Box fixture/provider phases to keep this regression in the stock-stack gate.
    let (f, mut request) = Box::pin(prior_fixture()).await;
    let verifier = ledger(&f, &request).await;
    let result = Box::pin(prior_history(&f, &mut request, &verifier)).await?;
    let proof = result.evidence.unwrap().as_uuid();
    Box::pin(scheduled_128(&f, &request, proof)).await;
    let observed = Arc::new(PausedVerifier::new(verifier.clone()));
    let stale = proof_command(&f, &request, &verifier, "128-paused-before-129").await;
    let command_started = std::time::Instant::now();
    let reconcile = observed.run(&f, &stale);
    let competitor = async {
        assert!(
            observed.wait_started().await,
            "reconciliation ended before verifier pause"
        );
        let consumed = Box::pin(consume_128(&f, &mut request, proof, command_started)).await;
        let parked = if consumed.is_some() {
            renew_fixture_lease(&f, request.fence).await;
            Box::pin(support::park_observed_as(&f, &request, &PAUSE_DIGESTED)).await
        } else {
            None
        };
        lease_phase(&f, request.fence, "prior_bounds", "parked", command_started).await;
        if parked.is_some() {
            barrier(&f, &request).await;
        }
        let before = digested_tables(&f, None, &PAUSE_DIGESTED).await;
        lease_phase(
            &f,
            request.fence,
            "prior_bounds",
            "pre_settlement_snapshot",
            command_started,
        )
        .await;
        if parked.is_some() {
            *observed.released_at.lock().unwrap() = Some(std::time::Instant::now());
            observed.release.notify_one();
        }
        (before, parked)
    };
    let (settled, (before, parked)) = tokio::join!(reconcile, competitor);
    observed.timeline(
        command_started,
        &format!("{:?}", settled.as_ref().map(|result| &result.outcome)),
    );
    let after = digested_tables(&f, None, &PAUSE_DIGESTED).await;
    if !observed.returned.load(Ordering::SeqCst) {
        support::withheld_timeout(
            &settled,
            command_started.elapsed(),
            observed
                .released_at
                .lock()
                .unwrap()
                .map(|time| time.duration_since(command_started)),
        );
        assert_eq!(
            after, before,
            "withheld proof must not write reconciliation facts"
        );
        uncaptured("BACKSTOP prior_bounds withheld_verification timeout; no reconciliation delta");
        return None;
    }
    assert!(
        parked.is_some(),
        "a void competitor must not release stale evidence"
    );
    let settled = settled.unwrap();
    assert_eq!(
        settled.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::BoundExceeded
        }
    );
    assert_eq!(settled.evidence, None);
    assert_eq!(observed.calls.load(Ordering::SeqCst), 1);
    only_refusal(&before, after, &stale, &settled);
    Box::pin(refuse_129(&f, &request, &verifier)).await;
    Some(())
}
