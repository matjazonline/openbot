//! Authentic owner inserts calibrate four native targets and their bounded negatives.
use super::*;

#[path = "action_reconciliation_command_probe_negative_tests.rs"]
mod negative_tests;

#[path = "action_reconciliation_audit_sql_tests.rs"]
mod audit_sql_tests;

#[path = "action_reconciliation_audit_execution_tests.rs"]
mod audit_execution_tests;

#[path = "action_reconciliation_reopen_tests.rs"]
mod reopen_tests;

#[path = "action_reconciliation_other_audit_tests.rs"]
mod other_audit_tests;

#[derive(Clone, Copy, Debug)]
enum FlushTarget {
    EvidenceCommand,
    CommandEvidence,
    EvidenceScope,
    CommandScope,
}
impl FlushTarget {
    fn key(self) -> &'static str {
        match self {
            Self::EvidenceCommand => "positive-evidence-command",
            Self::CommandEvidence => "positive-command-evidence",
            Self::EvidenceScope => "positive-evidence-scope",
            Self::CommandScope => "positive-command-scope",
        }
    }
    fn identifies(self, row: &Value) -> bool {
        match self {
            Self::EvidenceCommand => {
                row["relation"] == "workflow_action_evidence"
                    && row["reference"] == "workflow_action_evidence_commands"
                    && row["columns"]
                        == json!(["company_id", "command_key", "command_id", "request_digest"])
                    && row["reference_columns"]
                        == json!(["company_id", "command_key", "id", "request_digest"])
            }
            Self::CommandEvidence => {
                row["relation"] == "workflow_action_evidence_commands"
                    && row["reference"] == "workflow_action_evidence"
                    && row["columns"]
                        == json!([
                            "company_id",
                            "command_key",
                            "id",
                            "request_digest",
                            "evidence_id"
                        ])
                    && row["reference_columns"]
                        == json!([
                            "company_id",
                            "command_key",
                            "command_id",
                            "request_digest",
                            "id"
                        ])
            }
            Self::EvidenceScope => {
                row["relation"] == "workflow_action_evidence"
                    && row["name"] == "workflow_action_evidence_commit_guard"
                    && row["type"] == "t"
            }
            Self::CommandScope => {
                row["relation"] == "workflow_action_evidence_commands"
                    && row["name"] == "workflow_action_command_scope_guard"
                    && row["type"] == "t"
            }
        }
    }
}

async fn catalog(f: &AdmissionFixture) -> Vec<Value> {
    sqlx::query_scalar(include_str!(
        "action_reconciliation_command_probe_catalog.sql"
    ))
    .fetch_all(f.persistence().pool())
    .await
    .unwrap()
}

fn selected(catalog: &[Value], target: FlushTarget) -> &Value {
    let rows: Vec<_> = catalog
        .iter()
        .filter(|row| target.identifies(row))
        .collect();
    assert_eq!(rows.len(), 1, "exact live target {target:?}");
    let row = rows[0];
    assert_eq!(row["deferrable"], true);
    assert_eq!(row["initially_deferred"], true);
    assert_eq!(row["name_count"], 1, "public name is unambiguous");
    let triggers = row["triggers"].as_array().unwrap();
    assert!(!triggers.is_empty());
    for trigger in triggers {
        assert_eq!(trigger["enabled"], "O");
        if trigger["function"] == "RI_FKey_check_ins"
            || trigger["function"] == "RI_FKey_check_upd"
            || row["type"] == "t"
        {
            assert_eq!(trigger["deferrable"], true);
            assert_eq!(trigger["initially_deferred"], true);
        }
    }
    if row["type"] == "t" {
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0]["function"], row["name"]);
        assert_eq!(triggers[0]["relation"], row["relation"]);
    }
    row
}

fn expected(command: &ReconcileActionCommand, verifier: &LedgerVerifier, entry: Uuid) -> Value {
    let scope = command.scope;
    let common = json!({"company_id":scope.company.as_uuid(), "run_id":scope.run.as_uuid(),
        "execution_id":scope.execution.as_uuid(), "invocation_id":command.subject.invocation.as_uuid(),
        "argument_digest":command.subject.argument_digest.as_str(), "dispatch_id":command.marker.as_uuid(),
        "actor_id":command.actor.user_id(), "command_key":command.command_key.as_str(),
        "request_digest":command.request_digest().unwrap().as_str()});
    let mut evidence = common.clone();
    evidence.as_object_mut().unwrap().extend(json!({"disposition":"applied", "grant_eligible":false,
        "registration":verifier.registration.id().as_str(), "verifier_version":verifier.registration.version().as_str(),
        "provider":verifier.registration.provider().as_str(), "operation_signature":verifier.registration.operation().as_str(),
        "authoritative_reference":command.command_key.as_str(), "applied_request":"remote_entry", "applied_remote_entry_id":entry})
        .as_object().unwrap().clone());
    json!({"command":common,"evidence":evidence,"entry":entry})
}

async fn configure(
    f: &AdmissionFixture,
    command: &ReconcileActionCommand,
    verifier: &LedgerVerifier,
    entry: Uuid,
    target: FlushTarget,
    row: &Value,
) {
    sqlx::query("INSERT INTO fixture_command_probe_config(company_id,command_key,target,target_oid,target_name,expected) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).bind(target.key())
        .bind(row["oid"].as_i64().unwrap()).bind(row["name"].as_str().unwrap())
        .bind(expected(command, verifier, entry)).execute(f.persistence().pool()).await.unwrap();
    let absent: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_evidence_commands WHERE company_id=$1 AND command_key=$2")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str())
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(absent, 0, "fresh candidate cannot replay");
}

fn assert_source(readback: &Value, expected: &Value) {
    for relation in ["command", "evidence"] {
        for (field, value) in expected[relation].as_object().unwrap() {
            assert_eq!(&readback[relation][field], value, "{relation}.{field}");
        }
    }
    let command = &readback["command"];
    let evidence = &readback["evidence"];
    assert_eq!(command["evidence_id"], evidence["id"]);
    assert_eq!(command["id"], evidence["command_id"]);
    assert_eq!(readback["head_revision"], command["result_revision"]);
    assert_eq!(readback["marker"]["id"], evidence["dispatch_id"]);
    assert_eq!(readback["entries"].as_array().unwrap().len(), 1);
    assert_eq!(readback["entries"][0]["id"], expected["entry"]);
    assert_eq!(readback["coverage"].as_array().unwrap().len(), 1);
    assert_eq!(
        readback["coverage"][0]["remote_entry_id"],
        expected["entry"]
    );
    assert_eq!(readback["coverage"][0]["evidence_id"], evidence["id"]);
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "invocation_id",
        "argument_digest",
        "dispatch_id",
    ] {
        assert_eq!(readback["coverage"][0][field], evidence[field]);
        assert_eq!(readback["entries"][0][field], evidence[field]);
        if field != "dispatch_id" {
            assert_eq!(readback["marker"][field], evidence[field]);
        }
    }
    assert_eq!(readback["audit"]["sequence"], command["audit_sequence"]);
    for field in ["company_id", "run_id", "execution_id", "actor_id"] {
        assert_eq!(readback["audit"][field], command[field]);
    }
    assert_eq!(readback["audit"]["event_kind"], "action_reconciled");
    assert_eq!(readback["ledger"]["company_id"], evidence["company_id"]);
    assert_eq!(
        readback["ledger"]["invocation_id"],
        evidence["invocation_id"]
    );
    assert_eq!(readback["ledger"]["entry_id"], expected["entry"]);
    assert_eq!(readback["ledger"]["recover_result"], false);
    assert_eq!(readback["ledger"]["final_closed"], false);
    assert_eq!(readback["effect"]["entry_id"], expected["entry"]);
    assert_eq!(readback["effect"]["result"], readback["ledger"]["result"]);
    assert_eq!(readback["receipts"], json!([]));
    assert_eq!(readback["conflicts"], json!([]));
}

fn assert_distinct(
    foreign: &CompanyReceipt,
    request: &ActionDispatchRequest,
    candidate: &ReconcileActionCommand,
    entry: Uuid,
) {
    assert_ne!(request.scope().company, foreign.request.scope().company);
    assert_ne!(request.scope().run, foreign.request.scope().run);
    assert_ne!(request.scope().execution, foreign.request.scope().execution);
    assert_ne!(
        request.subject.invocation,
        foreign.request.subject.invocation
    );
    assert_ne!(
        candidate.actor.user_id(),
        foreign.fixture.binding.fixture.target.actor.user_id()
    );
    assert_ne!(candidate.marker.as_uuid(), foreign.marker);
    assert_ne!(entry, foreign.entry);
}

fn assert_committed(
    witness: &Value,
    readback: &Value,
    row: &Value,
    target: FlushTarget,
    result: &ReconciliationResult,
    database: &str,
) {
    assert_eq!(witness["target"], target.key());
    assert_eq!(witness["target_oid"], row["oid"]);
    assert_eq!(witness["target_name"], row["name"]);
    assert_eq!(witness["before_hits"], 2);
    assert_eq!(witness["after_hits"], 1);
    assert_eq!(witness["depth"], 1);
    assert_eq!(witness["selected_flush"], true);
    assert_eq!(witness["all_immediate"], true);
    assert_eq!(
        readback, &witness["pending"],
        "ordinary owner commit preserves exact pending facts"
    );
    assert_eq!(
        readback["evidence"]["id"],
        json!(result.evidence.unwrap().as_uuid())
    );
    assert_eq!(
        readback["command"]["result_revision"],
        json!(result.revision.0)
    );
    assert_eq!(readback["command"]["outcome"], json!(result.outcome));
    eprintln!(
        "positive target={target:?} database={database} oid={} name={} key={} command={} evidence={} selected_flush=true all_immediate=true ordinary_commit=true",
        row["oid"],
        row["name"],
        target.key(),
        readback["command"]["id"],
        readback["evidence"]["id"]
    );
}

async fn positive(foreign: &CompanyReceipt, target: FlushTarget, catalog: &[Value]) {
    // Box genuine owner/provider setup to retain the stock 2 MiB test stack.
    let (f, request) = Box::pin(foreign_action(&foreign.fixture)).await;
    let verifier = register_ledger(&f, &request).await;
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: false,
            lose: true,
        },
    );
    assert!(Box::pin(invoke(&f, &request, &provider)).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let marker = scoped_marker(&f, &request).await;
    let entry: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let mut candidate = command(&f, &request, marker).await;
    candidate.command_key = IdempotencyKey::parse(target.key()).unwrap();
    candidate.input = EvidenceInput::VerifiedReference {
        registration: verifier.registration.id().clone(),
        reference: EvidenceRecordReference::parse(target.key()).unwrap(),
    };
    assert_distinct(foreign, &request, &candidate, entry);
    let database: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let foreign_database: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(foreign.fixture.persistence().pool())
        .await
        .unwrap();
    assert_eq!(database, foreign_database);
    let row = selected(catalog, target);
    configure(&f, &candidate, &verifier, entry, target, row).await;
    let observed = Arc::new(ObservedVerifier::new(verifier.clone()));
    let result = Box::pin(run_command(&f, &candidate, observed.clone()))
        .await
        .unwrap();
    assert_eq!(observed.calls.load(Ordering::SeqCst), 1);
    assert!(!result.replayed);
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: false }
    );
    assert_ne!(result.evidence.unwrap().as_uuid(), foreign.evidence);
    let foreign_command: Uuid = sqlx::query_scalar(
        "SELECT command_id FROM workflow_action_evidence WHERE company_id=$1 AND id=$2",
    )
    .bind(foreign.request.scope().company.as_uuid())
    .bind(foreign.evidence)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let witness: Value = sqlx::query_scalar("SELECT jsonb_build_object('target',target,'target_oid',target_oid,'target_name',target_name,'before_hits',before_hits,'after_hits',after_hits,'depth',depth,'selected_flush',selected_flush,'all_immediate',all_immediate,'pending',pending) FROM fixture_command_probe_witness WHERE company_id=$1 AND command_key=$2")
        .bind(candidate.scope.company.as_uuid()).bind(candidate.command_key.as_str())
        .fetch_one(f.persistence().pool()).await.unwrap();
    let readback: Value = sqlx::query_scalar("SELECT fixture_command_probe_pending($1,$2)")
        .bind(candidate.scope.company.as_uuid())
        .bind(candidate.command_key.as_str())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_ne!(readback["command"]["id"], json!(foreign_command));
    assert_committed(&witness, &readback, row, target, &result, &database);
    assert_source(&readback, &expected(&candidate, &verifier, entry));
}

#[tokio::test]
async fn workflow_action_reconciliation_command_probe_positive_calibration() {
    // Existing one-time bootstrap and authentic foreign-company owner seams.
    let (f, request) = Box::pin(setup()).await;
    let verifier = ledger(&f, &request).await;
    let foreign = Box::pin(applied_history(f, request, verifier.clone())).await;
    let original = catalog(&foreign.fixture).await;
    for target in [
        FlushTarget::EvidenceCommand,
        FlushTarget::CommandEvidence,
        FlushTarget::EvidenceScope,
        FlushTarget::CommandScope,
    ] {
        selected(&original, target);
    }
    sqlx::raw_sql(include_str!(
        "action_reconciliation_command_probe_fixture.sql"
    ))
    .execute(foreign.fixture.persistence().pool())
    .await
    .unwrap();
    let foreign_readback: Value = sqlx::query_scalar("SELECT fixture_command_probe_pending($1,$2)")
        .bind(foreign.request.scope().company.as_uuid())
        .bind("company-applied-missing")
        .fetch_one(foreign.fixture.persistence().pool())
        .await
        .unwrap();
    let mut foreign_command = command(&foreign.fixture, &foreign.request, foreign.marker).await;
    foreign_command.command_key = IdempotencyKey::parse("company-applied-missing").unwrap();
    foreign_command.expected_revision.0 = foreign_readback["command"]["expected_revision"]
        .as_u64()
        .unwrap();
    foreign_command.input = EvidenceInput::VerifiedReference {
        registration: verifier.registration.id().clone(),
        reference: EvidenceRecordReference::parse("company-applied-missing").unwrap(),
    };
    assert_source(
        &foreign_readback,
        &expected(&foreign_command, &verifier, foreign.entry),
    );
    assert_eq!(foreign_readback["evidence"]["id"], json!(foreign.evidence));
    for target in [
        FlushTarget::EvidenceCommand,
        FlushTarget::CommandEvidence,
        FlushTarget::EvidenceScope,
        FlushTarget::CommandScope,
    ] {
        Box::pin(positive(&foreign, target, &original)).await;
    }
    assert_eq!(effects(&foreign.fixture).await, 5);
    assert_eq!(
        catalog(&foreign.fixture).await,
        original,
        "native catalog and functions byte preserved"
    );
    foreign.fixture.persistence().pool().close().await;
}
