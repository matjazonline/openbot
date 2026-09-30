//! Public queue topology, lease shape and genuine budget-owner refusal histories.
use super::*;

#[path = "action_reconciliation_eligibility_history_tests.rs"]
mod history_tests;

#[path = "action_reconciliation_eligibility_invariant_tests.rs"]
mod invariant_tests;

struct PendingAction {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
}

async fn pending_action() -> PendingAction {
    // Keep the real claim/provider setup off the stock 2 MiB test stack.
    let (fixture, request) = Box::pin(limited()).await;
    let verifier = ledger(&fixture, &request).await;
    let provider = LedgerProvider::new(&fixture, Delivery::Pending);
    assert!(invoke(&fixture, &request, &provider).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    PendingAction {
        fixture,
        request,
        verifier,
    }
}

async fn parked() -> PendingAction {
    let pending = Box::pin(pending_action()).await;
    park(&pending.fixture, &pending.request).await;
    barrier(&pending.fixture, &pending.request).await;
    pending
}

fn retained_accounting(before: &Value, after: &Value) {
    for table in [
        "workflow_executions",
        "task_attempts",
        "workflow_admissions",
        "workflow_run_budgets",
        "workflow_root_budgets",
        "workflow_root_budget_usage",
        "workflow_budget_receipts",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
        "workflow_action_evidence_consumptions",
    ] {
        assert_eq!(before[table], after[table], "retained {table}");
    }
}

fn truth_only(before: &Value, after: &Value, command: &ReconcileActionCommand) {
    let saved = &after["workflow_action_evidence_commands"][0];
    assert_eq!(saved["command_key"], command.command_key.as_str());
    assert_eq!(saved["expected_revision"], command.expected_revision.0);
    assert_eq!(saved["outcome"]["kind"], "blocked");
    assert_eq!(saved["outcome"]["reason"], "ineligible_job");
    assert!(saved["scheduled_job_id"].is_null());
    assert!(!saved["evidence_id"].is_null());
    assert_eq!(
        after["workflow_action_evidence"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        after["workflow_action_evidence_coverage"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        after["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        after["workflow_run_events"].as_array().unwrap().len(),
        before["workflow_run_events"].as_array().unwrap().len() + 1
    );
    assert!(after["workflow_runs"][0]["revision"].as_u64().unwrap() > command.expected_revision.0);
    assert_eq!(
        saved["result_revision"],
        after["workflow_runs"][0]["revision"]
    );
    let mut normalized = after.clone();
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_commands",
        "workflow_run_events",
    ] {
        for old in before[table].as_array().unwrap() {
            assert!(
                after[table].as_array().unwrap().contains(old),
                "old {table} fact"
            );
        }
        normalized[table] = before[table].clone();
    }
    normalized["workflow_runs"][0]["revision"] = before["workflow_runs"][0]["revision"].clone();
    assert_eq!(
        &normalized, before,
        "truth attachment changes no other public history"
    );
}

async fn blocked(pending: &PendingAction, key: &str) {
    let f = &pending.fixture;
    let before = all_tables(f).await;
    // The command must use the generated revision after all adversarial setup.
    let command = proof_command(f, &pending.request, &pending.verifier, key).await;
    let result = run_command(f, &command, pending.verifier.clone())
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob
        }
    );
    let after = all_tables(f).await;
    truth_only(&before, &after, &command);
    assert_eq!(
        result.revision.0,
        after["workflow_runs"][0]["revision"].as_u64().unwrap()
    );
    assert!(
        retry_safe(f, &pending.request).await,
        "genuine final absence retains action truth"
    );
    assert_eq!(entry_consumptions(f).await, (1, 0));
    assert_eq!(effects(f).await, 0);
    assert!(
        f.persistence()
            .claim_io(pending.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        after,
        all_tables(f).await,
        "ineligible subject cannot claim or debit"
    );
}

async fn schedule_control(pending: PendingAction) {
    let f = &pending.fixture;
    let before = all_tables(f).await;
    let command = proof_command(f, &pending.request, &pending.verifier, "eligible-control").await;
    let result = run_command(f, &command, pending.verifier.clone())
        .await
        .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let after = all_tables(f).await;
    retained_accounting(&before, &after);
    assert_eq!(after["background_tasks"].as_array().unwrap().len(), 1);
    for field in [
        "id",
        "payload",
        "workflow_execution_id",
        "retry_count",
        "max_retries",
    ] {
        assert_eq!(
            before["background_tasks"][0][field],
            after["background_tasks"][0][field]
        );
    }
    assert_eq!(after["background_tasks"][0]["status"], "pending");
    assert_eq!(after["workflow_runs"][0]["state"], "running");
    assert_eq!(entry_consumptions(f).await, (1, 0));
    assert_eq!(effects(f).await, 0);
}

async fn extra_pending(pending: &PendingAction) -> Uuid {
    let scope = pending.request.fence.scope;
    let execution = Uuid::new_v4();
    let job = Uuid::new_v4();
    let mut tx = pending.fixture.persistence().pool().begin().await.unwrap();
    // An adversarial queue candidate at an unused ordinal, with no invented owner history.
    sqlx::query("INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation) VALUES ($1,$2,$3,'start',2)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(execution).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO background_tasks (id,company_id,channel_id,thread_id,correlation_id,task_type,payload,queue_kind,workflow_execution_id)
        SELECT $2,company_id,channel_id,thread_id,correlation_id,'workflow_execution',jsonb_build_object('version',1,'execution_id',$3::uuid),'workflow',$3 FROM background_tasks WHERE id=$1")
        .bind(scope.job.0).bind(job).bind(execution).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    job
}

async fn subject_without_sibling_veto(pending: &PendingAction) {
    let scope = pending.request.fence.scope;
    // Assert every subject gate, omitting only the final runnable-sibling veto.
    let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
        WHERE owner.company_id=$1 AND owner.id=$2 AND task.id=$3 AND task.queue_kind='workflow'
          AND owner.state='waiting' AND owner.waiting_reason='reconciliation' AND task.status='failed'
          AND owner.deadline>clock_timestamp() AND execution.activation<=owner.max_steps
          AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
          AND execution.committed_output IS NULL AND execution.committed_route IS NULL AND execution.successor_execution_id IS NULL
          AND task.retry_count>0 AND task.retry_count<task.max_retries
          AND task.worker_id IS NULL AND task.execution_generation IS NULL AND task.locked_at IS NULL AND task.lock_expires_at IS NULL
          AND attempt.status='failed' AND attempt.finished_at IS NOT NULL AND attempt.workflow_retirement IN ('live','expired')
          AND attempt.workflow_failure_code NOT IN ('workflow.activation_limit','workflow.invalid_result','workflow.run_deadline','workflow.root_budget_exhausted')
          AND workflow_action_retry_safe($1,execution.id) IS TRUE AND workflow_action_reconciliation_budget_eligible($1,$2)
          AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal WHERE refusal.company_id=$1 AND refusal.run_id=$2 AND refusal.execution_id=execution.id AND refusal.job_id=$3)
          AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child')")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.job.0).fetch_one(pending.fixture.persistence().pool()).await.unwrap();
    assert!(eligible, "the subject passes every other reopen gate");
    let eligible: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .bind(scope.job.0)
            .fetch_one(pending.fixture.persistence().pool())
            .await
            .unwrap();
    assert!(!eligible, "the extra pending candidate prevents reopening");
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_extra_pending_sibling_records_truth_only() {
    let pending = Box::pin(parked()).await;
    let job = extra_pending(&pending).await;
    let before = all_tables(&pending.fixture).await;
    let jobs = before["background_tasks"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(
        jobs.iter().filter(|row| row["status"] == "pending").count(),
        1
    );
    let candidate = jobs.iter().find(|row| row["id"] == json!(job)).unwrap();
    let execution = before["workflow_executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == candidate["workflow_execution_id"])
        .unwrap();
    assert!(execution["activated_at"].is_null());
    assert_eq!(before["task_attempts"].as_array().unwrap().len(), 1);
    Box::pin(blocked(&pending, "runnable-sibling")).await;
    subject_without_sibling_veto(&pending).await;
    Box::pin(schedule_control(Box::pin(parked()).await)).await;
}

fn constraint(error: sqlx::Error, name: &str) {
    let database = error.as_database_error().unwrap();
    assert_eq!(database.code().as_deref(), Some("23514"));
    assert_eq!(database.constraint(), Some(name));
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_failed_lease_fields_are_schema_rejected() {
    let pending = Box::pin(pending_action()).await;
    let f = &pending.fixture;
    let processing = all_tables(f).await;
    assert_eq!(processing["background_tasks"][0]["status"], "processing");
    for field in [
        "worker_id",
        "execution_generation",
        "locked_at",
        "lock_expires_at",
    ] {
        assert!(
            !processing["background_tasks"][0][field].is_null(),
            "real processing lease {field}"
        );
    }
    park(f, &pending.request).await;
    barrier(f, &pending.request).await;
    let before = all_tables(f).await;
    assert_eq!(before["background_tasks"][0]["status"], "failed");
    for assignment in [
        "worker_id=gen_random_uuid()",
        "execution_generation=gen_random_uuid()",
        "locked_at=clock_timestamp()",
        "lock_expires_at=clock_timestamp()+interval '1 minute'",
    ] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let error = sqlx::query(&format!(
            "UPDATE background_tasks SET {assignment} WHERE id=$1"
        ))
        .bind(pending.request.fence.scope.job.0)
        .execute(&mut *tx)
        .await
        .unwrap_err();
        constraint(error, "background_tasks_lease_check");
        tx.rollback().await.unwrap();
        assert_eq!(
            before,
            all_tables(f).await,
            "rejected lease shape rolls back all history"
        );
    }
    // Rejected SQL never reaches reconciliation; the unchanged genuine job can schedule.
    Box::pin(schedule_control(pending)).await;
}

async fn reserve(
    pending: &PendingAction,
    resource: BudgetResource,
    key: &str,
    quantity: u32,
) -> BudgetReservationResult {
    let request = BudgetReservation::new(
        pending.request.fence,
        BudgetReservationKey::parse(key).unwrap(),
        BudgetCharge::new(resource, quantity).unwrap(),
    )
    .unwrap();
    pending
        .fixture
        .persistence()
        .reserve_budget(request, policy())
        .await
        .unwrap()
}

async fn owner_exhaustion(resource: BudgetResource) {
    let pending = Box::pin(pending_action()).await;
    assert_eq!(
        reserve(&pending, resource, "initial-grant", 1).await,
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    let granted = all_tables(&pending.fixture).await;
    let counter = match resource {
        BudgetResource::ModelCall => "model_calls",
        BudgetResource::Repetition => "repetitions",
        BudgetResource::Activation => unreachable!(),
    };
    assert_eq!(granted["workflow_root_budget_usage"][0][counter], 1);
    assert_eq!(granted["workflow_root_budgets"][0][counter], 2);
    assert_eq!(
        reserve(&pending, resource, "oversize-reservation", 2).await,
        BudgetReservationResult::Recorded(BudgetDisposition::Exhausted)
    );
    let exhausted = all_tables(&pending.fixture).await;
    assert_eq!(
        granted["workflow_root_budget_usage"],
        exhausted["workflow_root_budget_usage"]
    );
    for receipt in granted["workflow_budget_receipts"].as_array().unwrap() {
        assert!(
            exhausted["workflow_budget_receipts"]
                .as_array()
                .unwrap()
                .contains(receipt)
        );
    }
    exhausted_receipt(&exhausted, &pending.request, resource);
    assert_eq!(
        exhausted["task_attempts"][0]["workflow_failure_code"],
        "workflow.root_budget_exhausted"
    );
    assert_eq!(
        exhausted["task_attempts"][0]["workflow_retry_safety"],
        "unknown"
    );
    assert_eq!(exhausted["task_attempts"][0]["workflow_retirement"], "live");
    assert_eq!(exhausted["workflow_runs"][0]["state"], "waiting");
    assert_eq!(
        exhausted["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );
    barrier(&pending.fixture, &pending.request).await;
    Box::pin(blocked(&pending, "owner-budget-refusal")).await;
    let before = all_tables(&pending.fixture).await;
    assert_eq!(
        reserve(&pending, resource, "oversize-reservation", 2).await,
        BudgetReservationResult::Replayed(BudgetDisposition::Exhausted)
    );
    assert_eq!(
        before,
        all_tables(&pending.fixture).await,
        "exhausted replay never replenishes usage"
    );
}

fn exhausted_receipt(exhausted: &Value, request: &ActionDispatchRequest, resource: BudgetResource) {
    let receipts: Vec<_> = exhausted["workflow_budget_receipts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["reservation_key"] == "oversize-reservation")
        .collect();
    assert_eq!(receipts.len(), 1);
    let receipt = receipts[0];
    assert_eq!(receipt["run_id"], json!(request.scope().run.as_uuid()));
    assert_eq!(
        receipt["execution_id"],
        json!(request.scope().execution.as_uuid())
    );
    let name = match resource {
        BudgetResource::ModelCall => "model_call",
        BudgetResource::Repetition => "repetition",
        BudgetResource::Activation => unreachable!(),
    };
    assert_eq!(receipt["resource"], name);
    assert_eq!(receipt["quantity"], 2);
    assert_eq!(receipt["disposition"], "exhausted");
}

async fn budget_control(resource: BudgetResource) {
    let pending = Box::pin(pending_action()).await;
    // Model calls need headroom for the next claim; repetitions permit exact equality.
    let quantity = match resource {
        BudgetResource::ModelCall => 1,
        BudgetResource::Repetition => 2,
        BudgetResource::Activation => unreachable!(),
    };
    assert_eq!(
        reserve(&pending, resource, "allowed-reservation", quantity).await,
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    let state = all_tables(&pending.fixture).await;
    let counter = match resource {
        BudgetResource::ModelCall => "model_calls",
        BudgetResource::Repetition => "repetitions",
        BudgetResource::Activation => unreachable!(),
    };
    assert_eq!(state["workflow_root_budget_usage"][0][counter], quantity);
    assert!(
        state["workflow_budget_receipts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["disposition"] == "granted")
    );
    park(&pending.fixture, &pending.request).await;
    barrier(&pending.fixture, &pending.request).await;
    Box::pin(schedule_control(pending)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_genuine_root_reservation_exhaustion_blocks() {
    for resource in [BudgetResource::ModelCall, BudgetResource::Repetition] {
        // Box paired provider/owner histories rather than increasing test stack limits.
        Box::pin(owner_exhaustion(resource)).await;
        Box::pin(budget_control(resource)).await;
    }
}
