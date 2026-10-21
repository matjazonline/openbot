//! Real-PostgreSQL fixture for 04.11 messaging: a company with a run channel (email-capable),
//! a second in-app channel, the owner as the user recipient, a published `message.*` step and
//! its claimed execution.
use super::*;
use crate::adapters::{protocols::email::EmailRenderer, workflow_message::MessageRuntime};
use crate::application::transport::{DeliveryComposer, TransportRenderers};
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::application::workflow::{
    activation::{ActivationRequest, WorkflowJobId},
    lease::{ClaimedWorkflow, LeasePolicy, WorkflowWorkerId},
    polling::WorkflowStepKind,
};
use crate::domain::entities::correlation::CorrelationId;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;

pub(super) const APP_DOMAIN: &str = "agents.test";

pub(super) fn lease() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(30)).unwrap()
}

pub(super) struct Setup {
    pub(super) fixture: Fixture,
    /// The run's channel: user emails and their threads are sent from and live in it.
    pub(super) channel: Uuid,
    /// A second channel, an in-app destination.
    pub(super) other: Uuid,
    /// A plain member with a principal and an email address: the user recipient.
    pub(super) user: Uuid,
    /// The company owner, who is also the run actor.
    pub(super) owner: Uuid,
    pub(super) claim: ClaimedWorkflow,
    pub(super) kind: WorkflowStepKind,
}
impl Setup {
    pub(super) fn pool(&self) -> &sqlx::PgPool {
        self.fixture.persistence.pool()
    }
    pub(super) fn runtime(&self) -> MessageRuntime {
        runtime(&self.fixture.persistence)
    }
    pub(super) fn scope(&self) -> ActivationRequest {
        self.claim.fence.scope
    }
}

pub(super) fn runtime(persistence: &PostgresPersistence) -> MessageRuntime {
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(EmailRenderer::new(APP_DOMAIN)))
            .unwrap(),
    );
    let composer = DeliveryComposer::new(renderers, Arc::new(persistence.clone()));
    MessageRuntime::new(persistence.clone(), composer, APP_DOMAIN.into(), lease())
}

pub(super) async fn new_channel(fixture: &Fixture, slug: &str) -> Uuid {
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

/// A company member with `role` and a principal; their address is `<id>@example.test`.
pub(in crate::adapters::persistence::workflow::tests) async fn member(
    fixture: &Fixture,
    role: &str,
) -> Uuid {
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

/// Destinations by role, resolved to ids once the fixture exists.
#[derive(Clone, Copy)]
pub(super) enum To {
    Channel,
    User,
    Owner,
}

/// A `message.send` step to `to`, published, bound, admitted in the run channel and claimed.
pub(super) async fn send(to: &[To]) -> Setup {
    Box::pin(setup_with_owner(|channel, other, user, owner| {
        let destinations: Vec<Value> = to
            .iter()
            .map(|to| match to {
                To::Channel => json!({"kind":"channel","id":other.to_string()}),
                To::User => json!({"kind":"user","id":user.to_string()}),
                To::Owner => json!({"kind":"user","id":owner.to_string()}),
            })
            .collect();
        let _ = channel;
        (
            "message.send",
            json!({"content":{"literal":{"subject":"Status","body":"The run finished."}},
                   "destinations":{"literal":destinations}}),
        )
    }))
    .await
}

pub(super) async fn setup_with_owner(
    step: impl FnOnce(Uuid, Uuid, Uuid, Uuid) -> (&'static str, Value),
) -> Setup {
    let fixture = Fixture::new().await;
    let channel = new_channel(&fixture, "runs").await;
    let other = new_channel(&fixture, "notices").await;
    let user = member(&fixture, "member").await;
    let owner = fixture.target.actor.user_id();
    let (kind, with) = step(channel, other, user, owner);
    let version = Box::pin(published(&fixture, kind, with)).await;
    let target = Box::pin(bound(&fixture, version)).await;
    let claim = Box::pin(claimed(&fixture, target, channel)).await;
    Setup {
        fixture,
        channel,
        other,
        user,
        owner,
        claim,
        kind: WorkflowStepKind(kind.into()),
    }
}

pub(super) async fn published(fixture: &Fixture, kind: &str, with: Value) -> VersionId {
    let p = &fixture.persistence;
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["steps"]["start"]["with"] = with;
    let directory = message::MessagePublicationDirectory {
        persistence: p.clone(),
    };
    let definitions =
        DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    publication.version
}

pub(super) async fn bound(fixture: &Fixture, version: VersionId) -> BindingTarget {
    let p = &fixture.persistence;
    let target = BindingTarget {
        company: fixture.target.company,
        actor: fixture.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let state = bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::new(),
        })
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target
}

pub(super) async fn admitted(
    fixture: &Fixture,
    target: BindingTarget,
    channel: Uuid,
    key: &str,
) -> ActivationRequest {
    admitted_as(
        fixture,
        target,
        RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        key,
    )
    .await
}

pub(super) async fn admitted_as(
    fixture: &Fixture,
    target: BindingTarget,
    association: RelatedAssociation,
    key: &str,
) -> ActivationRequest {
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let admission = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            company_id: company,
            actor: fixture.target.actor,
            association,
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: target.binding,
            idempotency_key: IdempotencyKey::parse(key).unwrap(),
            input: json!({}),
        })
        .await
        .unwrap();
    let run = match admission {
        AdmissionResult::Created(run) => run,
        other => panic!("{other:?}"),
    };
    let (execution, job): (Uuid, Uuid) = sqlx::query_as("SELECT execution.id,job.id FROM workflow_executions AS execution JOIN background_tasks AS job ON job.workflow_execution_id=execution.id WHERE execution.run_id=$1")
        .bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    ActivationRequest {
        company,
        run,
        execution: ExecutionId::new(execution),
        job: WorkflowJobId(job),
    }
}

pub(super) async fn claimed(
    fixture: &Fixture,
    target: BindingTarget,
    channel: Uuid,
) -> ClaimedWorkflow {
    let scope = admitted(fixture, target, channel, "message-run").await;
    fixture
        .persistence
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap()
}

/// Every row a message invocation can own, scoped to the run.
#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct Rows {
    pub(super) markers: i64,
    pub(super) actions: i64,
    pub(super) destinations: i64,
    pub(super) deliveries: i64,
    pub(super) receipts: i64,
    pub(super) waits: i64,
    pub(super) messages: i64,
}
pub(super) async fn rows(pool: &sqlx::PgPool, run: RunId) -> Rows {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_action_dispatches WHERE run_id=$1) AS markers,
                (SELECT count(*) FROM workflow_message_actions WHERE run_id=$1) AS actions,
                (SELECT count(*) FROM workflow_message_destinations WHERE run_id=$1) AS destinations,
                (SELECT count(*) FROM workflow_message_destinations WHERE run_id=$1 AND delivery_id IS NOT NULL) AS deliveries,
                (SELECT count(*) FROM workflow_action_receipts WHERE run_id=$1) AS receipts,
                (SELECT count(*) FROM workflow_waits WHERE run_id=$1) AS waits,
                (SELECT count(*) FROM messages AS message JOIN workflow_message_destinations AS target
                   ON target.company_id=message.company_id AND target.message_id=message.id WHERE target.run_id=$1) AS messages",
    )
    .bind(run.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Run, attempt, job and wait state of the claimed execution.
#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct State {
    pub(super) run: String,
    pub(super) waiting: Option<String>,
    pub(super) job: String,
    pub(super) attempt: String,
    pub(super) stop: Option<String>,
    pub(super) wait: Option<String>,
}
pub(super) async fn state(pool: &sqlx::PgPool, scope: ActivationRequest) -> State {
    sqlx::query_as(
        "SELECT run.state AS run, run.waiting_reason AS waiting, job.status AS job,
                (SELECT attempt.status FROM task_attempts AS attempt WHERE attempt.task_id=job.id ORDER BY attempt_number DESC LIMIT 1) AS attempt,
                (SELECT attempt.stop_reason FROM task_attempts AS attempt WHERE attempt.task_id=job.id ORDER BY attempt_number DESC LIMIT 1) AS stop,
                (SELECT wait.state FROM workflow_waits AS wait WHERE wait.execution_id=$2 ORDER BY (wait.state='waiting') DESC, wait.created_at DESC LIMIT 1) AS wait
           FROM workflow_runs AS run JOIN background_tasks AS job ON job.id=$3 WHERE run.id=$1",
    )
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// The single email delivery of the run, by ordinal.
pub(super) async fn delivery(pool: &sqlx::PgPool, run: RunId, ordinal: i16) -> Uuid {
    sqlx::query_scalar(
        "SELECT delivery_id FROM workflow_message_destinations WHERE run_id=$1 AND ordinal=$2",
    )
    .bind(run.as_uuid())
    .bind(ordinal)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Drive a delivery's provider outcome the way the delivery engine's SQL does, without a
/// provider: claim, begin, and complete (or leave the request started).
pub(super) async fn deliver(pool: &sqlx::PgPool, delivery: Uuid, outcome: &str) {
    sqlx::query(
        "UPDATE message_deliveries SET status='sending', execution_id=gen_random_uuid(), owner_worker_id=gen_random_uuid(), \
           locked_at=clock_timestamp(), lock_expires_at=clock_timestamp()+interval '1 minute' WHERE id=$1",
    )
    .bind(delivery)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE message_delivery_parts SET status='sending', request_started_at=clock_timestamp(), attempt_count=attempt_count+1 WHERE delivery_id=$1")
        .bind(delivery).execute(pool).await.unwrap();
    let (part, parent) = match outcome {
        "delivered" => ("delivered", "delivered"),
        "dead" => ("dead", "dead_letter"),
        "unknown" => ("outcome_unknown", "outcome_unknown"),
        other => panic!("{other}"),
    };
    sqlx::query("UPDATE message_delivery_parts SET status=$2, delivered_at=CASE WHEN $2='delivered' THEN clock_timestamp() END, last_error_class=CASE WHEN $2='dead' THEN 'invalid_payload' END, last_error_detail=NULL WHERE delivery_id=$1")
        .bind(delivery).bind(part).execute(pool).await.unwrap();
    sqlx::query("UPDATE message_deliveries SET status=$2, attempt_count=attempt_count+1, delivered_at=CASE WHEN $2='delivered' THEN clock_timestamp() END, \
           last_error_class=CASE WHEN $2='dead_letter' THEN 'invalid_payload' WHEN $2='outcome_unknown' THEN 'timeout' END, last_error_detail=NULL, \
           execution_id=NULL, owner_worker_id=NULL, locked_at=NULL, lock_expires_at=NULL WHERE id=$1")
        .bind(delivery).bind(parent).execute(pool).await.unwrap();
}

pub(super) async fn action_state(pool: &sqlx::PgPool, run: RunId) -> String {
    sqlx::query_scalar("SELECT state FROM workflow_message_actions WHERE run_id=$1")
        .bind(run.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Arguments of a `message.send` to the given destinations.
pub(super) fn send_arguments(destinations: &[(&str, Uuid)], body: &str) -> Value {
    json!({"content":{"subject":"Status","body":body},
           "destinations":destinations.iter().map(|(kind, id)| json!({"kind":kind,"id":id.to_string()})).collect::<Vec<_>>()})
}

/// A direct model-tool `message.send` under the claimed fence.
pub(super) async fn tool(
    setup: &Setup,
    call: &str,
    arguments: Value,
) -> AppResult<crate::application::workflow::message::MessageObservation> {
    setup
        .runtime()
        .call_tool(
            setup.claim.fence,
            crate::application::workflow::message::MessageToolCall {
                kind: crate::application::workflow::message::MessageKind::Send,
                arguments,
                call: crate::application::workflow::actions::ModelToolCallId::parse(call).unwrap(),
            },
        )
        .await
}

/// The delivery of the run's most recent message action.
pub(super) async fn latest_delivery(pool: &sqlx::PgPool, run: RunId) -> Uuid {
    sqlx::query_scalar("SELECT destination.delivery_id FROM workflow_message_destinations AS destination JOIN workflow_message_actions AS action ON action.company_id=destination.company_id AND action.invocation_id=destination.invocation_id WHERE action.run_id=$1 AND destination.delivery_id IS NOT NULL ORDER BY action.created_at DESC LIMIT 1")
        .bind(run.as_uuid()).fetch_one(pool).await.unwrap()
}

/// D4 continuation of a parked model-tool delivery: settle it, consume its receipt, let the
/// wait reopen the parked job, and reclaim that job under a fresh fence.
pub(super) async fn continue_delivery(setup: &mut Setup, delivery: Uuid, outcome: &str) {
    use crate::application::workflow::{
        lease::{WorkflowLeases, WorkflowWorkerId},
        waits::{WaitProgress, WorkflowWaits},
    };
    let p = setup.fixture.persistence.clone();
    deliver(p.pool(), delivery, outcome).await;
    assert_eq!(
        p.consume_message_receipts(setup.scope(), lease())
            .await
            .unwrap(),
        1
    );
    assert!(matches!(
        p.resume_wait(setup.scope()).await.unwrap(),
        WaitProgress::Continued
    ));
    setup.claim = p
        .claim_io(setup.scope(), WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the continued job is claimable");
}

pub(super) async fn invocations(pool: &sqlx::PgPool, run: RunId) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT invocation_id FROM workflow_message_actions WHERE run_id=$1 ORDER BY created_at, invocation_id",
    )
    .bind(run.as_uuid())
    .fetch_all(pool)
    .await
    .unwrap()
}

pub(super) async fn effect_state(pool: &sqlx::PgPool, invocation: Uuid) -> (String, bool) {
    sqlx::query_as(
        "SELECT effect_state, evidence_conflict FROM workflow_action_effect_states WHERE invocation_id=$1",
    )
    .bind(invocation)
    .fetch_one(pool)
    .await
    .unwrap()
}

pub(super) async fn retry_safe(pool: &sqlx::PgPool, scope: ActivationRequest) -> Option<bool> {
    sqlx::query_scalar("SELECT workflow_action_retry_safe($1,$2)")
        .bind(scope.company.as_uuid())
        .bind(scope.execution.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Every receipt event of the run, as `(ordinal, old, new)`.
pub(super) async fn events(pool: &sqlx::PgPool, run: RunId) -> Vec<(i16, String, String)> {
    sqlx::query_as(
        "SELECT ordinal, old_status, status FROM workflow_message_delivery_events WHERE run_id=$1 ORDER BY seq",
    )
    .bind(run.as_uuid())
    .fetch_all(pool)
    .await
    .unwrap()
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct DeliveryRow {
    pub(super) status: String,
    pub(super) attempt_count: i32,
    pub(super) last_error_class: Option<String>,
    pub(super) cancellation_reason: Option<String>,
    pub(super) part: String,
}
pub(super) async fn delivery_row(pool: &sqlx::PgPool, delivery: Uuid) -> DeliveryRow {
    sqlx::query_as(
        "SELECT delivery.status, delivery.attempt_count, delivery.last_error_class, delivery.cancellation_reason, part.status AS part \
         FROM message_deliveries AS delivery JOIN message_delivery_parts AS part ON part.delivery_id=delivery.id WHERE delivery.id=$1",
    )
    .bind(delivery)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Cancel the run through the durable control owner.
pub(super) async fn cancel(setup: &Setup, key: &str) {
    let p = &setup.fixture.persistence;
    let scope = setup.scope();
    let head = p.head(scope.company, scope.run).await.unwrap().unwrap();
    p.cancel(CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: setup.fixture.target.actor,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: head.revision,
    })
    .await
    .unwrap();
}

/// Claim the one queued delivery `delivery` for a delivery worker.
pub(super) async fn claim(
    p: &PostgresPersistence,
    delivery: Uuid,
) -> crate::transport::ClaimedDelivery {
    use crate::transport::DeliveryQueue;
    // Only this delivery: every other claimable one is held back for the claim and restored.
    let held: Vec<(Uuid, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "UPDATE message_deliveries AS delivery SET available_at=clock_timestamp()+interval '1 day' \
           FROM message_deliveries AS before WHERE before.id=delivery.id AND delivery.id<>$1 \
             AND delivery.status IN ('pending','retryable') RETURNING delivery.id, before.available_at",
    )
    .bind(delivery)
    .fetch_all(p.pool())
    .await
    .unwrap();
    let claimed = p
        .claim_deliveries(
            crate::transport::WorkerId::random(),
            Duration::from_secs(60),
            50,
        )
        .await
        .unwrap();
    for (id, available) in held {
        sqlx::query("UPDATE message_deliveries SET available_at=$2 WHERE id=$1")
            .bind(id)
            .bind(available)
            .execute(p.pool())
            .await
            .unwrap();
    }
    assert_eq!(
        claimed.len(),
        1,
        "exactly the requested delivery was claimed"
    );
    claimed
        .into_iter()
        .find(|claimed| claimed.lease.row.as_uuid() == delivery)
        .expect("the delivery is claimable")
}
