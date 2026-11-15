//! Direct final publication through the real global queue, email adapter and fenced worker.
//! The provider records accepted submissions; it does not claim a remote SMTP/API submission.
use super::*;
use crate::adapters::persistence::delivery::enqueue::insert_delivery_on;
use crate::adapters::persistence::test_support::{DeliveryFixtureRequest, delivery_fixture};
use crate::adapters::protocols::email::mail::DeploymentMailTransports;
use crate::adapters::protocols::email::test_support::RecordingTransport;
use crate::adapters::protocols::email::{EmailRenderer, EmailSender};
use crate::application::services::delivery_worker::DeliveryWorker;
use crate::entities::delivery::{DeliveryFilter, DeliveryQuery};
use crate::entities::transport::{DeliveryId, DeliveryPurpose};
use crate::transport::ports::{TransportRegistry, TransportRenderers};
use crate::transport::{
    CanonicalContent, DeliveryComposer, DeliveryContext, DeliveryRequest, EmailDeliveryContext,
    EmailThreading, NewStandaloneDelivery, StandaloneDeliveryEnqueuer, StandaloneDeliveryRequest,
};
use crate::use_cases::delivery::{DeliveryReadScope, DeliveryReader};

pub(super) async fn assert_delivered(f: &DirectIntents, direct: DeliveryId) {
    let p = f.s.p();
    let mut listener = sqlx::postgres::PgListener::connect_with(f.s.pool())
        .await
        .unwrap();
    listener.listen("attention_changed").await.unwrap();
    let DeliverySiblings { desk, standalone } = Box::pin(queue_siblings(f)).await;
    assert_readers(f, direct, desk).await;
    let provider = Arc::new(RecordingTransport::default());
    let transports = TransportRegistry::new()
        .register(
            Arc::new(EmailRenderer::new(DOMAIN)),
            Arc::new(EmailSender::new(
                DeploymentMailTransports::only(provider.clone()),
                Arc::new(crate::adapters::persistence::test_support::thread_use_cases(p, DOMAIN)),
            )),
        )
        .unwrap();
    let worker = DeliveryWorker::new(Arc::new(p.clone()), Arc::new(transports));
    let (_shutdown, mut receiver) = tokio::sync::broadcast::channel(1);
    assert_eq!(
        Box::pin(worker.drain_once(&mut receiver)).await.unwrap(),
        3,
        "direct, desk and standalone rows must all survive one global claim batch"
    );
    let sent = provider.sent();
    assert_eq!(sent.len(), 3);
    let direct_mail = sent
        .iter()
        .find(|mail| mail.body_text == "The direct reply")
        .unwrap();
    assert!(
        direct_mail
            .from
            .as_str()
            .starts_with("direct-workflow-agent@")
    );
    let ids = [direct.as_uuid(), desk.as_uuid(), standalone.as_uuid()];
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM message_deliveries WHERE id=ANY($1)")
            .bind(ids.as_slice())
            .fetch_all(f.s.pool())
            .await
            .unwrap();
    assert_eq!(statuses, ["delivered", "delivered", "delivered"]);
    assert_readers(f, direct, desk).await;
    let delivered = p
        .get_delivery(read_scope(f, f.s.owner().user_id()), direct)
        .await
        .unwrap()
        .unwrap();
    assert!(!delivered.parts.is_empty());
    assert!(
        delivered.parts.iter().all(|part| part.status
            == crate::entities::transport::DeliveryPartStatus::Delivered
            && part.provider_message_key.is_some()),
        "provider acceptance must commit fenced parts and correlation keys"
    );

    assert_wakes(f, &mut listener, direct, desk, standalone).await;
}

struct DeliverySiblings {
    desk: DeliveryId,
    standalone: DeliveryId,
}

async fn queue_siblings(f: &DirectIntents) -> DeliverySiblings {
    let p = f.s.p();
    let company = f.s.fixture.target.company.as_uuid();
    let notice = standalone_delivery(f);
    let standalone = notice.id;
    p.enqueue_standalone_delivery(notice).await.unwrap();
    let fixture = delivery_fixture(
        p,
        DeliveryFixtureRequest::new(
            company,
            f.s.channel,
            f.s.thread,
            "direct-delivery-desk-sibling",
        ),
    )
    .await;
    let from: String = sqlx::query_scalar("SELECT address.slug::text||'@'||company.slug::text||'.'||$3 FROM addresses AS address JOIN companies AS company ON company.id=address.company_id WHERE address.company_id=$1 AND address.channel_id=$2 AND address.is_primary")
        .bind(company).bind(f.s.channel).bind(DOMAIN).fetch_one(f.s.pool()).await.unwrap();
    let content = CanonicalContent::parse("Re: order", "On its way.").unwrap();
    let delivery = composer(f)
        .compose(DeliveryRequest {
            company_id: company,
            target: AddressTarget::Desk(f.s.channel),
            message_id: fixture.message_id,
            correlation_id: fixture.delivery.correlation_id,
            purpose: DeliveryPurpose::WorkflowReply,
            source_key: "direct-delivery-desk-sibling".into(),
            content: &content,
            context: email_context(from.into()),
        })
        .await
        .unwrap()
        .delivery;
    let desk = delivery.id;
    let mut tx = f.s.pool().begin().await.unwrap();
    insert_delivery_on(&mut tx, &delivery).await.unwrap();
    tx.commit().await.unwrap();
    DeliverySiblings { desk, standalone }
}

fn composer(f: &DirectIntents) -> DeliveryComposer {
    let renderers = TransportRenderers::new()
        .register(Arc::new(EmailRenderer::new(DOMAIN)))
        .unwrap();
    DeliveryComposer::new(Arc::new(renderers), Arc::new(f.s.p().clone()))
}

fn email_context(from: crate::entities::value_objects::EmailAddress) -> DeliveryContext {
    DeliveryContext::Email(EmailDeliveryContext {
        from,
        from_name: None,
        recipient_to: "customer@partner.test".into(),
        recipients_cc: Vec::new(),
        threading: EmailThreading::Standalone,
        relay: None,
    })
}

fn standalone_delivery(f: &DirectIntents) -> NewStandaloneDelivery {
    let composer = composer(f);
    let content = CanonicalContent::parse("Queue sibling", "A standalone notification").unwrap();
    composer
        .compose_standalone(StandaloneDeliveryRequest {
            company_id: f.s.fixture.target.company.as_uuid(),
            correlation_id: crate::entities::correlation::CorrelationId::new(),
            purpose: DeliveryPurpose::Notification,
            source_key: format!("direct-delivery-sibling:{}", Uuid::new_v4()),
            content: &content,
            context: email_context(format!("notifications@{DOMAIN}").into()),
        })
        .unwrap()
}

fn read_scope(f: &DirectIntents, user: Uuid) -> DeliveryReadScope {
    DeliveryReadScope {
        company_id: f.s.fixture.target.company.as_uuid(),
        viewer: crate::application::workflow::WorkflowActor::authenticated(user).unwrap(),
    }
}

async fn assert_readers(f: &DirectIntents, direct: DeliveryId, desk: DeliveryId) {
    let scope = read_scope(f, f.s.owner().user_id());
    let entry = f.s.p().get_delivery(scope, direct).await.unwrap().unwrap();
    assert_eq!(entry.target, f.target());
    let entries =
        f.s.p()
            .list_company_deliveries(scope, &DeliveryFilter::new(DeliveryQuery::default()))
            .await
            .unwrap();
    assert_eq!(
        entries.len(),
        2,
        "standalone notifications have no canonical delivery view"
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.id == direct)
            .unwrap()
            .target,
        f.target()
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.id == desk)
            .unwrap()
            .target,
        AddressTarget::Desk(f.s.channel)
    );
}

async fn assert_wakes(
    f: &DirectIntents,
    listener: &mut sqlx::postgres::PgListener,
    direct: DeliveryId,
    desk: DeliveryId,
    standalone: DeliveryId,
) {
    let mut seen = std::collections::HashMap::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !seen.contains_key(&direct.as_uuid()) || !seen.contains_key(&desk.as_uuid()) {
            let event: Value =
                serde_json::from_str(listener.recv().await.unwrap().payload()).unwrap();
            assert_eq!(event["source_kind"], "delivery");
            seen.insert(
                serde_json::from_value::<Uuid>(event["source_id"].clone()).unwrap(),
                event["target"].clone(),
            );
        }
    })
    .await
    .expect("actual direct and desk delivery transitions emit native wakes");
    while let Ok(event) = tokio::time::timeout(Duration::from_millis(100), listener.recv()).await {
        let event: Value = serde_json::from_str(event.unwrap().payload()).unwrap();
        seen.insert(
            serde_json::from_value::<Uuid>(event["source_id"].clone()).unwrap(),
            event["target"].clone(),
        );
    }
    assert_eq!(seen[&direct.as_uuid()], json!(f.target()));
    assert_eq!(
        seen[&desk.as_uuid()],
        json!(AddressTarget::Desk(f.s.channel))
    );
    assert!(
        !seen.contains_key(&standalone.as_uuid()),
        "targetless standalone rows stay outside attention wakes"
    );
    sqlx::query("UPDATE message_deliveries SET status=status WHERE id=$1")
        .bind(direct.as_uuid())
        .execute(f.s.pool())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(150), listener.recv())
            .await
            .is_err(),
        "an unchanged delivery status must not emit a material-state wake"
    );
}

#[path = "direct_delivery_privacy_tests.rs"]
mod privacy;
