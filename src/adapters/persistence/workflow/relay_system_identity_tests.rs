//! An actual pure-workflow publication carries system authorship, without an agent's desk grants.
use super::*;
use crate::adapters::protocols::email::test_support::RecordingTransport;
use crate::adapters::protocols::email::{EmailSender, mail::DeploymentMailTransports};
use crate::application::services::delivery_worker::DeliveryWorker;
use crate::transport::TransportRegistry;

#[tokio::test]
async fn relay_identity_pure_workflow_system_reply_cannot_enter_a_team_only_desk() {
    let d = Box::pin(Deployment::new(vec![])).await;
    open_desk(&d).await;
    let receiver = restricted_desk(&d).await;
    Box::pin(bind(&d, example(0))).await;
    let mut inbound = customer_email(&d, "system-relay", None);
    inbound.from = d.address("restricted");
    let accepted = d.email(inbound).await;
    assert!(accepted.accepted);
    Box::pin(d.run_until("pure reply queued", async || {
        responses_published(&d, 1).await
            && d.count("SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND purpose='workflow_reply' AND status='pending'").await == 1
    })).await;
    let role: String = sqlx::query_scalar(
        "SELECT message.role FROM message_deliveries AS delivery JOIN messages AS message \
         ON message.company_id=delivery.company_id AND message.id=delivery.message_id \
         WHERE delivery.company_id=$1 AND delivery.purpose='workflow_reply'",
    )
    .bind(d.fixture.target.company.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(
        role, "system",
        "this must exercise a pure workflow, without an executing agent"
    );
    let smtp = Arc::new(RecordingTransport::default());
    let registry = TransportRegistry::new()
        .register(
            Arc::new(EmailRenderer::new(APP_DOMAIN)),
            Arc::new(EmailSender::new(
                DeploymentMailTransports::only(smtp.clone()),
                Arc::new(d.threads.clone()),
            )),
        )
        .unwrap();
    let worker = DeliveryWorker::new(Arc::new(d.p().clone()), Arc::new(registry));
    let (_shutdown, mut receiver_shutdown) = tokio::sync::broadcast::channel(1);
    worker.drain_once(&mut receiver_shutdown).await.unwrap();
    let refusal:String=sqlx::query_scalar(
        "SELECT last_error_detail FROM message_deliveries WHERE company_id=$1 AND purpose='workflow_reply' AND status='dead_letter'",
    ).bind(d.fixture.target.company.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    assert!(
        refusal.contains("Sender unauthorized for channel"),
        "{refusal}"
    );
    assert!(
        smtp.sent().is_empty(),
        "a refused internal hop cannot escape to SMTP"
    );
    let effects: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM thread_messages WHERE company_id=$1 AND channel_id=$2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(receiver)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(effects, 0);
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}
