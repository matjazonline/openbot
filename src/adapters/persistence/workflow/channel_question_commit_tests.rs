use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    Gate,GatePool,Stall,connect,resolution_behind};

struct QuestionPool<'a>(&'a Intents);
impl GatePool for QuestionPool<'_> {
    fn gate_pool(&self) -> &sqlx::PgPool {
        self.0.pool()
    }
}

#[tokio::test]
async fn channel_reply_hitl_prompt_lost_commit_ack_replays_human_wait_before_transport_receipt() {
    let _injected = Injected::mark();
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending_with(
        &s,
        1,
        |_| {},
        crate::application::workflow::DeliveryPolicy::InAppOnly,
    ))
    .await;
    let fixture = QuestionPool(&s);
    let (mut observer, _) = connect(&fixture).await;
    let mut gate = Gate::arm(
        &fixture,
        "workflow_channel_questions",
        "INSERT",
        "true",
        Stall::Commit,
    )
    .await;
    let runtime = runtime(s.p());
    let claim = pending.claim.clone();
    let publishing = tokio::spawn(async move {
        runtime
            .execute(&WorkflowStepKind("message.ask".into()), &claim)
            .await
    });
    let stalled = gate
        .stalled(&mut observer, &[&|| !publishing.is_finished()])
        .await;
    resolution_behind(
        &mut observer,
        stalled,
        lease(),
        &[&|| !publishing.is_finished()],
    )
    .await;
    gate.release().await;
    let outcome = publishing.await.unwrap();
    assert!(
        matches!(outcome, WorkflowHandlerOutcome::Suspended(_)),
        "{outcome:?}"
    );
    let c = counts(&s, pending.scope.run).await;
    assert_eq!(c.questions, 1);
    assert_eq!(c.waits, 1);
    assert_eq!(c.destinations, 1);
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_action_receipts WHERE run_id=$1")
            .bind(pending.scope.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(receipts, 1);
    assert!(matches!(
        s.p().resume_wait(pending.scope).await.unwrap(),
        WaitProgress::Pending
    ));
}

#[tokio::test]
async fn channel_reply_hitl_prompt_deferred_commit_cannot_outlive_its_real_claim() {
    let _injected = Injected::mark();
    let s = Box::pin(Intents::new()).await;
    let pending = Box::pin(pending_using(
        &s,
        1,
        |_| {},
        crate::application::workflow::DeliveryPolicy::InAppOnly,
        LeasePolicy::new(Duration::from_secs(3)).unwrap(),
    ))
    .await;
    let fixture = QuestionPool(&s);
    let (mut observer, _) = connect(&fixture).await;
    let mut gate = Gate::arm(
        &fixture,
        "workflow_channel_questions",
        "INSERT",
        "true",
        Stall::Commit,
    )
    .await;
    let runtime = runtime(s.p());
    let claim = pending.claim.clone();
    let publishing = tokio::spawn(async move {
        runtime
            .execute(&WorkflowStepKind("message.ask".into()), &claim)
            .await
    });
    gate.stalled(&mut observer, &[&|| !publishing.is_finished()])
        .await;
    let expiry: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT lock_expires_at FROM background_tasks WHERE company_id=$1 AND id=$2",
    )
    .bind(pending.scope.company.as_uuid())
    .bind(pending.scope.job.0)
    .fetch_one(s.pool())
    .await
    .unwrap();
    let remaining = expiry - chrono::Utc::now();
    tokio::time::sleep(remaining.to_std().unwrap_or_default() + Duration::from_millis(20)).await;
    gate.release().await;
    gate.retake().await;
    gate.release().await;
    assert!(matches!(
        publishing.await.unwrap(),
        WorkflowHandlerOutcome::Completed(Err(_))
    ));
    let c = counts(&s, pending.scope.run).await;
    assert_eq!(c.questions, 0);
    assert_eq!(c.waits, 0);
    assert_eq!(c.destinations, 0);
    assert_eq!(c.dispatches, 0);
}
