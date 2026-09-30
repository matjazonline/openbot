//! Genuine under-bound entry changes invalidate a verified snapshot independently of revision.
use super::*;

#[path = "action_reconciliation_snapshot_receipt_tests.rs"]
mod receipt_tests;

struct ClaimedFinal {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<ObservedVerifier>,
}

async fn claimed_after_final() -> ClaimedFinal {
    // Box the established admission/provider/service seams for stock 2 MiB stacks.
    let (f, mut request) = Box::pin(setup()).await;
    resources(&f).await;
    let verifier = Arc::new(ObservedVerifier::new(ledger(&f, &request).await));
    let pending = LedgerProvider::new(&f, Delivery::Pending);
    assert!(matches!(
        Box::pin(invoke(&f, &request, &pending)).await,
        Err(AppError::Timeout(_))
    ));
    assert_eq!(pending.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    park(&f, &request).await;
    barrier(&f, &request).await;
    assert!(!delayed_apply(&f, &request, &good()).await);
    let c = verified_command(&f, &request, &verifier, "constructor-final").await;
    let result = Box::pin(execute(&f, &c, verifier.clone())).await.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    assert!(result.evidence.is_some());
    // The normal competing-claimants owner settles the revision BEFORE our snapshot.
    new_claim(&f, &mut request).await;
    let before = all_tables(&f).await;
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(before["workflow_action_receipts"], json!([]));
    assert_eq!(
        before["workflow_action_evidence"][0]["disposition"],
        "final_not_applied"
    );
    assert_eq!(before["workflow_runs"][0]["state"], "running");
    assert_eq!(before["background_tasks"][0]["status"], "processing");
    assert_eq!(before["task_attempts"].as_array().unwrap().len(), 2);
    ClaimedFinal {
        fixture: f,
        request,
        verifier,
    }
}

async fn unchanged_coverage_control() {
    let ClaimedFinal {
        fixture: f,
        request,
        verifier,
    } = Box::pin(claimed_after_final()).await;
    let c = verified_command(&f, &request, &verifier, "unchanged-coverage").await;
    let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
    let ReconciliationPreparation::Snapshot(snapshot) = reader.snapshot(&c).await.unwrap() else {
        panic!("genuine claimed snapshot");
    };
    assert_eq!(snapshot.entries.len(), 1);
    let attestation = verifier
        .ledger
        .verify(
            &snapshot,
            &EvidenceRecordReference::parse("constructor-check").unwrap(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(matches!(
        attestation.disposition,
        VerifiedDisposition::FinalNotApplied
    ));
    let before = all_tables(&f).await;
    let result = Box::pin(execute(&f, &c, verifier.clone())).await.unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::NotAppliedRecorded);
    assert!(result.evidence.is_some());
    assert_eq!(result.revision.0, c.expected_revision.0 + 1);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 2);
    let after = all_tables(&f).await;
    for table in [
        "workflow_action_remote_entries",
        "workflow_action_evidence_consumptions",
        "workflow_action_receipts",
        "background_tasks",
        "task_attempts",
        "workflow_action_intents",
        "workflow_action_dispatches",
    ] {
        assert_eq!(
            after[table], before[table],
            "unchanged coverage preserves {table}"
        );
    }
    assert_eq!(
        after["workflow_action_evidence"].as_array().unwrap().len(),
        2
    );
    let evidence = after["workflow_action_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(result.evidence.unwrap().as_uuid()))
        .unwrap();
    assert_eq!(evidence["disposition"], "final_not_applied");
    assert_eq!(
        evidence["grant_eligible"], false,
        "same coverage cannot mint another grant"
    );
}

fn refusal_only(before: &Value, mut after: Value, result: &ReconciliationResult) {
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::StaleSnapshot
        }
    );
    assert_eq!(result.evidence, None);
    let commands = after["workflow_action_evidence_commands"]
        .as_array()
        .unwrap();
    assert_eq!(
        commands.len(),
        before["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .len()
            + 1
    );
    let refusal = commands
        .iter()
        .find(|row| row["outcome"] == json!(result.outcome))
        .expect("exact refusal receipt");
    assert!(
        refusal
            .get("evidence_id")
            .expect("command evidence column")
            .is_null()
    );
    assert!(
        refusal
            .get("scheduled_job_id")
            .expect("command scheduling column")
            .is_null()
    );
    assert_eq!(refusal["expected_revision"], json!(result.revision.0));
    assert_eq!(refusal["result_revision"], json!(result.revision.0));
    after["workflow_action_evidence_commands"] =
        before["workflow_action_evidence_commands"].clone();
    assert_eq!(
        after, *before,
        "only immutable refusal receipt may be added"
    );
}

fn entry_change(original: &Value, before: &Value, revision: RunRevision) {
    assert_eq!(before["workflow_runs"], original["workflow_runs"]);
    assert_eq!(before["workflow_runs"][0]["revision"], json!(revision.0));
    let old = &original["workflow_action_remote_entries"][0];
    let entries = before["workflow_action_remote_entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.iter().find(|row| row["id"] == old["id"]).unwrap(),
        old
    );
    let new = entries.iter().find(|row| row["id"] != old["id"]).unwrap();
    let mut ids: Vec<Uuid> = entries
        .iter()
        .map(|row| row["id"].as_str().unwrap().parse().unwrap())
        .collect();
    ids.sort();
    assert_ne!(ids[0], ids[1]);
    let consumption = &before["workflow_action_evidence_consumptions"][0];
    let proof = &original["workflow_action_evidence"][0];
    assert_eq!(consumption["remote_entry_id"], new["id"]);
    assert_eq!(consumption["evidence_id"], proof["id"]);
    assert_eq!(
        original["workflow_action_evidence_coverage"][0]["remote_entry_id"],
        old["id"]
    );
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
        "dispatch_id",
    ] {
        assert_eq!(
            consumption.get(field).unwrap(),
            new.get(field).unwrap(),
            "consumption matches new {field}"
        );
        assert_eq!(
            consumption.get(field).unwrap(),
            proof.get(field).unwrap(),
            "consumption matches final proof {field}"
        );
    }
    assert_eq!(
        before["workflow_action_evidence_consumptions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(before["workflow_action_receipts"], json!([]));
    for table in [
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_commands",
        "workflow_root_budget_usage",
        "workflow_budget_receipts",
        "task_attempts",
        "background_tasks",
    ] {
        assert_eq!(
            before[table], original[table],
            "real new entry preserves {table}"
        );
    }
    eprintln!(
        "authentic coverage entry IDs={ids:?}; consumption proof={} new_entry={}",
        proof["id"], new["id"]
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_snapshot_binding_under_bound_entry_change_refuses() {
    Box::pin(unchanged_coverage_control()).await;
    let ClaimedFinal {
        fixture: f,
        request,
        verifier: prepared,
    } = Box::pin(claimed_after_final()).await;
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let verifier = Arc::new(ObservedVerifier {
        ledger: prepared.ledger.clone(),
        calls: AtomicUsize::new(0),
        pause: Some((started.clone(), release.clone())),
    });
    let c = verified_command(&f, &request, &verifier, "entry-after-snapshot").await;
    let original = all_tables(&f).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    let competitor = async {
        started.notified().await; // Real final attestation and provider transaction are complete.
        let before = tokio::time::timeout(Duration::from_secs(2), async {
            assert!(matches!(
                Box::pin(invoke(&f, &request, &provider)).await,
                Err(AppError::Timeout(_))
            ));
            all_tables(&f).await
        })
        .await
        .expect("snapshot/provider locks released for the competing real owner");
        assert_eq!(entry_consumptions(&f).await, (2, 1));
        entry_change(&original, &before, c.expected_revision);
        release.notify_one();
        before
    };
    let (result, before) = tokio::join!(Box::pin(execute(&f, &c, verifier.clone())), competitor);
    let result = result.unwrap();
    assert_eq!(result.revision, c.expected_revision);
    refusal_only(&before, all_tables(&f).await, &result);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&f).await, 0);
    assert!(matches!(
        Box::pin(invoke(&f, &request, &provider)).await.unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        before["workflow_action_remote_entries"],
        all_tables(&f).await["workflow_action_remote_entries"]
    );
    eprintln!(
        "coverage1->2 under128; revision={} unchanged; authentic final proof refused with StaleSnapshot; no stale evidence/grant/schedule",
        c.expected_revision.0
    );
}
