//! A genuine final proof reopens work while preserving the other invocation's effect.
use super::super::commit_tests::{
    assert_issued, expected_evidence, expected_identity, observer, snapshot,
};
use super::*;
use std::sync::Mutex;

#[path = "action_reconciliation_commit_final_injected_tests.rs"]
mod injected;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum Case {
    Positive,
    Revision,
    WrongIdentity,
}
impl Case {
    fn mode(self) -> &'static str {
        match self {
            Self::Positive => "final_positive",
            Self::Revision => "final_revision",
            Self::WrongIdentity => "final_wrong_identity",
        }
    }
}
pub(in super::super) struct FinalVerifier {
    inner: Arc<LedgerVerifier>,
    entry: Uuid,
    coverage: crate::domain::workflow::ActionCoverageDigest,
    reference: EvidenceRecordReference,
    pub(in super::super) calls: AtomicUsize,
    pub(in super::super) issued: Mutex<Option<Value>>,
}
#[async_trait]
impl ActionEvidenceVerifier for FinalVerifier {
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
        assert_eq!(reference, &self.reference);
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].id.as_uuid(), self.entry);
        assert_eq!(snapshot.coverage_digest, self.coverage);
        let value = self.inner.verify(snapshot, reference, cancellation).await?;
        assert!(matches!(
            value.disposition,
            VerifiedDisposition::FinalNotApplied
        ));
        assert_eq!(value.authoritative_reference, self.reference);
        *self.issued.lock().unwrap() = Some(json!({
            "observed_at":value.observed_at,"valid_until":value.valid_until,
            "diagnostic":value.diagnostic,"coverage_digest":snapshot.coverage_digest.as_str(),
            "authoritative_reference":value.authoritative_reference.as_str()
        }));
        Ok(value)
    }
}
pub(in super::super) struct Final {
    pub(in super::super) fixture: AdmissionFixture,
    pub(in super::super) verifier: Arc<FinalVerifier>,
    pub(in super::super) command: ReconcileActionCommand,
    pub(in super::super) expected: Value,
}
pub(in super::super) async fn source() -> Final {
    // The genuine sibling/provider/service chain is boxed at its stock-stack seam.
    let (fixture, requests, inner) =
        Box::pin(seed_siblings(&[SiblingTruth::Receipt, SiblingTruth::Final])).await;
    let settled = Box::pin(reconcile(
        &fixture,
        &requests[0],
        scoped_marker(&fixture, &requests[0]).await,
        inner.clone(),
        "final-preserved-receipt",
    ))
    .await;
    assert_eq!(
        settled.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob
        }
    );
    let mut command = command(
        &fixture,
        &requests[1],
        scoped_marker(&fixture, &requests[1]).await,
    )
    .await;
    command.command_key = IdempotencyKey::parse("native-commit-final").unwrap();
    let reference = EvidenceRecordReference::parse(command.command_key.as_str()).unwrap();
    command.input = EvidenceInput::VerifiedReference {
        registration: inner.registration.id().clone(),
        reference: reference.clone(),
    };
    let reader = PostgresActionReconciliation::new(fixture.persistence().clone(), Resources);
    let ReconciliationPreparation::Snapshot(snapshot) = reader.snapshot(&command).await.unwrap()
    else {
        panic!("genuine parked Final snapshot");
    };
    snapshot.validate(&command).unwrap();
    assert_eq!(snapshot.entries.len(), 1);
    let entry = snapshot.entries[0].id.as_uuid();
    assert_source(&fixture, &requests, entry, &settled).await;
    let mut evidence = expected_evidence(&command, &inner);
    evidence.as_object_mut().unwrap().extend(
        json!({"disposition":"final_not_applied","grant_eligible":true,
        "applied_request":null,"applied_remote_entry_id":null})
        .as_object()
        .unwrap()
        .clone(),
    );
    let expected = json!({"command":expected_identity(&command),"evidence":evidence,"entry":entry,"coverage_digest":snapshot.coverage_digest.as_str()});
    Final {
        fixture,
        command,
        expected,
        verifier: Arc::new(FinalVerifier {
            inner,
            entry,
            coverage: snapshot.coverage_digest.clone(),
            reference,
            calls: AtomicUsize::new(0),
            issued: Mutex::new(None),
        }),
    }
}
async fn assert_source(
    f: &AdmissionFixture,
    requests: &[ActionDispatchRequest],
    entry: Uuid,
    settled: &ReconciliationResult,
) {
    let state = super::super::proof_tests::all_tables(f).await;
    let entries = state["workflow_action_remote_entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let ledgers = state["fixture_evidence_ledger"].as_array().unwrap();
    let operations = state["fixture_provider_operations"].as_array().unwrap();
    assert_eq!((ledgers.len(), operations.len()), (2, 2));
    let mut ids = Vec::new();
    for (index, request) in requests.iter().enumerate() {
        let invocation = json!(request.subject.invocation.as_uuid());
        let rows: Vec<_> = entries
            .iter()
            .filter(|row| row["invocation_id"] == invocation)
            .collect();
        assert_eq!(rows.len(), 1);
        let row = rows[0];
        ids.push(row["id"].clone());
        let ledger: Vec<_> = ledgers
            .iter()
            .filter(|ledger| ledger["invocation_id"] == invocation)
            .collect();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0]["entry_id"], row["id"]);
        let operation: Vec<_> = operations
            .iter()
            .filter(|op| op["invocation_id"] == invocation)
            .collect();
        assert_eq!(operation.len(), 1);
        let effects: Vec<_> = state["fixture_provider_effects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|effect| effect["entry_id"] == row["id"])
            .collect();
        if index == 0 {
            assert_eq!(ledger[0]["result"], good());
            assert_eq!(ledger[0]["recover_result"], true);
            assert_eq!(effects.len(), 1);
            assert_eq!(effects[0]["result"], good());
        } else {
            assert_eq!(row["id"], json!(entry));
            assert_eq!(ledger[0]["result"], Value::Null);
            assert_eq!(ledger[0]["final_closed"], true);
            assert_eq!(operation[0]["marker_closed"], true);
            assert!(effects.is_empty());
        }
    }
    assert_ne!(ids[0], ids[1]);
    assert_eq!(
        state["fixture_provider_effects"].as_array().unwrap().len(),
        1
    );
    assert_saved_receipt(&state, &requests[0], settled);
}
fn assert_saved_receipt(
    state: &Value,
    request: &ActionDispatchRequest,
    settled: &ReconciliationResult,
) {
    for table in [
        "workflow_action_receipts",
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
    ] {
        assert_eq!(
            state[table].as_array().unwrap().len(),
            1,
            "genuine Receipt {table}"
        );
    }
    assert_eq!(
        state["workflow_action_receipts"][0]["invocation_id"],
        json!(request.subject.invocation.as_uuid())
    );
    assert_eq!(
        state["workflow_action_receipts"][0]["reconciliation_evidence_id"],
        json!(settled.evidence.unwrap().as_uuid())
    );
    assert_eq!(state["workflow_action_receipts"][0]["result"], good());
    let command = &state["workflow_action_evidence_commands"][0];
    assert_eq!(
        command["evidence_id"],
        json!(settled.evidence.unwrap().as_uuid())
    );
    assert!(
        state["workflow_run_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|audit| audit["sequence"] == command["audit_sequence"]
                && audit["actor_id"] == command["actor_id"]
                && audit["event_kind"] == "action_reconciled")
    );
}
pub(in super::super) async fn install(source: &Final) -> Value {
    let native = observer::catalog(&source.fixture).await;
    sqlx::raw_sql(include_str!("action_reconciliation_commit_fixture.sql"))
        .execute(source.fixture.persistence().pool())
        .await
        .unwrap();
    let shared = observer::catalog(&source.fixture).await;
    observer::assert_installation(&native, &shared);
    sqlx::raw_sql(include_str!(
        "action_reconciliation_commit_final_fixture.sql"
    ))
    .execute(source.fixture.persistence().pool())
    .await
    .unwrap();
    let installed = observer::catalog(&source.fixture).await;
    assert_final_installation(&shared, &installed);
    installed
}
fn assert_final_installation(shared: &Value, installed: &Value) {
    let mut remaining = installed.as_object().unwrap().clone();
    for (name, body) in [
        (
            "fixture_commit_final_before",
            "NEW.result_revision := NEW.expected_revision",
        ),
        (
            "fixture_commit_final",
            "fixture_commit_delta(config,proposed,additions",
        ),
    ] {
        let keys: Vec<_> = remaining
            .iter()
            .filter(|(key, row)| key.starts_with("function:") && row["name"] == name)
            .map(|(key, _)| key.clone())
            .collect();
        assert_eq!(keys.len(), 1);
        let row = remaining.remove(&keys[0]).unwrap();
        assert!(row["definition"].as_str().unwrap().contains(body));
    }
    let trigger = remaining
        .remove("trigger:workflow_action_evidence_commands.fixture_commit_final_before")
        .unwrap();
    assert_eq!(trigger["enabled"], "O");
    assert_eq!(trigger["deferred"], false);
    assert_eq!(trigger["deferrable"], false);
    assert_eq!(
        trigger["definition"],
        "CREATE TRIGGER fixture_commit_final_before BEFORE INSERT ON public.workflow_action_evidence_commands FOR EACH ROW EXECUTE FUNCTION fixture_commit_final_before()"
    );
    let constraint = remaining
        .remove("constraint:fixture_commit_command_initial.fixture_commit_command_initial_pkey")
        .unwrap();
    assert_eq!(
        constraint["definition"],
        "PRIMARY KEY (company_id, command_key)"
    );
    for column in ["company_id", "command_key", "original_new", "returned_new"] {
        let row = remaining.remove(&format!("constraint:fixture_commit_command_initial.fixture_commit_command_initial_{column}_not_null")).unwrap();
        assert_eq!(row["definition"], format!("NOT NULL {column}"));
    }
    assert_eq!(
        Value::Object(remaining),
        *shared,
        "all native and shared definitions unchanged"
    );
}
pub(in super::super) async fn configure(
    source: &Final,
    case: Case,
    gate: &observer::Gate,
) -> Value {
    let baseline = snapshot(&source.fixture).await;
    let mut expected = source.expected.clone();
    if case == Case::WrongIdentity {
        expected["command"]["actor_id"] = json!(Uuid::new_v4());
    }
    sqlx::query("INSERT INTO fixture_commit_config(company_id,command_key,mode,gate_class,gate_key,expected,baseline) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(source.command.scope.company.as_uuid()).bind(source.command.command_key.as_str()).bind(case.mode())
        .bind(gate.class).bind(gate.key).bind(&expected).bind(&baseline).execute(source.fixture.persistence().pool()).await.unwrap();
    snapshot(&source.fixture).await
}
pub(in super::super) fn positive(
    source: &Final,
    after: &Value,
    result: ReconciliationResult,
    owner: &observer::Owner,
) {
    assert!(!result.replayed);
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let capture = &after["fixture_commit_observed"][0];
    assert_eq!(capture["owner_pid"], owner.pid);
    assert_eq!(capture["database_name"], owner.database);
    let mut proposed = capture["proposed"].clone();
    proposed["fixture_commit_observed"] = json!([capture]);
    assert_eq!(
        &proposed, after,
        "exact committed full-public proposal plus capture"
    );
    let candidates: Vec<_> = after["workflow_action_evidence_commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["command_key"] == source.command.command_key.as_str())
        .collect();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["result_revision"], result.revision.0);
    assert!(result.revision.0 > source.command.expected_revision.0);
    assert_eq!(
        candidates[0]["evidence_id"],
        json!(result.evidence.unwrap().as_uuid())
    );
    for table in ["fixture_commit_initial", "fixture_commit_command_initial"] {
        assert_eq!(
            after[table][0]["original_new"],
            after[table][0]["returned_new"]
        );
    }
    assert_issued(
        source.verifier.issued.lock().unwrap().as_ref().unwrap(),
        &after["fixture_commit_initial"][0]["original_new"],
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
                .expect("Final full proposal reached gate"),
        ),
        Case::Revision => {
            assert!(
                observation.unwrap().is_some(),
                "Final full proposal reached gate: {result:?}"
            );
            assert!(
                matches!(result,Err(AppError::Database(ref error)) if error=="error returned from database: invalid explicit workflow retry"),
                "{result:?}"
            );
            assert_eq!(after, before, "native COMMIT rolls back every public row");
        }
        Case::WrongIdentity => {
            assert!(
                observation.unwrap().is_none(),
                "Final identity assertion precedes gate"
            );
            assert!(
                matches!(result,Err(AppError::Database(ref error)) if error=="error returned from database: row3e observer: Final candidate identity"),
                "{result:?}"
            );
            assert_eq!(after, before, "calibration rolls back every public row");
        }
    }
    drop(gate);
    source.fixture.persistence().pool().close().await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_final_positive() {
    Box::pin(exercise(Case::Positive)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_final_native_revision() {
    Box::pin(exercise(Case::Revision)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_final_observer_calibration() {
    Box::pin(exercise(Case::WrongIdentity)).await;
}
