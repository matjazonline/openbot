//! Actual pending and nonfinal wait resumes do not consume final-response authority.
use super::*;
use crate::application::workflow::{
    CancelCommand, CancelResult, IdempotencyKey, WorkflowInspection, WorkflowRunTransitions,
    waits::*,
};

enum WaitCase {
    FutureTimer,
    NonfinalTimer,
    FinalTimer,
    Event,
    FinalEvent,
}

fn source(case: WaitCase) -> Value {
    let kind = if matches!(case, WaitCase::Event | WaitCase::FinalEvent) {
        "wait.event"
    } else {
        "wait.timer"
    };
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["output_schema"] = json!(true);
    let offset = if matches!(case, WaitCase::NonfinalTimer | WaitCase::FinalTimer) {
        chrono::Duration::seconds(-1)
    } else {
        chrono::Duration::hours(1)
    };
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+offset).to_rfc3339()});
    if matches!(case, WaitCase::NonfinalTimer | WaitCase::Event) {
        source["steps"]["start"]["routes"]["success"] = json!("finish");
        source["steps"]["finish"] = json!({"type":"data.map","with":{
            "value":{"literal":42},"output_schema":{"literal":true}},
            "routes":{"success":"$end"}});
    }
    source
}

async fn parked(case: WaitCase) -> ConfiguredFinal {
    let configured = Box::pin(configured_final(source(case))).await;
    let f = &configured.fixture;
    f.p.park_wait(f.scope).await.unwrap().unwrap();
    f.p.delete_user_account(configured.reviewer).await.unwrap();
    configured
}

async fn snapshot(f: &FinalFixture) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('base',$3::jsonb, \
         'waits',(SELECT jsonb_agg(to_jsonb(wait) ORDER BY id) FROM workflow_waits AS wait WHERE company_id=$1 AND run_id=$2), \
         'signals',(SELECT jsonb_agg(to_jsonb(event) ORDER BY id) FROM workflow_wait_events AS event WHERE company_id=$1 AND run_id=$2))",
    )
    .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(state(f).await)
    .fetch_one(f.p.pool()).await.unwrap()
}

fn event_for(f: &FinalFixture, before: &Value) -> WorkflowEvent {
    let wait = &before["waits"][0];
    WorkflowEvent {
        company: f.scope.company,
        run: f.scope.run,
        id: WorkflowEventId(Uuid::new_v4()),
        name: WorkflowEventName(wait["event_name"].as_str().unwrap().into()),
        correlation: WorkflowEventCorrelation(wait["correlation"].as_str().unwrap().into()),
        payload: json!("accepted"),
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_future_timer_resume_ignores_unused_retired_final_reviewers() {
    let configured = Box::pin(parked(WaitCase::FutureTimer)).await;
    let f = &configured.fixture;
    let before = snapshot(f).await;
    for _ in 0..2 {
        assert_eq!(
            f.p.resume_wait(f.scope).await.unwrap(),
            WaitProgress::Pending
        );
        assert_eq!(snapshot(f).await, before);
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_cancelled_wait_resume_ignores_unused_retired_final_reviewers() {
    let configured = Box::pin(parked(WaitCase::FutureTimer)).await;
    let f = &configured.fixture;
    let command = CancelCommand {
        company_id: f.scope.company,
        run_id: f.scope.run,
        actor: f.s.owner(),
        command_key: IdempotencyKey::parse("cancel-wait-observation").unwrap(),
        expected_revision: f
            .p
            .head(f.scope.company, f.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    assert!(matches!(
        f.p.cancel(command).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    let before = snapshot(f).await;
    assert_eq!(before["base"]["run"]["state"], "cancelled");
    assert_eq!(
        f.p.resume_wait(f.scope).await.unwrap(),
        WaitProgress::Refused
    );
    assert_eq!(snapshot(f).await, before);
}

#[tokio::test]
async fn workflow_reviewer_follow_nonfinal_timer_resume_ignores_unused_retired_final_reviewers() {
    let configured = Box::pin(parked(WaitCase::NonfinalTimer)).await;
    let f = &configured.fixture;
    let WaitProgress::Completed(done) = f.p.resume_wait(f.scope).await.unwrap() else {
        panic!("the actual due timer must complete its nonfinal step");
    };
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert!(done.successor.is_some());
    let stored = snapshot(f).await;
    assert_eq!(stored["base"]["run"]["state"], "running");
    assert_eq!(stored["base"]["proposals"], Value::Null);
    assert_eq!(stored["base"]["reply"], Value::Null);
    let WaitProgress::Completed(replay) = f.p.resume_wait(f.scope).await.unwrap() else {
        panic!("the completed timer must replay");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.successor, done.successor);
    assert_eq!(replay.output, done.output);
    assert_eq!(snapshot(f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_event_resume_ignores_unused_retired_final_reviewers() {
    let configured = Box::pin(parked(WaitCase::Event)).await;
    let f = &configured.fixture;
    let before = snapshot(f).await;
    assert_eq!(
        f.p.resume_wait(f.scope).await.unwrap(),
        WaitProgress::Pending
    );
    assert_eq!(snapshot(f).await, before);
    let event = event_for(f, &before);
    assert!(matches!(
        f.p.record_event(event.clone()).await.unwrap(),
        EventIngress::Recorded
    ));
    let WaitProgress::Completed(done) = f.p.resume_wait(f.scope).await.unwrap() else {
        panic!("the actual recorded event must complete its nonfinal step");
    };
    assert_eq!(done.output, event.payload);
    assert!(done.successor.is_some());
    let stored = snapshot(f).await;
    assert_eq!(stored["base"]["proposals"], Value::Null);
    assert_eq!(
        stored["signals"][0]["consumed_execution_id"],
        json!(f.scope.execution.as_uuid())
    );
    assert!(matches!(
        f.p.record_event(event).await.unwrap(),
        EventIngress::Duplicate
    ));
    let WaitProgress::Completed(replay) = f.p.resume_wait(f.scope).await.unwrap() else {
        panic!("the completed event must replay");
    };
    assert_eq!(replay.output, done.output);
    assert_eq!(replay.successor, done.successor);
    assert_eq!(snapshot(f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_fresh_final_timer_resume_requires_current_final_reviewers() {
    let configured = Box::pin(parked(WaitCase::FinalTimer)).await;
    let f = &configured.fixture;
    let before = snapshot(f).await;
    assert!(matches!(
        f.p.resume_wait(f.scope).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(snapshot(f).await, before);
}

struct ObservationGate {
    transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    backend: i32,
}

async fn observation_gate(f: &FinalFixture) -> ObservationGate {
    let mut transaction = f.p.pool().begin().await.unwrap();
    let backend = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.s.owner().user_id())
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SAVEPOINT wait_observation_company_gate")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.scope.company.as_uuid())
        .execute(&mut *transaction)
        .await
        .unwrap();
    ObservationGate {
        transaction,
        backend,
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_event_arrival_replans_before_new_actor_accounts() {
    let configured = Box::pin(configured_final(source(WaitCase::FinalEvent))).await;
    let f = &configured.fixture;
    f.p.park_wait(f.scope).await.unwrap().unwrap();
    let before = snapshot(f).await;
    let mut gate = Box::pin(observation_gate(f)).await;
    let resume = f.p.resume_wait(f.scope);
    tokio::pin!(resume);
    let observer = tokio::select! {
        result = resume.as_mut() => panic!("pending observation escaped company gate: {result:?}"),
        pid = blocked_by(f.p.pool(), gate.backend, "%companies%FOR NO KEY UPDATE%") => pid,
    };
    assert_ne!(observer, gate.backend);
    // The actual producer can commit the event while company ownership is queued.
    let event = event_for(f, &before);
    assert!(matches!(
        f.p.record_event(event).await.unwrap(),
        EventIngress::Recorded
    ));
    let received = snapshot(f).await;
    assert_eq!(received["signals"][0]["consumed_execution_id"], Value::Null);
    sqlx::query("ROLLBACK TO SAVEPOINT wait_observation_company_gate")
        .execute(&mut *gate.transaction)
        .await
        .unwrap();
    let result = tokio::select! {
        result = resume.as_mut() => result,
        _ = blocked_by(f.p.pool(), gate.backend, "%FROM users%FOR SHARE%") => {
            panic!("an event observer may not acquire newly required actor accounts after company");
        }
    };
    assert!(matches!(result, Err(AppError::Conflict(_))), "{result:?}");
    assert_eq!(snapshot(f).await, received);
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
        .bind(f.scope.company.as_uuid())
        .execute(&mut *gate.transaction)
        .await
        .expect("the stale observer must release company before returning its retry result");
    gate.transaction.rollback().await.unwrap();
    assert_eq!(
        f.p.resume_wait(f.scope).await.unwrap(),
        WaitProgress::Pending
    );
    let parked = snapshot(f).await;
    assert_eq!(parked["base"]["run"]["state"], "waiting");
    assert_eq!(parked["base"]["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(
        parked["signals"][0]["consumed_execution_id"],
        json!(f.scope.execution.as_uuid())
    );
    assert_eq!(
        f.p.resume_wait(f.scope).await.unwrap(),
        WaitProgress::Pending
    );
    assert_eq!(snapshot(f).await, parked);
}
