//! Existing-job eligibility and a shared-root debit after reconciliation schedules.
use super::*;
use crate::application::workflow::budget::{
    BudgetDisposition, BudgetReservation, BudgetReservationKey, BudgetReservationResult,
    WorkflowBudgets,
};
use crate::domain::workflow::{BudgetCharge, BudgetResource};

#[path = "action_reconciliation_claim_fairness_tests.rs"]
mod claim_fairness_tests;

#[path = "action_reconciliation_eligibility_poison_tests.rs"]
mod poison_tests;

#[path = "action_reconciliation_eligibility_remaining_tests.rs"]
mod remaining_tests;

async fn limited() -> (AdmissionFixture, ActionDispatchRequest) {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    let (f, scope) = fixture_source(source).await;
    Box::pin(setup_action_on_fixture(
        f,
        scope,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await
}

async fn debit(f: &AdmissionFixture, fence: WorkflowFence) {
    let reservation = BudgetReservation::new(
        fence,
        BudgetReservationKey::parse("eligibility-shared-root").unwrap(),
        BudgetCharge::new(BudgetResource::ModelCall, 2).unwrap(),
    )
    .unwrap();
    assert_eq!(
        f.persistence()
            .reserve_budget(reservation, policy())
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
}

async fn child(f: &AdmissionFixture, parent: ActivationRequest) -> ActivationRequest {
    let trigger = TriggerRef::new(
        parent.company,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: ChildCause::Execution(ExecutionRef::new(
                parent.company,
                parent.run,
                parent.execution,
                StepId::parse("start").unwrap(),
            )),
        },
    )
    .unwrap();
    let c = f
        .prepare(f.request("eligibility-budget-child", trigger))
        .await;
    assert!(matches!(
        f.persistence().admit(&c).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(c.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    ActivationRequest {
        company: parent.company,
        run: c.proposed_run_id(),
        execution: c.first_execution_id(),
        job: WorkflowJobId(job),
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_root_model_budget_blocks_schedule() {
    let (f, request) = Box::pin(limited()).await;
    let verifier = ledger(&f, &request).await;
    assert!(
        invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
            .await
            .is_err()
    );
    debit(&f, request.fence).await;
    park(&f, &request).await;
    barrier(&f, &request).await;
    let before = all_tables(&f).await;
    let c = proof_command(&f, &request, &verifier, "at-model-budget").await;
    let result = run_command(&f, &c, verifier).await.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob
        }
    );
    let after = all_tables(&f).await;
    preserved_history(&before, &after);
    assert_eq!(before["background_tasks"], after["background_tasks"]);
    assert_eq!(before["workflow_executions"], after["workflow_executions"]);
    assert_eq!(
        after["workflow_action_evidence"].as_array().unwrap().len(),
        1
    );
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(after["workflow_runs"][0]["state"], "waiting");
    let saved = all_tables(&f).await;
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(saved, all_tables(&f).await);
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_claim_rechecks_shared_root_budget() {
    let (f, request) = Box::pin(limited()).await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(invoke(&f, &request, &provider).await.is_err());
    park(&f, &request).await;
    barrier(&f, &request).await;
    let c = proof_command(&f, &request, &verifier, "before-shared-debit").await;
    assert_eq!(
        run_command(&f, &c, verifier).await.unwrap().outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let descendant = child(&f, request.fence.scope).await;
    let claim = f
        .persistence()
        .claim_io(descendant, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    debit(&f, claim.fence).await;
    let before = all_tables(&f).await;
    assert_eq!(before["workflow_root_budgets"].as_array().unwrap().len(), 1);
    assert_eq!(before["workflow_run_budgets"].as_array().unwrap().len(), 2);
    assert_eq!(before["workflow_root_budget_usage"][0]["model_calls"], 2);
    assert_eq!(before["workflow_root_budgets"][0]["model_calls"], 2);
    let (a, b) = tokio::join!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy()),
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
    );
    let claims = [a.unwrap(), b.unwrap()];
    assert_eq!(
        claims.iter().filter(|claim| claim.is_some()).count(),
        0,
        "a scheduled reconciliation continuation must recheck current shared-root exhaustion"
    );
    let after = all_tables(&f).await;
    assert_eq!(before["task_attempts"], after["task_attempts"]);
    // A refusal may append accounting facts; existing debits cannot be changed.
    for receipt in before["workflow_budget_receipts"].as_array().unwrap() {
        assert!(
            after["workflow_budget_receipts"]
                .as_array()
                .unwrap()
                .contains(receipt)
        );
    }
    assert_eq!(
        before["workflow_root_budget_usage"],
        after["workflow_root_budget_usage"]
    );
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_refusal(&before, &after, &request, &c);
    // Keep stock test stacks shallow across the additional DB/API boundary.
    Box::pin(refused_retries(&f, &request, &c, after)).await;
}

fn assert_refusal(
    before: &Value,
    after: &Value,
    request: &ActionDispatchRequest,
    command: &ReconcileActionCommand,
) {
    let episodes = before["workflow_action_claim_episodes"].as_array().unwrap();
    assert_eq!(episodes.len(), 1);
    let episode = &episodes[0];
    let scope = request.fence.scope;
    assert_eq!(episode["company_id"], json!(scope.company.as_uuid()));
    assert_eq!(episode["run_id"], json!(scope.run.as_uuid()));
    assert_eq!(episode["execution_id"], json!(scope.execution.as_uuid()));
    assert_eq!(episode["job_id"], json!(scope.job.0));
    assert_eq!(episode["command_key"], json!(command.command_key.as_str()));
    assert_eq!(episode["retired_attempt"], json!(request.fence.attempt.0));
    assert_eq!(
        before["workflow_action_claim_episodes"],
        after["workflow_action_claim_episodes"]
    );
    assert!(
        before["workflow_action_claim_budget_refusals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let refusals = after["workflow_action_claim_budget_refusals"]
        .as_array()
        .unwrap();
    assert_eq!(refusals.len(), 1);
    let refusal = &refusals[0];
    for field in [
        "company_id",
        "run_id",
        "execution_id",
        "job_id",
        "command_key",
        "retired_attempt",
    ] {
        assert_eq!(
            refusal[field], episode[field],
            "exact refusal scope: {field}"
        );
    }
    let events = after["workflow_run_events"].as_array().unwrap();
    let audits: Vec<_> = events
        .iter()
        .filter(|event| {
            event["company_id"] == episode["company_id"]
                && event["run_id"] == episode["run_id"]
                && event["sequence"] == refusal["audit_sequence"]
        })
        .collect();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0]["execution_id"], episode["execution_id"]);
    assert_eq!(audits[0]["event_kind"], "workflow.root_budget_exhausted");
    let job = after["background_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["id"] == episode["job_id"])
        .unwrap();
    assert_eq!(job["status"], "failed");
    assert_eq!(job["retry_count"], episode["retired_attempt"]);
    assert!(job["worker_id"].is_null());
    assert!(job["execution_generation"].is_null());
    assert!(job["lock_expires_at"].is_null());
    for table in [
        "workflow_executions",
        "workflow_budget_receipts",
        "workflow_action_evidence_commands",
        "workflow_action_evidence",
        "workflow_action_receipts",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
        "workflow_action_evidence_consumptions",
    ] {
        assert_eq!(before[table], after[table], "refusal preserves {table}");
    }
}

async fn refused_retries(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    command: &ReconcileActionCommand,
    frozen: Value,
) {
    for _ in 0..2 {
        assert!(
            f.persistence()
                .claim_io(request.fence.scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        frozen,
        all_tables(f).await,
        "repeated claims cannot hot-loop or erase refusal"
    );
    let revision = RunRevision(
        frozen["workflow_runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|run| run["id"] == json!(request.fence.scope.run.as_uuid()))
            .unwrap()["revision"]
            .as_u64()
            .unwrap(),
    );
    assert_eq!(
        f.persistence()
            .retry(RetryCommand {
                company_id: command.scope.company,
                run_id: command.scope.run,
                actor: command.actor,
                expected_revision: revision,
                command_key: IdempotencyKey::parse("budget-refused-ordinary-retry").unwrap(),
            })
            .await
            .unwrap(),
        RetryResult::Unsafe { revision }
    );
    let mut after = all_tables(f).await;
    assert_eq!(
        after["workflow_control_commands"].as_array().unwrap().len(),
        frozen["workflow_control_commands"]
            .as_array()
            .unwrap()
            .len()
            + 1
    );
    after["workflow_control_commands"] = frozen["workflow_control_commands"].clone();
    assert_eq!(
        frozen, after,
        "ordinary retry cannot change refused episode or accounting"
    );
}

#[path = "action_reconciliation_owner_lifecycle_tests.rs"]
mod owner_lifecycle_tests;

#[path = "action_reconciliation_claim_race_tests.rs"]
mod claim_race_tests;

#[path = "action_reconciliation_claim_sql_tests.rs"]
mod claim_sql_tests;
