//! The A → B → A loop, end to end against a real database.
//!
//! `docs/inter_channel_agent_communication.md` states the invariants this file pins down: A's call
//! to B creates a *new* B thread and a normal B task; B's answer creates **no** second A task but
//! resumes A's original one; and the answer lands in A's thread as context.
//!
//! The LLM is the only thing left out. A real run reaches `create_outreach_and_pause` because a
//! model chose to call the outreach tool; here the test calls it directly with the arguments the
//! tool would have built, so everything downstream — the outbox, the trusted internal transport,
//! reply correlation, and the resume — is the production code path.

use super::*;
use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::test_support::test_pool;
use crate::entities::message_contract::NormalizedInboundMessage;
use crate::entities::outreach::{CreateOutreachRequest, OutreachTargetRequest};
use crate::entities::task::TaskStatus;
use crate::services::outbound_dispatcher::OutboundEmail;
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::user::UserPersistence;
use chrono::Utc;

const APP_DOMAIN: &str = "mailagents.test";

fn loop_test_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
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
    threads: ThreadUseCases,
    company: crate::entities::company::Company,
    channel_a: Channel,
    channel_b: Channel,
    owner_email: String,
}

impl Fixture {
    fn address(&self, channel: &Channel) -> String {
        format!("{}@{}.{}", channel.slug, self.company.slug, APP_DOMAIN)
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

    let threads = ThreadUseCases::new(
        persistence.clone(),
        persistence.clone(),
        persistence.clone(),
        persistence.clone(),
        loop_test_config(),
    )
    .with_agent_persistence(persistence.clone());

    let channel_b = channels.pop().expect("the supplier channel exists");
    let channel_a = channels.pop().expect("the coordinator channel exists");
    Fixture {
        persistence,
        pool,
        threads,
        company,
        channel_a,
        channel_b,
        owner_email,
    }
}

fn inbound(from: &str, to: &str, message_id: &str, subject: &str) -> NormalizedInboundMessage {
    NormalizedInboundMessage {
        message_id: message_id.into(),
        thread_ref: None,
        references: Vec::new(),
        thread_index: None,
        sender: ParticipantIdentity::email(from),
        recipients_to: vec![ParticipantIdentity::email(to)],
        recipients_cc: Vec::new(),
        subject: subject.to_string(),
        clean_text: "Please find out the earliest delivery date.".to_string(),
        raw_text: None,
        raw_html: None,
        attachments: Vec::new(),
        is_auto_reply: false,
        is_forwarded: false,
        channel_id_header: None,
        hop_count: 0,
        trace_channels: Vec::new(),
        protocol: ChannelType::Email,
        spf_status: Some("pass".into()),
        dkim_status: Some("pass".into()),
        dmarc_status: Some("pass".into()),
        spam_score: Some(0.0),
        is_context_only: false,
    }
}

/// Drive one queued outbox row through exactly the sequence `TaskWorker::deliver_outbox_email`
/// uses for a platform recipient, and return the ingest verdict for the receiving side.
async fn deliver_internally(
    fixture: &Fixture,
    outbox_id: Uuid,
    email: OutboundEmail,
    idempotency_key: &str,
) -> InboundIngestResult {
    let worker_id = Uuid::new_v4();
    let prepared = fixture
        .threads
        .prepare_internal_channel_delivery(email, Some(idempotency_key))
        .await
        .expect("the internal destination resolves")
        .expect("a same-company channel is delivered internally, not over SMTP");

    fixture
        .threads
        .record_outreach_outbound_message(outbox_id, &prepared)
        .await
        .expect("the outbound message is recorded in the sender's thread");

    let ingested = fixture
        .threads
        .ingest_prepared_internal_message(&prepared)
        .await
        .expect("the trusted internal message is ingested");

    // The worker marks the row sent with the Message-ID it went out under. Reply correlation
    // matches on exactly that value, and without it a return hop reads as an unexplained cycle.
    TaskPersistence::claim_outbox_emails(
        fixture.persistence.as_ref(),
        worker_id,
        Utc::now() + chrono::Duration::minutes(5),
        10,
    )
    .await
    .expect("the outbox row is claimable");
    TaskPersistence::mark_outbox_email_sent(
        fixture.persistence.as_ref(),
        outbox_id,
        worker_id,
        prepared.outbound_message_id.as_str(),
    )
    .await
    .expect("the outbox row is marked sent");

    ingested
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
        .ingest_normalized_message(inbound(
            &fx.owner_email,
            &address_a,
            "<m0@example.com>",
            "Delivery date?",
        ))
        .await
        .expect("the inbound message is ingested");
    assert!(m0.accepted, "M0 rejected: {:?}", m0.reason);
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

    let outbox_to_b = Uuid::new_v4();
    let request_to_b = OutboundEmail {
        channel_id: fx.channel_a.id,
        channel_name: fx.channel_a.name.clone(),
        channel_slug: fx.channel_a.slug.clone(),
        company_slug: fx.company.slug.clone(),
        trigger_message_id: "<m0@example.com>".into(),
        thread_references: Vec::new(),
        recipient_to: address_b.clone().into(),
        recipients_cc: Vec::new(),
        subject: "Acquire supplier capacity data".into(),
        body_text: "Return the earliest delivery date.".into(),
        hop_count: 0,
        trace_channels: Vec::new(),
    };
    let progress = TaskPersistence::create_outreach_and_pause(
        fx.persistence.as_ref(),
        CreateOutreachRequest {
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
                outbox_id: outbox_to_b,
                outbox_payload: serde_json::to_value(&request_to_b).expect("payload serializes"),
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

    // M1: A → B over the trusted internal transport. No SMTP is involved.
    let to_b = deliver_internally(
        &fx,
        outbox_to_b,
        request_to_b,
        &format!("outreach:{outbox_to_b}:target:0"),
    )
    .await;
    assert!(to_b.accepted, "M1 rejected: {:?}", to_b.reason);

    let thread_b = to_b.thread.expect("M1 opens a thread on B");
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

    let m1_norm = to_b
        .normalized_message
        .as_ref()
        .expect("M1 carries its normalized form");
    let m1_message_id = to_b
        .inbound_message
        .as_ref()
        .expect("M1 is stored on B's thread")
        .message_id
        .clone();

    // M4: B answers A, quoting M1 so the reply correlates to A's outstanding outreach.
    let outbox_to_a = Uuid::new_v4();
    let answer_to_a = OutboundEmail {
        channel_id: fx.channel_b.id,
        channel_name: fx.channel_b.name.clone(),
        channel_slug: fx.channel_b.slug.clone(),
        company_slug: fx.company.slug.clone(),
        trigger_message_id: m1_message_id.clone(),
        thread_references: vec![m1_message_id.clone()],
        recipient_to: address_a.clone().into(),
        recipients_cc: Vec::new(),
        subject: "Re: Acquire supplier capacity data".into(),
        body_text: "Earliest delivery is 14 March.".into(),
        hop_count: m1_norm.hop_count,
        trace_channels: m1_norm.trace_channels.clone(),
    };
    let to_a = deliver_internally(
        &fx,
        outbox_to_a,
        answer_to_a,
        &format!("task:{}:agent-reply", b_tasks[0].id),
    )
    .await;
    assert!(to_a.accepted, "M4 rejected: {:?}", to_a.reason);

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
    assert_eq!(
        to_a.thread.as_ref().expect("M4 lands somewhere").id,
        thread_a.id,
        "B's answer belongs in A's original thread"
    );

    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .expect("the fixture company is removed");
}
