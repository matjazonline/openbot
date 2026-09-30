//! Isolate classified retirement poison from deadline, attempt and budget gates.
use super::*;
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

struct Retired {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
}

async fn retired(code: &str) -> Retired {
    // Box the genuine claim/provider seam so these tests retain stock 2 MiB stacks.
    let (f, request) = Box::pin(limited()).await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(invoke(&f, &request, &provider).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let failure = StepFailure::new(
        FailureClass::Terminal,
        FailureCode::parse(code).unwrap(),
        None,
    )
    .unwrap();
    assert!(
        f.persistence()
            .release_io(
                request.fence,
                policy(),
                LeaseReleaseCause::Classified(WorkflowFailure {
                    failure,
                    safety: RetrySafety::SafeToRetry,
                }),
            )
            .await
            .unwrap()
    );
    barrier(&f, &request).await;
    let state = all_tables(&f).await;
    assert_eq!(state["workflow_runs"][0]["state"], "waiting");
    assert_eq!(
        state["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );
    let attempt = &state["task_attempts"][0];
    assert_eq!(attempt["workflow_failure_code"], code);
    assert_eq!(attempt["workflow_retirement"], "live");
    assert_eq!(attempt["workflow_retry_safety"], "unknown");
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    Retired {
        fixture: f,
        request,
        verifier,
    }
}

async fn all_other_gates(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    // Deliberately omit only the poison-code veto from the production predicate.
    // A false production result then cannot be attributed to another gate.
    let eligible: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_runs AS owner
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
           AND task.worker_id IS NULL AND task.execution_generation IS NULL AND task.lock_expires_at IS NULL
           AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
           AND attempt.workflow_retirement IN ('live','expired')
           AND workflow_action_retry_safe($1,execution.id) IS TRUE
           AND workflow_action_reconciliation_budget_eligible($1,$2)
           AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
             WHERE refusal.company_id=$1 AND refusal.run_id=$2 AND refusal.execution_id=execution.id AND refusal.job_id=$3)
           AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
           AND NOT EXISTS(SELECT 1 FROM workflow_executions AS sibling
             JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
             WHERE sibling.company_id=$1 AND sibling.run_id=$2 AND other.queue_kind='workflow'
               AND other.id<>$3 AND other.status IN ('pending','processing')))"
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.scope().run.as_uuid())
    .bind(request.fence.scope.job.0)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert!(eligible, "every non-poison gate must independently pass");
    let eligible: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
            .bind(request.scope().company.as_uuid())
            .bind(request.scope().run.as_uuid())
            .bind(request.fence.scope.job.0)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert!(!eligible, "production poison veto remains authoritative");
}

fn unchanged_history(before: &Value, after: &Value) {
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
        assert_eq!(before[table], after[table], "{table}");
    }
}

async fn poison_case(code: &str) {
    let Retired {
        fixture: f,
        request,
        verifier,
    } = Box::pin(retired(code)).await;
    let before = all_tables(&f).await;
    let command = proof_command(&f, &request, &verifier, "poison-final").await;
    let result = run_command(&f, &command, verifier).await.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob,
        }
    );
    all_other_gates(&f, &request).await;
    let after = all_tables(&f).await;
    unchanged_history(&before, &after);
    assert_eq!(before["background_tasks"], after["background_tasks"]);
    for field in [
        "state",
        "waiting_reason",
        "terminal_execution_id",
        "max_steps",
    ] {
        assert_eq!(
            before["workflow_runs"][0][field],
            after["workflow_runs"][0][field]
        );
    }
    assert_eq!(
        after["workflow_action_evidence"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        after["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(after["workflow_action_claim_episodes"], json!([]));
    assert_eq!(after["workflow_action_schedule_witnesses"], json!([]));
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(effects(&f).await, 0);
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        after,
        all_tables(&f).await,
        "blocked proof creates no claim side effects"
    );

    Box::pin(positive_control()).await;
}

async fn positive_control() {
    let Retired {
        fixture: f,
        request,
        verifier,
    } = Box::pin(retired("provider.rejected")).await;
    let before = all_tables(&f).await;
    let command = proof_command(&f, &request, &verifier, "normal-final").await;
    assert_eq!(
        run_command(&f, &command, verifier).await.unwrap().outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let after = all_tables(&f).await;
    unchanged_history(&before, &after);
    assert_eq!(after["workflow_runs"][0]["state"], "running");
    assert_eq!(after["background_tasks"][0]["status"], "pending");
    assert_eq!(
        before["background_tasks"].as_array().unwrap().len(),
        after["background_tasks"].as_array().unwrap().len()
    );
    for field in ["id", "retry_count", "max_retries", "workflow_execution_id"] {
        assert_eq!(
            before["background_tasks"][0][field],
            after["background_tasks"][0][field]
        );
    }
    assert_eq!(entry_consumptions(&f).await, (1, 0));
    assert_eq!(effects(&f).await, 0);
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_poison_codes_preserve_history() {
    for code in [
        "workflow.activation_limit",
        "workflow.invalid_result",
        "workflow.run_deadline",
        "workflow.root_budget_exhausted",
    ] {
        // Keep the paired full owner histories off the test thread's stack.
        Box::pin(poison_case(code)).await;
    }
}
