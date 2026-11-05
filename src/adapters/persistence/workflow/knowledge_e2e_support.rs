use super::*;
use crate::application::knowledge::{KnowledgeScope, KnowledgeUploadFile};
use crate::application::ports::channel::ChannelUpdate;
use crate::application::services::delivery_worker::DeliveryWorker;
use crate::entities::{access_level::AccessLevel, auth::AuthVerdict, user::Viewer};
use crate::transport::{
    DeliveryRecord, ProviderSendOutcome, RenderedPart, TransportRegistry, TransportSender,
};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) const PRIVATE: &str = "PRIVATE FLOOR 60";

pub(super) enum Audience {
    Public,
    Team,
}

#[derive(Default)]
pub(super) struct CountedTransport {
    pub(super) calls: AtomicUsize,
    pub(super) payloads: std::sync::Mutex<Vec<Value>>,
}

#[async_trait]
impl TransportSender for CountedTransport {
    fn transport(&self) -> crate::entities::transport::TransportKind {
        crate::entities::transport::TransportKind::Email
    }

    async fn send(&self, _: &DeliveryRecord, part: &RenderedPart) -> ProviderSendOutcome {
        self.payloads
            .lock()
            .unwrap()
            .push(serde_json::to_value(&part.payload).unwrap());
        self.calls.fetch_add(1, Ordering::SeqCst);
        ProviderSendOutcome::Delivered { provider_key: None }
    }
}

pub(super) struct SalesJourney {
    pub(super) d: Deployment,
    pub(super) transport: Arc<CountedTransport>,
    pub(super) delivery: DeliveryWorker,
}

impl crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::GatePool for SalesJourney {
    fn gate_pool(&self) -> &sqlx::PgPool {
        self.d.p().pool()
    }
}

impl SalesJourney {
    pub(super) async fn new(turns: Vec<LlmTurn>, audience: Audience) -> Self {
        let d = Box::pin(Deployment::new(turns)).await;
        configure(&d, audience).await;
        d.knowledge
            .upload(
                scope(&d, d.fixture.target.actor.user_id()),
                vec![
                    file("0_sales.md", "AX-4401 public list price 100"),
                    file("5_sales.md", "AX-4410 PRIVATE FLOOR 60"),
                ],
            )
            .await
            .unwrap();
        let transport = Arc::new(CountedTransport::default());
        let registry = TransportRegistry::new()
            .register(Arc::new(EmailRenderer::new(APP_DOMAIN)), transport.clone())
            .unwrap();
        let delivery = DeliveryWorker::new(Arc::new(d.p().clone()), Arc::new(registry));
        Self {
            d,
            transport,
            delivery,
        }
    }

    pub(super) async fn member(&self, level: AccessLevel) -> Viewer {
        let (user, email) = verified_member(&self.d.fixture).await;
        sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
            .bind(user)
            .execute(self.d.p().pool())
            .await
            .unwrap();
        sqlx::query(
            "UPDATE company_members SET access_level=$3 WHERE company_id=$1 AND user_id=$2",
        )
        .bind(self.d.fixture.target.company.as_uuid())
        .bind(user)
        .bind(level.as_i16())
        .execute(self.d.p().pool())
        .await
        .unwrap();
        Viewer {
            user_id: user,
            email: email.into(),
            email_verified_at: Some(chrono::Utc::now()),
        }
    }

    pub(super) async fn drive(&self, finished_runs: i64) {
        let (_shutdown, receiver) = tokio::sync::broadcast::channel(1);
        self.d.run_until("knowledge workflow and actual delivery worker", async || {
            self.delivery.drain_once(&receiver).await.unwrap();
            self.d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND finished_at IS NOT NULL").await >= finished_runs
        }).await;
    }

    pub(super) async fn facts(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('ceiling',run.knowledge_ceiling,'content',run.content_level,
            'state',run.state,'proofs',(SELECT count(*) FROM workflow_knowledge_consumptions AS proof WHERE proof.run_id=run.id),
            'reply_levels',(SELECT coalesce(jsonb_agg(message.access_level),'[]'::jsonb) FROM workflow_message_destinations AS destination
                JOIN messages AS message ON (message.company_id,message.id)=(destination.company_id,destination.message_id) WHERE destination.run_id=run.id),
            'delivered',(SELECT count(*) FROM message_deliveries WHERE company_id=run.company_id AND status='delivered'))
            FROM workflow_runs AS run WHERE company_id=$1 ORDER BY run.created_at DESC LIMIT 1")
            .bind(self.d.fixture.target.company.as_uuid()).fetch_one(self.d.p().pool()).await.unwrap()
    }
}

async fn configure(d: &Deployment, audience: Audience) {
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(d.p().pool())
        .await
        .unwrap();
    let agent =
        sqlx::query_scalar("SELECT id FROM agents WHERE company_id=$1 AND slug='assistant'")
            .bind(d.fixture.target.company.as_uuid())
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    crate::adapters::persistence::agent_capability::assign_test_tool_names(
        d.p(),
        agent,
        &[crate::application::workflow::knowledge::SEARCH_KNOWLEDGE],
    )
    .await
    .unwrap();
    ChannelPersistence::update(
        d.p(),
        ChannelUpdate {
            company_id: d.fixture.target.company.as_uuid(),
            channel_id: d.channel,
            write: ChannelWrite {
                name: "Support".into(),
                slug: "support".into(),
                enabled: true,
                add_3rd_party: true,
                participant_emails: match audience {
                    Audience::Public => Some(vec!["@public".into()]),
                    Audience::Team => None,
                },
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
}

pub(super) fn scope(d: &Deployment, user: Uuid) -> KnowledgeScope {
    KnowledgeScope {
        company: d.fixture.target.company,
        actor_id: user,
    }
}

pub(super) fn file(name: &str, text: &str) -> KnowledgeUploadFile {
    KnowledgeUploadFile::from_upload(name, text.as_bytes().to_vec()).unwrap()
}

pub(super) fn turns(answer: &str) -> Vec<LlmTurn> {
    vec![
        LlmTurn::tool_call(
            "search_knowledge",
            json!({"query":"What is the discount floor for AX-4410?"}),
        ),
        LlmTurn::text(serde_json::to_string(answer).unwrap()),
    ]
}

pub(super) fn email(d: &Deployment, sender: &str, key: &str) -> RawInboundPayload {
    RawInboundPayload {
        from: sender.into(),
        to: d.address("support"),
        subject: Some("Discount question".into()),
        text: Some("What is the discount floor for AX-4410?".into()),
        headers: Some(format!("Message-ID: <{key}@partner.test>\n")),
        spf: AuthVerdict::Pass,
        dkim: AuthVerdict::Pass,
        dmarc: AuthVerdict::Pass,
        ..Default::default()
    }
}

pub(super) fn observed_private(journey: &mut SalesJourney) -> bool {
    journey
        .d
        .llm
        .observed()
        .iter()
        .any(|request| request.to_string().contains(PRIVATE))
}
