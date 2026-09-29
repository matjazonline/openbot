use super::*;

fn chain() -> Value {
    let mut source = wait_source(false);
    source["steps"]["wait"] = source["steps"]["start"].clone();
    let context: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["steps"]["context"] = context["steps"]["start"].clone();
    source["steps"]["context"]["routes"]["success"] = json!("wait");
    source["steps"]["recover"] = context["steps"]["start"].clone();
    source["steps"]["recover"]["routes"]["success"] = json!("finish");
    source["steps"]["wait"]["routes"]["success"] = json!("recover");
    source["steps"]["start"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":true}},"routes":{"success":"context"}});
    source
}

async fn compete(p: &PostgresPersistence, h: &Scripted, candidate: PollCandidate) {
    let left = PostgresPersistence::new(p.pool().clone());
    let right = PostgresPersistence::new(p.pool().clone());
    let a = polling(&left, h);
    let b = polling(&right, h);
    let barrier = Barrier::new(2);
    let cancel = CancellationToken::new();
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            a.process(candidate, &cancel).await
        },
        async {
            barrier.wait().await;
            b.process(candidate, &cancel).await
        }
    );
    a.unwrap();
    b.unwrap();
}

fn assert_recovered(saved: Value, scope: ActivationRequest, frozen: Value, calls: usize) {
    assert_eq!(calls, 2);
    assert_eq!(saved["executions"].as_array().unwrap().len(), 5);
    assert_eq!(saved["attempts"].as_array().unwrap().len(), 3);
    let recovered = saved["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(scope.execution.as_uuid()))
        .unwrap();
    assert_eq!(recovered["frozen_inputs"], frozen);
}

async fn assert_no_work(p: &PostgresPersistence) {
    assert!(p.poll_work(None, 128).await.unwrap().candidates.is_empty());
}

#[tokio::test]
async fn workflow_phase03_admission_wait_retry_restart_combined() {
    let (f, scope) = fixture_source(chain()).await;
    let p = f.persistence();
    let handler = scripted();
    let runner = polling(p, &handler);
    let wakeup = Notify::new();
    let cancel = CancellationToken::new();
    tokio::join!(runner.run(&wakeup, &cancel), async {
        until_state(p, scope, "waiting").await;
        cancel.cancel();
    });
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    let row: (Uuid, Uuid) = sqlx::query_as("SELECT wait.execution_id, job.id FROM workflow_waits AS wait JOIN background_tasks AS job ON job.workflow_execution_id=wait.execution_id WHERE wait.run_id=$1")
        .bind(scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let waiting = ActivationRequest {
        execution: ExecutionId::new(row.0),
        job: WorkflowJobId(row.1),
        ..scope
    };
    let event = signal(waiting);
    assert!(p.record_signal(event.clone()).await.unwrap());
    assert!(p.record_signal(event).await.unwrap());
    let fresh = PostgresPersistence::new(p.pool().clone());
    compete(
        &fresh,
        &handler,
        fresh.poll_work(None, 2).await.unwrap().candidates[0],
    )
    .await;
    let candidate = fresh.poll_work(None, 2).await.unwrap().candidates[0];
    let failure = super::integration_tests::FailingHandler(AtomicUsize::new(0));
    let failed = WorkflowWorker {
        port: &fresh,
        handler: &failure,
        worker: worker(),
        lease: policy(),
        poll: PollPolicy::new(Duration::from_millis(50), 2).unwrap(),
    };
    failed
        .process(candidate, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(failure.0.load(Ordering::SeqCst), 1);
    for _ in 0..2 {
        assert_no_work(&fresh).await;
    }
    let frozen: Value =
        sqlx::query_scalar("SELECT frozen_inputs FROM workflow_executions WHERE id=$1")
            .bind(candidate.scope.execution.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    due(&f, candidate.scope).await;
    let restarted = PostgresPersistence::new(p.pool().clone());
    compete(
        &restarted,
        &handler,
        restarted.poll_work(None, 2).await.unwrap().candidates[0],
    )
    .await;
    let final_worker = polling(&restarted, &handler);
    let cancel = CancellationToken::new();
    tokio::join!(final_worker.run(&wakeup, &cancel), async {
        until_state(&restarted, scope, "succeeded").await;
        cancel.cancel();
    });
    assert_recovered(
        snapshot(&f).await,
        candidate.scope,
        frozen,
        handler.calls.load(Ordering::SeqCst),
    );
    assert_no_work(&restarted).await;
}
