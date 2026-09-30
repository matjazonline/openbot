//! Exact deferred reopen predicates through genuine Final service owners.
use super::*;
use crate::application::workflow::lease::FencedWorkflowResult;
use serde::Deserialize;

#[path = "action_reconciliation_reopen_transport.rs"]
mod transport;

#[path = "action_reconciliation_state_witness_tests.rs"]
mod state_witness_tests;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ReopenCase {
    Positive,
    Actor,
    Execution,
    HistoricalRevision,
    AcceptanceControl,
    MismatchControl,
}
impl ReopenCase {
    fn key(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Actor => "actor",
            Self::Execution => "execution",
            Self::HistoricalRevision => "historical-revision",
            Self::AcceptanceControl => "acceptance-control",
            Self::MismatchControl => "mismatch-control",
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::AcceptanceControl => "FIXTURE_REOPEN_ACCEPTED_V1:",
            Self::MismatchControl => "FIXTURE_REOPEN_MISMATCH_V1:",
            Self::Positive => panic!("positive has no envelope"),
            _ => "FIXTURE_REOPEN_NATIVE_V1:",
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum History {
    First,
    Second,
}
struct FinalOwner {
    fixture: AdmissionFixture,
    first: ActivationRequest,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
    candidate: ReconcileActionCommand,
    other_actor: Uuid,
    history: Value,
    expected: Value,
}
struct Topology {
    fixture: AdmissionFixture,
    first: ActivationRequest,
    request: ActionDispatchRequest,
}
fn source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    source["steps"]["second"] = source["steps"]["start"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("second");
    source
}
async fn topology() -> Topology {
    // Box actual admission/completion/action seams for stock 2 MiB test stacks.
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
    Topology {
        fixture: f,
        first,
        request,
    }
}
async fn pending_request(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let provider = LedgerProvider::new(f, Delivery::Pending);
    assert!(matches!(
        Box::pin(invoke(f, request, &provider)).await,
        Err(AppError::Timeout(_))
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    park(f, request).await;
    barrier(f, request).await;
    assert!(!delayed_apply(f, request, &good()).await);
}
async fn first_schedule(
    f: &AdmissionFixture,
    request: &mut ActionDispatchRequest,
    verifier: &Arc<LedgerVerifier>,
) -> Value {
    let command = proof_command(f, request, verifier, "genuine-first-final").await;
    let result = Box::pin(run_command(f, &command, verifier.clone()))
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let history: Value = sqlx::query_scalar("SELECT jsonb_build_object('command',to_jsonb(command),'evidence',to_jsonb(evidence),'audit',to_jsonb(audit),'episode',to_jsonb(episode)) FROM workflow_action_evidence_commands AS command JOIN workflow_action_evidence AS evidence ON evidence.company_id=command.company_id AND evidence.id=command.evidence_id JOIN workflow_run_events AS audit ON audit.company_id=command.company_id AND audit.run_id=command.run_id AND audit.sequence=command.audit_sequence JOIN workflow_action_claim_episodes AS episode ON episode.company_id=command.company_id AND episode.command_key=command.command_key WHERE command.company_id=$1 AND command.command_key=$2")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(
        history["command"]["result_revision"],
        json!(result.revision.0)
    );
    new_claim(f, request).await;
    assert_eq!(request.fence.attempt.0, 2);
    Box::pin(pending_request(f, request)).await;
    assert_eq!(entry_consumptions(f).await, (2, 1));
    history
}
fn final_expected(command: &ReconcileActionCommand, verifier: &LedgerVerifier) -> Value {
    let scope = command.scope;
    let common = json!({"company_id":scope.company.as_uuid(),"run_id":scope.run.as_uuid(),"execution_id":scope.execution.as_uuid(),
        "invocation_id":command.subject.invocation.as_uuid(),"argument_digest":command.subject.argument_digest.as_str(),
        "dispatch_id":command.marker.as_uuid(),"actor_id":command.actor.user_id(),"command_key":command.command_key.as_str(),"request_digest":command.request_digest().unwrap().as_str()});
    let mut evidence = common.clone();
    evidence.as_object_mut().unwrap().extend(json!({"disposition":"final_not_applied","grant_eligible":true,
        "registration":verifier.registration.id().as_str(),"verifier_version":verifier.registration.version().as_str(),
        "provider":verifier.registration.provider().as_str(),"operation_signature":verifier.registration.operation().as_str(),
        "authoritative_reference":command.command_key.as_str(),"applied_request":null,"applied_remote_entry_id":null}).as_object().unwrap().clone());
    json!({"command":common,"evidence":evidence})
}
async fn owner(case: ReopenCase, history: History) -> FinalOwner {
    let Topology {
        fixture: f,
        first,
        mut request,
    } = Box::pin(topology()).await;
    let verifier = ledger(&f, &request).await;
    Box::pin(pending_request(&f, &request)).await;
    let retained = match history {
        History::First => Value::Null,
        History::Second => Box::pin(first_schedule(&f, &mut request, &verifier)).await,
    };
    support::resources(&f).await;
    let other_actor = support::principal(&f, None).await.user_id();
    assert_ne!(other_actor, f.binding.target.actor.user_id());
    let candidate = proof_command(&f, &request, &verifier, case.key()).await;
    let expected = final_expected(&candidate, &verifier);
    FinalOwner {
        fixture: f,
        first,
        request,
        verifier,
        candidate,
        other_actor,
        history: retained,
        expected,
    }
}
async fn target(f: &AdmissionFixture) -> Value {
    let rows: Vec<Value> =
        sqlx::query_scalar(include_str!("action_reconciliation_reopen_catalog.sql"))
            .fetch_all(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(rows.len(), 1, "unique native public target");
    let row = rows.into_iter().next().unwrap();
    assert_eq!(row["name_count"], 1);
    assert_eq!(row["relation"], "background_tasks");
    assert_eq!(row["function"], "check_workflow_control_retry");
    assert_eq!(row["enabled"], "O");
    assert_eq!(row["deferrable"], true);
    assert_eq!(row["initially_deferred"], true);
    assert_eq!(row["timing"], 17);
    row
}
async fn install(owner: &FinalOwner, case: ReopenCase, row: &Value) {
    let f = &owner.fixture;
    sqlx::raw_sql(include_str!("action_reconciliation_reopen_fixture.sql"))
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("action_reconciliation_reopen_validate.sql"))
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let baseline = all_tables(f).await;
    sqlx::query("INSERT INTO fixture_reopen_config(company_id,command_key,probe_case,target_oid,target_name,expected,first_execution,first_job,other_actor,history,baseline) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(owner.candidate.scope.company.as_uuid()).bind(owner.candidate.command_key.as_str()).bind(case.key())
        .bind(row["oid"].as_i64().unwrap()).bind(row["name"].as_str().unwrap()).bind(&owner.expected)
        .bind(owner.first.execution.as_uuid()).bind(owner.first.job.0).bind(owner.other_actor).bind(&owner.history).bind(baseline)
        .execute(f.persistence().pool()).await.unwrap();
}
fn assert_final(pending: &Value, owner: &FinalOwner) {
    for relation in ["command", "evidence"] {
        for (field, value) in owner.expected[relation].as_object().unwrap() {
            assert_eq!(&pending[relation][field], value, "{relation}.{field}");
        }
    }
    assert_eq!(pending["command"]["evidence_id"], pending["evidence"]["id"]);
    assert_eq!(pending["command"]["id"], pending["evidence"]["command_id"]);
    assert_eq!(
        pending["command"]["result_revision"],
        pending["run"]["revision"]
    );
    assert_eq!(
        pending["command"]["outcome"],
        json!({"kind":"scheduled","receipt_only":false})
    );
    assert_eq!(
        pending["command"]["scheduled_job_id"],
        json!(owner.request.fence.scope.job.0)
    );
    assert_eq!(pending["job"]["status"], "pending");
    assert_eq!(pending["run"]["state"], "running");
    assert_eq!(
        pending["audit"]["sequence"],
        pending["command"]["audit_sequence"]
    );
    for field in ["company_id", "run_id", "execution_id", "actor_id"] {
        assert_eq!(pending["audit"][field], pending["command"][field]);
    }
    assert_eq!(pending["audit"]["event_kind"], "action_reconciled");
    assert_eq!(
        pending["coverage"].as_array().unwrap().len(),
        pending["entries"].as_array().unwrap().len()
    );
    assert_eq!(
        pending["entries"].as_array().unwrap().len(),
        if owner.history.is_null() { 1 } else { 2 }
    );
    assert_eq!(pending["effects"], json!([]));
    assert_eq!(pending["receipts"], json!([]));
    assert_eq!(pending["conflicts"], json!([]));
    for ledger in pending["ledger"].as_array().unwrap() {
        assert_eq!(ledger["final_closed"], true);
        assert_eq!(ledger["result"], Value::Null);
    }
    assert_eq!(
        pending["episode"]["command_key"],
        pending["command"]["command_key"]
    );
    assert_eq!(
        pending["episode"]["retired_attempt"],
        pending["job"]["retry_count"]
    );
}
async fn positive(history: History) {
    let owner = Box::pin(owner(ReopenCase::Positive, history)).await;
    let f = &owner.fixture;
    let row = target(f).await;
    install(&owner, ReopenCase::Positive, &row).await;
    let result = Box::pin(run_command(f, &owner.candidate, owner.verifier.clone()))
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let witness:Value=sqlx::query_scalar("SELECT to_jsonb(witness) FROM fixture_reopen_witness AS witness WHERE witness.company_id=$1 AND witness.command_key=$2")
        .bind(owner.candidate.scope.company.as_uuid()).bind(owner.candidate.command_key.as_str()).fetch_one(f.persistence().pool()).await.unwrap();
    let readback: Value = sqlx::query_scalar("SELECT fixture_reopen_pending($1,$2)")
        .bind(owner.candidate.scope.company.as_uuid())
        .bind(owner.candidate.command_key.as_str())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(witness["selected_flush"], true);
    assert_eq!(witness["all_immediate"], true);
    assert_eq!(witness["pending"], readback);
    assert_final(&readback, &owner);
    assert_eq!(
        readback["command"]["result_revision"],
        json!(result.revision.0)
    );
    assert_eq!(
        readback["evidence"]["id"],
        json!(result.evidence.unwrap().as_uuid())
    );
    assert_eq!(target(f).await, row);
    assert_eq!(effects(f).await, 0);
    eprintln!(
        "reopen_positive history={history:?} selected_flush=true all_immediate=true ordinary_commit=true revision={}",
        result.revision.0
    );
    f.persistence().pool().close().await;
}
async fn negative(case: ReopenCase) {
    let history = if case == ReopenCase::HistoricalRevision {
        History::Second
    } else {
        History::First
    };
    let owner = Box::pin(owner(case, history)).await;
    let f = &owner.fixture;
    let row = target(f).await;
    install(&owner, case, &row).await;
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let baseline = all_tables(f).await;
    let error = Box::pin(run_command(f, &owner.candidate, owner.verifier.clone()))
        .await
        .expect_err("all nonpositive actual owners abort");
    let envelope = transport::decode(error, case);
    transport::assert_envelope(&envelope, &owner, case, &row, &database);
    transport::wait_rollback(&mut observer, &envelope, observer_pid).await;
    assert_eq!(
        all_tables(f).await,
        baseline,
        "all public bytes including provider/C1/config/captures; no exclusions or compensation"
    );
    assert_eq!(target(f).await, row);
    assert_eq!(effects(f).await, 0);
    eprintln!("reopen_negative case={case:?} all_public_equal=true owner_quiescent=true");
    drop(observer);
    f.persistence().pool().close().await;
}
#[tokio::test]
async fn workflow_action_reconciliation_reopen_matching_final_positives() {
    for history in [History::First, History::Second] {
        Box::pin(positive(history)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_reopen_native_and_transport_controls() {
    for case in [
        ReopenCase::Actor,
        ReopenCase::Execution,
        ReopenCase::HistoricalRevision,
        ReopenCase::AcceptanceControl,
        ReopenCase::MismatchControl,
    ] {
        Box::pin(negative(case)).await;
    }
}
