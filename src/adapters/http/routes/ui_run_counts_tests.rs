use super::*;

#[tokio::test(start_paused = true)]
async fn throttle_collapses_wakes_and_lag_without_delaying_eligible_work() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut changes = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let mut scheduler = RefreshScheduler::new();
    let start = Instant::now();
    assert!(scheduler.next(&mut changes).await);
    assert_eq!(Instant::now(), start);
    scheduler.finished();
    for _ in 0..20 {
        sender.send(Wake::Lagged).unwrap();
    }
    assert!(scheduler.next(&mut changes).await);
    assert_eq!(Instant::now() - start, Duration::from_secs(5));
    scheduler.finished();
    tokio::time::advance(Duration::from_secs(10)).await;
    sender.send(Wake::Lagged).unwrap();
    assert!(scheduler.next(&mut changes).await);
    assert_eq!(Instant::now() - start, Duration::from_secs(15));
}

#[tokio::test(start_paused = true)]
async fn slow_reads_keep_queued_wakes_and_skip_missed_ticks() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut changes = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let mut scheduler = RefreshScheduler::new();
    assert!(scheduler.next(&mut changes).await);
    // A read crossing several ticks, with a change committed after its snapshot.
    tokio::time::advance(Duration::from_secs(181)).await;
    sender.send(Wake::Lagged).unwrap();
    scheduler.finished();
    let finished = Instant::now();
    assert!(scheduler.next(&mut changes).await);
    assert_eq!(Instant::now() - finished, Duration::from_secs(5));
    scheduler.finished();
    assert!(scheduler.next(&mut changes).await);
    assert_eq!(Instant::now() - finished, Duration::from_secs(59));
}

#[tokio::test(start_paused = true)]
async fn sustained_spaced_wakes_cannot_debounce_the_refresh() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut changes = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver);
    let mut scheduler = RefreshScheduler::new();
    assert!(scheduler.next(&mut changes).await);
    scheduler.finished();
    let started = Instant::now();
    let producer = tokio::spawn(async move {
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(150)).await;
            sender.send(Wake::Lagged).unwrap();
        }
    });
    for pass in 1..=3 {
        assert!(scheduler.next(&mut changes).await);
        assert_eq!(Instant::now() - started, Duration::from_secs(pass * 5));
        scheduler.finished();
    }
    producer.abort();
    let _ = producer.await;
}

use super::super::ui::stream_tests::{Frames, test_config};
use crate::{
    adapters::{
        monitoring::in_memory_monitor::InMemoryMonitor,
        persistence::{PostgresPersistence, test_support::test_pool},
    },
    entities::run_counts::{RunCountRow, RunCountSnapshot},
    infra::events::{MailboxEvent, RunScope},
    use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};
use axum::response::IntoResponse;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Semaphore;

/// A company with two channels and its owner. The open runs are the reader's: these tests are
/// about the stream's protocol, and the SQL has its own test beside the run fixtures.
struct CountsFixture {
    persistence: Arc<PostgresPersistence>,
    company_id: Uuid,
    channels: [Uuid; 2],
    owner: Viewer,
}

impl CountsFixture {
    async fn new() -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("counts-{suffix}@example.com");
        persistence
            .create_user(&format!("counts-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let user = UserPersistence::get_by_email(persistence.as_ref(), &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            persistence.as_ref(),
            user.id,
            CompanyWrite {
                name: "Counts".into(),
                slug: format!("counts-{suffix}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut channels = Vec::new();
        for index in 0..2 {
            channels.push(
                ChannelPersistence::create(
                    persistence.as_ref(),
                    company.id,
                    ChannelWrite {
                        name: format!("Channel {index}"),
                        slug: format!("channel-{index}"),
                        enabled: false,
                        ..Default::default()
                    },
                )
                .await
                .unwrap()
                .id,
            );
        }
        Some(Self {
            persistence,
            company_id: company.id,
            channels: channels.try_into().unwrap(),
            owner: Viewer {
                user_id: user.id,
                email: email.into(),
                email_verified_at: Some(chrono::Utc::now()),
            },
        })
    }
}

/// Open runs by channel, read the way the SQL scopes them: only the channels asked for.
#[derive(Default)]
struct Runs(Mutex<BTreeMap<Uuid, Vec<&'static str>>>);

impl Runs {
    fn open(&self, channel: Uuid, state: &'static str) {
        self.0
            .lock()
            .unwrap()
            .entry(channel)
            .or_default()
            .push(state);
    }
    fn snapshot(&self, visible: &[Uuid]) -> RunCountSnapshot {
        let runs = self.0.lock().unwrap();
        RunCountSnapshot::from_rows(
            runs.iter()
                .filter(|(channel, _)| visible.contains(channel))
                .flat_map(|(channel, states)| {
                    states.iter().map(|state| RunCountRow {
                        channel_id: Some(*channel),
                        state: (*state).into(),
                        count: 1,
                    })
                })
                .collect::<Vec<_>>(),
        )
    }
}

struct ControlledReader {
    runs: Runs,
    calls: AtomicUsize,
    captured: Semaphore,
    release: Semaphore,
    block_initial: bool,
}

impl ControlledReader {
    fn new(block_initial: bool) -> Arc<Self> {
        Arc::new(Self {
            runs: Runs::default(),
            calls: AtomicUsize::new(0),
            captured: Semaphore::new(0),
            release: Semaphore::new(0),
            block_initial,
        })
    }
}

#[async_trait::async_trait]
impl RunCountsReader for ControlledReader {
    async fn run_counts(&self, _company_id: Uuid, ids: &[Uuid]) -> AppResult<RunCountSnapshot> {
        let result = self.runs.snapshot(ids);
        let initial = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
        if initial && self.block_initial {
            self.captured.add_permits(1);
            self.release.acquire().await.unwrap().forget();
        }
        Ok(result)
    }
}

fn workspace(
    f: &CountsFixture,
    reader: Arc<dyn RunCountsReader>,
    events: MailboxEvents,
    monitoring: Arc<dyn MonitoringService>,
) -> Workspace {
    Workspace {
        companies: Arc::new(CompanyUseCases::new(f.persistence.clone())),
        channels: Arc::new(ChannelUseCases::new(
            f.persistence.clone(),
            f.persistence.clone(),
            f.persistence.clone(),
            test_config(),
        )),
        reader,
        monitoring,
        events,
        viewer: f.owner.clone(),
    }
}

/// A run of the company changed state, as the `workflow_run_changed` trigger reports it.
fn wake(events: &MailboxEvents, f: &CountsFixture) {
    events.publish(MailboxEvent::WorkflowRunChanged(RunScope {
        company_id: f.company_id,
        channel_id: Some(f.channels[0]),
        thread_id: None,
        run_id: Uuid::new_v4(),
    }));
}

async fn open(workspace: Workspace, company_id: Uuid) -> Frames {
    Frames::of(
        run_counts_stream(workspace, Query(CountsQuery { company_id }))
            .await
            .unwrap()
            .into_response(),
    )
}

#[tokio::test]
async fn subscribe_before_query_preserves_a_change_after_the_initial_snapshot_was_captured() {
    let Some(f) = CountsFixture::new().await else {
        return;
    };
    let reader = ControlledReader::new(true);
    let events = MailboxEvents::new();
    let monitoring = Arc::new(InMemoryMonitor::new());
    let mut frames = open(
        workspace(&f, reader.clone(), events.clone(), monitoring.clone()),
        f.company_id,
    )
    .await;
    let initial = tokio::spawn(async move {
        let frame = frames.next_named(pages::RUN_COUNTS_EVENT).await;
        (frames, frame)
    });
    tokio::time::timeout(Duration::from_secs(10), reader.captured.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    reader.runs.open(f.channels[0], "queued");
    wake(&events, &f);
    reader.release.add_permits(1);
    let (mut frames, frame) = initial.await.unwrap();
    assert!(frame.data.contains("No open runs"));
    let finished = Instant::now();
    let frame = frames.next_named(pages::RUN_COUNTS_EVENT).await;
    assert!(finished.elapsed() >= Duration::from_millis(4900));
    assert!(frame.data.contains("1 pending"));
    assert!(frame.data.contains(&format!("channel:{}", f.channels[0])));
    assert!(
        !frame.data.contains("agent:"),
        "runs have no per-agent slots"
    );
    assert_eq!(reader.calls.load(Ordering::SeqCst), 2);
    let metrics = monitoring.get_stats_json();
    let histograms = metrics["histograms"].as_object().unwrap();
    assert!(histograms.iter().any(|(name, value)| {
        name.contains("run_counts_refresh_duration_seconds") && value["count"] == 2
    }));
    let reconnected = ControlledReader::new(false);
    reconnected.runs.open(f.channels[0], "queued");
    let mut reconnect = open(workspace(&f, reconnected, events, monitoring), f.company_id).await;
    assert!(
        reconnect
            .next_named(pages::RUN_COUNTS_EVENT)
            .await
            .data
            .contains("1 pending")
    );
}

#[tokio::test]
async fn unchanged_pass_is_suppressed_and_lag_reconciles_after_the_cooldown() {
    let Some(f) = CountsFixture::new().await else {
        return;
    };
    let events = MailboxEvents::new();
    let monitoring = Arc::new(InMemoryMonitor::new());
    let reader = ControlledReader::new(false);
    let mut frames = open(
        workspace(&f, reader.clone(), events.clone(), monitoring),
        f.company_id,
    )
    .await;
    assert!(
        frames
            .next_named(pages::RUN_COUNTS_EVENT)
            .await
            .data
            .contains("No open runs")
    );
    wake(&events, &f);
    // Polling the body drives the stream through its unchanged pass, but yields no data.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(5500),
            frames.next_named(pages::RUN_COUNTS_EVENT)
        )
        .await
        .is_err()
    );
    assert_eq!(reader.calls.load(Ordering::SeqCst), 2);
    reader.runs.open(f.channels[0], "waiting");
    for _ in 0..crate::infra::events::broadcast_capacity_for_tests() + 1 {
        wake(&events, &f);
    }
    let started = Instant::now();
    assert!(
        frames
            .next_named(pages::RUN_COUNTS_EVENT)
            .await
            .data
            .contains("1 waiting")
    );
    assert!(started.elapsed() >= Duration::from_secs(4));
}

/// p08 D4.1/D4.4: a listener reconnect's `Resync` reconciles the counts in full, though no run
/// event names the change.
#[tokio::test]
async fn a_resync_reconciles_a_change_no_event_named() {
    let Some(f) = CountsFixture::new().await else {
        return;
    };
    let events = MailboxEvents::new();
    let reader = ControlledReader::new(false);
    let mut frames = open(
        workspace(
            &f,
            reader.clone(),
            events.clone(),
            Arc::new(InMemoryMonitor::new()),
        ),
        f.company_id,
    )
    .await;
    frames.next_named(pages::RUN_COUNTS_EVENT).await;
    reader.runs.open(f.channels[1], "running");
    events.publish(MailboxEvent::Resync);
    let frame = frames.next_named(pages::RUN_COUNTS_EVENT).await;
    assert!(frame.data.contains("1 active"));
    assert!(frame.data.contains(&format!("channel:{}", f.channels[1])));
}

#[tokio::test]
async fn periodic_recheck_recovers_a_database_change_without_any_wake() {
    let Some(f) = CountsFixture::new().await else {
        return;
    };
    let events = MailboxEvents::new();
    let reader = ControlledReader::new(false);
    let mut frames = open(
        workspace(&f, reader.clone(), events, Arc::new(InMemoryMonitor::new())),
        f.company_id,
    )
    .await;
    frames.next_named(pages::RUN_COUNTS_EVENT).await;
    reader.runs.open(f.channels[0], "queued");
    // The parser normally has a 10s deadline. Keep polling across its keep-alives and allow the
    // actual fallback interval here, proving reconciliation without a fabricated notification.
    let frame = frames
        .next_named_with_timeout(pages::RUN_COUNTS_EVENT, Duration::from_secs(70))
        .await;
    assert!(frame.data.contains("1 pending"));
}

async fn admin(f: &CountsFixture) -> Viewer {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("counts-admin-{suffix}@example.com");
    f.persistence
        .create_user(&format!("counts-admin-{suffix}"), &email, "hash")
        .await
        .unwrap();
    let user = UserPersistence::get_by_email(f.persistence.as_ref(), &email)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(user.id)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let invite = f
        .persistence
        .create_invite(
            f.company_id,
            &email,
            crate::entities::company_member::CompanyAccessRole::Admin,
        )
        .await
        .unwrap();
    f.persistence
        .accept_pending_invite(invite.id, user.id, &email)
        .await
        .unwrap();
    Viewer {
        user_id: user.id,
        email: email.into(),
        email_verified_at: Some(chrono::Utc::now()),
    }
}

#[tokio::test]
async fn permissions_are_shared_rechecked_and_scoped_to_the_managed_company() {
    let Some(f) = CountsFixture::new().await else {
        return;
    };
    let viewer = admin(&f).await;
    let events = MailboxEvents::new();
    let monitor = Arc::new(InMemoryMonitor::new());
    let reader = ControlledReader::new(false);
    let mut ws = workspace(&f, reader.clone(), events.clone(), monitor.clone());
    ws.viewer = viewer.clone();
    sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE company_id = $1")
        .bind(f.company_id)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    assert!(
        ws.channels
            .list_managed_readable_channels(&viewer, f.company_id)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("INSERT INTO channel_principal_grants (company_id, channel_id, principal_id, capability, provenance)
        SELECT $1, $2, id, 'view', 'manager' FROM principals WHERE company_id = $1 AND user_id = $3")
        .bind(f.company_id).bind(f.channels[0]).bind(viewer.user_id).execute(f.persistence.pool()).await.unwrap();
    reader.runs.open(f.channels[0], "queued");
    reader.runs.open(f.channels[1], "queued");
    let mut frames = open(ws, f.company_id).await;
    let initial = frames.next_named(pages::RUN_COUNTS_EVENT).await.data;
    assert!(initial.contains(&format!("channel:{}", f.channels[0])));
    assert!(!initial.contains(&format!("channel:{}", f.channels[1])));
    sqlx::query("DELETE FROM channel_principal_grants WHERE company_id = $1")
        .bind(f.company_id)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    wake(&events, &f);
    let cleared = frames.next_named(pages::RUN_COUNTS_EVENT).await.data;
    assert!(cleared.contains("No open runs"));
    assert!(!cleared.contains("channel:"));
    sqlx::query(
        "UPDATE company_members SET role = 'member' WHERE company_id = $1 AND user_id = $2",
    )
    .bind(f.company_id)
    .bind(viewer.user_id)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    wake(&events, &f);
    frames.expect_closed().await;
    let mut reconnect = workspace(&f, reader.clone(), events.clone(), monitor.clone());
    reconnect.viewer = viewer;
    assert!(
        run_counts_stream(
            reconnect,
            Query(CountsQuery {
                company_id: f.company_id
            })
        )
        .await
        .is_err()
    );
    let Some(other) = CountsFixture::new().await else {
        return;
    };
    assert!(
        run_counts_stream(
            workspace(&f, reader, events, monitor),
            Query(CountsQuery {
                company_id: other.company_id
            })
        )
        .await
        .is_err()
    );
}
