//! The real question wait may own terminal finalization and its protected approval.
use super::*;
use crate::adapters::persistence::workflow::tests::message_tests::support::{lease, runtime};
use crate::application::workflow::{
    polling::WorkflowStepKind,
    triggers::MessageTriggerInput,
    waits::{IngressReply, ReplyIngress, WaitProgress, WorkflowEventId, WorkflowWaits},
    worker::WorkflowHandler,
};

#[derive(sqlx::FromRow)]
struct QuestionTarget {
    message_id: Uuid,
    respondent_id: Uuid,
}
struct FinalQuestion {
    fixture: FinalFixture,
    response: Value,
}

async fn pending_final_question() -> FinalQuestion {
    let mut source = final_source("message.ask");
    source["steps"]["start"]["with"] = json!({
        "source_message":{"ref":"/input/source_message"},
        "content":{"literal":{"body":"Please choose an option."}},
        "deadline":{"literal":(chrono::Utc::now()+chrono::Duration::minutes(10)).to_rfc3339()},
        "payload_schema":{"literal":MessageTriggerInput::schema()}});
    let s = Box::pin(Intents::new()).await;
    let owner_principal = s.owner_principal();
    let fixture = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source,
            contract: literal_contract("Approved response"),
            policy: Some(ReplyApprovalPolicy::new(vec![owner_principal], 60).unwrap()),
            run_as: None,
            publication: FinalPublication::Messaging,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await;
    let claim = fixture
        .p
        .claim_io(fixture.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let runtime = runtime(&fixture.p);
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("message.ask".into()), &claim)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let question:QuestionTarget=sqlx::query_as("SELECT message_id,respondent_id FROM workflow_channel_questions WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company.as_uuid()).bind(fixture.scope.run.as_uuid()).fetch_one(fixture.p.pool()).await.unwrap();
    let incoming = fixture
        .p
        .create_message(&inbound(
            fixture.s.thread,
            &format!("<final-answer-{}@partner.test>", Uuid::new_v4()),
            &[],
            "Chosen option",
        ))
        .await
        .unwrap()
        .canonical_id;
    let response = MessageTriggerInput::new(
        incoming,
        "Chosen option",
        "Question",
        "customer@partner.test",
    )
    .to_value();
    assert_eq!(
        fixture
            .p
            .deliver_reply(IngressReply {
                company: fixture.scope.company,
                thread: fixture.s.thread,
                answers: crate::entities::message::CanonicalMessageId::new(question.message_id),
                responder: Some(question.respondent_id),
                in_app: false,
                event: WorkflowEventId(Uuid::new_v4()),
                payload: response.clone(),
                park_grace_until: chrono::Utc::now() + chrono::Duration::minutes(5)
            })
            .await
            .unwrap(),
        ReplyIngress::Recorded(fixture.scope.run)
    );
    assert!(matches!(
        fixture.p.resume_wait(fixture.scope).await.unwrap(),
        WaitProgress::Pending
    ));
    assert_eq!(fixture.state().await["run"], "waiting");
    assert_eq!(fixture.state().await["proposals"], 1);
    FinalQuestion { fixture, response }
}

#[tokio::test]
async fn channel_reply_final_approval_question_uses_exact_saved_response() {
    let question = Box::pin(pending_final_question()).await;
    let f = &question.fixture;
    f.p.submit_decision(f.command("approve").await)
        .await
        .unwrap();
    let FinalizationProgress::Completed(done) = f.p.resume_finalization(f.scope).await.unwrap()
    else {
        panic!("the original question completion resumes after approval");
    };
    assert_eq!(done.output, question.response);
    assert_eq!(f.state().await["run"], "succeeded");
    assert_eq!(f.state().await["decision"], "pending");
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM task_attempts WHERE task_id=$1")
        .bind(f.scope.job.0)
        .fetch_one(f.p.pool())
        .await
        .unwrap();
    assert_eq!(attempts, 1, "only the real question I/O attempt exists");
    let prompts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_channel_questions WHERE run_id=$1")
            .bind(f.scope.run.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(prompts, 1);
}

#[tokio::test]
async fn channel_reply_final_approval_question_rejection_publishes_no_final() {
    let question = Box::pin(pending_final_question()).await;
    let f = &question.fixture;
    f.p.submit_decision(f.command("reject").await)
        .await
        .unwrap();
    assert_eq!(f.state().await["run"], "failed");
    assert!(f.state().await["decision"].is_null());
    let canonical: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages WHERE company_id=$1 AND direction='outbound'",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(
        canonical, 1,
        "only the declared question, never a private notice or final reply"
    );
}
