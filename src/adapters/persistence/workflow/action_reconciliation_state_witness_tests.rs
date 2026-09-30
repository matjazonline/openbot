//! Adversarial public UPDATEs and actual service NEW transformation, never a
//! successful terminal service schedule or a native COMMIT rejection claim.
use super::*;
use crate::application::workflow::lease::{LeaseReleaseCause, WorkflowFailure};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

#[path = "action_reconciliation_state_witness_transport.rs"]
mod witness_transport;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
enum Source {
    #[serde(rename = "FIRST_OLD_FAILED")]
    Failed,
    #[serde(rename = "FIRST_OLD_CANCELLED")]
    Cancelled,
    #[serde(rename = "FIRST_OLD_HUMAN_REASON")]
    Human,
    #[serde(rename = "WAITING_RECONCILIATION")]
    Waiting,
}
impl Source {
    fn label(self) -> &'static str {
        match self {
            Self::Failed => "FIRST_OLD_FAILED",
            Self::Cancelled => "FIRST_OLD_CANCELLED",
            Self::Human => "FIRST_OLD_HUMAN_REASON",
            Self::Waiting => "WAITING_RECONCILIATION",
        }
    }
    fn state(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Human | Self::Waiting => "waiting",
        }
    }
    fn reason(self) -> Option<&'static str> {
        match self {
            Self::Human => Some("decision"),
            Self::Waiting => Some("reconciliation"),
            _ => None,
        }
    }
}
struct WitnessOwner {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
    candidate: ReconcileActionCommand,
    source: Source,
    history: Value,
    expected: Value,
}
async fn failed_source(
    f: &AdmissionFixture,
    request: &mut ActionDispatchRequest,
    verifier: &Arc<LedgerVerifier>,
) -> Value {
    let command = proof_command(f, request, verifier, "genuine-before-failure").await;
    let result = Box::pin(run_command(f, &command, verifier.clone()))
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let history = all_tables(f).await;
    new_claim(f, request).await;
    assert_eq!(entry_consumptions(f).await, (1, 0));
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("fixture.terminal").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::EffectOutcomeUnknown,
    });
    assert!(
        f.persistence()
            .release_io(request.fence, policy(), cause)
            .await
            .unwrap()
    );
    history
}
async fn shape_source(f: &AdmissionFixture, request: &ActionDispatchRequest, source: Source) {
    match source {
        Source::Cancelled => {
            let command = command(f, request, marker(f).await).await;
            let result = f
                .persistence()
                .cancel(crate::application::workflow::CancelCommand {
                    company_id: command.scope.company,
                    run_id: command.scope.run,
                    actor: command.actor,
                    command_key: IdempotencyKey::parse("genuine-after-parking-cancel").unwrap(),
                    expected_revision: command.expected_revision,
                })
                .await
                .unwrap();
            assert!(matches!(
                result,
                crate::application::workflow::CancelResult::Applied { .. }
            ));
        }
        Source::Human => {
            // Deliberate public source shaping, not an authored human workflow owner.
            let mut tx = f.persistence().pool().begin().await.unwrap();
            assert_eq!(sqlx::query("UPDATE workflow_runs SET waiting_reason='decision' WHERE company_id=$1 AND id=$2 AND state='waiting' AND waiting_reason='reconciliation'")
                .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid()).execute(&mut *tx).await.unwrap().rows_affected(), 1);
            sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
        Source::Waiting => {}
        Source::Failed => unreachable!(),
    }
}
async fn witness_owner(source: Source) -> WitnessOwner {
    let Topology {
        fixture: f,
        first: _,
        mut request,
    } = Box::pin(topology()).await;
    let verifier = ledger(&f, &request).await;
    Box::pin(pending_request(&f, &request)).await;
    let parked = all_tables(&f).await;
    let history = if source == Source::Failed {
        Box::pin(failed_source(&f, &mut request, &verifier)).await
    } else {
        Box::pin(shape_source(&f, &request, source)).await;
        Value::Null
    };
    let committed = all_tables(&f).await;
    let run = &committed["workflow_runs"][0];
    assert_eq!(run["state"], source.state());
    assert_eq!(run["waiting_reason"], json!(source.reason()));
    assert_eq!(
        run["terminal_execution_id"],
        if source == Source::Failed {
            json!(request.scope().execution.as_uuid())
        } else {
            Value::Null
        }
    );
    if source != Source::Failed {
        assert_eq!(committed["background_tasks"], parked["background_tasks"]);
        assert_eq!(committed["task_attempts"], parked["task_attempts"]);
    }
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(effects(&f).await, 0);
    support::resources(&f).await;
    let candidate = proof_command(&f, &request, &verifier, source.label()).await;
    let mut expected = final_expected(&candidate, &verifier);
    expected["evidence"]["grant_eligible"] = json!(source != Source::Failed);
    WitnessOwner {
        fixture: f,
        request,
        verifier,
        candidate,
        source,
        history,
        expected,
    }
}
fn function_body<'a>(migration: &'a str, name: &str) -> &'a str {
    let start = migration
        .find(&format!("FUNCTION {name}("))
        .expect("immutable native definition");
    migration[start..]
        .split_once("$$")
        .unwrap()
        .1
        .split_once("$$")
        .unwrap()
        .0
}
async fn native_catalog(f: &AdmissionFixture) -> Value {
    let defs = [
        (
            "check_workflow_control_retry",
            include_str!(
                "../../../../migrations/20260930184000_workflow_action_audit_commands.sql"
            ),
        ),
        (
            "workflow_action_state_witness_owner",
            include_str!(
                "../../../../migrations/20260930181000_workflow_action_evidence_binding.sql"
            ),
        ),
        (
            "workflow_action_capture_state_witness",
            include_str!(
                "../../../../migrations/20260930181000_workflow_action_evidence_binding.sql"
            ),
        ),
        (
            "workflow_action_capture_schedule",
            include_str!("../../../../migrations/20261001100000_workflow_action_claim_budget.sql"),
        ),
        (
            "workflow_action_bind_schedule",
            include_str!("../../../../migrations/20261001100000_workflow_action_claim_budget.sql"),
        ),
        (
            "workflow_action_claim_episode_guard",
            include_str!("../../../../migrations/20261001100000_workflow_action_claim_budget.sql"),
        ),
        (
            "workflow_action_reconciliation_reopen_safe",
            include_str!("../../../../migrations/20261001100000_workflow_action_claim_budget.sql"),
        ),
        (
            "workflow_action_reconciliation_budget_eligible",
            include_str!("../../../../migrations/20261001100000_workflow_action_claim_budget.sql"),
        ),
    ];
    for (name, migration) in defs {
        let bodies: Vec<String> = sqlx::query_scalar(
            "SELECT prosrc FROM pg_proc WHERE pronamespace='public'::regnamespace AND proname=$1",
        )
        .bind(name)
        .fetch_all(f.persistence().pool())
        .await
        .unwrap();
        assert_eq!(
            bodies,
            vec![function_body(migration, name)],
            "exact immutable native body {name}"
        );
    }
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('oid',trigger.oid,'enabled',trigger.tgenabled,'definition',pg_get_triggerdef(trigger.oid),'function',pg_get_functiondef(trigger.tgfoid)) ORDER BY trigger.oid),'[]'::jsonb) FROM pg_trigger AS trigger JOIN pg_class AS relation ON relation.oid=trigger.tgrelid WHERE relation.relnamespace='public'::regnamespace AND trigger.tgname NOT LIKE 'fixture_%'")
        .fetch_one(f.persistence().pool()).await.unwrap()
}
async fn install_witness(owner: &WitnessOwner, mode: &str, target: &Value) {
    let f = &owner.fixture;
    sqlx::raw_sql(include_str!(
        "action_reconciliation_state_witness_fixture.sql"
    ))
    .execute(f.persistence().pool())
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "action_reconciliation_state_witness_validate.sql"
    ))
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let baseline = all_tables(f).await;
    sqlx::query("INSERT INTO fixture_state_config(company_id,run_id,execution_id,job_id,command_key,source_case,mode,target_oid,target_name,expected,history,baseline) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
        .bind(owner.candidate.scope.company.as_uuid()).bind(owner.candidate.scope.run.as_uuid()).bind(owner.candidate.scope.execution.as_uuid())
        .bind(owner.request.fence.scope.job.0).bind(owner.candidate.command_key.as_str()).bind(owner.source.label()).bind(mode)
        .bind(target["oid"].as_i64().unwrap()).bind(target["name"].as_str().unwrap()).bind(&owner.expected).bind(&owner.history).bind(baseline)
        .execute(f.persistence().pool()).await.unwrap();
}
async fn committed_control(source: Source, mode: &str) {
    let owner = Box::pin(witness_owner(source)).await;
    let f = &owner.fixture;
    let target = target(f).await;
    let catalog = native_catalog(f).await;
    install_witness(&owner, mode, &target).await;
    let before = all_tables(f).await;
    let result = Box::pin(run_command(f, &owner.candidate, owner.verifier.clone()))
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        if source == Source::Waiting {
            ReconciliationOutcome::Scheduled {
                receipt_only: false,
            }
        } else {
            ReconciliationOutcome::NotAppliedRecorded
        }
    );
    let witness: Value =
        sqlx::query_scalar("SELECT to_jsonb(witness) FROM fixture_state_witness AS witness")
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let pending: Value = sqlx::query_scalar("SELECT fixture_state_pending($1,$2)")
        .bind(owner.candidate.scope.company.as_uuid())
        .bind(owner.candidate.command_key.as_str())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(witness["pending"], pending);
    assert_eq!(witness["selected_flush"], true);
    assert_eq!(witness["all_immediate"], true);
    assert_eq!(
        pending["command"]["result_revision"],
        json!(result.revision.0)
    );
    assert_eq!(
        pending["command"]["evidence_id"],
        json!(result.evidence.unwrap().as_uuid())
    );
    if source != Source::Waiting {
        let after = all_tables(f).await;
        for relation in [
            "background_tasks",
            "task_attempts",
            "workflow_executions",
            "workflow_action_claim_episodes",
            "workflow_action_schedule_witnesses",
            "workflow_action_state_witnesses",
        ] {
            assert_eq!(before[relation], after[relation], "audit-only {relation}");
        }
        assert_eq!(pending["run"]["state"], source.state());
        assert_eq!(pending["run"]["waiting_reason"], json!(source.reason()));
    }
    assert_eq!(native_catalog(f).await, catalog);
    eprintln!(
        "state_witness_control source={} mode={mode} actual_commit=true selected=true all=true revision={}",
        source.label(),
        result.revision.0
    );
    f.persistence().pool().close().await;
}
async fn rejected_owner(source: Source, mode: &str) {
    let owner = Box::pin(witness_owner(source)).await;
    let f = &owner.fixture;
    let target = target(f).await;
    let catalog = native_catalog(f).await;
    install_witness(&owner, mode, &target).await;
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
        .expect_err("actual service owner abort");
    let envelope = witness_transport::decode(error, mode);
    witness_transport::assert_envelope(&envelope, &owner, mode, &target, &database);
    witness_transport::wait_rollback(&mut observer, &envelope, observer_pid).await;
    assert_eq!(
        all_tables(f).await,
        baseline,
        "EVERY public relation, exact full rollback"
    );
    assert_eq!(native_catalog(f).await, catalog);
    eprintln!(
        "state_witness_negative source={} mode={mode} whole_public_equal=true owner_quiescent=true",
        source.label()
    );
    drop(observer);
    f.persistence().pool().close().await;
}
#[tokio::test]
async fn workflow_action_reconciliation_first_old_failed() {
    Box::pin(rejected_owner(Source::Failed, "native")).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_first_old_cancelled() {
    Box::pin(rejected_owner(Source::Cancelled, "native")).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_first_old_human_reason() {
    Box::pin(rejected_owner(Source::Human, "native")).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_first_old_controls() {
    Box::pin(committed_control(Source::Waiting, "positive")).await;
    Box::pin(rejected_owner(Source::Waiting, "acceptance-control")).await;
    Box::pin(rejected_owner(Source::Human, "mismatch-control")).await;
    for source in [Source::Failed, Source::Cancelled, Source::Human] {
        Box::pin(committed_control(source, "no-attack")).await;
    }
}
