//! The A → B → A loop, end to end against a real database.
//!
//! `docs/inter_channel_agent_communication.md` states the invariants this file pins down: A's call
//! to B creates a *new* B thread and a normal B task; B's answer creates **no** second A task but
//! resumes A's original one; and the answer lands in A's thread as context.
//!
//! The LLM is the only thing left out. A real run reaches `create_outreach_and_pause` because a
//! model chose to call the outreach tool; here the test calls it directly with the arguments the
//! tool would have built, so everything downstream — the delivery queue, the real email sender and
//! the trusted internal transport it reaches first, reply correlation, and the resume — is the
//! production code path.

use super::*;
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::task::{CreateOutreachRequest, OutreachTargetRequest};
use crate::adapters::persistence::test_support::test_pool;
use crate::adapters::protocols::email::parser::RawInboundPayload;
use crate::adapters::protocols::email::{EmailRenderer, EmailSender};
use crate::entities::message::{MessageDirection, MessageRole};
use crate::entities::task::TaskStatus;
use crate::entities::transport::{DeliveryId, DeliveryPurpose, TransportKind};
use crate::transport::{
    CanonicalContent, ComposedDelivery, DeliveryComposer, DeliveryContext, DeliveryRequest,
    EmailDeliveryContext, EmailRelayTrace, NewDelivery, ProviderSendOutcome, TransportSender,
    ports::TransportRenderers,
};
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::user::UserPersistence;
use chrono::Utc;

const APP_DOMAIN: &str = "mailagents.test";

fn loop_test_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        hydradb: None,
        hindsight: None,
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: APP_DOMAIN.to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "smtp.invalid".to_string(),
        smtp_port: 2525,
        smtp_username: String::new(),
        smtp_password: String::new(),
        smtp_from_address: format!("noreply@{APP_DOMAIN}"),
        incoming_smtp_enabled: false,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: false,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        enable_llm_spam_guardrail: false,
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    })
}

/// One company, two agent channels, and the human who talks to the first one.
struct Fixture {
    persistence: Arc<PostgresPersistence>,
    pool: sqlx::PgPool,
    threads: Arc<ThreadUseCases>,
    /// The production email sender, with the thread use cases wired in as its internal relay --
    /// which is what makes a hop to a same-company channel an in-process ingest rather than SMTP.
    sender: EmailSender,
    deliveries: DeliveryComposer,
    company: crate::entities::company::Company,
    channel_a: Channel,
    channel_b: Channel,
    owner_email: String,
}

impl Fixture {
    fn address(&self, channel: &Channel) -> String {
        format!("{}@{}.{}", channel.slug, self.company.slug, APP_DOMAIN)
    }

    /// The one thread a channel has, once a hop has opened it.
    async fn thread_on(&self, channel_id: Uuid) -> Option<crate::entities::thread::Thread> {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM threads WHERE channel_id = $1 ORDER BY created_at LIMIT 1",
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await
        .expect("threads are readable")
        .flatten();
        match id {
            Some(id) => crate::use_cases::thread::ThreadPersistence::get_thread_by_id(
                self.persistence.as_ref(),
                id,
            )
            .await
            .expect("the thread is readable"),
            None => None,
        }
    }

    async fn tasks_for(&self, channel_id: Uuid) -> Vec<crate::entities::task::BackgroundTask> {
        let ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM background_tasks WHERE channel_id = $1 ORDER BY created_at",
        )
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await
        .expect("the task rows are readable");

        let mut tasks = Vec::with_capacity(ids.len());
        for id in ids {
            tasks.push(
                TaskPersistence::get_task_by_id(self.persistence.as_ref(), id)
                    .await
                    .expect("the task loads")
                    .expect("the task exists"),
            );
        }
        tasks
    }

    async fn status_of(&self, task_id: Uuid) -> TaskStatus {
        TaskPersistence::get_task_by_id(self.persistence.as_ref(), task_id)
            .await
            .expect("the task loads")
            .expect("the task exists")
            .status
    }
}

async fn fixture(pool: sqlx::PgPool) -> Fixture {
    let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
    let suffix = Uuid::new_v4().simple().to_string();
    let owner_email = format!("loop_owner_{suffix}@example.com");

    persistence
        .create_user(&format!("loop_owner_{suffix}"), &owner_email, "hash")
        .await
        .expect("the owner is created");
    let owner = UserPersistence::get_by_email(persistence.as_ref(), &owner_email)
        .await
        .expect("the owner loads")
        .expect("the owner exists");

    let company = CompanyPersistence::create(
        persistence.as_ref(),
        owner.id,
        CompanyWrite {
            name: "Loop Test".to_string(),
            slug: format!("loop-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .expect("the company is created");

    let mut channels = Vec::new();
    for (name, slug, description) in [
        ("Coordinator", "coordinator", "Fields customer requests."),
        (
            "Supplier Desk",
            "supplier",
            "Answers supplier capacity and delivery-date questions.",
        ),
    ] {
        let agent = AgentPersistence::create(
            persistence.as_ref(),
            company.id,
            AgentWrite {
                name: name.to_string(),
                slug: format!("{slug}-agent"),
                description: Some(description.to_string()),
                ..AgentWrite::default()
            },
        )
        .await
        .expect("the agent is created");

        channels.push(
            ChannelPersistence::create(
                persistence.as_ref(),
                company.id,
                ChannelWrite {
                    name: name.to_string(),
                    slug: slug.to_string(),
                    agent_ids: Some(vec![agent.id]),
                    participant_emails: Some(vec![owner_email.clone()]),
                    enabled: true,
                    ..ChannelWrite::default()
                },
            )
            .await
            .expect("the channel is created"),
        );
    }

    let config = loop_test_config();
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(EmailRenderer::new(&config.app_domain_name)))
            .expect("one renderer registers"),
    );
    let threads = Arc::new(
        ThreadUseCases::new(
            ThreadStores {
                threads: persistence.clone(),
                channels: persistence.clone(),
                companies: persistence.clone(),
                participants: persistence.clone(),
                tasks: persistence.clone(),
            },
            InboundIngestPorts {
                committer: persistence.clone(),
                correlation: persistence.clone(),
                bindings: persistence.clone(),
                standalone_deliveries: persistence.clone(),
            },
            renderers.clone(),
            config,
        )
        .with_agent_persistence(persistence.clone()),
    );
    // SMTP would refuse anyway (`smtp.invalid`), which is the point: every hop this test makes has
    // to be recognised as internal and relayed, or the send fails visibly.
    let sender = EmailSender::new(
        Arc::new(crate::services::outbound_dispatcher::DisabledMailTransport),
        threads.clone(),
    );
    let deliveries = DeliveryComposer::new(renderers, persistence.clone());

    let channel_b = channels.pop().expect("the supplier channel exists");
    let channel_a = channels.pop().expect("the coordinator channel exists");
    Fixture {
        persistence,
        pool,
        threads,
        sender,
        deliveries,
        company,
        channel_a,
        channel_b,
        owner_email,
    }
}

/// One arriving mail, through the same adapter the SMTP listener uses.
fn inbound(from: &str, to: &str, message_id: &str, subject: &str) -> RawInboundPayload {
    RawInboundPayload {
        from: from.to_string(),
        to: to.to_string(),
        subject: Some(subject.to_string()),
        text: Some("Please find out the earliest delivery date.".to_string()),
        headers: Some(format!("Message-ID: {message_id}")),
        spam_score: Some(0.0),
        ..RawInboundPayload::default()
    }
}

/// One hop this test wants to make, as a producer would compose it.
struct Hop<'a> {
    source: &'a Channel,
    recipient: &'a str,
    subject: &'a str,
    body: &'a str,
    in_reply_to: MessageId,
    references: Vec<MessageId>,
    hop_count: u32,
    trace_channels: Vec<Uuid>,
    task_id: Option<Uuid>,
    source_key: String,
    purpose: DeliveryPurpose,
}

impl Fixture {
    /// Compose one hop the way a producer does: mint the message id, freeze the mail, and record
    /// the question under the provider key it will go out under.
    ///
    /// That last part is what makes an answer findable. A reply from B quotes nothing but the
    /// question A asked, so the question has to carry the `Message-ID` the relay will deliver it
    /// under -- which the renderer can name before anything is sent.
    ///
    /// Nothing is written here: the caller commits both with whatever durable state they belong
    /// to, which is the whole point of the split.
    async fn compose_hop(&self, hop: Hop<'_>, thread_id: Uuid) -> (MessageWrite, NewDelivery) {
        let message_id = CanonicalMessageId::random();
        let subject = hop.subject.to_string();
        let body = hop.body.to_string();
        let composed = self.compose(hop, message_id).await;

        let message = MessageWrite {
            id: message_id,
            ..MessageWrite::internal(
                thread_id,
                MessageAuthorWrite::Platform,
                subject,
                body.clone(),
                MessageDirection::Outbound,
                MessageRole::Agent,
                CorrelationId::new(),
            )
        }
        .with_correlation(crate::use_cases::thread::MessageCorrelation::Email(
            crate::entities::email_message::EmailMessageMetadata::new(MessageId::from(
                composed
                    .provider_key
                    .as_ref()
                    .expect("mail can name its own Message-ID before it is sent")
                    .as_str()
                    .to_string(),
            ))
            .raw_bodies(Some(body), None),
        ));
        (message, composed.delivery)
    }

    /// Compose one delivery through the production composer and renderer.
    async fn compose(&self, hop: Hop<'_>, message_id: CanonicalMessageId) -> ComposedDelivery {
        let content = CanonicalContent::parse(hop.subject, hop.body).expect("bounded test content");
        self.deliveries
            .compose(DeliveryRequest {
                company_id: self.company.id,
                channel_id: hop.source.id,
                message_id,
                task_id: hop.task_id,
                correlation_id: CorrelationId::new(),
                purpose: hop.purpose,
                source_key: hop.source_key.clone(),
                content: &content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from: crate::entities::channel::Channel::address_for(
                        &hop.source.slug,
                        &self.company.slug,
                        APP_DOMAIN,
                    ),
                    from_name: Some(hop.source.name.clone()),
                    recipient_to: hop.recipient.into(),
                    recipients_cc: Vec::new(),
                    in_reply_to: Some(hop.in_reply_to.clone()),
                    references: hop.references.clone(),
                    relay: Some(EmailRelayTrace {
                        source_channel_id: hop.source.id,
                        hop_count: hop.hop_count,
                        trace_channels: hop.trace_channels.clone(),
                    }),
                }),
            })
            .await
            .expect("the channel has an email interface to compose against")
    }

    /// Send one queued delivery through the real [`EmailSender`], which reaches the internal relay
    /// before SMTP, and record the provider key it went out under.
    ///
    /// The queue transition is written directly rather than claimed: `claim_deliveries` is global
    /// by design, and this test shares a database with everything else running in parallel. What
    /// the fenced claim protocol does with the same outcome is
    /// `src/adapters/persistence/delivery/tests.rs`'s subject, not this file's.
    async fn deliver(&self, delivery: &NewDelivery) -> MessageId {
        let record = self.record_of(delivery.id).await;
        let part = &delivery.parts[0];
        let outcome = self.sender.send(&record, part).await;
        let provider_key = match outcome {
            ProviderSendOutcome::Delivered { provider_key } => {
                provider_key.expect("an email delivery names the Message-ID it went out under")
            }
            other => panic!(
                "a same-company hop must be relayed internally, got {other:?} for {}",
                delivery.idempotency_key
            ),
        };

        sqlx::query(
            "UPDATE message_delivery_parts
                SET status = 'delivered', provider_message_key = $2,
                    request_started_at = CURRENT_TIMESTAMP, delivered_at = CURRENT_TIMESTAMP
              WHERE delivery_id = $1",
        )
        .bind(delivery.id.as_uuid())
        .bind(provider_key.as_str())
        .execute(&self.pool)
        .await
        .expect("the part records what the provider said");
        sqlx::query(
            "UPDATE message_deliveries
                SET status = 'delivered', delivered_at = CURRENT_TIMESTAMP
              WHERE id = $1",
        )
        .bind(delivery.id.as_uuid())
        .execute(&self.pool)
        .await
        .expect("the delivery settles");

        MessageId::from(provider_key.as_str().to_string())
    }

    /// The durable identity the sender is handed, read back the way a claim would build it.
    async fn record_of(&self, delivery_id: DeliveryId) -> crate::transport::DeliveryRecord {
        crate::transport::DeliveryRecord {
            id: delivery_id,
            attribution: Some(crate::transport::DeliveryAttribution {
                company_id: self.company.id,
                channel_id: self.channel_a.id,
                message_id: CanonicalMessageId::random(),
                source_binding_id: self.binding_of(self.channel_a.id).await,
                destination_binding_id: self.binding_of(self.channel_a.id).await,
            }),
            external_destination: None,
            task_id: None,
            correlation_id: CorrelationId::new(),
            transport: TransportKind::Email,
            purpose: DeliveryPurpose::Reply,
            idempotency_key: crate::transport::DeliveryKey::parse("test:record")
                .expect("a short key"),
            attempt_count: 0,
            max_attempts: crate::transport::MAX_DELIVERY_ATTEMPTS,
        }
    }

    async fn binding_of(&self, channel_id: Uuid) -> crate::entities::transport::ChannelBindingId {
        crate::use_cases::integration::ChannelBindingPersistence::active_bindings_for_channel(
            self.persistence.as_ref(),
            self.company.id,
            channel_id,
        )
        .await
        .expect("interfaces are readable")
        .into_iter()
        .find(|binding| binding.transport == TransportKind::Email)
        .expect("a channel has its canonical email interface")
        .id
    }
}

#[tokio::test]
async fn agent_a_delegates_to_agent_b_and_b_s_answer_resumes_a_s_original_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let fx = fixture(pool).await;
    let address_a = fx.address(&fx.channel_a);
    let address_b = fx.address(&fx.channel_b);

    // M0: the human writes to A.
    let m0 = fx
        .threads
        .ingest_test_email(inbound(
            &fx.owner_email,
            &address_a,
            "<m0@example.com>",
            "Delivery date?",
        ))
        .await
        .expect("the inbound message is ingested");
    assert!(m0.accepted, "M0 rejected: {:?}", m0.reason());
    let thread_a = m0.thread.expect("M0 opens a thread on A");
    let task_a = m0.task_id.expect("M0 enqueues a dispatch task for A");

    // A's agent calls the outreach tool with B as its single target. Everything below this line is
    // production code.
    let worker_id = Uuid::new_v4();
    assert!(
        TaskPersistence::claim_task(
            fx.persistence.as_ref(),
            task_a,
            worker_id,
            Utc::now() + chrono::Duration::minutes(5),
        )
        .await
        .expect("the task is claimable")
    );

    // The question A asks B: one canonical message, and one delivery carrying it. Both are written
    // by the transaction that parks A's task, exactly as the outreach tool composes them.
    let hop_to_b = Hop {
        source: &fx.channel_a,
        recipient: &address_b,
        subject: "Acquire supplier capacity data",
        body: "Return the earliest delivery date.",
        in_reply_to: "<m0@example.com>".into(),
        references: Vec::new(),
        hop_count: 0,
        trace_channels: Vec::new(),
        task_id: Some(task_a),
        source_key: format!("task:{task_a}:outreach:0"),
        purpose: DeliveryPurpose::Outreach,
    };
    let (question_to_b, request_to_b) = fx.compose_hop(hop_to_b, thread_a.id).await;
    let progress = TaskPersistence::create_outreach_and_pause(
        fx.persistence.as_ref(),
        CreateOutreachRequest {
            correlation_id: CorrelationId::new(),
            id: Uuid::new_v4(),
            task_id: task_a,
            company_id: fx.company.id,
            channel_id: fx.channel_a.id,
            worker_id,
            outreach_key: "delegate-to-b".into(),
            required_threshold_percent: 100.0,
            expires_at: Utc::now() + chrono::Duration::hours(96),
            subject: "Acquire supplier capacity data".into(),
            body: "Return the earliest delivery date.".into(),
            targets: vec![OutreachTargetRequest {
                email: address_b.clone().into(),
                request: question_to_b,
                delivery: request_to_b.clone(),
            }],
        },
    )
    .await
    .expect("the outreach is created");
    assert!(progress.suspended, "delegating must park A's task");
    assert_eq!(
        fx.status_of(task_a).await,
        TaskStatus::WaitingForThirdPartyReply
    );

    // M1: A → B through the real sender, which recognises a same-company address and relays it in
    // process. No SMTP is involved; the configured relay is `smtp.invalid`, so a hop that reached
    // it would fail visibly rather than pass quietly.
    let m1_message_id = fx.deliver(&request_to_b).await;

    let thread_b = fx
        .thread_on(fx.channel_b.id)
        .await
        .expect("M1 opens a thread on B");
    assert_ne!(thread_b.id, thread_a.id, "B must get its own thread");
    assert_eq!(thread_b.channel_id, fx.channel_b.id);

    let b_tasks = fx.tasks_for(fx.channel_b.id).await;
    assert_eq!(b_tasks.len(), 1, "M1 enqueues exactly one task for B");
    assert_eq!(b_tasks[0].status, TaskStatus::Pending);
    assert_eq!(
        fx.status_of(task_a).await,
        TaskStatus::WaitingForThirdPartyReply,
        "A stays parked while B works"
    );

    // M4: B answers A, quoting M1 so the reply correlates to A's outstanding outreach. The hop
    // budget is carried, not reset: the relay stamped `hop_count + 1` onto M1, and B's answer
    // continues from there.
    let hop_to_a = Hop {
        source: &fx.channel_b,
        recipient: &address_a,
        subject: "Re: Acquire supplier capacity data",
        body: "Earliest delivery is 14 March.",
        in_reply_to: m1_message_id.clone(),
        references: vec![m1_message_id.clone()],
        hop_count: 1,
        trace_channels: vec![fx.channel_a.id],
        task_id: Some(b_tasks[0].id),
        source_key: format!("task:{}:reply", b_tasks[0].id),
        purpose: DeliveryPurpose::Reply,
    };
    let (answer, answer_to_a) = fx.compose_hop(hop_to_a, thread_b.id).await;
    crate::use_cases::thread::ThreadPersistence::create_message_with_deliveries(
        fx.persistence.as_ref(),
        &answer,
        std::slice::from_ref(&answer_to_a),
    )
    .await
    .expect("B's answer and its delivery land together");
    fx.deliver(&answer_to_a).await;

    // The invariants the design doc names.
    assert_eq!(
        fx.status_of(task_a).await,
        TaskStatus::Pending,
        "B's answer must resume A's original task"
    );
    let a_tasks = fx.tasks_for(fx.channel_a.id).await;
    assert_eq!(
        a_tasks.len(),
        1,
        "B's answer must not create a second task for A, found {:?}",
        a_tasks.iter().map(|t| t.status).collect::<Vec<_>>()
    );
    assert_eq!(a_tasks[0].id, task_a);
    // And it landed in A's own conversation rather than opening a third one.
    let in_a: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)
             FROM thread_messages AS association
             JOIN messages AS message
               ON (message.company_id, message.id) =
                  (association.company_id, association.message_id)
            WHERE association.thread_id = $1 AND message.direction = 'inbound'
              AND message.clean_text_body LIKE '%14 March%'"#,
    )
    .bind(thread_a.id)
    .fetch_one(&fx.pool)
    .await
    .expect("A's thread is readable");
    assert_eq!(in_a, 1, "B's answer belongs in A's original thread");

    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .expect("the fixture company is removed");
}
