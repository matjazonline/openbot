//! p08 D1.9 against PostgreSQL: schedules on bindings. Who may manage a schedule, which binding
//! may drive it, how a captured occurrence becomes exactly one run under competing materializers
//! and failures, and what the schema refuses outright.
use super::*;
use crate::adapters::monitoring::in_memory_monitor::InMemoryMonitor;
use crate::adapters::persistence::workflow::schedule_test_support::{
    bind, publish, schedule_use_cases, source,
};
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::application::use_cases::schedule::{SchedulePersistence, ScheduleUseCases};
use crate::application::workflow::manual::{self, StartRun};
use crate::application::workflow::message_admission::WorkflowAdmitter;
use crate::application::workflow::schedule_admission::*;
use crate::domain::entities::schedule::{
    ChannelSchedule, OccurrenceStatus, ScheduleTimezone, ScheduleType, ScheduleWrite,
};
use crate::domain::monitoring::MonitoringService;
use chrono::{SubsecRound, Utc};
use serde_json::{Value, json};

type Admission = WorkflowService<
    PostgresPersistence,
    PostgresPersistence,
    PostgresPersistence,
    PostgresPersistence,
    LifecycleAuthorizer<PostgresPersistence, PostgresPersistence, PostgresPersistence>,
>;

fn admission(p: &PostgresPersistence) -> Admission {
    WorkflowService::new(
        p.clone(),
        p.clone(),
        p.clone(),
        p.clone(),
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
    )
}

/// The real admission service, failing after it committed: a materializer that crashed before
/// recording the run.
struct CrashAfterAdmission(Admission);

#[async_trait]
impl WorkflowAdmitter for CrashAfterAdmission {
    async fn admit(&self, request: AdmitWorkflowRequest) -> AppResult<AdmissionResult> {
        self.0.admit(request).await?;
        Err(AppError::Database("scripted crash after admission".into()))
    }
}

/// Admission is down: every call fails transiently before anything is written.
struct Outage;

#[async_trait]
impl WorkflowAdmitter for Outage {
    async fn admit(&self, _: AdmitWorkflowRequest) -> AppResult<AdmissionResult> {
        Err(AppError::Database("scripted admission outage".into()))
    }
}

/// A company with a channel, its owner, and an active company binding that accepts the
/// schedule input, run as the owner.
struct Schedules {
    fixture: Fixture,
    channel: Uuid,
    binding: Uuid,
    use_cases: ScheduleUseCases,
}

impl Schedules {
    async fn new() -> Self {
        let fixture = Fixture::new().await;
        let channel = channel(&fixture, "reports").await;
        let p = fixture.persistence.clone();
        let version = publish(
            &p,
            fixture.target.company.as_uuid(),
            fixture.target.actor.user_id(),
            source(None),
        )
        .await;
        let binding = bind(
            &p,
            fixture.target.company.as_uuid(),
            fixture.target.actor.user_id(),
            version,
            RelatedAssociation::Company,
            None,
        )
        .await
        .as_uuid();
        let use_cases = schedule_use_cases(&p);
        Self {
            fixture,
            channel,
            binding,
            use_cases,
        }
    }

    fn p(&self) -> &PostgresPersistence {
        &self.fixture.persistence
    }

    fn pool(&self) -> &sqlx::PgPool {
        self.p().pool()
    }

    fn company(&self) -> Uuid {
        self.fixture.target.company.as_uuid()
    }

    fn owner(&self) -> Uuid {
        self.fixture.target.actor.user_id()
    }

    fn write(&self, name: &str, binding: Uuid) -> ScheduleWrite {
        ScheduleWrite {
            name: name.into(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(3600),
            scheduled_at: None,
            timezone: ScheduleTimezone::utc(),
            binding_id: binding,
            enabled: true,
        }
    }

    async fn schedule(&self, name: &str) -> ChannelSchedule {
        self.use_cases
            .create_schedule(
                self.owner(),
                self.company(),
                self.channel,
                self.write(name, self.binding),
            )
            .await
            .unwrap()
    }

    /// Make `schedule`'s next slot due a minute ago.
    async fn due(&self, schedule: Uuid) -> chrono::DateTime<Utc> {
        let slot = (Utc::now() - chrono::Duration::minutes(1)).trunc_subsecs(6);
        sqlx::query("UPDATE channel_schedules SET next_run_at = $2 WHERE id = $1")
            .bind(schedule)
            .bind(slot)
            .execute(self.pool())
            .await
            .unwrap();
        slot
    }

    fn materializer<A: WorkflowAdmitter>(
        &self,
        admitter: A,
    ) -> ScheduleMaterializer<PostgresPersistence, A> {
        ScheduleMaterializer::new(self.p().clone(), admitter)
    }

    /// One materializer iteration: claim a page and materialize every occurrence in it.
    async fn drain<A: WorkflowAdmitter>(
        &self,
        materializer: &ScheduleMaterializer<PostgresPersistence, A>,
    ) -> Vec<Materialized> {
        let worker = ScheduleWorkerId(Uuid::new_v4());
        let mut outcomes = Vec::new();
        for occurrence in materializer.claim(worker).await.unwrap() {
            outcomes.push(materializer.materialize(worker, &occurrence).await.unwrap());
        }
        outcomes
    }

    /// `(id, status, run, refusal, attempts)` of every occurrence of `schedule`, oldest first.
    async fn occurrences(
        &self,
        schedule: Uuid,
    ) -> Vec<(Uuid, String, Option<Uuid>, Option<String>, i32)> {
        sqlx::query_as(
            "SELECT id, materialization_status, run_id, refusal, materialization_attempts \
             FROM schedule_runs WHERE schedule_id = $1 ORDER BY created_at, id",
        )
        .bind(schedule)
        .fetch_all(self.pool())
        .await
        .unwrap()
    }

    /// Runs admitted for `occurrence`, the trigger a schedule admission names.
    async fn runs_for(&self, occurrence: Uuid) -> Vec<Uuid> {
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id = $1 AND trigger_id = $2")
            .bind(self.company())
            .bind(occurrence)
            .fetch_all(self.pool())
            .await
            .unwrap()
    }

    /// Make every pending occurrence of `schedule` due now, as if its backoff had passed.
    async fn retry_now(&self, schedule: Uuid) {
        sqlx::query(
            "UPDATE schedule_runs SET materialization_available_at = clock_timestamp() \
             WHERE schedule_id = $1 AND materialization_status = 'pending'",
        )
        .bind(schedule)
        .execute(self.pool())
        .await
        .unwrap();
    }

    /// A second active company binding, of a new workflow that accepts the schedule input.
    async fn another_binding(&self) -> Uuid {
        let version = publish(self.p(), self.company(), self.owner(), source(None)).await;
        bind(
            self.p(),
            self.company(),
            self.owner(),
            version,
            RelatedAssociation::Company,
            None,
        )
        .await
        .as_uuid()
    }

    async fn last_error(&self, schedule: Uuid) -> Option<String> {
        sqlx::query_scalar("SELECT last_error FROM channel_schedules WHERE id = $1")
            .bind(schedule)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    async fn set_active(&self, binding: Uuid, active: bool) {
        let bindings = BindingService::new(
            LifecycleAuthorizer::new(self.p().clone(), self.p().clone(), self.p().clone()),
            self.p().clone(),
            self.p().clone(),
            self.p().clone(),
        );
        let state = self
            .p()
            .binding(self.fixture.target.company, WorkflowBindingId::new(binding))
            .await
            .unwrap()
            .unwrap();
        let activity = SetBindingActivity {
            target: BindingTarget {
                company: self.fixture.target.company,
                actor: self.fixture.target.actor,
                binding: WorkflowBindingId::new(binding),
            },
            expected: state.revision,
        };
        if active {
            bindings.activate(activity).await.unwrap();
        } else {
            bindings.deactivate(activity).await.unwrap();
        }
    }
}

async fn channel(fixture: &Fixture, slug: &str) -> Uuid {
    ChannelPersistence::create(
        &fixture.persistence,
        fixture.target.company.as_uuid(),
        ChannelWrite {
            name: slug.to_string(),
            slug: slug.to_string(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap()
    .id
}

/// A company member with `role` and a principal.
async fn member(fixture: &Fixture, role: &str) -> Uuid {
    let user = Uuid::new_v4();
    let pool = fixture.persistence.pool();
    let company = fixture.target.company.as_uuid();
    sqlx::query("INSERT INTO users(id,username,email,password_hash) VALUES($1,$2,$3,'test')")
        .bind(user)
        .bind(user.simple().to_string())
        .bind(format!("{}@example.test", user.simple()))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(company)
        .bind(user)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'member')")
        .bind(Uuid::new_v4()).bind(company).bind(user).execute(pool).await.unwrap();
    user
}

async fn allowlist(pool: &sqlx::PgPool, channel: Uuid) {
    sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE id = $1")
        .bind(channel)
        .execute(pool)
        .await
        .unwrap();
}

fn constraint(error: sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.constraint()),
        Some(expected),
        "{error}"
    );
}

/// F011.4: two materializers racing for the same due slot admit one run between them.
#[tokio::test]
async fn schedule_slot_is_admitted_once_under_competing_materializers() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    let slot = s.due(schedule.id).await;
    let first = s.materializer(admission(s.p()));
    let second = s.materializer(admission(s.p()));
    let (a, b) = tokio::join!(s.drain(&first), s.drain(&second));
    let admitted: Vec<_> = a
        .iter()
        .chain(&b)
        .filter(|outcome| matches!(outcome, Materialized::Admitted(_)))
        .collect();
    assert_eq!(admitted.len(), 1, "{a:?} {b:?}");

    let occurrences = s.occurrences(schedule.id).await;
    assert_eq!(occurrences.len(), 1, "one occurrence for the slot");
    let (occurrence, status, run, refusal, attempts) = occurrences[0].clone();
    assert_eq!(
        (status.as_str(), refusal, attempts),
        ("materialized", None, 1)
    );
    assert_eq!(s.runs_for(occurrence).await, vec![run.unwrap()]);
    let (scheduled_for, input): (chrono::DateTime<Utc>, Value) = sqlx::query_as(
        "SELECT occurrence.scheduled_for, run.input FROM schedule_runs AS occurrence \
         JOIN workflow_runs AS run ON run.company_id = occurrence.company_id AND run.id = occurrence.run_id \
         WHERE occurrence.id = $1",
    )
    .bind(occurrence)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(scheduled_for, slot);
    assert_eq!(
        input,
        json!({"scheduled_for": slot.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)})
    );
    // The run acts as the binding's run-as member, in the schedule's channel.
    let (actor, channel, thread): (Uuid, Option<Uuid>, Option<Uuid>) =
        sqlx::query_as("SELECT actor_id, channel_id, thread_id FROM workflow_runs WHERE id = $1")
            .bind(run.unwrap())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!((actor, channel, thread), (s.owner(), Some(s.channel), None));

    // Nothing else is due: the cadence moved on from the slot.
    assert!(s.drain(&first).await.is_empty());
    let next: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT next_run_at FROM channel_schedules WHERE id = $1")
            .bind(schedule.id)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert!(next > Utc::now());
}

/// F011.4: admission records the run and occurrence before a worker can crash.
#[tokio::test]
async fn schedule_occurrence_admitted_before_a_crash_replays_its_run() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    s.due(schedule.id).await;
    let crashing = s.materializer(CrashAfterAdmission(admission(s.p())));
    assert_eq!(s.drain(&crashing).await, vec![Materialized::LeaseLost]);
    let (occurrence, status, run, _, attempts) = s.occurrences(schedule.id).await[0].clone();
    assert_eq!(
        (status.as_str(), run, attempts),
        ("materialized", Some(s.runs_for(occurrence).await[0]), 1)
    );
    let admitted = s.runs_for(occurrence).await;
    assert_eq!(
        admitted.len(),
        1,
        "the crashed attempt's admission committed"
    );
    assert_eq!(s.last_error(schedule.id).await, None);

    sqlx::query(
        "UPDATE schedule_runs SET materialization_available_at = clock_timestamp() WHERE id = $1",
    )
    .bind(occurrence)
    .execute(s.pool())
    .await
    .unwrap();
    let retry = s.materializer(admission(s.p()));
    assert!(s.drain(&retry).await.is_empty());
    assert_eq!(s.runs_for(occurrence).await, admitted, "no second run");
    let (_, status, run, _, attempts) = s.occurrences(schedule.id).await[0].clone();
    assert_eq!(
        (status.as_str(), run, attempts),
        ("materialized", Some(admitted[0]), 1)
    );
    assert_eq!(
        s.last_error(schedule.id).await,
        None,
        "a recorded run clears the error"
    );
}

/// F011.4: a refused occurrence fails with its refusal, which the schedule names; a later run
/// clears it.
#[tokio::test]
async fn schedule_refusal_fails_the_occurrence_and_names_it_on_the_schedule() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    s.due(schedule.id).await;
    s.set_active(s.binding, false).await;
    let materializer = s.materializer(admission(s.p()));
    assert_eq!(
        s.drain(&materializer).await,
        vec![Materialized::Refused("unavailable")]
    );
    let (occurrence, status, run, refusal, attempts) = s.occurrences(schedule.id).await[0].clone();
    assert_eq!(
        (status.as_str(), run, refusal.as_deref(), attempts),
        ("failed", None, Some("unavailable"), 1)
    );
    assert!(s.runs_for(occurrence).await.is_empty());
    assert!(
        s.last_error(schedule.id)
            .await
            .unwrap()
            .contains("unavailable")
    );
    let enabled: bool = sqlx::query_scalar("SELECT enabled FROM channel_schedules WHERE id = $1")
        .bind(schedule.id)
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert!(enabled, "a refusal does not pause the schedule");

    s.set_active(s.binding, true).await;
    let run = s
        .use_cases
        .trigger_schedule_now(s.owner(), s.company(), schedule.id, "after-reactivation")
        .await
        .unwrap();
    assert_eq!(run.status, OccurrenceStatus::Materialized);
    assert_eq!(s.last_error(schedule.id).await, None);
}

/// F011.4: run-now captures one occurrence per command key and returns its run on a repeat.
#[tokio::test]
async fn schedule_run_now_replays_its_command_key() {
    let s = Schedules::new().await;
    let schedule = s.schedule("On demand").await;
    let first = s
        .use_cases
        .trigger_schedule_now(s.owner(), s.company(), schedule.id, "click-1")
        .await
        .unwrap();
    assert!(first.manual);
    assert_eq!(first.status, OccurrenceStatus::Materialized);
    let run = first.run_id.unwrap();
    let again = s
        .use_cases
        .trigger_schedule_now(s.owner(), s.company(), schedule.id, "click-1")
        .await
        .unwrap();
    assert_eq!((again.id, again.run_id), (first.id, Some(run)));
    let other = s
        .use_cases
        .trigger_schedule_now(s.owner(), s.company(), schedule.id, "click-2")
        .await
        .unwrap();
    assert_ne!(other.id, first.id);
    assert_ne!(other.run_id, Some(run));
    assert_eq!(s.occurrences(schedule.id).await.len(), 2);
    // A run-now occurrence never takes the cadence's slot: a slot due at the same instant is
    // still captured.
    sqlx::query("UPDATE channel_schedules SET next_run_at = (SELECT scheduled_for FROM schedule_runs WHERE id = $2) WHERE id = $1")
        .bind(schedule.id)
        .bind(first.id)
        .execute(s.pool())
        .await
        .unwrap();
    let outcomes = s.drain(&s.materializer(admission(s.p()))).await;
    assert!(
        matches!(outcomes[..], [Materialized::Admitted(_)]),
        "{outcomes:?}"
    );
    assert_eq!(s.occurrences(schedule.id).await.len(), 3);
    let listed = s
        .use_cases
        .list_schedule_runs(s.owner(), s.company(), schedule.id, 0, 10)
        .await
        .unwrap();
    assert_eq!(listed.len(), 3);
    assert_eq!(listed.iter().filter(|run| run.manual).count(), 2);
    assert!(listed.iter().all(|run| run.run_state.is_some()));
    for key in ["", &"k".repeat(201)] {
        assert!(matches!(
            s.use_cases
                .trigger_schedule_now(s.owner(), s.company(), schedule.id, key)
                .await,
            Err(AppError::BadRequest(_))
        ));
    }
}

/// Every outcome write names the claim it belongs to; a lost claim changes nothing.
#[tokio::test]
async fn schedule_outcome_writes_are_fenced_by_the_claim() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    s.due(schedule.id).await;
    let worker = ScheduleWorkerId(Uuid::new_v4());
    let lease = Utc::now() + chrono::Duration::minutes(1);
    let claimed = s.p().claim_due(worker, lease, 10).await.unwrap();
    assert_eq!(claimed.len(), 1);
    let claim = OccurrenceClaim {
        company: s.fixture.target.company,
        occurrence: claimed[0].id,
        worker,
        generation: claimed[0].materialization_generation,
    };
    let run = RunId::new(Uuid::new_v4());
    for stale in [
        OccurrenceClaim {
            worker: ScheduleWorkerId(Uuid::new_v4()),
            ..claim
        },
        OccurrenceClaim {
            generation: Uuid::new_v4(),
            ..claim
        },
        OccurrenceClaim {
            company: CompanyId::new(Uuid::new_v4()),
            ..claim
        },
    ] {
        assert!(!s.p().record_run(stale, run).await.unwrap());
        assert!(!s.p().record_refusal(stale, "unavailable").await.unwrap());
        assert_eq!(
            s.p().record_error(stale, "lost").await.unwrap(),
            ErrorRecorded::Lost
        );
    }
    // The lease lapses and another worker reclaims: the first claim is gone.
    sqlx::query("UPDATE schedule_runs SET materialization_locked_at = clock_timestamp() - interval '2 minutes', materialization_lock_expires_at = clock_timestamp() - interval '1 second' WHERE id = $1")
        .bind(claim.occurrence)
        .execute(s.pool())
        .await
        .unwrap();
    let next = ScheduleWorkerId(Uuid::new_v4());
    let reclaimed = s
        .p()
        .claim_occurrence(claim.company, claim.occurrence, next, lease)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        s.p().record_error(claim, "late").await.unwrap(),
        ErrorRecorded::Lost
    );
    let current = OccurrenceClaim {
        worker: next,
        generation: reclaimed.materialization_generation,
        ..claim
    };
    assert_eq!(
        s.p().record_error(current, "transient").await.unwrap(),
        ErrorRecorded::Retrying
    );
    let (_, status, _, _, attempts) = s.occurrences(schedule.id).await[0].clone();
    assert_eq!((status.as_str(), attempts), ("pending", 2));
    let reclaimed = s
        .p()
        .claim_occurrence(claim.company, claim.occurrence, next, lease)
        .await
        .unwrap();
    assert!(reclaimed.is_none(), "backoff keeps it unclaimable");
}

/// D1.9 authority: owner or admin who can see the schedule's channel, for every operation.
#[tokio::test]
async fn schedule_management_requires_an_owner_or_admin_who_sees_the_channel() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    let plain = member(&s.fixture, "member").await;
    let admin = member(&s.fixture, "admin").await;
    let refused =
        |result: AppResult<()>| assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
    let all = async |user: Uuid| -> Vec<AppResult<()>> {
        let uc = &s.use_cases;
        vec![
            uc.create_schedule(user, s.company(), s.channel, s.write("New", s.binding))
                .await
                .map(drop),
            uc.update_schedule(
                user,
                s.company(),
                schedule.id,
                s.channel,
                s.write("Edited", s.binding),
            )
            .await
            .map(drop),
            uc.toggle_schedule(user, s.company(), schedule.id, false)
                .await,
            uc.trigger_schedule_now(user, s.company(), schedule.id, "k")
                .await
                .map(drop),
            uc.list_schedule_runs(user, s.company(), schedule.id, 0, 10)
                .await
                .map(drop),
            uc.list_channel_schedules(user, s.company(), s.channel)
                .await
                .map(drop),
            uc.delete_schedule(user, s.company(), schedule.id).await,
        ]
    };
    for result in all(plain).await {
        refused(result);
    }
    assert_eq!(
        s.use_cases
            .get_schedule(plain, s.company(), schedule.id)
            .await
            .unwrap(),
        None
    );
    // An admin who cannot see an allowlisted channel cannot manage its schedules either.
    allowlist(s.pool(), s.channel).await;
    for result in all(admin).await {
        refused(result);
    }
    assert!(
        s.use_cases
            .list_company_schedules(admin, s.company())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.occurrences(schedule.id).await.len(), 0);
    // Persistence rechecks under the company lock: a write that skipped the preflight is refused
    // just the same.
    let actor = WorkflowActor::authenticated(admin).unwrap();
    assert!(matches!(
        s.p()
            .set_enabled(actor, s.fixture.target.company, schedule.id, false)
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        s.p()
            .capture_now(actor, s.fixture.target.company, schedule.id, "k")
            .await,
        Err(AppError::NotFound(_))
    ));
    // The owner sees every channel.
    assert_eq!(
        s.use_cases
            .list_company_schedules(s.owner(), s.company())
            .await
            .unwrap()
            .len(),
        1
    );
}

/// D1.9 compatibility: the binding must be the company's or the channel's, accept the schedule
/// input, and run as someone who sees the channel.
#[tokio::test]
async fn schedule_binding_must_fit_the_schedule_channel() {
    let s = Schedules::new().await;
    let p = s.p();
    let company = s.company();
    let owner = s.owner();
    let other_channel = channel(&s.fixture, "elsewhere").await;
    let version = publish(p, company, owner, source(None)).await;
    let elsewhere = bind(
        p,
        company,
        owner,
        version,
        RelatedAssociation::Channel(RelatedChannelId::new(other_channel)),
        None,
    )
    .await
    .as_uuid();
    let narrow = publish(
        p,
        company,
        owner,
        source(Some(
            json!({"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}),
        )),
    )
    .await;
    let rejecting = bind(p, company, owner, narrow, RelatedAssociation::Company, None)
        .await
        .as_uuid();
    let admin = member(&s.fixture, "admin").await;
    let admins = bind(
        p,
        company,
        admin,
        version,
        RelatedAssociation::Company,
        None,
    )
    .await
    .as_uuid();
    let this_channel = bind(
        p,
        company,
        owner,
        version,
        RelatedAssociation::Channel(RelatedChannelId::new(s.channel)),
        None,
    )
    .await
    .as_uuid();
    allowlist(s.pool(), s.channel).await;
    for (binding, reason) in [
        (elsewhere, "neither the company nor"),
        (rejecting, "does not accept the schedule input"),
        (admins, "run-as member cannot see"),
        (Uuid::new_v4(), "does not exist"),
    ] {
        let error = s
            .use_cases
            .create_schedule(owner, company, s.channel, s.write("Refused", binding))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, AppError::BadRequest(message) if message.contains(reason)),
            "{error:?}"
        );
    }
    for binding in [s.binding, this_channel] {
        s.use_cases
            .create_schedule(owner, company, s.channel, s.write("Fits", binding))
            .await
            .unwrap();
    }
    let choices = s
        .use_cases
        .binding_choices(owner, company, Some(s.channel))
        .await
        .unwrap();
    let offered: Vec<Uuid> = choices.iter().map(|choice| choice.binding_id).collect();
    assert!(offered.contains(&this_channel) && offered.contains(&s.binding));
    assert!(
        !offered.contains(&elsewhere),
        "another channel's binding is not offered"
    );
}

/// D1.9 revalidation: reconfiguring a binding a schedule uses is refused, naming the schedule,
/// when the new configuration could not drive it.
#[tokio::test]
async fn binding_reconfiguration_is_refused_by_an_incompatible_schedule() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Nightly digest").await;
    let p = s.p();
    let narrow = publish(
        p,
        s.company(),
        s.owner(),
        source(Some(
            json!({"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}),
        )),
    )
    .await;
    let bindings = BindingService::new(
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        p.clone(),
        p.clone(),
    );
    let target = BindingTarget {
        company: s.fixture.target.company,
        actor: s.fixture.target.actor,
        binding: WorkflowBindingId::new(s.binding),
    };
    let current = p
        .binding(target.company, target.binding)
        .await
        .unwrap()
        .unwrap();
    let reconfigure = |version: VersionId, run_as: Option<Uuid>| ConfigureBinding {
        target,
        expected: Some(current.revision),
        association: RelatedAssociation::Company,
        version,
        params: json!({}),
        resources: Default::default(),
        run_as: run_as.map(|user| WorkflowActor::authenticated(user).unwrap()),
        message_trigger: None,
    };
    let error = bindings
        .configure(reconfigure(narrow, None))
        .await
        .err()
        .expect("an input schema the schedule cannot satisfy is refused");
    assert!(format!("{error:?}").contains("Nightly digest"), "{error:?}");
    // A run-as who cannot see the schedule's channel is refused the same way.
    let admin = member(&s.fixture, "admin").await;
    allowlist(s.pool(), s.channel).await;
    let version = current
        .configuration
        .bundle()
        .compiled()
        .graph()
        .definition()
        .version_id;
    let error = bindings
        .configure(reconfigure(version, Some(admin)))
        .await
        .err()
        .expect("a run-as who cannot see the channel is refused");
    assert!(format!("{error:?}").contains("cannot see"), "{error:?}");
    let after = p
        .binding(target.company, target.binding)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, current.revision, "nothing changed");
    // Without the schedule the same reconfiguration is fine.
    s.use_cases
        .delete_schedule(s.owner(), s.company(), schedule.id)
        .await
        .unwrap();
    bindings.configure(reconfigure(narrow, None)).await.unwrap();
}

/// The claim steps from the slot that was due, so a run cannot drift later than its slot and a
/// backlog after downtime collapses into the next slot instead of replaying.
#[tokio::test]
async fn claiming_anchors_the_next_run_on_the_slot_not_the_clock() {
    let s = Schedules::new().await;
    let hourly = s.schedule("Hourly").await;
    // Three and a half hours ago: three runs were missed.
    let missed_slot = (Utc::now() - chrono::Duration::minutes(210)).trunc_subsecs(6);
    sqlx::query("UPDATE channel_schedules SET next_run_at = $1 WHERE id = $2")
        .bind(missed_slot)
        .bind(hourly.id)
        .execute(s.pool())
        .await
        .unwrap();
    let claimed = s
        .p()
        .claim_due(
            ScheduleWorkerId(Uuid::new_v4()),
            Utc::now() + chrono::Duration::minutes(1),
            10,
        )
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1, "one slot for the whole backlog");
    assert_eq!(claimed[0].scheduled_for, missed_slot);
    assert_eq!(claimed[0].binding_id, s.binding);
    let next = SchedulePersistence::get(s.p(), s.fixture.target.company, hourly.id)
        .await
        .unwrap()
        .unwrap()
        .next_run_at
        .unwrap();
    assert!(next > Utc::now());
    assert!(next - Utc::now() < chrono::Duration::minutes(35));
    assert_eq!(
        (next - missed_slot).num_microseconds().unwrap() % 3_600_000_000,
        0,
        "the next run stays on the original slot's rhythm"
    );
}

/// What the schema refuses on its own: occurrence scope at capture, its identity afterwards,
/// a snapshot naming another scope, a run that is not this occurrence's, and the state shape.
#[tokio::test]
async fn schedule_occurrence_schema_keeps_scope_identity_and_run() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    let other = s.schedule("Other").await;
    let mut tx = s.pool().begin().await.unwrap();
    let occurrence = Uuid::new_v4();
    sqlx::query("INSERT INTO schedule_runs (id, schedule_id, scheduled_for, schedule_snapshot) VALUES ($1, $2, clock_timestamp(), '{}')")
        .bind(occurrence)
        .bind(schedule.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let (company, channel, binding): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT company_id, channel_id, binding_id FROM schedule_runs WHERE id = $1",
    )
    .bind(occurrence)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        (company, channel, binding),
        (s.company(), s.channel, s.binding),
        "captured from its schedule"
    );

    async fn refused(tx: &mut sqlx::PgConnection, sql: &str, bind: Uuid, expected: &str) {
        sqlx::query("SAVEPOINT refused")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query(sql)
            .bind(bind)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        constraint(error, expected);
        sqlx::query("ROLLBACK TO SAVEPOINT refused")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let elsewhere = channel_of(&s).await;
    for (column, value) in [
        ("company_id", Uuid::new_v4()),
        ("channel_id", elsewhere),
        ("binding_id", Uuid::new_v4()),
    ] {
        refused(
            &mut tx,
            &format!("INSERT INTO schedule_runs (id, schedule_id, scheduled_for, schedule_snapshot, {column}) VALUES (gen_random_uuid(), '{}', clock_timestamp(), '{{}}', $1)", schedule.id),
            value,
            "schedule_occurrence_owner_scope",
        )
        .await;
    }
    for set in [
        "binding_id = $1",
        "schedule_id = $1",
        "command_key = $1::text",
        "scheduled_for = scheduled_for + interval '1 second' WHERE $1 IS NOT NULL",
    ] {
        let value = if set.starts_with("schedule_id") {
            other.id
        } else {
            Uuid::new_v4()
        };
        let sql = if set.contains("WHERE") {
            format!("UPDATE schedule_runs SET {set} AND id = '{occurrence}'")
        } else {
            format!("UPDATE schedule_runs SET {set} WHERE id = '{occurrence}'")
        };
        refused(
            &mut tx,
            &sql,
            value,
            "schedule_occurrence_identity_immutable",
        )
        .await;
    }
    refused(
        &mut tx,
        &format!("INSERT INTO schedule_runs (id, schedule_id, scheduled_for, schedule_snapshot) VALUES (gen_random_uuid(), '{}', clock_timestamp(), jsonb_build_object('binding_id', $1::text))", schedule.id),
        Uuid::new_v4(),
        "schedule_occurrence_snapshot_scope",
    )
    .await;
    // A run of another trigger, or of another binding, is not this occurrence's run.
    let manual = manual::start(
        &admission(s.p()),
        StartRun {
            company_id: s.fixture.target.company,
            actor: s.fixture.target.actor,
            binding_id: WorkflowBindingId::new(s.binding),
            association: RelatedAssociation::Channel(RelatedChannelId::new(s.channel)),
            input: json!({}),
            idempotency_key: "schema".into(),
            rerun_of: None,
        },
    )
    .await
    .unwrap()
    .run;
    refused(
        &mut tx,
        &format!("UPDATE schedule_runs SET run_id = $1, materialization_status = 'materialized' WHERE id = '{occurrence}'"),
        manual.as_uuid(),
        "schedule_occurrence_run_scope",
    )
    .await;
    sqlx::query("SAVEPOINT refused")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query(&format!("UPDATE schedule_runs SET run_id = $1, materialization_status = 'materialized' WHERE id = '{occurrence}'"))
        .bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert!(error.as_database_error().is_some());
    sqlx::query("ROLLBACK TO SAVEPOINT refused")
        .execute(&mut *tx)
        .await
        .unwrap();
    // A refusal only ever ends an occurrence.
    refused(
        &mut tx,
        &format!("UPDATE schedule_runs SET refusal = 'unavailable' WHERE id = '{occurrence}' AND $1 IS NOT NULL"),
        Uuid::new_v4(),
        "schedule_runs_materialization_state_check",
    )
    .await;
    refused(
        &mut tx,
        &format!("UPDATE schedule_runs SET refusal = 'made_up', materialization_status = 'failed' WHERE id = '{occurrence}' AND $1 IS NOT NULL"),
        Uuid::new_v4(),
        "schedule_runs_refusal_check",
    )
    .await;
    tx.rollback().await.unwrap();
}

async fn channel_of(s: &Schedules) -> Uuid {
    channel(&s.fixture, &format!("c{}", Uuid::new_v4().simple())).await
}

/// A schedule's channel cannot move, even while an occurrence is being captured from it.
#[tokio::test]
async fn schedule_capture_serializes_with_a_rejected_channel_move() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    let moved = channel_of(&s).await;
    let mut capture = s.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO schedule_runs (id, schedule_id, scheduled_for, schedule_snapshot) VALUES (gen_random_uuid(), $1, clock_timestamp(), '{}')")
        .bind(schedule.id)
        .execute(&mut *capture)
        .await
        .unwrap();
    let mut change = s.pool().begin().await.unwrap();
    let id = schedule.id;
    let mutation = tokio::spawn(async move {
        let result = sqlx::query(
            "UPDATE channel_schedules SET channel_id = $2, name = 'Rejected edit' WHERE id = $1",
        )
        .bind(id)
        .bind(moved)
        .execute(&mut *change)
        .await;
        change.rollback().await.unwrap();
        result
    });
    crate::adapters::persistence::test_support::wait_until_a_backend_is_blocked(s.pool()).await;
    capture.commit().await.unwrap();
    let error = tokio::time::timeout(std::time::Duration::from_secs(5), mutation)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    constraint(error, "schedule_channel_immutable");
    let captured: Uuid =
        sqlx::query_scalar("SELECT channel_id FROM schedule_runs WHERE schedule_id = $1")
            .bind(schedule.id)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(captured, s.channel);
}

/// Manual starts (WF-C001): the caller's authority, a client key that replays, a conflict for a
/// reused key, and a rerun link that inspection shows.
#[tokio::test]
async fn manual_start_returns_its_run_replays_its_key_and_links_a_rerun() {
    let s = Schedules::new().await;
    let service = admission(s.p());
    let start = |key: &str, input: Value, actor: Uuid, rerun_of: Option<RunId>| StartRun {
        company_id: s.fixture.target.company,
        actor: WorkflowActor::authenticated(actor).unwrap(),
        binding_id: WorkflowBindingId::new(s.binding),
        association: RelatedAssociation::Company,
        input,
        idempotency_key: key.into(),
        rerun_of,
    };
    let first = manual::start(&service, start("start-1", json!({"n": 1}), s.owner(), None))
        .await
        .unwrap();
    assert!(!first.replayed);
    let again = manual::start(&service, start("start-1", json!({"n": 1}), s.owner(), None))
        .await
        .unwrap();
    assert_eq!((again.run, again.replayed), (first.run, true));
    assert!(matches!(
        manual::start(&service, start("start-1", json!({"n": 2}), s.owner(), None)).await,
        Err(AppError::Conflict(_))
    ));
    let rerun = manual::start(
        &service,
        start("rerun-1", json!({"n": 2}), s.owner(), Some(first.run)),
    )
    .await
    .unwrap();
    let head = s
        .p()
        .head(s.fixture.target.company, rerun.run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(head.rerun_of, Some(first.run));
    assert!(matches!(
        head.causality.trigger().source(),
        TriggerSource::Manual
    ));
    // A client key cannot claim an automatic admission's key space.
    let schedule = s.schedule("Hourly").await;
    s.due(schedule.id).await;
    let occurrence = {
        s.drain(&s.materializer(admission(s.p()))).await;
        s.occurrences(schedule.id).await[0].0
    };
    let squatter = manual::start(
        &service,
        start(&format!("s:{occurrence}"), json!({}), s.owner(), None),
    )
    .await
    .unwrap();
    assert_ne!(
        Some(squatter.run.as_uuid()),
        s.occurrences(schedule.id).await[0].2
    );
    // Only an owner or admin may start a run.
    let plain = member(&s.fixture, "member").await;
    assert!(matches!(
        manual::start(&service, start("start-2", json!({}), plain, None)).await,
        Err(AppError::NotFound(_))
    ));
}

/// F032 (`src/application/AGENTS.md` poison batch): a full page of failing occurrences is moved
/// out of the claimable set, so the next iteration, without time advancing, claims none of them.
#[tokio::test]
async fn schedule_poison_batch_is_not_reclaimed_by_the_next_iteration() {
    let s = Schedules::new().await;
    let mut schedules = Vec::new();
    for n in 0..SCHEDULE_PAGE {
        let schedule = s.schedule(&format!("Hourly {n}")).await;
        s.due(schedule.id).await;
        schedules.push(schedule.id);
    }
    let failing = s.materializer(Outage);
    let first = s.drain(&failing).await;
    assert_eq!(first.len(), SCHEDULE_PAGE as usize, "a full page");
    assert!(
        first
            .iter()
            .all(|outcome| *outcome == Materialized::Deferred)
    );
    let second = s.drain(&failing).await;
    assert!(
        second.is_empty(),
        "the failed page is not reclaimed: {second:?}"
    );
    for schedule in schedules {
        let (_, status, _, _, attempts) = s.occurrences(schedule).await[0].clone();
        assert_eq!((status.as_str(), attempts), ("pending", 1));
    }
}

/// F032: an edit that leaves the cadence alone keeps the next run and does not resume a paused
/// schedule; a changed cadence re-anchors it and still does not resume it.
#[tokio::test]
async fn schedule_edit_keeps_its_next_run_and_pause_unless_the_cadence_changes() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    s.use_cases
        .toggle_schedule(s.owner(), s.company(), schedule.id, false)
        .await
        .unwrap();
    let renamed = s
        .use_cases
        .update_schedule(
            s.owner(),
            s.company(),
            schedule.id,
            s.channel,
            s.write("Renamed", s.binding),
        )
        .await
        .unwrap();
    assert_eq!(renamed.name, "Renamed");
    assert_eq!(renamed.next_run_at, schedule.next_run_at);
    assert!(
        !renamed.enabled,
        "an edit does not resume a paused schedule"
    );
    let mut daily = s.write("Renamed", s.binding);
    daily.interval_seconds = Some(86_400);
    let rescheduled = s
        .use_cases
        .update_schedule(s.owner(), s.company(), schedule.id, s.channel, daily)
        .await
        .unwrap();
    assert!(rescheduled.next_run_at.unwrap() > schedule.next_run_at.unwrap());
    assert!(!rescheduled.enabled);
}

/// F032: capturing a one-off's slot ends the schedule.
#[tokio::test]
async fn a_captured_one_off_schedule_is_disabled() {
    let s = Schedules::new().await;
    let mut write = s.write("Once", s.binding);
    write.schedule_type = ScheduleType::OneOff;
    write.interval_seconds = None;
    write.scheduled_at = Some(Utc::now() + chrono::Duration::hours(1));
    let once = s
        .use_cases
        .create_schedule(s.owner(), s.company(), s.channel, write)
        .await
        .unwrap();
    s.due(once.id).await;
    let outcomes = s.drain(&s.materializer(admission(s.p()))).await;
    assert!(
        matches!(outcomes[..], [Materialized::Admitted(_)]),
        "{outcomes:?}"
    );
    let after = SchedulePersistence::get(s.p(), s.fixture.target.company, once.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((after.enabled, after.next_run_at), (false, None));
    assert!(after.last_run_at.is_some());
    assert!(s.drain(&s.materializer(admission(s.p()))).await.is_empty());
}

/// F032 (B2-g): an occurrence admits under the binding captured with it, even after the
/// schedule was retargeted; the schedule's next slot uses the new binding.
#[tokio::test]
async fn retargeting_a_schedule_after_capture_keeps_the_captured_binding() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    s.due(schedule.id).await;
    assert_eq!(
        s.drain(&s.materializer(Outage)).await,
        vec![Materialized::Deferred]
    );
    let captured = s.occurrences(schedule.id).await[0].0;

    let retarget = s.another_binding().await;
    s.use_cases
        .update_schedule(
            s.owner(),
            s.company(),
            schedule.id,
            s.channel,
            s.write("Hourly", retarget),
        )
        .await
        .unwrap();
    s.retry_now(schedule.id).await;
    let admitted = s.drain(&s.materializer(admission(s.p()))).await;
    assert!(
        matches!(admitted[..], [Materialized::Admitted(_)]),
        "{admitted:?}"
    );
    let binding_of = async |occurrence: Uuid| -> Uuid {
        sqlx::query_scalar(
            "SELECT run.binding_id FROM schedule_runs AS occurrence \
             JOIN workflow_runs AS run ON run.company_id = occurrence.company_id AND run.id = occurrence.run_id \
             WHERE occurrence.id = $1",
        )
        .bind(occurrence)
        .fetch_one(s.pool())
        .await
        .unwrap()
    };
    assert_eq!(
        binding_of(captured).await,
        s.binding,
        "the captured binding"
    );

    s.due(schedule.id).await;
    s.drain(&s.materializer(admission(s.p()))).await;
    let occurrences = s.occurrences(schedule.id).await;
    assert_eq!(occurrences.len(), 2);
    let next = occurrences.iter().find(|row| row.0 != captured).unwrap().0;
    assert_eq!(
        binding_of(next).await,
        retarget,
        "the next slot uses the new binding"
    );
}

/// F032: the outcome fence is the claim's worker and generation. A claim whose lease lapsed but
/// that nobody reclaimed is still the latest claim and records what it did; the terminal sweep
/// takes only occurrences on their last attempt.
#[tokio::test]
async fn a_lapsed_but_unreclaimed_claim_still_records_its_outcome() {
    let s = Schedules::new().await;
    let schedule = s.schedule("Hourly").await;
    s.due(schedule.id).await;
    let worker = ScheduleWorkerId(Uuid::new_v4());
    let claimed = s
        .p()
        .claim_due(worker, Utc::now() + chrono::Duration::minutes(1), 10)
        .await
        .unwrap();
    sqlx::query("UPDATE schedule_runs SET materialization_locked_at = clock_timestamp() - interval '2 minutes', materialization_lock_expires_at = clock_timestamp() - interval '1 second' WHERE id = $1")
        .bind(claimed[0].id)
        .execute(s.pool())
        .await
        .unwrap();
    let claim = OccurrenceClaim {
        company: s.fixture.target.company,
        occurrence: claimed[0].id,
        worker,
        generation: claimed[0].materialization_generation,
    };
    assert!(s.p().record_refusal(claim, "unavailable").await.unwrap());
    let (_, status, _, refusal, _) = s.occurrences(schedule.id).await[0].clone();
    assert_eq!(
        (status.as_str(), refusal.as_deref()),
        ("failed", Some("unavailable"))
    );
}

/// F033: every way an occurrence ends without a run is counted and logged with its cause, and
/// the schedule names it; a lost claim is counted too.
#[tokio::test]
async fn schedule_terminal_failures_and_lost_claims_are_observable() {
    let s = Schedules::new().await;
    let monitor = Arc::new(InMemoryMonitor::new());
    let counter = |key: &str| monitor.get_stats_json()["custom_counters"][key].clone();

    // The last attempt's lease lapses: the sweep fails it and names it on the schedule.
    let lapsed = s.schedule("Lapsed").await;
    s.due(lapsed.id).await;
    let worker = ScheduleWorkerId(Uuid::new_v4());
    s.p()
        .claim_due(worker, Utc::now() + chrono::Duration::minutes(1), 10)
        .await
        .unwrap();
    sqlx::query("UPDATE schedule_runs SET materialization_attempts = 5, materialization_locked_at = clock_timestamp() - interval '2 minutes', materialization_lock_expires_at = clock_timestamp() - interval '1 second' WHERE schedule_id = $1")
        .bind(lapsed.id)
        .execute(s.pool())
        .await
        .unwrap();
    let materializer = s.materializer(Outage).with_monitoring(monitor.clone());
    assert!(materializer.claim(worker).await.unwrap().is_empty());
    let (_, status, _, _, attempts) = s.occurrences(lapsed.id).await[0].clone();
    assert_eq!((status.as_str(), attempts), ("failed", 5));
    assert!(
        s.last_error(lapsed.id)
            .await
            .unwrap()
            .contains("lease expired")
    );
    assert_eq!(
        counter("workflow_schedule_occurrences_failed_total{cause=lease_expired}"),
        1
    );

    // The fifth transient failure ends the occurrence.
    let flaky = s.schedule("Flaky").await;
    s.due(flaky.id).await;
    for attempt in 1..=5 {
        let outcomes = s.drain(&materializer).await;
        let expected = if attempt < 5 {
            Materialized::Deferred
        } else {
            Materialized::Failed
        };
        assert_eq!(outcomes, vec![expected], "attempt {attempt}");
        s.retry_now(flaky.id).await;
    }
    let (_, status, _, refusal, attempts) = s.occurrences(flaky.id).await[0].clone();
    assert_eq!((status.as_str(), refusal, attempts), ("failed", None, 5));
    assert!(s.last_error(flaky.id).await.unwrap().contains("outage"));
    assert_eq!(
        counter("workflow_schedule_occurrences_failed_total{cause=transient}"),
        1
    );

    // A claim reclaimed by another worker before its outcome was written is lost, and counted.
    let contested = s.schedule("Contested").await;
    s.due(contested.id).await;
    let first = ScheduleWorkerId(Uuid::new_v4());
    let claimed = s
        .p()
        .claim_due(first, Utc::now() + chrono::Duration::minutes(1), 10)
        .await
        .unwrap();
    let mine = claimed
        .iter()
        .find(|row| row.schedule.id == contested.id)
        .unwrap();
    sqlx::query("UPDATE schedule_runs SET materialization_locked_at = clock_timestamp() - interval '2 minutes', materialization_lock_expires_at = clock_timestamp() - interval '1 second' WHERE id = $1")
        .bind(mine.id)
        .execute(s.pool())
        .await
        .unwrap();
    s.p()
        .claim_occurrence(
            s.fixture.target.company,
            mine.id,
            ScheduleWorkerId(Uuid::new_v4()),
            Utc::now() + chrono::Duration::minutes(1),
        )
        .await
        .unwrap()
        .unwrap();
    let real = s
        .materializer(admission(s.p()))
        .with_monitoring(monitor.clone());
    assert_eq!(
        real.materialize(first, mine).await.unwrap(),
        Materialized::LeaseLost
    );
    assert_eq!(counter("workflow_schedule_occurrence_leases_lost_total"), 1);
}

/// F035: an occurrence the sweep failed is logged and counted even when its schedule cannot be
/// updated to name the failure.
#[tokio::test]
async fn an_expired_occurrence_is_reported_when_its_schedule_cannot_name_it() {
    let s = Schedules::new().await;
    let monitor = Arc::new(InMemoryMonitor::new());
    let lapsed = s.schedule("Locked out").await;
    s.due(lapsed.id).await;
    let worker = ScheduleWorkerId(Uuid::new_v4());
    s.p()
        .claim_due(worker, Utc::now() + chrono::Duration::minutes(1), 10)
        .await
        .unwrap();
    sqlx::query("UPDATE schedule_runs SET materialization_attempts = 5, materialization_locked_at = clock_timestamp() - interval '2 minutes', materialization_lock_expires_at = clock_timestamp() - interval '1 second' WHERE schedule_id = $1")
        .bind(lapsed.id)
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("CREATE FUNCTION reject_test_schedule_update() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected schedule update failure'; END; $$")
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TRIGGER reject_schedule_update BEFORE UPDATE ON channel_schedules FOR EACH ROW \
         WHEN (OLD.id = '{}'::uuid) EXECUTE FUNCTION reject_test_schedule_update()",
        lapsed.id
    ))
    .execute(s.pool())
    .await
    .unwrap();

    let swept = s
        .materializer(Outage)
        .with_monitoring(monitor.clone())
        .claim(worker)
        .await;
    sqlx::query("DROP TRIGGER reject_schedule_update ON channel_schedules")
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION reject_test_schedule_update()")
        .execute(s.pool())
        .await
        .unwrap();

    assert!(swept.unwrap().is_empty());
    let (_, status, _, _, attempts) = s.occurrences(lapsed.id).await[0].clone();
    assert_eq!((status.as_str(), attempts), ("failed", 5));
    assert_eq!(s.last_error(lapsed.id).await, None);
    assert_eq!(
        monitor.get_stats_json()["custom_counters"]["workflow_schedule_occurrences_failed_total{cause=lease_expired}"],
        1
    );
}
