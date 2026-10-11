//! Controlled durable backstops for the fresh-fixture boundary proof.
use super::*;

#[tokio::test]
async fn prior_bounds_lapsed_observation_and_refused_release_preserve_state() {
    let (f, request) = Box::pin(setup()).await;
    let _ledger = ledger(&f, &request).await;
    expire(&f, request.fence.scope).await;
    assert!(support::lapsed(&f, &request).await);
    let before = all_tables(&f).await;
    assert!(
        Box::pin(support::classify_pending(
            &f,
            &request,
            invocation_rows(&f, &request).await,
            (0, 0),
            Ok(RemoteDispatchObservation::Interrupted),
        ))
        .await
        .is_none()
    );
    // The Interrupted observation is injected; exact-owner expiry and both durable
    // backstops are real. Expiring before dispatch would instead fail authorization.
    assert_eq!(all_tables(&f).await, before);
    assert!(
        Box::pin(support::park_observed(&f, &request))
            .await
            .is_none()
    );
    assert_eq!(all_tables(&f).await, before);
    let mut foreign = request.clone();
    foreign.fence.worker = worker();
    assert!(
        !support::lapsed(&f, &foreign).await,
        "expiry cannot excuse a foreign fence"
    );
}

#[tokio::test]
async fn prior_bounds_reservation_unknown_requires_durable_backstop() {
    for committed in [false, true] {
        let (f, request) = Box::pin(setup()).await;
        let _ledger = ledger(&f, &request).await;
        let rows = invocation_rows(&f, &request).await;
        let initial = all_tables(&f).await;
        if committed {
            assert!(matches!(
                adapter(&f, 0).reserve_remote(&request, None).await.unwrap(),
                RemoteReservationResult::Reserved(_)
            ));
        }
        let before = all_tables(&f).await;
        // Inject only the lost acknowledgement at the observer seam. The reservation
        // above, and the refusal to adopt it in the backstop, are real durable calls.
        let unknown = Err(AppError::Conflict(
            "Workflow action.reserve_remote commit outcome unknown".into(),
        ));
        assert!(
            Box::pin(support::classify_pending(
                &f,
                &request,
                rows,
                (0, 0),
                unknown
            ))
            .await
            .is_none()
        );
        assert_eq!(
            all_tables(&f).await,
            before,
            "the backstop itself never mutates history"
        );
        if committed {
            assert_eq!(entry_consumptions(&f).await, (1, 0));
            assert_ne!(before, initial);
            assert!(!retry_safe(&f, &request).await);
        } else {
            assert_eq!(before, initial);
        }
    }
}

struct PausedFixture {
    fixture: AdmissionFixture,
    verifier: Arc<PausedVerifier>,
    command: ReconcileActionCommand,
}

async fn paused_fixture() -> PausedFixture {
    let (f, request) = Box::pin(setup()).await;
    let ledger = ledger(&f, &request).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(
        Box::pin(pending_loss(&f, &request, &provider))
            .await
            .is_some()
    );
    assert!(Box::pin(park_live(&f, &request)).await.is_some());
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &ledger, "controlled-pause").await;
    PausedFixture {
        fixture: f,
        verifier: Arc::new(PausedVerifier::new(ledger)),
        command,
    }
}

#[tokio::test]
async fn prior_bounds_withheld_verifier_budget_writes_nothing() {
    let PausedFixture {
        fixture: f,
        verifier: observed,
        command,
    } = Box::pin(paused_fixture()).await;
    let before = all_tables(&f).await;
    let started = std::time::Instant::now();
    let (result, entered) = tokio::join!(observed.run(&f, &command), observed.wait_started());
    assert!(
        entered,
        "the real ledger was read before the controlled pause"
    );
    support::withheld_timeout(&result, started.elapsed(), None);
    assert!(observed.finished.is_cancelled());
    assert!(!observed.returned.load(Ordering::SeqCst));
    assert_eq!(observed.calls.load(Ordering::SeqCst), 1);
    assert_eq!(all_tables(&f).await, before);
}

#[tokio::test]
async fn prior_bounds_early_command_failure_drains_pause_waiter() {
    let PausedFixture {
        fixture: f,
        verifier: observed,
        mut command,
    } = Box::pin(paused_fixture()).await;
    command.actor = WorkflowActor::authenticated(Uuid::new_v4()).unwrap();
    let before = all_tables(&f).await;
    // The production timeout is the hang guard; completion must wake the waiter
    // even though verify() never reaches its started notification.
    let (result, entered) = tokio::join!(observed.run(&f, &command), observed.wait_started());
    assert!(
        result.is_err() && !matches!(result, Err(AppError::Timeout(_))),
        "strict early error: {result:?}"
    );
    assert!(!entered);
    assert_eq!(observed.calls.load(Ordering::SeqCst), 0);
    assert!(!observed.returned.load(Ordering::SeqCst));
    assert_eq!(all_tables(&f).await, before);
}

#[tokio::test]
async fn prior_bounds_three_void_fixtures_are_not_a_proof() {
    let fixtures = std::sync::Mutex::new(Vec::new());
    let result = Box::pin(support::fresh_proof(|| async {
        let (f, request) = Box::pin(setup()).await;
        let _ledger = ledger(&f, &request).await;
        fixtures.lock().unwrap().push(request.scope().run);
        expire(&f, request.fence.scope).await;
        Box::pin(support::park_observed(&f, &request)).await
    }))
    .await;
    assert_eq!(
        result,
        Err("prior_bounds: all three fresh fixtures were void; boundary unproven")
    );
    let fixtures = fixtures.into_inner().unwrap();
    assert_eq!(fixtures.len(), 3);
    assert!(fixtures[0] != fixtures[1] && fixtures[1] != fixtures[2] && fixtures[0] != fixtures[2]);
}

#[tokio::test]
#[should_panic(expected = "expected Pending provider's own loss")]
async fn prior_bounds_unexpected_dispatch_error_is_not_void() {
    let (f, request) = Box::pin(setup()).await;
    let rows = invocation_rows(&f, &request).await;
    Box::pin(support::classify_pending(
        &f,
        &request,
        rows,
        (0, 0),
        Err(AppError::Timeout("unrelated timeout".into())),
    ))
    .await;
}

#[tokio::test]
#[should_panic(expected = "release refusal without exact-owner expiry")]
async fn prior_bounds_foreign_release_refusal_is_not_void() {
    let (f, mut request) = Box::pin(setup()).await;
    expire(&f, request.fence.scope).await;
    request.fence.worker = worker();
    Box::pin(support::park_observed(&f, &request)).await;
}
