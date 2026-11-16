use super::*;
use crate::adapters::persistence::workflow::tests::message_tests::sql_tests::sql_publication;
use crate::application::agent::ToolCallOrigin;
use crate::application::workflow::{actions::ModelToolCallId, completion::*, message::*};

struct Filled {
    fixture: FinalFixture,
    claim: crate::application::workflow::lease::ClaimedWorkflow,
    output: Value,
    policy: LeasePolicy,
}

async fn filled(count: usize, approval: bool) -> Filled {
    let s = Intents::new().await;
    let other = new_channel(&s.fixture, "slot-target").await;
    let mut source: Value =
        serde_json::from_str(&registry::example("message.send").unwrap().source).unwrap();
    source["steps"]["start"]["with"] = json!({"destinations":{"literal":[{"kind":"channel","id":other}]},"content":{"literal":{"subject":"Explicit","body":"seed"}}});
    let policy = approval.then(|| ReplyApprovalPolicy::new(vec![s.owner_principal()], 60).unwrap());
    let fixture = FinalFixture::from_case(
        s,
        FinalCase {
            source,
            contract: literal_contract("Done"),
            policy,
            run_as: None,
            publication: FinalPublication::Messaging,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    )
    .await;
    let policy = LeasePolicy::new(Duration::from_secs(30)).unwrap();
    let claim = fixture
        .p
        .claim_io(
            fixture.scope,
            crate::application::workflow::lease::WorkflowWorkerId(Uuid::new_v4()),
            policy,
        )
        .await
        .unwrap()
        .unwrap();
    let runtime = crate::adapters::workflow_message::MessageRuntime::new(
        fixture.p.clone(),
        approval_test_support::composer(&fixture.p),
        "agents.test".into(),
        policy,
    );
    let observed=runtime.call_tool(claim.fence,MessageToolCall {origin:ToolCallOrigin::Workflow,kind:MessageKind::Send,
        arguments:json!({"destinations":[{"kind":"channel","id":other}],"content":{"subject":"Explicit","body":"seed"}}),
        call:ModelToolCallId::parse("seed").unwrap()}).await.unwrap();
    let MessageObservation::Committed(output) = observed else {
        panic!("explicit seed was not published")
    };
    let invocation: Uuid=sqlx::query_scalar("SELECT invocation_id FROM workflow_action_dispatches WHERE company_id=$1 AND execution_id=$2")
        .bind(fixture.scope.company.as_uuid()).bind(fixture.scope.execution.as_uuid()).fetch_one(fixture.p.pool()).await.unwrap();
    let mut tx = fixture.p.pool().begin().await.unwrap();
    for _ in 1..count {
        sql_publication(&mut tx, invocation).await.unwrap();
    }
    tx.commit().await.unwrap();
    Filled {
        fixture,
        claim,
        output: output.result,
        policy,
    }
}

async fn slots(f: &FinalFixture) -> i64 {
    sqlx::query_scalar("SELECT workflow_message_slots($1,$2)")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .fetch_one(f.p.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn channel_reply_completion_reserves_128th_slot_once_under_competing_finalizers() {
    let f = Box::pin(filled(127, false)).await;
    let barrier = tokio::sync::Barrier::new(2);
    let complete = || async {
        barrier.wait().await;
        f.fixture
            .p
            .complete_io(FencedWorkflowResult {
                fence: f.claim.fence,
                output: f.output.clone(),
                policy: f.policy,
            })
            .await
            .unwrap()
    };
    let (a, b) = tokio::join!(complete(), complete());
    assert!(a.is_some() && b.is_some());
    assert_eq!(slots(&f.fixture).await, 128);
    assert_eq!(f.fixture.state().await["proposals"], 1);
    assert_eq!(f.fixture.state().await["decision"], "pending");
    let publisher = super::publication_tests::publisher(&f.fixture);
    let claim = super::publication_tests::claim(&f.fixture).await;
    assert!(matches!(
        publisher.publish_claim(claim).await.unwrap(),
        ReplyPublicationOutcome::Published(_)
    ));
    assert_eq!(
        slots(&f.fixture).await,
        128,
        "publication does not debit a second slot at the ceiling"
    );
}

#[tokio::test]
async fn channel_reply_completion_full_execution_rejects_final_without_another_slot() {
    let f = Box::pin(filled(128, false)).await;
    assert!(
        f.fixture
            .p
            .complete_io(FencedWorkflowResult {
                fence: f.claim.fence,
                output: f.output,
                policy: f.policy
            })
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(slots(&f.fixture).await, 128);
    assert_eq!(f.fixture.state().await["proposals"], 0);
    assert_eq!(f.fixture.state().await["decision"], Value::Null);
    assert_eq!(f.fixture.state().await["run"], "failed");
}

#[tokio::test]
async fn channel_reply_completion_rejected_approval_keeps_its_slot_reservation() {
    let f = Box::pin(filled(127, true)).await;
    assert!(
        f.fixture
            .p
            .complete_io(FencedWorkflowResult {
                fence: f.claim.fence,
                output: f.output,
                policy: f.policy
            })
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(slots(&f.fixture).await, 128);
    f.fixture
        .p
        .submit_decision(f.fixture.command("reject").await)
        .await
        .unwrap();
    assert_eq!(slots(&f.fixture).await, 128);
    assert_eq!(f.fixture.state().await["run"], "failed");
}
