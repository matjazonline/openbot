//! A provider request can outlive a lost response. Final closure and actual
//! application serialize on the provider's durable operation row, not a local lease.
use super::*;

enum Delivery {
    Pending,
    Apply {
        result: Value,
        recover: bool,
        lose: bool,
    },
}
struct LedgerProvider {
    persistence: PostgresPersistence,
    delivery: Delivery,
    calls: AtomicUsize,
}
impl LedgerProvider {
    fn new(f: &AdmissionFixture, delivery: Delivery) -> Self {
        Self {
            persistence: f.persistence().clone(),
            delivery,
            calls: AtomicUsize::new(0),
        }
    }
}
#[async_trait]
impl RemoteAction for LedgerProvider {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, action: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut tx = self.persistence.pool().begin().await?;
        let invocation = sqlx::query_scalar::<_, Uuid>("SELECT id FROM workflow_action_intents WHERE company_id=$1 AND execution_id=$2 AND argument_digest=$3")
            .bind(action.scope().company.as_uuid()).bind(action.scope().execution.as_uuid())
            .bind(action.argument_digest().as_str()).fetch_one(&mut *tx).await?;
        let key: String = sqlx::query_scalar("SELECT provider_key FROM fixture_provider_operations WHERE invocation_id=$1 FOR UPDATE")
            .bind(invocation).fetch_one(&mut *tx).await?;
        assert_eq!(
            key,
            action.idempotency_key().as_str(),
            "frozen logical operation survives retry"
        );
        let entry: Uuid = sqlx::query_scalar("SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2 ORDER BY created_at DESC,id DESC LIMIT 1")
            .bind(action.scope().company.as_uuid()).bind(invocation).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO fixture_evidence_ledger(company_id,invocation_id,entry_id,final_closed,recover_result) VALUES($1,$2,$3,false,false)")
            .bind(action.scope().company.as_uuid()).bind(invocation).bind(entry).execute(&mut *tx).await?;
        if let Delivery::Apply {
            result, recover, ..
        } = &self.delivery
        {
            apply_on(&mut tx, entry, result, *recover).await?;
        }
        tx.commit().await?;
        match &self.delivery {
            Delivery::Pending => Err(AppError::Timeout("provider request remains active".into())),
            Delivery::Apply { lose: true, .. } => Err(AppError::Timeout(
                "response lost after durable effect".into(),
            )),
            Delivery::Apply { result, .. } => Ok(result.clone()),
        }
    }
}

async fn apply_on(
    tx: &mut Transaction<'_, Postgres>,
    entry: Uuid,
    result: &Value,
    recover: bool,
) -> AppResult<bool> {
    let changed = sqlx::query("UPDATE fixture_evidence_ledger SET result=$2,recover_result=$3,observed_at=clock_timestamp() WHERE entry_id=$1 AND NOT final_closed AND result IS NULL")
        .bind(entry).bind(result).bind(recover).execute(&mut **tx).await?.rows_affected();
    if changed == 1 {
        sqlx::query("INSERT INTO fixture_provider_effects(entry_id,result) VALUES($1,$2)")
            .bind(entry)
            .bind(result)
            .execute(&mut **tx)
            .await?;
    }
    Ok(changed == 1)
}
async fn delayed_apply(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    result: &Value,
) -> bool {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query(
        "SELECT invocation_id FROM fixture_provider_operations WHERE invocation_id=$1 FOR UPDATE",
    )
    .bind(request.subject.invocation.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    let entry: Uuid = sqlx::query_scalar("SELECT entry_id FROM fixture_evidence_ledger WHERE invocation_id=$1 ORDER BY observed_at DESC LIMIT 1")
        .bind(request.subject.invocation.as_uuid()).fetch_one(&mut *tx).await.unwrap();
    let applied = apply_on(&mut tx, entry, result, true).await.unwrap();
    tx.commit().await.unwrap();
    applied
}
async fn barrier(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("UPDATE fixture_provider_operations SET marker_closed=true WHERE invocation_id=$1")
        .bind(request.subject.invocation.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    // Materialize requests that crashed before polling too. A provider call with
    // this old entry will then fail its unique insert, forever refusing application.
    sqlx::query("INSERT INTO fixture_evidence_ledger(company_id,invocation_id,entry_id,final_closed,recover_result) SELECT company_id,invocation_id,id,true,false FROM workflow_action_remote_entries WHERE invocation_id=$1 ON CONFLICT(entry_id) DO UPDATE SET final_closed=true,observed_at=clock_timestamp() WHERE fixture_evidence_ledger.result IS NULL")
        .bind(request.subject.invocation.as_uuid()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn marker(f: &AdmissionFixture) -> Uuid {
    sqlx::query_scalar("SELECT id FROM workflow_action_dispatches")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}
async fn invoke(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    provider: &LedgerProvider,
) -> AppResult<RemoteDispatchObservation> {
    ActionService::new(adapter(f, 0))
        .dispatch_remote(request, provider, &CancellationToken::new())
        .await
}
async fn effects(f: &AdmissionFixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM fixture_provider_effects")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}
fn good() -> Value {
    json!({"items":[],"token_count":0})
}

#[tokio::test]
async fn workflow_action_reconciliation_provider_lost_applied_restart_without_io() {
    let (f, mut request) = setup().await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    let observed = invoke(&f, &request, &provider).await;
    assert!(
        matches!(&observed, Err(AppError::Timeout(_))),
        "{:?}",
        observed.err()
    );
    assert_eq!(effects(&f).await, 1);
    assert_eq!(counts(&f).await.0, 0);
    park(&f, &request).await;
    let saved = reconcile(&f, &request, marker(&f).await, verifier, "recover").await;
    assert!(matches!(
        saved.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    ));
    assert_eq!(counts(&f).await.0, 1, "receipt commits before continuation");
    new_claim(&f, &mut request).await;
    let restart = LedgerProvider::new(&f, Delivery::Pending);
    let RemoteDispatchObservation::Committed(receipt) =
        invoke(&f, &request, &restart).await.unwrap()
    else {
        panic!("saved receipt")
    };
    assert_eq!(receipt.result, good());
    assert_eq!(restart.calls.load(Ordering::SeqCst), 0);
    assert_eq!(effects(&f).await, 1);
    assert!(
        f.persistence()
            .complete_io(FencedWorkflowResult {
                fence: request.fence,
                output: receipt.result
            })
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_provider_active_is_unknown_until_actual_effect_or_barrier()
{
    for apply in [true, false] {
        let (f, mut request) = setup().await;
        let verifier = ledger(&f, &request).await;
        let old = LedgerProvider::new(&f, Delivery::Pending);
        let observed = invoke(&f, &request, &old).await;
        assert!(
            matches!(&observed, Err(AppError::Timeout(_))),
            "{:?}",
            observed.err()
        );
        park(&f, &request).await;
        let first = reconcile(
            &f,
            &request,
            marker(&f).await,
            verifier.clone(),
            "absence-now",
        )
        .await;
        assert!(matches!(
            first.outcome,
            ReconciliationOutcome::UnknownRecorded
        ));
        assert!(!retry_safe(&f, &request).await);
        assert_eq!(snapshot(&f).await["jobs"][0]["status"], "failed");
        assert_eq!(snapshot(&f).await["runs"][0]["state"], "waiting");
        assert_eq!(projection(&f).await, ("needs_reconciliation".into(), false));
        assert_eq!(old.calls.load(Ordering::SeqCst), 1);
        if apply {
            assert!(delayed_apply(&f, &request, &good()).await);
            assert!(matches!(
                reconcile(&f, &request, marker(&f).await, verifier, "delayed-effect")
                    .await
                    .outcome,
                ReconciliationOutcome::Scheduled { receipt_only: true }
            ));
            assert_eq!(effects(&f).await, 1);
        } else {
            barrier(&f, &request).await;
            assert!(
                !delayed_apply(&f, &request, &good()).await,
                "barrier permanently prevents covered request application"
            );
            assert!(matches!(
                reconcile(&f, &request, marker(&f).await, verifier, "final-barrier")
                    .await
                    .outcome,
                ReconciliationOutcome::Scheduled {
                    receipt_only: false
                }
            ));
            new_claim(&f, &mut request).await;
            let new = LedgerProvider::new(
                &f,
                Delivery::Apply {
                    result: good(),
                    recover: true,
                    lose: false,
                },
            );
            assert!(matches!(
                invoke(&f, &request, &new).await.unwrap(),
                RemoteDispatchObservation::Committed(_)
            ));
            assert_eq!(new.calls.load(Ordering::SeqCst), 1);
            assert_eq!(effects(&f).await, 1);
        }
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_provider_applied_without_usable_output_never_downgrades() {
    for (result, recover) in [
        (good(), false),
        (json!(42), true),
        (json!({"payload":"x".repeat(65_537)}), true),
    ] {
        let (f, request) = setup().await;
        let verifier = ledger(&f, &request).await;
        let provider = LedgerProvider::new(
            &f,
            Delivery::Apply {
                result,
                recover,
                lose: true,
            },
        );
        let observed = invoke(&f, &request, &provider).await;
        assert!(
            matches!(&observed, Err(AppError::Timeout(_))),
            "{:?}",
            observed.err()
        );
        park(&f, &request).await;
        assert!(matches!(
            reconcile(
                &f,
                &request,
                marker(&f).await,
                verifier.clone(),
                "no-output"
            )
            .await
            .outcome,
            ReconciliationOutcome::AppliedRecorded { receipt: false }
        ));
        assert_eq!(snapshot(&f).await["jobs"][0]["status"], "failed");
        assert_eq!(snapshot(&f).await["runs"][0]["state"], "waiting");
        assert_eq!(counts(&f).await.0, 0);
        assert_eq!(
            projection(&f).await,
            ("applied_without_result".into(), false)
        );
        assert_eq!(effects(&f).await, 1);
        assert!(!retry_safe(&f, &request).await);
        // An intentionally contradictory trusted adapter cannot downgrade retained
        // positive truth merely because no canonical receipt could be created.
        let contradiction = ForcedVerifier {
            inner: verifier,
            final_not_applied: true,
            future: false,
        };
        let outcome = reconcile_with(
            &f,
            &request,
            marker(&f).await,
            Arc::new(contradiction),
            "contradict",
        )
        .await
        .unwrap();
        assert_eq!(outcome.outcome, ReconciliationOutcome::EvidenceConflict);
        assert!(projection(&f).await.1);
        assert!(!retry_safe(&f, &request).await);
    }
}

struct ForcedVerifier {
    inner: Arc<LedgerVerifier>,
    final_not_applied: bool,
    future: bool,
}
#[async_trait]
impl ActionEvidenceVerifier for ForcedVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.inner.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        let mut value = self.inner.verify(snapshot, reference, cancellation).await?;
        if self.final_not_applied {
            value.disposition = VerifiedDisposition::FinalNotApplied;
        }
        if self.future {
            value.observed_at += chrono::Duration::seconds(2);
        }
        Ok(value)
    }
}
async fn reconcile_with(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    marker: Uuid,
    verifier: Arc<dyn ActionEvidenceVerifier>,
    key: &str,
) -> AppResult<ReconciliationResult> {
    let mut command = command(f, request, marker).await;
    command.command_key = IdempotencyKey::parse(key).unwrap();
    command.input = EvidenceInput::VerifiedReference {
        registration: verifier.registration().id().clone(),
        reference: EvidenceRecordReference::parse(key).unwrap(),
    };
    run_command(f, &command, verifier).await
}
async fn run_command(
    f: &AdmissionFixture,
    command: &ReconcileActionCommand,
    verifier: Arc<dyn ActionEvidenceVerifier>,
) -> AppResult<ReconciliationResult> {
    let persistence = f.persistence().clone();
    ActionReconciliationService::new(
        PostgresActionReconciliation::new(persistence.clone(), Resources),
        LifecycleAuthorizer::new(
            persistence.clone(),
            persistence.clone(),
            persistence.clone(),
        ),
        Directory(persistence),
        Some(verifier),
    )
    .reconcile(command, &CancellationToken::new(), Duration::from_secs(5))
    .await
}

#[tokio::test]
async fn workflow_action_reconciliation_live_db_observation_and_future_timestamp_settlement() {
    let (f, request) = setup().await;
    let verifier = ledger(&f, &request).await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker")
    };
    let marker = reservation.marker;
    park(&f, &request).await;
    barrier(&f, &request).await;
    let future = ForcedVerifier {
        inner: verifier.clone(),
        final_not_applied: false,
        future: true,
    };
    assert!(
        reconcile_with(&f, &request, marker, Arc::new(future), "future")
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_action_evidence")
            .fetch_one(f.persistence().pool())
            .await
            .unwrap(),
        0,
        "independent settlement DB clock rejects otherwise bounded trusted future observation"
    );
    let result = reconcile(&f, &request, marker, verifier, "live-clock").await;
    assert!(matches!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    ));
    let live: bool = sqlx::query_scalar("SELECT observed_at>=(SELECT created_at FROM workflow_action_dispatches) AND verified_at>=observed_at FROM workflow_action_evidence")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert!(live);
}

async fn retry_safe(f: &AdmissionFixture, request: &ActionDispatchRequest) -> bool {
    sqlx::query_scalar::<_, Option<bool>>("SELECT workflow_action_retry_safe($1,$2)")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
        .unwrap()
}

#[path = "action_reconciliation_authority_tests.rs"]
mod authority_tests;
