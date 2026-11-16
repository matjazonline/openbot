use super::*;
use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};

#[derive(sqlx::FromRow)]
struct HandledExecution {
    committed_route: String,
    committed_output: Value,
    has_successor: bool,
}

async fn park_and_expire(f: &GuardFixture) {
    assert!(
        f.p()
            .park_decision(f.scope, &notices(f.p()))
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        f.p().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Pending
    );
    sqlx::query(
        "UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds', \
         deadline=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2 AND state='waiting'",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .execute(f.p().pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn workflow_escalation_trigger_decision_deadline_fails_real_wait_and_delivers_once() {
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    park_and_expire(&f).await;
    assert_eq!(
        f.p().sweep_waits(128).await.unwrap(),
        1,
        "the actual wait sweeper owns the expired human decision"
    );
    let outcome: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('run',escalation.run_id,'execution',escalation.execution_id, \
          'reason',escalation.reason_code,'step',escalation.step_id,'wait',wait.state,'job',job.status) \
         FROM workflow_escalation_cases AS escalation \
         JOIN workflow_waits AS wait ON wait.company_id=escalation.company_id AND wait.execution_id=escalation.execution_id \
         JOIN background_tasks AS job ON job.company_id=wait.company_id AND job.workflow_execution_id=wait.execution_id \
         WHERE escalation.company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(
        outcome,
        json!({"run":f.scope.run.as_uuid(),"execution":f.scope.execution.as_uuid(),
        "reason":"decision.timed_out","step":"start","wait":"expired","job":"failed"})
    );
    assert_eq!(f.p().sweep_waits(128).await.unwrap(), 0);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":1,"controls":2,"hops":1,"notices":1,"guard_events":0})
    );
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_escalation_person_receipts WHERE company_id=$1 AND outcome='email_queued'",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(receipts, 1);
}

#[tokio::test]
async fn workflow_escalation_trigger_decision_authored_handler_suppresses_ladder_even_at_end() {
    for route in ["$end", "handled"] {
        let people = Box::pin(PeopleFixture::new(1)).await;
        let mut source: Value =
            serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
        source["input_schema"] = json!(true);
        source["steps"]["start"]["with"]["data_schema"] = json!({"literal":true});
        source["steps"]["start"]["with"]["feedback_required"] = json!({"literal":[]});
        source["steps"]["start"]["with"]["reviewers"] = json!({"literal":["reviewer"]});
        source["steps"]["start"]["with"]["deadline"] =
            json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
        source["steps"]["start"]["routes"]["error"] = json!(route);
        if route == "handled" {
            source["steps"]["handled"] = json!({"type":"data.map", "with":{
                "value":{"literal":1},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
        }
        let version = republish(&people, source).await;
        let f = Box::pin(GuardFixture::admit(people, Some(policy()), Some(version))).await;
        park_and_expire(&f).await;
        assert_eq!(f.p().sweep_waits(128).await.unwrap(), 1);
        let accepted: HandledExecution = sqlx::query_as(
            "SELECT committed_route,committed_output,successor_execution_id IS NOT NULL AS has_successor \
             FROM workflow_executions WHERE company_id=$1 AND id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .fetch_one(f.p().pool())
        .await
        .unwrap();
        assert_eq!(accepted.committed_route, "final_error");
        assert_eq!(accepted.committed_output, Value::Null);
        assert_eq!(accepted.has_successor, route == "handled");
        let state = f.state().await;
        for key in ["cases", "controls", "hops", "notices"] {
            assert_eq!(state[key], 0, "{route}: {key}");
        }
        if route == "$end" {
            assert_eq!(
                state["state"], "failed",
                "an authored terminal handler still suppresses the ladder"
            );
            let eligible: bool = sqlx::query_scalar(
                "SELECT workflow_escalation_run_source_valid($1,$2,$3,'decision.timed_out','start')",
            )
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .bind(f.scope.execution.as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
            assert!(
                !eligible,
                "the SQL source guard also rejects a consumed failure"
            );
        }
        assert_eq!(f.p().sweep_waits(128).await.unwrap(), 0);
    }
}

#[tokio::test]
async fn workflow_escalation_competing_decision_sweepers_commit_one_failure_case() {
    let f = Box::pin(GuardFixture::new(Some(policy()))).await;
    park_and_expire(&f).await;
    let barrier = tokio::sync::Barrier::new(3);
    let first = async {
        barrier.wait().await;
        f.p().sweep_waits(128).await.unwrap()
    };
    let second = async {
        barrier.wait().await;
        f.p().sweep_waits(128).await.unwrap()
    };
    let (first, second, _) = tokio::join!(first, second, barrier.wait());
    assert_eq!(first + second, 1);
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":1,"controls":1,"hops":0,"notices":0,"guard_events":0})
    );
}
