use super::*;

struct FailingHandler(AtomicUsize);
#[async_trait]
impl WorkflowHandler for FailingHandler {
    fn supports(&self, _: &WorkflowStepKind) -> bool {
        true
    }
    async fn execute(&self, _: &WorkflowStepKind, _: &ClaimedWorkflow) -> AppResult<Value> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(AppError::Conflict("scripted temporary failure".into()))
    }
}
#[tokio::test]
async fn workflow_poll_failed_batch_is_not_immediately_reclaimed() {
    let (f, _) = fixture().await;
    let p = f.persistence();
    let handler = FailingHandler(AtomicUsize::new(0));
    let runner = WorkflowWorker {
        port: p,
        handler: &handler,
        worker: worker(),
        lease: policy(),
        poll: PollPolicy::new(Duration::from_millis(50), 1).unwrap(),
    };
    let cancel = CancellationToken::new();
    let first = p.poll_work(None, 1).await.unwrap();
    runner.process(first.candidates[0], &cancel).await.unwrap();
    assert!(p.poll_work(None, 1).await.unwrap().candidates.is_empty());
    assert!(
        p.poll_work(first.next, 1)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_poll_complete_chain_and_competing_wait_consumption() {
    let mut source = wait_source(false);
    source["steps"]["wait"] = source["steps"]["start"].clone();
    source["steps"]["wait"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(5)).to_rfc3339()});
    let context: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["steps"]["context"] = context["steps"]["start"].clone();
    source["steps"]["context"]["routes"]["success"] = json!("wait");
    source["steps"]["start"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":true}},"routes":{"success":"context"}});
    let (f, scope) = fixture_source(source).await;
    let p = f.persistence();
    let handler = scripted();
    let runner = polling(p, &handler);
    let wakeup = Notify::new();
    let cancel = CancellationToken::new();
    tokio::join!(runner.run(&wakeup, &cancel), async {
        until_state(p, scope, "waiting").await;
        cancel.cancel();
    });
    let row:(Uuid,Uuid)=sqlx::query_as("SELECT wait.execution_id,job.id FROM workflow_waits AS wait JOIN background_tasks AS job ON job.workflow_execution_id=wait.execution_id WHERE wait.run_id=$1")
        .bind(scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let waiting = ActivationRequest {
        execution: ExecutionId::new(row.0),
        job: WorkflowJobId(row.1),
        ..scope
    };
    p.record_signal(signal(waiting)).await.unwrap();
    let candidate = p.poll_work(None, 2).await.unwrap().candidates[0];
    assert_eq!(candidate.work, PollWork::Wait);
    let left = polling(p, &handler);
    let right = polling(p, &handler);
    let cancel = CancellationToken::new();
    let barrier = Barrier::new(2);
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            left.process(candidate, &cancel).await
        },
        async {
            barrier.wait().await;
            right.process(candidate, &cancel).await
        }
    );
    a.unwrap();
    b.unwrap();
    tokio::join!(runner.run(&wakeup, &cancel), async {
        until_state(p, scope, "succeeded").await;
        cancel.cancel();
    });
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE run_id=$1")
        .bind(scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 4);
}

#[tokio::test]
async fn workflow_poll_unsupported_expired_lease_retires_once_without_fresh_claim() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    p.claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    expire(&f, scope).await;
    let handler = Scripted {
        calls: AtomicUsize::new(0),
        supported: false,
    };
    let runner = polling(p, &handler);
    let cancel = CancellationToken::new();
    let candidate = p.poll_work(None, 1).await.unwrap().candidates[0];
    let barrier = Barrier::new(2);
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            runner.process(candidate, &cancel).await
        },
        async {
            barrier.wait().await;
            runner.process(candidate, &cancel).await
        }
    );
    a.unwrap();
    b.unwrap();
    let state = snapshot(&f).await;
    assert_eq!(state["jobs"][0]["status"], "pending");
    assert_eq!(state["jobs"][0]["retry_count"], 1);
    assert_eq!(state["attempts"].as_array().unwrap().len(), 1);
    due(&f, scope).await;
    // Stale expired discovery must not turn into a fresh unsupported claim.
    runner.process(candidate, &cancel).await.unwrap();
    assert_eq!(snapshot(&f).await["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
}

struct UnsupportedTiming(std::sync::Mutex<Vec<Instant>>);
#[async_trait]
impl WorkflowHandler for UnsupportedTiming {
    fn supports(&self, _: &WorkflowStepKind) -> bool {
        self.0.lock().unwrap().push(Instant::now());
        false
    }
    async fn execute(&self, _: &WorkflowStepKind, _: &ClaimedWorkflow) -> AppResult<Value> {
        panic!("unsupported handler");
    }
}
#[tokio::test]
async fn workflow_poll_unchanged_page_and_notification_storm_preserve_delay() {
    let (f, _) = fixture().await;
    let p = f.persistence();
    let handler = UnsupportedTiming(std::sync::Mutex::new(Vec::new()));
    let runner = WorkflowWorker {
        port: p,
        handler: &handler,
        worker: worker(),
        lease: policy(),
        poll: PollPolicy::new(Duration::from_millis(50), 1).unwrap(),
    };
    let cancel = CancellationToken::new();
    let wakeup = Notify::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runner.run(&wakeup, &cancel), async {
            loop {
                wakeup.notify_one();
                if handler.0.lock().unwrap().len() >= 3 {
                    cancel.cancel();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
    })
    .await
    .unwrap();
    let times = handler.0.lock().unwrap();
    assert_eq!(times.len(), 3);
    // One positive wait follows the full unchanged page and another the wrap.
    assert!(
        times
            .windows(2)
            .all(|pair| pair[1] - pair[0] >= Duration::from_millis(100))
    );
}
