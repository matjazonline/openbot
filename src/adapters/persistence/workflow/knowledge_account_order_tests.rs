use super::account_support::*;
use super::*;
use crate::adapters::persistence::workflow::tests::approval_test_support::composer;
use crate::adapters::protocols::email::EmailRenderer;
use crate::adapters::workflow_http::NoLocal;
use crate::application::services::delivery_worker::DeliveryWorker;
use crate::application::workflow::polling::WorkflowStepKind;
use crate::entities::value_objects::EmailAddress;
use crate::transport::{
    DeliveryRecord, ProviderSendOutcome, RenderedPart, TransportRegistry, TransportSender,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

#[derive(Clone)]
struct RoutesGate {
    persistence: PostgresPersistence,
    prepared: Arc<Notify>,
    resume: Arc<Notify>,
}

#[async_trait]
impl MessageActionPreparation for RoutesGate {
    async fn message_step_request(
        &self,
        claim: &ClaimedWorkflow,
        kind: &WorkflowStepKind,
    ) -> AppResult<ActionRequest> {
        self.persistence.message_step_request(claim, kind).await
    }
    async fn message_tool_request(
        &self,
        scope: ActionScope,
        call: &MessageToolCall,
    ) -> AppResult<ActionRequest> {
        self.persistence.message_tool_request(scope, call).await
    }
    async fn message_routes(
        &self,
        request: &ActionRequest,
        arguments: &MessageArguments,
    ) -> AppResult<MessageRoutes> {
        let routes = self.persistence.message_routes(request, arguments).await?;
        self.prepared.notify_one();
        self.resume.notified().await;
        Ok(routes)
    }
}

#[async_trait]
impl ActionIntents for RoutesGate {
    async fn prepare(
        &self,
        action: &FrozenAction,
        call: Option<&ModelToolCallId>,
    ) -> AppResult<ActionIntent> {
        self.persistence.prepare(action, call).await
    }
}

type GatedRuntime = MessageActions<
    RoutesGate,
    crate::adapters::persistence::workflow::action_dispatch::PostgresActionDispatch<
        crate::adapters::persistence::workflow::message::MessageActionAuthority,
        NoLocal,
    >,
>;

struct PreparedGate {
    runtime: GatedRuntime,
    prepared: Arc<Notify>,
    resume: Arc<Notify>,
}

fn gated_runtime(setup: &Setup) -> PreparedGate {
    let p = &setup.fixture.persistence;
    let gate = RoutesGate {
        persistence: p.clone(),
        prepared: Arc::new(Notify::new()),
        resume: Arc::new(Notify::new()),
    };
    let dispatch =
        crate::adapters::persistence::workflow::action_dispatch::PostgresActionDispatch::new(
            p.clone(),
            crate::adapters::persistence::workflow::message::MessageActionAuthority,
            NoLocal,
        );
    let prepared = gate.prepared.clone();
    let resume = gate.resume.clone();
    let runtime = MessageActions::new(gate, dispatch, composer(p), APP_DOMAIN.into(), lease());
    PreparedGate {
        runtime,
        prepared,
        resume,
    }
}

#[tokio::test]
async fn knowledge_run_level_confirmed_address_writer_first_refuses_the_real_prepared_publication()
{
    let setup = Box::pin(send(&[To::User])).await;
    let old = EmailAddress::from(verified_route_recipient(&setup).await);
    raise(&setup, AccessLevel::L5).await;
    let gated = gated_runtime(&setup);
    let publication = gated.runtime.execute(&setup.kind, &setup.claim);
    tokio::pin!(publication);
    tokio::select! {
        _=gated.prepared.notified()=>{},
        _=publication.as_mut()=>panic!("real routes must be prepared before publication resumes"),
    }
    confirm_address(&setup, &old).await;
    gated.resume.notify_one();
    let outcome = publication.await;
    let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
        panic!("the real runtime must refuse its retired prepared route before any effect")
    };
    assert_eq!(failure.failure.code().as_str(), "message.rejected");
    assert_eq!(failure.failure.class(), FailureClass::Terminal);
    assert!(
        failure.failure.knowledge_refusal().is_none(),
        "route refusal is distinct from the release-predicate seam"
    );
    assert_no_effects(&setup).await;
}

#[tokio::test]
async fn knowledge_run_level_publication_first_serializes_address_change_and_worker_refuses_retired_email()
 {
    let setup = Box::pin(send(&[To::User])).await;
    let old = EmailAddress::from(verified_route_recipient(&setup).await);
    raise(&setup, AccessLevel::L5).await;
    let gate = member_gate(&setup).await;
    let runtime = setup.runtime();
    let publication = runtime.execute(&setup.kind, &setup.claim);
    tokio::pin!(publication);
    let mut observer = setup.pool().acquire().await.unwrap();
    tokio::select! {
        _=member_route_wait(&mut observer,gate.backend)=>{},
        _=publication.as_mut()=>panic!("real publication must hold company and reach its member gate"),
    }
    let publisher: i32 = sqlx::query_scalar(
        "SELECT activity.pid FROM pg_stat_activity AS activity \
        WHERE $1=ANY(pg_blocking_pids(activity.pid)) AND activity.wait_event_type='Lock' \
          AND activity.query LIKE 'SELECT principal.id, account.email FROM company_members%'",
    )
    .bind(gate.backend)
    .fetch_one(&mut *observer)
    .await
    .unwrap();
    drop(observer);
    let change = confirm_address(&setup, &old);
    tokio::pin!(change);
    tokio::select! {
        _=account_writer_wait(setup.pool(),publisher)=>{},
        _=change.as_mut()=>panic!("real address writer cannot pass the publication's account/company ownership"),
    }
    gate.transaction.rollback().await.unwrap();
    let (outcome, _) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(publication, change)
    })
    .await
    .expect("publication and subsequent address mutation both make progress");
    assert!(
        matches!(outcome, WorkflowHandlerOutcome::Suspended(_)),
        "publication wins this controlled order"
    );
    let counts = rows(setup.pool(), setup.scope().run).await;
    assert_eq!(counts.markers, 1);
    assert_eq!(counts.deliveries, 1);
    assert_eq!(counts.messages, 1);
    assert_worker_refuses(&setup, &old).await;
}

#[derive(Default)]
struct ProviderCounter(AtomicUsize);

#[async_trait]
impl TransportSender for ProviderCounter {
    fn transport(&self) -> crate::entities::transport::TransportKind {
        crate::entities::transport::TransportKind::Email
    }
    async fn send(&self, _: &DeliveryRecord, _: &RenderedPart) -> ProviderSendOutcome {
        self.0.fetch_add(1, Ordering::SeqCst);
        ProviderSendOutcome::Delivered { provider_key: None }
    }
}

async fn assert_worker_refuses(setup: &Setup, old: &EmailAddress) {
    // Setup::Fixture::new owns its database, so this worker may sweep the real whole queue.
    let provider = Arc::new(ProviderCounter::default());
    let registry = TransportRegistry::new()
        .register(Arc::new(EmailRenderer::new(APP_DOMAIN)), provider.clone())
        .unwrap();
    let worker = DeliveryWorker::new(
        Arc::new(setup.fixture.persistence.clone()),
        Arc::new(registry),
    );
    let (_shutdown, mut receiver) = tokio::sync::broadcast::channel(1);
    assert_eq!(
        worker.drain_once(&mut receiver).await.unwrap(),
        1,
        "the actual worker consumes this queued delivery"
    );
    assert_eq!(
        provider.0.load(Ordering::SeqCst),
        0,
        "the retired address must never reach the actual transport boundary"
    );
    let id = delivery(setup.pool(), setup.scope().run, 0).await;
    let row = delivery_row(setup.pool(), id).await;
    assert_eq!(row.status, "dead_letter");
    assert_eq!(row.attempt_count, 0);
    assert_eq!(
        row.cancellation_reason.as_deref(),
        Some("recipient_revoked")
    );
    assert_eq!(row.part, "dead");
    let facts: Value = sqlx::query_scalar("SELECT jsonb_build_object('address',target.recipient_address, \
        'started',part.request_started_at IS NOT NULL) FROM workflow_message_destinations AS target \
        JOIN message_delivery_parts AS part ON part.delivery_id=target.delivery_id WHERE target.delivery_id=$1")
        .bind(id).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(facts, json!({"address":old.as_str(),"started":false}));
}

async fn maximum_user_send() -> Setup {
    let fixture = Box::pin(Fixture::new()).await;
    let channel = new_channel(&fixture, "maximum-users").await;
    let other = new_channel(&fixture, "unused").await;
    let mut users = Vec::new();
    for _ in 0..crate::application::workflow::registry::MAX_MESSAGE_DESTINATIONS {
        users.push(member(&fixture, "member").await);
    }
    users.sort_unstable();
    let user = *users.last().unwrap();
    let owner = fixture.target.actor.user_id();
    let destinations: Vec<Value> = users
        .iter()
        .map(|user| json!({"kind":"user","id":user}))
        .collect();
    let with = json!({"content":{"literal":{"subject":"Maximum recipients","body":"Public message"}},
        "destinations":{"literal":destinations}});
    let version = Box::pin(published(&fixture, "message.send", with)).await;
    let target = Box::pin(bound(&fixture, version)).await;
    let claim = Box::pin(claimed(&fixture, target, channel)).await;
    Setup {
        fixture,
        channel,
        other,
        user,
        owner,
        claim,
        kind: WorkflowStepKind("message.send".into()),
    }
}

#[tokio::test]
async fn knowledge_run_level_maximum_twenty_user_publication_prelocks_every_recipient_before_company()
 {
    let setup = Box::pin(maximum_user_send()).await;
    let mut gate = setup.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(setup.user)
        .execute(&mut *gate)
        .await
        .unwrap();
    let runtime = setup.runtime();
    let publication = runtime.execute(&setup.kind, &setup.claim);
    tokio::pin!(publication);
    let blocked = async {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity AS activity \
                    WHERE $1=ANY(pg_blocking_pids(activity.pid)) AND activity.wait_event_type='Lock' \
                      AND activity.query LIKE 'SELECT id FROM users WHERE id=ANY%FOR SHARE%')")
                    .bind(blocker).fetch_one(setup.pool()).await.unwrap();
                if waiting { return; }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }).await.expect("the last of twenty real recipients participates in the sorted account prefix");
    };
    tokio::select! {
        _=blocked=>{},
        _=publication.as_mut()=>panic!("publication must lock the last actual recipient before continuing"),
    }
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
        .bind(setup.scope().company.as_uuid())
        .execute(&mut *gate)
        .await
        .expect("publication waiting on a recipient must not already own company");
    gate.rollback().await.unwrap();
    assert!(matches!(
        publication.await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let counts = rows(setup.pool(), setup.scope().run).await;
    assert_eq!(counts.messages, 20);
    assert_eq!(counts.deliveries, 20);
    assert_eq!(counts.destinations, 20);
}
