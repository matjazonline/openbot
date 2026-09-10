use super::*;

async fn receipt_versions(fixture: &Fixture, run: &RunCheckpoint) -> Vec<(Uuid, String)> {
    sqlx::query_as("SELECT id, xmin::text FROM task_harness_invocations WHERE run_id = $1 ORDER BY call_ordinal")
        .bind(run.run_id.0).fetch_all(&fixture.persistence.pool).await.unwrap()
}

#[tokio::test]
async fn checkpoint_writes_batch_receipts_and_leave_unchanged_rows_untouched() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _claim_guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let fixture = Fixture::new(pool).await;
    let original = applied(
        fixture
            .persistence
            .open_run(fixture.request())
            .await
            .unwrap(),
    );
    let (mut run, second_id) = batch_checkpoint(&original);
    // One snapshot with multiple new receipts exercises the batched insert path.
    let mut tx = fixture.persistence.pool.begin().await.unwrap();
    assert!(
        lock_task_execution_on(&mut tx, fixture.identity.company_id, fixture.lease)
            .await
            .unwrap()
    );
    let locked = load_on(
        &mut tx,
        fixture.identity.company_id,
        fixture.lease.task_id,
        run.run_id,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(locked, original);
    persist_checkpoint_on(
        &mut tx,
        fixture.identity.company_id,
        fixture.lease.task_id,
        &locked,
        &run,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let before = receipt_versions(&fixture, &run).await;
    assert_eq!(before.len(), 2);
    run = applied(
        fixture
            .persistence
            .record_result(&write(&fixture, &run), second_id, json!({"second":true}))
            .await
            .unwrap(),
    );
    let after = receipt_versions(&fixture, &run).await;
    assert_eq!(before[0], after[0], "completed receipt was rewritten");
    assert_ne!(before[1].1, after[1].1, "changed receipt was not updated");
    run = applied(
        fixture
            .persistence
            .reserve_model(&write(&fixture, &run), reservation())
            .await
            .unwrap(),
    );
    assert_eq!(
        after,
        receipt_versions(&fixture, &run).await,
        "checkpoint-only write touched receipts"
    );
    let receipts: Vec<sqlx::types::Json<SavedInvocation>> = sqlx::query_scalar(
        "SELECT invocation FROM task_harness_invocations WHERE run_id = $1 ORDER BY call_ordinal",
    )
    .bind(run.run_id.0)
    .fetch_all(&fixture.persistence.pool)
    .await
    .unwrap();
    assert_eq!(
        receipts.into_iter().map(|row| row.0).collect::<Vec<_>>(),
        run.invocations
    );
    fixture.cleanup().await;
}

fn batch_checkpoint(original: &RunCheckpoint) -> (RunCheckpoint, InvocationId) {
    let budget = reservation();
    let ids = [InvocationId(Uuid::new_v4()), InvocationId(Uuid::new_v4())];
    let mut run = original.apply(Mutation::Reserve(budget.clone())).unwrap().0;
    run = run
        .apply(Mutation::Model(SavedModelTurn {
            token_usage_source: TokenUsageSource::Reported,
            invalid_response: None,
            request_id: budget.request_id,
            text: String::new(),
            calls: ids
                .iter()
                .enumerate()
                .map(|(ordinal, id)| SavedToolCall {
                    invocation_id: *id,
                    call_id: format!("call_{ordinal}"),
                    item_id: None,
                    tool_id: "lookup".into(),
                    arguments: json!({}),
                })
                .collect(),
            continuation: vec![],
            input_tokens: 70,
            output_tokens: 50,
        }))
        .unwrap()
        .0;
    run = run.apply(Mutation::Prepare(ids[0])).unwrap().0;
    run = run
        .apply(Mutation::Result(ids[0], json!({"first":true})))
        .unwrap()
        .0;
    run = run.apply(Mutation::Prepare(ids[1])).unwrap().0;
    (run, ids[1])
}
