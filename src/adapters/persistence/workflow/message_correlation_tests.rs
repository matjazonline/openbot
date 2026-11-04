//! Stored protocol metadata must agree with the exact part that delivery will send.
use super::*;
use crate::adapters::protocols::email::EmailRenderer;
use crate::adapters::workflow_message::MessageRuntime;
use crate::app_error::AppResult;
use crate::application::transport::ports::RenderedCorrelation;
use crate::application::transport::{
    DeliveryComposer, RenderedPart, TransportPayload, TransportRenderer, TransportRenderers,
};
use std::sync::Arc;

pub(super) async fn assert_frozen(setup: &Setup) {
    let (rfc, parent, references, payload): (String, Option<String>, Option<Vec<String>>, Value) = sqlx::query_as(
        "SELECT metadata.rfc_message_id, metadata.in_reply_to, metadata.references_list, part.payload \
         FROM workflow_message_destinations AS target \
         JOIN email_message_metadata AS metadata ON metadata.company_id=target.company_id AND metadata.message_id=target.message_id \
         JOIN message_delivery_parts AS part ON part.company_id=target.company_id AND part.delivery_id=target.delivery_id \
         WHERE target.run_id=$1",
    ).bind(setup.scope().run.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    let payload: TransportPayload = serde_json::from_value(payload).unwrap();
    let wire: Value = payload.decode(TransportKind::Email, 1).unwrap();
    assert_eq!(rfc, wire["message_id"].as_str().unwrap());
    assert_eq!(parent.as_deref(), wire["in_reply_to"].as_str());
    assert_eq!(
        serde_json::to_value(references.unwrap_or_default()).unwrap(),
        wire["references"]
    );
}
#[tokio::test]
async fn workflow_message_stores_anchored_rendered_headers_without_rederiving_them() {
    let conversation = Box::pin(conversation(
        |_, _, _, _, user| send_to_user(user),
        Source::Question,
    ))
    .await;
    let setup = &conversation.setup;
    assert!(matches!(
        runtime_for(setup, Rendering::Anchored)
            .execute(&setup.kind, &setup.claim)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    assert_frozen(setup).await;
    let (parent, references): (Option<String>, Option<Vec<String>>) = sqlx::query_as(
        "SELECT metadata.in_reply_to, metadata.references_list FROM workflow_message_destinations AS target \
         JOIN email_message_metadata AS metadata ON metadata.company_id=target.company_id AND metadata.message_id=target.message_id \
         WHERE target.run_id=$1",
    ).bind(setup.scope().run.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    let parent = parent.expect("anchored mail has its actual parent");
    assert!(parent.starts_with('<') && parent.ends_with("@agents.test>"));
    assert_eq!(references, Some(vec![parent]));
}

#[derive(Clone, Copy, Debug)]
enum Rendering {
    Anchored,
    Empty,
    WrongTransport,
    WrongVersion,
    Malformed,
}
struct TestRenderer {
    email: EmailRenderer,
    fault: Rendering,
}
impl TransportRenderer for TestRenderer {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }
    fn render(
        &self,
        envelope: &crate::transport::DeliveryEnvelope,
    ) -> AppResult<Vec<RenderedPart>> {
        // User sends currently request Standalone. This fixture chooses an actual anchored
        // wire representation to prove persistence follows the frozen part, not its input.
        if matches!(self.fault, Rendering::Anchored) {
            let mut anchored = envelope.clone();
            let crate::transport::DeliveryContext::Email(context) = &mut anchored.context;
            context.threading = crate::transport::EmailThreading::Anchored(
                crate::transport::ConversationAnchor::parse("p07-native-anchor").unwrap(),
            );
            return self.email.render(&anchored);
        }
        let mut parts = self.email.render(envelope)?;
        if matches!(self.fault, Rendering::Empty) {
            return Ok(Vec::new());
        }
        let (transport, version) = match self.fault {
            Rendering::WrongTransport => (TransportKind::Slack, 1),
            Rendering::WrongVersion => (TransportKind::Email, 2),
            Rendering::Malformed => (TransportKind::Email, 1),
            Rendering::Empty | Rendering::Anchored => unreachable!(),
        };
        parts[0].payload =
            TransportPayload::encode(transport, version, &json!({"invalid":true})).unwrap();
        Ok(parts)
    }
    fn render_standalone(
        &self,
        envelope: &crate::transport::StandaloneDeliveryEnvelope,
    ) -> AppResult<Vec<RenderedPart>> {
        self.email.render_standalone(envelope)
    }
    fn rendered_correlation(&self, part: &RenderedPart) -> AppResult<RenderedCorrelation> {
        self.email.rendered_correlation(part)
    }
    fn application_message_key(
        &self,
        company: &crate::entities::value_objects::CompanySlug,
        nonce: Uuid,
    ) -> AppResult<ExternalMessageKey> {
        self.email.application_message_key(company, nonce)
    }
}
fn runtime_for(setup: &Setup, fault: Rendering) -> MessageRuntime {
    let p = &setup.fixture.persistence;
    let renderers = TransportRenderers::new()
        .register(Arc::new(TestRenderer {
            email: EmailRenderer::new(APP_DOMAIN),
            fault,
        }))
        .unwrap();
    let composer = DeliveryComposer::new(Arc::new(renderers), Arc::new(p.clone()));
    MessageRuntime::new(p.clone(), composer, APP_DOMAIN.into(), lease())
}
async fn durable_counts(setup: &Setup) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM messages WHERE company_id=$1), (SELECT count(*) FROM message_deliveries WHERE company_id=$1)")
        .bind(setup.fixture.target.company.as_uuid()).fetch_one(setup.pool()).await.unwrap()
}
#[tokio::test]
async fn workflow_message_invalid_frozen_correlation_creates_no_message_or_delivery() {
    for fault in [
        Rendering::Empty,
        Rendering::WrongTransport,
        Rendering::WrongVersion,
        Rendering::Malformed,
    ] {
        let conversation = Box::pin(conversation(
            |_, _, source, _, _| reply(source),
            Source::Question,
        ))
        .await;
        let setup = &conversation.setup;
        let runtime = runtime_for(setup, fault);
        let before = durable_counts(setup).await;
        let outcome = runtime.execute(&setup.kind, &setup.claim).await;
        assert!(
            matches!(outcome, WorkflowHandlerOutcome::Completed(Err(_))),
            "{fault:?}: {outcome:?}"
        );
        assert_eq!(durable_counts(setup).await, before, "{fault:?}");
    }
}
