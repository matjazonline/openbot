//! Coupled schema shapes and public refusal without invented action/attempt history.
use super::*;
use crate::application::workflow::{
    activation::WorkflowActivation, completion::WorkflowCompletion, lease::FencedWorkflowResult,
    polling::PollWork,
};
use tokio::sync::Barrier;

fn source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["input_schema"] =
        json!({"type":"object","required":["value"],"properties":{"value":true}});
    source["steps"]["start"]["with"]["value"] = json!({"ref":"/input/value"});
    source["steps"]["start"]["with"]["output_schema"] = json!({"literal":true});
    source
}

async fn rejected_shape(f: &AdmissionFixture, scope: ActivationRequest, assignment: &str) {
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let error = sqlx::query(&format!(
        "UPDATE workflow_executions SET {assignment} WHERE company_id=$1 AND run_id=$2 AND id=$3"
    ))
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap_err();
    constraint(error, "workflow_execution_output_shape");
    tx.rollback().await.unwrap();
    assert_eq!(before, all_tables(f).await, "output-shape rollback");
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_coupled_output_route_shapes() {
    // Admission has no activation, output, action or attempt to accidentally bypass a gate.
    let (f, scope) = Box::pin(fixture_source(source())).await;
    for assignment in [
        "committed_output='{}'::jsonb",
        "completed_at=clock_timestamp()",
        "completed_at=clock_timestamp(),committed_output='{}'::jsonb",
    ] {
        rejected_shape(&f, scope, assignment).await;
    }
    rejected_routes(&f, scope).await;
    f.persistence().activate(scope).await.unwrap();
    rejected_shape(&f, scope, "committed_output='{}'::jsonb").await;
    rejected_shape(&f, scope, "completed_at=clock_timestamp()").await;
    rejected_routes(&f, scope).await;
    Box::pin(completion_control()).await;
}

async fn rejected_routes(f: &AdmissionFixture, scope: ActivationRequest) {
    for successor in [false, true] {
        let before = all_tables(f).await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let next = if successor {
            // Satisfy the earlier same-run/step/next-ordinal progression trigger.
            Some(sqlx::query_scalar::<_, Uuid>("INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) SELECT company_id,run_id,gen_random_uuid(),step_id,activation+1 FROM workflow_executions WHERE id=$1 RETURNING id")
                .bind(scope.execution.as_uuid()).fetch_one(&mut *tx).await.unwrap())
        } else {
            None
        };
        let error = sqlx::query("UPDATE workflow_executions SET committed_route='success',route_target=CASE WHEN $2::uuid IS NULL THEN '$end' ELSE step_id END,successor_execution_id=$2 WHERE id=$1")
            .bind(scope.execution.as_uuid()).bind(next).execute(&mut *tx).await.unwrap_err();
        constraint(error, "workflow_route_shape");
        tx.rollback().await.unwrap();
        assert_eq!(before, all_tables(f).await, "route-shape full rollback");
    }
}

async fn completion_control() {
    // The existing claim/provider/completion owners create the complete valid shape.
    let (f, request) = Box::pin(limited()).await;
    ledger(&f, &request).await;
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
        panic!("genuine result receipt")
    };
    let result = f
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence: request.fence,
            output: receipt.result,
        })
        .await
        .unwrap()
        .unwrap();
    assert!(result.successor.is_none());
    let saved = all_tables(&f).await;
    let execution = &saved["workflow_executions"][0];
    assert!(execution["activated_at"].is_string());
    assert!(execution["completed_at"].is_string());
    assert_eq!(execution["committed_output"], good());
    assert_eq!(execution["committed_route"], "success");
    assert_eq!(execution["route_target"], "$end");
    assert!(execution["successor_execution_id"].is_null());
    assert_eq!(saved["background_tasks"][0]["status"], "completed");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_activation_immutable() {
    let pending = Box::pin(parked()).await;
    let before = all_tables(&pending.fixture).await;
    for assignment in [
        "frozen_inputs='{}'::jsonb",
        "activated_at=NULL,frozen_inputs=NULL",
    ] {
        let mut tx = pending.fixture.persistence().pool().begin().await.unwrap();
        let error = sqlx::query(&format!(
            "UPDATE workflow_executions SET {assignment} WHERE id=$1"
        ))
        .bind(pending.request.scope().execution.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap_err();
        constraint(error, "workflow_activation_immutable");
        tx.rollback().await.unwrap();
        assert_eq!(before, all_tables(&pending.fixture).await);
    }
    // The real immutable activation and retired attempt remain suitable after rejection.
    Box::pin(schedule_control(pending)).await;
}

fn no_action_history(saved: &Value) {
    assert_eq!(saved["workflow_executions"].as_array().unwrap().len(), 1);
    assert!(saved["workflow_executions"][0]["activated_at"].is_null());
    assert!(saved["workflow_executions"][0]["frozen_inputs"].is_null());
    for table in [
        "task_attempts",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
        "workflow_action_receipts",
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
        "workflow_action_evidence_consumptions",
        "workflow_action_claim_episodes",
        "workflow_budget_receipts",
    ] {
        assert!(
            saved[table].as_array().unwrap().is_empty(),
            "no invented {table}"
        );
    }
    for field in ["activations", "model_calls", "repetitions"] {
        assert_eq!(saved["workflow_root_budget_usage"][0][field], 0);
    }
}

async fn missing_action_boundary(
    f: &AdmissionFixture,
    scope: ActivationRequest,
    control: &PendingAction,
) {
    let before = all_tables(f).await;
    no_action_history(&before);
    // These are input identifiers from a separate genuine control, never facts on this
    // execution. Restore rejects the absent scoped intent before consulting the marker.
    let mut input = command(
        &control.fixture,
        &control.request,
        scoped_marker(&control.fixture, &control.request).await,
    )
    .await;
    input.actor = f.binding.target.actor;
    input.scope.company = scope.company;
    input.scope.run = scope.run;
    input.scope.execution = scope.execution;
    input.expected_revision = f
        .persistence()
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    let error = PostgresActionReconciliation::new(f.persistence().clone(), Resources)
        .snapshot(&input)
        .await
        .err()
        .expect("absent scoped intent cannot yield a reconciliation snapshot");
    assert!(matches!(error, AppError::NotFound(ref message) if message == "Workflow resource"));
    let safe: bool =
        sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .bind(scope.job.0)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert!(
        !safe,
        "coupled absent history predicate; no isolated activation claim"
    );
    assert_eq!(
        before,
        all_tables(f).await,
        "public rejection creates no evidence or scheduling"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_unactivated_has_no_scoped_action() {
    let (f, scope) = Box::pin(fixture_source(source())).await;
    sqlx::query("UPDATE workflow_runs SET input='{}'::jsonb WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = all_tables(&f).await;
    assert!(matches!(
        f.persistence().activate(scope).await,
        Err(AppError::BadRequest(_))
    ));
    assert_eq!(
        before,
        all_tables(&f).await,
        "invalid activation cannot charge or create history"
    );
    let control = Box::pin(parked()).await;
    missing_action_boundary(&f, scope, &control).await;
    Box::pin(schedule_control(control)).await;
}

fn retirement_delta(
    before: &Value,
    after: &Value,
    scope: ActivationRequest,
    retirement_transaction: &str,
) {
    let mut unchanged = after.clone();
    for field in ["status", "updated_at"] {
        unchanged["background_tasks"][0][field] = before["background_tasks"][0][field].clone();
    }
    for field in ["state", "terminal_execution_id", "revision"] {
        unchanged["workflow_runs"][0][field] = before["workflow_runs"][0][field].clone();
    }
    assert_eq!(
        after["workflow_run_events"].as_array().unwrap().len(),
        before["workflow_run_events"].as_array().unwrap().len() + 1
    );
    assert_eq!(
        after["workflow_run_events"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["event_kind"],
        "workflow.attempts_exhausted"
    );
    unchanged["workflow_run_events"] = before["workflow_run_events"].clone();
    let witnesses = after["workflow_action_state_witnesses"].as_array().unwrap();
    let previous = before["workflow_action_state_witnesses"]
        .as_array()
        .unwrap();
    assert_eq!(witnesses.len(), previous.len() + 1);
    assert!(previous.iter().all(|witness| witnesses.contains(witness)));
    let appended: Vec<_> = witnesses
        .iter()
        .filter(|witness| !previous.contains(witness))
        .collect();
    assert_eq!(
        appended,
        vec![&json!({
            "company_id": scope.company.as_uuid(),
            "run_id": scope.run.as_uuid(),
            "transaction_id": retirement_transaction,
            "initial_state": "queued",
            "initial_waiting_reason": null,
        })],
        "one exact queued-to-failed witness from the retirement transaction"
    );
    unchanged["workflow_action_state_witnesses"] =
        before["workflow_action_state_witnesses"].clone();
    assert_eq!(
        before, &unchanged,
        "only owner retirement fields, one audit and the verified state witness can change"
    );
}

async fn exhausted_retirement(f: &AdmissionFixture, scope: ActivationRequest) {
    let before = all_tables(f).await;
    no_action_history(&before);
    let page = f.persistence().poll_work(None, 128).await.unwrap();
    assert_eq!(page.candidates.len(), 1);
    assert_eq!(page.candidates[0].work, PollWork::ExhaustedPending);
    let barrier = Barrier::new(2);
    let retire = || async {
        barrier.wait().await;
        f.persistence()
            .retire_exhausted_work(scope, policy())
            .await
            .unwrap()
    };
    let (a, b) = tokio::join!(retire(), retire());
    assert_ne!(a, b, "exactly one normal owner retires the pending job");
    let after = all_tables(f).await;
    no_action_history(&after);
    assert_eq!(after["background_tasks"][0]["status"], "failed");
    assert_eq!(after["workflow_runs"][0]["state"], "failed");
    // xmin independently identifies the transaction that wrote the terminal owner row.
    let retirement_transaction: String =
        sqlx::query_scalar("SELECT xmin::text FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    retirement_delta(&before, &after, scope, &retirement_transaction);
    for table in [
        "workflow_executions",
        "workflow_root_budgets",
        "workflow_root_budget_usage",
        "workflow_budget_receipts",
    ] {
        assert_eq!(before[table], after[table]);
    }
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
    assert!(
        !f.persistence()
            .retire_exhausted_work(scope, policy())
            .await
            .unwrap()
    );
    assert!(
        f.persistence()
            .poll_work(None, 128)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    assert!(
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        after,
        all_tables(f).await,
        "retirement replay/poll/claim creates no attempt"
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_eligibility_exhausted_pending_has_no_retired_attempt() {
    let (f, scope) = Box::pin(fixture_source(source())).await;
    // Declared adversarial allowance projection, matching the established pending tests.
    sqlx::query("UPDATE background_tasks SET retry_count=max_retries WHERE id=$1")
        .bind(scope.job.0)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    exhausted_retirement(&f, scope).await;
    let control = Box::pin(parked()).await;
    missing_action_boundary(&f, scope, &control).await;
    let before = all_tables(&f).await;
    let result = f
        .persistence()
        .retry(RetryCommand {
            company_id: scope.company,
            run_id: scope.run,
            actor: f.binding.target.actor,
            command_key: IdempotencyKey::parse("exhausted-no-attempt").unwrap(),
            expected_revision: f
                .persistence()
                .head(scope.company, scope.run)
                .await
                .unwrap()
                .unwrap()
                .revision,
        })
        .await
        .unwrap();
    assert!(matches!(result, RetryResult::Unsafe { .. }));
    let mut after = all_tables(&f).await;
    assert_eq!(
        after["workflow_control_commands"].as_array().unwrap().len(),
        1
    );
    assert_eq!(after["workflow_control_commands"][0]["result"], "unsafe");
    after["workflow_control_commands"] = before["workflow_control_commands"].clone();
    assert_eq!(before, after, "ordinary retry records only its refusal");
    Box::pin(schedule_control(control)).await;
}
