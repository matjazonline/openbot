use super::*;
use crate::adapters::persistence::workflow::{action_uncertainty, pending_recovery};
use crate::application::workflow::{
    budget::*, completion::WorkflowCompletion, polling::WorkflowPolling,
};
use crate::domain::workflow::{
    BudgetCharge, BudgetResource, FailureClass, FailureCode, RetrySafety, StepFailure,
};

fn classified() -> LeaseReleaseCause {
    LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("provider.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    })
}
async fn state(f: &AdmissionFixture) -> String {
    sqlx::query_scalar("SELECT effect_state FROM workflow_action_effect_states")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}
async fn audit(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(observation)),'[]') FROM workflow_action_reconciliations AS observation")
        .fetch_one(f.persistence().pool()).await.unwrap()
}
async fn parked(f: &AdmissionFixture, request: &ActionDispatchRequest, code: &str, class: &str) {
    let saved = snapshot(f).await;
    assert_eq!(saved["runs"][0]["state"], "waiting");
    assert_eq!(saved["runs"][0]["waiting_reason"], "reconciliation");
    assert_eq!(saved["jobs"][0]["status"], "failed");
    assert_eq!(saved["jobs"][0]["retry_count"], 1);
    assert_eq!(saved["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(saved["attempts"][0]["workflow_failure_code"], code);
    assert_eq!(saved["attempts"][0]["workflow_failure_class"], class);
    assert_eq!(saved["attempts"][0]["workflow_retry_safety"], "unknown");
    assert_eq!(saved["executions"][0]["committed_output"], Value::Null);
    assert_eq!(
        saved["executions"][0]["successor_execution_id"],
        Value::Null
    );
    assert_eq!(state(f).await, "needs_reconciliation");
    let observations = audit(f).await;
    assert_eq!(observations.as_array().unwrap().len(), 1);
    assert_eq!(observations[0]["failure_code"], code);
    assert_eq!(
        observations[0]["invocation_id"],
        request.subject.invocation.as_uuid().to_string()
    );
    let (a, b) = tokio::join!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy()),
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
    );
    assert!(a.unwrap().is_none() && b.unwrap().is_none());
    assert_eq!(snapshot(f).await, saved);
}

#[tokio::test]
async fn workflow_action_uncertainty_actual_effect_errors_and_exact_retirement() {
    for mode in 0..6 {
        let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, true).await;
        let mut provider = dedup(&f, contract, mode != 1);
        if mode == 1 {
            provider.result = json!("invalid response after applied write");
        }
        assert!(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(effect_count(&f).await, 1);
        assert_eq!(state(&f).await, "possible_dispatch");
        let (code, class) = match mode {
            0 | 1 => {
                let (a, b) = tokio::join!(
                    f.persistence()
                        .release_io(request.fence, policy(), classified()),
                    f.persistence()
                        .release_io(request.fence, policy(), classified())
                );
                assert_eq!(usize::from(a.unwrap()) + usize::from(b.unwrap()), 1);
                ("provider.rejected", "terminal")
            }
            2 => {
                assert!(
                    f.persistence()
                        .complete_io(FencedWorkflowResult {
                            fence: request.fence,
                            output: json!("invalid handler output")
                        })
                        .await
                        .unwrap()
                        .is_none()
                );
                ("workflow.invalid_result", "terminal")
            }
            3 => {
                assert!(
                    f.persistence()
                        .release_io(request.fence, policy(), LeaseReleaseCause::Deadline)
                        .await
                        .unwrap()
                );
                ("workflow.operation_deadline", "retryable")
            }
            4 => {
                assert!(
                    f.persistence()
                        .release_io(
                            request.fence,
                            policy(),
                            LeaseReleaseCause::LocalInterruption
                        )
                        .await
                        .unwrap()
                );
                ("workflow.interrupted", "retryable")
            }
            _ => {
                expire(&f, request.fence.scope).await;
                assert!(
                    f.persistence()
                        .claim_io(request.fence.scope, worker(), policy())
                        .await
                        .unwrap()
                        .is_none()
                );
                ("workflow.lease_expired", "retryable")
            }
        };
        parked(&f, &request, code, class).await;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(facts(&f).await, (1, 0, 0));
        assert!(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn workflow_action_uncertainty_root_budget_overrides_read_handler_default() {
    let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, true).await;
    let provider = dedup(&f, contract, true);
    assert!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .is_err()
    );
    let reservation = BudgetReservation::new(
        request.fence,
        BudgetReservationKey::parse("over").unwrap(),
        BudgetCharge::new(BudgetResource::ModelCall, 1000).unwrap(),
    )
    .unwrap();
    assert_eq!(
        f.persistence()
            .reserve_budget(reservation, policy())
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    let reservation = BudgetReservation::new(
        request.fence,
        BudgetReservationKey::parse("over-again").unwrap(),
        BudgetCharge::new(BudgetResource::ModelCall, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        f.persistence()
            .reserve_budget(reservation, policy())
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Exhausted)
    );
    parked(&f, &request, "workflow.root_budget_exhausted", "terminal").await;
    assert_eq!(effect_count(&f).await, 1);
}

#[tokio::test]
async fn workflow_action_uncertainty_pending_failures_consult_expired_action_proof() {
    use pending_recovery::PendingFailure::*;
    for failure in [
        InvalidInput,
        InvalidOutput,
        ActivationLimit,
        AttemptsExhausted,
        RootBudgetExhausted,
    ] {
        let (f, request, contract) =
            receipt_fixture(ActionRecovery::ProviderIdempotency, true).await;
        let provider = dedup(&f, contract, true);
        assert!(
            ActionService::new(adapter(&f, 0))
                .dispatch_remote(&request, &provider, &CancellationToken::new())
                .await
                .is_err()
        );
        expire(&f, request.fence.scope).await;
        assert!(
            f.persistence()
                .claim_io(request.fence.scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(snapshot(&f).await["jobs"][0]["status"], "pending");
        // Retention no longer spans the owning run horizon. Historical safety is
        // re-evaluated before every pending failure, even on this read-kind handler.
        sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '2 years'")
            .execute(f.persistence().pool())
            .await
            .unwrap();
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs FOR UPDATE")
            .execute(&mut *tx)
            .await
            .unwrap();
        pending_recovery::settle(&mut tx, request.fence.scope, failure)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let saved = snapshot(&f).await;
        assert_eq!(saved["runs"][0]["state"], "waiting");
        assert_eq!(saved["jobs"][0]["retry_count"], 1);
        assert_eq!(saved["attempts"].as_array().unwrap().len(), 1);
        assert_eq!(
            saved["executions"][0]["successor_execution_id"],
            Value::Null
        );
        assert_eq!(state(&f).await, "needs_reconciliation");
        assert_eq!(audit(&f).await.as_array().unwrap().len(), 1);
    }
}

async fn cancel(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let command = CancelCommand {
        company_id: request.scope().company,
        run_id: request.scope().run,
        actor: f.binding.target.actor,
        command_key: IdempotencyKey::parse("uncertainty-cancel").unwrap(),
        expected_revision: f
            .persistence()
            .head(request.scope().company, request.scope().run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    assert!(matches!(
        f.persistence().cancel(command).await.unwrap(),
        CancelResult::Applied { .. }
    ));
}

#[tokio::test]
async fn workflow_action_uncertainty_parked_deadline_cancel_and_late_receipt_truth() {
    for cancelled in [false, true] {
        let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, false).await;
        let provider = dedup(&f, contract, false);
        let writer = adapter(&f, 0);
        let RemoteReservationResult::Reserved(reservation) =
            writer.reserve_remote(&request, None).await.unwrap()
        else {
            panic!("reserved")
        };
        let entry = writer.enter_remote(*reservation).await.unwrap();
        let result = provider
            .invoke(&entry.action, &entry.provider)
            .await
            .unwrap();
        assert!(
            f.persistence()
                .release_io(request.fence, policy(), classified())
                .await
                .unwrap()
        );
        let observations = audit(&f).await;
        if cancelled {
            cancel(&f, &request).await;
        } else {
            sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute'").execute(f.persistence().pool()).await.unwrap();
            let page = f.persistence().poll_work(None, 10).await.unwrap();
            assert_eq!(page.candidates.len(), 1);
            assert_eq!(page.candidates[0].scope, request.fence.scope);
            assert!(
                f.persistence()
                    .expire_run(request.fence.scope)
                    .await
                    .unwrap()
            );
            assert!(
                !f.persistence()
                    .expire_run(request.fence.scope)
                    .await
                    .unwrap()
            );
            assert!(
                f.persistence()
                    .poll_work(None, 10)
                    .await
                    .unwrap()
                    .candidates
                    .is_empty()
            );
        }
        assert_eq!(audit(&f).await, observations);
        assert_eq!(snapshot(&f).await["jobs"][0]["retry_count"], 1);
        assert!(matches!(
            writer.finish_remote(entry, result).await.unwrap(),
            RemoteDispatchObservation::Interrupted
        ));
        assert_eq!(state(&f).await, "committed");
        assert_eq!(audit(&f).await, observations);
        let saved = snapshot(&f).await;
        assert_eq!(
            saved["runs"][0]["state"],
            if cancelled { "cancelled" } else { "failed" }
        );
        assert_eq!(saved["executions"][0]["committed_output"], Value::Null);
        assert_eq!(
            saved["executions"][0]["successor_execution_id"],
            Value::Null
        );
    }
}

#[tokio::test]
async fn workflow_action_uncertainty_deferred_commit_failure_rolls_back_retirement() {
    let (f, request, contract) = receipt_fixture(ActionRecovery::Reconcile, true).await;
    let provider = dedup(&f, contract, true);
    assert!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .is_err()
    );
    let saved = snapshot(&f).await;
    sqlx::raw_sql("CREATE FUNCTION uncertainty_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected deferred fault'; END $$; CREATE CONSTRAINT TRIGGER uncertainty_fault AFTER INSERT ON workflow_action_reconciliations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION uncertainty_fault();").execute(f.persistence().pool()).await.unwrap();
    assert!(
        f.persistence()
            .release_io(request.fence, policy(), classified())
            .await
            .is_err()
    );
    assert_eq!(snapshot(&f).await, saved);
    assert_eq!(audit(&f).await, json!([]));
    assert_eq!(state(&f).await, "possible_dispatch");
    assert_eq!(effect_count(&f).await, 1);
}

#[path = "action_uncertainty_acceptance_tests.rs"]
mod acceptance_tests;
