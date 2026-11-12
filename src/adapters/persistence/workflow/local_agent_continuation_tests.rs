use super::*;
use crate::application::local_agents::workflow::{LocalRunInputs, LocalRunProgress};
use crate::application::ports::channel::{ChannelPersistence, ChannelWrite};
use crate::application::workflow::actions::ActionService;
use std::time::Duration;

async fn original(s: &Setup) -> SessionId {
    let p = &s.fixture.persistence;
    let claim = s.claim().await;
    s.park(&claim).await;
    s.native_success().await;
    p.resume_wait(s.scope).await.unwrap();
    let next = s.claim().await;
    let output = s
        .handler()
        .execute(&WorkflowStepKind(KIND.into()), &next)
        .await
        .completed_result()
        .unwrap();
    let session =
        SessionId::new(Uuid::parse_str(output["session_id"].as_str().unwrap()).unwrap()).unwrap();
    p.complete_io(FencedWorkflowResult {
        fence: next.fence,
        output,
        policy: lease(),
    })
    .await
    .unwrap()
    .unwrap();
    session
}

fn inputs(profile: ProfileId, session: SessionId) -> Value {
    json!({"profile_id":{"literal":profile},"session_id":{"literal":session},"prompt":{"literal":"explicit workflow follow-up"}})
}

async fn invoke(
    s: &Setup,
    claim: &ClaimedWorkflow,
) -> crate::app_error::AppResult<LocalRunProgress> {
    let inputs: LocalRunInputs = serde_json::from_value(claim.activation.inputs.clone()).unwrap();
    ActionService::new(local_agents::PostgresLocalAgentWorkflow::new(
        s.fixture.persistence.clone(),
        Some(super::super::approval_test_support::notices(
            &s.fixture.persistence,
        )),
    ))
    .run_local_agent(claim, &inputs, lease())
    .await
}

#[tokio::test]
async fn cloudroom_workflow_explicit_continuation_keeps_frozen_selection_and_approval_after_profile_edit()
 {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let session = original(&s).await;
    let mut with = inputs(s.profile.id, session);
    with["approval"] = json!({"literal":{"reviewers":[{"kind":"user","id":s.fixture.target.actor.user_id()}],"deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()}});
    let scope = s
        .activate(s.fixture.target.actor, with, RelatedAssociation::Company)
        .await;
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        invoke(&s, &claim).await.unwrap(),
        LocalRunProgress::Suspended(_)
    ));
    let before:Value=sqlx::query_scalar("SELECT jsonb_build_object('digest',argument_digest,'operation',operation) FROM workflow_action_intents WHERE run_id=$1")
        .bind(scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let mut edited = s.profile.clone();
    edited.selection.policy.reasoning = LocalReasoningLevel::High;
    p.save_profile(s.fixture.target.actor.user_id(), &edited)
        .await
        .unwrap();
    let command =
        super::super::approval_test_support::command(p, scope, s.fixture.target.actor, "approve")
            .await;
    p.submit_decision(command).await.unwrap();
    let next = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        invoke(&s, &next).await.unwrap(),
        LocalRunProgress::Suspended(_)
    ));
    let after:Value=sqlx::query_scalar("SELECT jsonb_build_object('digest',argument_digest,'operation',operation) FROM workflow_action_intents WHERE run_id=$1")
        .bind(scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(before, after);
    assert_eq!(
        after["operation"]["arguments"]["selection"],
        serde_json::to_value(&s.profile.selection).unwrap()
    );
    let native = s.native_started().await;
    let command: Value =
        sqlx::query_scalar("SELECT command FROM local_agent_operations WHERE id=$1")
            .bind(native.operation.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(command["kind"], "send");
    assert_eq!(command["session_id"], json!(session));
    s.native_complete(&native).await;
    p.resume_wait(scope).await.unwrap();
    let final_claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        invoke(&s, &final_claim).await.unwrap(),
        LocalRunProgress::Completed(_)
    ));
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM local_agent_sessions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(sessions, 1);
}

#[tokio::test]
async fn cloudroom_workflow_continuation_refuses_other_profile_and_other_original_association() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let session = original(&s).await;
    let mut other = s.profile.clone();
    other.id = ProfileId::new(Uuid::new_v4()).unwrap();
    other.name = "Other profile".into();
    p.save_profile(s.fixture.target.actor.user_id(), &other)
        .await
        .unwrap();
    let channel = ChannelPersistence::create(
        p,
        s.scope.company.as_uuid(),
        ChannelWrite {
            name: "Other scope".into(),
            slug: format!("scope-{}", Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for (profile, association) in [
        (other.id, RelatedAssociation::Company),
        (
            s.profile.id,
            RelatedAssociation::Channel(crate::application::workflow::RelatedChannelId::new(
                channel.id,
            )),
        ),
    ] {
        let scope = s
            .activate(
                s.fixture.target.actor,
                inputs(profile, session),
                association,
            )
            .await;
        let claim = p
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            invoke(&s, &claim).await,
            Err(AppError::NotFound(_))
        ));
    }
    assert_eq!(s.rows().await["operations"], 1);
}

async fn admin(s: &Setup) -> WorkflowActor {
    let user: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM company_members WHERE company_id=$1 AND role='admin'",
    )
    .bind(s.scope.company.as_uuid())
    .fetch_one(s.fixture.persistence.pool())
    .await
    .unwrap();
    WorkflowActor::authenticated(user).unwrap()
}

#[tokio::test]
async fn cloudroom_workflow_continuation_refuses_withdrawn_original_private_access() {
    let s = Setup::configured(|_, _| {}, TestAssociation::PrivateConversation).await;
    let p = &s.fixture.persistence;
    let session = original(&s).await;
    let actor = admin(&s).await;
    sqlx::query("UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1")
        .bind(actor.user_id())
        .execute(p.pool())
        .await
        .unwrap();
    let (channel, thread): (Uuid, Uuid) =
        sqlx::query_as("SELECT channel_id,thread_id FROM workflow_runs WHERE id=$1")
            .bind(s.scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) SELECT $1,$2,id,'view','configured_allowlist' FROM principals WHERE company_id=$1 AND user_id=$3")
        .bind(s.scope.company.as_uuid()).bind(channel).bind(actor.user_id()).execute(p.pool()).await.unwrap();
    assert!(
        crate::application::local_agents::management::LocalAgentManagementStore::read_session(
            p,
            actor.user_id(),
            s.scope.company.as_uuid(),
            session
        )
        .await
        .is_ok()
    );
    let association = RelatedAssociation::Thread {
        channel_id: crate::application::workflow::RelatedChannelId::new(channel),
        thread_id: crate::application::workflow::RelatedThreadId::new(thread),
    };
    let scope = s
        .activate(actor, inputs(s.profile.id, session), association)
        .await;
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    sqlx::query("DELETE FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 AND principal_id IN (SELECT id FROM principals WHERE company_id=$1 AND user_id=$3)")
        .bind(s.scope.company.as_uuid()).bind(channel).bind(actor.user_id()).execute(p.pool()).await.unwrap();
    assert!(matches!(
        invoke(&s, &claim).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(s.rows().await["operations"], 1);
}

pub(super) async fn wait_for_lock(s: &Setup) {
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock'")
                .fetch_one(s.fixture.persistence.pool()).await.unwrap();
            if waiting>0 {return;}
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
}

#[tokio::test]
async fn cloudroom_workflow_first_delivery_role_withdrawal_race_refuses_new_and_reconciles_attempted_work()
 {
    for attempted in [false, true] {
        let s = Setup::new().await;
        let p = &s.fixture.persistence;
        let actor = admin(&s).await;
        let scope=s.activate(actor,json!({"profile_id":{"literal":s.profile.id},"prompt":{"literal":"authorized workflow effect"}}),RelatedAssociation::Company).await;
        let claim = p
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap();
        s.park(&claim).await;
        let (fence, _) = s.peer().await;
        assert!(p.finish_reconciliation(fence).await.unwrap());
        let id: Uuid = sqlx::query_scalar(
            "SELECT operation_id FROM workflow_local_agent_calls WHERE run_id=$1",
        )
        .bind(scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        if attempted {
            assert!(
                p.pending_commands(fence)
                    .await
                    .unwrap()
                    .iter()
                    .any(|delivery| delivery.operation.id().as_uuid() == id)
            );
        }
        let mut blocker = p.pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
            .bind(s.scope.company.as_uuid())
            .execute(&mut *blocker)
            .await
            .unwrap();
        let pending = p.pending_commands(fence);
        let withdraw = async {
            wait_for_lock(&s).await;
            sqlx::query(
                "UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2",
            )
            .bind(s.scope.company.as_uuid())
            .bind(actor.user_id())
            .execute(&mut *blocker)
            .await
            .unwrap();
            blocker.commit().await.unwrap();
        };
        let (deliveries, ()) = tokio::join!(pending, withdraw);
        let deliveries = deliveries.unwrap();
        let delivered = deliveries
            .iter()
            .find(|delivery| delivery.operation.id().as_uuid() == id);
        if attempted {
            assert!(matches!(
                delivered.unwrap().mode,
                crate::application::local_agents::protocol::DeliveryMode::Reconcile
            ));
        } else {
            assert!(delivered.is_none());
            let attempted: bool = sqlx::query_scalar(
                "SELECT delivery_attempted_at IS NOT NULL FROM local_agent_operations WHERE id=$1",
            )
            .bind(id)
            .fetch_one(p.pool())
            .await
            .unwrap();
            assert!(!attempted);
        }
    }
}

#[tokio::test]
async fn cloudroom_workflow_call_origin_foreign_key_refuses_direct_link_even_without_immutable_guard()
 {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let claim = s.claim().await;
    s.park(&claim).await;
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE local_agent_operations DISABLE TRIGGER local_operation_inputs_immutable",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let error =
        sqlx::query("UPDATE local_agent_operations SET origin='direct' WHERE origin='workflow'")
            .execute(&mut *tx)
            .await
            .unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation(),
        "the composite call FK independently prevents cross-origin linkage"
    );
    tx.rollback().await.unwrap();
}

async fn select_native_agent(s: &mut Setup) -> crate::entities::agent_capability::AgentRevisionId {
    use crate::application::agent::{
        AgentPublicationKey, AgentPublicationScope, AgentPublications, PublishAgent,
    };
    use crate::entities::agent_capability::AgentId;
    use crate::use_cases::agent::{AgentPersistence, AgentWrite};
    let p = &s.fixture.persistence;
    let agent = AgentPersistence::create(
        p,
        s.scope.company.as_uuid(),
        AgentWrite {
            name: "Native workflow".into(),
            slug: "native-workflow".into(),
            system_prompt: Some("Frozen native workflow instructions".into()),
            native_permissions: LocalPermissions::Auto,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let revision = p
        .publish_agent(PublishAgent {
            scope: AgentPublicationScope {
                company: s.scope.company,
                actor: s.fixture.target.actor,
                agent: AgentId::new(agent.id),
            },
            expected_draft_revision: agent.draft_revision,
            expected_active_revision: None,
            key: AgentPublicationKey::parse("workflow-native-initial").unwrap(),
        })
        .await
        .unwrap();
    s.profile.agent_id = Some(AgentId::new(agent.id));
    p.save_profile(s.fixture.target.actor.user_id(), &s.profile)
        .await
        .unwrap();
    revision.id
}

fn native_approval(actor: Uuid) -> Value {
    json!({"literal":{"reviewers":[{"kind":"user","id":actor}],"deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()}})
}

async fn frozen_native(s: &Setup, scope: ActivationRequest) -> Value {
    sqlx::query_scalar(
        "SELECT operation->'arguments'->'harness' FROM workflow_action_intents WHERE run_id=$1",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(s.fixture.persistence.pool())
    .await
    .unwrap()
}

async fn approve_native(s: &Setup, scope: ActivationRequest) -> ClaimedWorkflow {
    let p = &s.fixture.persistence;
    p.submit_decision(
        super::super::approval_test_support::command(p, scope, s.fixture.target.actor, "approve")
            .await,
    )
    .await
    .unwrap();
    p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn cloudroom_native_workflow_freezes_agent_before_approval_and_ignores_later_profile_choice()
{
    let mut s = Setup::with_inputs(|inputs, actor| {
        inputs["approval"] = native_approval(actor);
    })
    .await;
    let revision = select_native_agent(&mut s).await;
    let claim = s.claim().await;
    assert!(matches!(
        invoke(&s, &claim).await.unwrap(),
        LocalRunProgress::Suspended(_)
    ));
    let captured = frozen_native(&s, s.scope).await;
    assert_eq!(captured["revision_id"], json!(revision));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_agent_operations")
        .fetch_one(s.fixture.persistence.pool())
        .await
        .unwrap();
    assert_eq!(count, 0, "approval must precede native effects");
    let mut changed = s.profile.clone();
    changed.agent_id = None;
    s.fixture
        .persistence
        .save_profile(s.fixture.target.actor.user_id(), &changed)
        .await
        .unwrap();
    let approved = approve_native(&s, s.scope).await;
    assert!(matches!(
        invoke(&s, &approved).await.unwrap(),
        LocalRunProgress::Suspended(_)
    ));
    let session:Value=sqlx::query_scalar("SELECT jsonb_build_object('revision_id',agent_revision_id,'permissions',selection->'policy'->'permissions') FROM local_agent_sessions")
        .fetch_one(s.fixture.persistence.pool()).await.unwrap();
    assert_eq!(
        session,
        json!({"revision_id":revision,"permissions":"auto"})
    );
    assert_eq!(frozen_native(&s, s.scope).await, captured);
}

#[tokio::test]
async fn cloudroom_native_workflow_continuation_keeps_original_agent_before_its_new_approval() {
    let mut s = Setup::new().await;
    let revision = select_native_agent(&mut s).await;
    let session = original(&s).await;
    let mut changed = s.profile.clone();
    changed.agent_id = None;
    s.fixture
        .persistence
        .save_profile(s.fixture.target.actor.user_id(), &changed)
        .await
        .unwrap();
    let mut with = inputs(s.profile.id, session);
    with["approval"] = native_approval(s.fixture.target.actor.user_id());
    let scope = s
        .activate(s.fixture.target.actor, with, RelatedAssociation::Company)
        .await;
    let claim = s
        .fixture
        .persistence
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        invoke(&s, &claim).await.unwrap(),
        LocalRunProgress::Suspended(_)
    ));
    let captured = frozen_native(&s, scope).await;
    assert_eq!(captured["revision_id"], json!(revision));
    let next = approve_native(&s, scope).await;
    assert!(matches!(
        invoke(&s, &next).await.unwrap(),
        LocalRunProgress::Suspended(_)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_agent_sessions")
        .fetch_one(s.fixture.persistence.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(frozen_native(&s, scope).await, captured);
    let send: Value = sqlx::query_scalar(
        "SELECT command FROM local_agent_operations WHERE command->>'kind'='send'",
    )
    .fetch_one(s.fixture.persistence.pool())
    .await
    .unwrap();
    assert_eq!(send["session_id"], json!(session));
}
