//! Actual memory handler and delivery worker, with the service's contract fake.
use super::*;
use super::effect_hints::{EffectExit, root_state};
use crate::adapters::persistence::workflow::tests::message_tests::support::lease;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::memory_steps_tests::{Memory, memory_for};
use crate::application::memory::{MemoryDeliveryWorker, MemorySettingsStore};
use crate::application::workflow::{
    completion::CommitDisposition, lease::WorkflowHandlerOutcome,
    memory::MemoryStepRuntime, polling::WorkflowStepKind, worker::WorkflowHandler,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct MemoryWait {
    configured: UnusedFinalPolicy,
    service: Memory,
}

async fn parked_memory(exit: EffectExit) -> MemoryWait {
    let s = Box::pin(Intents::new()).await;
    let mut source = kind_source("memory.save");
    if !matches!(exit, EffectExit::Final) {
        let route = if matches!(exit, EffectExit::Next) {
            "success"
        } else {
            "error"
        };
        source["steps"]["start"]["routes"][route] = json!("finish");
        source["steps"]["finish"] = json!({"type":"data.map","with":{
            "value":{"literal":43},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    }
    let configured = Box::pin(unused_final_policy(s, source)).await;
    let f = &configured.fixture;
    let service = Box::pin(memory_for(&f.p, f.scope.company, f.s.owner())).await;
    let p = &service.persistence;
    let claim = p
        .claim_io(f.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let runtime = MemoryStepRuntime::new(p.clone(), service.client.clone());
    let outcome = Box::pin(runtime.execute(&WorkflowStepKind("memory.save".into()), &claim)).await;
    let WorkflowHandlerOutcome::Suspended(parked) = outcome else {
        panic!("actual memory.save must park its delivery intent: {outcome:?}");
    };
    assert!(
        p.confirm_suspension(claim.fence, parked.wait)
            .await
            .unwrap()
    );
    MemoryWait {
        configured,
        service,
    }
}

async fn delivered(f: &MemoryWait) {
    let worker = MemoryDeliveryWorker::new(
        Arc::new(f.service.persistence.clone()),
        f.service.client.clone(),
    );
    assert!(worker.pass(&CancellationToken::new()).await.unwrap());
    let state: String = sqlx::query_scalar(
        "SELECT state FROM memory_deliveries WHERE company_id=$1 AND execution_id=$2",
    )
    .bind(f.configured.fixture.scope.company.as_uuid())
    .bind(f.configured.fixture.scope.execution.as_uuid())
    .fetch_one(f.service.persistence.pool())
    .await
    .unwrap();
    assert_eq!(state, "delivered");
}

async fn snapshot(f: &MemoryWait) -> Value {
    let mut result = child_snapshot(&f.configured).await;
    result["memory_effect"] = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(intent) ORDER BY id) FROM memory_deliveries AS intent WHERE company_id=$1",
    ).bind(f.configured.fixture.scope.company.as_uuid())
        .fetch_one(f.service.persistence.pool()).await.unwrap();
    result
}

#[tokio::test]
async fn workflow_reviewer_follow_pending_memory_effect_ignores_unused_retired_final_reviewers() {
    let f = Box::pin(parked_memory(EffectExit::Final)).await;
    let before = snapshot(&f).await;
    for _ in 0..2 {
        assert_eq!(
            f.service
                .persistence
                .resume_wait(f.configured.fixture.scope)
                .await
                .unwrap(),
            WaitProgress::Pending
        );
        assert_eq!(snapshot(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_nonfinal_memory_receipt_ignores_unused_retired_final_reviewers() {
    let f = Box::pin(parked_memory(EffectExit::Next)).await;
    Box::pin(delivered(&f)).await;
    let p = &f.service.persistence;
    let scope = f.configured.fixture.scope;
    let WaitProgress::Completed(done) = p.resume_wait(scope).await.unwrap() else {
        panic!("received save must complete");
    };
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert_eq!(done.output["saved"].as_array().unwrap().len(), 1);
    let next = done.successor.unwrap();
    let stored = snapshot(&f).await;
    assert_eq!(root_state(&f.configured, &stored), "running");
    assert_eq!(stored["proposals"], Value::Null);
    let WaitProgress::Completed(replay) = p.resume_wait(scope).await.unwrap() else {
        panic!("saved receipt replay");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.output, done.output);
    assert_eq!(replay.successor, Some(next));
    assert_eq!(snapshot(&f).await, stored);
    assert!(matches!(
        p.advance_pure(next, BatchBudget::new(4, Duration::from_secs(1)).unwrap())
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(snapshot(&f).await, stored);
}

async fn failed_memory(exit: EffectExit, expected: &str) {
    let f = Box::pin(parked_memory(exit)).await;
    let p = &f.service.persistence;
    let scope = f.configured.fixture.scope;
    p.disable_memory(scope.company, f.configured.fixture.s.owner(), 1)
        .await
        .unwrap();
    let before = snapshot(&f).await;
    assert_eq!(before["memory_effect"][0]["state"], "failed");
    assert_eq!(before["memory_effect"][0]["outcome_code"], "disabled");
    let WaitProgress::Completed(done) = p.resume_wait(scope).await.unwrap() else {
        panic!("actual memory failure must route");
    };
    assert_eq!(done.route, RouteSelection::FinalError);
    assert_eq!(done.successor.is_some(), expected == "running");
    let stored = snapshot(&f).await;
    assert_eq!(root_state(&f.configured, &stored), expected);
    assert_eq!(stored["proposals"], Value::Null);
    let WaitProgress::Completed(replay) = p.resume_wait(scope).await.unwrap() else {
        panic!("memory failure replay");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.successor, done.successor);
    assert_eq!(snapshot(&f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_failed_memory_effect_ignores_unused_retired_final_reviewers() {
    Box::pin(failed_memory(EffectExit::Final, "failed")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_failed_memory_error_route_ignores_unused_retired_final_reviewers()
{
    Box::pin(failed_memory(EffectExit::ErrorNext, "running")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_fresh_final_memory_receipt_requires_current_final_reviewers() {
    let f = Box::pin(parked_memory(EffectExit::Final)).await;
    Box::pin(delivered(&f)).await;
    let before = snapshot(&f).await;
    assert!(matches!(
        f.service
            .persistence
            .resume_wait(f.configured.fixture.scope)
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(snapshot(&f).await, before);
}
