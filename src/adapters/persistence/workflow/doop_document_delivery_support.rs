//! Real delivery claims/rendering/receipts with a counted, local-only email provider.
use super::*;
use crate::adapters::protocols::email::{EmailRenderer, OUTBOUND_EMAIL_VERSION, OutboundEmailV1};
use crate::application::services::delivery_worker::DeliveryWorker;
use crate::entities::transport::{DeliveryId, TransportKind};
use crate::transport::{
    DeliveryRecord, ProviderSendOutcome, RenderedPart, TransportRegistry, TransportSender,
};
use std::sync::Mutex;

pub(super) struct SentDocument {
    pub id: DeliveryId,
    pub email: OutboundEmailV1,
}

#[derive(Default)]
pub(super) struct DocumentTransport {
    pub sent: Mutex<Vec<SentDocument>>,
}

#[async_trait::async_trait]
impl TransportSender for DocumentTransport {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }

    async fn send(&self, record: &DeliveryRecord, part: &RenderedPart) -> ProviderSendOutcome {
        let email = part
            .payload
            .decode(TransportKind::Email, OUTBOUND_EMAIL_VERSION)
            .unwrap();
        let mut sent = self.sent.lock().unwrap();
        assert!(
            sent.len() < 20,
            "Unexpected repeated document notifications"
        );
        sent.push(SentDocument {
            id: record.id,
            email,
        });
        ProviderSendOutcome::Delivered { provider_key: None }
    }
}

pub(super) struct DeliveryFixture {
    pub channel: crate::application::workflow::RelatedChannelId,
    pub worker: DeliveryWorker,
    pub transport: Arc<DocumentTransport>,
}

pub(super) async fn delivery(f: &Fixture, connection: &CompanyMcpConnection) -> DeliveryFixture {
    // Keep real database setup out of the worker-construction poll frame.
    let channel = Box::pin(prepare_delivery(f, connection)).await;
    worker(f, channel)
}

pub(super) fn worker(
    f: &Fixture,
    channel: crate::application::workflow::RelatedChannelId,
) -> DeliveryFixture {
    let transport = Arc::new(DocumentTransport::default());
    let registry = TransportRegistry::new()
        .register(
            Arc::new(EmailRenderer::new("agents.test")),
            transport.clone(),
        )
        .unwrap();
    DeliveryFixture {
        channel,
        worker: DeliveryWorker::new(Arc::new(f.persistence.clone()), Arc::new(registry)),
        transport,
    }
}

pub(super) async fn prepare_delivery(
    f: &Fixture,
    connection: &CompanyMcpConnection,
) -> crate::application::workflow::RelatedChannelId {
    f.persistence
        .set_mcp_level(
            crate::application::workflow::connection_level::ConnectionLevelCommand {
                company: f.target.company,
                actor: f.target.actor,
                connection: connection.id,
                expected_revision: connection.revision,
                access_level: crate::entities::access_level::AccessLevel::L2,
            },
        )
        .await
        .unwrap();
    let channel =
        crate::adapters::persistence::workflow::tests::message_tests::support::new_channel(
            f,
            "document-notices",
        )
        .await;
    verified_recipient(f, f.target.actor.user_id()).await;
    crate::application::workflow::RelatedChannelId::new(channel)
}

pub(super) async fn verified_recipient(f: &Fixture, user: Uuid) {
    let pool = f.persistence.pool();
    let company = f.target.company.as_uuid();
    // Trusted fixture setup for the same verified-email/member-level release rule as production.
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2 AND role<>'owner'")
        .bind(company)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO participant_identities(id,company_id,principal_id,transport,namespace,subject,status,claim_metadata,provenance) SELECT $1,$2,principal.id,'email','email',account.email,'verified','{\"version\":1,\"kind\":\"account\"}'::jsonb,'account' FROM principals AS principal JOIN users AS account ON account.id=principal.user_id WHERE principal.company_id=$2 AND principal.user_id=$3 ON CONFLICT (company_id,transport,namespace,subject) DO UPDATE SET status='verified',claim_metadata=EXCLUDED.claim_metadata,provenance=EXCLUDED.provenance WHERE participant_identities.principal_id=EXCLUDED.principal_id")
        .bind(Uuid::new_v4()).bind(company).bind(user).execute(pool).await.unwrap();
}
