//! Real directory SQL failures stay operational errors at each authorization boundary.
use super::super::proof_tests::all_tables;
use super::*;

async fn make_unavailable(f: &AdmissionFixture) {
    sqlx::query(
        "ALTER TABLE fixture_actor_resources RENAME TO fixture_actor_resources_unavailable",
    )
    .execute(f.persistence().pool())
    .await
    .unwrap();
}

async fn restore_directory(f: &AdmissionFixture) {
    sqlx::query(
        "ALTER TABLE fixture_actor_resources_unavailable RENAME TO fixture_actor_resources",
    )
    .execute(f.persistence().pool())
    .await
    .unwrap();
}

fn operational_error(result: AppResult<ReconciliationResult>) {
    assert!(
        matches!(result, Err(AppError::Database(message))
            if message.contains("relation \"fixture_actor_resources\" does not exist")),
        "the actual directory SQL failure must remain a database error"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_directory_preflight_error_is_not_access_refusal() {
    // Box the extended fixture seam to preserve stock 2 MiB test stacks.
    let (f, request, verifier) = Box::pin(prepared()).await;
    let c = verified_command(&f, &request, &verifier, "directory-preflight").await;
    let p = f.persistence().clone();
    // Snapshot uses its genuine, separate fixture_action_resources directory.
    // Only the later application ResourceDirectory lookup is made unavailable.
    let service = ActionReconciliationService::new(
        PostgresActionReconciliation::new(p.clone(), Resources),
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        ActorResources(p),
        Some(verifier.clone() as Arc<dyn ActionEvidenceVerifier>),
    );
    let cancellation = CancellationToken::new();
    grant(&f, c.actor, false).await;
    let denied = all_tables(&f).await;
    assert!(matches!(
        Box::pin(service.reconcile(&c, &cancellation, Duration::from_secs(5))).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(all_tables(&f).await, denied);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 0);
    grant(&f, c.actor, true).await;
    make_unavailable(&f).await;
    let unavailable = all_tables(&f).await;
    operational_error(Box::pin(service.reconcile(&c, &cancellation, Duration::from_secs(5))).await);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 0);
    assert_eq!(all_tables(&f).await, unavailable);
    restore_directory(&f).await;
    assert!(matches!(
        Box::pin(service.reconcile(&c, &cancellation, Duration::from_secs(5)))
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::UnknownRecorded
    ));
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_directory_transactional_snapshot_error_propagates() {
    let (f, request, verifier) = Box::pin(prepared()).await;
    let c = verified_command(&f, &request, &verifier, "directory-snapshot").await;
    make_unavailable(&f).await;
    let unavailable = all_tables(&f).await;
    // The real SQL resource lookup inside snapshot runs before inspect/verifier.
    operational_error(Box::pin(execute(&f, &c, verifier.clone())).await);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 0);
    assert_eq!(all_tables(&f).await, unavailable);
    restore_directory(&f).await;
    assert!(matches!(
        Box::pin(execute(&f, &c, verifier.clone()))
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::UnknownRecorded
    ));
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_directory_settlement_error_cannot_commit_final_proof() {
    let (f, request, prepared_verifier) = Box::pin(prepared()).await;
    barrier(&f, &request).await;
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let verifier = Arc::new(ObservedVerifier {
        ledger: prepared_verifier.ledger.clone(),
        calls: AtomicUsize::new(0),
        pause: Some((started.clone(), release.clone())),
    });
    let c = verified_command(&f, &request, &verifier, "directory-settlement").await;
    let fault = async {
        started.notified().await;
        let unavailable = tokio::time::timeout(Duration::from_secs(2), async {
            make_unavailable(&f).await;
            all_tables(&f).await
        })
        .await
        .expect("snapshot directory locks released before verifier I/O");
        release.notify_one();
        unavailable
    };
    let (result, unavailable) = tokio::join!(Box::pin(execute(&f, &c, verifier.clone())), fault);
    operational_error(result);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(all_tables(&f).await, unavailable);
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(all_tables(&f).await, unavailable);
    restore_directory(&f).await;
    // The exact same authentic closed ledger and command can schedule only once
    // the real directory is available, proving the error case reached its guard.
    assert_eq!(
        Box::pin(execute(&f, &c, prepared_verifier.clone()))
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    assert_eq!(prepared_verifier.calls.load(Ordering::SeqCst), 1);
}
