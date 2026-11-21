//! Schedule configuration is scoped to its desk; run inspection and initiation still need View.
use super::*;
use crate::adapters::persistence::workflow::schedule_test_support::{
    bind, publish, schedule_use_cases, source,
};
use crate::application::ports::channel::{ChannelPersistence, ChannelWrite};
use crate::application::ports::participant::PrincipalAccessPersistence;
use crate::application::use_cases::schedule::{SchedulePersistence, ScheduleUseCases};
use crate::application::workflow::{RelatedAssociation, RelatedChannelId};
use crate::domain::workflow::{VersionId, WorkflowBindingId};
use crate::entities::schedule::{
    ChannelSchedule, OccurrenceStatus, ScheduleTimezone, ScheduleType, ScheduleWrite,
};

struct ManagedSchedules {
    d: Delegated,
    desk: Uuid,
    version: VersionId,
    binding: WorkflowBindingId,
    schedule: ChannelSchedule,
    cases: ScheduleUseCases,
}

fn write(binding: WorkflowBindingId, name: &str) -> ScheduleWrite {
    ScheduleWrite {
        name: name.into(),
        schedule_type: ScheduleType::Interval,
        interval_seconds: Some(3600),
        scheduled_at: None,
        timezone: ScheduleTimezone::utc(),
        binding_id: binding.as_uuid(),
        enabled: true,
    }
}

impl ManagedSchedules {
    async fn on(pool: PgPool) -> Self {
        let d = Delegated::on(pool).await;
        let remote = d.remote().await;
        let desk = d.f.desk_of(remote.id).await;
        let p = &d.f.persistence;
        let version = publish(p, d.f.company, d.f.owner, source(None)).await;
        let binding = bind(
            p,
            d.f.company,
            d.f.owner,
            version,
            RelatedAssociation::Channel(RelatedChannelId::new(desk)),
            None,
        )
        .await;
        let cases = schedule_use_cases(p);
        let schedule = cases
            .create_schedule(d.f.owner, d.f.company, desk, write(binding, "Initial"))
            .await
            .unwrap();
        Self {
            d,
            desk,
            version,
            binding,
            schedule,
            cases,
        }
    }

    fn p(&self) -> &PostgresPersistence {
        &self.d.f.persistence
    }

    async fn assert_no_view(&self) {
        let context = self
            .p()
            .access_context_for_user(self.d.f.company, self.d.f.member)
            .await
            .unwrap()
            .unwrap();
        let desk = ChannelPersistence::get_by_id(self.p(), self.desk)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !desk.viewer_access(context),
            "the real ancestor manager has no desk grant"
        );
    }

    async fn sibling(&self) -> ChannelSchedule {
        let desk = self.d.f.desk_of(self.d.sibling.id).await;
        let binding = bind(
            self.p(),
            self.d.f.company,
            self.d.f.owner,
            self.version,
            RelatedAssociation::Channel(RelatedChannelId::new(desk)),
            None,
        )
        .await;
        self.cases
            .create_schedule(
                self.d.f.owner,
                self.d.f.company,
                desk,
                write(binding, "Sibling"),
            )
            .await
            .unwrap()
    }

    async fn foreign(&self) -> (CompanyId, Uuid, ChannelSchedule) {
        let p = self.p();
        let company = self.d.f.foreign_company;
        let owner =
            crate::adapters::persistence::test_support::company_owner_actor(p, company).await;
        let desk = crate::adapters::persistence::test_support::create_channel(
            p,
            company,
            ChannelWrite {
                name: "Foreign schedule".into(),
                slug: "foreign-schedule".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let version = publish(p, company, owner.user_id(), source(None)).await;
        let binding = bind(
            p,
            company,
            owner.user_id(),
            version,
            RelatedAssociation::Channel(RelatedChannelId::new(desk.id)),
            None,
        )
        .await;
        let schedule = self
            .cases
            .create_schedule(owner.user_id(), company, desk.id, write(binding, "Foreign"))
            .await
            .unwrap();
        (CompanyId::new(company), desk.id, schedule)
    }

    async fn grant_view(&self) {
        let email: String = sqlx::query_scalar(
            "UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1 RETURNING email",
        )
        .bind(self.d.f.member)
        .fetch_one(self.p().pool())
        .await
        .unwrap();
        let current = ChannelPersistence::get_by_id(self.p(), self.desk)
            .await
            .unwrap()
            .unwrap();
        super::desks::cases(&self.d)
            .update_channel(
                self.d.f.owner,
                self.d.f.company,
                self.desk,
                ChannelWrite {
                    name: current.name,
                    slug: current.slug.to_string(),
                    participant_emails: Some(vec![email]),
                    enabled: true,
                    ..Default::default()
                },
                false,
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn delegated_admin_schedule_configuration_and_lists_follow_managed_units_without_view() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = ManagedSchedules::on(pool).await;
    f.assert_no_view().await;
    let sibling = f.sibling().await;
    let actor = f.d.f.member;
    let company = f.d.f.company;
    let created = f
        .cases
        .create_schedule(actor, company, f.desk, write(f.binding, "Created"))
        .await
        .unwrap();
    let updated = f
        .cases
        .update_schedule(
            actor,
            company,
            created.id,
            f.desk,
            write(f.binding, "Edited"),
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Edited");
    f.cases
        .toggle_schedule(actor, company, created.id, false)
        .await
        .unwrap();
    assert!(
        !f.cases
            .get_schedule(actor, company, created.id)
            .await
            .unwrap()
            .unwrap()
            .enabled
    );
    let mut ids: Vec<_> = f
        .cases
        .list_company_schedules(actor, company)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect();
    ids.sort();
    let mut expected = vec![f.schedule.id, created.id];
    expected.sort();
    assert_eq!(ids, expected);
    assert!(
        f.cases
            .get_schedule(actor, company, sibling.id)
            .await
            .unwrap()
            .is_none()
    );
    f.cases
        .delete_schedule(actor, company, created.id)
        .await
        .unwrap();
    assert!(
        f.p()
            .get(f.d.company(), created.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.p()
            .get(f.d.company(), sibling.id)
            .await
            .unwrap()
            .is_some()
    );
    f.assert_no_view().await;
}

#[tokio::test]
async fn delegated_admin_schedule_run_inspection_and_initiation_require_current_human_view() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = ManagedSchedules::on(database.pool.clone()).await;
    f.assert_no_view().await;
    let actor = f.d.f.member;
    let company = f.d.f.company;
    hidden(
        f.cases
            .list_schedule_runs(actor, company, f.schedule.id, 0, 10)
            .await,
    );
    hidden(
        f.cases
            .trigger_schedule_now(actor, company, f.schedule.id, "private")
            .await,
    );
    hidden(
        f.p()
            .capture_now(
                f.d.scope(actor).actor,
                f.d.company(),
                f.schedule.id,
                "direct-private",
            )
            .await,
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM schedule_runs WHERE company_id=$1 AND schedule_id=$2",
    )
    .bind(company)
    .bind(f.schedule.id)
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(count, 0);
    f.grant_view().await;
    let run = f
        .cases
        .trigger_schedule_now(actor, company, f.schedule.id, "granted")
        .await
        .unwrap();
    assert_eq!(run.status, OccurrenceStatus::Materialized);
    let runs = f
        .cases
        .list_schedule_runs(actor, company, f.schedule.id, 0, 10)
        .await
        .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, run.id);
}

async fn refused_writes(f: &ManagedSchedules, actor: WorkflowActor) {
    let p = f.p();
    let company = f.d.company();
    let id = f.schedule.id;
    hidden(
        SchedulePersistence::create(p, actor, company, f.desk, write(f.binding, "Refused")).await,
    );
    hidden(
        SchedulePersistence::update(p, actor, company, id, f.desk, write(f.binding, "Refused"))
            .await,
    );
    hidden(p.set_enabled(actor, company, id, false).await);
    hidden(SchedulePersistence::delete(p, actor, company, id).await);
    hidden(p.capture_now(actor, company, id, "refused").await);
    assert_eq!(p.get(company, id).await.unwrap(), Some(f.schedule.clone()));
}

#[tokio::test]
async fn delegated_admin_schedule_persistence_refuses_sibling_and_foreign_writers() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = ManagedSchedules::on(pool).await;
    refused_writes(&f, f.d.scope(f.d.sibling_lead).actor).await;
    let foreign_owner: Uuid = sqlx::query_scalar("SELECT user_id FROM companies WHERE id=$1")
        .bind(f.d.f.foreign_company)
        .fetch_one(f.p().pool())
        .await
        .unwrap();
    refused_writes(&f, f.d.scope(foreign_owner).actor).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channel_schedules WHERE company_id=$1 AND channel_id=$2",
    )
    .bind(f.d.f.company)
    .bind(f.desk)
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
    f.p()
        .set_enabled(
            f.d.scope(f.d.f.member).actor,
            f.d.company(),
            f.schedule.id,
            false,
        )
        .await
        .unwrap();
    assert!(
        !f.p()
            .get(f.d.company(), f.schedule.id)
            .await
            .unwrap()
            .unwrap()
            .enabled
    );
}

#[tokio::test]
async fn delegated_admin_schedule_persistence_hides_foreign_ids_in_the_local_scope() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = ManagedSchedules::on(pool).await;
    let (foreign_company, foreign_desk, foreign) = f.foreign().await;
    let actor = f.d.scope(f.d.f.member).actor;
    let company = f.d.company();
    let p = f.p();
    hidden(
        SchedulePersistence::create(p, actor, company, foreign_desk, write(f.binding, "Refused"))
            .await,
    );
    hidden(
        SchedulePersistence::update(
            p,
            actor,
            company,
            foreign.id,
            f.desk,
            write(f.binding, "Refused"),
        )
        .await,
    );
    hidden(SchedulePersistence::delete(p, actor, company, foreign.id).await);
    hidden(p.set_enabled(actor, company, foreign.id, false).await);
    hidden(p.capture_now(actor, company, foreign.id, "foreign").await);
    assert_eq!(
        p.get(foreign_company, foreign.id).await.unwrap(),
        Some(foreign.clone())
    );
    assert_eq!(
        p.get(company, f.schedule.id).await.unwrap(),
        Some(f.schedule.clone())
    );
    assert!(p.get(company, foreign.id).await.unwrap().is_none());
}

#[tokio::test]
async fn delegated_admin_competing_role_removal_precedes_schedule_configuration() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(ManagedSchedules::on(database.pool.clone()).await);
    let d = &f.d;
    let mut removal = begin_department_write(f.p(), d.scope(d.f.owner))
        .await
        .unwrap();
    clear_department_role_on(
        &mut removal,
        &d.f.departments.templates,
        d.company(),
        d.scope(d.f.owner).actor,
        d.unit.id,
        crate::application::ports::department::DepartmentRoleRemoval {
            role: DepartmentRole::Lead,
            principal: d.f.member_principal,
            successor: None,
        },
    )
    .await
    .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *removal)
        .await
        .unwrap();
    let writer = tokio::spawn({
        let f = f.clone();
        async move {
            f.p()
                .set_enabled(
                    f.d.scope(f.d.f.member).actor,
                    f.d.company(),
                    f.schedule.id,
                    false,
                )
                .await
        }
    });
    crate::adapters::persistence::test_support::wait_until_blocked_by(d.f.pool(), blocker).await;
    removal.commit().await.unwrap();
    hidden(writer.await.unwrap());
    assert_eq!(
        f.p().get(d.company(), f.schedule.id).await.unwrap(),
        Some(f.schedule.clone())
    );
}

#[path = "delegated_schedule_account_lock_tests.rs"]
mod account_locks;
