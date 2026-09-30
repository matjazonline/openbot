//! Genuine registered Unknown source for prospective native evidence checks.
use super::*;
use std::sync::Mutex;

#[path = "action_reconciliation_native_diagnostic_tests.rs"]
mod native_diagnostic_tests;

struct CapturedVerifier {
    ledger: Arc<LedgerVerifier>,
    calls: AtomicUsize,
    observation: Mutex<Option<Value>>,
}

#[async_trait]
impl ActionEvidenceVerifier for CapturedVerifier {
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
        assert!(self.registration().matches_snapshot(snapshot)?);
        let attested = self
            .ledger
            .verify(snapshot, reference, cancellation)
            .await?;
        assert!(matches!(attested.disposition, VerifiedDisposition::Unknown));
        assert_eq!(snapshot.entries.len(), 1);
        let observed = json!({
            "coverage_digest": snapshot.coverage_digest.as_str(),
            "marker": snapshot.marker.as_uuid(),
            "entry": snapshot.entries[0].id.as_uuid(),
            "reference": attested.authoritative_reference.as_str(),
            "observed_at": attested.observed_at,
            "valid_until": attested.valid_until,
            "diagnostic": attested.diagnostic,
        });
        assert!(self.observation.lock().unwrap().replace(observed).is_none());
        Ok(attested)
    }
}

struct UnknownSource {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    provider: LedgerProvider,
    verifier: Arc<CapturedVerifier>,
}

async fn unknown_source() -> UnknownSource {
    // Box real fixture/provider seams to retain stock 2 MiB test stacks.
    let (fixture, request) = Box::pin(setup()).await;
    resources(&fixture).await;
    let ledger = ledger(&fixture, &request).await;
    let provider = LedgerProvider::new(&fixture, Delivery::Pending);
    let error = Box::pin(invoke(&fixture, &request, &provider))
        .await
        .err()
        .expect("actual pending provider error");
    assert!(
        matches!(error, AppError::Timeout(ref message) if message == "provider request remains active")
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    park(&fixture, &request).await;
    UnknownSource {
        fixture,
        request,
        provider,
        verifier: Arc::new(CapturedVerifier {
            ledger,
            calls: AtomicUsize::new(0),
            observation: Mutex::new(None),
        }),
    }
}

fn one<'a>(state: &'a Value, relation: &str) -> &'a Value {
    let rows = state[relation].as_array().unwrap();
    assert_eq!(rows.len(), 1, "one exact {relation} owner");
    &rows[0]
}

fn instant(value: &Value) -> chrono::DateTime<chrono::Utc> {
    value.as_str().unwrap().parse().unwrap()
}

fn assert_source(source: &UnknownSource, before: &Value) {
    let request = &source.request;
    let marker = one(before, "workflow_action_dispatches");
    let entry = one(before, "workflow_action_remote_entries");
    let ledger = one(before, "fixture_evidence_ledger");
    let operation = one(before, "fixture_provider_operations");
    for row in [marker, entry] {
        assert_eq!(row["company_id"], json!(request.scope().company.as_uuid()));
        assert_eq!(row["run_id"], json!(request.scope().run.as_uuid()));
        assert_eq!(
            row["execution_id"],
            json!(request.scope().execution.as_uuid())
        );
        assert_eq!(
            row["invocation_id"],
            json!(request.subject.invocation.as_uuid())
        );
        assert_eq!(
            row["argument_digest"],
            request.subject.argument_digest.as_str()
        );
    }
    assert_eq!(entry["dispatch_id"], marker["id"]);
    assert!(instant(&marker["created_at"]) < instant(&entry["created_at"]));
    assert_eq!(ledger["company_id"], marker["company_id"]);
    assert_eq!(ledger["invocation_id"], marker["invocation_id"]);
    assert_eq!(ledger["entry_id"], entry["id"]);
    assert_eq!(ledger["final_closed"], false);
    assert_eq!(ledger["recover_result"], false);
    assert!(ledger["result"].is_null());
    assert_eq!(operation["invocation_id"], marker["invocation_id"]);
    assert_eq!(operation["marker_closed"], false);
    assert_eq!(
        source.verifier.registration().company(),
        request.scope().company
    );
    for relation in [
        "fixture_provider_effects",
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_consumptions",
        "workflow_action_receipts",
        "workflow_action_evidence_conflicts",
    ] {
        assert_eq!(before[relation], json!([]), "empty {relation}");
    }
}

fn assert_evidence(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    after: &Value,
) {
    let evidence = one(after, "workflow_action_evidence");
    let observed = source.verifier.observation.lock().unwrap();
    let observed = observed.as_ref().unwrap();
    let registration = source.verifier.registration();
    assert_eq!(evidence["id"], json!(result.evidence.unwrap().as_uuid()));
    assert_eq!(evidence["command_key"], command.command_key.as_str());
    assert_eq!(
        evidence["request_digest"],
        command.request_digest().unwrap().as_str()
    );
    assert_eq!(evidence["coverage_digest"], observed["coverage_digest"]);
    assert_eq!(evidence["dispatch_id"], observed["marker"]);
    assert_eq!(evidence["disposition"], "unknown");
    assert_eq!(evidence["grant_eligible"], false);
    assert!(evidence["applied_request"].is_null());
    assert!(evidence["applied_remote_entry_id"].is_null());
    assert_eq!(evidence["registration"], registration.id().as_str());
    assert_eq!(
        evidence["verifier_version"],
        registration.version().as_str()
    );
    assert_eq!(evidence["provider"], registration.provider().as_str());
    assert_eq!(
        evidence["operation_signature"],
        registration.operation().as_str()
    );
    assert_eq!(evidence["authoritative_reference"], observed["reference"]);
    assert_eq!(
        instant(&evidence["observed_at"]),
        instant(&observed["observed_at"])
    );
    assert_eq!(
        instant(&evidence["valid_until"]),
        instant(&observed["valid_until"])
    );
    let verified = instant(&evidence["verified_at"]);
    assert!(instant(&evidence["observed_at"]) <= verified);
    assert!(verified <= instant(&evidence["created_at"]));
    assert!(instant(&evidence["valid_until"]) > verified);
    assert!(instant(&evidence["valid_until"]) <= verified + chrono::Duration::hours(24));
    assert_envelope(evidence, observed, registration);
    let coverage = one(after, "workflow_action_evidence_coverage");
    assert_eq!(coverage["evidence_id"], evidence["id"]);
    assert_eq!(coverage["remote_entry_id"], observed["entry"]);
}

fn assert_envelope(
    evidence: &Value,
    observed: &Value,
    registration: &EvidenceVerifierRegistration,
) {
    let verified = instant(&evidence["verified_at"]);
    let issued_verified = instant(&evidence["diagnostic"]["attestation"]["verified_at"]);
    // PostgreSQL timestamps retain microseconds; the issued envelope retains nanoseconds.
    assert_eq!(
        issued_verified.timestamp_micros(),
        verified.timestamp_micros()
    );
    assert_eq!(
        evidence["diagnostic"],
        json!({
            "attestation": {
                "version": 1, "registration": registration.id().as_str(),
                "verifier_version": registration.version().as_str(),
                "provider": registration.provider().as_str(), "reference": observed["reference"],
                "observed_at": instant(&observed["observed_at"]), "verified_at": issued_verified,
                "valid_until": instant(&observed["valid_until"]), "diagnostic": observed["diagnostic"],
            },
            "result": null,
        })
    );
}

fn assert_commit(
    command: &ReconcileActionCommand,
    result: &ReconciliationResult,
    before: &Value,
    after: &Value,
) {
    let evidence = one(after, "workflow_action_evidence");
    let receipt = one(after, "workflow_action_evidence_commands");
    let coverage = one(after, "workflow_action_evidence_coverage");
    let marker = one(before, "workflow_action_dispatches");
    for row in [evidence, receipt, coverage] {
        for column in [
            "company_id",
            "run_id",
            "execution_id",
            "invocation_id",
            "argument_digest",
        ] {
            assert_eq!(row[column], marker[column], "exact {column}");
        }
        assert_eq!(row["dispatch_id"], marker["id"]);
    }
    assert_eq!(receipt["id"], evidence["command_id"]);
    assert_eq!(receipt["evidence_id"], evidence["id"]);
    for column in ["command_key", "request_digest", "actor_id"] {
        assert_eq!(receipt[column], evidence[column]);
    }
    assert_eq!(evidence["actor_id"], json!(command.actor.user_id()));
    assert_eq!(
        receipt["expected_revision"],
        json!(command.expected_revision.0)
    );
    assert_eq!(receipt["result_revision"], json!(result.revision.0));
    assert_eq!(receipt["outcome"], json!({"kind":"unknown_recorded"}));
    assert!(receipt["scheduled_job_id"].is_null());
    assert_changes(result, before, after);
}

fn assert_changes(result: &ReconciliationResult, before: &Value, after: &Value) {
    let evidence = one(after, "workflow_action_evidence");
    let receipt = one(after, "workflow_action_evidence_commands");
    let old_events = before["workflow_run_events"].as_array().unwrap();
    let events = after["workflow_run_events"].as_array().unwrap();
    let added: Vec<_> = events
        .iter()
        .filter(|row| !old_events.contains(row))
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(events.len(), old_events.len() + 1);
    let audit = added[0];
    assert_eq!(audit["event_kind"], "action_reconciled");
    assert_eq!(audit["company_id"], evidence["company_id"]);
    assert_eq!(audit["run_id"], evidence["run_id"]);
    assert_eq!(audit["execution_id"], evidence["execution_id"]);
    assert_eq!(audit["actor_id"], evidence["actor_id"]);
    assert_eq!(audit["sequence"], receipt["audit_sequence"]);
    let mut normalized = after.clone();
    for relation in [
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
        "workflow_action_evidence_coverage",
    ] {
        normalized[relation] = before[relation].clone();
    }
    normalized["workflow_run_events"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row != audit);
    let run = one(after, "workflow_runs");
    assert_eq!(run["revision"], receipt["result_revision"]);
    assert_eq!(
        result.revision.0,
        before["workflow_runs"][0]["revision"].as_u64().unwrap() + 1,
        "Unknown evidence advances the audit-only run revision exactly once"
    );
    normalized["workflow_runs"][0]["revision"] = before["workflow_runs"][0]["revision"].clone();
    assert_eq!(
        normalized, *before,
        "only declared facts/audit/revision changed"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_native_evidence_registered_unknown_commits() {
    // Box the real service/fixture seams for stock 2 MiB test stacks.
    let source = Box::pin(unknown_source()).await;
    let command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.ledger,
        "native-unknown-source",
    )
    .await;
    let before = all_tables(&source.fixture).await;
    assert_source(&source, &before);
    let catalog = native_catalog(&source.fixture).await;
    let result = Box::pin(run_command(
        &source.fixture,
        &command,
        source.verifier.clone(),
    ))
    .await
    .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::UnknownRecorded);
    assert!(!result.replayed);
    let after = all_tables(&source.fixture).await;
    assert_evidence(&source, &command, &result, &after);
    assert_commit(&command, &result, &before, &after);
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 0);
    assert_eq!(native_catalog(&source.fixture).await, catalog);
    eprintln!(
        "native_unknown source=actual_pending_ledger registered=true coverage=1 ordinary_commit=true verifier_calls=1 provider_calls=1 effects=0"
    );
    source.fixture.persistence().pool().close().await;
}
