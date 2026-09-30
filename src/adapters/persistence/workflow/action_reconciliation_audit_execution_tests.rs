//! Genuine same-run audit execution attack through the ordinary reconciliation owner.
use super::*;
use crate::application::workflow::lease::FencedWorkflowResult;
use serde::Deserialize;

#[path = "action_reconciliation_audit_execution_transport.rs"]
mod transport;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AuditCase {
    Positive,
    AuditExecution,
    AcceptanceControl,
    MismatchControl,
}
impl AuditCase {
    fn key(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::AuditExecution => "audit-execution",
            Self::AcceptanceControl => "acceptance-control",
            Self::MismatchControl => "mismatch-control",
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::AuditExecution => "FIXTURE_AUDIT_EXECUTION_NATIVE_V1:",
            Self::AcceptanceControl => "FIXTURE_AUDIT_EXECUTION_ACCEPTED_V1:",
            Self::MismatchControl => "FIXTURE_AUDIT_EXECUTION_MISMATCH_V1:",
            Self::Positive => panic!("positive has no error envelope"),
        }
    }
}

struct AuditOwner {
    fixture: AdmissionFixture,
    first: ActivationRequest,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
    provider: LedgerProvider,
    candidate: ReconcileActionCommand,
    entry: Uuid,
}

fn source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    source["steps"]["second"] = source["steps"]["start"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("second");
    source
}

async fn owner(case: AuditCase) -> AuditOwner {
    // Box real admission/completion/action/provider seams to retain stock 2 MiB stacks.
    let (f, first) = Box::pin(fixture_source(source())).await;
    let claim = f
        .persistence()
        .claim_io(first, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let completed = f
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output: good(),
        })
        .await
        .unwrap()
        .unwrap();
    let second = completed.successor.unwrap();
    assert_eq!(first.company, second.company);
    assert_eq!(first.run, second.run);
    assert_ne!(first.execution, second.execution);
    assert_ne!(first.job, second.job);
    let (f, request) = Box::pin(setup_action_on_fixture(
        f,
        second,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    let verifier = ledger(&f, &request).await;
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
    park(&f, &request).await;
    let entry: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let candidate = proof_command(&f, &request, &verifier, case.key()).await;
    AuditOwner {
        fixture: f,
        first,
        request,
        verifier,
        provider,
        candidate,
        entry,
    }
}

async fn install(owner: &AuditOwner, case: AuditCase, row: &Value) {
    let f = &owner.fixture;
    sqlx::raw_sql(include_str!(
        "action_reconciliation_command_probe_fixture.sql"
    ))
    .execute(f.persistence().pool())
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "action_reconciliation_audit_execution_fixture.sql"
    ))
    .execute(f.persistence().pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO fixture_audit_execution_config(company_id,command_key,probe_case,target_oid,target_name,expected,first_execution,first_job) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(owner.candidate.scope.company.as_uuid()).bind(owner.candidate.command_key.as_str())
        .bind(case.key()).bind(row["oid"].as_i64().unwrap()).bind(row["name"].as_str().unwrap())
        .bind(expected(&owner.candidate, &owner.verifier, owner.entry))
        .bind(owner.first.execution.as_uuid()).bind(owner.first.job.0)
        .execute(f.persistence().pool()).await.unwrap();
    assert_eq!(
        owner.candidate.scope.execution,
        owner.request.scope().execution
    );
}

async fn positive() {
    let owner = Box::pin(owner(AuditCase::Positive)).await;
    let f = &owner.fixture;
    let original_catalog = catalog(f).await;
    let row = selected(&original_catalog, FlushTarget::CommandScope);
    install(&owner, AuditCase::Positive, row).await;
    let observed = Arc::new(ObservedVerifier::new(owner.verifier.clone()));
    let result = Box::pin(run_command(f, &owner.candidate, observed.clone()))
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::AppliedRecorded { receipt: false }
    );
    assert_eq!(observed.calls.load(Ordering::SeqCst), 1);
    let witness: Value = sqlx::query_scalar("SELECT to_jsonb(witness) FROM fixture_audit_execution_witness AS witness WHERE witness.company_id=$1 AND witness.command_key=$2")
        .bind(owner.candidate.scope.company.as_uuid()).bind(owner.candidate.command_key.as_str())
        .fetch_one(f.persistence().pool()).await.unwrap();
    let readback: Value = sqlx::query_scalar("SELECT fixture_command_probe_pending($1,$2)")
        .bind(owner.candidate.scope.company.as_uuid())
        .bind(owner.candidate.command_key.as_str())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert!(witness["owner_pid"].as_i64().unwrap() > 0);
    assert_eq!(witness["selected_flush"], true);
    assert_eq!(witness["all_immediate"], true);
    assert_eq!(
        witness["pending"], readback,
        "ordinary service COMMIT and exact readback"
    );
    assert_source(
        &readback,
        &expected(&owner.candidate, &owner.verifier, owner.entry),
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
    assert_eq!(catalog(f).await, original_catalog);
    assert_eq!(effects(f).await, 1);
    assert_eq!(owner.provider.calls.load(Ordering::SeqCst), 1);
    eprintln!(
        "audit_execution_positive selected_flush=true all_immediate=true ordinary_commit=true command={} evidence={} first_execution={} successor_execution={}",
        readback["command"]["id"],
        readback["evidence"]["id"],
        owner.first.execution.as_uuid(),
        owner.candidate.scope.execution.as_uuid()
    );
    f.persistence().pool().close().await;
}

async fn negative(case: AuditCase) {
    let owner = Box::pin(owner(case)).await;
    let f = &owner.fixture;
    let original_catalog = catalog(f).await;
    let row = selected(&original_catalog, FlushTarget::CommandScope);
    install(&owner, case, row).await;
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let before = all_tables(f).await;
    let observed = Arc::new(ObservedVerifier::new(owner.verifier.clone()));
    let error = Box::pin(run_command(f, &owner.candidate, observed.clone()))
        .await
        .expect_err("native/accepted/mismatch branches abort complete ordinary owner");
    assert_eq!(observed.calls.load(Ordering::SeqCst), 1);
    let envelope = transport::decode(error, case);
    transport::assert_envelope(&envelope, &owner, case, row, &database);
    transport::wait_rollback(&mut observer, &envelope, observer_pid).await;
    assert_eq!(
        all_tables(f).await,
        before,
        "every public table equals baseline without exclusions or compensation"
    );
    assert_eq!(owner.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(f).await, 1);
    assert_eq!(catalog(f).await, original_catalog);
    eprintln!(
        "audit_execution_negative_complete case={case:?} all_public_equality=true provider_history_unchanged=true"
    );
    drop(observer);
    f.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_audit_execution_positive() {
    Box::pin(positive()).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_audit_execution_native_and_transport_controls() {
    for case in [
        AuditCase::AuditExecution,
        AuditCase::AcceptanceControl,
        AuditCase::MismatchControl,
    ] {
        Box::pin(negative(case)).await;
    }
}
