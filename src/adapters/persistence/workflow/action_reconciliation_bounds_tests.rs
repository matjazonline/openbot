//! Late overflow must invalidate reconciliation authority without hiding history.
use super::*;
use tokio::sync::Notify;

#[path = "action_reconciliation_prior_bounds_tests.rs"]
mod prior_bounds_tests;

async fn overflow_sibling(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    assert!(
        f.persistence()
            .renew_io(request.fence, policy())
            .await
            .unwrap()
            .is_some()
    );
    let extra = sibling(f, request, 9129).await;
    let provider = LedgerProvider::new(
        f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    assert!(matches!(
        invoke(f, &extra, &provider).await.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(!retry_safe(f, request).await);
    assert_eq!(
        all_tables(f).await["workflow_action_dispatches"]
            .as_array()
            .unwrap()
            .len(),
        129
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_bounds_paused_128_to_129_settlement_refuses() {
    let (f, request, _) = Box::pin(boundary_tests::accepted_limit()).await;
    let action = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap()
        .action;
    let original = Arc::new(LedgerVerifier {
        persistence: f.persistence().clone(),
        registration: EvidenceVerifierRegistration::approve(
            EvidenceVerifierId::parse("fixture.ledger").unwrap(),
            EvidenceVerifierVersion::parse("v1").unwrap(),
            EvidenceProviderId::parse("fixture").unwrap(),
            &action.request().contract,
            &action.request().target,
        )
        .unwrap(),
    });
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let verifier = Arc::new(ObservedVerifier {
        ledger: original,
        calls: AtomicUsize::new(0),
        pause: Some((started.clone(), release.clone())),
    });
    let c = verified_command(&f, &request, &verifier, "late-overflow").await;
    let competitor = async {
        started.notified().await;
        Box::pin(overflow_sibling(&f, &request)).await;
        let before = all_tables(&f).await;
        release.notify_one();
        before
    };
    let (result, before) = tokio::join!(run_command(&f, &c, verifier.clone()), competitor);
    let result = result.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::BoundExceeded
        }
    );
    assert_eq!(result.evidence, None);
    assert_eq!(result.revision, c.expected_revision);
    let mut after = all_tables(&f).await;
    assert_eq!(
        after["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    after["workflow_action_evidence_commands"] =
        before["workflow_action_evidence_commands"].clone();
    assert_eq!(
        after, before,
        "only the immutable refusal can commit after overflow"
    );
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
}

async fn scheduled_limit() -> (AdmissionFixture, ActionDispatchRequest) {
    let (f, mut request) = setup().await;
    let verifier = ledger(&f, &request).await;
    let pending = LedgerProvider::new(&f, Delivery::Pending);
    assert!(matches!(
        invoke(&f, &request, &pending).await,
        Err(AppError::Timeout(_))
    ));
    let receipt = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    for number in 1..128 {
        assert!(
            f.persistence()
                .renew_io(request.fence, policy())
                .await
                .unwrap()
                .is_some()
        );
        let other = sibling(&f, &request, number + 2000).await;
        assert!(matches!(
            invoke(&f, &other, &receipt).await.unwrap(),
            RemoteDispatchObservation::Committed(_)
        ));
    }
    park(&f, &request).await;
    barrier(&f, &request).await;
    assert_eq!(
        all_tables(&f).await["workflow_action_dispatches"]
            .as_array()
            .unwrap()
            .len(),
        128
    );
    let result = reconcile(
        &f,
        &request,
        scoped_marker(&f, &request).await,
        verifier,
        "bounded-final",
    )
    .await;
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    assert_eq!(entry_consumptions(&f).await, (128, 0));
    new_claim(&f, &mut request).await;
    (f, request)
}

#[tokio::test]
async fn workflow_action_reconciliation_bounds_sibling_overflow_before_proof_reserve_refuses() {
    let (f, request) = Box::pin(scheduled_limit()).await;
    Box::pin(overflow_sibling(&f, &request)).await;
    let before = all_tables(&f).await;
    let result = adapter(&f, 0).reserve_remote(&request, None).await;
    let received = match result {
        Ok(RemoteReservationResult::Reserved(_)) => "reserved",
        Ok(RemoteReservationResult::PossibleDispatchExists) => "possible_dispatch_exists",
        Ok(_) => "other",
        Err(_) => "error",
    };
    assert_eq!(
        entry_consumptions(&f).await,
        (129, 0),
        "129 siblings must not mint another reconciliation reservation; received={received}"
    );
    assert_eq!(received, "possible_dispatch_exists");
    assert_eq!(all_tables(&f).await, before);
}

#[tokio::test]
async fn workflow_action_reconciliation_bounds_sibling_overflow_before_proof_enter_refuses() {
    let (f, request) = Box::pin(scheduled_limit()).await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("exactly128 siblings permit one proof reservation")
    };
    assert_eq!(entry_consumptions(&f).await, (129, 1));
    Box::pin(overflow_sibling(&f, &request)).await;
    let before = all_tables(&f).await;
    assert!(
        writer.enter_remote(*reservation).await.is_err(),
        "late129th sibling must prevent polling with consumed reconciliation authority"
    );
    assert_eq!(all_tables(&f).await, before);
    assert_eq!(entry_consumptions(&f).await, (130, 1));
}

#[tokio::test]
async fn workflow_action_reconciliation_bounds_exclusion_requires_exact_consumption() {
    let (f, request) = Box::pin(scheduled_limit()).await;
    let company = request.scope().company.as_uuid();
    let invocation = request.subject.invocation.as_uuid();
    let old: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(company)
    .bind(invocation)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let unrelated: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id<>$2 LIMIT 1",
    )
    .bind(company)
    .bind(invocation)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let missing = uuid::Uuid::new_v4();
    let before = all_tables(&f).await;
    let proof: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2)")
            .bind(company)
            .bind(invocation)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert!(proof.is_some(), "128 siblings retain available final proof");
    for excluded in [old, unrelated, missing] {
        let available: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2,$3)")
                .bind(company)
                .bind(invocation)
                .bind(excluded)
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        assert_eq!(
            available, None,
            "an unmatched caller-named entry grants nothing"
        );
    }
    assert_eq!(all_tables(&f).await, before);
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("valid proof reserves once")
    };
    let entry = reservation.entry;
    let consumed = all_tables(&f).await;
    for excluded in [None, Some(old), Some(unrelated), Some(missing), Some(entry)] {
        let available: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2,$3)")
                .bind(company)
                .bind(invocation)
                .bind(excluded)
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        assert_eq!(
            available,
            (excluded == Some(entry)).then_some(proof.unwrap())
        );
    }
    assert_eq!(all_tables(&f).await, consumed);
    writer.enter_remote(*reservation).await.unwrap();
    assert_eq!(all_tables(&f).await, consumed);
}
