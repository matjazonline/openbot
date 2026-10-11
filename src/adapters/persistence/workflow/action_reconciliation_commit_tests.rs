//! Observe the complete proposal, then let the ordinary service COMMIT decide.
use super::*;
use std::sync::Mutex;

#[path = "action_reconciliation_commit_observer_tests.rs"]
pub(super) mod observer;

#[path = "action_reconciliation_commit_injected_tests.rs"]
pub(super) mod injected;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Positive,
    Chronology,
    WrongIdentity,
}
impl Case {
    fn mode(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Chronology => "chronology",
            Self::WrongIdentity => "wrong_identity",
        }
    }
}

struct CapturedVerifier {
    inner: Arc<LedgerVerifier>,
    entry: Uuid,
    calls: AtomicUsize,
    issued: Mutex<Option<Value>>,
}
#[async_trait]
impl ActionEvidenceVerifier for CapturedVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.inner.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let value = self.inner.verify(snapshot, reference, cancellation).await?;
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].id.as_uuid(), self.entry);
        let VerifiedDisposition::Applied {
            recovered_result,
            request,
        } = &value.disposition
        else {
            panic!("actual provider ledger must attest Applied");
        };
        assert_eq!(recovered_result.as_ref(), Some(&good()));
        assert!(
            matches!(request, AppliedEvidenceRequest::RemoteEntry(id) if id.as_uuid()==self.entry)
        );
        *self.issued.lock().unwrap() = Some(json!({
            "observed_at":value.observed_at,"valid_until":value.valid_until,
            "diagnostic":value.diagnostic,"coverage_digest":snapshot.coverage_digest.as_str(),
            "authoritative_reference":value.authoritative_reference.as_str()
        }));
        Ok(value)
    }
}

struct Applied {
    fixture: AdmissionFixture,
    provider: LedgerProvider,
    verifier: Arc<CapturedVerifier>,
    command: ReconcileActionCommand,
    expected: Value,
}

async fn source() -> Applied {
    // Box genuine service/fixture seams to preserve the stock 2 MiB test stack.
    let (fixture, request) = Box::pin(setup()).await;
    let inner = ledger(&fixture, &request).await;
    let provider = LedgerProvider::new(
        &fixture,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    let lost = Box::pin(invoke(&fixture, &request, &provider)).await;
    assert!(
        matches!(lost, Err(AppError::Timeout(ref message)) if message=="response lost after durable effect"),
        "{:?}",
        lost.err()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&fixture).await, 1);
    assert_eq!(counts(&fixture).await.0, 0);
    park(&fixture, &request).await;
    let mut command = command(&fixture, &request, marker(&fixture).await).await;
    command.command_key = IdempotencyKey::parse("native-commit-applied").unwrap();
    command.input = EvidenceInput::VerifiedReference {
        registration: inner.registration.id().clone(),
        reference: EvidenceRecordReference::parse(command.command_key.as_str()).unwrap(),
    };
    let reader = PostgresActionReconciliation::new(fixture.persistence().clone(), Resources);
    let ReconciliationPreparation::Snapshot(snapshot) = reader.snapshot(&command).await.unwrap()
    else {
        panic!("genuine parked snapshot");
    };
    snapshot.validate(&command).unwrap();
    assert_eq!(snapshot.entries.len(), 1);
    let entry = snapshot.entries[0].id.as_uuid();
    let expected = expected(&command, &inner, entry, snapshot.coverage_digest.as_str());
    Applied {
        fixture,
        provider,
        command,
        expected,
        verifier: Arc::new(CapturedVerifier {
            inner,
            entry,
            calls: AtomicUsize::new(0),
            issued: Mutex::new(None),
        }),
    }
}

pub(super) fn expected_identity(command: &ReconcileActionCommand) -> Value {
    let scope = command.scope;
    json!({"company_id":scope.company.as_uuid(),"run_id":scope.run.as_uuid(),
        "execution_id":scope.execution.as_uuid(),"invocation_id":command.subject.invocation.as_uuid(),
        "argument_digest":command.subject.argument_digest.as_str(),"dispatch_id":command.marker.as_uuid(),
        "actor_id":command.actor.user_id(),"command_key":command.command_key.as_str(),
        "request_digest":command.request_digest().unwrap().as_str()})
}

pub(super) fn expected(
    command: &ReconcileActionCommand,
    verifier: &LedgerVerifier,
    entry: Uuid,
    coverage: &str,
) -> Value {
    let common = expected_identity(command);
    let mut evidence = expected_evidence(command, verifier);
    evidence.as_object_mut().unwrap().extend(
        json!({"disposition":"applied","grant_eligible":false,
        "applied_request":"remote_entry","applied_remote_entry_id":entry})
        .as_object()
        .unwrap()
        .clone(),
    );
    json!({"command":common,"evidence":evidence,"entry":entry,"coverage_digest":coverage,"result":good()})
}

pub(super) fn expected_evidence(
    command: &ReconcileActionCommand,
    verifier: &LedgerVerifier,
) -> Value {
    let mut evidence = expected_identity(command);
    evidence.as_object_mut().unwrap().extend(json!({
        "registration":verifier.registration.id().as_str(),"verifier_version":verifier.registration.version().as_str(),
        "provider":verifier.registration.provider().as_str(),"operation_signature":verifier.registration.operation().as_str(),
        "authoritative_reference":command.command_key.as_str(),
        "diagnostic":{"attestation":{"version":1,"registration":verifier.registration.id().as_str(),
            "verifier_version":verifier.registration.version().as_str(),"provider":verifier.registration.provider().as_str(),
            "reference":command.command_key.as_str(),"diagnostic":AttestationDiagnostic::new(EvidenceProviderId::parse("fixture").unwrap())}}})
        .as_object().unwrap().clone());
    evidence
}

pub(super) async fn snapshot(f: &AdmissionFixture) -> Value {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .unwrap();
    let value = sqlx::query_scalar("SELECT fixture_commit_public()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    value
}

async fn configure(source: &Applied, case: Case, gate: &observer::Gate) -> Value {
    let baseline = snapshot(&source.fixture).await;
    let mut expected = source.expected.clone();
    if case == Case::WrongIdentity {
        expected["command"]["actor_id"] = json!(Uuid::new_v4());
    }
    sqlx::query("INSERT INTO fixture_commit_config(company_id,command_key,mode,gate_class,gate_key,expected,baseline) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(source.command.scope.company.as_uuid()).bind(source.command.command_key.as_str())
        .bind(case.mode()).bind(gate.class).bind(gate.key).bind(&expected).bind(&baseline)
        .execute(source.fixture.persistence().pool()).await.unwrap();
    let actual: Value = sqlx::query_scalar("SELECT expected FROM fixture_commit_config")
        .fetch_one(source.fixture.persistence().pool())
        .await
        .unwrap();
    assert_eq!(actual, expected);
    snapshot(&source.fixture).await
}

pub(super) fn assert_issued(issued: &Value, initial: &Value) {
    for key in ["coverage_digest", "authoritative_reference"] {
        assert_eq!(initial[key], issued[key], "issued {key}");
    }
    assert_eq!(
        initial["diagnostic"]["attestation"]["diagnostic"],
        issued["diagnostic"]
    );
    for key in ["observed_at", "valid_until"] {
        let read = chrono::DateTime::parse_from_rfc3339(initial[key].as_str().unwrap()).unwrap();
        let issued = chrono::DateTime::parse_from_rfc3339(issued[key].as_str().unwrap()).unwrap();
        assert_eq!(read, issued, "issued {key}");
    }
}

fn assert_positive(
    source: &Applied,
    after: &Value,
    result: ReconciliationResult,
    owner: &observer::Owner,
) {
    assert!(
        !result.replayed,
        "fresh authentic command, never saved replay"
    );
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    );
    let capture = &after["fixture_commit_observed"][0];
    assert_eq!(capture["owner_pid"], owner.pid);
    assert_eq!(capture["database_name"], owner.database);
    let mut proposed = capture["proposed"].clone();
    proposed["fixture_commit_observed"] = json!([capture]);
    assert_eq!(
        &proposed, after,
        "exact full-public committed proposal plus capture"
    );
    let command = &after["workflow_action_evidence_commands"][0];
    assert_eq!(command["result_revision"], result.revision.0);
    assert!(result.revision.0 > source.command.expected_revision.0);
    assert_eq!(
        command["evidence_id"],
        json!(result.evidence.unwrap().as_uuid())
    );
    let initial = &after["fixture_commit_initial"][0];
    assert_eq!(initial["original_new"], initial["returned_new"]);
    assert_issued(
        source.verifier.issued.lock().unwrap().as_ref().unwrap(),
        &initial["original_new"],
    );
    assert_eq!(after["workflow_action_receipts"][0]["result"], good());
}

async fn exercise(case: Case) {
    let source = Box::pin(source()).await;
    let native = observer::catalog(&source.fixture).await;
    sqlx::raw_sql(include_str!("action_reconciliation_commit_fixture.sql"))
        .execute(source.fixture.persistence().pool())
        .await
        .unwrap();
    let installed = observer::catalog(&source.fixture).await;
    observer::assert_installation(&native, &installed);
    let mut gate = observer::Gate::hold(&source.fixture).await;
    let before = configure(&source, case, &gate).await;
    let (result, observation) = gate
        .run(&source.fixture, &source.command, source.verifier.clone())
        .await;
    // Gate released and service drained before assertions, including calibration failure.
    let after = snapshot(&source.fixture).await;
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert!(source.verifier.issued.lock().unwrap().is_some());
    assert_eq!(effects(&source.fixture).await, 1);
    assert_eq!(observer::catalog(&source.fixture).await, installed);
    match case {
        Case::Positive => assert_positive(
            &source,
            &after,
            result.unwrap(),
            &observation.unwrap().expect("reached final observer gate"),
        ),
        Case::Chronology => {
            assert!(
                observation.unwrap().is_some(),
                "complete proposal reached final gate: {result:?}"
            );
            assert!(
                matches!(result, Err(AppError::Database(ref error)) if error=="error returned from database: invalid workflow action evidence provenance"),
                "{result:?}"
            );
            assert_eq!(after, before, "native COMMIT rolls back every public row");
        }
        Case::WrongIdentity => {
            assert!(
                observation.unwrap().is_none(),
                "identity assertion precedes gate"
            );
            assert!(
                matches!(result, Err(AppError::Database(ref error)) if error=="error returned from database: row3e observer: candidate identity"),
                "{result:?}"
            );
            assert_eq!(after, before, "calibration rolls back every public row");
        }
    }
    drop(gate);
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_commit_applied_positive() {
    Box::pin(exercise(Case::Positive)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_applied_native_chronology() {
    Box::pin(exercise(Case::Chronology)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_applied_observer_calibration() {
    Box::pin(exercise(Case::WrongIdentity)).await;
}
