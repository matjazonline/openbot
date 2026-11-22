//! `unit_archive_runs_schedule_` (Plan08 02B.2, D1.6): an archived unit's schedules change no
//! state while it stays archived, refuse configuration except disable and delete, and fire once
//! after a restore; an occurrence claimed just before the archive is dropped, not failed.
use super::*;
use crate::application::ports::department::OpenWork;
use crate::entities::department::DepartmentId;

/// The fixture's binding, driving schedules on a desk of a non-root unit.
struct Archived {
    s: Schedules,
    unit: Uuid,
    desk: Uuid,
}

impl Archived {
    async fn new() -> Self {
        let s = Schedules::new().await;
        let unit = s.p().test_unit(s.company(), "scheduled").await;
        let desk = crate::adapters::persistence::test_support::create_channel(
            s.p(),
            s.company(),
            ChannelWrite {
                name: "Scheduled desk".into(),
                slug: "scheduled-desk".into(),
                department_id: Some(DepartmentId::new(unit)),
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap()
        .id;
        Self { s, unit, desk }
    }
    async fn schedule(&self, write: ScheduleWrite) -> AppResult<ChannelSchedule> {
        self.s
            .use_cases
            .create_schedule(self.s.owner(), self.s.company(), self.desk, write)
            .await
    }
    fn one_off(&self) -> ScheduleWrite {
        let mut write = self.s.write("Once", self.s.binding);
        write.schedule_type = ScheduleType::OneOff;
        write.interval_seconds = None;
        write.scheduled_at = Some(Utc::now() + chrono::Duration::hours(1));
        write
    }
    async fn archive(&self) {
        crate::adapters::persistence::test_support::department_use_cases(self.s.p())
            .archive_department(
                self.s.owner(),
                self.s.company(),
                DepartmentId::new(self.unit),
                OpenWork::Refuse,
            )
            .await
            .unwrap();
    }
    async fn restore(&self) {
        crate::adapters::persistence::test_support::department_use_cases(self.s.p())
            .restore_department(
                self.s.owner(),
                self.s.company(),
                DepartmentId::new(self.unit),
            )
            .await
            .unwrap();
    }
    /// `(enabled, next_run_at, last_run_at, last_error)` of `schedule`.
    async fn columns(
        &self,
        schedule: Uuid,
    ) -> (
        bool,
        Option<chrono::DateTime<Utc>>,
        Option<chrono::DateTime<Utc>>,
        Option<String>,
    ) {
        sqlx::query_as(
            "SELECT enabled, next_run_at, last_run_at, last_error FROM channel_schedules WHERE id = $1",
        )
        .bind(schedule)
        .fetch_one(self.s.pool())
        .await
        .unwrap()
    }
}

fn refused<T>(result: &AppResult<T>) -> bool {
    matches!(result, Err(AppError::UnitArchived))
}

#[tokio::test]
async fn unit_archive_runs_schedule_is_untouched_while_archived_and_fires_once_after_restore() {
    let a = Archived::new().await;
    let hourly = a.schedule(a.s.write("Hourly", a.s.binding)).await.unwrap();
    let once = a.schedule(a.one_off()).await.unwrap();
    let paused = a.schedule(a.s.write("Paused", a.s.binding)).await.unwrap();
    a.s.use_cases
        .toggle_schedule(a.s.owner(), a.s.company(), paused.id, false)
        .await
        .unwrap();
    a.s.due(hourly.id).await;
    a.s.due(once.id).await;
    a.archive().await;
    let before = (a.columns(hourly.id).await, a.columns(once.id).await);
    let materializer = a.s.materializer(admission(a.s.p()));
    assert!(a.s.drain(&materializer).await.is_empty());
    assert_eq!(
        (a.columns(hourly.id).await, a.columns(once.id).await),
        before,
        "an archived unit's schedules change no state"
    );
    assert!(a.s.occurrences(hourly.id).await.is_empty());
    assert!(a.s.occurrences(once.id).await.is_empty());

    // Configuration refuses; disable and delete pass.
    assert!(refused(&a.schedule(a.s.write("New", a.s.binding)).await));
    assert!(refused(
        &a.s.use_cases
            .update_schedule(
                a.s.owner(),
                a.s.company(),
                hourly.id,
                a.desk,
                a.s.write("Renamed", a.s.binding),
            )
            .await
    ));
    assert!(refused(
        &a.s.use_cases
            .toggle_schedule(a.s.owner(), a.s.company(), paused.id, true)
            .await
    ));
    assert!(refused(
        &a.s.use_cases
            .trigger_schedule_now(a.s.owner(), a.s.company(), hourly.id, "archived-run-now")
            .await
    ));
    assert!(a.s.occurrences(hourly.id).await.is_empty());
    a.s.use_cases
        .toggle_schedule(a.s.owner(), a.s.company(), hourly.id, false)
        .await
        .expect("a schedule of an archived unit can be paused");
    a.s.use_cases
        .delete_schedule(a.s.owner(), a.s.company(), paused.id)
        .await
        .expect("a schedule of an archived unit can be deleted");
    a.s.use_cases
        .toggle_schedule(a.s.owner(), a.s.company(), hourly.id, true)
        .await
        .expect_err("still archived");

    // After the restore the one-off still due fires once; the interval resumes too.
    a.restore().await;
    a.s.use_cases
        .toggle_schedule(a.s.owner(), a.s.company(), hourly.id, true)
        .await
        .unwrap();
    a.s.due(hourly.id).await;
    let outcomes = a.s.drain(&materializer).await;
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");
    assert!(
        outcomes
            .iter()
            .all(|outcome| matches!(outcome, Materialized::Admitted(_))),
        "{outcomes:?}"
    );
    assert_eq!(a.s.occurrences(once.id).await.len(), 1);
    assert_eq!(a.s.occurrences(hourly.id).await.len(), 1);
    assert!(a.s.drain(&materializer).await.is_empty(), "each fired once");
}

#[tokio::test]
async fn unit_archive_runs_schedule_claimed_before_the_archive_is_dropped_not_failed() {
    let a = Archived::new().await;
    let hourly = a.schedule(a.s.write("Hourly", a.s.binding)).await.unwrap();
    a.s.due(hourly.id).await;
    let materializer = a.s.materializer(admission(a.s.p()));
    let worker = ScheduleWorkerId(Uuid::new_v4());
    let claimed = materializer.claim(worker).await.unwrap();
    assert_eq!(claimed.len(), 1);
    a.archive().await;
    assert_eq!(
        materializer.materialize(worker, &claimed[0]).await.unwrap(),
        Materialized::Skipped
    );
    assert!(
        a.s.occurrences(hourly.id).await.is_empty(),
        "the claimed occurrence is gone, with no failed row"
    );
    assert_eq!(a.columns(hourly.id).await.3, None, "no refusal is named");
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE channel_id = $1")
        .bind(a.desk)
        .fetch_one(a.s.pool())
        .await
        .unwrap();
    assert_eq!(runs, 0);
}
