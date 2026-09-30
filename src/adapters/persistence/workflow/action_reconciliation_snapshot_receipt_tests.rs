//! Actual receipt arrival is independent of entry coverage and run revision.
use super::*;

#[derive(Clone, Copy, Debug)]
enum Observation {
    Unknown,
    AppliedMissing,
}

struct LateReceipt {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    entered: RemoteEntry,
    ledger: Arc<LedgerVerifier>,
    provider: LedgerProvider,
}

async fn prepared_receipt(observation: Observation) -> LateReceipt {
    // Box admission/provider seams so the discriminatory races run at stock 2 MiB.
    let (f, request) = Box::pin(setup()).await;
    resources(&f).await;
    let ledger = ledger(&f, &request).await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reserved) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("authentic marker and old entry");
    };
    let entered = writer.enter_remote(*reserved).await.unwrap();
    let delivery = match observation {
        Observation::Unknown => Delivery::Pending,
        Observation::AppliedMissing => Delivery::Apply {
            result: good(),
            recover: false,
            lose: false,
        },
    };
    let provider = LedgerProvider::new(&f, delivery);
    let response = Box::pin(provider.invoke(&entered.action, &entered.provider)).await;
    match observation {
        Observation::Unknown => assert!(matches!(response, Err(AppError::Timeout(_)))),
        Observation::AppliedMissing => assert_eq!(response.unwrap(), good()),
    }
    park(&f, &request).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    LateReceipt {
        fixture: f,
        request,
        entered,
        ledger,
        provider,
    }
}

async fn authentic_observation(
    f: &AdmissionFixture,
    command: &ReconcileActionCommand,
    verifier: &ObservedVerifier,
    observation: Observation,
) {
    let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
    let ReconciliationPreparation::Snapshot(snapshot) = reader.snapshot(command).await.unwrap()
    else {
        panic!("authentic parked snapshot");
    };
    assert_eq!(snapshot.entries.len(), 1);
    let attestation = verifier
        .ledger
        .verify(
            &snapshot,
            &EvidenceRecordReference::parse("receipt-observation-control").unwrap(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    match observation {
        Observation::Unknown => assert!(matches!(
            attestation.disposition,
            VerifiedDisposition::Unknown
        )),
        Observation::AppliedMissing => assert!(matches!(
            attestation.disposition,
            VerifiedDisposition::Applied {
                recovered_result: None,
                ..
            }
        )),
    }
}

fn actual_receipt_only(before: &Value, mut after: Value, entry: Uuid) {
    for table in [
        "workflow_action_receipts",
        "workflow_action_actual_receipt_observations",
    ] {
        let rows = after[table].as_array().unwrap();
        assert_eq!(rows.len(), 1, "exact actual {table}");
        assert_eq!(rows[0]["remote_entry_id"], json!(entry));
        assert_eq!(rows[0]["result"], good());
        after[table] = before[table].clone();
    }
    assert_eq!(after, *before, "late receipt changes no other public state");
}

async fn settle_after_receipt(observation: Observation) {
    let LateReceipt {
        fixture: f,
        mut request,
        entered,
        ledger,
        provider,
    } = Box::pin(prepared_receipt(observation)).await;
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let verifier = Arc::new(ObservedVerifier {
        ledger,
        calls: AtomicUsize::new(0),
        pause: Some((started.clone(), release.clone())),
    });
    let c = verified_command(&f, &request, &verifier, "late-actual-receipt-only").await;
    authentic_observation(&f, &c, &verifier, observation).await;
    let initial = all_tables(&f).await;
    let competitor = async {
        started.notified().await; // Ledger attestation and its transaction have completed.
        let saved = tokio::time::timeout(Duration::from_secs(2), async {
            if matches!(observation, Observation::Unknown) {
                assert!(delayed_apply(&f, &request, &good()).await);
            }
            assert_eq!(effects(&f).await, 1);
            let before = all_tables(&f).await;
            let entry = entered.entry;
            assert!(matches!(
                adapter(&f, 0).finish_remote(entered, good()).await.unwrap(),
                RemoteDispatchObservation::Interrupted
            ));
            let after = all_tables(&f).await;
            actual_receipt_only(&before, after.clone(), entry);
            for table in [
                "workflow_runs",
                "workflow_action_remote_entries",
                "workflow_action_intents",
                "workflow_action_dispatches",
                "background_tasks",
                "task_attempts",
            ] {
                assert_eq!(
                    after[table], initial[table],
                    "receipt arrival preserves {table}"
                );
            }
            assert_eq!(
                after["workflow_runs"][0]["revision"],
                json!(c.expected_revision.0)
            );
            after
        })
        .await
        .expect("snapshot/provider locks released for actual receipt owner");
        release.notify_one();
        saved
    };
    let (result, before) = tokio::join!(Box::pin(execute(&f, &c, verifier.clone())), competitor);
    let result = result.unwrap();
    validate_settlement(&before, &all_tables(&f).await, &result, observation);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(effects(&f).await, 1);
    verify_continuation(&f, &mut request, &provider, observation).await;
    eprintln!(
        "late actual receipt {observation:?}: same one-entry coverage and revision {}; outcome {:?}; no new remote I/O",
        c.expected_revision.0, result.outcome
    );
}

fn validate_settlement(
    before: &Value,
    after: &Value,
    result: &ReconciliationResult,
    observation: Observation,
) {
    let expected = match observation {
        Observation::Unknown => ReconciliationOutcome::UnknownRecorded,
        Observation::AppliedMissing => ReconciliationOutcome::Scheduled { receipt_only: true },
    };
    assert_eq!(result.outcome, expected);
    assert!(result.evidence.is_some());
    for table in [
        "workflow_action_remote_entries",
        "workflow_action_evidence_consumptions",
        "workflow_action_receipts",
        "workflow_action_actual_receipt_observations",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "task_attempts",
    ] {
        assert_eq!(after[table], before[table], "settlement preserves {table}");
    }
    assert_eq!(after["workflow_action_evidence_conflicts"], json!([]));
    let evidence = &after["workflow_action_evidence"][0];
    assert_eq!(evidence["grant_eligible"], false);
    assert_eq!(
        evidence["disposition"],
        match observation {
            Observation::Unknown => "unknown",
            Observation::AppliedMissing => "applied",
        }
    );
    assert_eq!(
        after["workflow_action_evidence_commands"][0]["result_revision"],
        json!(result.revision.0)
    );
    if matches!(observation, Observation::Unknown) {
        assert_eq!(after["workflow_runs"][0]["state"], "waiting");
        assert_eq!(after["background_tasks"], before["background_tasks"]);
        assert_eq!(
            result.revision.0,
            before["workflow_runs"][0]["revision"].as_u64().unwrap() + 1
        );
    } else {
        assert_eq!(after["workflow_runs"][0]["state"], "running");
        assert_eq!(after["background_tasks"][0]["status"], "pending");
    }
    assert_eq!(
        after["workflow_executions"][0]["committed_output"],
        Value::Null
    );
    assert_eq!(
        after["workflow_executions"][0]["successor_execution_id"],
        Value::Null
    );
}

async fn verify_continuation(
    f: &AdmissionFixture,
    request: &mut ActionDispatchRequest,
    provider: &LedgerProvider,
    observation: Observation,
) {
    match observation {
        Observation::Unknown => assert!(
            f.persistence()
                .claim_io(request.fence.scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        ),
        Observation::AppliedMissing => {
            new_claim(f, request).await;
            let RemoteDispatchObservation::Committed(receipt) =
                Box::pin(invoke(f, request, provider)).await.unwrap()
            else {
                panic!("normal claimed worker returns the actual receipt");
            };
            assert_eq!(receipt.result, good());
        }
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(f).await, (1, 0));
    assert_eq!(effects(f).await, 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_snapshot_binding_late_actual_receipt_same_coverage_revision()
 {
    for observation in [Observation::Unknown, Observation::AppliedMissing] {
        Box::pin(settle_after_receipt(observation)).await;
    }
}
