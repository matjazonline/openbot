use super::*;
use crate::adapters::persistence::workflow::tests::approval_test_support::{
    command, composer, notices,
};
use crate::adapters::persistence::workflow::{
    action_dispatch::PostgresActionDispatch, message::MessageActionAuthority,
};
use crate::adapters::workflow_http::NoLocal;
use crate::application::workflow::{decisions::WorkflowDecisions, message::MessageActions};

#[tokio::test]
async fn channel_reply_hitl_prompt_approval_notice_stays_private_and_then_parks_the_original_question()
 {
    let s = Box::pin(Intents::new()).await;
    let owner = s.owner();
    let pending = Box::pin(pending_with(
        &s,
        1,
        |source| {
            source["steps"]["start"]["with"]["approval"] = json!({"literal":{
            "reviewers":[{"kind":"principal","id":s.owner_principal()}],
            "deadline":(Utc::now()+chrono::Duration::minutes(10)).to_rfc3339()}});
        },
        crate::application::workflow::DeliveryPolicy::Deliver,
    ))
    .await;
    let dispatch = PostgresActionDispatch::new(s.p().clone(), MessageActionAuthority, NoLocal)
        .with_decision_notices(notices(s.p()));
    let runtime = MessageActions::new(
        s.p().clone(),
        dispatch,
        composer(s.p()),
        "agents.test".into(),
        lease(),
    );
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("message.ask".into()), &pending.claim)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    assert_eq!(counts(&s, pending.scope.run).await.questions, 0);
    let canonical: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages WHERE company_id=$1 AND direction='outbound'",
    )
    .bind(pending.scope.company.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        canonical, 0,
        "a private approval notice is never a customer-thread message"
    );
    let reviewers:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_decision_reviewers WHERE company_id=$1 AND notice_delivery_id IS NOT NULL")
        .bind(pending.scope.company.as_uuid()).fetch_one(s.pool()).await.unwrap();
    assert_eq!(reviewers, 1);
    let decision = command(s.p(), pending.scope, owner, "approve").await;
    s.p().submit_decision(decision).await.unwrap();
    let claim = s
        .p()
        .claim_io(pending.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let asked = Box::pin(published(
        &s,
        &PendingQuestion {
            scope: pending.scope,
            claim,
        },
    ))
    .await;
    assert!(matches!(
        s.p().resume_wait(asked.answer).await.unwrap(),
        WaitProgress::Pending
    ));
    let c = counts(&s, asked.run).await;
    assert_eq!(c.questions, 1);
    assert_eq!(c.destinations, 1);
    assert_eq!(c.dispatches, 1);
    assert_eq!(c.waits, 2);
}
