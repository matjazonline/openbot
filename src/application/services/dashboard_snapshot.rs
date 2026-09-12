//! One dashboard reading per view per tick, however many tabs are watching it.
//!
//! `/ui/dashboard/events` re-reads on every [`DASHBOARD_TICK`], for every connected tab, and one
//! reading is eight aggregates over the two largest tables. Three of them filter `task_attempts` on
//! a `started_at` window that has no index and no retention sweep. Unshared, that cost grows with
//! the number of open tabs. Shared, it grows with the number of distinct views: companies being
//! watched, times the windows picked, plus the operator rollup.
//!
//! `plan/db_improve/05-deferred-until-traffic.md` §2 ranks this above adding an index. The cache
//! removes the read pressure at no write cost. An index on `background_tasks` would be maintained
//! on every claim, lease renewal and completion.
//!
//! The adapter answers the question, and this service decides how often it is worth asking. That
//! keeps the cache out of [`DashboardPersistence`], so tests can still drive the uncached path
//! directly.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as SyncMutex, PoisonError},
    time::Duration,
};

use async_trait::async_trait;
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::dashboard::{DashboardSnapshot, DashboardWindow},
};

/// How often a connected dashboard re-reads, and so how long one reading is shared.
///
/// Slow enough that a room full of open tabs is not a load generator, fast enough that a queue
/// draining is visibly a queue draining. The SSE loop ticks on this constant and the cache expires
/// on it, so the two cannot drift apart. A reading kept longer than a tick would show an operator
/// stale queue depths during an incident. A reading dropped sooner would let every tab query for
/// itself again.
pub const DASHBOARD_TICK: Duration = Duration::from_secs(5);

#[async_trait]
pub trait DashboardPersistence: Send + Sync {
    /// One complete reading, for `company` or — with `None` — for every company at once.
    async fn dashboard_snapshot(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<DashboardSnapshot>;
}

/// Which reading a request wants.
///
/// The key is the whole `Option<Uuid>`, not whether it is set, and it includes the window. Leave
/// out either half and one view's reading is served to another: company A's rollup on company B's
/// page, or a company's numbers under the operator's global heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct View {
    company: Option<Uuid>,
    window: DashboardWindow,
}

/// One reading, successful or not, and when it was taken.
struct Reading {
    result: AppResult<DashboardSnapshot>,
    /// When the queries behind this reading were issued. The numbers are this old.
    read_at: Instant,
    /// When the queries came back.
    ready_at: Instant,
}

impl Reading {
    /// Less than one TTL old, measured from when the queries were issued.
    ///
    /// Measured from the issue time, not the completion time. The SSE loop ticks on a fixed
    /// schedule, so measuring from completion would put every tick a few milliseconds inside the
    /// previous tick's reading. A lone tab would then get fresh numbers only on every other tick.
    fn is_fresh(&self, now: Instant, ttl: Duration) -> bool {
        now < self.read_at + ttl
    }

    /// Whether this reading answers a request made at `asked_at`.
    ///
    /// A fresh reading answers anything. A stale one still answers a caller who was already
    /// waiting when it came back: that caller waited for exactly this reading, and asking again
    /// would repeat it. This matters only when a reading takes longer than a tick. Without it,
    /// every caller queued behind a slow reading would find it stale and run another, one after
    /// another, exactly when the database is struggling.
    fn answers(&self, asked_at: Instant, now: Instant, ttl: Duration) -> bool {
        self.is_fresh(now, ttl) || self.ready_at > asked_at
    }
}

/// One view's cached reading, behind the lock that makes it single-flight.
type Slot = Arc<Mutex<Option<Reading>>>;

/// Dashboard readings shared across tabs for one tick, keyed by [`View`].
///
/// This copies [`super::database_query_health::DatabaseQueryHealthService`]. One mutex
/// deliberately covers both the cache check and the refresh. A refresh happens at most once, and
/// concurrent callers wait for it and then reuse the same reading. Here that mutex is per view
/// rather than one for the whole service. One company's refresh then never waits behind another
/// company's, and a slow operator rollup never holds up a company's page.
///
/// Failures are cached too, for the same one tick. A database that is refusing connections
/// should not receive eight aggregate queries per tab per tick on top of whatever is already wrong
/// with it. The caller that ran the failed reading and every caller served from it get the same
/// error. The SSE loop already treats that as a skipped tick.
pub struct DashboardSnapshotService {
    persistence: Arc<dyn DashboardPersistence>,
    ttl: Duration,
    views: SyncMutex<HashMap<View, Slot>>,
}

impl DashboardSnapshotService {
    pub fn new(persistence: Arc<dyn DashboardPersistence>) -> Self {
        Self::with_ttl(persistence, DASHBOARD_TICK)
    }

    /// A TTL longer than any test takes, so a database-backed test can tell a cached reading from
    /// a fresh one without racing the clock.
    #[cfg(test)]
    pub(crate) fn with_long_ttl(persistence: Arc<dyn DashboardPersistence>) -> Self {
        Self::with_ttl(persistence, Duration::from_secs(3600))
    }

    fn with_ttl(persistence: Arc<dyn DashboardPersistence>, ttl: Duration) -> Self {
        Self {
            persistence,
            ttl,
            views: SyncMutex::new(HashMap::new()),
        }
    }

    /// The reading for `company` over `window`, from the cache when one is fresh enough.
    pub async fn snapshot(
        &self,
        company: Option<Uuid>,
        window: DashboardWindow,
    ) -> AppResult<DashboardSnapshot> {
        let asked_at = Instant::now();
        let slot = self.slot(View { company, window }, asked_at);
        let mut cached = slot.lock().await;
        if let Some(reading) = cached
            .as_ref()
            .filter(|reading| reading.answers(asked_at, Instant::now(), self.ttl))
        {
            return reading.result.clone();
        }

        let read_at = Instant::now();
        let result = self.persistence.dashboard_snapshot(company, window).await;
        *cached = Some(Reading {
            result: result.clone(),
            read_at,
            ready_at: Instant::now(),
        });
        result
    }

    /// The slot for `view`, created on first use.
    ///
    /// Idle slots are swept whenever a new slot is created. A slot nobody holds, whose reading is
    /// no longer fresh, can never be served again: a stale reading answers only callers already
    /// waiting on it, and a caller that is waiting holds the slot. Without the sweep the map would
    /// keep one stale snapshot for every company and window read since boot.
    fn slot(&self, view: View, now: Instant) -> Slot {
        let mut views = self.views.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(slot) = views.get(&view) {
            return slot.clone();
        }
        // A slot's strong count is one only when the map alone holds it. A caller holding the
        // lock also holds an `Arc`, so `try_lock` cannot fail on a slot that reaches it.
        views.retain(|_, slot| {
            Arc::strong_count(slot) > 1
                || slot.try_lock().is_ok_and(|reading| {
                    reading
                        .as_ref()
                        .is_some_and(|reading| reading.is_fresh(now, self.ttl))
                })
        });
        views.entry(view).or_default().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::{app_error::AppError, entities::dashboard::TaskQueueHealth};

    /// Persistence that records every view it was asked for and tags each reading with that call's
    /// index. A reading served to a request can then be traced back to the call that produced it.
    struct Reads {
        calls: SyncMutex<Vec<View>>,
        fail: AtomicBool,
        latency: Duration,
    }

    impl Reads {
        fn new() -> Self {
            Self {
                calls: SyncMutex::new(Vec::new()),
                fail: AtomicBool::new(false),
                latency: Duration::ZERO,
            }
        }

        fn failing() -> Self {
            Self {
                fail: AtomicBool::new(true),
                ..Self::new()
            }
        }

        fn taking(latency: Duration) -> Self {
            Self {
                latency,
                ..Self::new()
            }
        }

        fn count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        /// The view whose read produced `snapshot`.
        fn view_behind(&self, snapshot: &DashboardSnapshot) -> View {
            self.calls.lock().unwrap()[snapshot.tasks.due_now as usize]
        }
    }

    #[async_trait]
    impl DashboardPersistence for Reads {
        async fn dashboard_snapshot(
            &self,
            company: Option<Uuid>,
            window: DashboardWindow,
        ) -> AppResult<DashboardSnapshot> {
            let call = {
                let mut calls = self.calls.lock().unwrap();
                calls.push(View { company, window });
                calls.len() - 1
            };
            if self.latency.is_zero() {
                tokio::task::yield_now().await;
            } else {
                tokio::time::sleep(self.latency).await;
            }
            if self.fail.load(Ordering::SeqCst) {
                return Err(AppError::Database("connection refused".into()));
            }
            Ok(DashboardSnapshot {
                tasks: TaskQueueHealth {
                    due_now: call as i64,
                    ..TaskQueueHealth::default()
                },
                ..DashboardSnapshot::default()
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_reading_is_shared_for_one_tick_and_refreshed_after_it() {
        let persistence = Arc::new(Reads::new());
        let service = DashboardSnapshotService::new(persistence.clone());
        let window = DashboardWindow::last_hour();

        service.snapshot(None, window).await.unwrap();
        tokio::time::advance(DASHBOARD_TICK - Duration::from_millis(1)).await;
        service.snapshot(None, window).await.unwrap();
        assert_eq!(persistence.count(), 1);

        tokio::time::advance(Duration::from_millis(1)).await;
        service.snapshot(None, window).await.unwrap();
        assert_eq!(persistence.count(), 2);
    }

    /// A lone tab ticking on [`DASHBOARD_TICK`] gets a fresh reading on every tick.
    ///
    /// This is why freshness runs from when a reading was issued. Measured from completion, each
    /// tick would land inside the previous reading's TTL by however long that reading took, and a
    /// single tab would refresh only every other tick.
    #[tokio::test(start_paused = true)]
    async fn a_lone_tab_gets_a_fresh_reading_on_every_tick() {
        let persistence = Arc::new(Reads::taking(Duration::from_millis(200)));
        let service = DashboardSnapshotService::new(persistence.clone());
        let mut ticker = tokio::time::interval(DASHBOARD_TICK);

        for tick in 1..=4 {
            ticker.tick().await;
            service
                .snapshot(None, DashboardWindow::last_hour())
                .await
                .unwrap();
            assert_eq!(
                persistence.count(),
                tick,
                "tick {tick} was served a stale reading"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn failures_are_cached_for_one_tick_too() {
        let persistence = Arc::new(Reads::failing());
        let service = DashboardSnapshotService::new(persistence.clone());
        let window = DashboardWindow::last_hour();

        assert!(matches!(
            service.snapshot(None, window).await,
            Err(AppError::Database(_))
        ));
        tokio::time::advance(DASHBOARD_TICK - Duration::from_millis(1)).await;
        assert!(matches!(
            service.snapshot(None, window).await,
            Err(AppError::Database(_))
        ));
        assert_eq!(persistence.count(), 1);

        tokio::time::advance(Duration::from_millis(1)).await;
        persistence.fail.store(false, Ordering::SeqCst);
        service.snapshot(None, window).await.unwrap();
        assert_eq!(persistence.count(), 2);
    }

    #[tokio::test]
    async fn concurrent_misses_on_one_view_share_one_read() {
        let persistence = Arc::new(Reads::new());
        let service = Arc::new(DashboardSnapshotService::new(persistence.clone()));
        let company = Some(Uuid::new_v4());

        let mut readers = Vec::new();
        for _ in 0..20 {
            let service = service.clone();
            readers.push(tokio::spawn(async move {
                service
                    .snapshot(company, DashboardWindow::last_hour())
                    .await
            }));
        }
        for reader in readers {
            reader.await.unwrap().unwrap();
        }
        assert_eq!(persistence.count(), 1);
    }

    /// A reading slower than a tick is still shared by everyone who queued behind it.
    #[tokio::test(start_paused = true)]
    async fn callers_queued_behind_a_slow_reading_reuse_it() {
        let persistence = Arc::new(Reads::taking(DASHBOARD_TICK * 3));
        let service = Arc::new(DashboardSnapshotService::new(persistence.clone()));

        let mut readers = Vec::new();
        for _ in 0..5 {
            let service = service.clone();
            readers.push(tokio::spawn(async move {
                service.snapshot(None, DashboardWindow::last_hour()).await
            }));
            // Spread the arrivals across the slow read, well past one tick after it started.
            tokio::time::sleep(DASHBOARD_TICK / 2).await;
        }
        for reader in readers {
            reader.await.unwrap().unwrap();
        }
        assert_eq!(persistence.count(), 1);

        // Someone arriving after it came back, now more than a tick old, gets a new one.
        service
            .snapshot(None, DashboardWindow::last_hour())
            .await
            .unwrap();
        assert_eq!(persistence.count(), 2);
    }

    /// The cache must never serve one company's reading to another, the global rollup to a
    /// company, a company to the global rollup, or one window's reading to another.
    ///
    /// Every reading carries the index of the call that produced it, so each answer, cached or
    /// not, is checked against the view it was actually read for. A cache keyed on the window
    /// alone, or on whether a company is set rather than which one, fails here.
    #[tokio::test(start_paused = true)]
    async fn every_view_is_answered_from_its_own_reading() {
        let persistence = Arc::new(Reads::new());
        let service = DashboardSnapshotService::new(persistence.clone());
        let (a, b) = (Some(Uuid::new_v4()), Some(Uuid::new_v4()));
        let views = [
            View {
                company: a,
                window: DashboardWindow::last_hour(),
            },
            View {
                company: b,
                window: DashboardWindow::last_hour(),
            },
            View {
                company: None,
                window: DashboardWindow::last_hour(),
            },
            View {
                company: a,
                window: DashboardWindow::last_day(),
            },
        ];

        let mut first = Vec::new();
        for view in views {
            let snapshot = service.snapshot(view.company, view.window).await.unwrap();
            assert_eq!(persistence.view_behind(&snapshot), view);
            first.push(snapshot.tasks.due_now);
        }
        assert_eq!(persistence.count(), views.len(), "each view reads once");

        tokio::time::advance(DASHBOARD_TICK / 2).await;
        for (view, first) in views.into_iter().zip(first) {
            let snapshot = service.snapshot(view.company, view.window).await.unwrap();
            assert_eq!(persistence.view_behind(&snapshot), view);
            assert_eq!(
                snapshot.tasks.due_now, first,
                "{view:?} was not served from its cache"
            );
        }
        assert_eq!(
            persistence.count(),
            views.len(),
            "inside the tick, nothing re-reads"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_view_is_dropped_when_another_is_first_read() {
        let persistence = Arc::new(Reads::new());
        let service = DashboardSnapshotService::new(persistence.clone());
        let window = DashboardWindow::last_hour();
        let (a, b) = (Some(Uuid::new_v4()), Some(Uuid::new_v4()));

        service.snapshot(a, window).await.unwrap();
        tokio::time::advance(DASHBOARD_TICK).await;
        service.snapshot(b, window).await.unwrap();

        let views = service.views.lock().unwrap();
        assert!(views.contains_key(&View { company: b, window }));
        assert!(
            !views.contains_key(&View { company: a, window }),
            "a reading that can never be served again is not kept"
        );
    }
}
