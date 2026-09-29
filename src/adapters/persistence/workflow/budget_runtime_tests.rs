use super::*;
use crate::adapters::persistence::workflow::budget::reserve_on;
use crate::application::workflow::{budget::*, polling::WorkflowPolling};

fn request(
    fence: WorkflowFence,
    resource: BudgetResource,
    key: &str,
    quantity: u32,
) -> BudgetReservation {
    BudgetReservation::new(
        fence,
        BudgetReservationKey::parse(key).unwrap(),
        BudgetCharge::new(resource, quantity).unwrap(),
    )
    .unwrap()
}
async fn limited() -> (AdmissionFixture, ActivationRequest) {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":2,"repetitions":2});
    fixture_source(source).await
}
async fn claim(f: &AdmissionFixture, scope: ActivationRequest) -> WorkflowFence {
    f.persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap()
        .fence
}
async fn accounting(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('usage',(SELECT jsonb_agg(to_jsonb(usage) ORDER BY root_run_id) FROM workflow_root_budget_usage AS usage),'receipts',(SELECT COALESCE(jsonb_agg(to_jsonb(receipt) ORDER BY run_id,execution_id,resource,reservation_key),'[]') FROM workflow_budget_receipts AS receipt))").fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_budget_runtime_duplicate_conflict_exact_and_exhausted_retry() {
    for resource in [BudgetResource::ModelCall, BudgetResource::Repetition] {
        let (f, scope) = limited().await;
        let fence = claim(&f, scope).await;
        let req = request(fence, resource, "op", 2);
        let barrier = Barrier::new(2);
        let reserve = || async {
            barrier.wait().await;
            f.persistence()
                .reserve_budget(req.clone(), policy())
                .await
                .unwrap()
        };
        let (a, b) = tokio::join!(reserve(), reserve());
        assert!(matches!(
            (a, b),
            (
                BudgetReservationResult::Recorded(BudgetDisposition::Granted),
                BudgetReservationResult::Replayed(BudgetDisposition::Granted)
            ) | (
                BudgetReservationResult::Replayed(BudgetDisposition::Granted),
                BudgetReservationResult::Recorded(BudgetDisposition::Granted)
            )
        ));
        let saved = accounting(&f).await;
        assert!(matches!(
            f.persistence()
                .reserve_budget(request(fence, resource, "op", 1), policy())
                .await,
            Err(AppError::Conflict(_))
        ));
        assert_eq!(accounting(&f).await, saved);
        let exhausted = request(fence, resource, "over", 1);
        assert_eq!(
            f.persistence()
                .reserve_budget(exhausted.clone(), policy())
                .await
                .unwrap(),
            BudgetReservationResult::Recorded(BudgetDisposition::Exhausted)
        );
        let retry_safe: bool = sqlx::query_scalar("SELECT workflow_control_retry_safe($1,$2,$3)")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .bind(scope.job.0)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
        assert!(!retry_safe);
        let state = snapshot(&f).await;
        assert_eq!(state["runs"][0]["state"], "failed");
        assert_eq!(
            state["attempts"][0]["workflow_failure_code"],
            "workflow.root_budget_exhausted"
        );
        assert_eq!(state["jobs"][0]["retry_count"], 1);
        assert_eq!(state["executions"].as_array().unwrap().len(), 1);
        assert_eq!(
            f.persistence()
                .reserve_budget(exhausted, policy())
                .await
                .unwrap(),
            BudgetReservationResult::Replayed(BudgetDisposition::Exhausted)
        );
        assert!(matches!(
            f.persistence()
                .retry(retry(command(&f, scope, "retry-budget").await))
                .await
                .unwrap(),
            RetryResult::Unsafe { .. }
        ));
        for _ in 0..2 {
            assert!(
                f.persistence()
                    .poll_work(None, 128)
                    .await
                    .unwrap()
                    .candidates
                    .is_empty()
            );
        }
        let saved = accounting(&f).await;
        assert_eq!(saved["receipts"].as_array().unwrap().len(), 3);
        assert_eq!(
            saved["usage"][0][if resource == BudgetResource::ModelCall {
                "model_calls"
            } else {
                "repetitions"
            }],
            2
        );
    }
}

#[tokio::test]
async fn workflow_budget_runtime_scope_fence_expiry_and_cancel() {
    let (f, scope) = limited().await;
    let fence = claim(&f, scope).await;
    let saved = accounting(&f).await;
    for bad in [
        WorkflowFence {
            worker: worker(),
            ..fence
        },
        WorkflowFence {
            generation: WorkflowGeneration(Uuid::new_v4()),
            ..fence
        },
        WorkflowFence {
            attempt: WorkflowAttempt(2),
            ..fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                company: CompanyId::new(Uuid::new_v4()),
                ..scope
            },
            ..fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                run: RunId::new(Uuid::new_v4()),
                ..scope
            },
            ..fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                execution: ExecutionId::new(Uuid::new_v4()),
                ..scope
            },
            ..fence
        },
        WorkflowFence {
            scope: ActivationRequest {
                job: WorkflowJobId(Uuid::new_v4()),
                ..scope
            },
            ..fence
        },
    ] {
        assert_eq!(
            f.persistence()
                .reserve_budget(request(bad, BudgetResource::ModelCall, "op", 1), policy())
                .await
                .unwrap(),
            BudgetReservationResult::NotOwned
        );
        assert_eq!(accounting(&f).await, saved);
    }
    let req = request(fence, BudgetResource::ModelCall, "op", 1);
    f.persistence()
        .reserve_budget(req.clone(), policy())
        .await
        .unwrap();
    expire(&f, scope).await;
    let saved = accounting(&f).await;
    assert_eq!(
        f.persistence()
            .reserve_budget(
                request(fence, BudgetResource::ModelCall, "fresh", 1),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::NotOwned
    );
    assert!(matches!(
        f.persistence()
            .cancel(command(&f, scope, "cancel-budget").await)
            .await
            .unwrap(),
        CancelResult::Applied { .. }
    ));
    assert_eq!(
        f.persistence().reserve_budget(req, policy()).await.unwrap(),
        BudgetReservationResult::Replayed(BudgetDisposition::Granted)
    );
    assert_eq!(
        f.persistence()
            .reserve_budget(
                request(fence, BudgetResource::ModelCall, "fresh", 1),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::NotOwned
    );
    assert_eq!(accounting(&f).await, saved);
}

#[tokio::test]
async fn workflow_budget_runtime_rollback_restart_and_safe_retry_preserve_charge() {
    let (f, scope) = limited().await;
    let fence = claim(&f, scope).await;
    let req = request(fence, BudgetResource::ModelCall, "op", 1);
    let saved = accounting(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    reserve_on(&mut tx, &req, policy()).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(accounting(&f).await, saved);
    let fresh = PostgresPersistence::new(f.persistence().pool().clone());
    assert_eq!(
        fresh.reserve_budget(req.clone(), policy()).await.unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    let fresh = PostgresPersistence::new(f.persistence().pool().clone());
    assert_eq!(
        fresh.reserve_budget(req.clone(), policy()).await.unwrap(),
        BudgetReservationResult::Replayed(BudgetDisposition::Granted)
    );
    let saved = accounting(&f).await;
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("script.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    });
    assert!(fresh.release_io(fence, policy(), cause).await.unwrap());
    assert!(matches!(
        fresh
            .retry(retry(command(&f, scope, "retry-safe").await))
            .await
            .unwrap(),
        RetryResult::Applied { .. }
    ));
    let next = claim(&f, scope).await;
    assert_ne!(next.generation, fence.generation);
    assert_eq!(
        fresh
            .reserve_budget(request(next, BudgetResource::ModelCall, "op", 1), policy())
            .await
            .unwrap(),
        BudgetReservationResult::Replayed(BudgetDisposition::Granted)
    );
    assert_eq!(accounting(&f).await, saved);
    assert_eq!(
        fresh
            .reserve_budget(
                request(next, BudgetResource::ModelCall, "second", 1),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
}

#[tokio::test]
async fn workflow_budget_runtime_conflicting_competitors_debit_one_payload() {
    let (f, scope) = limited().await;
    let fence = claim(&f, scope).await;
    let barrier = Barrier::new(2);
    let fixture = &f;
    let barrier = &barrier;
    let reserve = |quantity| async move {
        barrier.wait().await;
        fixture
            .persistence()
            .reserve_budget(
                request(fence, BudgetResource::ModelCall, "op", quantity),
                policy(),
            )
            .await
    };
    let (a, b) = tokio::join!(reserve(1), reserve(2));
    assert_ne!(a.is_ok(), b.is_ok());
    assert!(matches!(
        a.as_ref().err().or(b.as_ref().err()),
        Some(AppError::Conflict(_))
    ));
    let saved = accounting(&f).await;
    assert_eq!(saved["receipts"].as_array().unwrap().len(), 2);
    assert_eq!(
        saved["usage"][0]["model_calls"],
        saved["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .find(
                |receipt| receipt["resource"] == "model_call" && receipt["reservation_key"] == "op"
            )
            .unwrap()["quantity"]
    );
}

#[path = "budget_race_tests.rs"]
mod race_tests;
