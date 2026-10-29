//! Live-stream tests for the two mailbox streams, on a real database. The thread activity comes
//! from a scripted reader: its query is tested against real runs in the persistence tests.
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

use std::sync::Mutex;

use super::*;
use crate::{
    adapters::persistence::{PostgresPersistence, test_support::test_pool},
    entities::thread_activity::ThreadWorkSummary,
    infra::events::{AttentionScope, AttentionWakeSource, ThreadScope},
    use_cases::{
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
pub(in crate::adapters::http::routes) struct Frame {
    pub(in crate::adapters::http::routes) event: String,
    pub(in crate::adapters::http::routes) data: String,
}

/// A response body being read one SSE frame at a time.
pub(in crate::adapters::http::routes) struct Frames {
    body: axum::body::BodyDataStream,
    buffer: String,
    timeout: Duration,
}

impl Frames {
    pub(in crate::adapters::http::routes) fn of(response: axum::response::Response) -> Self {
        Self {
            body: response.into_body().into_data_stream(),
            buffer: String::new(),
            timeout: FRAME_TIMEOUT,
        }
    }

    /// The next frame, or a panic naming what was still buffered when time ran out.
    async fn next(&mut self) -> Frame {
        loop {
            if let Some(frame) = self.take_buffered() {
                return frame;
            }
            let chunk =
                tokio::time::timeout(self.timeout, futures::StreamExt::next(&mut self.body))
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
    pub(in crate::adapters::http::routes) async fn next_named(&mut self, event: &str) -> Frame {
        loop {
            let frame = self.next().await;
            if frame.event == event {
                return frame;
            }
        }
    }

    pub(in crate::adapters::http::routes) async fn next_named_with_timeout(
        &mut self,
        event: &str,
        timeout: Duration,
    ) -> Frame {
        self.timeout = timeout;
        let frame = tokio::time::timeout(timeout, self.next_named(event))
            .await
            .expect("named event before deadline");
        self.timeout = FRAME_TIMEOUT;
        frame
    }

    pub(in crate::adapters::http::routes) async fn expect_closed(&mut self) {
        let next = tokio::time::timeout(FRAME_TIMEOUT, futures::StreamExt::next(&mut self.body))
            .await
            .unwrap();
        assert!(next.is_none(), "revoked stream must close");
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

/// A deployment with no spam scanning, which is all these streams need.
pub(in crate::adapters::http::routes) fn test_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: crate::infra::config::ResendApiConfig::default(),
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
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    })
}

/// Each thread's run activity, as the test sets it.
#[derive(Default)]
struct ScriptedActivity {
    summaries: Mutex<HashMap<Uuid, ThreadWorkSummary>>,
    runs: Mutex<HashMap<Uuid, crate::application::thread_activity::ThreadRuns>>,
}

#[async_trait::async_trait]
impl ThreadActivityReader for ScriptedActivity {
    async fn inspectable_runs(
        &self,
        _company: Uuid,
        _viewer: Uuid,
        ids: &[Uuid],
    ) -> AppResult<HashSet<Uuid>> {
        Ok(ids.iter().copied().collect())
    }

    async fn thread_runs(
        &self,
        _company_id: Uuid,
        thread_id: Uuid,
        _viewer: Uuid,
    ) -> AppResult<crate::application::thread_activity::ThreadRuns> {
        if let Some(runs) = self.runs.lock().unwrap().get(&thread_id) {
            return Ok(runs.clone());
        }
        let items = self
            .summaries
            .lock()
            .unwrap()
            .get(&thread_id)
            .and_then(|row| {
                row.activity.map(|activity| {
                    crate::application::thread_activity::ThreadRunActivity {
                        run_id: row.run_id,
                        title: "Workflow".into(),
                        activity,
                    }
                })
            })
            .into_iter()
            .collect();
        Ok(crate::application::thread_activity::ThreadRuns {
            inspectable: Default::default(),
            items,
            has_more: false,
            message_runs: vec![],
        })
    }

    async fn thread_activity(
        &self,
        _company_id: Uuid,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadWorkSummary>> {
        let activity = self.summaries.lock().unwrap();
        Ok(thread_ids
            .iter()
            .filter_map(|id| activity.get(id).map(|summary| (*id, *summary)))
            .collect())
    }
}

struct Harness {
    persistence: Arc<PostgresPersistence>,
    channels: Arc<ChannelUseCases>,
    threads: Arc<ThreadUseCases>,
    activity: Arc<ScriptedActivity>,
    events: MailboxEvents,
    config: Arc<AppConfig>,
    company_id: Uuid,
    channel_id: Uuid,
    owner: Viewer,
}

impl Harness {
    async fn new() -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("activity-stream-{suffix}@example.com");
        persistence
            .create_user(&format!("activity-stream-{suffix}"), &email, "hash")
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
                name: "Activity Streams".into(),
                slug: format!("activity-stream-{suffix}"),
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
                slug: format!("activity-stream-support-{suffix}"),
                ..ChannelWrite::default()
            },
        )
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
            activity: Arc::new(ScriptedActivity::default()),
            events: MailboxEvents::new(),
            config,
            persistence,
            company_id: company.id,
            channel_id: channel.id,
            owner: Viewer {
                user_id: owner.id,
                email: EmailAddress::from(email.as_str()),
                email_verified_at: Some(chrono::Utc::now()),
            },
        })
    }

    async fn thread(&self, subject: &str) -> Uuid {
        ThreadPersistence::create_thread(self.persistence.as_ref(), self.channel_id, subject, &[])
            .await
            .unwrap()
            .id
    }

    /// Set what a thread's newest run is doing; `None` is a thread with no run, or a finished one.
    fn set_activity(&self, thread_id: Uuid, activity: Option<ThreadActivity>) {
        self.activity.summaries.lock().unwrap().insert(
            thread_id,
            ThreadWorkSummary {
                run_id: Uuid::new_v4(),
                activity,
            },
        );
    }

    /// What the `workflow_runs` activity trigger publishes when a thread's run changes state.
    fn publish_activity_change(&self, thread_id: Uuid) {
        self.events
            .publish(MailboxEvent::ActivityChanged(ThreadScope {
                thread_id,
                channel_id: self.channel_id,
                company_id: self.company_id,
            }));
    }

    fn reader(&self) -> Arc<dyn ThreadActivityReader> {
        self.activity.clone()
    }

    async fn column_stream(&self) -> Frames {
        Frames::of(
            thread_column_stream(
                State(self.channels.clone()),
                State(self.threads.clone()),
                State(self.reader()),
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
                State(self.reader()),
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
}

/// p08 D4.6. A connect seeds every first-page badge from the runs, and a working thread's row
/// arrives with its badge already on it.
#[tokio::test]
async fn the_column_stream_badges_each_thread_by_its_newest_run() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let working = harness.thread("Invoice 4471").await;
    let quiet = harness.thread("Nothing running here").await;
    harness.set_activity(working, Some(ThreadActivity::Working));

    let mut frames = harness.column_stream().await;

    // The subscribe-then-snapshot seeding: one badge event per first-page thread, before any row.
    let mut badges = HashMap::new();
    while badges.len() < 2 {
        let frame = frames.next().await;
        if let Some(id) = frame.event.strip_prefix("activity-") {
            badges.insert(id.to_string(), frame.data);
        }
    }
    assert!(
        badges[&working.to_string()].contains("Agent working"),
        "the working thread's badge is seeded on connect"
    );
    assert_eq!(
        badges[&quiet.to_string()],
        "",
        "a thread with no run gets the empty payload that clears its slot"
    );

    let mut rows = Vec::new();
    while rows.len() < 2 {
        let frame = frames.next().await;
        if frame.event == "thread" {
            rows.push(frame.data);
        }
    }
    let working_row = rows
        .iter()
        .find(|row| row.contains(&working.to_string()))
        .expect("the working thread's row streams in");
    assert!(working_row.contains("Agent working"), "{working_row}");
    assert!(working_row.contains(&format!(r#"sse-swap="activity-{working}""#)));
    let quiet_row = rows
        .iter()
        .find(|row| row.contains(&quiet.to_string()))
        .expect("the quiet thread's row streams in");
    assert!(!quiet_row.contains("Agent working"));
}

/// A run changing state redraws that thread's badge, and a finished run clears it.
#[tokio::test]
async fn a_run_change_redraws_its_threads_badge() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let thread_id = harness.thread("Refund").await;
    harness.set_activity(thread_id, Some(ThreadActivity::Queued));
    let mut frames = harness.column_stream().await;
    let seeded = frames.next_named(&format!("activity-{thread_id}")).await;
    assert!(seeded.data.contains("Queued"), "{}", seeded.data);

    harness.set_activity(thread_id, Some(ThreadActivity::AwaitingReview));
    harness.publish_activity_change(thread_id);
    let review = frames.next_named(&format!("activity-{thread_id}")).await;
    assert!(review.data.contains("Awaiting review"), "{}", review.data);

    harness.set_activity(thread_id, None);
    harness.publish_activity_change(thread_id);
    let cleared = frames.next_named(&format!("activity-{thread_id}")).await;
    assert_eq!(cleared.data, "", "a succeeded run leaves nothing to show");
}

/// A listener reconnect may have lost notifications: the column re-sends every first-page badge.
#[tokio::test]
async fn a_resync_re_sends_the_columns_badges() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let thread_id = harness.thread("Lagged").await;
    let mut frames = harness.column_stream().await;
    assert_eq!(
        frames
            .next_named(&format!("activity-{thread_id}"))
            .await
            .data,
        ""
    );

    // Changed with no notification for it, as across a dropped listener connection.
    harness.set_activity(thread_id, Some(ThreadActivity::NeedsAttention));
    harness.events.publish(MailboxEvent::Resync);
    let resent = frames.next_named(&format!("activity-{thread_id}")).await;
    assert!(resent.data.contains("Needs attention"), "{}", resent.data);
}

/// The open thread streams its activity strip on connect and after its run changes; an attention
/// change is not its news.
#[tokio::test]
async fn the_open_thread_streams_its_activity_strip() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let thread_id = harness.thread("Open").await;
    let mut frames = harness.message_stream(thread_id).await;
    assert_eq!(
        frames.next_named("activity").await.data,
        "",
        "a thread with no run streams an empty strip"
    );

    harness.set_activity(thread_id, Some(ThreadActivity::Failed));
    harness
        .events
        .publish(MailboxEvent::AttentionChanged(AttentionScope {
            company_id: harness.company_id,
            channel_id: harness.channel_id,
            source_kind: AttentionWakeSource::Delivery,
            source_id: Uuid::new_v4(),
        }));
    harness.publish_activity_change(thread_id);
    let failed = frames.next_named("activity").await;
    assert!(failed.data.contains("Run failed"), "{}", failed.data);
}

#[path = "ui_conversation_stream_tests.rs"]
mod conversation_stream_tests;

#[path = "ui_message_tests.rs"]
mod message_source_tests;
