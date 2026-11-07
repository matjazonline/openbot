use super::*;
use crate::application::workflow::{completion::*, polling::*, waits::*, worker::*};
use tokio_util::sync::CancellationToken;

fn source(kind: &str) -> Value {
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["output_schema"] = json!(true);
    source
}

#[tokio::test]
async fn channel_reply_completion_timer_keeps_source_wait_and_final_approval_distinct() {
    let mut source = source("wait.timer");
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()-chrono::Duration::seconds(1)).to_rfc3339()});
    let f = FinalFixture::configured(Intents::new().await, source, literal_contract("Done"), true)
        .await;
    let original = f.p.park_wait(f.scope).await.unwrap().unwrap();
    assert!(matches!(
        f.p.resume_wait(f.scope).await.unwrap(),
        WaitProgress::Pending
    ));
    let source_wait: Uuid=sqlx::query_scalar("SELECT source_wait_id FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2 AND owner_kind='wait'")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(source_wait, original.wait.as_uuid());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_waits WHERE company_id=$1 AND run_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(count, 2);
    assert_eq!(f.state().await["run"], "waiting");
    f.p.submit_decision(f.command("approve").await)
        .await
        .unwrap();
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
    assert_eq!(f.state().await["decision"], "pending");
}

#[tokio::test]
async fn channel_reply_completion_human_choice_parks_final_approval_before_success() {
    let s = Intents::new().await;
    let mut source = source("decision.human");
    source["steps"]["start"]["with"]["reviewers"] =
        json!({"literal":[{"kind":"user","id":s.owner().user_id()}]});
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(10)).to_rfc3339()});
    source["steps"]["start"]["with"]["data_schema"] = json!({"literal":true});
    let f = FinalFixture::configured(s, source, literal_contract("Done"), true).await;
    let notices = approval_test_support::notices(&f.p);
    let original = f.p.park_decision(f.scope, &notices).await.unwrap().unwrap();
    let decision: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND wait_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(original.wait.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    f.p.submit_decision(f.decision_command(DecisionId(decision), "continue").await)
        .await
        .unwrap();
    assert_eq!(f.state().await["run"], "waiting");
    assert_eq!(f.state().await["completed"], false);
    f.p.submit_decision(f.command("approve").await)
        .await
        .unwrap();
    let FinalizationProgress::Completed(step) = f.p.resume_finalization(f.scope).await.unwrap()
    else {
        panic!("final approval was not resumed")
    };
    assert_eq!(step.output["choice"], "continue");
    assert_eq!(f.state().await["decision"], "pending");
}

struct NeverExecute;
#[async_trait]
impl WorkflowHandler for NeverExecute {
    fn supports(&self, _: &WorkflowStepKind) -> bool {
        panic!("saved finalization must precede handler selection")
    }
    async fn execute(&self, _: &WorkflowStepKind, _: &ClaimedWorkflow) -> WorkflowHandlerOutcome {
        panic!("saved finalization must not execute a handler")
    }
}

#[tokio::test]
async fn channel_reply_completion_fenced_approval_resumes_before_worker_handler() {
    let f = FinalFixture::configured(
        Intents::new().await,
        source("context.history"),
        literal_contract("Done"),
        true,
    )
    .await;
    let policy = LeasePolicy::new(Duration::from_secs(3)).unwrap();
    let worker = WorkflowWorkerId(Uuid::new_v4());
    let claim =
        f.p.claim_io(f.scope, worker, policy)
            .await
            .unwrap()
            .unwrap();
    let output = json!({"items":[],"token_count":0,"omitted":0});
    assert!(
        f.p.complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy
        })
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(f.state().await["run"], "waiting");
    assert_eq!(f.state().await["completed"], false);
    f.p.submit_decision(f.command("approve").await)
        .await
        .unwrap();
    assert!(
        f.p.claim_io(f.scope, worker, policy)
            .await
            .unwrap()
            .is_none(),
        "a staged final cannot enter the original I/O handler"
    );
    let worker = WorkflowWorker {
        port: &f.p,
        handler: &NeverExecute,
        notices: None,
        worker,
        lease: policy,
        poll: PollPolicy::new(Duration::from_millis(50), 1).unwrap(),
        active_jobs: None,
        monitoring: None,
    };
    worker
        .process(
            PollCandidate {
                scope: f.scope,
                work: PollWork::Job,
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(f.state().await["decision"], "pending");
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1")
        .bind(f.scope.job.0)
        .fetch_one(f.p.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 1, "approval must not create a second I/O attempt");
}

#[tokio::test]
async fn channel_reply_completion_child_terminal_parks_only_the_owned_parent() {
    let f = Box::pin(child_waiting_finalization()).await;
    f.p.submit_decision(f.command("approve").await)
        .await
        .unwrap();
    let FinalizationProgress::Completed(step) = f.p.resume_finalization(f.scope).await.unwrap()
    else {
        panic!("parent finalization was not resumed")
    };
    assert_eq!(step.response_disposition, ResponseDisposition::Automatic);
    assert_eq!(step.output, json!({"result":42}));
    assert_eq!(f.state().await["decision"], "pending");
}

async fn child_waiting_finalization() -> FinalFixture {
    use crate::application::workflow::children::WorkflowChildren;
    let s = Intents::new().await;
    let child = WorkflowId::new(Uuid::new_v4());
    let mut leaf = source("channel.no_reply");
    leaf["steps"]["start"]["with"] =
        json!({"value":{"literal":{"result":42}},"output_schema":{"literal":true}});
    let version = s.fixture.publish_workflow(child, leaf).await;
    let mut parent = source("workflow.call");
    parent["steps"]["start"]["child_workflow_id"] = json!(child.to_string());
    parent["steps"]["start"]["child_version_id"] = json!(version.to_string());
    let f = FinalFixture::configured(s, parent, literal_contract("Done"), true).await;
    f.p.park_child(f.scope).await.unwrap().unwrap();
    let row: ScopeRow=sqlx::query_as("SELECT wait.child_run_id AS run_id,execution.id AS execution_id,job.id AS job_id FROM workflow_waits AS wait \
        JOIN workflow_executions AS execution ON execution.company_id=wait.company_id AND execution.run_id=wait.child_run_id \
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
        WHERE wait.company_id=$1 AND wait.run_id=$2 AND wait.reason='child_run'")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    let child_scope = ActivationRequest {
        run: RunId::new(row.run_id),
        execution: ExecutionId::new(row.execution_id),
        job: WorkflowJobId(row.job_id),
        ..f.scope
    };
    let child_result =
        f.p.advance_pure(
            child_scope,
            BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        child_result.response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
    assert!(matches!(
        f.p.resume_wait(f.scope).await.unwrap(),
        WaitProgress::Pending
    ));
    assert_eq!(f.state().await["run"], "waiting");
    f
}

#[tokio::test]
async fn channel_reply_completion_exhausted_saved_child_result_never_reconciles_original_handler() {
    let f = Box::pin(child_waiting_finalization()).await;
    assert!(!crate::application::workflow::actions::retry_safe_without_marker("workflow.call"));
    f.p.submit_decision(f.command("approve").await)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE background_tasks SET retry_count=max_retries WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.job.0)
    .execute(f.p.pool())
    .await
    .unwrap();
    assert!(
        f.p.retire_exhausted_work(f.scope, LeasePolicy::new(Duration::from_secs(3)).unwrap())
            .await
            .unwrap()
    );
    assert_eq!(f.state().await["run"], "failed");
    assert_eq!(f.state().await["decision"], Value::Null);
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Refused
    ));
    let children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND parent_run_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(children, 1);
}
