//! Removing an agent from a channel, against a real database.
//!
//! The sequential tests share the test database and scope every assertion to their own company.
//! The races take a database of their own, because their barrier counts every blocked backend.

use chrono::Utc;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        channel_gate::{ChannelGateAccess, ChannelGateKey, acquire_channel_gate_on},
        delivery::enqueue::insert_delivery_on,
        task::{insert_task, record_outreach_reply_on},
        test_support::{
            DeliveryFixtureRequest, delivery_fixture, own_database, test_pool,
            wait_until_a_backend_is_blocked, wait_until_backends_are_blocked,
        },
    },
    entities::{
        creation::CreationProvenance,
        message::{MessageDirection, MessageRole},
        outreach::{OutreachReplyMatch, OutreachStatus},
        task::{BackgroundTask, NewTask, ResumeActor, TaskLeaseRef},
        transport::{DeliveryId, DeliveryPurpose},
    },
    task_queue::{
        AgentDispatchCommit, CreateOutreachRequest, DispatchCommit, OutreachTargetIdentity,
        OutreachTargetRequest, TaskPersistence,
    },
    transport::NewDelivery,
    use_cases::{
        agent::{AgentPersistence, AgentWrite, OwnedAgentChannelPersistence},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::{
            AgentAuthor, AgentReply, MessageAuthorWrite, MessageWrite, ThreadPersistence,
            qualified_email_identity,
            test_support::{EmailMessageDraft, email_write},
        },
        user::UserPersistence,
    },
};

/// The one external address the outreach races ask.
const PARTNER: &str = "partner@partner.test";

/// A company whose "removal" channel has agents A and B, and whose "elsewhere" channel has A.
struct Fixture {
    persistence: PostgresPersistence,
    company_id: Uuid,
    manager: Uuid,
    channel_id: Uuid,
    elsewhere_id: Uuid,
    thread_id: Uuid,
    agent_a: Uuid,
    agent_b: Uuid,
    principal_a: Uuid,
    principal_b: Uuid,
    person: Uuid,
}

async fn fixture(pool: &sqlx::PgPool) -> Fixture {
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("removal_manager_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let manager = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        manager.id,
        CompanyWrite {
            name: "Removal Test".into(),
            slug: format!("removal-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent_a = agent(&persistence, company.id, "agent-a").await;
    let agent_b = agent(&persistence, company.id, "agent-b").await;
    let channel = ChannelPersistence::create(
        &persistence,
        company.id,
        write("removal", &[agent_a, agent_b]),
    )
    .await
    .unwrap();
    let elsewhere =
        ChannelPersistence::create(&persistence, company.id, write("elsewhere", &[agent_a]))
            .await
            .unwrap();
    let thread_id = ThreadPersistence::create_thread(&persistence, channel.id, "Removal", &[])
        .await
        .unwrap()
        .id;
    let principal = |filter: &'static str, id: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Uuid>(&format!(
                "SELECT id FROM principals WHERE company_id = $1 AND {filter} = $2"
            ))
            .bind(company.id)
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    Fixture {
        principal_a: principal("agent_id", agent_a).await,
        principal_b: principal("agent_id", agent_b).await,
        person: principal("user_id", manager.id).await,
        persistence,
        company_id: company.id,
        manager: manager.id,
        channel_id: channel.id,
        elsewhere_id: elsewhere.id,
        thread_id,
        agent_a,
        agent_b,
    }
}

async fn agent(persistence: &PostgresPersistence, company_id: Uuid, slug: &str) -> Uuid {
    AgentPersistence::create(
        persistence,
        company_id,
        AgentWrite {
            name: slug.into(),
            slug: slug.into(),
            created_by: Some(CreationProvenance::system()),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}

fn write(slug: &str, agents: &[Uuid]) -> ChannelWrite {
    ChannelWrite {
        name: slug.into(),
        slug: slug.into(),
        agent_ids: Some(agents.to_vec()),
        enabled: false,
        ..ChannelWrite::default()
    }
}

impl Fixture {
    fn request(&self, agents: &[Uuid]) -> ChannelUpdate {
        ChannelUpdate {
            company_id: self.company_id,
            channel_id: self.channel_id,
            actor_user_id: self.manager,
            write: write("removal", agents),
        }
    }

    async fn save(&self, agents: &[Uuid]) -> AppResult<AssignmentRemoval> {
        update_channel_within(
            self.persistence.pool(),
            self.request(agents),
            RemovalBudget::DEFAULT,
        )
        .await
    }

    /// A task on `channel_id`, owned by whichever agent holds position 0 there.
    async fn enqueue(&self, channel_id: Uuid) -> Uuid {
        self.persistence
            .enqueue_task(NewTask::starting_new_chain(
                self.company_id,
                channel_id,
                None,
                "removal_test",
                serde_json::json!({}),
            ))
            .await
            .unwrap()
            .id
    }

    async fn set_status(&self, task_id: Uuid, status: &str) {
        sqlx::query("UPDATE background_tasks SET status = $2 WHERE id = $1")
            .bind(task_id)
            .bind(status)
            .execute(self.persistence.pool())
            .await
            .unwrap();
    }

    async fn set_owner(&self, task_id: Uuid, principal_id: Uuid, kind: &str) {
        sqlx::query(
            "UPDATE background_tasks SET owner_principal_id = $2, owner_principal_kind = $3 WHERE id = $1",
        )
        .bind(task_id)
        .bind(principal_id)
        .bind(kind)
        .execute(self.persistence.pool())
        .await
        .unwrap();
    }

    /// A queued delivery produced by `task_id`, parked beyond every claim's horizon so a
    /// neighbouring test's global claim cannot take it.
    async fn queue_delivery(&self, task_id: Uuid, key: &str) -> DeliveryId {
        let fixture = delivery_fixture(
            &self.persistence,
            DeliveryFixtureRequest {
                task_id: Some(task_id),
                ..DeliveryFixtureRequest::new(self.company_id, self.channel_id, self.thread_id, key)
            },
        )
        .await;
        let mut tx = self.persistence.pool().begin().await.unwrap();
        insert_delivery_on(&mut tx, &fixture.delivery)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("UPDATE message_deliveries SET available_at = $2 WHERE id = $1")
            .bind(fixture.delivery.id.as_uuid())
            .bind(Utc::now() + chrono::Duration::days(3650))
            .execute(self.persistence.pool())
            .await
            .unwrap();
        fixture.delivery.id
    }

    async fn status(&self, task_id: Uuid) -> String {
        sqlx::query_scalar("SELECT status FROM background_tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    async fn owner(&self, task_id: Uuid) -> Option<Uuid> {
        sqlx::query_scalar("SELECT owner_principal_id FROM background_tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    async fn events(&self, task_id: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM task_status_events WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    async fn delivery(&self, delivery_id: DeliveryId) -> (String, Option<String>) {
        sqlx::query_as("SELECT status, cancellation_reason FROM message_deliveries WHERE id = $1")
            .bind(delivery_id.as_uuid())
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }

    async fn assignments(&self) -> Vec<Uuid> {
        ChannelPersistence::get_by_id(&self.persistence, self.channel_id)
            .await
            .unwrap()
            .unwrap()
            .agent_ids
            .unwrap_or_default()
    }

    async fn delete(self) {
        CompanyPersistence::delete(&self.persistence, self.company_id)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn removing_an_agent_stops_exactly_its_work_in_that_channel() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;

    let pending = f.enqueue(f.channel_id).await;
    let pending_delivery = f.queue_delivery(pending, "pending").await;
    let processing = f.enqueue(f.channel_id).await;
    assert!(
        f.persistence
            .claim_task(
                processing,
                Uuid::new_v4(),
                Utc::now() + chrono::Duration::minutes(5)
            )
            .await
            .unwrap()
    );
    sqlx::query(
        r#"INSERT INTO task_attempts
               (id, task_id, attempt_number, execution_generation, status, worker_id, machine_id)
           SELECT gen_random_uuid(), id, 1, execution_generation, 'processing', worker_id, 'test'
             FROM background_tasks WHERE id = $1"#,
    )
    .bind(processing)
    .execute(&pool)
    .await
    .unwrap();
    let mut parked = Vec::new();
    for status in [
        "failed",
        "dead_letter",
        "pending_approval",
        "waiting_for_third_party_reply",
    ] {
        let task = f.enqueue(f.channel_id).await;
        f.set_status(task, status).await;
        parked.push(task);
    }
    let completed = f.enqueue(f.channel_id).await;
    f.set_status(completed, "completed").await;
    let completed_output = f.queue_delivery(completed, "completed").await;
    let stopped_quietly = f.enqueue(f.channel_id).await;
    f.set_status(stopped_quietly, "stopped").await;
    let other_agent = f.enqueue(f.channel_id).await;
    f.set_owner(other_agent, f.principal_b, "agent").await;
    let person = f.enqueue(f.channel_id).await;
    f.set_owner(person, f.person, "person").await;
    let elsewhere = f.enqueue(f.elsewhere_id).await;
    let quiet_events = f.events(stopped_quietly).await;
    let completed_events = f.events(completed).await;

    let removal = f.save(&[f.agent_b]).await.unwrap();

    assert_eq!(removal.removed_agent_ids, vec![f.agent_a]);
    assert_eq!(removal.stopped_tasks, 6);
    assert_eq!(removal.cancelled_deliveries, 2);
    assert!(removal.potentially_sent_delivery_ids.is_empty());
    for task in [pending, processing]
        .into_iter()
        .chain(parked.iter().copied())
    {
        assert_eq!(f.status(task).await, "stopped");
        assert_eq!(
            f.owner(task).await,
            Some(f.principal_a),
            "stopping preserves the owner and its history"
        );
    }
    let (reason, actor_kind, actor_id): (String, String, Option<Uuid>) = sqlx::query_as(
        r#"SELECT reason, actor_kind, actor_id FROM task_status_events
            WHERE task_id = $1 ORDER BY sequence DESC LIMIT 1"#,
    )
    .bind(pending)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (reason.as_str(), actor_kind.as_str(), actor_id),
        ("channel_agent_removed", "operator", Some(f.manager))
    );
    let (attempt_status, stop_reason): (String, Option<String>) =
        sqlx::query_as("SELECT status, stop_reason FROM task_attempts WHERE task_id = $1")
            .bind(processing)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(attempt_status, "failed");
    assert_eq!(stop_reason.as_deref(), Some("channel_agent_removed"));

    assert_eq!(f.status(completed).await, "completed");
    for delivery in [pending_delivery, completed_output] {
        assert_eq!(
            f.delivery(delivery).await,
            (
                "dead_letter".to_string(),
                Some("channel_agent_removed".to_string())
            ),
            "a completed task's queued reply is withdrawn along with running work"
        );
    }
    assert_eq!(f.status(stopped_quietly).await, "stopped");
    assert_eq!(f.events(stopped_quietly).await, quiet_events);
    for untouched in [other_agent, person, elsewhere] {
        assert_eq!(f.status(untouched).await, "pending");
    }
    assert_eq!(f.assignments().await, vec![f.agent_b]);

    // Saving the same assignments again touches no task and writes no event.
    let pending_events = f.events(pending).await;
    assert_eq!(
        f.save(&[f.agent_b]).await.unwrap(),
        AssignmentRemoval::default()
    );
    assert_eq!(f.events(pending).await, pending_events);
    assert_eq!(f.events(completed).await, completed_events);

    // Re-adding the agent revives nothing, and a resume must not make unrunnable work runnable.
    let resumed = f
        .persistence
        .resume_task(pending, ResumeActor::Operator(f.manager))
        .await;
    assert!(matches!(resumed, Err(AppError::Conflict(_))), "{resumed:?}");
    assert_eq!(
        f.save(&[f.agent_b, f.agent_a]).await.unwrap(),
        AssignmentRemoval::default()
    );
    assert_eq!(f.status(pending).await, "stopped");
    assert_eq!(f.delivery(pending_delivery).await.0, "dead_letter");

    f.delete().await;
}

#[tokio::test]
async fn reordering_and_adding_agents_change_no_work() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;
    let task = f.enqueue(f.channel_id).await;
    let events = f.events(task).await;
    let added = agent(&f.persistence, f.company_id, "agent-z").await;

    let removal = f.save(&[f.agent_b, f.agent_a, added]).await.unwrap();

    assert_eq!(removal, AssignmentRemoval::default());
    assert_eq!(f.assignments().await, vec![f.agent_b, f.agent_a, added]);
    assert_eq!(f.status(task).await, "pending");
    assert_eq!(f.events(task).await, events);
    f.delete().await;
}

#[tokio::test]
async fn a_failed_edit_rolls_back_the_removal_with_it() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;
    let task = f.enqueue(f.channel_id).await;
    let delivery = f.queue_delivery(task, "rollback").await;

    // The settings write fails after every cleanup statement has run: the slug is taken.
    let mut request = f.request(&[f.agent_b]);
    request.write.slug = "elsewhere".into();
    let refused = update_channel_within(&pool, request, RemovalBudget::DEFAULT).await;

    assert!(refused.is_err());
    assert_eq!(f.status(task).await, "pending");
    assert_eq!(f.delivery(delivery).await, ("pending".to_string(), None));
    assert_eq!(f.assignments().await, vec![f.agent_a, f.agent_b]);
    f.delete().await;
}

#[tokio::test]
async fn a_removal_over_its_budget_is_refused_whole() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;
    let first = f.enqueue(f.channel_id).await;
    let second = f.enqueue(f.channel_id).await;
    f.queue_delivery(first, "budget").await;

    let too_many_tasks = RemovalBudget {
        max_tasks: 1,
        ..RemovalBudget::DEFAULT
    };
    let refused = update_channel_within(&pool, f.request(&[f.agent_b]), too_many_tasks).await;
    assert!(matches!(refused, Err(AppError::Conflict(_))), "{refused:?}");

    let too_much_work = RemovalBudget {
        max_dependent_rows: 0,
        ..RemovalBudget::DEFAULT
    };
    let refused = update_channel_within(&pool, f.request(&[f.agent_b]), too_much_work).await;
    assert!(matches!(refused, Err(AppError::Conflict(_))), "{refused:?}");

    for task in [first, second] {
        assert_eq!(
            f.status(task).await,
            "pending",
            "nothing is stopped partially"
        );
    }
    assert_eq!(f.assignments().await, vec![f.agent_a, f.agent_b]);
    f.delete().await;
}

#[tokio::test]
async fn an_owned_channel_cannot_lose_its_owner() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;
    let (owner, owned) = OwnedAgentChannelPersistence::create_owned_agent_channel(
        &f.persistence,
        f.company_id,
        AgentWrite {
            name: "Owner".into(),
            slug: "owner".into(),
            created_by: Some(CreationProvenance::system()),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        },
        write("owner", &[]),
    )
    .await
    .unwrap();

    let refused = update_channel_within(
        &pool,
        ChannelUpdate {
            company_id: f.company_id,
            channel_id: owned.id,
            actor_user_id: f.manager,
            write: write("owner", &[f.agent_b]),
        },
        RemovalBudget::DEFAULT,
    )
    .await;

    assert!(matches!(refused, Err(AppError::Conflict(_))), "{refused:?}");
    let stored = ChannelPersistence::get_by_id(&f.persistence, owned.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.agent_ids, Some(vec![owner.id]));
    f.delete().await;
}

/// A task admitted while the removal waits on the gate is visible to the removal, and stopped.
#[tokio::test]
async fn a_task_admitted_before_the_removal_commits_is_stopped_by_it() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;

    let mut admission = pool.begin().await.unwrap();
    acquire_channel_gate_on(
        &mut admission,
        ChannelGateKey::new(f.company_id, f.channel_id),
        ChannelGateAccess::Admit,
    )
    .await
    .unwrap();
    let task = insert_task(
        &mut admission,
        NewTask::starting_new_chain(
            f.company_id,
            f.channel_id,
            None,
            "removal_race",
            serde_json::json!({}),
        ),
    )
    .await
    .unwrap();

    let removal = {
        let pool = pool.clone();
        let request = f.request(&[f.agent_b]);
        tokio::spawn(
            async move { update_channel_within(&pool, request, RemovalBudget::DEFAULT).await },
        )
    };
    wait_until_a_backend_is_blocked(&pool).await;
    admission.commit().await.unwrap();

    let removal = removal.await.unwrap().unwrap();
    assert_eq!(removal.stopped_tasks, 1);
    assert_eq!(f.status(task.id).await, "stopped");
}

/// A task admitted while the removal holds the gate waits for it, then takes its owner from the
/// assignments the removal left behind.
#[tokio::test]
async fn a_task_admitted_after_the_removal_commits_goes_to_a_remaining_agent() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;

    let mut removal = pool.begin().await.unwrap();
    update_channel_on(
        &mut removal,
        &f.request(&[f.agent_b]),
        RemovalBudget::DEFAULT,
    )
    .await
    .unwrap();

    let admission = {
        let persistence = PostgresPersistence::new(pool.clone());
        let task = NewTask::starting_new_chain(
            f.company_id,
            f.channel_id,
            None,
            "removal_race",
            serde_json::json!({}),
        );
        tokio::spawn(async move { persistence.enqueue_task(task).await.unwrap() })
    };
    wait_until_a_backend_is_blocked(&pool).await;
    removal.commit().await.unwrap();

    let task = admission.await.unwrap();
    assert_eq!(f.owner(task.id).await, Some(f.principal_b));
    assert_eq!(f.status(task.id).await, "pending");
}

/// A claim that meets a task the uncommitted removal has locked waits, and then finds it stopped.
#[tokio::test]
async fn a_claim_racing_the_removal_finds_the_task_stopped() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;
    let task = f.enqueue(f.channel_id).await;

    let mut removal = pool.begin().await.unwrap();
    update_channel_on(
        &mut removal,
        &f.request(&[f.agent_b]),
        RemovalBudget::DEFAULT,
    )
    .await
    .unwrap();

    let claim = {
        let persistence = PostgresPersistence::new(pool.clone());
        tokio::spawn(async move {
            persistence
                .claim_task(
                    task,
                    Uuid::new_v4(),
                    Utc::now() + chrono::Duration::minutes(5),
                )
                .await
                .unwrap()
        })
    };
    wait_until_a_backend_is_blocked(&pool).await;
    removal.commit().await.unwrap();

    assert!(!claim.await.unwrap(), "a stopped task is not claimable");
    assert_eq!(f.status(task).await, "stopped");
}

/// A resume that arrives while the removal holds the gate is refused once the removal commits,
/// even for a task the removal itself had no reason to lock.
#[tokio::test]
async fn a_resume_racing_the_removal_is_refused() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;
    let task = f.enqueue(f.channel_id).await;
    f.set_status(task, "stopped").await;

    let mut removal = pool.begin().await.unwrap();
    let removed = update_channel_on(
        &mut removal,
        &f.request(&[f.agent_b]),
        RemovalBudget::DEFAULT,
    )
    .await
    .unwrap();
    assert_eq!(
        removed.stopped_tasks, 0,
        "a quietly stopped task is not in the working set"
    );

    let resume = {
        let persistence = PostgresPersistence::new(pool.clone());
        let manager = f.manager;
        tokio::spawn(async move {
            persistence
                .resume_task(task, ResumeActor::Operator(manager))
                .await
        })
    };
    wait_until_a_backend_is_blocked(&pool).await;
    removal.commit().await.unwrap();

    let resumed = resume.await.unwrap();
    assert!(matches!(resumed, Err(AppError::Conflict(_))), "{resumed:?}");
    assert_eq!(f.status(task).await, "stopped");
}

impl Fixture {
    /// A task on the removal channel's thread, claimed by a worker, with its lease.
    async fn claimed(&self, task_type: &str) -> (BackgroundTask, TaskLeaseRef) {
        let task = self
            .persistence
            .enqueue_task(NewTask::starting_new_chain(
                self.company_id,
                self.channel_id,
                Some(self.thread_id),
                task_type,
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        assert!(
            self.persistence
                .claim_task(
                    task.id,
                    Uuid::new_v4(),
                    Utc::now() + chrono::Duration::minutes(5)
                )
                .await
                .unwrap()
        );
        let claimed = self
            .persistence
            .get_task_by_id(task.id)
            .await
            .unwrap()
            .unwrap();
        let lease = TaskLeaseRef::of(&claimed).unwrap();
        (claimed, lease)
    }

    /// The final reply `task` would publish, and the one delivery that carries it.
    async fn final_reply(&self, task: &BackgroundTask, key: &str) -> (AgentReply, NewDelivery) {
        let reply = AgentReply {
            message: MessageWrite::internal(
                self.thread_id,
                MessageAuthorWrite::Agent(AgentAuthor {
                    agent_id: self.agent_a,
                    display_label: "Agent A".into(),
                }),
                "Answer",
                "Here is the answer",
                MessageDirection::Outbound,
                MessageRole::Agent,
                task.correlation_id,
            )
            .external_conversation(),
            also_in_threads: Vec::new(),
        };
        let mut delivery = delivery_fixture(
            &self.persistence,
            DeliveryFixtureRequest {
                task_id: Some(task.id),
                body: "Here is the answer",
                subject: "Answer",
                ..DeliveryFixtureRequest::new(self.company_id, self.channel_id, self.thread_id, key)
            },
        )
        .await
        .delivery;
        delivery.message_id = reply.message.id;
        delivery.correlation_id = task.correlation_id;
        (reply, delivery)
    }

    /// An outreach from `task` asking [`PARTNER`] alone, whose one answer meets the quorum.
    async fn outreach_request(
        &self,
        task: &BackgroundTask,
        lease: TaskLeaseRef,
        key: &str,
    ) -> CreateOutreachRequest {
        let question = delivery_fixture(
            &self.persistence,
            DeliveryFixtureRequest {
                task_id: Some(task.id),
                recipient: PARTNER,
                purpose: DeliveryPurpose::Outreach,
                ..DeliveryFixtureRequest::new(self.company_id, self.channel_id, self.thread_id, key)
            },
        )
        .await
        .delivery;
        CreateOutreachRequest {
            invocation: None,
            id: Uuid::new_v4(),
            lease,
            company_id: self.company_id,
            channel_id: self.channel_id,
            correlation_id: task.correlation_id,
            outreach_key: key.into(),
            required_threshold_percent: 100.0,
            expires_at: Utc::now() + chrono::Duration::hours(24),
            subject: "Question".into(),
            body: "Please respond".into(),
            targets: vec![OutreachTargetRequest {
                target: OutreachTargetIdentity::External {
                    identity: qualified_email_identity(PARTNER).unwrap(),
                },
                request: email_write(EmailMessageDraft {
                    id: Uuid::new_v4(),
                    thread_id: self.thread_id,
                    message_id: format!("<{key}@mailagents.test>").into(),
                    sender: "removal@acme.mailagents.test".into(),
                    recipients_to: vec![PARTNER.into()],
                    subject: "Question".into(),
                    clean_text_body: "Please respond".into(),
                    direction: MessageDirection::Outbound,
                    role: MessageRole::Agent,
                    ..EmailMessageDraft::default()
                }),
                delivery: question,
            }],
        }
    }

    /// A claimed task parked on a committed outreach to [`PARTNER`], and the match an answer from
    /// them would produce, with the association that answer arrived as.
    async fn waiting_on_partner(&self, key: &str) -> (BackgroundTask, OutreachReplyMatch, Uuid) {
        let (task, lease) = self.claimed("outreach_reply_race").await;
        let request = self.outreach_request(&task, lease, key).await;
        let outreach_id = request.id;
        assert!(
            self.persistence
                .create_outreach_and_pause(request)
                .await
                .unwrap()
                .suspended
        );
        let target_id = sqlx::query_scalar(
            "SELECT id FROM task_outreach_targets WHERE outreach_id = $1 AND email = $2",
        )
        .bind(outreach_id)
        .bind(PARTNER)
        .fetch_one(self.persistence.pool())
        .await
        .unwrap();
        let answer = self
            .persistence
            .create_message(&email_write(EmailMessageDraft {
                id: Uuid::new_v4(),
                thread_id: self.thread_id,
                message_id: format!("<answer-{key}@partner.test>").into(),
                sender: PARTNER.into(),
                subject: "Re: Question".into(),
                clean_text_body: "Confirmed".into(),
                direction: MessageDirection::Inbound,
                role: MessageRole::Human,
                ..EmailMessageDraft::default()
            }))
            .await
            .unwrap();
        let matched = OutreachReplyMatch {
            outreach_id,
            task_id: task.id,
            target_id,
            target_email: PARTNER.into(),
        };
        (task, matched, answer.id)
    }

    async fn task_counts(&self, task_id: Uuid) -> (i64, i64) {
        sqlx::query_as(
            r#"SELECT (SELECT count(*) FROM task_outreaches WHERE task_id = $1),
                      (SELECT count(*) FROM message_deliveries WHERE task_id = $1)"#,
        )
        .bind(task_id)
        .fetch_one(self.persistence.pool())
        .await
        .unwrap()
    }

    async fn outreach_state(&self, matched: &OutreachReplyMatch) -> (String, String) {
        sqlx::query_as(
            r#"SELECT outreach.status, target.status
                 FROM task_outreaches AS outreach
                 JOIN task_outreach_targets AS target ON target.outreach_id = outreach.id
                WHERE outreach.id = $1 AND target.id = $2"#,
        )
        .bind(matched.outreach_id)
        .bind(matched.target_id)
        .fetch_one(self.persistence.pool())
        .await
        .unwrap()
    }

    fn spawn_removal(&self) -> tokio::task::JoinHandle<AppResult<AssignmentRemoval>> {
        let pool = self.persistence.pool().clone();
        let request = self.request(&[self.agent_b]);
        tokio::spawn(
            async move { update_channel_within(&pool, request, RemovalBudget::DEFAULT).await },
        )
    }

    /// Take `task_id`'s row lock in a transaction of its own, parking any writer that needs it.
    async fn hold_task_row(&self, task_id: Uuid) -> sqlx::Transaction<'static, sqlx::Postgres> {
        let mut holder = self.persistence.pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM background_tasks WHERE id = $1 FOR UPDATE")
            .bind(task_id)
            .execute(&mut *holder)
            .await
            .unwrap();
        holder
    }
}

fn spawn_dispatch(
    pool: &sqlx::PgPool,
    lease: TaskLeaseRef,
    reply: AgentReply,
    delivery: NewDelivery,
) -> tokio::task::JoinHandle<DispatchCommit> {
    let persistence = PostgresPersistence::new(pool.clone());
    tokio::spawn(async move {
        persistence
            .commit_agent_dispatch(AgentDispatchCommit {
                lease,
                reply: &reply,
                deliveries: vec![delivery],
                review_candidate: None,
                payload: serde_json::json!({}),
                complete_outreach: false,
            })
            .await
            .unwrap()
    })
}

#[tokio::test]
async fn an_edit_that_omits_the_agent_list_keeps_the_agents_and_their_work() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;
    let task = f.enqueue(f.channel_id).await;
    let mut request = f.request(&[]);
    request.write.agent_ids = None;

    let removal = update_channel_within(&pool, request, RemovalBudget::DEFAULT)
        .await
        .unwrap();

    assert_eq!(removal, AssignmentRemoval::default());
    assert_eq!(f.assignments().await, vec![f.agent_a, f.agent_b]);
    assert_eq!(f.status(task).await, "pending");
    f.delete().await;
}

/// The database giving up on a lock, or the edit outliving its deadline, is a timeout that says
/// nothing changed -- not a database fault.
#[tokio::test]
async fn a_removal_that_cannot_take_its_locks_in_time_is_refused_as_a_timeout() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;
    let task = f.enqueue(f.channel_id).await;
    let mut admission = pool.begin().await.unwrap();
    acquire_channel_gate_on(
        &mut admission,
        ChannelGateKey::new(f.company_id, f.channel_id),
        ChannelGateAccess::Admit,
    )
    .await
    .unwrap();

    let impatient = RemovalBudget {
        lock_timeout: Duration::from_millis(100),
        ..RemovalBudget::DEFAULT
    };
    let refused = update_channel_within(&pool, f.request(&[f.agent_b]), impatient).await;
    assert!(
        matches!(&refused, Err(AppError::Timeout(message)) if message.contains("nothing was changed")),
        "{refused:?}"
    );

    let hurried = RemovalBudget {
        lock_timeout: Duration::from_secs(10),
        deadline: Duration::from_millis(300),
        ..RemovalBudget::DEFAULT
    };
    let refused = update_channel_within(&pool, f.request(&[f.agent_b]), hurried).await;
    assert!(
        matches!(&refused, Err(AppError::Timeout(message)) if message.contains("nothing was changed")),
        "{refused:?}"
    );

    admission.rollback().await.unwrap();
    assert_eq!(f.status(task).await, "pending");
    assert_eq!(f.assignments().await, vec![f.agent_a, f.agent_b]);
    f.delete().await;
}

/// A final dispatch that reaches the gate while the removal holds it publishes nothing: once the
/// removal commits, its lease fence finds the task stopped.
#[tokio::test]
async fn a_final_dispatch_waiting_on_the_removal_publishes_nothing() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;
    let (task, lease) = f.claimed("dispatch_race").await;
    let (reply, delivery) = f.final_reply(&task, "dispatch-loses").await;
    let message_id = reply.message.id;

    let mut removal = pool.begin().await.unwrap();
    update_channel_on(
        &mut removal,
        &f.request(&[f.agent_b]),
        RemovalBudget::DEFAULT,
    )
    .await
    .unwrap();
    let dispatch = spawn_dispatch(&pool, lease, reply, delivery);
    wait_until_a_backend_is_blocked(&pool).await;
    removal.commit().await.unwrap();

    assert!(matches!(dispatch.await.unwrap(), DispatchCommit::LeaseLost));
    assert_eq!(f.status(task.id).await, "stopped");
    assert_eq!(f.task_counts(task.id).await, (0, 0));
    let published: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE id = $1")
        .bind(message_id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(published, 0, "the stopped task's reply was never published");
}

/// A final dispatch already past the gate when the removal arrives commits first, and the removal
/// then withdraws the delivery it queued.
#[tokio::test]
async fn a_final_dispatch_ahead_of_the_removal_is_withdrawn_by_it() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;
    let (task, lease) = f.claimed("dispatch_race").await;
    let (reply, delivery) = f.final_reply(&task, "dispatch-wins").await;
    let delivery_id = delivery.id;

    // The dispatch takes the gate, then parks at its lease fence on the held row; the removal then
    // parks on the gate behind it.
    let row = f.hold_task_row(task.id).await;
    let dispatch = spawn_dispatch(&pool, lease, reply, delivery);
    wait_until_a_backend_is_blocked(&pool).await;
    let removal = f.spawn_removal();
    wait_until_backends_are_blocked(&pool, 2).await;
    row.rollback().await.unwrap();

    assert!(
        matches!(dispatch.await.unwrap(), DispatchCommit::Committed { .. }),
        "the dispatch held the gate first"
    );
    let removal = removal.await.unwrap().unwrap();
    assert_eq!(removal.stopped_tasks, 1);
    assert_eq!(removal.cancelled_deliveries, 1);
    assert_eq!(f.status(task.id).await, "stopped");
    assert_eq!(
        f.delivery(delivery_id).await,
        (
            "dead_letter".to_string(),
            Some("channel_agent_removed".to_string())
        )
    );
}

/// Asking outreach while the removal holds the gate queues nothing: the outreach waits, then its
/// lease fence finds the task stopped.
#[tokio::test]
async fn an_outreach_waiting_on_the_removal_queues_nothing() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;
    let (task, lease) = f.claimed("outreach_race").await;
    let request = f.outreach_request(&task, lease, "outreach-loses").await;

    let mut removal = pool.begin().await.unwrap();
    update_channel_on(
        &mut removal,
        &f.request(&[f.agent_b]),
        RemovalBudget::DEFAULT,
    )
    .await
    .unwrap();
    let outreach = {
        let persistence = PostgresPersistence::new(pool.clone());
        tokio::spawn(async move { persistence.create_outreach_and_pause(request).await })
    };
    wait_until_a_backend_is_blocked(&pool).await;
    removal.commit().await.unwrap();

    let refused = outreach.await.unwrap();
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(f.status(task.id).await, "stopped");
    assert_eq!(
        f.task_counts(task.id).await,
        (0, 0),
        "no outreach and no question for a stopped task"
    );
}

/// An outreach already past the gate commits first; the removal then cancels it, its target and
/// the question it queued.
#[tokio::test]
async fn an_outreach_ahead_of_the_removal_is_cancelled_by_it() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;
    let (task, lease) = f.claimed("outreach_race").await;
    let request = f.outreach_request(&task, lease, "outreach-wins").await;
    let outreach_id = request.id;
    let question = request.targets[0].delivery.id;

    let row = f.hold_task_row(task.id).await;
    let outreach = {
        let persistence = PostgresPersistence::new(pool.clone());
        tokio::spawn(async move { persistence.create_outreach_and_pause(request).await })
    };
    wait_until_a_backend_is_blocked(&pool).await;
    let removal = f.spawn_removal();
    wait_until_backends_are_blocked(&pool, 2).await;
    row.rollback().await.unwrap();

    assert!(outreach.await.unwrap().unwrap().suspended);
    let removal = removal.await.unwrap().unwrap();
    assert_eq!(removal.stopped_tasks, 1);
    assert_eq!(removal.cancelled_deliveries, 1);
    assert_eq!(f.status(task.id).await, "stopped");
    let target_id =
        sqlx::query_scalar("SELECT id FROM task_outreach_targets WHERE outreach_id = $1")
            .bind(outreach_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let matched = OutreachReplyMatch {
        outreach_id,
        task_id: task.id,
        target_id,
        target_email: PARTNER.into(),
    };
    assert_eq!(
        f.outreach_state(&matched).await,
        ("cancelled".to_string(), "cancelled".to_string())
    );
    assert_eq!(
        f.delivery(question).await,
        (
            "dead_letter".to_string(),
            Some("channel_agent_removed".to_string())
        )
    );
}

/// An answer that arrives while the removal holds the outreach is recorded late: it wakes nothing.
#[tokio::test]
async fn an_outreach_reply_waiting_on_the_removal_wakes_nothing() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;
    let (task, matched, answer) = f.waiting_on_partner("reply-loses").await;

    let mut removal = pool.begin().await.unwrap();
    let removed = update_channel_on(
        &mut removal,
        &f.request(&[f.agent_b]),
        RemovalBudget::DEFAULT,
    )
    .await
    .unwrap();
    assert_eq!(removed.stopped_tasks, 1);
    let reply = {
        let persistence = PostgresPersistence::new(pool.clone());
        let matched = matched.clone();
        tokio::spawn(async move { persistence.record_outreach_reply(&matched, answer).await })
    };
    wait_until_a_backend_is_blocked(&pool).await;
    removal.commit().await.unwrap();

    let progress = reply.await.unwrap().unwrap();
    assert_eq!(progress.status, OutreachStatus::Cancelled);
    assert_eq!(f.status(task.id).await, "stopped");
    let disposition: String =
        sqlx::query_scalar("SELECT disposition FROM task_outreach_replies WHERE outreach_id = $1")
            .bind(matched.outreach_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(disposition, "late");
}

/// An answer recorded before the removal reaches the outreach wakes the task; the removal then
/// stops the woken task and keeps the answer as history.
#[tokio::test]
async fn an_outreach_reply_ahead_of_the_removal_is_kept_and_its_task_stopped() {
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;
    let (task, matched, answer) = f.waiting_on_partner("reply-wins").await;

    let mut reply = pool.begin().await.unwrap();
    let progress = record_outreach_reply_on(&mut reply, &matched, answer)
        .await
        .unwrap();
    assert_eq!(progress.status, OutreachStatus::ThresholdMet);
    let removal = f.spawn_removal();
    wait_until_a_backend_is_blocked(&pool).await;
    reply.commit().await.unwrap();

    let removal = removal.await.unwrap().unwrap();
    assert_eq!(removal.stopped_tasks, 1);
    assert_eq!(f.status(task.id).await, "stopped");
    assert_eq!(
        f.outreach_state(&matched).await,
        ("cancelled".to_string(), "responded".to_string()),
        "the answer that arrived stays on record"
    );
}

/// A representative backlog: 5,000 unsettled tasks alongside 20,000 settled ones. The removal stops
/// exactly the unsettled work inside the default budget and refuses one task past a tighter one;
/// once it is done, the whole history contributes nothing to a later removal's working set.
#[tokio::test]
async fn a_five_thousand_task_backlog_is_removed_inside_the_default_budget() {
    const BACKLOG: i64 = 5_000;
    const HISTORY: i64 = 20_000;
    const QUEUED_RUNNING: usize = 50;
    const QUEUED_COMPLETED: usize = 10;
    let Some(database) = own_database().await else {
        return;
    };
    let pool = database.pool.clone();
    let f = fixture(&pool).await;

    // Agent A owns every row through the insert trigger, exactly as an enqueued task is owned.
    sqlx::query(
        r#"INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type, status)
           SELECT gen_random_uuid(), $1, $2, gen_random_uuid(), 'removal_load',
                  CASE WHEN n <= $3 THEN 'pending' ELSE 'completed' END
             FROM generate_series(1, $3 + $4) AS n"#,
    )
    .bind(f.company_id)
    .bind(f.channel_id)
    .bind(BACKLOG)
    .bind(HISTORY)
    .execute(&pool)
    .await
    .unwrap();
    for status in ["failed", "dead_letter", "pending_approval"] {
        sqlx::query(
            r#"UPDATE background_tasks SET status = $2
                WHERE id IN (SELECT id FROM background_tasks
                              WHERE company_id = $1 AND status = 'pending'
                              ORDER BY id LIMIT 1000)"#,
        )
        .bind(f.company_id)
        .bind(status)
        .execute(&pool)
        .await
        .unwrap();
    }
    let ids = |status: &'static str, limit: usize| {
        let pool = pool.clone();
        let company_id = f.company_id;
        async move {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM background_tasks WHERE company_id = $1 AND status = $2 ORDER BY id LIMIT $3",
            )
            .bind(company_id)
            .bind(status)
            .bind(limit as i64)
            .fetch_all(&pool)
            .await
            .unwrap()
        }
    };
    for (index, task) in ids("pending", QUEUED_RUNNING).await.into_iter().enumerate() {
        f.queue_delivery(task, &format!("load-running-{index}"))
            .await;
    }
    for (index, task) in ids("completed", QUEUED_COMPLETED)
        .await
        .into_iter()
        .enumerate()
    {
        f.queue_delivery(task, &format!("load-completed-{index}"))
            .await;
    }
    let affected = BACKLOG as usize + QUEUED_COMPLETED;
    let history_events: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM task_status_events AS event
             JOIN background_tasks AS task ON task.id = event.task_id
            WHERE task.company_id = $1 AND task.status = 'completed'"#,
    )
    .bind(f.company_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let one_short = RemovalBudget {
        max_tasks: affected - 1,
        ..RemovalBudget::DEFAULT
    };
    let refused = update_channel_within(&pool, f.request(&[f.agent_b]), one_short).await;
    assert!(matches!(refused, Err(AppError::Conflict(_))), "{refused:?}");

    let started = Instant::now();
    let removal = f.save(&[f.agent_b]).await.unwrap();
    let elapsed = started.elapsed();

    assert_eq!(removal.stopped_tasks, BACKLOG as u64);
    assert_eq!(
        removal.cancelled_deliveries,
        (QUEUED_RUNNING + QUEUED_COMPLETED) as u64
    );
    assert!(
        elapsed < RemovalBudget::DEFAULT.deadline,
        "the backlog took {elapsed:?}"
    );
    let by_status: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT status, count(*) FROM background_tasks
            WHERE company_id = $1 AND task_type = 'removal_load'
            GROUP BY status ORDER BY status"#,
    )
    .bind(f.company_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        by_status,
        vec![
            ("completed".to_string(), HISTORY),
            ("stopped".to_string(), BACKLOG)
        ]
    );
    let events_after: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM task_status_events AS event
             JOIN background_tasks AS task ON task.id = event.task_id
            WHERE task.company_id = $1 AND task.status = 'completed'"#,
    )
    .bind(f.company_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events_after, history_events, "settled work is left alone");

    // Twenty-five thousand settled tasks now, none of them in a working set: re-adding the agent
    // and removing it again fits a budget of zero tasks.
    f.save(&[f.agent_b, f.agent_a]).await.unwrap();
    let nothing = RemovalBudget {
        max_tasks: 0,
        ..RemovalBudget::DEFAULT
    };
    let again = update_channel_within(&pool, f.request(&[f.agent_b]), nothing)
        .await
        .unwrap();
    assert_eq!(again.stopped_tasks, 0);
    assert_eq!(again.removed_agent_ids, vec![f.agent_a]);
}
