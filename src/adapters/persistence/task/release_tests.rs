//! `release_tasks_for_removed_principal()`: what happens to a principal's tasks when they go away.
//!
//! Two entry paths share one trigger body. A person's principal is demoted to `external` by team
//! removal — an `UPDATE OF kind` — and an agent's principal is deleted outright, cascaded from the
//! agent row. `background_tasks_owner_principal_fk` is `ON DELETE RESTRICT` and carries the
//! principal's `kind` with no `ON UPDATE CASCADE`, so neither path can commit unless the trigger
//! has cleared every task the principal owned first.
//!
//! These tests were written against the row-at-a-time implementation and are the contract the
//! set-based rewrite has to keep. They pin the two things a rewrite can quietly get wrong: the
//! ownership event's `sequence`, which is each task's own version plus one rather than a running
//! count, and the attempt fencing, which reaches exactly the attempts of the tasks that were
//! `processing`, on the generation those tasks were running.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::test_support::test_pool;
use crate::entities::company_member::CompanyAccessRole;
use crate::entities::creation::CreationProvenance;
use crate::entities::harness::HarnessKind;
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
use crate::use_cases::company_invite::CompanyInvitePersistence;
use crate::use_cases::user::UserPersistence;

/// Every status the queue's state machine allows, so the release is tested on history as well as
/// on live work — the trigger's `SELECT` carries no status predicate, and that is the finding
/// `plan/db_audit/phase6.md` records.
const EVERY_STATUS: [&str; 8] = [
    "pending",
    "processing",
    "pending_approval",
    "waiting_for_third_party_reply",
    "completed",
    "failed",
    "dead_letter",
    "stopped",
];

/// A company with a joined member, an unassigned agent, and a channel to hang tasks off.
struct ReleaseFixture {
    pool: PgPool,
    company_id: Uuid,
    channel_id: Uuid,
    member_user_id: Uuid,
    member_principal: Uuid,
    member_label: String,
    agent_id: Uuid,
    agent_principal: Uuid,
    agent_label: String,
}

/// The company is built the way the product builds one: the member arrives through an invite they
/// accept, so their principal is the real one, and the agent gets its principal from
/// `AgentPersistence::create`. The agent is left off every channel so `AgentPersistence::delete`
/// reaches the cascade instead of the "active on an enabled channel" conflict.
async fn release_fixture(persistence: &PostgresPersistence, label: &str) -> ReleaseFixture {
    let pool = persistence.pool().clone();
    let suffix = Uuid::new_v4().simple().to_string();

    let owner_email = format!("{label}_owner_{suffix}@example.com");
    persistence
        .create_user(&format!("{label}_owner_{suffix}"), &owner_email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(persistence, &owner_email)
        .await
        .unwrap()
        .unwrap();
    let member_email = format!("{label}_member_{suffix}@example.com");
    persistence
        .create_user(&format!("{label}_member_{suffix}"), &member_email, "hash")
        .await
        .unwrap();
    let member = UserPersistence::get_by_email(persistence, &member_email)
        .await
        .unwrap()
        .unwrap();

    let company = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Released Work".to_string(),
            slug: format!("{label}-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let invite = persistence
        .create_invite(company.id, &member_email, CompanyAccessRole::Member)
        .await
        .unwrap();
    persistence
        .accept_pending_invite(invite.id, member.id, &member_email)
        .await
        .unwrap()
        .unwrap();

    let agent = AgentPersistence::create(
        persistence,
        company.id,
        AgentWrite {
            name: "Release Agent".to_string(),
            slug: format!("{label}-agent-{suffix}"),
            created_by: Some(CreationProvenance::system()),
            harness_kind: Some(HarnessKind::AiAgents),
            ..AgentWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Release Desk".to_string(),
            slug: format!("{label}-desk-{suffix}"),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();

    let (member_principal, member_label) = principal_of(&pool, company.id, "user_id", member.id)
        .await
        .expect("the accepted invite wrote a person principal");
    let (agent_principal, agent_label) = principal_of(&pool, company.id, "agent_id", agent.id)
        .await
        .expect("creating the agent wrote an agent principal");

    ReleaseFixture {
        pool,
        company_id: company.id,
        channel_id: channel.id,
        member_user_id: member.id,
        member_principal,
        member_label,
        agent_id: agent.id,
        agent_principal,
        agent_label,
    }
}

/// The principal a company row of `column` belongs to, with the label the release event copies.
async fn principal_of(
    pool: &PgPool,
    company_id: Uuid,
    column: &str,
    subject_id: Uuid,
) -> Option<(Uuid, String)> {
    sqlx::query_as::<_, (Uuid, String)>(&format!(
        "SELECT principal.id, principal.display_label FROM principals AS principal \
         WHERE principal.company_id = $1 AND principal.{column} = $2"
    ))
    .bind(company_id)
    .bind(subject_id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

/// One task as it stood before the release, so the assertions can name what it used to be.
struct OwnedTask {
    id: Uuid,
    status: &'static str,
    ownership_version: i64,
    execution_generation: Option<Uuid>,
}

impl OwnedTask {
    /// The sequence the release event must carry: this task's own version plus one.
    fn released_sequence(&self) -> i64 {
        self.ownership_version + 1
    }
}

#[derive(sqlx::FromRow)]
struct ReleasedRow {
    status: String,
    owner_principal_id: Option<Uuid>,
    owner_principal_kind: Option<String>,
    worker_id: Option<Uuid>,
    execution_generation: Option<Uuid>,
    locked_at: Option<DateTime<Utc>>,
    lock_expires_at: Option<DateTime<Utc>>,
    ownership_version: i64,
    transition_reason: Option<String>,
    transition_actor_kind: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ReleaseEventRow {
    task_id: Uuid,
    sequence: i64,
    from_version: i64,
    to_version: i64,
    operation: String,
    reason: String,
    actor_kind: String,
    previous_owner_principal_id: Option<Uuid>,
    previous_owner_kind: Option<String>,
    previous_owner_label: Option<String>,
    new_owner_principal_id: Option<Uuid>,
    new_owner_kind: Option<String>,
    command_fingerprint: String,
}

#[derive(sqlx::FromRow)]
struct AttemptRow {
    attempt_number: i32,
    status: String,
    stop_reason: Option<String>,
    error: Option<String>,
    finished_at: Option<DateTime<Utc>>,
}

impl ReleaseFixture {
    /// A task owned by `principal` in `status`, moved to ownership version `ownership_version`.
    ///
    /// `run_at` is pushed a day out because this suite shares one database: a `pending` row due now
    /// is claimable by whatever worker test is running alongside, and a claim would change the very
    /// status this test is about to assert on. The `processing` rows carry the whole lease
    /// `background_tasks_lease_check` demands, and nothing else does.
    async fn owned_task(
        &self,
        principal_id: Uuid,
        principal_kind: &str,
        status: &'static str,
        ownership_version: i64,
    ) -> OwnedTask {
        let id = Uuid::new_v4();
        let running = status == "processing";
        let execution_generation = running.then(Uuid::new_v4);
        let worker_id = running.then(Uuid::new_v4);
        sqlx::query(
            "INSERT INTO background_tasks (
                 id, company_id, channel_id, correlation_id, task_type, status,
                 owner_principal_id, owner_principal_kind, run_at, worker_id,
                 execution_generation, locked_at, lock_expires_at, transition_reason,
                 transition_actor_kind, transition_actor_id
             ) VALUES (
                 $1, $2, $3, gen_random_uuid(), 'agent_run', $4, $5, $6,
                 CURRENT_TIMESTAMP + INTERVAL '1 day', $7, $8,
                 CASE WHEN $8::uuid IS NULL THEN NULL ELSE CURRENT_TIMESTAMP END,
                 CASE WHEN $8::uuid IS NULL THEN NULL
                      ELSE CURRENT_TIMESTAMP + INTERVAL '5 minutes' END,
                 CASE WHEN $8::uuid IS NULL THEN NULL ELSE 'claimed' END,
                 CASE WHEN $8::uuid IS NULL THEN NULL ELSE 'worker' END,
                 $7
             )",
        )
        .bind(id)
        .bind(self.company_id)
        .bind(self.channel_id)
        .bind(status)
        .bind(principal_id)
        .bind(principal_kind)
        .bind(worker_id)
        .bind(execution_generation)
        .execute(&self.pool)
        .await
        .unwrap();

        // The insert trigger forces version 1, so the version each task is released *from* is set
        // afterwards. Distinct versions are what make the sequence assertion mean something: a
        // running counter over the released set would match only if every version were equal.
        sqlx::query(
            "UPDATE background_tasks AS task SET ownership_version = $3
              WHERE task.company_id = $1 AND task.id = $2",
        )
        .bind(self.company_id)
        .bind(id)
        .bind(ownership_version)
        .execute(&self.pool)
        .await
        .unwrap();

        OwnedTask {
            id,
            status,
            ownership_version,
            execution_generation,
        }
    }

    /// One task per status, each at its own ownership version.
    async fn tasks_in_every_status(
        &self,
        principal_id: Uuid,
        principal_kind: &str,
    ) -> Vec<OwnedTask> {
        let mut owned = Vec::with_capacity(EVERY_STATUS.len());
        for (offset, status) in EVERY_STATUS.iter().enumerate() {
            owned.push(
                self.owned_task(principal_id, principal_kind, status, 2 + offset as i64)
                    .await,
            );
        }
        owned
    }

    /// An attempt row on `task`, in whatever generation and status the test needs.
    async fn attempt(
        &self,
        task_id: Uuid,
        attempt_number: i32,
        execution_generation: Uuid,
        status: &str,
    ) {
        sqlx::query(
            "INSERT INTO task_attempts (
                 id, task_id, attempt_number, status, execution_generation, worker_id, machine_id
             ) VALUES (gen_random_uuid(), $1, $2, $3, $4, gen_random_uuid(), 'test-machine')",
        )
        .bind(task_id)
        .bind(attempt_number)
        .bind(status)
        .bind(execution_generation)
        .execute(&self.pool)
        .await
        .unwrap();
    }

    async fn task_row(&self, task_id: Uuid) -> ReleasedRow {
        sqlx::query_as::<_, ReleasedRow>(
            "SELECT task.status, task.owner_principal_id, task.owner_principal_kind,
                    task.worker_id, task.execution_generation, task.locked_at,
                    task.lock_expires_at, task.ownership_version, task.transition_reason,
                    task.transition_actor_kind
               FROM background_tasks AS task
              WHERE task.company_id = $1 AND task.id = $2",
        )
        .bind(self.company_id)
        .bind(task_id)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    /// The `owner_removed` events of exactly the tasks this test created.
    async fn release_events(&self, task_ids: &[Uuid]) -> Vec<ReleaseEventRow> {
        sqlx::query_as::<_, ReleaseEventRow>(
            "SELECT event.task_id, event.sequence, event.from_version, event.to_version,
                    event.operation, event.reason, event.actor_kind,
                    event.previous_owner_principal_id, event.previous_owner_kind,
                    event.previous_owner_label, event.new_owner_principal_id,
                    event.new_owner_kind, event.command_fingerprint
               FROM task_ownership_events AS event
              WHERE event.company_id = $1 AND event.task_id = ANY($2)
                AND event.operation = 'owner_removed'
              ORDER BY event.task_id, event.sequence",
        )
        .bind(self.company_id)
        .bind(task_ids)
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }

    async fn attempts(&self, task_id: Uuid) -> Vec<AttemptRow> {
        sqlx::query_as::<_, AttemptRow>(
            "SELECT attempt.attempt_number, attempt.status, attempt.stop_reason, attempt.error,
                    attempt.finished_at
               FROM task_attempts AS attempt
              WHERE attempt.task_id = $1
              ORDER BY attempt.attempt_number",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }

    /// Every ownership event this company has, so an "and nothing else happened" assertion can be
    /// made without counting rows another test wrote.
    async fn ownership_event_count(&self) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM task_ownership_events AS event WHERE event.company_id = $1",
        )
        .bind(self.company_id)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }
}

/// Assert the whole release: the task rows, one event per task at the right sequence, and nothing
/// claimed about the tasks that were not running.
async fn assert_released(
    fixture: &ReleaseFixture,
    owned: &[OwnedTask],
    previous_principal: Uuid,
    previous_kind: &str,
    previous_label: &str,
) {
    for task in owned {
        let row = fixture.task_row(task.id).await;
        let expected_status = if task.status == "processing" {
            "pending"
        } else {
            task.status
        };
        assert_eq!(
            row.status, expected_status,
            "{} keeps its status unless it was running",
            task.status
        );
        assert_eq!(
            row.owner_principal_id, None,
            "{} keeps no owner",
            task.status
        );
        assert_eq!(row.owner_principal_kind, None);
        assert_eq!(row.worker_id, None, "{} keeps no lease", task.status);
        assert_eq!(row.execution_generation, None);
        assert_eq!(row.locked_at, None);
        assert_eq!(row.lock_expires_at, None);
        assert_eq!(
            row.ownership_version,
            task.ownership_version + 1,
            "{} advances one ownership version",
            task.status
        );
        if task.status == "processing" {
            assert_eq!(
                row.transition_reason.as_deref(),
                Some("ownership_transferred")
            );
            assert_eq!(row.transition_actor_kind.as_deref(), Some("system"));
        } else {
            assert_eq!(
                row.transition_reason.as_deref(),
                None,
                "{} was not transitioned, so nothing is attributed to it",
                task.status
            );
            assert_eq!(row.transition_actor_kind.as_deref(), None);
        }
    }

    let task_ids: Vec<Uuid> = owned.iter().map(|task| task.id).collect();
    let events = fixture.release_events(&task_ids).await;
    assert_eq!(
        events.len(),
        owned.len(),
        "one removal event per released task, whatever its status"
    );
    for task in owned {
        let event = events
            .iter()
            .find(|event| event.task_id == task.id)
            .unwrap_or_else(|| panic!("{} was released without an event", task.status));
        assert_eq!(
            event.sequence,
            task.released_sequence(),
            "{} numbers its event off its own version, not a running count",
            task.status
        );
        assert_eq!(event.from_version, task.ownership_version);
        assert_eq!(event.to_version, task.released_sequence());
        assert_eq!(event.operation, "owner_removed");
        assert_eq!(event.reason, "owner_removed");
        assert_eq!(event.actor_kind, "system");
        assert_eq!(
            event.previous_owner_principal_id,
            Some(previous_principal),
            "{} names who it was taken from",
            task.status
        );
        assert_eq!(event.previous_owner_kind.as_deref(), Some(previous_kind));
        assert_eq!(
            event.previous_owner_label.as_deref(),
            Some(previous_label),
            "the label is copied while the principal still has one"
        );
        assert_eq!(event.new_owner_principal_id, None);
        assert_eq!(event.new_owner_kind.as_deref(), Some("unassigned"));
        assert_eq!(
            event.command_fingerprint,
            format!(
                "owner-removed:{previous_principal}:{}",
                task.ownership_version
            ),
            "{} fingerprints the version it was released from",
            task.status
        );
    }
}

/// The task in `owned` that was left `processing`.
fn running_task(owned: &[OwnedTask]) -> &OwnedTask {
    owned
        .iter()
        .find(|task| task.status == "processing")
        .expect("the fixture leaves exactly one task running")
}

/// The task in `owned` in `status`.
fn task_in<'a>(owned: &'a [OwnedTask], status: &str) -> &'a OwnedTask {
    owned
        .iter()
        .find(|task| task.status == status)
        .unwrap_or_else(|| panic!("the fixture owns a {status} task"))
}

/// Taking a person off the team releases everything they owned, in every status.
///
/// This is the demotion arm of the trigger: `remove_member` flips the principal's `kind` to
/// `external`, and the owner foreign key carries `kind`, so the update cannot commit while any
/// task still points at them.
#[tokio::test]
async fn removing_a_member_releases_every_task_they_owned_in_every_status() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fixture = release_fixture(&persistence, "member-release").await;
    let owned = fixture
        .tasks_in_every_status(fixture.member_principal, "person")
        .await;

    // Three attempt rows that together state which attempts the release may touch: the running
    // task's live attempt, the same task's attempt from a superseded generation, and a live
    // attempt on a task that is not running at all.
    let running = running_task(&owned);
    let live_generation = running
        .execution_generation
        .expect("a running task has one");
    let superseded_generation = Uuid::new_v4();
    fixture
        .attempt(running.id, 1, live_generation, "processing")
        .await;
    fixture
        .attempt(running.id, 2, superseded_generation, "processing")
        .await;
    let parked = task_in(&owned, "pending_approval");
    fixture
        .attempt(parked.id, 1, Uuid::new_v4(), "processing")
        .await;

    persistence
        .remove_member(fixture.company_id, fixture.member_user_id)
        .await
        .unwrap();

    let demoted = sqlx::query_as::<_, (String, Option<Uuid>)>(
        "SELECT principal.kind, principal.user_id FROM principals AS principal
          WHERE principal.company_id = $1 AND principal.id = $2",
    )
    .bind(fixture.company_id)
    .bind(fixture.member_principal)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(demoted, ("external".to_string(), None));

    assert_released(
        &fixture,
        &owned,
        fixture.member_principal,
        "human",
        &fixture.member_label,
    )
    .await;

    let fenced = fixture.attempts(running.id).await;
    assert_eq!(fenced.len(), 2);
    assert_eq!(fenced[0].attempt_number, 1);
    assert_eq!(fenced[0].status, "failed", "the live attempt is fenced");
    assert_eq!(
        fenced[0].stop_reason.as_deref(),
        Some("ownership_transferred")
    );
    assert_eq!(
        fenced[0].error.as_deref(),
        Some("Task ownership was removed")
    );
    assert!(fenced[0].finished_at.is_some());
    assert_eq!(fenced[1].attempt_number, 2);
    assert_eq!(
        fenced[1].status, "processing",
        "a superseded generation is not this release's to close"
    );
    assert_eq!(fenced[1].stop_reason, None);

    let untouched = fixture.attempts(parked.id).await;
    assert_eq!(untouched.len(), 1);
    assert_eq!(
        untouched[0].status, "processing",
        "the release fences the attempts of running tasks only"
    );
    assert_eq!(untouched[0].stop_reason, None);
    assert_eq!(untouched[0].finished_at, None);

    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

/// Deleting an agent takes the same path and produces the same result.
///
/// Here the principal is deleted rather than demoted — cascaded from `agents` — so this is the
/// `DELETE` arm of the same trigger body, and `ON DELETE RESTRICT` on the owner foreign key means
/// the delete fails outright if the release misses a task.
#[tokio::test]
async fn deleting_an_agent_releases_every_task_its_principal_owned() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fixture = release_fixture(&persistence, "agent-release").await;
    let owned = fixture
        .tasks_in_every_status(fixture.agent_principal, "agent")
        .await;

    let running = running_task(&owned);
    let live_generation = running
        .execution_generation
        .expect("a running task has one");
    fixture
        .attempt(running.id, 1, live_generation, "processing")
        .await;

    AgentPersistence::delete(&persistence, fixture.agent_id)
        .await
        .unwrap();

    let surviving: Option<(Uuid,)> = sqlx::query_as(
        "SELECT principal.id FROM principals AS principal
          WHERE principal.company_id = $1 AND principal.id = $2",
    )
    .bind(fixture.company_id)
    .bind(fixture.agent_principal)
    .fetch_optional(&fixture.pool)
    .await
    .unwrap();
    assert!(
        surviving.is_none(),
        "the agent's principal is cascaded away with it"
    );

    assert_released(
        &fixture,
        &owned,
        fixture.agent_principal,
        "agent",
        &fixture.agent_label,
    )
    .await;

    let fenced = fixture.attempts(running.id).await;
    assert_eq!(fenced.len(), 1);
    assert_eq!(fenced[0].status, "failed");
    assert_eq!(
        fenced[0].stop_reason.as_deref(),
        Some("ownership_transferred")
    );

    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}

/// A principal that owns nothing goes away without writing anything.
///
/// The empty case is the one a set-based rewrite can get wrong: an `UPDATE` that matches no row
/// returns no `RETURNING` row, and the two statements fed from it must do nothing rather than fail
/// or insert a stray event.
#[tokio::test]
async fn a_principal_owning_no_tasks_is_removed_without_an_ownership_event() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let fixture = release_fixture(&persistence, "empty-release").await;
    assert_eq!(
        fixture.ownership_event_count().await,
        0,
        "the fixture queues no tasks, so it writes no ownership events"
    );

    persistence
        .remove_member(fixture.company_id, fixture.member_user_id)
        .await
        .unwrap();
    AgentPersistence::delete(&persistence, fixture.agent_id)
        .await
        .unwrap();

    assert_eq!(
        fixture.ownership_event_count().await,
        0,
        "neither removal invents an event for a principal that owned nothing"
    );

    CompanyPersistence::delete(&persistence, fixture.company_id)
        .await
        .unwrap();
}
