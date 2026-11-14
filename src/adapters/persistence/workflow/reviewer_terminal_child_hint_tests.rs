//! Received child truth only consumes final reviewer accounts for an actual final effect.
use super::*;
use crate::application::workflow::{
    CancelCommand, CancelResult, IdempotencyKey, WorkflowInspection, WorkflowRunTransitions,
    completion::CommitDisposition,
};

async fn parked(leaf: Value, exit: ChildExit) -> UnusedFinalPolicy {
    let f = Box::pin(unused_child_with(leaf, exit)).await;
    f.fixture
        .p
        .park_child(f.fixture.scope)
        .await
        .unwrap()
        .unwrap();
    f
}

async fn successful(exit: ChildExit) -> UnusedFinalPolicy {
    let f = Box::pin(parked(kind_source("channel.no_reply"), exit)).await;
    let child = Box::pin(child_scope(&f)).await;
    assert_eq!(
        f.fixture
            .p
            .advance_pure(child, BatchBudget::new(4, Duration::from_secs(1)).unwrap())
            .await
            .unwrap()
            .disposition,
        BatchDisposition::Completed
    );
    f
}

async fn cancelled(exit: ChildExit) -> UnusedFinalPolicy {
    let f = Box::pin(parked(kind_source("channel.no_reply"), exit)).await;
    let child = Box::pin(child_scope(&f)).await;
    let p = &f.fixture.p;
    let command = CancelCommand {
        company_id: child.company,
        run_id: child.run,
        actor: f.fixture.s.owner(),
        command_key: IdempotencyKey::parse("cancel-child-result").unwrap(),
        expected_revision: p
            .head(child.company, child.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    assert!(matches!(
        p.cancel(command).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    f
}

fn root_state(f: &UnusedFinalPolicy, snapshot: &Value) -> Value {
    snapshot["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["id"] == json!(f.fixture.scope.run.as_uuid()))
        .unwrap()["state"]
        .clone()
}

async fn assert_failure(f: &UnusedFinalPolicy, expected: &str) {
    let scope = f.fixture.scope;
    let WaitProgress::Completed(done) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("actual child failure must settle the parent through the final-error owner");
    };
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert_eq!(done.route, RouteSelection::FinalError);
    assert_eq!(done.output, Value::Null);
    let stored = child_snapshot(f).await;
    assert_eq!(root_state(f, &stored), expected);
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(stored["decisions"], Value::Null);
    assert_eq!(done.successor.is_some(), expected == "running");
    let WaitProgress::Completed(replay) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("actual child failure must replay its saved final-error result");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.route, done.route);
    assert_eq!(replay.output, done.output);
    assert_eq!(replay.successor, done.successor);
    assert_eq!(child_snapshot(f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_nonfinal_child_result_ignores_unused_retired_final_reviewers() {
    let f = Box::pin(successful(ChildExit::Next)).await;
    let scope = f.fixture.scope;
    let WaitProgress::Completed(done) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("actual child success must complete its nonfinal parent step");
    };
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert_eq!(done.route, RouteSelection::Success);
    let successor = done.successor.unwrap();
    let stored = child_snapshot(&f).await;
    assert_eq!(root_state(&f, &stored), "running");
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(stored["decisions"], Value::Null);
    let WaitProgress::Completed(replay) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("completed nonfinal child result must replay");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.output, done.output);
    assert_eq!(replay.successor, Some(successor));
    assert_eq!(child_snapshot(&f).await, stored);
    assert!(matches!(
        f.fixture
            .p
            .advance_pure(
                successor,
                BatchBudget::new(4, Duration::from_secs(1)).unwrap()
            )
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(child_snapshot(&f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_cancelled_child_result_ignores_unused_retired_final_reviewers() {
    let f = Box::pin(cancelled(ChildExit::Final)).await;
    Box::pin(assert_failure(&f, "failed")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_child_failure_error_route_ignores_unused_retired_final_reviewers()
{
    let f = Box::pin(cancelled(ChildExit::ErrorNext)).await;
    Box::pin(assert_failure(&f, "running")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_failed_child_result_ignores_unused_retired_final_reviewers() {
    let f = Box::pin(parked(kind_source("wait.event"), ChildExit::Final)).await;
    let child = Box::pin(child_scope(&f)).await;
    let parked = f.fixture.p.park_wait(child).await.unwrap().unwrap();
    sqlx::query(
        "UPDATE workflow_waits SET deadline=clock_timestamp() WHERE company_id=$1 AND id=$2",
    )
    .bind(child.company.as_uuid())
    .bind(parked.wait.as_uuid())
    .execute(f.fixture.p.pool())
    .await
    .unwrap();
    assert_eq!(
        f.fixture.p.resume_wait(child).await.unwrap(),
        WaitProgress::Expired(CommitDisposition::Committed)
    );
    Box::pin(assert_failure(&f, "failed")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_fresh_final_child_result_requires_current_final_reviewers() {
    let f = Box::pin(successful(ChildExit::Final)).await;
    let before = child_snapshot(&f).await;
    assert!(matches!(
        f.fixture.p.resume_wait(f.fixture.scope).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(child_snapshot(&f).await, before);
}

enum RepeatCase {
    Done,
    Exhausted,
    Next,
}

async fn repeated(case: RepeatCase) -> UnusedFinalPolicy {
    let mut parent = kind_source("flow.repeat");
    parent["steps"]["start"]["repeat"]["exit_when"] =
        json!({"literal":matches!(case, RepeatCase::Done)});
    parent["steps"]["start"]["repeat"]["next_input"] =
        parent["steps"]["start"]["with"]["input"].clone();
    parent["steps"]["start"]["max_iterations"] = json!(if matches!(case, RepeatCase::Next) {
        2
    } else {
        1
    });
    parent["steps"]["start"]["routes"]["choices"] = json!({"done":"finish","exhausted":"finish"});
    parent["steps"]["finish"] = json!({"type":"data.map","with":{
        "value":{"literal":43},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    let f = Box::pin(unused_child_setup(ChildSetup {
        leaf: kind_source("channel.no_reply"),
        parent,
        exit: ChildExit::Final,
    }))
    .await;
    f.fixture
        .p
        .park_child(f.fixture.scope)
        .await
        .unwrap()
        .unwrap();
    let child = Box::pin(child_scope(&f)).await;
    assert_eq!(
        f.fixture
            .p
            .advance_pure(child, BatchBudget::new(4, Duration::from_secs(1)).unwrap())
            .await
            .unwrap()
            .disposition,
        BatchDisposition::Completed
    );
    f
}

async fn assert_repeat_finish(case: RepeatCase, expected: &str) {
    let f = Box::pin(repeated(case)).await;
    let scope = f.fixture.scope;
    let WaitProgress::Completed(done) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("the actual repeat completion must reach its nonfinal route");
    };
    let RouteSelection::Choice(choice) = &done.route else {
        panic!("repeat choice");
    };
    assert_eq!(choice.as_str(), expected);
    assert_eq!(done.output["rounds"], 1);
    assert_eq!(done.disposition, CommitDisposition::Committed);
    let successor = done.successor.unwrap();
    let stored = child_snapshot(&f).await;
    assert_eq!(root_state(&f, &stored), "running");
    assert_eq!(stored["proposals"], Value::Null);
    let WaitProgress::Completed(replay) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("repeat replay");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.output, done.output);
    assert_eq!(replay.route, done.route);
    assert_eq!(child_snapshot(&f).await, stored);
    assert!(matches!(
        f.fixture
            .p
            .advance_pure(
                successor,
                BatchBudget::new(4, Duration::from_secs(1)).unwrap()
            )
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(child_snapshot(&f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_repeat_done_ignores_unused_retired_final_reviewers() {
    Box::pin(assert_repeat_finish(RepeatCase::Done, "done")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_repeat_exhausted_ignores_unused_retired_final_reviewers() {
    Box::pin(assert_repeat_finish(RepeatCase::Exhausted, "exhausted")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_repeat_next_round_ignores_unused_retired_final_reviewers() {
    let f = Box::pin(repeated(RepeatCase::Next)).await;
    let scope = f.fixture.scope;
    assert_eq!(
        f.fixture.p.resume_wait(scope).await.unwrap(),
        WaitProgress::NextRound
    );
    let stored = child_snapshot(&f).await;
    assert_eq!(root_state(&f, &stored), "waiting");
    assert_eq!(stored["runs"].as_array().unwrap().len(), 3);
    assert_eq!(
        stored["waits"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|wait| wait["state"] == "waiting")
            .count(),
        1
    );
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(stored["decisions"], Value::Null);
    assert_eq!(
        f.fixture.p.resume_wait(scope).await.unwrap(),
        WaitProgress::Pending
    );
    assert_eq!(
        child_snapshot(&f).await,
        stored,
        "pending round replay must not debit or create another child"
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_nonfinal_child_arrival_commits_without_unused_actor_lock() {
    let f = Box::pin(parked(kind_source("channel.no_reply"), ChildExit::Next)).await;
    let scope = f.fixture.scope;
    let child_scope = Box::pin(child_scope(&f)).await;
    let mut gate = Box::pin(child_gate(&f, child_scope)).await;
    let child = f.fixture.p.advance_pure(
        child_scope,
        BatchBudget::new(4, Duration::from_secs(1)).unwrap(),
    );
    let parent = f.fixture.p.resume_wait(scope);
    tokio::pin!(child, parent);
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
    let WaitProgress::Completed(done) = arrival.resumed.unwrap() else {
        panic!("both actual child and nonfinal parent must commit with actor UPDATE still held");
    };
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert_eq!(done.route, RouteSelection::Success);
    assert!(done.successor.is_some());
    let stored = child_snapshot(&f).await;
    assert_eq!(root_state(&f, &stored), "running");
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(stored["decisions"], Value::Null);
    let WaitProgress::Completed(replay) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("nonfinal child replay");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.output, done.output);
    assert_eq!(replay.successor, done.successor);
    assert_eq!(child_snapshot(&f).await, stored);
    gate.transaction.rollback().await.unwrap();
}
