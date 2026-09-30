//! Later verified observations retain stronger truth and its canonical provenance.
use super::*;

#[path = "action_reconciliation_retained_truth_assertions.rs"]
mod retained_assertions;
use retained_assertions::{TruthChange, truth_boundary};

enum LaterObservation {
    DifferentValidOutput,
    Unknown,
}
struct LaterVerifier {
    ledger: Arc<LedgerVerifier>,
    observation: LaterObservation,
    calls: AtomicUsize,
}
#[async_trait]
impl ActionEvidenceVerifier for LaterVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.ledger.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut attestation = self
            .ledger
            .verify(snapshot, reference, cancellation)
            .await?;
        match self.observation {
            LaterObservation::DifferentValidOutput => {
                let VerifiedDisposition::Applied {
                    recovered_result, ..
                } = &mut attestation.disposition
                else {
                    return Err(invalid());
                };
                assert_eq!(*recovered_result, Some(good()));
                // An inconsistent trusted adapter reports a second schema-valid
                // result; the underlying entered effect and ledger stay immutable.
                *recovered_result = Some(json!({"items":[],"token_count":1}));
            }
            LaterObservation::Unknown => {
                // A later unavailable answer cannot erase earlier durable truth.
                attestation.disposition = VerifiedDisposition::Unknown;
            }
        }
        Ok(attestation)
    }
}

async fn scoped_projection(h: &MixedTruth, index: usize) -> (String, bool) {
    sqlx::query_as("SELECT effect_state,evidence_conflict FROM workflow_action_effect_states WHERE company_id=$1 AND invocation_id=$2")
        .bind(h.requests[index].scope().company.as_uuid())
        .bind(h.requests[index].subject.invocation.as_uuid())
        .fetch_one(h.fixture.persistence().pool()).await.unwrap()
}

async fn replay_observation(
    h: &MixedTruth,
    command: &ReconcileActionCommand,
    verifier: Arc<LaterVerifier>,
    saved: &ReconciliationResult,
) {
    let before = all_tables(&h.fixture).await;
    let replay = Box::pin(run_command(&h.fixture, command, verifier.clone()))
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.outcome, saved.outcome);
    assert_eq!(replay.revision, saved.revision);
    assert_eq!(replay.evidence, saved.evidence);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(all_tables(&h.fixture).await, before);
}

async fn refused_continuation(h: &MixedTruth, effect_count: i64) {
    Box::pin(live_recovery(h, false)).await;
    let live = all_tables(&h.fixture).await;
    park(&h.fixture, &h.requests[0]).await;
    let before = all_tables(&h.fixture).await;
    parked_fence(&live, &before, &h.requests[0]);
    Box::pin(refused_probes(h, &before, effect_count)).await;
}

async fn refused_probes(h: &MixedTruth, before: &Value, effect_count: i64) {
    let (left, right) = tokio::join!(
        h.fixture
            .persistence()
            .claim_io(h.requests[0].fence.scope, worker(), policy()),
        h.fixture
            .persistence()
            .claim_io(h.requests[0].fence.scope, worker(), policy())
    );
    assert!(left.unwrap().is_none() && right.unwrap().is_none());
    let service = ActionService::new(mixed_writer(&h.fixture));
    assert!(matches!(
        Box::pin(service.dispatch_remote(&h.requests[2], &h.supported, &CancellationToken::new()))
            .await,
        Err(AppError::Conflict(_))
    ));
    assert!(
        h.fixture
            .persistence()
            .complete_io(FencedWorkflowResult {
                fence: h.requests[0].fence,
                output: good(),
            })
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        all_tables(&h.fixture).await,
        *before,
        "no attempt, accounting, entry, provider poll, output, route or successor"
    );
    assert_eq!(h.supported.inner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.first.calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.second.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&h.fixture).await, effect_count);
    assert_eq!(entry_consumptions(&h.fixture).await, (3, 0));
}

async fn verified_conflict() {
    let h = Box::pin(seeded(UnusableResult::Missing)).await;
    Box::pin(record_live(&h)).await;
    let before = all_tables(&h.fixture).await;
    let canonical = rows(&before, "workflow_action_receipts")[0].clone();
    let prior = ActionEvidenceId::new(
        canonical["reconciliation_evidence_id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
    );
    let later = Arc::new(LaterVerifier {
        ledger: h.ledger.clone(),
        observation: LaterObservation::DifferentValidOutput,
        calls: AtomicUsize::new(0),
    });
    let command = evidence_command(&h, 0, "verified-valid-output-disagreement").await;
    let saved = Box::pin(run_command(&h.fixture, &command, later.clone()))
        .await
        .unwrap();
    assert_eq!(saved.outcome, ReconciliationOutcome::EvidenceConflict);
    let after = all_tables(&h.fixture).await;
    truth_boundary(
        &before,
        &after,
        &command,
        &saved,
        TruthChange {
            disposition: "applied",
            recovered: Some(json!({"items":[],"token_count":1})),
            receipt: false,
            contradiction: Some(prior),
        },
    );
    assert_eq!(rows(&after, "workflow_action_receipts"), &[canonical]);
    let fact = by_id(
        &after,
        "workflow_action_evidence",
        saved.evidence.unwrap().as_uuid(),
    );
    assert!(
        fact["diagnostic"]["result"].is_null(),
        "second output passed frozen result validation"
    );
    assert_eq!(fact["applied_request"], "remote_entry");
    assert_eq!(scoped_projection(&h, 0).await, ("committed".into(), true));
    Box::pin(replay_observation(&h, &command, later, &saved)).await;
    // Recover only availability of the already applied sibling result. With both
    // Reconcile siblings receipted and SafeRepeat supported, conflict alone vetoes.
    let before_retrieval = all_tables(&h.fixture).await;
    sqlx::query("UPDATE fixture_evidence_ledger SET recover_result=true,observed_at=clock_timestamp() WHERE invocation_id=$1")
        .bind(h.requests[1].subject.invocation.as_uuid()).execute(h.fixture.persistence().pool()).await.unwrap();
    let available = all_tables(&h.fixture).await;
    retrieval_boundary(&before_retrieval, &available, &h.requests[1]);
    let command = evidence_command(&h, 1, "genuine-sibling-output-after-conflict").await;
    let saved = Box::pin(run_command(&h.fixture, &command, h.ledger.clone()))
        .await
        .unwrap();
    assert_eq!(saved.outcome, ReconciliationOutcome::EvidenceConflict);
    let after = all_tables(&h.fixture).await;
    truth_boundary(
        &available,
        &after,
        &command,
        &saved,
        TruthChange {
            disposition: "applied",
            recovered: Some(good()),
            receipt: true,
            contradiction: None,
        },
    );
    assert_eq!(rows(&after, "workflow_action_receipts").len(), 2);
    Box::pin(refused_continuation(&h, 2)).await;
}

async fn unknown_after_positive(index: usize) {
    let h = Box::pin(seeded(UnusableResult::Missing)).await;
    Box::pin(record_live(&h)).await;
    let projection = if index == 0 {
        "committed"
    } else {
        "applied_without_result"
    };
    let before = all_tables(&h.fixture).await;
    let later = Arc::new(LaterVerifier {
        ledger: h.ledger.clone(),
        observation: LaterObservation::Unknown,
        calls: AtomicUsize::new(0),
    });
    let command = evidence_command(&h, index, "verified-unknown-after-positive").await;
    let saved = Box::pin(run_command(&h.fixture, &command, later.clone()))
        .await
        .unwrap();
    assert_eq!(saved.outcome, ReconciliationOutcome::UnknownRecorded);
    let after = all_tables(&h.fixture).await;
    truth_boundary(
        &before,
        &after,
        &command,
        &saved,
        TruthChange {
            disposition: "unknown",
            recovered: None,
            receipt: false,
            contradiction: None,
        },
    );
    assert_eq!(
        scoped_projection(&h, index).await,
        (projection.into(), false)
    );
    assert_eq!(
        before["workflow_action_receipts"],
        after["workflow_action_receipts"]
    );
    Box::pin(replay_observation(&h, &command, later, &saved)).await;
    Box::pin(refused_continuation(&h, 2)).await;
}

async fn seeded_final() -> MixedTruth {
    let (fixture, first_request) = Box::pin(setup()).await;
    let ledger = ledger(&fixture, &first_request).await;
    let requests = mixed_requests(&fixture, &first_request).await;
    let first = LedgerProvider::new(&fixture, Delivery::Pending);
    let second = LedgerProvider::new(
        &fixture,
        Delivery::Apply {
            result: good(),
            recover: false,
            lose: true,
        },
    );
    let supported = RegisteredLedger::new(&fixture, &requests[2], Delivery::Pending).await;
    let service = ActionService::new(mixed_writer(&fixture));
    for (request, provider) in [(&requests[0], &first), (&requests[1], &second)] {
        assert!(matches!(
            Box::pin(service.dispatch_remote(request, provider, &CancellationToken::new())).await,
            Err(AppError::Timeout(_))
        ));
    }
    assert!(matches!(
        Box::pin(service.dispatch_remote(&requests[2], &supported, &CancellationToken::new()))
            .await,
        Err(AppError::Timeout(_))
    ));
    barrier(&fixture, &requests[0]).await;
    MixedTruth {
        fixture,
        requests,
        ledger,
        first,
        second,
        supported,
        unusable: UnusableResult::Missing,
    }
}

async fn available_proof(h: &MixedTruth) -> Option<Uuid> {
    sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2)")
        .bind(h.requests[0].scope().company.as_uuid())
        .bind(h.requests[0].subject.invocation.as_uuid())
        .fetch_one(h.fixture.persistence().pool())
        .await
        .unwrap()
}

async fn unknown_after_final() {
    let h = Box::pin(seeded_final()).await;
    Box::pin(live_recovery(&h, false)).await;
    let live = all_tables(&h.fixture).await;
    park(&h.fixture, &h.requests[0]).await;
    let parked = all_tables(&h.fixture).await;
    parked_fence(&live, &parked, &h.requests[0]);
    let command = evidence_command(&h, 0, "genuine-final-before-unknown").await;
    let before = all_tables(&h.fixture).await;
    let saved = Box::pin(run_command(&h.fixture, &command, h.ledger.clone()))
        .await
        .unwrap();
    assert_eq!(
        saved.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob,
        }
    );
    let final_state = all_tables(&h.fixture).await;
    truth_boundary(
        &before,
        &final_state,
        &command,
        &saved,
        TruthChange {
            disposition: "final_not_applied",
            recovered: None,
            receipt: false,
            contradiction: None,
        },
    );
    let proof = saved.evidence.unwrap();
    assert_eq!(available_proof(&h).await, Some(proof.as_uuid()));
    assert_eq!(
        scoped_projection(&h, 0).await,
        ("not_applied".into(), false)
    );
    let later = Arc::new(LaterVerifier {
        ledger: h.ledger.clone(),
        observation: LaterObservation::Unknown,
        calls: AtomicUsize::new(0),
    });
    let command = evidence_command(&h, 0, "verified-unknown-after-final").await;
    let saved = Box::pin(run_command(&h.fixture, &command, later.clone()))
        .await
        .unwrap();
    assert_eq!(saved.outcome, ReconciliationOutcome::UnknownRecorded);
    let after = all_tables(&h.fixture).await;
    truth_boundary(
        &final_state,
        &after,
        &command,
        &saved,
        TruthChange {
            disposition: "unknown",
            recovered: None,
            receipt: false,
            contradiction: None,
        },
    );
    assert_eq!(available_proof(&h).await, Some(proof.as_uuid()));
    assert_eq!(
        scoped_projection(&h, 0).await,
        ("not_applied".into(), false)
    );
    assert_eq!(rows(&after, "workflow_action_receipts").len(), 0);
    Box::pin(replay_observation(&h, &command, later, &saved)).await;
    Box::pin(refused_probes(&h, &after, 1)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_retained_verified_valid_output_conflict() {
    Box::pin(verified_conflict()).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_retained_unknown_after_receipt() {
    Box::pin(unknown_after_positive(0)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_retained_unknown_after_applied_without_result() {
    Box::pin(unknown_after_positive(1)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_retained_unknown_after_final_proof() {
    Box::pin(unknown_after_final()).await;
}
