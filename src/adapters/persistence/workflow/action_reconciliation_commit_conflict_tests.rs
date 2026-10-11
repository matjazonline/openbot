//! A real consumed grant produces new truth whose explicit host attribution is unresolved.
use super::super::commit_tests::{
    assert_issued, expected_evidence, expected_identity, observer, snapshot,
};
use super::*;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Positive,
    Chronology,
    WrongIdentity,
}
impl Case {
    fn mode(self) -> &'static str {
        match self {
            Self::Positive => "conflict_positive",
            Self::Chronology => "conflict_chronology",
            Self::WrongIdentity => "conflict_wrong_identity",
        }
    }
}
struct ConflictVerifier {
    inner: Arc<LedgerVerifier>,
    old_entry: Uuid,
    entry: Uuid,
    calls: AtomicUsize,
    issued: Mutex<Option<Value>>,
}
impl ConflictVerifier {
    fn new(inner: Arc<LedgerVerifier>, old_entry: Uuid, entry: Uuid) -> Self {
        Self {
            inner,
            old_entry,
            entry,
            calls: AtomicUsize::new(0),
            issued: Mutex::new(None),
        }
    }
}
#[async_trait]
impl ActionEvidenceVerifier for ConflictVerifier {
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
        let mut value = self.inner.verify(snapshot, reference, cancellation).await?;
        assert_eq!(snapshot.entries.len(), 2);
        let mut entries: Vec<_> = snapshot.entries.iter().map(|e| e.id.as_uuid()).collect();
        entries.sort();
        let mut expected = vec![self.old_entry, self.entry];
        expected.sort();
        assert_eq!(entries, expected);
        let VerifiedDisposition::Applied {
            recovered_result,
            request,
        } = &mut value.disposition
        else {
            panic!("actual durable provider application on new entry");
        };
        assert_eq!(recovered_result.as_ref(), Some(&good()));
        assert_eq!(
            *request,
            AppliedEvidenceRequest::RemoteEntry(ActionRemoteEntryId::new(self.entry))
        );
        // Only trusted-host attribution changes; provider result and coverage stay genuine.
        *request = AppliedEvidenceRequest::Unattributed;
        *self.issued.lock().unwrap() = Some(
            json!({"observed_at":value.observed_at,"valid_until":value.valid_until,
            "diagnostic":value.diagnostic,"coverage_digest":snapshot.coverage_digest.as_str(),
            "authoritative_reference":value.authoritative_reference.as_str()}),
        );
        Ok(value)
    }
}
struct ConflictSource {
    fixture: AdmissionFixture,
    old: LedgerProvider,
    new: LedgerProvider,
    command: ReconcileActionCommand,
    expected: Value,
    verifier: Arc<ConflictVerifier>,
}
async fn source() -> ConflictSource {
    // Box genuine provider and service seams for the stock 2 MiB stack.
    let (fixture, mut request) = Box::pin(setup()).await;
    let inner = ledger(&fixture, &request).await;
    let old = LedgerProvider::new(&fixture, Delivery::Pending);
    assert!(matches!(
        Box::pin(invoke(&fixture, &request, &old)).await,
        Err(AppError::Timeout(_))
    ));
    park(&fixture, &request).await;
    barrier(&fixture, &request).await;
    let grant = Box::pin(reconcile(
        &fixture,
        &request,
        marker(&fixture).await,
        inner.clone(),
        "conflict-genuine-final",
    ))
    .await;
    assert_eq!(
        grant.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let granted = super::proof_tests::all_tables(&fixture).await;
    let proof = grant.evidence.unwrap().as_uuid();
    new_claim(&fixture, &mut request).await;
    let new = LedgerProvider::new(
        &fixture,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    let lost = Box::pin(invoke(&fixture, &request, &new)).await;
    assert!(
        matches!(lost,Err(AppError::Timeout(ref message)) if message=="response lost after durable effect"),
        "{:?}",
        lost.err()
    );
    park(&fixture, &request).await;
    let state = super::proof_tests::all_tables(&fixture).await;
    let old_entry = entry_id(&granted["workflow_action_remote_entries"][0]);
    let entry = assert_source(&state, &granted, proof, old_entry);
    assert_eq!(old.calls.load(Ordering::SeqCst), 1);
    assert_eq!(new.calls.load(Ordering::SeqCst), 1);
    let mut command = command(&fixture, &request, marker(&fixture).await).await;
    command.command_key = IdempotencyKey::parse("native-commit-conflict").unwrap();
    command.input = EvidenceInput::VerifiedReference {
        registration: inner.registration.id().clone(),
        reference: EvidenceRecordReference::parse(command.command_key.as_str()).unwrap(),
    };
    let reader = PostgresActionReconciliation::new(fixture.persistence().clone(), Resources);
    let ReconciliationPreparation::Snapshot(snapshot) = reader.snapshot(&command).await.unwrap()
    else {
        panic!("parked consumed snapshot")
    };
    snapshot.validate(&command).unwrap();
    assert_eq!(snapshot.entries.len(), 2);
    let expected = conflict_expected(
        &command,
        &inner,
        &snapshot,
        json!({"entry":entry,
        "old_entry":old_entry,"proof":proof,"job":request.fence.scope.job.0}),
    );
    ConflictSource {
        fixture,
        old,
        new,
        command,
        expected,
        verifier: Arc::new(ConflictVerifier::new(inner, old_entry, entry)),
    }
}
fn conflict_expected(
    command: &ReconcileActionCommand,
    inner: &LedgerVerifier,
    snapshot: &ReconciliationSnapshot,
    mut source: Value,
) -> Value {
    let mut evidence = expected_evidence(command, inner);
    evidence.as_object_mut().unwrap().extend(
        json!({"disposition":"applied","grant_eligible":false,
        "applied_request":"unattributed","applied_remote_entry_id":null})
        .as_object()
        .unwrap()
        .clone(),
    );
    source.as_object_mut().unwrap().extend(
        json!({"command":expected_identity(command),"evidence":evidence,
        "coverage_digest":snapshot.coverage_digest.as_str(),"result":good()})
        .as_object()
        .unwrap()
        .clone(),
    );
    source
}

fn entry_id(row: &Value) -> Uuid {
    Uuid::parse_str(row["id"].as_str().unwrap()).unwrap()
}
fn assert_source(state: &Value, granted: &Value, proof: Uuid, old: Uuid) -> Uuid {
    let entries = state["workflow_action_remote_entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let new = entries.iter().find(|e| e["id"] != json!(old)).unwrap();
    let entry = entry_id(new);
    assert_eq!(
        entries.iter().find(|e| e["id"] == json!(old)).unwrap(),
        &granted["workflow_action_remote_entries"][0],
        "old entry preserved"
    );
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_commands",
    ] {
        assert_eq!(state[table], granted[table], "old Final history {table}");
    }
    assert_eq!(
        state["workflow_action_evidence"].as_array().unwrap().len(),
        1
    );
    assert_eq!(state["workflow_action_evidence"][0]["id"], json!(proof));
    assert_eq!(
        state["workflow_action_evidence"][0]["disposition"],
        "final_not_applied"
    );
    let coverage = state["workflow_action_evidence_coverage"]
        .as_array()
        .unwrap();
    assert_eq!(coverage.len(), 1);
    assert_eq!(coverage[0]["evidence_id"], json!(proof));
    assert_eq!(coverage[0]["remote_entry_id"], json!(old));
    let consumed = state["workflow_action_evidence_consumptions"]
        .as_array()
        .unwrap();
    assert_eq!(consumed.len(), 1);
    assert_eq!(consumed[0]["evidence_id"], json!(proof));
    assert_eq!(consumed[0]["remote_entry_id"], json!(entry));
    let ledger = state["fixture_evidence_ledger"].as_array().unwrap();
    assert_eq!(ledger.len(), 2);
    let old_ledger = ledger.iter().find(|l| l["entry_id"] == json!(old)).unwrap();
    assert_eq!(old_ledger["final_closed"], true);
    assert!(old_ledger["result"].is_null());
    let new_ledger = ledger
        .iter()
        .find(|l| l["entry_id"] == json!(entry))
        .unwrap();
    assert_eq!(new_ledger["recover_result"], true);
    assert_eq!(new_ledger["result"], good());
    assert_eq!(
        state["fixture_provider_effects"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        state["fixture_provider_effects"][0]["entry_id"],
        json!(entry)
    );
    assert_eq!(state["fixture_provider_effects"][0]["result"], good());
    assert_eq!(state["workflow_action_receipts"], json!([]));
    assert_eq!(state["workflow_action_evidence_conflicts"], json!([]));
    assert_eq!(state["workflow_runs"][0]["state"], "waiting");
    assert_eq!(
        state["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );
    assert_eq!(state["background_tasks"][0]["status"], "failed");
    assert_eq!(state["background_tasks"][0]["retry_count"], 2);
    assert_eq!(state["task_attempts"].as_array().unwrap().len(), 2);
    assert!(
        state["task_attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| !a["finished_at"].is_null())
    );
    entry
}
async fn install(source: &ConflictSource) -> Value {
    let native = observer::catalog(&source.fixture).await;
    sqlx::raw_sql(include_str!("action_reconciliation_commit_fixture.sql"))
        .execute(source.fixture.persistence().pool())
        .await
        .unwrap();
    let shared = observer::catalog(&source.fixture).await;
    observer::assert_installation(&native, &shared);
    sqlx::raw_sql(include_str!(
        "action_reconciliation_commit_conflict_fixture.sql"
    ))
    .execute(source.fixture.persistence().pool())
    .await
    .unwrap();
    let installed = observer::catalog(&source.fixture).await;
    let mut remaining = installed.as_object().unwrap().clone();
    let keys: Vec<_> = remaining
        .iter()
        .filter(|(key, row)| {
            key.starts_with("function:") && row["name"] == "fixture_commit_conflict"
        })
        .map(|(key, _)| key.clone())
        .collect();
    assert_eq!(keys.len(), 1);
    let row = remaining.remove(&keys[0]).unwrap();
    let body = row["definition"].as_str().unwrap();
    assert!(body.contains("fixture_commit_conflict(config fixture_commit_config, candidate workflow_action_evidence_commands)"));
    assert!(body.contains("fixture_commit_delta(config,proposed,additions"));
    assert!(body.contains("'exact new conflict'"));
    assert_eq!(
        Value::Object(remaining),
        shared,
        "all native/shared definitions unchanged"
    );
    installed
}
async fn configure(source: &ConflictSource, case: Case, gate: &observer::Gate) -> Value {
    let baseline = snapshot(&source.fixture).await;
    let mut expected = source.expected.clone();
    if case == Case::WrongIdentity {
        expected["command"]["actor_id"] = json!(Uuid::new_v4());
    }
    sqlx::query("INSERT INTO fixture_commit_config(company_id,command_key,mode,gate_class,gate_key,expected,baseline) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(source.command.scope.company.as_uuid()).bind(source.command.command_key.as_str()).bind(case.mode())
        .bind(gate.class).bind(gate.key).bind(expected).bind(baseline).execute(source.fixture.persistence().pool()).await.unwrap();
    snapshot(&source.fixture).await
}
fn positive(
    source: &ConflictSource,
    after: &Value,
    result: ReconciliationResult,
    owner: &observer::Owner,
) {
    assert!(!result.replayed);
    assert_eq!(result.outcome, ReconciliationOutcome::EvidenceConflict);
    let capture = &after["fixture_commit_observed"][0];
    assert_eq!(capture["owner_pid"], owner.pid);
    assert_eq!(capture["database_name"], owner.database);
    let mut proposed = capture["proposed"].clone();
    proposed["fixture_commit_observed"] = json!([capture]);
    assert_eq!(
        &proposed, after,
        "complete committed public delta including captures"
    );
    let command = after["workflow_action_evidence_commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["command_key"] == source.command.command_key.as_str())
        .unwrap();
    assert_eq!(command["result_revision"], result.revision.0);
    assert_eq!(result.revision.0, source.command.expected_revision.0 + 2);
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
    assert_eq!(
        after["workflow_action_evidence_conflicts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
async fn exercise(case: Case) {
    let source = Box::pin(source()).await;
    let installed = install(&source).await;
    let mut gate = observer::Gate::hold(&source.fixture).await;
    let before = configure(&source, case, &gate).await;
    let (result, observation) = gate
        .run(&source.fixture, &source.command, source.verifier.clone())
        .await;
    let after = snapshot(&source.fixture).await;
    assert_eq!(source.old.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.new.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert!(source.verifier.issued.lock().unwrap().is_some());
    assert_eq!(effects(&source.fixture).await, 1);
    assert_eq!(observer::catalog(&source.fixture).await, installed);
    match case {
        Case::Positive => positive(
            &source,
            &after,
            result.unwrap(),
            &observation
                .unwrap()
                .expect("complete C proposal reached gate"),
        ),
        Case::Chronology => {
            assert!(
                observation.unwrap().is_some(),
                "complete new conflict proposal reached gate: {result:?}"
            );
            assert!(
                matches!(result,Err(AppError::Database(ref error)) if error=="error returned from database: invalid workflow action evidence provenance"),
                "{result:?}"
            );
            assert_eq!(
                after, before,
                "native COMMIT rolls back new conflict and every public row"
            );
        }
        Case::WrongIdentity => {
            assert!(
                observation.unwrap().is_none(),
                "candidate identity assertion precedes gate"
            );
            assert!(
                matches!(result,Err(AppError::Database(ref error)) if error=="error returned from database: row3e observer: candidate identity"),
                "{result:?}"
            );
            assert_eq!(after, before, "calibration rolls back every public row");
        }
    }
    drop(gate);
    source.fixture.persistence().pool().close().await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_conflict_positive() {
    Box::pin(exercise(Case::Positive)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_conflict_native_chronology() {
    Box::pin(exercise(Case::Chronology)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_conflict_observer_calibration() {
    Box::pin(exercise(Case::WrongIdentity)).await;
}
