//! Genuine completion histories and an adversarial mutable max_steps projection.
use super::*;
use crate::application::workflow::completion::WorkflowCompletion;
use crate::application::workflow::lease::FencedWorkflowResult;

#[path = "action_reconciliation_terminal_revision_tests.rs"]
mod terminal_revision_tests;

fn source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    source
}

fn two_steps() -> Value {
    let mut source = source();
    source["steps"]["second"] = source["steps"]["start"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("second");
    source
}

struct CompletedAction {
    pending: PendingAction,
    successor: Option<ActivationRequest>,
}

async fn completed_action(source: Value) -> CompletedAction {
    // Box the admission/action/provider seam to preserve stock 2 MiB test stacks.
    let (f, scope) = Box::pin(fixture_source(source)).await;
    let (f, request) = Box::pin(setup_action_on_fixture(
        f,
        scope,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    let verifier = ledger(&f, &request).await;
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    let RemoteDispatchObservation::Committed(receipt) =
        invoke(&f, &request, &provider).await.unwrap()
    else {
        panic!("actual provider return must commit its receipt")
    };
    assert_eq!(receipt.result, good());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let completed = f
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence: request.fence,
            output: receipt.result,
        })
        .await
        .unwrap()
        .unwrap();
    CompletedAction {
        pending: PendingAction {
            fixture: f,
            request,
            verifier,
        },
        successor: completed.successor,
    }
}

struct SuccessorAction {
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
}

async fn successor_action(completed: &CompletedAction) -> SuccessorAction {
    let f = &completed.pending.fixture;
    let scope = completed.successor.unwrap();
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    // The first action installed the validated dependency and fixture resources.
    // Prepare a new real intent against that same bundle, without rewriting history.
    let mut action = crate::application::workflow::actions::tests::request(ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    });
    action.context.actor = f.binding.target.actor.user_id();
    action.contract.policy.recovery = publication::ActionRecovery::Reconcile;
    action.arguments = json!({"value":2});
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(action)
        .await
        .unwrap();
    let request = ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: claim.fence,
        lease: policy(),
    };
    let authority = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap();
    let verifier = matched_verifier(f, &authority.action);
    sqlx::query(
        "INSERT INTO fixture_provider_operations(invocation_id,provider_key) VALUES($1,$2)",
    )
    .bind(request.subject.invocation.as_uuid())
    .bind(authority.action.idempotency_key().as_str())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let provider = LedgerProvider::new(f, Delivery::Pending);
    assert!(invoke(f, &request, &provider).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    park(f, &request).await;
    barrier(f, &request).await;
    SuccessorAction { request, verifier }
}

async fn history_command(
    pending: &PendingAction,
    request: &ActionDispatchRequest,
    key: &str,
) -> ReconcileActionCommand {
    let f = &pending.fixture;
    let marker: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_action_dispatches WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let mut command = command(f, request, marker).await;
    command.command_key = IdempotencyKey::parse(key).unwrap();
    command.input = EvidenceInput::VerifiedReference {
        registration: pending.verifier.registration().id().clone(),
        reference: EvidenceRecordReference::parse(key).unwrap(),
    };
    command
}

fn row<'a>(snapshot: &'a Value, table: &str, id: Uuid) -> &'a Value {
    snapshot[table]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(id))
        .unwrap()
}

fn completion_history(before: &Value, completed: &CompletedAction) {
    let first = completed.pending.request.fence.scope;
    let execution = row(before, "workflow_executions", first.execution.as_uuid());
    assert_eq!(execution["activation"], 1);
    assert!(execution["activated_at"].is_string());
    assert!(execution["completed_at"].is_string());
    assert_eq!(execution["committed_output"], good());
    assert_eq!(execution["committed_route"], "success");
    assert_eq!(
        row(before, "background_tasks", first.job.0)["status"],
        "completed"
    );
    assert_eq!(
        before["workflow_action_receipts"].as_array().unwrap().len(),
        1
    );
    match completed.successor {
        Some(second) => {
            assert_eq!(
                execution["successor_execution_id"],
                json!(second.execution.as_uuid())
            );
            assert_eq!(
                row(before, "workflow_executions", second.execution.as_uuid())["activation"],
                2
            );
            assert_eq!(before["workflow_runs"][0]["state"], "waiting");
            assert_eq!(
                before["workflow_runs"][0]["waiting_reason"],
                "reconciliation"
            );
        }
        None => {
            assert!(execution["successor_execution_id"].is_null());
            assert_eq!(before["workflow_runs"][0]["state"], "succeeded");
        }
    }
}

fn recorded_truth(
    before: &Value,
    after: &Value,
    command: &ReconcileActionCommand,
    outcome: &ReconciliationOutcome,
) {
    let saved = &after["workflow_action_evidence_commands"][0];
    assert_eq!(saved["command_key"], command.command_key.as_str());
    assert_eq!(saved["expected_revision"], command.expected_revision.0);
    assert_eq!(saved["outcome"], serde_json::to_value(outcome).unwrap());
    assert!(!saved["evidence_id"].is_null());
    assert_eq!(
        saved["result_revision"],
        after["workflow_runs"][0]["revision"]
    );
    assert!(after["workflow_runs"][0]["revision"].as_u64().unwrap() > command.expected_revision.0);
    let mut normalized = after.clone();
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_evidence_commands",
        "workflow_run_events",
    ] {
        assert_eq!(
            after[table].as_array().unwrap().len(),
            before[table].as_array().unwrap().len() + 1,
            "one new {table} fact"
        );
        for old in before[table].as_array().unwrap() {
            assert!(
                after[table].as_array().unwrap().contains(old),
                "retained {table}"
            );
        }
        normalized[table] = before[table].clone();
    }
    normalized["workflow_runs"][0]["revision"] = before["workflow_runs"][0]["revision"].clone();
    assert_eq!(
        &normalized, before,
        "only truth, command, actor audit and generated revision may change"
    );
}

async fn completed_refusal(source: Value) {
    let completed = Box::pin(completed_action(source)).await;
    if completed.successor.is_some() {
        Box::pin(successor_action(&completed)).await;
    }
    let pending = &completed.pending;
    let f = &pending.fixture;
    let before = all_tables(f).await;
    completion_history(&before, &completed);
    let command = history_command(pending, &pending.request, "completed-history").await;
    let expected = if completed.successor.is_some() {
        // The completed first job is rejected before evaluating the reopen predicate.
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob,
        }
    } else {
        // The terminal end-route run records truth at the earlier state boundary.
        ReconciliationOutcome::AppliedRecorded { receipt: true }
    };
    let result = run_command(f, &command, pending.verifier.clone())
        .await
        .unwrap();
    assert_eq!(result.outcome, expected);
    assert!(!result.replayed);
    let after = all_tables(f).await;
    recorded_truth(&before, &after, &command, &expected);
    assert!(after["workflow_action_evidence_commands"][0]["scheduled_job_id"].is_null());
    assert_eq!(
        after["workflow_action_evidence"][0]["disposition"],
        "applied"
    );
    let replay = run_command(f, &command, pending.verifier.clone())
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.outcome, expected);
    assert_eq!(all_tables(f).await, after);
    assert!(
        f.persistence()
            .claim_io(pending.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        all_tables(f).await,
        after,
        "completed subject creates no attempt or debit"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_completed_route_successor_history_is_retained()
{
    // Both reachable completion shapes retain their genuine invocation/marker/receipt.
    Box::pin(completed_refusal(two_steps())).await;
    Box::pin(completed_refusal(source())).await;
}

async fn restore_bound_control(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    after: &Value,
) {
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let eligible: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
            .bind(request.scope().company.as_uuid())
            .bind(request.scope().run.as_uuid())
            .bind(request.fence.scope.job.0)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(!eligible);
    sqlx::query("UPDATE workflow_runs SET max_steps=2 WHERE id=$1")
        .bind(request.scope().run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let eligible: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
            .bind(request.scope().company.as_uuid())
            .bind(request.scope().run.as_uuid())
            .bind(request.fence.scope.job.0)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(
        eligible,
        "changing only the mutable ordinal bound restores every reopen gate"
    );
    tx.rollback().await.unwrap();
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(&all_tables(f).await, after);
}

fn scheduled_history(
    before: &Value,
    after: &Value,
    request: &ActionDispatchRequest,
    first: WorkflowJobId,
) {
    let old = row(before, "background_tasks", request.fence.scope.job.0);
    let new = row(after, "background_tasks", request.fence.scope.job.0);
    assert_eq!(old["status"], "failed");
    assert_eq!(new["status"], "pending");
    for field in [
        "id",
        "payload",
        "workflow_execution_id",
        "retry_count",
        "max_retries",
        "worker_id",
        "execution_generation",
        "locked_at",
        "lock_expires_at",
    ] {
        assert_eq!(old[field], new[field], "retained target job {field}");
    }
    assert_eq!(
        row(before, "background_tasks", first.0),
        row(after, "background_tasks", first.0)
    );
    assert_eq!(after["workflow_runs"][0]["state"], "running");
}

fn matched_verifier(f: &AdmissionFixture, action: &FrozenAction) -> Arc<LedgerVerifier> {
    Arc::new(LedgerVerifier {
        persistence: f.persistence().clone(),
        registration: EvidenceVerifierRegistration::approve(
            EvidenceVerifierId::parse("fixture.ledger").unwrap(),
            EvidenceVerifierVersion::parse("v1").unwrap(),
            EvidenceProviderId::parse("fixture").unwrap(),
            &action.request().contract,
            &action.request().target,
        )
        .unwrap(),
    })
}

async fn max_steps_case(max_steps: i32) {
    let mut completed = Box::pin(completed_action(two_steps())).await;
    let successor = Box::pin(successor_action(&completed)).await;
    let request = successor.request;
    // Fixture action targets are execution-scoped; match the successor's real target.
    completed.pending.verifier = successor.verifier;
    let pending = &completed.pending;
    let f = &pending.fixture;
    let activated = all_tables(f).await;
    let execution = row(
        &activated,
        "workflow_executions",
        request.scope().execution.as_uuid(),
    );
    assert_eq!(execution["activation"], 2);
    assert!(execution["activated_at"].is_string());
    assert!(execution["completed_at"].is_null());
    assert!(activated["workflow_runs"][0]["max_steps"].as_i64().unwrap() >= 2);
    if max_steps == 1 {
        // Explicit malformed mutable run projection, after genuine ordinal-2 activation.
        // Frozen allowance, completion, action history and root accounting stay intact.
        sqlx::query("UPDATE workflow_runs SET max_steps=$2 WHERE company_id=$1")
            .bind(request.scope().company.as_uuid())
            .bind(max_steps)
            .execute(f.persistence().pool())
            .await
            .unwrap();
    }
    let before = all_tables(f).await;
    let mut normalized = before.clone();
    normalized["workflow_runs"][0]["max_steps"] =
        activated["workflow_runs"][0]["max_steps"].clone();
    normalized["workflow_runs"][0]["revision"] = activated["workflow_runs"][0]["revision"].clone();
    assert_eq!(
        normalized, activated,
        "projection setup changes only its bound and generated revision"
    );
    let command = history_command(pending, &request, "ordinal-two").await;
    let result = run_command(f, &command, pending.verifier.clone())
        .await
        .unwrap();
    let expected = if max_steps == 1 {
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::IneligibleJob,
        }
    } else {
        ReconciliationOutcome::Scheduled {
            receipt_only: false,
        }
    };
    assert_eq!(result.outcome, expected);
    let after = all_tables(f).await;
    let saved = &after["workflow_action_evidence_commands"][0];
    assert_eq!(saved["command_key"], command.command_key.as_str());
    assert_eq!(saved["expected_revision"], command.expected_revision.0);
    assert_eq!(saved["outcome"], serde_json::to_value(&expected).unwrap());
    assert_eq!(saved["result_revision"], result.revision.0);
    assert_eq!(
        after["workflow_action_evidence"][0]["disposition"],
        "final_not_applied"
    );
    assert_eq!(
        before["workflow_runs"][0]["bundle"],
        after["workflow_runs"][0]["bundle"]
    );
    retained_accounting(&before, &after);
    assert_eq!(entry_consumptions(f).await, (2, 0));
    assert_eq!(effects(f).await, 1);
    if max_steps == 1 {
        recorded_truth(&before, &after, &command, &expected);
        Box::pin(restore_bound_control(f, &request, &after)).await;
    } else {
        scheduled_history(&before, &after, &request, pending.request.fence.scope.job);
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_adversarial_max_steps_projection_blocks() {
    // Paired owner histories stay shallow without increasing libtest's stack bound.
    Box::pin(max_steps_case(1)).await;
    Box::pin(max_steps_case(2)).await;
}
