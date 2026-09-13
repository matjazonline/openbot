//! Live-stream tests for the two mailbox streams, on a real database.
//!
//! The handlers are called directly with the extractors axum would build -- this repo has no
//! HTTP-level harness -- and their `Sse` response is read frame by frame off the body. Every
//! assertion is scoped to the company the test created, because the test database is shared and
//! these run in parallel.
//!
//! What is *not* here, deliberately: any assertion that a stream stayed quiet for a period. A
//! "nothing arrived within N milliseconds" test is a timing bet on shared hardware. Where quiet is
//! the subject -- the simulation and schedule-run streams -- it is asserted at the predicate in
//! [`super::live_updates`], which is where the decision actually lives.

use std::time::Duration;

use axum::response::IntoResponse;

use super::*;
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        test_support::{ThreadHandoffFixtureRequest, test_pool, thread_handoff_fixture},
    },
    entities::{
        attention::BusinessPriority,
        thread_handoff::{ThreadHandoffCommand, ThreadHandoffOperation},
        transport::PrincipalId,
    },
    infra::events::{AttentionScope, AttentionWakeSource},
    use_cases::{
        agent::SpamScanning,
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::{InboundIngestPorts, ThreadPersistence, ThreadStores},
        user::UserPersistence,
    },
};

/// How long a frame may take to arrive before the test gives up.
///
/// Generous rather than tight: every wait here is for something that *must* arrive, so a long
/// bound costs nothing when the code is right and fails clearly when it is not.
const FRAME_TIMEOUT: Duration = Duration::from_secs(10);

/// One parsed `text/event-stream` frame.
#[derive(Debug)]
struct Frame {
    event: String,
    data: String,
}

/// A response body being read one SSE frame at a time.
struct Frames {
    body: axum::body::BodyDataStream,
    buffer: String,
}

impl Frames {
    fn of(response: axum::response::Response) -> Self {
        Self {
            body: response.into_body().into_data_stream(),
            buffer: String::new(),
        }
    }

    /// The next frame, or a panic naming what was still buffered when time ran out.
    async fn next(&mut self) -> Frame {
        loop {
            if let Some(frame) = self.take_buffered() {
                return frame;
            }
            let chunk =
                tokio::time::timeout(FRAME_TIMEOUT, futures::StreamExt::next(&mut self.body))
                    .await
                    .unwrap_or_else(|_| {
                        panic!("a frame within the timeout; buffered: {}", self.buffer)
                    })
                    .expect("the stream is still open")
                    .expect("the body chunk reads");
            self.buffer.push_str(&String::from_utf8_lossy(&chunk));
        }
    }

    /// Frames until one whose event name matches, that one included.
    async fn next_named(&mut self, event: &str) -> Frame {
        loop {
            let frame = self.next().await;
            if frame.event == event {
                return frame;
            }
        }
    }

    fn take_buffered(&mut self) -> Option<Frame> {
        let end = self.buffer.find("\n\n")?;
        let raw = self.buffer[..end].to_string();
        self.buffer.drain(..end + 2);
        let mut event = String::new();
        let mut data = String::new();
        for line in raw.lines() {
            if let Some(name) = line.strip_prefix("event: ") {
                event = name.to_string();
            } else if let Some(payload) = line.strip_prefix("data: ") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(payload);
            } else if line == "data:" && data.is_empty() {
                // An empty payload, which is what clears a badge.
                data.push_str("");
            }
        }
        // A keep-alive comment carries neither, and is not a frame anybody is waiting for.
        if event.is_empty() && data.is_empty() {
            return self.take_buffered();
        }
        Some(Frame { event, data })
    }
}

/// A deployment with no memory providers and no spam scanning, which is all these streams need.
fn test_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        default_agent_harness: crate::entities::harness::HarnessKind::AiAgents,
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: crate::infra::config::ResendApiConfig::default(),
        hydradb: None,
        hindsight: None,
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: String::new(),
        smtp_password: String::new(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
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

struct Harness {
    persistence: Arc<PostgresPersistence>,
    channels: Arc<ChannelUseCases>,
    threads: Arc<ThreadUseCases>,
    handoffs: Arc<ThreadHandoffUseCases>,
    agents: Arc<AgentUseCases>,
    events: MailboxEvents,
    config: Arc<AppConfig>,
    company_id: Uuid,
    channel_id: Uuid,
    owner: Viewer,
    owner_principal_id: PrincipalId,
}

impl Harness {
    async fn new() -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("handoff-stream-{suffix}@example.com");
        persistence
            .create_user(&format!("handoff-stream-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(persistence.as_ref(), &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            persistence.as_ref(),
            owner.id,
            CompanyWrite {
                name: "Handoff Streams".into(),
                slug: format!("handoff-stream-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            persistence.as_ref(),
            company.id,
            ChannelWrite {
                name: "Support".into(),
                slug: format!("handoff-stream-support-{suffix}"),
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let owner_principal_id: Uuid =
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
                .bind(company.id)
                .bind(owner.id)
                .fetch_one(persistence.pool())
                .await
                .unwrap();

        let config = test_config();
        let renderers = Arc::new(
            crate::transport::ports::TransportRenderers::new()
                .register(Arc::new(
                    crate::adapters::protocols::email::EmailRenderer::new(&config.app_domain_name),
                ))
                .expect("one renderer registers"),
        );
        Some(Self {
            channels: Arc::new(ChannelUseCases::new(
                persistence.clone(),
                persistence.clone(),
                persistence.clone(),
                config.clone(),
            )),
            threads: Arc::new(ThreadUseCases::new(
                ThreadStores {
                    threads: persistence.clone(),
                    channels: persistence.clone(),
                    companies: persistence.clone(),
                    participants: persistence.clone(),
                    tasks: persistence.clone(),
                    handoff_policy: persistence.clone(),
                },
                InboundIngestPorts {
                    committer: persistence.clone(),
                    correlation: persistence.clone(),
                    bindings: persistence.clone(),
                    standalone_deliveries: persistence.clone(),
                },
                renderers,
                config.clone(),
            )),
            handoffs: Arc::new(ThreadHandoffUseCases::new(persistence.clone())),
            agents: Arc::new(AgentUseCases::new(
                persistence.clone(),
                persistence.clone(),
                persistence.clone(),
                SpamScanning::Unavailable,
            )),
            events: MailboxEvents::new(),
            config,
            persistence,
            company_id: company.id,
            channel_id: channel.id,
            owner: Viewer {
                user_id: owner.id,
                email: EmailAddress::from(email.as_str()),
            },
            owner_principal_id: PrincipalId::from(owner_principal_id),
        })
    }

    async fn thread(&self, subject: &str) -> Uuid {
        ThreadPersistence::create_thread(self.persistence.as_ref(), self.channel_id, subject, &[])
            .await
            .unwrap()
            .id
    }

    /// One held reply on `thread_id`, at version 1, nobody's yet.
    async fn hold(&self, thread_id: Uuid) -> (Uuid, Uuid) {
        let held = thread_handoff_fixture(
            self.persistence.as_ref(),
            ThreadHandoffFixtureRequest::new(self.company_id, self.channel_id, thread_id),
        )
        .await;
        (held.handoff_id, held.generation)
    }

    /// What the Phase 3 trigger publishes when a handoff row changes.
    fn publish_handoff_change(&self, handoff_id: Uuid) {
        self.events
            .publish(MailboxEvent::AttentionChanged(AttentionScope {
                company_id: self.company_id,
                channel_id: self.channel_id,
                source_kind: AttentionWakeSource::ThreadHandoff,
                source_id: handoff_id,
            }));
    }

    async fn column_stream(&self) -> Frames {
        Frames::of(
            thread_column_stream(
                State(self.channels.clone()),
                State(self.threads.clone()),
                State(self.handoffs.clone()),
                State(self.events.clone()),
                State(self.config.clone()),
                self.owner.clone(),
                HeaderMap::new(),
                Query(ChannelStreamQuery {
                    company_id: self.company_id,
                    channel_id: self.channel_id,
                    after: None,
                }),
            )
            .await
            .expect("the column stream opens")
            .into_response(),
        )
    }

    async fn message_stream(&self, thread_id: Uuid) -> Frames {
        Frames::of(
            thread_message_stream(
                State(self.channels.clone()),
                State(self.threads.clone()),
                State(self.handoffs.clone()),
                State(self.agents.clone()),
                State(self.events.clone()),
                self.owner.clone(),
                HeaderMap::new(),
                Query(ThreadStreamQuery {
                    company_id: self.company_id,
                    channel_id: self.channel_id,
                    thread_id,
                    after: None,
                }),
            )
            .await
            .expect("the message stream opens")
            .into_response(),
        )
    }

    /// The owner claims a held reply, exactly as the banner's Claim button does.
    async fn claim(&self, handoff_id: Uuid, generation: Uuid, expected_version: u64) {
        self.handoffs
            .change_thread_handoff(ThreadHandoffCommand {
                company_id: self.company_id,
                handoff_id,
                command_id: Uuid::new_v4(),
                expected_version,
                expected_generation: generation,
                operation: ThreadHandoffOperation::Claim,
                priority: BusinessPriority::Normal,
                due_at: None,
                actor_principal_id: self.owner_principal_id,
                visible_channel_ids: vec![self.channel_id],
            })
            .await
            .expect("the owner may claim an unclaimed reply");
    }

    /// Close a held reply the way a send does: resolved, and out of every open-handoff read.
    async fn resolve(&self, handoff_id: Uuid) {
        sqlx::query(
            r#"UPDATE thread_handoffs
               SET state = 'resolved', closed_at = CURRENT_TIMESTAMP, version = version + 1
               WHERE company_id = $1 AND id = $2"#,
        )
        .bind(self.company_id)
        .bind(handoff_id)
        .execute(self.persistence.pool())
        .await
        .unwrap();
    }
}

/// Cases 13 and 15. A connect seeds every first-page badge, and a held thread's row arrives with
/// its badge already on it.
#[tokio::test]
async fn the_column_stream_badges_the_held_thread_and_clears_the_others() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let held_thread = harness.thread("Invoice 4471").await;
    let quiet_thread = harness.thread("Nothing held here").await;
    let (handoff_id, _) = harness.hold(held_thread).await;

    let mut frames = harness.column_stream().await;

    // The subscribe-then-snapshot seeding: one badge event per first-page thread, before any row.
    let mut badges = std::collections::HashMap::new();
    while badges.len() < 2 {
        let frame = frames.next().await;
        if let Some(id) = frame.event.strip_prefix("handoff-") {
            badges.insert(id.to_string(), frame.data);
        }
    }
    assert!(
        badges[&held_thread.to_string()].contains("Needs input"),
        "the held thread's badge is seeded on connect"
    );
    assert_eq!(
        badges[&quiet_thread.to_string()],
        "",
        "a thread with nothing held gets the empty payload that clears its slot"
    );

    // Case 15: the row itself streams in already carrying the badge, so it cannot blink from held
    // to plain between arriving and the next handoff change.
    let mut rows = Vec::new();
    while rows.len() < 2 {
        let frame = frames.next().await;
        if frame.event == "thread" {
            rows.push(frame.data);
        }
    }
    let held_row = rows
        .iter()
        .find(|row| row.contains(&held_thread.to_string()))
        .expect("the held thread's row streams in");
    assert!(held_row.contains("Needs input"), "{held_row}");
    assert!(held_row.contains(&format!(r#"sse-swap="handoff-{held_thread}""#)));

    let quiet_row = rows
        .iter()
        .find(|row| row.contains(&quiet_thread.to_string()))
        .expect("the quiet thread's row streams in");
    assert!(!quiet_row.contains("Needs input"));

    // And a later change re-sends every first-page badge, held one included.
    harness.publish_handoff_change(handoff_id);
    let frame = frames.next_named(&format!("handoff-{held_thread}")).await;
    assert!(frame.data.contains("Needs input"));
}

/// Case 14. The general file's manual step 7 as an automated test: sending the reply clears the
/// badge without a page reload.
#[tokio::test]
async fn a_resolved_handoff_clears_its_badge_over_the_stream() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let thread_id = harness.thread("Invoice 4471").await;
    let (handoff_id, _) = harness.hold(thread_id).await;

    let mut frames = harness.column_stream().await;
    let seeded = frames.next_named(&format!("handoff-{thread_id}")).await;
    assert!(seeded.data.contains("Needs input"));

    harness.resolve(handoff_id).await;
    harness.publish_handoff_change(handoff_id);

    let cleared = frames.next_named(&format!("handoff-{thread_id}")).await;
    assert_eq!(
        cleared.data, "",
        "a resolved handoff is absent from the read, and an empty payload is what clears the badge"
    );
}

/// Case 16. A lag reconciles both sets, so nothing is left showing a badge it no longer has.
#[tokio::test]
async fn a_lagged_column_re_sends_both_the_activity_and_the_handoff_badges() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let thread_id = harness.thread("Invoice 4471").await;
    let (handoff_id, _) = harness.hold(thread_id).await;

    let mut frames = harness.column_stream().await;
    assert!(
        frames
            .next_named(&format!("handoff-{thread_id}"))
            .await
            .data
            .contains("Needs input")
    );
    // Drain the seeded activity badge and the row, so what follows is the lag's own work.
    frames.next_named("thread").await;

    // A lag is what the broadcast reports when a subscriber falls further behind than the channel
    // holds, which one burst past its capacity produces.
    for _ in 0..(crate::infra::events::broadcast_capacity_for_tests() + 8) {
        harness.publish_handoff_change(handoff_id);
    }

    let handoff = frames.next_named(&format!("handoff-{thread_id}")).await;
    assert!(handoff.data.contains("Needs input"));
    let activity = frames
        .next_named(&pages::thread_activity_event(thread_id))
        .await;
    assert_eq!(
        activity.data, "",
        "the activity set is re-seeded by the same catch-up, and this thread has no task"
    );
}

/// Case 17. The open thread gets its banner on connect, and again after a claim -- with the
/// responsibility changed.
#[tokio::test]
async fn the_open_thread_streams_its_banner_on_connect_and_after_a_claim() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let thread_id = harness.thread("Invoice 4471").await;
    let (handoff_id, generation) = harness.hold(thread_id).await;

    let mut frames = harness.message_stream(thread_id).await;
    let connected = frames.next_named("handoff").await;
    assert!(
        connected.data.contains("Channel team"),
        "an unclaimed reply is the channel team's: {}",
        connected.data
    );
    assert!(connected.data.contains("Needs instruction"));
    assert!(connected.data.contains(">Claim<"));

    harness.claim(handoff_id, generation, 1).await;
    harness.publish_handoff_change(handoff_id);

    let claimed = frames.next_named("handoff").await;
    assert!(
        !claimed.data.contains("Channel team"),
        "somebody has it now: {}",
        claimed.data
    );
    assert!(claimed.data.contains(">Generate draft<"));
    assert!(
        claimed.data.contains(r#"data-handoff-version="2""#),
        "the banner's fence moves with the row it describes"
    );
}

/// Case 18. The accepted cost of the channel-scoped term, asserted rather than pretended away: a
/// handoff change on another thread re-renders this banner, identically.
#[tokio::test]
async fn a_handoff_change_on_another_thread_re_renders_this_banner_unchanged() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let open_thread = harness.thread("Invoice 4471").await;
    let other_thread = harness.thread("Invoice 4472").await;
    let (_, _) = harness.hold(open_thread).await;
    let (other_handoff, _) = harness.hold(other_thread).await;

    let mut frames = harness.message_stream(open_thread).await;
    let first = frames.next_named("handoff").await;

    harness.publish_handoff_change(other_handoff);
    let second = frames.next_named("handoff").await;

    assert_eq!(
        first.data, second.data,
        "the re-read costs one query and produces an identical swap"
    );
}

/// A thread with no held reply still gets the banner event on connect, carrying nothing -- which
/// is what clears a banner left over from a previous connection.
#[tokio::test]
async fn a_thread_with_no_held_reply_streams_an_empty_banner() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let thread_id = harness.thread("Nothing held here").await;

    let mut frames = harness.message_stream(thread_id).await;
    assert_eq!(frames.next_named("handoff").await.data, "");
}
