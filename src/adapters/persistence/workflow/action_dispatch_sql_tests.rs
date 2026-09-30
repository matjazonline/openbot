use super::*;
use tokio::sync::Notify;

fn wrong_request(request: &ActionDispatchRequest, mode: usize) -> ActionDispatchRequest {
    let mut wrong = request.clone();
    match mode {
        0 => wrong.fence.scope.company = CompanyId::new(Uuid::new_v4()),
        1 => wrong.fence.scope.run = RunId::new(Uuid::new_v4()),
        2 => wrong.fence.scope.execution = ExecutionId::new(Uuid::new_v4()),
        3 => wrong.subject.invocation = ActionInvocationId::new(Uuid::new_v4()),
        4 => wrong.subject.argument_digest = serde_json::from_value(json!("0".repeat(64))).unwrap(),
        5 => wrong.fence.scope.job = WorkflowJobId(Uuid::new_v4()),
        6 => wrong.fence.attempt = WorkflowAttempt(99),
        7 => wrong.fence.generation = WorkflowGeneration(Uuid::new_v4()),
        _ => wrong.fence.worker = worker(),
    }
    wrong
}
async fn receipt(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    marker: Uuid,
    kind: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid()).bind(request.scope().execution.as_uuid())
        .bind(request.subject.invocation.as_uuid()).bind(request.subject.argument_digest.as_str()).bind(marker).bind(kind).bind(json!({"written":true})).execute(&mut **tx).await?;
    Ok(())
}
#[tokio::test]
async fn workflow_action_dispatch_sql_provenance_foreign_keys_and_receipt_linkage() {
    let (f, request) = setup().await;
    for mode in 0..9 {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let error = Dispatch::insert(
            &mut tx,
            &wrong_request(&request, mode),
            Uuid::new_v4(),
            "remote",
            json!({}),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&error, AppError::Database(message) if message.contains("foreign key")),
            "SQL itself must reject wrong provenance {mode}"
        );
        tx.rollback().await.unwrap();
        assert_eq!(facts(&f).await, (0, 0, 0));
    }
    for mode in 0..7 {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let marker = Uuid::new_v4();
        Dispatch::insert(&mut tx, &request, marker, "local", json!({}))
            .await
            .unwrap();
        let wrong = if mode < 5 {
            wrong_request(&request, mode)
        } else {
            request.clone()
        };
        let error = receipt(
            &mut tx,
            &wrong,
            if mode == 5 { Uuid::new_v4() } else { marker },
            if mode == 6 { "remote" } else { "local" },
        )
        .await
        .unwrap_err();
        let code = error.as_database_error().unwrap().code().unwrap();
        assert!(
            code == "23503" || (mode == 6 && code == "23514"),
            "unexpected SQL failure {error}"
        );
        tx.rollback().await.unwrap();
        assert_eq!(facts(&f).await, (0, 0, 0));
    }
}
#[tokio::test]
async fn workflow_action_dispatch_sql_deferred_run_deadline_rejects_effect_and_receipt() {
    let (f, request) = setup().await;
    let writer = adapter(&f, 0);
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let AuthorizedAction {
        action,
        current_policy: policy,
        ..
    } = writer.authorize_on(&mut tx, &request).await.unwrap();
    let marker = Uuid::new_v4();
    Dispatch::insert(&mut tx, &request, marker, "local", policy)
        .await
        .unwrap();
    Effect(0).apply(&mut tx, &action).await.unwrap();
    receipt(&mut tx, &request, marker, "local").await.unwrap();
    sqlx::query(
        "UPDATE workflow_runs SET created_at=clock_timestamp()-interval '10 seconds', deadline=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(request.scope().run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    let live: bool = sqlx::query_scalar(
        "SELECT lock_expires_at>clock_timestamp() FROM background_tasks WHERE id=$1",
    )
    .bind(request.fence.scope.job.0)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(
        live,
        "independent run deadline failure, while lease remains live"
    );
    assert!(tx.commit().await.is_err());
    assert_eq!(facts(&f).await, (0, 0, 0));
}

struct BlockedEffect<'a> {
    entered: &'a Notify,
    release: &'a Notify,
}
#[async_trait]
impl SqlLocalAction for BlockedEffect<'_> {
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        action: &FrozenAction,
    ) -> AppResult<Value> {
        let result = Effect(0).apply(tx, action).await?;
        self.entered.notify_one();
        self.release.notified().await;
        Ok(result)
    }
}
#[tokio::test]
async fn workflow_action_dispatch_revokers_wait_for_local_effect_and_receipt_commit() {
    for sql in [
        "UPDATE fixture_action_resources SET enabled=false WHERE company_id=$1",
        "UPDATE fixture_action_policies SET enabled=false WHERE company_id=$1",
        "DELETE FROM company_members WHERE company_id=$1",
    ] {
        let (f, request) = setup().await;
        let entered = Notify::new();
        let release = Notify::new();
        let writer = PostgresActionDispatch::new(
            f.persistence().clone(),
            Authority,
            BlockedEffect {
                entered: &entered,
                release: &release,
            },
        );
        let revoke = async {
            entered.notified().await;
            let mut tx = f.persistence().pool().begin().await.unwrap();
            sqlx::query(sql)
                .bind(request.scope().company.as_uuid())
                .execute(&mut *tx)
                .await
                .unwrap();
            assert_eq!(
                facts(&f).await,
                (1, 1, 1),
                "revoker cannot acquire authority lock until effect AND receipt commit"
            );
            tx.commit().await.unwrap();
        };
        let unblock = async {
            crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
                f.persistence().pool(),
                1,
            )
            .await;
            assert_eq!(
                facts(&f).await,
                (0, 0, 0),
                "uncommitted effect remains invisible while revoker waits"
            );
            release.notify_one();
        };
        let (result, (), ()) = tokio::join!(writer.local(&request), revoke, unblock);
        assert!(matches!(result.unwrap(), LocalDispatchResult::Committed(_)));
        assert_eq!(facts(&f).await, (1, 1, 1));
        assert!(writer.local(&request).await.is_err());
    }
}
#[tokio::test]
async fn workflow_action_dispatch_saved_model_tool_and_step_share_effect_wrong_digest_refused() {
    let (f, request) = setup().await;
    let saved = f
        .persistence()
        .action_authority(request.scope(), &request.subject)
        .await
        .unwrap();
    let intent = ActionService::new(f.persistence().clone())
        .prepare_tool(
            saved.action.request().clone(),
            ModelToolCallId::parse("dispatch-model-call").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(intent.approval_subject(), request.subject);
    let service = ActionService::new(adapter(&f, 0));
    assert!(
        service
            .dispatch_local(&wrong_request(&request, 4))
            .await
            .is_err()
    );
    assert_eq!(facts(&f).await, (0, 0, 0));
    let mut tool = request.clone();
    tool.subject = intent.approval_subject();
    let (step, tool) = tokio::join!(
        service.dispatch_local(&request),
        service.dispatch_local(&tool)
    );
    for result in [step, tool] {
        assert!(matches!(result.unwrap(), LocalDispatchResult::Committed(_)));
    }
    assert_eq!(facts(&f).await, (1, 1, 1));
    let fresh = ActionService::new(f.persistence().clone())
        .prepare_tool(
            saved.action.request().clone(),
            ModelToolCallId::parse("dispatch-model-call").unwrap(),
        )
        .await
        .unwrap();
    assert!(fresh.replayed);
    assert_eq!(fresh.approval_subject(), request.subject);
    assert!(
        service
            .dispatch_local(&wrong_request(&request, 4))
            .await
            .is_err()
    );
    assert_eq!(facts(&f).await, (1, 1, 1));
}
