use super::*;

#[tokio::test]
async fn workflow_budget_runtime_unknown_effect_exhaustion_reconciles() {
    let mut source: Value =
        serde_json::from_str(&registry::example("memory.save").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":1,"repetitions":1});
    let (f, scope) = fixture_source(source).await;
    let fence = claim(&f, scope).await;
    f.persistence()
        .reserve_budget(
            request(fence, BudgetResource::ModelCall, "ambiguous", 1),
            policy(),
        )
        .await
        .unwrap();
    assert_eq!(
        f.persistence()
            .reserve_budget(
                request(fence, BudgetResource::ModelCall, "over", 1),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Exhausted)
    );
    let saved = snapshot(&f).await;
    assert_eq!(saved["runs"][0]["state"], "waiting");
    assert_eq!(saved["runs"][0]["waiting_reason"], "reconciliation");
    assert_eq!(saved["attempts"][0]["workflow_retry_safety"], "unknown");
    assert_eq!(saved["executions"].as_array().unwrap().len(), 1);
    assert!(matches!(
        f.persistence()
            .retry(retry(command(&f, scope, "retry-unknown-budget").await))
            .await
            .unwrap(),
        RetryResult::Unsafe { .. }
    ));
    assert_eq!(accounting(&f).await["usage"][0]["model_calls"], 1);
}

#[tokio::test]
async fn workflow_budget_runtime_usage_wait_crossing_lease_or_deadline_rolls_back() {
    for deadline in [false, true] {
        for consumed in [false, true] {
            let (f, scope) = limited().await;
            let fence = claim(&f, scope).await;
            if consumed {
                f.persistence()
                    .reserve_budget(
                        request(fence, BudgetResource::ModelCall, "used", 2),
                        policy(),
                    )
                    .await
                    .unwrap();
            }
            let mut held = f.persistence().pool().begin().await.unwrap();
            sqlx::query("SELECT root_run_id FROM workflow_root_budget_usage FOR UPDATE")
                .execute(&mut *held)
                .await
                .unwrap();
            let sql = if deadline {
                "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '250 milliseconds' WHERE id=$1"
            } else {
                "UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '250 milliseconds' WHERE id=$1"
            };
            sqlx::query(sql)
                .bind(if deadline {
                    scope.run.as_uuid()
                } else {
                    scope.job.0
                })
                .execute(f.persistence().pool())
                .await
                .unwrap();
            let before = accounting(&f).await;
            let req = request(fence, BudgetResource::ModelCall, "blocked", 1);
            let reserve = f.persistence().reserve_budget(req, policy());
            tokio::pin!(reserve);
            tokio::select! { result=&mut reserve => panic!("reservation must block on shared usage: {result:?}"), _=tokio::time::sleep(Duration::from_millis(350)) => {} }
            held.rollback().await.unwrap();
            assert!(reserve.await.is_err());
            assert_eq!(accounting(&f).await, before);
        }
    }
}

#[tokio::test]
async fn workflow_budget_runtime_deferred_commit_failure_rolls_back_grant_and_refusal() {
    for consumed in [false, true] {
        let (f, scope) = limited().await;
        let fence = claim(&f, scope).await;
        if consumed {
            f.persistence()
                .reserve_budget(
                    request(fence, BudgetResource::ModelCall, "used", 2),
                    policy(),
                )
                .await
                .unwrap();
        }
        sqlx::raw_sql("CREATE FUNCTION reject_budget_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected budget commit'; END $$; CREATE CONSTRAINT TRIGGER reject_budget_commit AFTER INSERT ON workflow_budget_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_budget_commit()").execute(f.persistence().pool()).await.unwrap();
        let before = accounting(&f).await;
        let state = snapshot(&f).await;
        assert!(
            f.persistence()
                .reserve_budget(
                    request(fence, BudgetResource::ModelCall, "lost", 1),
                    policy()
                )
                .await
                .is_err()
        );
        assert_eq!(accounting(&f).await, before);
        assert_eq!(snapshot(&f).await, state);
    }
}

#[tokio::test]
async fn workflow_budget_runtime_reservation_competes_with_cancel_and_completion() {
    for cancel in [false, true] {
        let (f, scope) = limited().await;
        let fence = claim(&f, scope).await;
        let cmd = command(&f, scope, "cancel-race-budget").await;
        let barrier = Barrier::new(2);
        let reserve = async {
            barrier.wait().await;
            f.persistence()
                .reserve_budget(
                    request(fence, BudgetResource::ModelCall, "racing", 1),
                    policy(),
                )
                .await
                .unwrap()
        };
        let finish = async {
            barrier.wait().await;
            if cancel {
                assert!(matches!(
                    f.persistence().cancel(cmd).await.unwrap(),
                    CancelResult::Applied { .. }
                ));
            } else {
                assert!(
                    f.persistence()
                        .complete_io(FencedWorkflowResult {
                            fence,
                            output: json!({"items":[],"token_count":0})
                        })
                        .await
                        .unwrap()
                        .is_some()
                );
            }
        };
        let (result, ()) = tokio::join!(reserve, finish);
        let charged = match result {
            BudgetReservationResult::Recorded(BudgetDisposition::Granted) => 1,
            BudgetReservationResult::NotOwned => 0,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(accounting(&f).await["usage"][0]["model_calls"], charged);
        assert_eq!(
            snapshot(&f).await["runs"][0]["state"],
            if cancel { "cancelled" } else { "succeeded" }
        );
        assert_eq!(
            f.persistence()
                .reserve_budget(
                    request(fence, BudgetResource::ModelCall, "after", 1),
                    policy()
                )
                .await
                .unwrap(),
            BudgetReservationResult::NotOwned
        );
    }
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
    let command = f
        .prepare(f.request(&format!("budget-child-{}", parent.run.as_uuid()), trigger))
        .await;
    f.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    ActivationRequest {
        company: parent.company,
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    }
}

#[tokio::test]
async fn workflow_budget_runtime_descendants_share_without_locking_root_or_parent() {
    let (f, root) = limited().await;
    let parent = child(&f, root).await;
    let descendant = child(&f, parent).await;
    let parent_fence = claim(&f, parent).await;
    let fence = claim(&f, descendant).await;
    f.persistence()
        .reserve_budget(
            request(parent_fence, BudgetResource::ModelCall, "parent", 1),
            policy(),
        )
        .await
        .unwrap();
    let mut held = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id IN ($1,$2) FOR UPDATE")
        .bind(root.run.as_uuid())
        .bind(parent.run.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE id IN ($1,$2) FOR UPDATE")
        .bind(root.execution.as_uuid())
        .bind(parent.execution.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    assert_eq!(
        f.persistence()
            .reserve_budget(
                request(fence, BudgetResource::ModelCall, "child", 1),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    assert_eq!(
        f.persistence()
            .reserve_budget(
                request(fence, BudgetResource::Repetition, "child", 2),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    assert_eq!(
        f.persistence()
            .reserve_budget(
                request(fence, BudgetResource::ModelCall, "over", 1),
                policy()
            )
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Exhausted)
    );
    held.rollback().await.unwrap();
    let saved = accounting(&f).await;
    assert_eq!(saved["usage"].as_array().unwrap().len(), 1);
    assert_eq!(saved["usage"][0]["model_calls"], 2);
    assert_eq!(saved["usage"][0]["repetitions"], 2);
    assert_eq!(
        f.persistence()
            .head(root.company, root.run)
            .await
            .unwrap()
            .unwrap()
            .state,
        RunState::Queued
    );
}
