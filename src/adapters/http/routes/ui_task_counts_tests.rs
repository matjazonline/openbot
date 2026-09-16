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
        persistence::task::counts_test_support::CountsFixture,
    },
    entities::{task::TaskStatus, task_counts::TaskCountSnapshot},
    infra::events::{AttentionScope, AttentionWakeSource, MailboxEvent},
};
use axum::response::IntoResponse;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Semaphore;

struct ControlledReader {
    inner: Arc<dyn TaskCountsReader>,
    calls: AtomicUsize,
    captured: Semaphore,
    release: Semaphore,
    block_initial: bool,
}

#[async_trait::async_trait]
impl TaskCountsReader for ControlledReader {
    async fn task_counts(&self, company_id: Uuid, ids: &[Uuid]) -> AppResult<TaskCountSnapshot> {
        let result = self.inner.task_counts(company_id, ids).await?;
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
    reader: Arc<dyn TaskCountsReader>,
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

fn wake(events: &MailboxEvents, f: &CountsFixture) {
    events.publish(MailboxEvent::AttentionChanged(AttentionScope {
        company_id: f.company_id,
        channel_id: f.channels[0],
        source_kind: AttentionWakeSource::Task,
        source_id: Uuid::new_v4(),
    }));
}

async fn open(workspace: Workspace, company_id: Uuid) -> Frames {
    Frames::of(
        task_counts_stream(workspace, Query(CountsQuery { company_id }))
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
    let reader = Arc::new(ControlledReader {
        inner: f.persistence.clone(),
        calls: AtomicUsize::new(0),
        captured: Semaphore::new(0),
        release: Semaphore::new(0),
        block_initial: true,
    });
    let events = MailboxEvents::new();
    let monitoring = Arc::new(InMemoryMonitor::new());
    let mut frames = open(
        workspace(&f, reader.clone(), events.clone(), monitoring.clone()),
        f.company_id,
    )
    .await;
    let initial = tokio::spawn(async move {
        let frame = frames.next_named(pages::TASK_COUNTS_EVENT).await;
        (frames, frame)
    });
    tokio::time::timeout(Duration::from_secs(10), reader.captured.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    f.task(
        f.channels[0],
        Some((f.principals[0], "agent")),
        TaskStatus::Pending,
    )
    .await;
    wake(&events, &f);
    reader.release.add_permits(1);
    let (mut frames, frame) = initial.await.unwrap();
    assert!(frame.data.contains("No open tasks"));
    let finished = Instant::now();
    let frame = frames.next_named(pages::TASK_COUNTS_EVENT).await;
    assert!(finished.elapsed() >= Duration::from_millis(4900));
    assert!(frame.data.contains("1 pending"));
    assert!(frame.data.contains(&format!("agent:{}", f.agents[0])));
    assert_eq!(reader.calls.load(Ordering::SeqCst), 2);
    let metrics = monitoring.get_stats_json();
    let histograms = metrics["histograms"].as_object().unwrap();
    assert!(histograms.iter().any(|(name, value)| {
        name.contains("task_counts_refresh_duration_seconds") && value["count"] == 2
    }));
    let mut reconnect = open(
        workspace(&f, f.persistence.clone(), events, monitoring),
        f.company_id,
    )
    .await;
    assert!(
        reconnect
            .next_named(pages::TASK_COUNTS_EVENT)
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
    let reader = Arc::new(ControlledReader {
        inner: f.persistence.clone(),
        calls: AtomicUsize::new(0),
        captured: Semaphore::new(0),
        release: Semaphore::new(0),
        block_initial: false,
    });
    let mut frames = open(
        workspace(&f, reader.clone(), events.clone(), monitoring),
        f.company_id,
    )
    .await;
    assert!(
        frames
            .next_named(pages::TASK_COUNTS_EVENT)
            .await
            .data
            .contains("No open tasks")
    );
    wake(&events, &f);
    // Polling the body drives the stream through its unchanged pass, but yields no data.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(5500),
            frames.next_named(pages::TASK_COUNTS_EVENT)
        )
        .await
        .is_err()
    );
    assert_eq!(reader.calls.load(Ordering::SeqCst), 2);
    f.task(f.channels[0], None, TaskStatus::PendingApproval)
        .await;
    for _ in 0..crate::infra::events::broadcast_capacity_for_tests() + 1 {
        wake(&events, &f);
    }
    let started = Instant::now();
    assert!(
        frames
            .next_named(pages::TASK_COUNTS_EVENT)
            .await
            .data
            .contains("1 waiting")
    );
    assert!(started.elapsed() >= Duration::from_secs(4));
}

#[tokio::test]
async fn periodic_recheck_recovers_a_database_change_without_any_wake() {
    let Some(f) = CountsFixture::new().await else {
        return;
    };
    let events = MailboxEvents::new();
    let mut frames = open(
        workspace(
            &f,
            f.persistence.clone(),
            events,
            Arc::new(InMemoryMonitor::new()),
        ),
        f.company_id,
    )
    .await;
    frames.next_named(pages::TASK_COUNTS_EVENT).await;
    f.task(f.channels[0], None, TaskStatus::Pending).await;
    // The parser normally has a 10s deadline. Keep polling across its keep-alives and allow the
    // actual fallback interval here, proving reconciliation without a fabricated notification.
    let frame = frames
        .next_named_with_timeout(pages::TASK_COUNTS_EVENT, Duration::from_secs(70))
        .await;
    assert!(frame.data.contains("1 pending"));
}

async fn admin(f: &CountsFixture) -> Viewer {
    use crate::use_cases::{company_invite::CompanyInvitePersistence, user::UserPersistence};
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
    let mut ws = workspace(&f, f.persistence.clone(), events.clone(), monitor.clone());
    ws.viewer = viewer.clone();
    sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE company_id = $1")
        .bind(f.company_id)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(
        ws.channels
            .list_managed_readable_channels(&f.owner, f.company_id)
            .await
            .unwrap()
            .len(),
        2
    );
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
    f.task(
        f.channels[0],
        Some((f.principals[0], "agent")),
        TaskStatus::Pending,
    )
    .await;
    f.task(
        f.channels[1],
        Some((f.principals[1], "agent")),
        TaskStatus::Pending,
    )
    .await;
    assert_eq!(
        ws.channels
            .list_managed_readable_channels(&viewer, f.company_id)
            .await
            .unwrap()
            .len(),
        1
    );
    let mut frames = open(ws, f.company_id).await;
    let initial = frames.next_named(pages::TASK_COUNTS_EVENT).await.data;
    assert!(initial.contains(&format!("agent:{}", f.agents[0])));
    assert!(!initial.contains(&format!("agent:{}", f.agents[1])));
    sqlx::query("DELETE FROM channel_principal_grants WHERE company_id = $1")
        .bind(f.company_id)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    wake(&events, &f);
    let cleared = frames.next_named(pages::TASK_COUNTS_EVENT).await.data;
    assert!(cleared.contains("No open tasks"));
    assert!(!cleared.contains("agent:"));
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
    let mut reconnect = workspace(&f, f.persistence.clone(), events.clone(), monitor.clone());
    reconnect.viewer = viewer;
    assert!(
        task_counts_stream(
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
        task_counts_stream(
            workspace(&f, f.persistence.clone(), events, monitor),
            Query(CountsQuery {
                company_id: other.company_id
            })
        )
        .await
        .is_err()
    );
}
