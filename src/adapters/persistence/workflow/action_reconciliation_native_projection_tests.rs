//! Owned-adapter projection: the stored evidence row carries exactly the closed typed diagnostic,
//! and the whole-table delta is exactly the reconciliation commit. Raw provider material cannot be
//! stored because `EvidenceAttestation.diagnostic` is the closed `AttestationDiagnostic`
//! (verification.rs:105-129: private field, sole constructor from `EvidenceProviderId`, custom
//! `Serialize`, no `Deserialize`); this test proves the exact stored projection, not that guarantee.
use super::*;

/// Test-only owned adapter: holds an in-memory raw provider response, delegates to the genuine
/// ledger verifier and projects only the allowlisted `ledger` field into the typed diagnostic.
/// The type admits nothing else, so `raw` has no path to the service.
struct ProjectingVerifier {
    captured: Arc<CapturedVerifier>,
    raw: Value,
    projections: AtomicUsize,
}

#[async_trait]
impl ActionEvidenceVerifier for ProjectingVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.captured.registration()
    }

    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        let attested = self
            .captured
            .verify(snapshot, reference, cancellation)
            .await?;
        let ledger = EvidenceProviderId::parse(self.raw["ledger"].as_str().unwrap()).unwrap();
        let diagnostic = AttestationDiagnostic::new(ledger);
        assert_eq!(
            diagnostic, attested.diagnostic,
            "projection equals delegate"
        );
        self.projections.fetch_add(1, Ordering::SeqCst);
        Ok(EvidenceAttestation {
            diagnostic,
            ..attested
        })
    }
}

/// The stored diagnostic is exactly the closed projection: outer keys, attestation keys and the
/// verifier diagnostic `{version, ledger}`.
fn assert_closed_projection(after: &Value, registration: &EvidenceVerifierRegistration) {
    let evidence = one(after, "workflow_action_evidence");
    let envelope = evidence["diagnostic"].as_object().unwrap();
    let mut outer: Vec<_> = envelope.keys().map(String::as_str).collect();
    outer.sort_unstable();
    assert_eq!(outer, ["attestation", "result"]);
    let mut attestation: Vec<_> = envelope["attestation"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    attestation.sort_unstable();
    assert_eq!(
        attestation,
        [
            "diagnostic",
            "observed_at",
            "provider",
            "reference",
            "registration",
            "valid_until",
            "verified_at",
            "verifier_version",
            "version",
        ]
    );
    assert_eq!(
        envelope["attestation"]["diagnostic"],
        json!({"version": 1, "ledger": registration.provider().as_str()}),
    );
    assert!(envelope["result"].is_null());
    let commands = one(after, "workflow_action_evidence_commands");
    assert_eq!(commands["outcome"], json!({"kind": "unknown_recorded"}));
}

#[tokio::test]
async fn workflow_action_reconciliation_native_projection_stores_only_closed_diagnostic() {
    // Box the real service/fixture seams for stock 2 MiB test stacks.
    let source = Box::pin(unknown_source()).await;
    let registration = source.verifier.registration();
    let raw = json!({
        "ledger": registration.provider().as_str(),
        "body": "raw provider transport body",
        "headers": {"authorization": "Bearer adapter-held-token"},
    });
    let verifier = Arc::new(ProjectingVerifier {
        captured: source.verifier.clone(),
        raw,
        projections: AtomicUsize::new(0),
    });
    let command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.ledger,
        "native-projection-source",
    )
    .await;
    let before = all_tables(&source.fixture).await;
    assert_source(&source, &before);
    let catalog = native_catalog(&source.fixture).await;
    let result = Box::pin(run_command(&source.fixture, &command, verifier.clone()))
        .await
        .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::UnknownRecorded);
    assert!(!result.replayed);
    let after = all_tables(&source.fixture).await;
    assert_evidence(&source, &command, &result, &after);
    assert_closed_projection(&after, source.verifier.registration());
    // Exact whole-table delta (every public table, bytea included): only evidence, command,
    // coverage, one audit row and the run revision.
    assert_commit(&command, &result, &before, &after);
    assert_eq!(verifier.projections.load(Ordering::SeqCst), 1);
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 0);
    assert_eq!(native_catalog(&source.fixture).await, catalog);
    source.fixture.persistence().pool().close().await;
}

/// No installed reconciliation function reads the evidence diagnostic, so an SQL-legal but
/// semantically odd diagnostic cannot become authority (S1 item 4 regress detector).
#[tokio::test]
async fn workflow_action_reconciliation_native_projection_diagnostic_is_not_read() {
    let (fixture, _request) = Box::pin(setup()).await;
    let readers: Vec<String> = sqlx::query_scalar(
        "SELECT p.oid::regprocedure::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace \
         WHERE n.nspname='public' AND (p.prosrc ~ 'workflow_action_evidence' OR p.oid IN \
         (SELECT t.tgfoid FROM pg_trigger t WHERE t.tgrelid::regclass::text ~ '^workflow_action_evidence')) ORDER BY 1",
    )
    .fetch_all(fixture.persistence().pool())
    .await
    .unwrap();
    assert!(!readers.is_empty(), "evidence functions are installed");
    let diagnostic: Vec<String> = sqlx::query_scalar(
        "SELECT p.oid::regprocedure::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace \
         WHERE n.nspname='public' AND (p.prosrc ~ 'workflow_action_evidence' OR p.oid IN \
         (SELECT t.tgfoid FROM pg_trigger t WHERE t.tgrelid::regclass::text ~ '^workflow_action_evidence')) \
         AND p.prosrc ~* 'diagnostic' ORDER BY 1",
    )
    .fetch_all(fixture.persistence().pool())
    .await
    .unwrap();
    assert_eq!(diagnostic, Vec::<String>::new());
    fixture.persistence().pool().close().await;
}
