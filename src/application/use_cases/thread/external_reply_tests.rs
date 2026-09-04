//! The customer's round trip, end to end against a real database and a scripted model.
//!
//! Mail arrives, an agent answers, the answer is rendered and posted under a `Message-ID`, and the
//! customer replies to that id — which has to land back in the same conversation and wake the
//! channel again.
//!
//! `inter_channel_tests.rs` covers the same loop between two of our own channels, where the send
//! never leaves the process and the delegating agent is replaced by a direct call to the outreach
//! transaction. Both of those are the interesting parts here: the recipient is a stranger, so the
//! mail goes through `EmailSender` to a real `MailTransport`; and the reply text comes from
//! `AgentRunner` itself, driven by a scripted model over a socket
//! ([`crate::services::test_support`]). Everything between is the production path.

use super::*;
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::test_support::test_pool;
use crate::adapters::protocols::email::parser::RawInboundPayload;
use crate::adapters::protocols::email::test_support::{RecordingTransport, recording_transport};
use crate::adapters::protocols::email::{EmailRenderer, EmailSender};
use crate::entities::message::MessageRole;
use crate::entities::task::{TaskLeaseRef, TaskStatus};
use crate::entities::transport::{DeliveryId, TransportKind};
use crate::entities::value_objects::MessageId;
use crate::services::test_support::{
    LlmTurn, SCRIPTED_MODEL, SCRIPTED_PROVIDER, ScriptedLlm, scripted_agent_config, scripted_llm,
};
use crate::transport::{ProviderSendOutcome, TransportSender, ports::TransportRenderers};
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence, CompanyWrite};
use crate::use_cases::user::UserPersistence;
use chrono::Utc;

const APP_DOMAIN: &str = "mailagents.test";

/// The customer's first mail, and the id their reply will quote.
const CUSTOMER_FIRST_MESSAGE_ID: &str = "<c0@client.example>";

/// What the scripted agent answers. Distinctive enough to find in a body column.
const AGENT_ANSWER: &str = "Your invoice was reissued on 14 March.";

fn external_test_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_inbound: None,
        resend_outbound: None,
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
        // A second model call would consume a scripted turn the test did not budget for, and the
        // guardrail is not what this file is about.
        enable_llm_spam_guardrail: false,
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    })
}

/// One company, one agent channel wired to a scripted model, and the customer who writes to it.
struct Fixture {
    persistence: Arc<PostgresPersistence>,
    pool: sqlx::PgPool,
    threads: Arc<ThreadUseCases>,
    /// The production sender, posting to a transport that keeps what it was given. The recipient
    /// is a stranger, so this is the SMTP leg rather than the internal relay.
    transport: Arc<RecordingTransport>,
    sender: EmailSender,
    renderer: EmailRenderer,
    company: crate::entities::company::Company,
    channel: Channel,
    customer_email: String,
}

impl Fixture {
    fn channel_address(&self) -> String {
        format!("{}@{}.{}", self.channel.slug, self.company.slug, APP_DOMAIN)
    }

    async fn threads_on_channel(&self) -> Vec<Uuid> {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM threads WHERE channel_id = $1 ORDER BY created_at",
        )
        .bind(self.channel.id)
        .fetch_all(&self.pool)
        .await
        .expect("threads are readable")
    }

    async fn tasks_on_channel(&self) -> Vec<crate::entities::task::BackgroundTask> {
        let ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM background_tasks WHERE channel_id = $1 ORDER BY created_at",
        )
        .bind(self.channel.id)
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

    /// Claim a queued task the way the worker does, and hand back the lease dispatch is fenced on.
    async fn claim(&self, task_id: Uuid) -> TaskLeaseRef {
        let claimed = TaskPersistence::claim_task(
            self.persistence.as_ref(),
            task_id,
            Uuid::new_v4(),
            Utc::now() + chrono::Duration::minutes(5),
        )
        .await
        .expect("the task is claimable");
        assert!(claimed, "the fixture's own task must be claimable");

        let task = TaskPersistence::get_task_by_id(self.persistence.as_ref(), task_id)
            .await
            .expect("the task loads")
            .expect("the task exists");
        TaskLeaseRef::of(&task).expect("a claimed task holds a lease")
    }

    /// The one delivery queued on this channel, with its frozen parts, read back from the queue.
    async fn queued_delivery(&self) -> QueuedDelivery {
        let rows = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, idempotency_key FROM message_deliveries WHERE channel_id = $1",
        )
        .bind(self.channel.id)
        .fetch_all(&self.pool)
        .await
        .expect("the delivery rows are readable");
        assert_eq!(rows.len(), 1, "the agent's reply queues exactly one mail");
        let (delivery_id, idempotency_key) = rows.into_iter().next().expect("one row");

        let part_key = sqlx::query_scalar::<_, String>(
            "SELECT part_key FROM message_delivery_parts WHERE delivery_id = $1 ORDER BY part_index",
        )
        .bind(delivery_id)
        .fetch_all(&self.pool)
        .await
        .expect("the part rows are readable");
        assert_eq!(part_key.len(), 1, "one email renders one part");

        QueuedDelivery {
            id: DeliveryId::from(delivery_id),
            idempotency_key,
            part_key: crate::transport::PartKey::parse(part_key.into_iter().next().expect("a key"))
                .expect("a stored part key is within its bound"),
        }
    }

    /// Post one queued delivery through the real sender and settle it, as the delivery worker
    /// would. Returns the `Message-ID` it went out under — the id the customer's reply will quote.
    ///
    /// The queue transition is written directly rather than claimed: `claim_deliveries` is global
    /// by design and this test shares a database with everything else running in parallel. The
    /// fenced claim protocol is `src/adapters/persistence/delivery/tests.rs`'s subject.
    async fn deliver(&self, delivery: &QueuedDelivery) -> MessageId {
        let record = crate::transport::DeliveryRecord {
            id: delivery.id,
            attribution: None,
            external_destination: None,
            task_id: None,
            correlation_id: CorrelationId::new(),
            transport: TransportKind::Email,
            purpose: crate::entities::transport::DeliveryPurpose::Reply,
            idempotency_key: crate::transport::DeliveryKey::parse(delivery.idempotency_key.clone())
                .expect("a stored delivery key is within its bound"),
            attempt_count: 0,
            max_attempts: crate::transport::MAX_DELIVERY_ATTEMPTS,
        };
        let part = self.stored_part(delivery).await;

        let provider_key = match self.sender.send(&record, &part).await {
            ProviderSendOutcome::Delivered { provider_key } => {
                provider_key.expect("an email delivery names the Message-ID it went out under")
            }
            other => panic!("a mail to a stranger must be posted, got {other:?}"),
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

    /// The frozen part as the queue stored it, rebuilt into what a sender is handed.
    async fn stored_part(&self, delivery: &QueuedDelivery) -> crate::transport::RenderedPart {
        let (payload, digest) = sqlx::query_as::<_, (serde_json::Value, String)>(
            "SELECT payload, content_digest FROM message_delivery_parts
              WHERE delivery_id = $1 ORDER BY part_index LIMIT 1",
        )
        .bind(delivery.id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .expect("the part row is readable");

        crate::transport::RenderedPart {
            index: crate::transport::PartIndex::new(0),
            key: delivery.part_key.clone(),
            payload: serde_json::from_value(payload).expect("the stored payload decodes"),
            digest: crate::transport::ContentDigest::parse(digest)
                .expect("a stored digest is well formed"),
        }
    }
}

/// One queued mail, identified by everything a sender needs to post it.
struct QueuedDelivery {
    id: DeliveryId,
    idempotency_key: String,
    part_key: crate::transport::PartKey,
}

async fn fixture(pool: sqlx::PgPool, llm_base_url: &str) -> Fixture {
    let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
    let suffix = Uuid::new_v4().simple().to_string();
    let owner_email = format!("reply_owner_{suffix}@example.com");
    let customer_email = format!("customer_{suffix}@client.example");

    persistence
        .create_user(&format!("reply_owner_{suffix}"), &owner_email, "hash")
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
            name: "Reply Test".to_string(),
            slug: format!("reply-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .expect("the company is created");

    // The credential the runner resolves. Production reads it from here rather than from the
    // agent, so a fixture that skipped this would fail in `resolve_agent_params` before the
    // scripted model was ever reached.
    CompanyPersistence::replace_model_connections_for_user(
        persistence.as_ref(),
        owner.id,
        company.id,
        vec![
            CompanyModelConnectionWrite::new(
                SCRIPTED_PROVIDER,
                Some("scripted-key".to_string()),
                vec![SCRIPTED_MODEL.to_string()],
                true,
            )
            .expect("the scripted provider and model are permitted"),
        ],
    )
    .await
    .expect("the company model connection is stored");

    let agent = AgentPersistence::create(
        persistence.as_ref(),
        company.id,
        AgentWrite {
            name: "Billing Desk".to_string(),
            slug: "billing-agent".to_string(),
            description: Some("Answers invoice questions.".to_string()),
            provider: Some(SCRIPTED_PROVIDER.to_string()),
            model: Some(SCRIPTED_MODEL.to_string()),
            system_prompt: Some("Answer the customer briefly.".to_string()),
            config_json: Some(scripted_agent_config(llm_base_url)),
            ..AgentWrite::default()
        },
    )
    .await
    .expect("the agent is created");

    let channel = ChannelPersistence::create(
        persistence.as_ref(),
        company.id,
        ChannelWrite {
            name: "Billing".to_string(),
            slug: "billing".to_string(),
            agent_ids: Some(vec![agent.id]),
            // The customer is an explicit participant, so they are authorized to write here and
            // their mail is not weighed against the spam thresholds.
            participant_emails: Some(vec![customer_email.clone()]),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .expect("the channel is created");

    let config = external_test_config();
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
            config.clone(),
        )
        .with_agent_persistence(persistence.clone()),
    );

    let transport = recording_transport();
    let sender = EmailSender::new(transport.clone(), threads.clone());

    Fixture {
        persistence,
        pool,
        threads,
        transport,
        sender,
        renderer: EmailRenderer::new(&config.app_domain_name),
        company,
        channel,
        customer_email,
    }
}

/// One arriving mail, through the same adapter the SMTP listener uses.
fn inbound(fx: &Fixture, message_id: &str, subject: &str, body: &str) -> RawInboundPayload {
    RawInboundPayload {
        from: fx.customer_email.clone(),
        to: fx.channel_address(),
        subject: Some(subject.to_string()),
        text: Some(body.to_string()),
        headers: Some(format!("Message-ID: {message_id}")),
        spam_score: Some(0.0),
        ..RawInboundPayload::default()
    }
}

/// The customer's reply, quoting the id the agent's answer actually went out under.
fn inbound_reply(fx: &Fixture, message_id: &str, answered: &MessageId) -> RawInboundPayload {
    RawInboundPayload {
        headers: Some(format!(
            "Message-ID: {message_id}\nIn-Reply-To: {answered}\nReferences: {CUSTOMER_FIRST_MESSAGE_ID} {answered}",
            answered = answered.as_str(),
        )),
        subject: Some("Re: Invoice question".to_string()),
        text: Some("Thanks — one more thing.".to_string()),
        ..inbound(
            fx,
            message_id,
            "Re: Invoice question",
            "Thanks — one more thing.",
        )
    }
}

#[tokio::test]
async fn an_agent_reply_is_sent_and_the_customer_s_reply_rejoins_its_thread() {
    let Some(pool) = test_pool().await else {
        return;
    };
    // One turn: a plain answer, which ends the tool loop. A second request would find the listener
    // gone, so an unbudgeted model call fails loudly rather than hanging.
    let mut llm = scripted_llm(vec![LlmTurn::text(AGENT_ANSWER)]).await;
    let fx = fixture(pool, &llm.base_url).await;

    // The customer writes in.
    let first = fx
        .threads
        .ingest_test_email(inbound(
            &fx,
            CUSTOMER_FIRST_MESSAGE_ID,
            "Invoice question",
            "My invoice looks wrong.",
        ))
        .await
        .expect("the inbound message is ingested");
    assert!(
        first.accepted,
        "the first mail was rejected: {:?}",
        first.reason()
    );
    let thread = first.thread.clone().expect("the first mail opens a thread");
    let first_task = first
        .task_id
        .expect("the first mail enqueues a dispatch task");

    // The agent runs. Everything from here to the delivery row is production code; only the model
    // is scripted.
    let lease = fx.claim(first_task).await;
    let outcome = fx
        .threads
        .execute_claimed_agent_task_and_dispatch(
            &first,
            ReplyDelivery::Send,
            lease,
            first
                .correlation_id()
                .expect("an accepted ingest has a chain"),
        )
        .await
        .expect("the scripted agent answers");
    assert!(
        matches!(outcome, DispatchOutcome::Replied(_)),
        "a plain answer must commit a reply, got {outcome:?}"
    );
    assert_scripted_model_was_used(&mut llm);

    // One agent message in the thread, carrying what the model said.
    let history = ThreadPersistence::list_agent_history(fx.persistence.as_ref(), thread.id)
        .await
        .expect("the thread history is readable");
    assert_eq!(
        history.len(),
        2,
        "the thread reads customer then agent, got {history:?}"
    );
    assert_eq!(history[1].role, MessageRole::Agent);
    assert!(
        history[1].body.contains(AGENT_ANSWER),
        "the agent turn must carry the scripted answer, got {:?}",
        history[1].body
    );

    // The reply is posted to the stranger through the real sender.
    let queued = fx.queued_delivery().await;
    let answered_under = fx.deliver(&queued).await;

    assert_eq!(
        answered_under.as_str(),
        fx.renderer.message_id_for(&queued.part_key).as_str(),
        "the id a reply will quote is the one the renderer named before the send"
    );
    let posted = fx.transport.only_mail();
    assert_eq!(
        posted.in_reply_to.as_ref().map(MessageId::as_str),
        Some(CUSTOMER_FIRST_MESSAGE_ID),
        "the answer threads onto the mail it answers"
    );
    assert!(
        posted.body_text.contains(AGENT_ANSWER),
        "the posted mail must carry the agent's answer, got {:?}",
        posted.body_text
    );

    // The customer replies to that id.
    let reply = fx
        .threads
        .ingest_test_email(inbound_reply(&fx, "<c1@client.example>", &answered_under))
        .await
        .expect("the reply is ingested");
    assert!(
        reply.accepted,
        "the reply was rejected: {:?}",
        reply.reason()
    );

    // The invariants this file exists for.
    assert_eq!(
        reply.thread.as_ref().map(|reply_thread| reply_thread.id),
        Some(thread.id),
        "a reply quoting the answer's Message-ID belongs to the conversation it answers"
    );
    assert_eq!(
        fx.threads_on_channel().await,
        vec![thread.id],
        "the reply must not open a second conversation"
    );

    // Both halves of the correlation are on record. The conversation root the customer's
    // `References` names was bound when their first mail was committed:
    let bound_to: Option<Uuid> = sqlx::query_scalar(
        "SELECT thread_id FROM external_threads
          WHERE company_id = $1 AND external_thread_key = $2",
    )
    .bind(fx.company.id)
    .bind(CUSTOMER_FIRST_MESSAGE_ID)
    .fetch_optional(&fx.pool)
    .await
    .expect("the provider conversation bindings are readable");
    assert_eq!(
        bound_to,
        Some(thread.id),
        "the conversation root must stay bound to the thread it opened"
    );

    // ...and the answer itself is mapped under the id it was posted with, onto the outbound
    // message in this thread. That mapping is what lets a reply quoting only the answer find its
    // way home, and it is the half the reply path used to leave unwritten.
    let answer_is_mapped: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)
             FROM external_messages AS mapping
             JOIN messages AS message
               ON (message.company_id, message.id) = (mapping.company_id, mapping.message_id)
             JOIN thread_messages AS association
               ON (association.company_id, association.message_id)
                  = (message.company_id, message.id)
            WHERE mapping.company_id = $1 AND mapping.external_message_key = $2
              AND association.thread_id = $3 AND message.direction = 'outbound'"#,
    )
    .bind(fx.company.id)
    .bind(answered_under.as_str())
    .bind(thread.id)
    .fetch_one(&fx.pool)
    .await
    .expect("the provider message mappings are readable");
    assert_eq!(
        answer_is_mapped, 1,
        "the answer must be findable under the Message-ID it was sent with"
    );

    // One new task, and the first one untouched by it.
    let tasks = fx.tasks_on_channel().await;
    assert_eq!(
        tasks.len(),
        2,
        "the reply wakes the channel exactly once, found {:?}",
        tasks.iter().map(|task| task.status).collect::<Vec<_>>()
    );
    assert_eq!(tasks[0].id, first_task);
    let follow_up = tasks[1].clone();
    assert_ne!(follow_up.id, first_task);
    assert_eq!(follow_up.status, TaskStatus::Pending);

    // And the conversation now reads customer, agent, customer.
    let history = ThreadPersistence::list_agent_history(fx.persistence.as_ref(), thread.id)
        .await
        .expect("the thread history is readable");
    assert_eq!(
        history
            .iter()
            .map(|message| message.role)
            .collect::<Vec<_>>(),
        vec![MessageRole::Human, MessageRole::Agent, MessageRole::Human],
        "the reply belongs at the end of the same conversation"
    );

    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .expect("the fixture company is removed");
}

/// A reply that names only the answer it replies to, with no `References` back to the customer's
/// own first mail.
///
/// Most clients send both, which is the path the test above walks. Some send only `In-Reply-To`,
/// and then the conversation root the ingress derives *is* the agent's outbound `Message-ID`. That
/// resolves through `find_ancestor_thread`, the second of `resolve_thread`'s steps -- but only
/// because the answer was registered in `external_messages` under the id it was sent with. The
/// reply path used to drop that id, and this case opened a second thread.
#[tokio::test]
async fn a_reply_naming_only_the_answer_still_finds_the_thread_it_answers() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let mut llm = scripted_llm(vec![LlmTurn::text(AGENT_ANSWER)]).await;
    let fx = fixture(pool, &llm.base_url).await;

    let first = fx
        .threads
        .ingest_test_email(inbound(
            &fx,
            CUSTOMER_FIRST_MESSAGE_ID,
            "Invoice question",
            "My invoice looks wrong.",
        ))
        .await
        .expect("the inbound message is ingested");
    assert!(
        first.accepted,
        "the first mail was rejected: {:?}",
        first.reason()
    );
    let thread = first.thread.clone().expect("the first mail opens a thread");
    let first_task = first
        .task_id
        .expect("the first mail enqueues a dispatch task");

    let lease = fx.claim(first_task).await;
    fx.threads
        .execute_claimed_agent_task_and_dispatch(
            &first,
            ReplyDelivery::Send,
            lease,
            first
                .correlation_id()
                .expect("an accepted ingest has a chain"),
        )
        .await
        .expect("the scripted agent answers");
    assert_scripted_model_was_used(&mut llm);

    let queued = fx.queued_delivery().await;
    let answered_under = fx.deliver(&queued).await;

    // The same reply as the test above, minus the `References` header.
    let reply = fx
        .threads
        .ingest_test_email(RawInboundPayload {
            headers: Some(format!(
                "Message-ID: <c1-no-refs@client.example>\nIn-Reply-To: {}",
                answered_under.as_str(),
            )),
            ..inbound(
                &fx,
                "<c1-no-refs@client.example>",
                "Re: Invoice question",
                "Thanks — one more thing.",
            )
        })
        .await
        .expect("the reply is ingested");
    assert!(
        reply.accepted,
        "the reply was rejected: {:?}",
        reply.reason()
    );

    assert_eq!(
        reply.thread.as_ref().map(|reply_thread| reply_thread.id),
        Some(thread.id),
        "the answer's own Message-ID is enough to find the conversation it belongs to"
    );
    assert_eq!(
        fx.threads_on_channel().await,
        vec![thread.id],
        "a reply without References must not open a conversation of its own"
    );

    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .expect("the fixture company is removed");
}

/// The scripted model is load-bearing: assert it was actually asked, and asked once.
///
/// Without this a change that stopped reaching the provider — a cached answer, a short-circuit —
/// would leave the test green while proving nothing about the agent.
fn assert_scripted_model_was_used(llm: &mut ScriptedLlm) {
    let requests = llm.observed();
    assert_eq!(
        requests.len(),
        1,
        "the dispatch must make exactly one model call, made {}",
        requests.len()
    );
    assert_eq!(
        requests[0]["model"].as_str(),
        Some(SCRIPTED_MODEL),
        "the request must name the model the company connection enabled"
    );
}
