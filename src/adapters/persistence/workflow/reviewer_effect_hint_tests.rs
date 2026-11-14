//! Real message publication and receipt consumption separate final authority from received truth.
use super::*;
use crate::adapters::persistence::workflow::tests::message_tests::support as messages;
use crate::application::workflow::{
    completion::CommitDisposition,
    lease::WorkflowHandlerOutcome,
    message::{MessageActionPreparation, MessageArguments, MessageKind},
    polling::WorkflowStepKind,
    worker::WorkflowHandler,
};

#[derive(Clone, Copy)]
pub(super) enum EffectExit {
    Final,
    Next,
    ErrorNext,
}

enum Receipt {
    Accepted,
    Failed,
}

async fn published_message(exit: EffectExit) -> UnusedFinalPolicy {
    let s = Box::pin(Intents::new()).await;
    let receiver = member(&s.fixture, "admin").await;
    let mut source = kind_source("message.send");
    source["steps"]["start"]["with"] = json!({
        "content":{"literal":{"subject":"Receipt scope","body":"An existing result."}},
        "destinations":{"literal":[{"kind":"user","id":receiver.to_string()}]}
    });
    if !matches!(exit, EffectExit::Final) {
        let route = match exit {
            EffectExit::Next => "success",
            EffectExit::ErrorNext => "error",
            EffectExit::Final => unreachable!(),
        };
        source["steps"]["start"]["routes"][route] = json!("finish");
        source["steps"]["finish"] = json!({"type":"data.map","with":{
            "value":{"literal":43},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    }
    // Messaging publication must freeze the real built-in contract. The generic
    // fixture directory intentionally supplies no trusted messaging snapshots.
    let f = Box::pin(unused_final_policy_for(
        s,
        source,
        FinalPublication::Messaging,
    ))
    .await;
    let scope = f.fixture.scope;
    let claim = f
        .fixture
        .p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), messages::lease())
        .await
        .unwrap()
        .unwrap();
    let request = f
        .fixture
        .p
        .message_step_request(&claim, &WorkflowStepKind("message.send".into()))
        .await
        .expect("the real message step must prepare before testing its receipt wait");
    let arguments = MessageArguments::parse(MessageKind::Send, &request.arguments).unwrap();
    f.fixture
        .p
        .message_routes(&request, &arguments)
        .await
        .expect("the fixture must have a real current member email route");
    let outcome = messages::runtime(&f.fixture.p)
        .execute(&WorkflowStepKind("message.send".into()), &claim)
        .await;
    let WorkflowHandlerOutcome::Suspended(parked) = outcome else {
        panic!("actual publication to a real member email must park on its receipt: {outcome:?}");
    };
    assert!(
        f.fixture
            .p
            .confirm_suspension(claim.fence, parked.wait)
            .await
            .unwrap()
    );
    f
}

async fn received_message(exit: EffectExit, receipt: Receipt) -> UnusedFinalPolicy {
    let f = Box::pin(published_message(exit)).await;
    let scope = f.fixture.scope;
    let delivery = messages::delivery(f.fixture.p.pool(), scope.run, 0).await;
    let status = match receipt {
        Receipt::Accepted => "delivered",
        Receipt::Failed => "dead",
    };
    // Existing delivery-ledger fixture supplies transport truth; the production
    // receipt consumer creates its actual canonical receipt. No provider-call claim.
    messages::deliver(f.fixture.p.pool(), delivery, status).await;
    assert_eq!(
        f.fixture
            .p
            .consume_message_receipts(scope, messages::lease())
            .await
            .unwrap(),
        1
    );
    f
}

async fn snapshot(f: &UnusedFinalPolicy) -> Value {
    let mut state = child_snapshot(f).await;
    state["message_effect"] = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'messages',(SELECT jsonb_agg(to_jsonb(message) ORDER BY id) FROM messages AS message WHERE company_id=$1), \
         'deliveries',(SELECT jsonb_agg(to_jsonb(delivery) ORDER BY id) FROM message_deliveries AS delivery WHERE company_id=$1), \
         'actions',(SELECT jsonb_agg(to_jsonb(action) ORDER BY invocation_id) FROM workflow_message_actions AS action WHERE company_id=$1), \
         'receipts',(SELECT jsonb_agg(to_jsonb(receipt) ORDER BY invocation_id) FROM workflow_action_receipts AS receipt WHERE company_id=$1))",
    )
    .bind(f.fixture.scope.company.as_uuid()).fetch_one(f.fixture.p.pool()).await.unwrap();
    state
}

pub(super) fn root_state(f: &UnusedFinalPolicy, snapshot: &Value) -> Value {
    snapshot["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["id"] == json!(f.fixture.scope.run.as_uuid()))
        .unwrap()["state"]
        .clone()
}

#[tokio::test]
async fn workflow_reviewer_follow_pending_message_effect_ignores_unused_retired_final_reviewers() {
    let f = Box::pin(published_message(EffectExit::Final)).await;
    let before = snapshot(&f).await;
    assert_eq!(root_state(&f, &before), "waiting");
    assert_eq!(before["message_effect"]["receipts"], Value::Null);
    for _ in 0..2 {
        assert_eq!(
            f.fixture.p.resume_wait(f.fixture.scope).await.unwrap(),
            WaitProgress::Pending
        );
        assert_eq!(snapshot(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_nonfinal_message_receipt_ignores_unused_retired_final_reviewers()
{
    let f = Box::pin(received_message(EffectExit::Next, Receipt::Accepted)).await;
    let scope = f.fixture.scope;
    let WaitProgress::Completed(done) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("actual message receipt completes nonfinal step");
    };
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert_eq!(done.output["status"], "accepted");
    let successor = done.successor.unwrap();
    let stored = snapshot(&f).await;
    assert_eq!(root_state(&f, &stored), "running");
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(
        stored["message_effect"]["receipts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let WaitProgress::Completed(replay) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("receipt replay");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.output, done.output);
    assert_eq!(replay.successor, Some(successor));
    assert_eq!(snapshot(&f).await, stored);
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
    assert_eq!(snapshot(&f).await, stored);
}

async fn assert_failed(exit: EffectExit, expected: &str) {
    let f = Box::pin(received_message(exit, Receipt::Failed)).await;
    let scope = f.fixture.scope;
    let WaitProgress::Completed(done) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("actual failed receipt uses final-error owner");
    };
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert_eq!(done.route, RouteSelection::FinalError);
    assert_eq!(done.output, Value::Null);
    assert_eq!(done.successor.is_some(), expected == "running");
    let stored = snapshot(&f).await;
    assert_eq!(root_state(&f, &stored), expected);
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(
        stored["message_effect"]["receipts"][0]["result"]["status"],
        "failed"
    );
    let WaitProgress::Completed(replay) = f.fixture.p.resume_wait(scope).await.unwrap() else {
        panic!("failed receipt replay");
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.successor, done.successor);
    assert_eq!(snapshot(&f).await, stored);
}

#[tokio::test]
async fn workflow_reviewer_follow_failed_message_receipt_ignores_unused_retired_final_reviewers() {
    Box::pin(assert_failed(EffectExit::Final, "failed")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_failed_message_error_route_ignores_unused_retired_final_reviewers()
 {
    Box::pin(assert_failed(EffectExit::ErrorNext, "running")).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_fresh_final_message_receipt_requires_current_final_reviewers() {
    let f = Box::pin(received_message(EffectExit::Final, Receipt::Accepted)).await;
    let before = snapshot(&f).await;
    assert!(matches!(
        f.fixture.p.resume_wait(f.fixture.scope).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(snapshot(&f).await, before);
}

struct ReceiptGate {
    transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    backend: i32,
}

async fn receipt_gate(f: &FinalFixture) -> ReceiptGate {
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
    sqlx::query("SAVEPOINT receipt_company_gate")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.scope.company.as_uuid())
        .execute(&mut *transaction)
        .await
        .unwrap();
    ReceiptGate {
        transaction,
        backend,
    }
}

async fn receipt_arrives(exit: EffectExit) {
    let f = Box::pin(published_message(exit)).await;
    let scope = f.fixture.scope;
    let p = &f.fixture.p;
    let delivery = messages::delivery(p.pool(), scope.run, 0).await;
    messages::deliver(p.pool(), delivery, "delivered").await;
    let mut gate = Box::pin(receipt_gate(&f.fixture)).await;
    let resume = p.resume_wait(scope);
    tokio::pin!(resume);
    let observer = tokio::select! {
        result = resume.as_mut() => panic!("pending observation escaped company gate: {result:?}"),
        pid = blocked_by(p.pool(), gate.backend, "%companies%FOR NO KEY UPDATE%") => pid,
    };
    assert_ne!(observer, gate.backend);
    // The real consumer records transport truth while the observer is queued.
    assert_eq!(
        p.consume_message_receipts(scope, messages::lease())
            .await
            .unwrap(),
        1
    );
    let received = snapshot(&f).await;
    assert_eq!(
        received["message_effect"]["receipts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    sqlx::query("ROLLBACK TO SAVEPOINT receipt_company_gate")
        .execute(&mut *gate.transaction)
        .await
        .unwrap();
    let result = tokio::select! {
        result = resume.as_mut() => result,
        _ = blocked_by(p.pool(), gate.backend, "%FROM users%FOR SHARE%") => {
            panic!("an observed receipt cannot acquire late actor accounts after company");
        }
    };
    let stored = snapshot(&f).await;
    if matches!(exit, EffectExit::Final) {
        assert!(matches!(result, Err(AppError::Conflict(_))), "{result:?}");
        assert_eq!(
            stored, received,
            "stale final observation must roll back whole state"
        );
    } else {
        let WaitProgress::Completed(done) = result.unwrap() else {
            panic!("the actual nonfinal receipt must commit while actor UPDATE remains held");
        };
        assert_eq!(done.disposition, CommitDisposition::Committed);
        assert_eq!(done.output["status"], "accepted");
        assert!(done.successor.is_some());
        assert_eq!(root_state(&f, &stored), "running");
        assert_eq!(stored["proposals"], Value::Null);
        assert!(matches!(
            p.resume_wait(scope).await.unwrap(),
            WaitProgress::Completed(_)
        ));
        assert_eq!(snapshot(&f).await, stored);
    }
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
        .bind(scope.company.as_uuid())
        .execute(&mut *gate.transaction)
        .await
        .expect("the actual observer must release company before returning");
    gate.transaction.rollback().await.unwrap();
    if matches!(exit, EffectExit::Final) {
        assert!(matches!(
            p.resume_wait(scope).await,
            Err(AppError::NotFound(_))
        ));
        assert_eq!(
            snapshot(&f).await,
            received,
            "fresh final still requires its current reviewer"
        );
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_final_receipt_arrival_replans_before_new_actor_accounts() {
    Box::pin(receipt_arrives(EffectExit::Final)).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_nonfinal_receipt_arrival_commits_with_actor_account_locked() {
    Box::pin(receipt_arrives(EffectExit::Next)).await;
}
