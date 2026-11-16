//! Parking registers waits/children; final-response approval belongs to completion.
use super::*;
use crate::app_error::AppResult;
use crate::application::workflow::children::WorkflowChildren;
use crate::application::workflow::waits::*;
use std::{future::Future, pin::Pin, time::Duration};

#[path = "reviewer_terminal_child_hint_tests.rs"]
mod terminal_hints;

#[path = "reviewer_effect_hint_tests.rs"]
mod effect_hints;

#[path = "reviewer_memory_effect_hint_tests.rs"]
mod memory_effect_hints;

#[path = "reviewer_agent_effect_hint_tests.rs"]
mod agent_effect_hints;

struct UnusedFinalPolicy {
    fixture: FinalFixture,
    retired: Uuid,
}

fn kind_source(kind: &str) -> Value {
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["output_schema"] = json!(true);
    source
}

fn timer_source() -> Value {
    let mut source = kind_source("wait.timer");
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(10)).to_rfc3339()});
    source
}

fn unused_final_policy(s: Intents, source: Value) -> impl Future<Output = UnusedFinalPolicy> {
    unused_final_policy_for(s, source, FinalPublication::Standard)
}

async fn unused_final_policy_for(
    s: Intents,
    source: Value,
    publication: FinalPublication,
) -> UnusedFinalPolicy {
    let retired = member(&s.fixture, "admin").await;
    let principal = s.fixture.principal(retired).await;
    let fixture = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source,
            contract: literal_contract("Complete"),
            policy: Some(ReplyApprovalPolicy::new(vec![principal], 60).unwrap()),
            run_as: None,
            publication,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await;
    fixture.p.delete_user_account(retired).await.unwrap();
    UnusedFinalPolicy { fixture, retired }
}

async fn assert_parked(f: &UnusedFinalPolicy, reason: &str) {
    let state: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('run',(SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2), \
         'reason',(SELECT waiting_reason FROM workflow_runs WHERE company_id=$1 AND id=$2), \
         'waits',(SELECT count(*) FROM workflow_waits WHERE company_id=$1 AND run_id=$2), \
         'events',(SELECT count(*) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND event_kind='wait_parked'), \
         'proposals',(SELECT count(*) FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2), \
         'retired',NOT EXISTS(SELECT 1 FROM users WHERE id=$3))",
    )
    .bind(f.fixture.scope.company.as_uuid()).bind(f.fixture.scope.run.as_uuid()).bind(f.retired)
    .fetch_one(f.fixture.p.pool()).await.unwrap();
    assert_eq!(
        state,
        json!({"run":"waiting","reason":reason,"waits":1,"events":1,"proposals":0,"retired":true})
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_timer_park_and_replay_ignore_unused_retired_final_reviewers() {
    let s = Box::pin(Intents::new()).await;
    let f = Box::pin(unused_final_policy(s, timer_source())).await;
    let first = f
        .fixture
        .p
        .park_wait(f.fixture.scope)
        .await
        .unwrap()
        .unwrap();
    let replay = f
        .fixture
        .p
        .park_wait(f.fixture.scope)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replay, first);
    assert_parked(&f, "timer").await;
}

#[derive(Clone, Copy)]
enum ChildExit {
    Final,
    Next,
    ErrorNext,
}

fn unused_child_policy() -> impl Future<Output = UnusedFinalPolicy> {
    unused_child_with(kind_source("channel.no_reply"), ChildExit::Final)
}

struct ChildSetup {
    leaf: Value,
    parent: Value,
    exit: ChildExit,
}

fn unused_child_with(leaf: Value, exit: ChildExit) -> impl Future<Output = UnusedFinalPolicy> {
    unused_child_setup(ChildSetup {
        leaf,
        parent: kind_source("workflow.call"),
        exit,
    })
}

async fn unused_child_setup(setup: ChildSetup) -> UnusedFinalPolicy {
    let ChildSetup {
        leaf,
        parent: mut source,
        exit,
    } = setup;
    let s = Box::pin(Intents::new()).await;
    let child = WorkflowId::new(Uuid::new_v4());
    let version = Box::pin(s.fixture.publish_workflow(child, leaf)).await;
    source["steps"]["start"]["child_workflow_id"] = json!(child.to_string());
    source["steps"]["start"]["child_version_id"] = json!(version.to_string());
    if !matches!(exit, ChildExit::Final) {
        let route = match exit {
            ChildExit::Next => "success",
            ChildExit::ErrorNext => "error",
            ChildExit::Final => unreachable!(),
        };
        source["steps"]["start"]["routes"][route] = json!("finish");
        source["steps"]["finish"] = json!({"type":"data.map","with":{
            "value":{"literal":43},"output_schema":{"literal":true}},
            "routes":{"success":"$end"}});
    }
    Box::pin(unused_final_policy(s, source)).await
}

#[tokio::test]
async fn workflow_reviewer_follow_child_park_and_replay_ignore_unused_retired_final_reviewers() {
    let f = Box::pin(unused_child_policy()).await;
    let first = f
        .fixture
        .p
        .park_child(f.fixture.scope)
        .await
        .unwrap()
        .unwrap();
    let replay = f
        .fixture
        .p
        .park_child(f.fixture.scope)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replay, first);
    assert_parked(&f, "child_run").await;
    let children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2",
    )
    .bind(f.fixture.scope.company.as_uuid())
    .bind(f.fixture.scope.run.as_uuid())
    .fetch_one(f.fixture.p.pool())
    .await
    .unwrap();
    assert_eq!(children, 1, "replay must not create a second child");
}

#[tokio::test]
async fn workflow_reviewer_follow_wait_park_company_precedes_run_during_actual_retirement() {
    use crate::adapters::persistence::user_lifecycle::{
        finish_account_deletion_on, lock_account_on,
    };
    let s = Box::pin(Intents::new()).await;
    let f = Box::pin(unused_final_policy(s, timer_source())).await;
    let fixture = &f.fixture;
    let user = member(&fixture.s.fixture, "admin").await;
    let mut retirement = fixture.p.pool().begin().await.unwrap();
    let locks = lock_account_on(&mut retirement, user).await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let park = fixture.p.park_wait(fixture.scope);
    tokio::pin!(park);
    tokio::select! {
        _ = blocked_by(fixture.p.pool(), blocker, "%companies%FOR NO KEY UPDATE%") => {},
        result = park.as_mut() => panic!("wait owner must first wait for retirement's company: {result:?}"),
    }
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE NOWAIT")
        .bind(fixture.scope.company.as_uuid())
        .bind(fixture.scope.run.as_uuid())
        .execute(&mut *retirement)
        .await
        .expect("wait company ownership must precede run ownership");
    finish_account_deletion_on(&mut retirement, user, locks)
        .await
        .unwrap();
    retirement.commit().await.unwrap();
    park.await.unwrap().unwrap();
    let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
        .bind(user)
        .fetch_one(fixture.p.pool())
        .await
        .unwrap();
    assert!(
        !present,
        "actual account retirement and actual wait park both commit"
    );
    assert_parked(&f, "timer").await;
}

async fn child_snapshot(f: &UnusedFinalPolicy) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'runs',(SELECT jsonb_agg(to_jsonb(run) ORDER BY id) FROM workflow_runs AS run WHERE company_id=$1), \
         'executions',(SELECT jsonb_agg(to_jsonb(execution) ORDER BY id) FROM workflow_executions AS execution WHERE company_id=$1), \
         'jobs',(SELECT jsonb_agg(to_jsonb(job) ORDER BY id) FROM background_tasks AS job WHERE company_id=$1), \
         'waits',(SELECT jsonb_agg(to_jsonb(wait) ORDER BY id) FROM workflow_waits AS wait WHERE company_id=$1), \
         'events',(SELECT jsonb_agg(to_jsonb(event) ORDER BY run_id,sequence) FROM workflow_run_events AS event WHERE company_id=$1), \
         'proposals',(SELECT jsonb_agg(to_jsonb(proposal) ORDER BY id) FROM channel_reply_proposals AS proposal WHERE company_id=$1), \
         'decisions',(SELECT jsonb_agg(to_jsonb(decision) ORDER BY id) FROM workflow_decisions AS decision WHERE company_id=$1), \
         'receipts',(SELECT jsonb_agg(to_jsonb(receipt) ORDER BY run_id,execution_id,resource,reservation_key) FROM workflow_budget_receipts AS receipt WHERE company_id=$1), \
         'usage',(SELECT jsonb_agg(to_jsonb(usage) ORDER BY root_run_id) FROM workflow_root_budget_usage AS usage WHERE company_id=$1))",
    )
    .bind(f.fixture.scope.company.as_uuid())
    .fetch_one(f.fixture.p.pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_reviewer_follow_pending_child_resume_ignores_unused_retired_final_reviewers() {
    use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};
    let f = Box::pin(unused_child_policy()).await;
    f.fixture
        .p
        .park_child(f.fixture.scope)
        .await
        .unwrap()
        .unwrap();
    assert_parked(&f, "child_run").await;
    let before = child_snapshot(&f).await;
    assert_eq!(before["runs"].as_array().unwrap().len(), 2);
    for _ in 0..2 {
        assert_eq!(
            f.fixture.p.resume_wait(f.fixture.scope).await.unwrap(),
            WaitProgress::Pending
        );
        assert_eq!(child_snapshot(&f).await, before);
    }
}

#[derive(sqlx::FromRow)]
struct ChildActivation {
    run_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
}

async fn child_scope(f: &UnusedFinalPolicy) -> ActivationRequest {
    let scope = f.fixture.scope;
    let child: ChildActivation = sqlx::query_as(
        "SELECT child.id AS run_id,execution.id AS execution_id,job.id AS job_id \
         FROM workflow_waits AS wait \
         JOIN workflow_runs AS child ON child.company_id=wait.company_id AND child.id=wait.child_run_id \
         JOIN workflow_executions AS execution ON execution.company_id=child.company_id AND execution.run_id=child.id \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE wait.company_id=$1 AND wait.run_id=$2 AND wait.execution_id=$3 AND wait.reason='child_run' \
         ORDER BY execution.activation,job.id LIMIT 1",
    )
    .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
    .fetch_one(f.fixture.p.pool()).await.unwrap();
    ActivationRequest {
        company: scope.company,
        run: RunId::new(child.run_id),
        execution: ExecutionId::new(child.execution_id),
        job: WorkflowJobId(child.job_id),
    }
}

struct ChildGate {
    transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    backend: i32,
}

async fn child_gate(f: &UnusedFinalPolicy, child: ActivationRequest) -> ChildGate {
    let mut transaction = f.fixture.p.pool().begin().await.unwrap();
    let backend = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.fixture.s.owner().user_id())
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SAVEPOINT child_terminal_execution_gate")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(child.company.as_uuid())
        .bind(child.execution.as_uuid())
        .execute(&mut *transaction)
        .await
        .unwrap();
    ChildGate {
        transaction,
        backend,
    }
}

async fn queued_child<Child, Parent>(
    pool: &sqlx::PgPool,
    blocker: i32,
    mut child: Pin<&mut Child>,
    mut parent: Pin<&mut Parent>,
) where
    Child: Future<Output = AppResult<BatchResult>>,
    Parent: Future<Output = AppResult<WaitProgress>>,
{
    let child_pid = tokio::select! {
        _ = child.as_mut() => panic!("actual child completion escaped its execution gate"),
        pid = blocked_by(pool, blocker, "%FROM workflow_executions%FOR UPDATE%") => pid,
    };
    let parent_pid = tokio::select! {
        _ = child.as_mut() => panic!("child must retain company before its execution gate"),
        _ = parent.as_mut() => panic!("pending parent bypassed the child's company ownership"),
        pid = blocked_by(pool, child_pid, "%companies%FOR NO KEY UPDATE%") => pid,
    };
    assert_ne!(child_pid, parent_pid);
}

struct ChildArrival {
    completed: BatchResult,
    resumed: AppResult<WaitProgress>,
}

async fn finish_child<Child, Parent>(
    pool: &sqlx::PgPool,
    blocker: i32,
    mut child: Pin<&mut Child>,
    mut parent: Pin<&mut Parent>,
) -> ChildArrival
where
    Child: Future<Output = AppResult<BatchResult>>,
    Parent: Future<Output = AppResult<WaitProgress>>,
{
    tokio::select! {
        (completed, resumed) = async { tokio::join!(child.as_mut(), parent.as_mut()) } => {
            ChildArrival { completed: completed.unwrap(), resumed }
        }
        _ = blocked_by(pool, blocker, "%FROM users%FOR SHARE%") => {
            panic!("a pending parent may not acquire newly required actor accounts after company");
        }
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_child_terminal_arrival_replans_before_new_actor_accounts() {
    let f = Box::pin(unused_child_policy()).await;
    let scope = f.fixture.scope;
    f.fixture.p.park_child(scope).await.unwrap().unwrap();
    let child_scope = Box::pin(child_scope(&f)).await;
    let mut gate = Box::pin(child_gate(&f, child_scope)).await;
    let child = f.fixture.p.advance_pure(
        child_scope,
        BatchBudget::new(4, Duration::from_secs(1)).unwrap(),
    );
    let parent = f.fixture.p.resume_wait(scope);
    tokio::pin!(child, parent);
    // Box observation and result phases to keep the stock test stack bounded.
    Box::pin(queued_child(
        f.fixture.p.pool(),
        gate.backend,
        child.as_mut(),
        parent.as_mut(),
    ))
    .await;
    sqlx::query("ROLLBACK TO SAVEPOINT child_terminal_execution_gate")
        .execute(&mut *gate.transaction)
        .await
        .unwrap();
    let arrival = Box::pin(finish_child(
        f.fixture.p.pool(),
        gate.backend,
        child.as_mut(),
        parent.as_mut(),
    ))
    .await;
    assert_eq!(arrival.completed.disposition, BatchDisposition::Completed);
    assert!(matches!(arrival.resumed, Err(AppError::Conflict(_))));
    let received = child_snapshot(&f).await;
    let runs = received["runs"].as_array().unwrap();
    let root = runs
        .iter()
        .find(|run| run["id"] == json!(scope.run.as_uuid()))
        .unwrap();
    let leaf = runs
        .iter()
        .find(|run| run["id"] == json!(child_scope.run.as_uuid()))
        .unwrap();
    assert_eq!(root["state"], "waiting");
    assert_eq!(leaf["state"], "succeeded");
    assert_eq!(received["proposals"], Value::Null);
    assert_eq!(received["decisions"], Value::Null);
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
        .bind(scope.company.as_uuid())
        .execute(&mut *gate.transaction)
        .await
        .expect("the stale parent observer must release company before returning its retry");
    gate.transaction.rollback().await.unwrap();
    assert!(matches!(
        f.fixture.p.resume_wait(scope).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        child_snapshot(&f).await,
        received,
        "fresh terminal processing must still refuse its actually required missing final reviewer"
    );
}
