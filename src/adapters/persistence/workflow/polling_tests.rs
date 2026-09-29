use super::*;
use crate::application::workflow::{polling::*, waits::*, worker::*};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct Scripted {
    calls: AtomicUsize,
    supported: bool,
}
#[async_trait]
impl WorkflowHandler for Scripted {
    fn supports(&self, _: &WorkflowStepKind) -> bool {
        self.supported
    }
    async fn execute(&self, _: &WorkflowStepKind, _: &ClaimedWorkflow) -> WorkflowHandlerResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(json!({"items":[],"token_count":0}))
    }
}
fn scripted() -> Scripted {
    Scripted {
        calls: AtomicUsize::new(0),
        supported: true,
    }
}
fn polling<'a>(
    p: &'a PostgresPersistence,
    h: &'a Scripted,
) -> WorkflowWorker<'a, PostgresPersistence, Scripted> {
    WorkflowWorker {
        port: p,
        handler: h,
        worker: worker(),
        lease: policy(),
        poll: PollPolicy::new(Duration::from_millis(50), 2).unwrap(),
    }
}
async fn run_state(p: &PostgresPersistence, scope: ActivationRequest) -> String {
    sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
}
async fn until_state(p: &PostgresPersistence, scope: ActivationRequest, expected: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while run_state(p, scope).await != expected {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
fn wait_source(timer: bool) -> Value {
    let name = if timer { "wait.timer" } else { "wait.event" };
    let mut source: Value = serde_json::from_str(&registry::example(name).unwrap().source).unwrap();
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(30)).to_rfc3339()});
    if !timer {
        source["steps"]["start"]["with"]["payload_schema"] = json!({"literal":true});
    }
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    source
}
fn signal(scope: ActivationRequest) -> WorkflowSignal {
    WorkflowSignal {
        scope,
        id: WorkflowEventId(Uuid::new_v4()),
        name: WorkflowEventName("review.completed".into()),
        correlation: WorkflowEventCorrelation("request-1".into()),
        payload: json!({"ok":true}),
    }
}

#[tokio::test]
async fn workflow_poll_restart_without_notifications_executes_once() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    let discovered = p.poll_work(None, 2).await.unwrap();
    assert_eq!(discovered.candidates.len(), 1);
    // Crash after discovery loses only memory, not durable eligibility.
    let fresh = PostgresPersistence::new(p.pool().clone());
    let handler = scripted();
    let worker = polling(&fresh, &handler);
    let wakeup = Notify::new();
    let shutdown = CancellationToken::new();
    tokio::join!(worker.run(&wakeup, &shutdown), async {
        until_state(&fresh, scope, "succeeded").await;
        shutdown.cancel();
    });
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert!(
        fresh
            .poll_work(None, 128)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    let state = snapshot(&f).await;
    assert_eq!(state["attempts"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn workflow_poll_competing_workers_share_one_actual_invocation() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    let handler = scripted();
    let left = polling(p, &handler);
    let right = polling(p, &handler);
    let page = p.poll_work(None, 2).await.unwrap();
    let candidate = page.candidates[0];
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
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert_eq!(run_state(p, scope).await, "succeeded");
}

#[tokio::test]
async fn workflow_poll_stored_events_before_park_and_during_downtime_resume() {
    for before_park in [true, false] {
        let (f, scope) = fixture_source(wait_source(false)).await;
        let p = f.persistence();
        if !before_park {
            p.park_wait(scope).await.unwrap().unwrap();
        }
        p.record_signal(signal(scope)).await.unwrap();
        let fresh = PostgresPersistence::new(p.pool().clone());
        let handler = scripted();
        let worker = polling(&fresh, &handler);
        let wakeup = Notify::new();
        let cancel = CancellationToken::new();
        tokio::join!(worker.run(&wakeup, &cancel), async {
            until_state(p, scope, "succeeded").await;
            cancel.cancel();
        });
        assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workflow_waits WHERE state='completed'")
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(count, 1);
    }
}

#[tokio::test]
async fn workflow_poll_timer_becomes_due_after_empty_scan() {
    let (f, scope) = fixture_source(wait_source(true)).await;
    let p = f.persistence();
    p.park_wait(scope).await.unwrap();
    assert!(p.poll_work(None, 2).await.unwrap().candidates.is_empty());
    // Make the already parked timer due after the empty scan. Fixture setup is
    // intentionally independent of how long parallel fresh migrations take.
    sqlx::query("UPDATE workflow_waits SET deadline=clock_timestamp() WHERE execution_id=$1")
        .bind(scope.execution.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    let handler = scripted();
    let worker = polling(p, &handler);
    let wakeup = Notify::new();
    let cancel = CancellationToken::new();
    tokio::join!(worker.run(&wakeup, &cancel), async {
        until_state(p, scope, "succeeded").await;
        cancel.cancel();
    });
}

#[tokio::test]
async fn workflow_poll_lost_claim_recovers_with_positive_backoff() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    let first = p
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert!(p.poll_work(None, 2).await.unwrap().candidates.is_empty());
    expire(&f, scope).await;
    let handler = scripted();
    let runner = polling(p, &handler);
    let cancel = CancellationToken::new();
    let expired = p.poll_work(None, 2).await.unwrap().candidates[0];
    assert_eq!(expired.work, PollWork::ExpiredLease);
    runner.process(expired, &cancel).await.unwrap();
    assert!(p.poll_work(None, 2).await.unwrap().candidates.is_empty());
    assert!(!p.validate_io(first.fence, policy()).await.unwrap());
    let wakeup = Notify::new();
    tokio::join!(runner.run(&wakeup, &cancel), async {
        until_state(p, scope, "succeeded").await;
        cancel.cancel();
    });
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot(&f).await["attempts"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn workflow_poll_cursor_pages_wrap_and_revisit_newly_due_jobs() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    let mut ids = vec![scope.job.0];
    for index in 0..4 {
        let command = f
            .prepare(f.request(&format!("extra-{index}"), f.manual()))
            .await;
        p.admit(&command).await.unwrap();
        ids.push(
            sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
                .bind(command.first_execution_id().as_uuid())
                .fetch_one(p.pool())
                .await
                .unwrap(),
        );
    }
    ids.sort();
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()+interval '1 minute' WHERE id=$1",
    )
    .bind(ids[0])
    .execute(p.pool())
    .await
    .unwrap();
    let first = p.poll_work(None, 2).await.unwrap();
    assert_eq!(
        first
            .candidates
            .iter()
            .map(|c| c.scope.job.0)
            .collect::<Vec<_>>(),
        ids[1..3]
    );
    sqlx::query("UPDATE background_tasks SET run_at=clock_timestamp() WHERE id=$1")
        .bind(ids[0])
        .execute(p.pool())
        .await
        .unwrap();
    let second = p.poll_work(first.next, 2).await.unwrap();
    assert_eq!(second.candidates.len(), 2);
    let end = p.poll_work(second.next, 2).await.unwrap();
    assert!(end.next.is_none());
    assert_eq!(
        p.poll_work(end.next, 2).await.unwrap().candidates[0]
            .scope
            .job
            .0,
        ids[0]
    );
    assert!(p.poll_work(None, 0).await.is_err());
    assert!(p.poll_work(None, 129).await.is_err());
    assert!(PollPolicy::new(Duration::ZERO, 1).is_err());
}

struct DroppingHandler {
    started: Notify,
    dropped: AtomicUsize,
}
struct DropWitness<'a>(&'a AtomicUsize);
impl Drop for DropWitness<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl WorkflowHandler for DroppingHandler {
    fn supports(&self, _: &WorkflowStepKind) -> bool {
        true
    }
    async fn execute(&self, _: &WorkflowStepKind, _: &ClaimedWorkflow) -> WorkflowHandlerResult {
        let _witness = DropWitness(&self.dropped);
        self.started.notify_one();
        std::future::pending().await
    }
}
#[tokio::test]
async fn workflow_poll_shutdown_drops_actual_future_and_leaves_recoverable_lease() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    let handler = DroppingHandler {
        started: Notify::new(),
        dropped: AtomicUsize::new(0),
    };
    let runner = WorkflowWorker {
        port: p,
        handler: &handler,
        worker: worker(),
        lease: policy(),
        poll: PollPolicy::new(Duration::from_millis(50), 2).unwrap(),
    };
    let cancel = CancellationToken::new();
    let wakeup = Notify::new();
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(runner.run(&wakeup, &cancel), async {
            handler.started.notified().await;
            cancel.cancel();
        });
    })
    .await
    .unwrap();
    assert_eq!(handler.dropped.load(Ordering::SeqCst), 1);
    assert!(p.poll_work(None, 2).await.unwrap().candidates.is_empty());
    expire(&f, scope).await;
    let next = scripted();
    let restart = polling(p, &next);
    let cancel = CancellationToken::new();
    tokio::join!(restart.run(&wakeup, &cancel), async {
        until_state(p, scope, "succeeded").await;
        cancel.cancel();
    });
    assert_eq!(next.calls.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot(&f).await["attempts"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn workflow_poll_database_failure_and_spurious_wakeups_do_not_stop_periodic_progress() {
    let (f, scope) = fixture().await;
    let p = f.persistence();
    let mut lock = p.pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE background_tasks IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let handler = scripted();
    let runner = polling(p, &handler);
    let cancel = CancellationToken::new();
    let wakeup = Notify::new();
    tokio::join!(runner.run(&wakeup, &cancel), async {
        // First poll must time out; no reconnect notification is sent afterward.
        tokio::time::sleep(Duration::from_millis(1300)).await;
        lock.commit().await.unwrap();
        for _ in 0..100 {
            wakeup.notify_one();
        }
        until_state(p, scope, "succeeded").await;
        cancel.cancel();
    });
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workflow_poll_unsupported_full_page_stays_bounded_and_later_work_progresses() {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    let (f, mut scope) = fixture_source(source).await;
    let p = f.persistence();
    // Force the unsupported row before all healthy work, independent of UUID luck.
    let first_job = Uuid::from_u128(1);
    sqlx::query("UPDATE background_tasks SET id=$2 WHERE id=$1")
        .bind(scope.job.0)
        .bind(first_job)
        .execute(p.pool())
        .await
        .unwrap();
    scope.job = WorkflowJobId(first_job);
    let other = f.prepare(f.request("other", f.manual())).await;
    p.admit(&other).await.unwrap();
    let other_job: Uuid =
        sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
            .bind(other.first_execution_id().as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let other_scope = ActivationRequest {
        company: scope.company,
        run: other.proposed_run_id(),
        execution: other.first_execution_id(),
        job: WorkflowJobId(other_job),
    };
    // One run already completed its I/O; its pure successor is available among
    // unsupported work. No handler can execute the remaining I/O boundary.
    let claim = p
        .claim_io(other_scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    use crate::application::workflow::completion::WorkflowCompletion;
    p.complete_io(FencedWorkflowResult {
        fence: claim.fence,
        output: json!({"items":[],"token_count":0}),
    })
    .await
    .unwrap()
    .unwrap();
    let handler = Scripted {
        calls: AtomicUsize::new(0),
        supported: false,
    };
    let runner = WorkflowWorker {
        poll: PollPolicy::new(Duration::from_millis(50), 1).unwrap(),
        ..polling(p, &handler)
    };
    assert_eq!(
        p.poll_work(None, 1).await.unwrap().candidates[0].scope,
        scope
    );
    let began = Instant::now();
    let cancel = CancellationToken::new();
    let wakeup = Notify::new();
    tokio::join!(runner.run(&wakeup, &cancel), async {
        until_state(p, other_scope, "succeeded").await;
        cancel.cancel();
    });
    assert!(began.elapsed() >= Duration::from_millis(50));
    assert_eq!(run_state(p, scope).await, "queued");
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1")
        .bind(scope.job.0)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 0);
}

#[path = "polling_integration_tests.rs"]
mod integration_tests;

#[path = "maintenance_worker_tests.rs"]
mod maintenance_worker_tests;

#[path = "phase03_tests.rs"]
mod phase03_tests;
