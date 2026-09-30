//! Genuine child proof records truth while every non-lineage reopen gate passes.
use super::*;

struct ChildProof {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
    command: ReconcileActionCommand,
}

async fn uncertain(f: AdmissionFixture, scope: ActivationRequest) -> ChildProof {
    // Box the actual provider/action seam to preserve stock 2 MiB test stacks.
    let (f, request) = Box::pin(setup_action_on_fixture(
        f,
        scope,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    let verifier = ledger(&f, &request).await;
    assert!(
        invoke(&f, &request, &LedgerProvider::new(&f, Delivery::Pending))
            .await
            .is_err()
    );
    park(&f, &request).await;
    barrier(&f, &request).await;
    let command = proof_command(&f, &request, &verifier, "child-policy-proof").await;
    ChildProof {
        fixture: f,
        request,
        verifier,
        command,
    }
}

async fn non_lineage_gates(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    // Mirror the existing owner's non-lineage conjunction to isolate child policy.
    // No source/lineage fact or production predicate is modified.
    let eligible: bool = sqlx::query_scalar(r#"
        SELECT EXISTS(SELECT 1 FROM workflow_runs AS owner
            JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
            JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
            JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
            WHERE owner.company_id=$1 AND owner.id=$2 AND task.id=$3 AND task.queue_kind='workflow'
                AND owner.deadline>clock_timestamp() AND execution.activation<=owner.max_steps
                AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
                AND execution.committed_output IS NULL AND execution.committed_route IS NULL AND execution.successor_execution_id IS NULL
                AND task.retry_count>0 AND task.retry_count<task.max_retries
                AND task.worker_id IS NULL AND task.execution_generation IS NULL AND task.lock_expires_at IS NULL
                AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
                AND attempt.workflow_retirement IN ('live','expired')
                AND attempt.workflow_failure_code NOT IN ('workflow.activation_limit','workflow.invalid_result','workflow.run_deadline','workflow.root_budget_exhausted')
                AND workflow_action_retry_safe($1,execution.id) IS TRUE
                AND workflow_action_reconciliation_budget_eligible($1,$2)
                AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
                    WHERE refusal.company_id=$1 AND refusal.run_id=$2 AND refusal.execution_id=execution.id AND refusal.job_id=$3)
                AND NOT EXISTS(SELECT 1 FROM workflow_executions AS sibling
                    JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                    WHERE sibling.company_id=$1 AND sibling.run_id=$2 AND other.queue_kind='workflow'
                        AND other.id<>$3 AND other.status IN ('pending','processing')))
        "#).bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .bind(request.fence.scope.job.0).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(
        eligible,
        "every ordinary reopen gate except admission lineage passes"
    );
}

fn waiting(state: &Value, request: &ActionDispatchRequest) {
    let owner = row(state, "workflow_runs", "id", request.scope().run.as_uuid());
    assert_eq!(owner["state"], "waiting");
    assert_eq!(owner["waiting_reason"], "reconciliation");
    assert_eq!(
        row(state, "background_tasks", "id", request.fence.scope.job.0)["status"],
        "failed"
    );
}

fn child_truth_only(
    before: &Value,
    after: &Value,
    request: &ActionDispatchRequest,
    result: &ReconciliationResult,
) {
    for (table, saved) in before.as_object().unwrap() {
        if matches!(
            table.as_str(),
            "workflow_action_evidence"
                | "workflow_action_evidence_coverage"
                | "workflow_action_evidence_commands"
                | "workflow_run_events"
        ) {
            let rows = after[table].as_array().unwrap();
            for old in saved.as_array().unwrap() {
                assert!(rows.contains(old), "historical {table}");
            }
        } else if table == "workflow_runs" {
            let mut expected = saved.clone();
            for owner in expected.as_array_mut().unwrap() {
                if owner["id"] == json!(request.scope().run.as_uuid()) {
                    assert!(result.revision.0 > owner["revision"].as_u64().unwrap());
                    owner["revision"] = json!(result.revision.0);
                }
            }
            assert_eq!(expected, after[table], "only owning run revision advances");
        } else {
            assert_eq!(
                saved, &after[table],
                "blocked child preserves entire {table}"
            );
        }
    }
    waiting(after, request);
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
    let evidence = &after["workflow_action_evidence"][0];
    assert_eq!(evidence["disposition"], "final_not_applied");
    assert_eq!(evidence["grant_eligible"], true);
    assert_eq!(evidence["run_id"], json!(request.scope().run.as_uuid()));
    assert_eq!(
        after["workflow_action_evidence_coverage"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        after["workflow_run_events"].as_array().unwrap().len(),
        before["workflow_run_events"].as_array().unwrap().len() + 1
    );
    assert!(after["workflow_action_evidence_commands"][0]["scheduled_job_id"].is_null());
    assert!(
        after["workflow_action_claim_episodes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        after["workflow_action_claim_budget_refusals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_claim_fairness_child_policy_records_proof_without_schedule()
{
    // Box genuine root/parent/grandchild and provider seams for stock 2 MiB stacks.
    let s = Box::pin(descendant_fixture()).await;
    let lineage = all_tables(&s.fixture).await;
    assert_lineage(&s, &lineage);
    let ChildProof {
        fixture: f,
        request,
        verifier,
        command,
    } = Box::pin(uncertain(s.fixture, s.descendant)).await;
    let before = all_tables(&f).await;
    waiting(&before, &request);
    assert_eq!(before["workflow_root_budget_usage"][0]["model_calls"], 0);
    let result = run_command(&f, &command, verifier).await.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob
        }
    );
    non_lineage_gates(&f, &request).await;
    let after = all_tables(&f).await;
    child_truth_only(&before, &after, &request, &result);
    assert!(
        retry_safe(&f, &request).await,
        "accepted final absence grants action truth"
    );
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
        "blocked child never becomes claimable"
    );

    Box::pin(non_child_control()).await;
}

async fn non_child_control() {
    // Use the identical binding and limits; only the actual admission lineage differs.
    let control = Box::pin(descendant_fixture()).await;
    let scope = control.root;
    let control = control.fixture;
    let ChildProof {
        fixture: control,
        request,
        verifier,
        command,
    } = Box::pin(uncertain(control, scope)).await;
    let before = all_tables(&control).await;
    waiting(&before, &request);
    let admission = row(
        &before,
        "workflow_admissions",
        "run_id",
        scope.run.as_uuid(),
    );
    assert!(
        admission["source_key"]
            .as_str()
            .unwrap()
            .starts_with("v1:[\"manual\"")
    );
    let result = run_command(&control, &command, verifier).await.unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        },
        "corresponding non-child proof schedules"
    );
    non_lineage_gates(&control, &request).await;
    let after = all_tables(&control).await;
    assert_eq!(
        after["workflow_action_claim_episodes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        row(&after, "background_tasks", "id", scope.job.0)["status"],
        "pending"
    );
    assert_eq!(
        row(&after, "workflow_runs", "id", scope.run.as_uuid())["state"],
        "running"
    );
    assert_eq!(entry_consumptions(&control).await, (1, 0));
}
