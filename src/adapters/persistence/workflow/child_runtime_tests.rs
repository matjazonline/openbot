//! Runtime `workflow.call` (07-D1 §D1.2, §D1.4, §D1.5, §D1.9): park, consume, cancel, expire.
use super::child_repeat_tests::{repeat_parent, round_child};
use super::*;
use crate::adapters::persistence::workflow::tests::admission_tests::admission_history_tests::Conversation;
use crate::application::workflow::children::WorkflowChildren;
use crate::application::workflow::completion::CommitDisposition;
use crate::application::workflow::history::WorkflowHistory;
use crate::application::workflow::lease::{ClaimedWorkflow, WorkflowHandlerOutcome};
use crate::application::workflow::lease::{LeasePolicy, WorkflowLeases, WorkflowWorkerId};
use crate::application::workflow::polling::{
    PollCandidate, PollPolicy, PollWork, WorkflowFairPolling, WorkflowPolling, WorkflowStepKind,
};
use crate::application::workflow::waits::*;
use crate::application::workflow::worker::{WorkDisposition, WorkflowHandler, WorkflowWorker};
use tokio::sync::Barrier;

fn example(kind: &str) -> Value {
    serde_json::from_str(&registry::example(kind).unwrap().source).unwrap()
}

fn data_map() -> Value {
    example("data.map")
}

async fn state(f: &AdmissionFixture, run: RunId) -> String {
    sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

async fn children(f: &AdmissionFixture, parent: RunId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1")
        .bind(parent.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

/// A root calling `leaf` whose failures take an `error` route to `recover`.
fn routed(child: WorkflowId, version: VersionId) -> Value {
    let mut source = child_support::call_source(child, version);
    source["steps"]["start"]["routes"]["error"] = json!("recover");
    source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":"recovered"},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    source
}

#[tokio::test]
async fn workflow_call_success_completes_parent_once_with_child_output() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let (parent, child) = (scopes[0], scopes[1]);
    let p = f.persistence();
    assert_eq!(state(&f, parent.run).await, "waiting");
    assert_eq!(p.resume_wait(parent).await.unwrap(), WaitProgress::Pending);
    p.advance_pure(child, budget(4)).await.unwrap();
    // Competing consumers commit the parent's continuation once.
    let barrier = Barrier::new(2);
    let resume = || async {
        barrier.wait().await;
        p.resume_wait(parent).await.unwrap()
    };
    let (a, b) = tokio::join!(resume(), resume());
    let committed: Vec<_> = [a, b]
        .into_iter()
        .map(|progress| match progress {
            WaitProgress::Completed(step) => step,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        committed
            .iter()
            .filter(|step| step.disposition == CommitDisposition::Committed)
            .count(),
        1
    );
    let child_output: Value =
        sqlx::query_scalar("SELECT committed_output FROM workflow_executions WHERE id=$1")
            .bind(child.execution.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert!(committed.iter().all(|step| step.output == child_output));
    assert_eq!(state(&f, parent.run).await, "succeeded");
    assert_eq!(children(&f, parent.run).await, 1);
}

#[tokio::test]
async fn workflow_call_competing_parkers_create_one_child_and_replay_its_wait() {
    let (f, parent) = AdmissionFixture::caller(data_map()).await;
    let p = f.persistence();
    let barrier = Barrier::new(2);
    let park = || async {
        barrier.wait().await;
        p.park_child(parent).await.unwrap()
    };
    let (a, b) = tokio::join!(park(), park());
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.wait, b.wait);
    assert_eq!(p.park_child(parent).await.unwrap().unwrap().wait, a.wait);
    assert_eq!(children(&f, parent.run).await, 1);
    let source: String = sqlx::query_scalar("SELECT admission.source_key FROM workflow_admissions AS admission JOIN workflow_runs AS run ON run.id=admission.run_id WHERE run.parent_run_id=$1")
        .bind(parent.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        source,
        format!(
            "v1:{}",
            json!([
                "child",
                parent.run.as_uuid(),
                parent.execution.as_uuid(),
                "start",
                null,
                1
            ])
        )
    );
}

#[tokio::test]
async fn workflow_call_injected_failure_rolls_back_child_and_wait_and_never_hot_polls() {
    let (f, parent) = AdmissionFixture::caller(data_map()).await;
    let p = f.persistence();
    sqlx::raw_sql("CREATE FUNCTION reject_child_wait() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'child wait injected'; END $$; CREATE TRIGGER reject_child_wait BEFORE INSERT ON workflow_waits FOR EACH ROW WHEN (NEW.reason='child_run') EXECUTE FUNCTION reject_child_wait()")
        .execute(p.pool()).await.unwrap();
    let before = snapshot(&f).await;
    assert!(p.park_child(parent).await.is_err());
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(children(&f, parent.run).await, 0);
    // An unclassified failure is offered at most once per fair-poll epoch: the next poll moves
    // past it instead of spinning on it (07-D1 §D1.9).
    let worker = WorkflowWorkerId(Uuid::new_v4());
    let offered = |page: &crate::application::workflow::polling::FairPollPage| {
        page.candidates.iter().any(|c| c.scope.run == parent.run)
    };
    assert!(offered(&p.poll_fair(worker, 128).await.unwrap()));
    assert!(p.park_child(parent).await.is_err());
    assert!(!offered(&p.poll_fair(worker, 128).await.unwrap()));
    // Its terminal bound is the run deadline, retired by the existing deadline work.
    sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE id=$1")
        .bind(parent.run.as_uuid()).execute(p.pool()).await.unwrap();
    assert!(p.expire_run(parent).await.unwrap());
    assert_eq!(state(&f, parent.run).await, "failed");
    for _ in 0..2 {
        assert!(!offered(&p.poll_fair(worker, 128).await.unwrap()));
    }
    sqlx::raw_sql("DROP TRIGGER reject_child_wait ON workflow_waits")
        .execute(p.pool())
        .await
        .unwrap();
    assert!(p.park_child(parent).await.unwrap().is_none());
}

#[tokio::test]
async fn workflow_call_child_failure_and_cancellation_route_or_fail_never_succeed() {
    // Routed: the error successor receives null.
    let (f, parent) = AdmissionFixture::with_child(data_map(), routed).await;
    let p = f.persistence();
    let child = f.spawn(parent).await;
    sqlx::query("UPDATE workflow_runs SET state='failed' WHERE id=$1")
        .bind(child.run.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    let WaitProgress::Completed(step) = p.resume_wait(parent).await.unwrap() else {
        panic!("routed")
    };
    assert_eq!(step.route, RouteSelection::FinalError);
    assert_eq!(step.output, Value::Null);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind='workflow.child_failed'")
        .bind(parent.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(audit, 1);

    // Unrouted: an operator cancels the child; the parent fails at its call step and is never
    // retry-safe, since its parked job closed without a failed attempt (07-D1 §D1.7).
    let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let (parent, child) = (scopes[0], scopes[1]);
    let p = f.persistence();
    let revision = p
        .head(child.company, child.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    let cancel = CancelCommand {
        company_id: child.company,
        actor: f.binding.target.actor,
        run_id: child.run,
        command_key: IdempotencyKey::parse("cancel-child").unwrap(),
        expected_revision: revision,
    };
    assert!(matches!(
        p.cancel(cancel).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    p.resume_wait(parent).await.unwrap();
    let run: Value =
        sqlx::query_scalar("SELECT to_jsonb(run) FROM workflow_runs AS run WHERE id=$1")
            .bind(parent.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(run["state"], "failed");
    assert_eq!(
        run["terminal_execution_id"],
        json!(parent.execution.as_uuid())
    );
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind='workflow.child_cancelled'")
        .bind(parent.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(audit, 1);
    let revision = p
        .head(parent.company, parent.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    let retry = RetryCommand {
        company_id: parent.company,
        actor: f.binding.target.actor,
        run_id: parent.run,
        command_key: IdempotencyKey::parse("retry-parent").unwrap(),
        expected_revision: revision,
    };
    assert!(matches!(
        p.retry(retry).await.unwrap(),
        RetryResult::Unsafe { .. }
    ));
}

#[tokio::test]
async fn workflow_call_child_limits_reject_input_at_park_and_route_it() {
    // The child's own context limit is smaller than the run input it would receive.
    let mut leaf = data_map();
    leaf["steps"]["start"]["with"] =
        json!({"value": {"ref": "/input"}, "output_schema": {"literal": true}});
    leaf["limits"]["max_context_bytes"] = json!(8);
    let (f, parent) = AdmissionFixture::with_child(leaf, routed).await;
    let p = f.persistence();
    assert!(p.park_child(parent).await.unwrap().is_none());
    assert_eq!(children(&f, parent.run).await, 0);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind='workflow.invalid_input'")
        .bind(parent.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(audit, 1);
    let recovered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_executions WHERE run_id=$1 AND step_id='recover'",
    )
    .bind(parent.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(recovered, 1);
    for _ in 0..2 {
        let page = p
            .poll_fair(WorkflowWorkerId(Uuid::new_v4()), 128)
            .await
            .unwrap();
        assert!(
            page.candidates
                .iter()
                .all(|c| c.scope.execution != parent.execution)
        );
    }
}

#[tokio::test]
async fn workflow_child_history_is_empty_and_later_messages_stay_invisible() {
    // F001: a child receives explicit inputs only, never a fresh thread snapshot.
    let f = AdmissionFixture::calling(example("context.history")).await;
    let c = Conversation::new(&f).await;
    let command = f.prepare(c.request(&f, "thread-root", c.message)).await;
    let p = f.persistence();
    p.admit(&command).await.unwrap();
    let parent = child_support::entry(&f, command.proposed_run_id()).await;
    let pool = p.pool().clone();
    let members = |run: RunId| {
        let pool = pool.clone();
        async move {
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM workflow_history_members WHERE run_id=$1")
                    .bind(run.as_uuid())
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            count
        }
    };
    assert_eq!(members(parent.run).await, 1);
    let mut db = p.pool().acquire().await.unwrap();
    c.insert(&mut db, &f, Uuid::new_v4()).await;
    let child = f.spawn(parent).await;
    assert_eq!(members(child.run).await, 0);
    assert_eq!(members(parent.run).await, 1);
    let association: (Option<Uuid>, Option<Uuid>) =
        sqlx::query_as("SELECT channel_id, thread_id FROM workflow_runs WHERE id=$1")
            .bind(child.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(association, (Some(c.channel), Some(c.thread)));
    let claim = p
        .claim_io(
            child,
            WorkflowWorkerId(Uuid::new_v4()),
            LeasePolicy::new(Duration::from_secs(3)).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    let history = p.frozen_history(&claim).await.unwrap();
    assert_eq!(history.eligible, 0);
    assert!(history.newest.is_empty());
}

#[tokio::test]
async fn workflow_cancel_cascades_to_every_live_descendant() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 2).await;
    let (root, child, grandchild) = (scopes[0], scopes[1], scopes[2]);
    let p = f.persistence();
    let revision = p
        .head(root.company, root.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    let cancel = CancelCommand {
        company_id: root.company,
        actor: f.binding.target.actor,
        run_id: root.run,
        command_key: IdempotencyKey::parse("cancel-tree").unwrap(),
        expected_revision: revision,
    };
    assert!(matches!(
        p.cancel(cancel).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    for scope in [root, child, grandchild] {
        assert_eq!(state(&f, scope.run).await, "cancelled");
    }
    let audits: Vec<Uuid> = sqlx::query_scalar("SELECT run_id FROM workflow_run_events WHERE event_kind='cancelled_by_parent' ORDER BY run_id")
        .fetch_all(p.pool()).await.unwrap();
    let mut expected = vec![child.run.as_uuid(), grandchild.run.as_uuid()];
    expected.sort();
    assert_eq!(audits, expected);
    let waiting: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_waits WHERE state='waiting'")
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(waiting, 0);
    assert!(p.poll_work(None, 128).await.unwrap().candidates.is_empty());
}

async fn cancel_current(f: &AdmissionFixture, scope: ActivationRequest, key: &str) -> CancelResult {
    let p = f.persistence();
    let revision = p
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    p.cancel(CancelCommand {
        company_id: scope.company,
        actor: f.binding.target.actor,
        run_id: scope.run,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: revision,
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn workflow_cancel_competes_with_round_transition_and_child_completion() {
    for _ in 0..3 {
        // Against a round transition that creates the next child.
        let (f, parent) = AdmissionFixture::with_child(round_child(), |child, version| {
            repeat_parent(child, version, 3)
        })
        .await;
        let p = f.persistence();
        p.park_child(parent).await.unwrap().unwrap();
        let round = f.current_child(parent).await.unwrap();
        p.park_wait(round).await.unwrap().unwrap();
        p.record_event(WorkflowEvent {
            company: round.company,
            run: round.run,
            id: WorkflowEventId(Uuid::new_v4()),
            name: WorkflowEventName("review.completed".into()),
            correlation: WorkflowEventCorrelation("round".into()),
            payload: json!({"choice":"revise","artifact":{"body":"Draft"},"feedback":"More"}),
        })
        .await
        .unwrap();
        p.resume_wait(round).await.unwrap();
        let barrier = Barrier::new(2);
        let transition = async {
            barrier.wait().await;
            p.resume_wait(parent).await
        };
        let cancel = async {
            barrier.wait().await;
            let mut result = cancel_current(&f, parent, "cancel-a").await;
            if !matches!(result, CancelResult::Applied { .. }) {
                result = cancel_current(&f, parent, "cancel-b").await;
            }
            result
        };
        let (_, cancelled) = tokio::join!(transition, cancel);
        assert!(
            matches!(cancelled, CancelResult::Applied { .. }),
            "{cancelled:?}"
        );
        let live: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1 AND state IN ('queued','running','waiting')")
            .bind(parent.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
        assert_eq!(live, 0);

        // Against the child's own completion: whichever commits first, nothing stays live.
        let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
        let (parent, child) = (scopes[0], scopes[1]);
        let p = f.persistence();
        let barrier = Barrier::new(2);
        let complete = async {
            barrier.wait().await;
            p.advance_pure(child, budget(4)).await
        };
        let cancel = async {
            barrier.wait().await;
            cancel_current(&f, parent, "cancel-c").await
        };
        let (_, cancelled) = tokio::join!(complete, cancel);
        assert!(
            matches!(cancelled, CancelResult::Applied { .. }),
            "{cancelled:?}"
        );
        assert!(matches!(
            state(&f, child.run).await.as_str(),
            "succeeded" | "cancelled"
        ));
        assert_eq!(state(&f, parent.run).await, "cancelled");
    }
}

#[tokio::test]
async fn workflow_child_timeout_fails_child_and_reaches_parent_error_route() {
    let (f, parent) = AdmissionFixture::with_child(round_child(), |child, version| {
        let mut source = routed(child, version);
        source["steps"]["start"]["with"]["input"] =
            json!({"literal": {"previous_body": "", "feedback": ""}});
        source
    })
    .await;
    let p = f.persistence();
    let child = f.spawn(parent).await;
    p.park_wait(child).await.unwrap().unwrap();
    // The child's own wait passes its deadline; its run fails and the parent is told.
    sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1")
        .bind(child.run.as_uuid()).execute(p.pool()).await.unwrap();
    assert_eq!(p.sweep_waits(8).await.unwrap(), 1);
    assert_eq!(state(&f, child.run).await, "failed");
    let WaitProgress::Completed(step) = p.resume_wait(parent).await.unwrap() else {
        panic!("routed")
    };
    assert_eq!(step.route, RouteSelection::FinalError);
    // The child's deadline is the parent's: it can never outlive it.
    let deadlines: (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
        "SELECT parent.deadline, child.deadline FROM workflow_runs AS parent JOIN workflow_runs AS child ON child.parent_run_id=parent.id WHERE parent.id=$1",
    )
    .bind(parent.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(deadlines.0, deadlines.1);
}

#[tokio::test]
async fn workflow_child_wait_resumes_after_restart() {
    let (f, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let (parent, child) = (scopes[0], scopes[1]);
    let options = f.persistence().pool().connect_options();
    let pool = crate::adapters::persistence::test_support::pool_options()
        .max_connections(2)
        .connect_with((*options).clone())
        .await
        .unwrap();
    let restarted = PostgresPersistence::new(pool.clone());
    restarted.advance_pure(child, budget(4)).await.unwrap();
    assert!(matches!(
        restarted.resume_wait(parent).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    assert_eq!(state(&f, parent.run).await, "succeeded");
    pool.close().await;
}

/// These workflows have no I/O step: the worker itself runs `data.map`, parks the call and
/// resumes its wait.
struct NoIo;
#[async_trait]
impl WorkflowHandler for NoIo {
    fn supports(&self, _: &WorkflowStepKind) -> bool {
        false
    }
    async fn execute(&self, _: &WorkflowStepKind, _: &ClaimedWorkflow) -> WorkflowHandlerOutcome {
        unreachable!("no I/O step")
    }
}

#[tokio::test]
async fn workflow_call_runs_through_the_worker_and_fair_poll() {
    let (f, root) = AdmissionFixture::caller(data_map()).await;
    let p = f.persistence();
    let worker = WorkflowWorker {
        port: p,
        handler: &NoIo,
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: LeasePolicy::new(std::time::Duration::from_secs(30)).unwrap(),
        poll: PollPolicy::new(std::time::Duration::from_millis(50), 16).unwrap(),
        active_jobs: None,
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    let job = |scope| PollCandidate {
        scope,
        work: PollWork::Job,
    };
    // The worker parks the root on a new child and returns; nothing holds the root.
    assert_eq!(
        worker.process(job(root), &cancel).await.unwrap(),
        WorkDisposition::Done
    );
    assert_eq!(state(&f, root.run).await, "waiting");
    let child = f.current_child(root).await.unwrap();
    // Polling runs the child, then offers the root's wait once the child is terminal.
    for _ in 0..8 {
        if state(&f, root.run).await == "succeeded" {
            break;
        }
        for candidate in p.poll_fair(worker.worker, 16).await.unwrap().candidates {
            assert_ne!(
                candidate,
                job(root),
                "a waiting parent is never offered as a job"
            );
            worker.process(candidate, &cancel).await.unwrap();
        }
    }
    assert_eq!(state(&f, child.run).await, "succeeded");
    assert_eq!(state(&f, root.run).await, "succeeded");
    assert_eq!(children(&f, root.run).await, 1);
    assert!(
        p.poll_fair(worker.worker, 16)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
}

/// p08 D1.7: a child inherits its parent run's delivery policy, so a simulated run's whole tree
/// stays in-app.
#[tokio::test]
async fn workflow_call_child_inherits_the_parents_delivery_policy() {
    use crate::application::workflow::DeliveryPolicy;
    for policy in [DeliveryPolicy::Deliver, DeliveryPolicy::InAppOnly] {
        let f = AdmissionFixture::calling(data_map()).await;
        let mut request = f.request("policy-root", f.manual());
        request.delivery_policy = policy;
        let command = f.prepare(request).await;
        f.persistence().admit(&command).await.unwrap();
        let root = child_support::entry(&f, command.proposed_run_id()).await;
        let child = f.spawn(root).await;
        let stored: String =
            sqlx::query_scalar("SELECT delivery_policy FROM workflow_runs WHERE id = $1")
                .bind(child.run.as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        assert_eq!(stored, policy.as_str());
    }
}
