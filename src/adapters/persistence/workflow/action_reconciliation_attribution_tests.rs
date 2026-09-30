//! Host attribution distinguishes proven old-request breach from unresolved provenance.
use super::*;
use crate::adapters::persistence::workflow::action_uncertainty;

#[path = "action_reconciliation_attribution_assertions.rs"]
mod assertions;
use assertions::{append, committed_conflict, native_catalog, positive_links, rows, subject_scope};

#[derive(Clone, Copy)]
enum Attribution {
    CoveredEntry,
    Unattributed,
    EmptyMarker,
}
impl Attribution {
    fn stored(self) -> &'static str {
        match self {
            Self::CoveredEntry => "remote_entry",
            Self::Unattributed => "unattributed",
            Self::EmptyMarker => "marker_reservation",
        }
    }
    fn reason(self) -> &'static str {
        match self {
            Self::Unattributed => "unattributed_applied",
            Self::CoveredEntry | Self::EmptyMarker => "finality_breach",
        }
    }
}
struct AttributionVerifier {
    ledger: Arc<LedgerVerifier>,
    attribution: Attribution,
    calls: AtomicUsize,
}
#[async_trait]
impl ActionEvidenceVerifier for AttributionVerifier {
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
        let mut value = self
            .ledger
            .verify(snapshot, reference, cancellation)
            .await?;
        match self.attribution {
            Attribution::CoveredEntry | Attribution::Unattributed => {
                let VerifiedDisposition::Applied { request, .. } = &mut value.disposition else {
                    panic!("actual durable provider application");
                };
                assert_eq!(snapshot.entries.len(), 1);
                assert_eq!(
                    *request,
                    AppliedEvidenceRequest::RemoteEntry(snapshot.entries[0].id)
                );
                if matches!(self.attribution, Attribution::Unattributed) {
                    *request = AppliedEvidenceRequest::Unattributed;
                }
            }
            Attribution::EmptyMarker => {
                assert!(snapshot.entries.is_empty());
                assert!(matches!(
                    value.disposition,
                    VerifiedDisposition::FinalNotApplied
                ));
                // Deliberately contradictory trusted host evidence, with ZERO
                // physical effects. No provider application is claimed here.
                value.disposition = VerifiedDisposition::Applied {
                    recovered_result: Some(good()),
                    request: AppliedEvidenceRequest::MarkerReservation,
                };
            }
        }
        Ok(value)
    }
}
struct AttributionOwner {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    ledger: Arc<LedgerVerifier>,
    proof: ActionEvidenceId,
    provider_calls: usize,
    catalog: Value,
}

async fn marker_only(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let before = all_tables(f).await;
    let writer = adapter(f, 0);
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let authorized = Box::pin(writer.authorize_on(&mut tx, request))
        .await
        .unwrap();
    assert!(!authorized.approval_required);
    let provider = writer
        .provider_on(&mut tx, request, &authorized.action, None)
        .await
        .unwrap();
    let mut policy = authorized.current_policy;
    policy["provider_replay"] = provider.proof().unwrap();
    let marker = writer
        .marker_on(&mut tx, request, &authorized.action, policy, &provider)
        .await
        .unwrap()
        .expect("authorized pre-entry marker");
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut after = all_tables(f).await;
    let added = append(&before, &after, "workflow_action_dispatches", 1);
    subject_scope(added[0], request);
    assert_eq!(added[0]["id"], json!(marker));
    assert_eq!(added[0]["effect_kind"], "remote");
    let stored_subject: Vec<u8> =
        sqlx::query_scalar("SELECT replay_subject FROM workflow_action_dispatches WHERE id=$1")
            .bind(marker)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(
        stored_subject,
        crate::application::workflow::actions::replay_subject(
            &authorized.action.request().contract,
            &authorized.action.request().target
        )
        .unwrap()
    );
    after["workflow_action_dispatches"] = before["workflow_action_dispatches"].clone();
    assert_eq!(
        after, before,
        "native marker-only COMMIT preserves every other public row"
    );
    assert_eq!(entry_consumptions(f).await, (0, 0));
    assert_eq!(effects(f).await, 0);
}

fn proof_source(state: &Value, proof: ActionEvidenceId, expected: usize) {
    assert_eq!(
        state["workflow_action_evidence"][0]["id"],
        json!(proof.as_uuid())
    );
    assert_eq!(state["workflow_action_evidence"][0]["grant_eligible"], true);
    assert_eq!(
        rows(state, "workflow_action_evidence_coverage").len(),
        expected
    );
    for coverage in rows(state, "workflow_action_evidence_coverage") {
        assert_eq!(coverage["evidence_id"], json!(proof.as_uuid()));
        assert_eq!(
            coverage["remote_entry_id"],
            state["workflow_action_remote_entries"][0]["id"]
        );
    }
    assert_eq!(state["background_tasks"][0]["status"], "pending");
    assert_eq!(state["workflow_runs"][0]["state"], "running");
    assert!(rows(state, "workflow_action_receipts").is_empty());
}

async fn owner(attribution: Attribution) -> AttributionOwner {
    // Box the established admission/service phases to preserve stock 2 MiB stacks.
    let (f, request) = Box::pin(setup()).await;
    let ledger = ledger(&f, &request).await;
    let catalog = native_catalog(&f).await;
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    match attribution {
        Attribution::EmptyMarker => Box::pin(marker_only(&f, &request)).await,
        Attribution::CoveredEntry | Attribution::Unattributed => {
            assert!(matches!(
                Box::pin(invoke(&f, &request, &provider)).await,
                Err(AppError::Timeout(_))
            ));
            assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
            assert_eq!(effects(&f).await, 1);
        }
    }
    park(&f, &request).await;
    let verifier: Arc<dyn ActionEvidenceVerifier> = match attribution {
        Attribution::EmptyMarker => {
            barrier(&f, &request).await;
            ledger.clone()
        }
        // An erroneous trusted closure must not erase the actual provider effect.
        Attribution::CoveredEntry | Attribution::Unattributed => Arc::new(ForcedVerifier {
            inner: ledger.clone(),
            final_not_applied: true,
            future: false,
        }),
    };
    let result = Box::pin(reconcile_with(
        &f,
        &request,
        marker(&f).await,
        verifier,
        "attribution-final",
    ))
    .await
    .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let proof = result.evidence.unwrap();
    let state = all_tables(&f).await;
    let expected = match attribution {
        Attribution::EmptyMarker => 0,
        _ => 1,
    };
    proof_source(&state, proof, expected);
    assert_eq!(entry_consumptions(&f).await, (expected as i64, 0));
    AttributionOwner {
        fixture: f,
        request,
        ledger,
        proof,
        provider_calls: provider.calls.load(Ordering::SeqCst),
        catalog,
    }
}

async fn veto_and_replay(
    h: &AttributionOwner,
    command: &ReconcileActionCommand,
    verifier: Arc<AttributionVerifier>,
    result: &ReconciliationResult,
) {
    let f = &h.fixture;
    let before = all_tables(f).await;
    assert_eq!(projection(f).await, ("committed".into(), true));
    assert!(!retry_safe(f, &h.request).await);
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert!(
        action_uncertainty::conflicted_on(&mut tx, h.request.scope())
            .await
            .unwrap()
    );
    tx.rollback().await.unwrap();
    let (left, right) = tokio::join!(
        f.persistence()
            .claim_io(h.request.fence.scope, worker(), policy()),
        f.persistence()
            .claim_io(h.request.fence.scope, worker(), policy()),
    );
    assert!(left.unwrap().is_none() && right.unwrap().is_none());
    let provider = LedgerProvider::new(f, Delivery::Pending);
    // This retired-fence refusal is supplemental; it does not isolate the conflict gate.
    assert!(matches!(
        Box::pin(invoke(f, &h.request, &provider)).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    let replayed = Box::pin(run_command(f, command, verifier.clone()))
        .await
        .unwrap();
    assert!(replayed.replayed);
    assert_eq!(replayed.outcome, result.outcome);
    assert_eq!(replayed.evidence, result.evidence);
    assert_eq!(replayed.revision, result.revision);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(all_tables(f).await, before);
    assert_eq!(effects(f).await, h.provider_calls as i64);
}

async fn conflict_case(attribution: Attribution) {
    let h = Box::pin(owner(attribution)).await;
    let before = all_tables(&h.fixture).await;
    let verifier = Arc::new(AttributionVerifier {
        ledger: h.ledger.clone(),
        attribution,
        calls: AtomicUsize::new(0),
    });
    let command = proof_command(&h.fixture, &h.request, &h.ledger, "attribution-applied").await;
    let result = Box::pin(run_command(&h.fixture, &command, verifier.clone()))
        .await
        .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::EvidenceConflict);
    assert!(!result.replayed);
    let after = all_tables(&h.fixture).await;
    committed_conflict(&h, attribution, &before, &after, &command, &result);
    Box::pin(veto_and_replay(&h, &command, verifier, &result)).await;
    assert_eq!(native_catalog(&h.fixture).await, h.catalog);
    h.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_attribution_covered_old_entry_breaches_finality() {
    Box::pin(conflict_case(Attribution::CoveredEntry)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_attribution_unattributed_preserves_unresolved_provenance() {
    Box::pin(conflict_case(Attribution::Unattributed)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_attribution_zero_entry_marker_breaches_finality() {
    Box::pin(conflict_case(Attribution::EmptyMarker)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_attribution_consumed_new_entry_is_legitimate() {
    let (f, mut request) = Box::pin(setup()).await;
    let ledger = ledger(&f, &request).await;
    let catalog = native_catalog(&f).await;
    let old = LedgerProvider::new(&f, Delivery::Pending);
    assert!(matches!(
        Box::pin(invoke(&f, &request, &old)).await,
        Err(AppError::Timeout(_))
    ));
    park(&f, &request).await;
    barrier(&f, &request).await;
    let grant = Box::pin(reconcile(
        &f,
        &request,
        marker(&f).await,
        ledger.clone(),
        "new-entry-final",
    ))
    .await;
    assert_eq!(
        grant.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let granted = all_tables(&f).await;
    new_claim(&f, &mut request).await;
    let new = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    assert!(matches!(
        Box::pin(invoke(&f, &request, &new)).await,
        Err(AppError::Timeout(_))
    ));
    assert_eq!(entry_consumptions(&f).await, (2, 1));
    park(&f, &request).await;
    let before = all_tables(&f).await;
    let recovered = Box::pin(reconcile(
        &f,
        &request,
        marker(&f).await,
        ledger,
        "new-entry-applied",
    ))
    .await;
    assert_eq!(
        recovered.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    );
    let after = all_tables(&f).await;
    positive_links(
        &request,
        &granted,
        &before,
        &after,
        grant.evidence.unwrap(),
        &recovered,
    );
    assert_eq!(projection(&f).await, ("committed".into(), false));
    assert!(retry_safe(&f, &request).await);
    assert_eq!(effects(&f).await, 1);
    assert_eq!(old.calls.load(Ordering::SeqCst), 1);
    assert_eq!(new.calls.load(Ordering::SeqCst), 1);
    Box::pin(receipt_restart(&f, &mut request)).await;
    assert_eq!(native_catalog(&f).await, catalog);
    f.persistence().pool().close().await;
}

async fn receipt_restart(f: &AdmissionFixture, request: &mut ActionDispatchRequest) {
    new_claim(f, request).await;
    let claimed = all_tables(f).await;
    let restart = LedgerProvider::new(f, Delivery::Pending);
    assert!(matches!(
        Box::pin(invoke(f, request, &restart)).await.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(restart.calls.load(Ordering::SeqCst), 0);
    assert_eq!(entry_consumptions(f).await, (2, 1));
    assert_eq!(effects(f).await, 1);
    assert_eq!(
        all_tables(f).await,
        claimed,
        "receipt replay makes no new write or provider call"
    );
}
