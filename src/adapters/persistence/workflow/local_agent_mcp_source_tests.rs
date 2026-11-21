//! Actual Workflow-origin native ingress, with a distinct application-message requester.
use super::*;
use crate::app_error::AppResult;
use crate::application::agent::{AgentToolOrigin, ToolCallOrigin};
use crate::application::ports::thread::{MessageAuthorWrite, MessageWrite, ThreadPersistence};
use crate::application::workflow::RelatedChannelId;
use crate::application::workflow::actions::{ActionScope, FrozenAction};
use crate::application::workflow::message_admission::{MessageAdmissions, MessageIntentWorkerId};
use crate::domain::workflow::{MessageTrigger, WorkflowBindingId, WorkflowId};
use crate::entities::agent_capability::{AgentBindingId, AgentRevisionId, DependencyManifestId};
use crate::entities::message::{MessageDirection, MessageRole};
use crate::entities::transport::PrincipalId;
use std::future::Future;
#[path = "local_agent_mcp_entry_tests.rs"]
mod entry_tests;

struct Source {
    local: Setup,
    native: NativeTurn,
    requester: Uuid,
    revision: Uuid,
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct EffectState {
    calls: i64,
    brokers: i64,
    runs: i64,
    intents: i64,
    dispatches: i64,
}

#[derive(sqlx::FromRow)]
struct AdmittedSource {
    run: Uuid,
    execution: Uuid,
    job: Uuid,
}

async fn effects(s: &Setup) -> EffectState {
    sqlx::query_as(
        "SELECT
        (SELECT count(*) FROM native_mcp_calls WHERE company_id=$1) AS calls,
        (SELECT count(*) FROM native_mcp_brokers WHERE company_id=$1) AS brokers,
        (SELECT count(*) FROM workflow_runs WHERE company_id=$1) AS runs,
        (SELECT count(*) FROM workflow_action_intents WHERE company_id=$1) AS intents,
        (SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1) AS dispatches",
    )
    .bind(s.scope.company.as_uuid())
    .fetch_one(s.fixture.persistence.pool())
    .await
    .unwrap()
}

async fn source() -> Source {
    let mut local = Box::pin(Setup::new()).await;
    configure(&mut local, "http://127.0.0.1:9").await;
    Box::pin(start_source(local)).await
}

async fn start_source(mut local: Setup) -> Source {
    let p = &local.fixture.persistence;
    let requester: Uuid = sqlx::query_scalar(
        "SELECT member.user_id FROM principals AS member_principal JOIN LATERAL membership_of(member_principal.user_id, member_principal.company_id) AS member ON true WHERE member_principal.company_id=$1 AND role='admin' LIMIT 1",
    )
    .bind(local.scope.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    local.scope = Box::pin(admit_application_request(&local, requester)).await;
    let claim = local.claim().await;
    local.park(&claim).await;
    let native = local.native_started().await;
    let revision: Uuid = sqlx::query_scalar(
        "SELECT agent_revision_id FROM local_agent_sessions WHERE company_id=$1 AND workflow_run_id=$2",
    ).bind(local.scope.company.as_uuid()).bind(local.scope.run.as_uuid())
        .fetch_one(local.fixture.persistence.pool()).await.unwrap();
    let mut tx = local.fixture.persistence.pool().begin().await.unwrap();
    let actual = crate::adapters::persistence::workflow::provisioning::requester_on(
        &mut tx,
        ActionScope {
            company: local.scope.company,
            run: local.scope.run,
            execution: local.scope.execution,
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(actual.user_id(), requester);
    assert_ne!(requester, local.fixture.target.actor.user_id());
    tx.rollback().await.unwrap();
    Source {
        local,
        native,
        requester,
        revision,
    }
}

async fn admit_application_request(s: &Setup, requester: Uuid) -> ActivationRequest {
    let fixture = Fixture {
        _db: s.fixture._db.clone(),
        persistence: s.fixture.persistence.clone(),
        target: DefinitionTarget {
            workflow: WorkflowId::new(Uuid::new_v4()),
            ..s.fixture.target
        },
    };
    let p = &fixture.persistence;
    let mut source: Value = serde_json::from_str(&registry::example(KIND).unwrap().source).unwrap();
    source["workflow_id"] = json!(fixture.target.workflow);
    source["input_schema"] = json!(true);
    source["steps"]["start"]["with"] = json!({
        "profile_id":{"literal":s.profile.id},"prompt":{"literal":"application requester native task"}});
    let version = super::super::graph_support::publish(&fixture, source).await;
    let channel =
        super::super::super::message_tests::support::new_channel(&fixture, "native-requester")
            .await;
    let thread = p
        .create_thread(
            fixture.target.company.as_uuid(),
            crate::entities::address::AddressTarget::Desk(channel),
            "Native request",
            &[],
        )
        .await
        .unwrap()
        .id;
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
            association: RelatedAssociation::Channel(RelatedChannelId::new(channel)),
            version,
            params: json!({}),
            resources: Default::default(),
            run_as: None,
            message_trigger: Some(MessageTrigger::Direct),
            escalation: None,
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
    admit_message(&fixture, target.binding, channel, thread, requester).await
}

async fn admit_message(
    f: &Fixture,
    binding: WorkflowBindingId,
    channel: Uuid,
    thread: Uuid,
    requester: Uuid,
) -> ActivationRequest {
    let p = &f.persistence;
    let message = p
        .create_message(&MessageWrite::internal(
            thread,
            MessageAuthorWrite::Principal(PrincipalId::new(f.principal(requester).await)),
            "Native request",
            "Run the company tool",
            MessageDirection::Inbound,
            MessageRole::Human,
            crate::domain::entities::correlation::CorrelationId::new(),
        ))
        .await
        .unwrap()
        .canonical_id;
    let intent = Uuid::new_v4();
    // This is the application-message admission fixture, not a forged native action/origin.
    sqlx::query("INSERT INTO workflow_message_intents(company_id,id,message_id,channel_id,thread_id,
        role,mentioned,admissible,origin,responder_principal_id,hop_count,trace_targets,delivery_policy,correlation_id)
        SELECT $1,$2,message.id,$3,$4,'to',false,true,'application',message.author_principal_id,0,'[]','deliver',$2
        FROM messages AS message WHERE message.company_id=$1 AND message.id=$5")
        .bind(f.target.company.as_uuid()).bind(intent).bind(channel).bind(thread).bind(message.as_uuid())
        .execute(p.pool()).await.unwrap();
    let admissions = MessageAdmissions::new(
        p.clone(),
        WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight),
        p.clone(),
    );
    let claims = admissions
        .claim(MessageIntentWorkerId(Uuid::new_v4()), 1)
        .await
        .unwrap();
    assert_eq!(
        claims.len(),
        1,
        "the fixture owns this database's only message intent"
    );
    for claim in claims {
        admissions.process(claim).await.unwrap();
    }
    let outcome: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('outcome',outcome,'reason',reason) FROM workflow_message_intent_bindings
         WHERE company_id=$1 AND intent_id=$2 AND binding_id=$3",
    ).bind(f.target.company.as_uuid()).bind(intent).bind(binding.as_uuid())
        .fetch_optional(p.pool()).await.unwrap();
    assert!(
        outcome
            .as_ref()
            .is_some_and(|value| value["outcome"] == "admitted"),
        "the genuine application intent must admit the native workflow: {outcome:?}"
    );
    let admitted: AdmittedSource = sqlx::query_as(
        "SELECT run.id AS run,execution.id AS execution,job.id AS job FROM workflow_runs AS run
         JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
         WHERE run.company_id=$1 AND run.binding_id=$2 AND run.trigger_id=$3",
    ).bind(f.target.company.as_uuid()).bind(binding.as_uuid()).bind(intent).fetch_one(p.pool()).await.unwrap();
    ActivationRequest {
        company: f.target.company,
        run: RunId::new(admitted.run),
        execution: ExecutionId::new(admitted.execution),
        job: WorkflowJobId(admitted.job),
    }
}

#[derive(Clone, Copy, Debug)]
enum Owner {
    Account(Uuid),
    Revision(Uuid),
    Run(Uuid),
}

fn owners(source: &Source) -> [Owner; 3] {
    [
        Owner::Account(source.local.fixture.target.actor.user_id()),
        Owner::Account(source.requester),
        Owner::Revision(source.revision),
    ]
}

async fn competitor(source: &Source, owner: Owner) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut tx = source
        .local
        .fixture
        .persistence
        .pool()
        .begin()
        .await
        .unwrap();
    match owner {
        Owner::Account(id) => {
            sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        Owner::Revision(id) => {
            sqlx::query("SELECT id FROM agent_revisions WHERE company_id=$1 AND id=$2 FOR UPDATE")
                .bind(source.local.scope.company.as_uuid())
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        Owner::Run(id) => {
            sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
                .bind(source.local.scope.company.as_uuid())
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    tx
}

async fn busy<T>(future: impl Future<Output = AppResult<T>>) {
    let error = tokio::time::timeout(Duration::from_secs(2), future)
        .await
        .expect("native authority must never wait for this owner")
        .err()
        .expect("busy owner must refuse");
    assert!(
        matches!(&error, AppError::ServiceUnavailable(_)),
        "{error:?}"
    );
}

async fn ingress(source: &Source, owner: Owner) -> NativeMcpResponse {
    let s = &source.local;
    let p = &s.fixture.persistence;
    let request = request(s, &source.native);
    let authentication = NativeBundleRequest {
        verifier: request.verifier,
        identity: request.identity,
        operation: request.operation,
    };
    let before = effects(s).await;
    let competitor = competitor(source, owner).await;
    busy(p.authorize_native_mcp(&authentication)).await;
    busy(p.native_mcp(&request)).await;
    assert_eq!(
        effects(s).await,
        before,
        "busy ingress must roll back every new owner/effect"
    );
    competitor.rollback().await.unwrap();
    p.authorize_native_mcp(&authentication).await.unwrap();
    p.native_mcp(&request).await.unwrap()
}

async fn live(source: &Source, claim: &ClaimedWorkflow, owner: Owner) {
    let s = &source.local;
    let p = &s.fixture.persistence;
    let before = effects(s).await;
    let competitor = competitor(source, owner).await;
    busy(p.mcp_step_request(claim, &WorkflowStepKind("mcp.call".into()))).await;
    assert_eq!(
        effects(s).await,
        before,
        "busy live preparation must leave no new state"
    );
    competitor.rollback().await.unwrap();
    let prepared = p
        .mcp_step_request(claim, &WorkflowStepKind("mcp.call".into()))
        .await
        .unwrap();
    assert_eq!(
        prepared.arguments,
        json!({"query":"from originating workflow"})
    );
    assert_eq!(prepared.context.actor, s.fixture.target.actor.user_id());
    assert!(p.validate_io(claim.fence, lease()).await.unwrap());
}

#[tokio::test]
async fn native_mcp_workflow_actor_requester_and_revision_contention_rolls_back_and_retries() {
    let source = Box::pin(source()).await;
    let mut admitted = None;
    for owner in owners(&source) {
        let response = Box::pin(ingress(&source, owner)).await;
        if let Some(original) = admitted {
            assert_eq!(response.run_id, original);
        }
        admitted = Some(response.run_id);
    }
    let s = &source.local;
    let run = admitted.unwrap();
    let revision: Uuid = sqlx::query_scalar("SELECT broker.agent_revision_id FROM native_mcp_calls AS call
        JOIN native_mcp_brokers AS broker ON broker.company_id=call.company_id AND broker.id=call.broker_id
        WHERE call.company_id=$1 AND call.run_id=$2")
        .bind(s.scope.company.as_uuid()).bind(run.as_uuid()).fetch_one(s.fixture.persistence.pool()).await.unwrap();
    assert_eq!(
        revision, source.revision,
        "retry keeps the original captured native agent revision"
    );
    let claim = Box::pin(broker_claim(s, run)).await;
    for owner in owners(&source) {
        Box::pin(live(&source, &claim, owner)).await;
    }
    let state = effects(s).await;
    assert_eq!(state.calls, 1);
    assert_eq!(state.brokers, 1);
    assert_eq!(state.dispatches, 1);
}

#[tokio::test]
async fn native_mcp_source_local_action_rejects_agent_origin_substitution() {
    let (s, _) = Box::pin(originating()).await;
    let value: Value = sqlx::query_scalar(
        "SELECT operation FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(s.scope.run.as_uuid())
    .fetch_one(s.fixture.persistence.pool())
    .await
    .unwrap();
    let saved = FrozenAction::restore(value).unwrap();
    assert_eq!(saved.request().context.origin, ToolCallOrigin::Workflow);
    let mut changed = saved.request().clone();
    FrozenAction::local_agent(changed.clone()).unwrap();
    changed.context.origin = ToolCallOrigin::Agent(AgentToolOrigin {
        agent: s.profile.agent_id.unwrap(),
        revision: AgentRevisionId::new(Uuid::new_v4()),
        manifest: DependencyManifestId::new(Uuid::new_v4()),
        binding: AgentBindingId::new(Uuid::new_v4()),
    });
    let before = effects(&s).await;
    assert!(FrozenAction::local_agent(changed).is_err());
    assert_eq!(effects(&s).await, before);
}
