//! The originating workflow is real and parked on its native operation. Both the
//! parent and broker fences come from normal worker claims.
use super::*;
use crate::application::agent::{
    AgentPublicationKey, AgentPublicationScope, AgentPublications, PublishAgent,
};
use crate::application::local_agents::protocol::ConnectionIdentity;
use crate::application::local_agents::{harness::NativeBundleRequest, mcp::*};
use crate::application::workflow::{activation::WorkflowJobId, mcp::*};
use crate::domain::workflow::{ExecutionId, RunId};
use crate::entities::agent_capability::{AgentId, AgentToolAlias};
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use std::time::Duration;
#[path = "local_agent_mcp_history_tests.rs"]
mod history;

async fn configure(s: &mut Setup, endpoint: &str) {
    let p = &s.fixture.persistence;
    let company = s.fixture.target.company;
    let tool = crate::adapters::persistence::local_agents::test_support::native_mcp_binding(
        p,
        company.as_uuid(),
        s.fixture.target.actor.user_id(),
        endpoint,
    )
    .await;
    let agent = AgentPersistence::create(
        p,
        company.as_uuid(),
        AgentWrite {
            name: "Workflow native MCP".into(),
            slug: "workflow-native-mcp".into(),
            native_permissions: LocalPermissions::Full,
            tool_bindings: Some(vec![tool]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    p.publish_agent(PublishAgent {
        scope: AgentPublicationScope {
            company,
            actor: s.fixture.target.actor,
            agent: AgentId::new(agent.id),
        },
        expected_draft_revision: agent.draft_revision,
        expected_active_revision: None,
        key: AgentPublicationKey::parse("workflow-native-mcp").unwrap(),
    })
    .await
    .unwrap();
    let catalog = json!({"projects":["proj_test"],"providers":[{"provider":"claude-code","permission_modes":["full","auto","accept_edits"],
        "models":[{"model":"fixture-claude","reasoning_levels":["none","low","high"]}]}]});
    sqlx::query("INSERT INTO local_machine_capabilities(company_id,machine_id,installation_id,sequence,catalog) VALUES($1,$2,$3,2,$4)")
        .bind(company.as_uuid()).bind(s.profile.selection.machine_id.as_uuid()).bind(s.paired.receipt.machine.installation_id.as_uuid()).bind(catalog)
        .execute(p.pool()).await.unwrap();
    sqlx::query("UPDATE local_machines SET capabilities_sequence=2 WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid())
        .bind(s.profile.selection.machine_id.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    s.profile.agent_id = Some(AgentId::new(agent.id));
    s.profile.selection.provider = LocalProvider::ClaudeCode;
    s.profile.selection.model = "fixture-claude".to_owned().try_into().unwrap();
    p.save_profile(s.fixture.target.actor.user_id(), &s.profile)
        .await
        .unwrap();
}

async fn originating() -> (Setup, NativeTurn) {
    Box::pin(originating_at("http://127.0.0.1:9")).await
}

async fn originating_at(endpoint: &str) -> (Setup, NativeTurn) {
    let mut s = Box::pin(Setup::new()).await;
    configure(&mut s, endpoint).await;
    let p = &s.fixture.persistence;
    let company = s.scope.company;
    let admin: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM company_members WHERE company_id=$1 AND role='admin' LIMIT 1",
    )
    .bind(company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(company.as_uuid())
        .bind(admin)
        .execute(p.pool())
        .await
        .unwrap();
    let scope=s.activate(WorkflowActor::authenticated(admin).unwrap(),json!({"profile_id":{"literal":s.profile.id},"prompt":{"literal":"native workflow MCP"}}),RelatedAssociation::Company).await;
    s.scope = scope;
    let claim = s.claim().await;
    s.park(&claim).await;
    let native = s.native_started().await;
    // Current actor knowledge can grow; the originating workflow's immutable ceiling cannot.
    sqlx::query("UPDATE company_members SET access_level=9 WHERE company_id=$1 AND user_id=$2")
        .bind(company.as_uuid())
        .bind(admin)
        .execute(s.fixture.persistence.pool())
        .await
        .unwrap();
    (s, native)
}

fn request(s: &Setup, native: &NativeTurn) -> NativeMcpRequest {
    NativeMcpRequest {
        verifier: s.paired.candidate.credential.verifier(),
        identity: ConnectionIdentity::from_fence(native.fence).unwrap(),
        operation: native.operation,
        body: NativeMcpBody {
            invocation: NativeMcpInvocation {
                thread_id: native.thread.clone(),
                turn_id: native.turn.clone(),
                client_request_id: "creq_workflow_native".to_owned().try_into().unwrap(),
                model_call_id: "toolu_workflow_native".to_owned().try_into().unwrap(),
            },
            action: NativeMcpAction::Call {
                alias: AgentToolAlias::parse("crm.lookup").unwrap(),
                arguments: json!({"query":"from originating workflow"}),
            },
        },
    }
}

async fn broker_claim(s: &Setup, run: RunId) -> ClaimedWorkflow {
    let (execution,job):(Uuid,Uuid)=sqlx::query_as("SELECT execution.id,job.id FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2")
        .bind(s.scope.company.as_uuid()).bind(run.as_uuid()).fetch_one(s.fixture.persistence.pool()).await.unwrap();
    s.fixture
        .persistence
        .claim_io(
            ActivationRequest {
                company: s.scope.company,
                run,
                execution: ExecutionId::new(execution),
                job: WorkflowJobId(job),
            },
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn native_mcp_workflow_created_origin_obeys_original_knowledge_ceiling_and_parent_run_locks()
{
    let (s, native) = Box::pin(originating()).await;
    let p = &s.fixture.persistence;
    let request = request(&s, &native);
    let authentication = NativeBundleRequest {
        verifier: request.verifier,
        identity: request.identity,
        operation: request.operation,
    };
    let mut competitor = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(s.scope.company.as_uuid())
        .bind(s.scope.run.as_uuid())
        .execute(&mut *competitor)
        .await
        .unwrap();
    let auth = tokio::time::timeout(
        Duration::from_secs(2),
        p.authorize_native_mcp(&authentication),
    )
    .await
    .expect("native pre-body auth must not wait for workflow parent");
    assert!(matches!(auth, Err(AppError::ServiceUnavailable(_))));
    let denied = tokio::time::timeout(Duration::from_secs(2), p.native_mcp(&request))
        .await
        .expect("MCP admission must not wait for workflow parent");
    assert!(matches!(denied, Err(AppError::ServiceUnavailable(_))));
    competitor.rollback().await.unwrap();
    let response = p.native_mcp(&request).await.unwrap();
    let facts:(i16,i16,bool,Uuid)=sqlx::query_as("SELECT run.knowledge_ceiling::smallint,run.content_level::smallint,run.content_exact,session.workflow_run_id FROM workflow_runs AS run JOIN native_mcp_calls AS call ON call.company_id=run.company_id AND call.run_id=run.id JOIN local_agent_sessions AS session ON session.company_id=call.company_id AND session.id=call.session_id WHERE run.company_id=$1 AND run.id=$2")
        .bind(s.scope.company.as_uuid()).bind(response.run_id.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(facts, (5, 5, false, s.scope.run.as_uuid()));
    let claim = Box::pin(broker_claim(&s, response.run_id)).await;
    assert!(p.validate_io(claim.fence, lease()).await.unwrap());
    assert!(
        p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into()))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn native_mcp_workflow_parent_cancel_ends_real_broker_lease_and_preparation_before_any_effect()
 {
    let (s, native) = Box::pin(originating()).await;
    let p = &s.fixture.persistence;
    let response = p.native_mcp(&request(&s, &native)).await.unwrap();
    let claim = Box::pin(broker_claim(&s, response.run_id)).await;
    p.cancel(CancelCommand {
        company_id: s.scope.company,
        run_id: s.scope.run,
        actor: s.fixture.target.actor,
        command_key: IdempotencyKey::parse("cancel-native-mcp-source").unwrap(),
        expected_revision: p
            .head(s.scope.company, s.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    })
    .await
    .unwrap();
    assert!(!p.validate_io(claim.fence, lease()).await.unwrap());
    assert!(
        p.mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into()))
            .await
            .is_err()
    );
    let markers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.scope.company.as_uuid())
    .bind(response.run_id.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(markers, 0);
    assert!(p.native_mcp(&request(&s, &native)).await.is_err());
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 1);
}
