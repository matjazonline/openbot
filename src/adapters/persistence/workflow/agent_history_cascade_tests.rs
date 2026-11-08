//! Actual source/turn/admission lock contention through the native parent cancellation owner.
use super::*;
use crate::adapters::persistence::workflow::agent_publication::AgentPublicationDirectory;
use crate::application::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    company::{CompanyModelConnectionWrite, CompanyPersistence},
};
use crate::application::workflow::{
    activation::{ActivationRequest, WorkflowJobId},
    agent::{AgentPreparation, AgentPreparationResult},
    agent_history::AgentHistorySessionRef,
};
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};

async fn setup() -> ToolSetup {
    let fixture = Fixture::new().await;
    let p = &fixture.persistence;
    CompanyPersistence::replace_model_connections_for_user(
        p,
        fixture.target.actor.user_id(),
        fixture.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let leaf = AgentPersistence::create(
        p,
        fixture.target.company.as_uuid(),
        AgentWrite {
            name: "Leaf".into(),
            slug: "leaf".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    fixture.publish_agent(leaf.id).await;
    let mut child: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    let existing = decision_child();
    child["workflow_id"] = json!(Uuid::from_u128(CHILD));
    child["input_schema"] = existing["input_schema"].clone();
    child["parameter_schema"] = existing["parameter_schema"].clone();
    child["steps"]["start"]["with"]["agent"] = json!({"literal":"leaf"});
    child["steps"]["start"]["with"]["context"] = json!({"ref":"/input"});
    let version = publish_through(
        &fixture,
        WorkflowId::new(Uuid::from_u128(CHILD)),
        "history-leaf",
        child.to_string(),
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: Preflight,
        },
    )
    .await
    .unwrap();
    let reviewer = fixture.target.actor.user_id();
    Box::pin(ToolSetup::bind_child(fixture, reviewer, version)).await
}

#[derive(sqlx::FromRow)]
struct Job {
    execution: Uuid,
    job: Uuid,
}
async fn reused_root(setup: &ToolSetup, from: RunId) -> ClaimedWorkflow {
    let p = setup.persistence();
    let company = setup.target.company;
    let result = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            history_session: Some(AgentHistorySessionRef { from_run: from }),
            company_id: company,
            actor: setup.target.actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: crate::entities::correlation::CorrelationId::new(),
            binding_id: setup.target.binding,
            idempotency_key: IdempotencyKey::parse("cascade-reused-root").unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await
        .unwrap();
    let AdmissionResult::Created(run) = result else {
        panic!("genuine reused root")
    };
    let job:Job=sqlx::query_as("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2")
        .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    p.claim_io(
        ActivationRequest {
            company,
            run,
            execution: ExecutionId::new(job.execution),
            job: WorkflowJobId(job.job),
        },
        WorkflowWorkerId(Uuid::new_v4()),
        lease(),
    )
    .await
    .unwrap()
    .unwrap()
}

#[derive(sqlx::FromRow)]
struct Blocked {
    pid: i32,
    query: String,
}
async fn blocked_by(p: &PostgresPersistence, blocker: i32) -> Blocked {
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            let row:Option<Blocked>=sqlx::query_as("SELECT pid,query FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) ORDER BY pid LIMIT 1")
                .bind(blocker).fetch_optional(p.pool()).await.unwrap();
            if let Some(row)=row {return row;}
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("native contender must reach its known blocker")
}

async fn cancel_command(setup: &ToolSetup, scope: ActivationRequest) -> CancelCommand {
    let revision = setup
        .persistence()
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: setup.target.actor,
        command_key: IdempotencyKey::parse("history-cascade").unwrap(),
        expected_revision: revision,
    }
}

async fn conversation(p: &PostgresPersistence, claim: &ClaimedWorkflow) -> Uuid {
    sqlx::query_scalar("SELECT conversation_id FROM agent_history_exchanges WHERE company_id=$1 AND execution_id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid()).fetch_one(p.pool()).await.unwrap()
}

#[tokio::test]
async fn agent_history_recovery_parent_cancel_private_child_and_other_root_admission_do_not_deadlock()
 {
    let setup = Box::pin(setup()).await;
    let p = setup.persistence();
    let ParkedParent { parent, llm } = Box::pin(park_parent(&setup)).await;
    let parent_conversation = conversation(p, &parent).await;
    let child_scope = setup.child_of(parent.fence.scope).await;
    let child = p
        .claim_io(child_scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let other = Box::pin(reused_root(&setup, parent.fence.scope.run)).await;
    let command = cancel_command(&setup, parent.fence.scope).await;
    let mut leaf = p.pool().begin().await.unwrap();
    let leaf_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *leaf)
        .await
        .unwrap();
    sqlx::query("SELECT company_id FROM agent_history_admission WHERE company_id=$1 FOR UPDATE")
        .bind(parent.fence.scope.company.as_uuid())
        .fetch_one(&mut *leaf)
        .await
        .unwrap();
    let child_prepare = {
        let p = p.clone();
        let claim = child.clone();
        tokio::spawn(async move { p.prepare_agent(&claim, lease()).await })
    };
    let blocked_child = blocked_by(p, leaf_pid).await;
    assert!(blocked_child.query.contains("agent_history_admit"));
    let cancel = {
        let p = p.clone();
        tokio::spawn(async move { p.cancel(command).await })
    };
    let blocked_cancel = blocked_by(p, blocked_child.pid).await;
    let other_prepare = {
        let p = p.clone();
        let claim = other.clone();
        tokio::spawn(async move { p.prepare_agent(&claim, lease()).await })
    };
    let blocked_other = blocked_by(p, blocked_cancel.pid).await;
    // Existing writer authority serializes preparation/cancel at the company first.
    // Both are before another conversation/admission lock; no cycle can be introduced here.
    assert!(
        blocked_cancel.query.contains("FROM companies")
            && blocked_cancel.query.contains("FOR NO KEY UPDATE")
    );
    assert!(
        blocked_other.query.contains("FROM companies")
            && blocked_other.query.contains("FOR NO KEY UPDATE")
    );
    let held: Option<Uuid> = sqlx::query_scalar(
        "SELECT active_exchange_id FROM agent_history_conversations WHERE company_id=$1 AND id=$2",
    )
    .bind(parent.fence.scope.company.as_uuid())
    .bind(parent_conversation)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        held,
        Some(parent.fence.scope.execution.as_uuid()),
        "blocked cancellation cannot release an unfinished parent"
    );
    leaf.rollback().await.unwrap();
    finish(CascadePending {
        child: child_prepare,
        cancel,
        other: other_prepare,
    })
    .await;
    check_cascade(
        p,
        Turns {
            parent: &parent,
            child: &child,
            other: &other,
        },
        parent_conversation,
    )
    .await;
    assert_eq!(llm.finish().await.unwrap(), 1);
}

use std::time::Duration;

struct Turns<'a> {
    parent: &'a ClaimedWorkflow,
    child: &'a ClaimedWorkflow,
    other: &'a ClaimedWorkflow,
}
async fn check_cascade(p: &PostgresPersistence, turns: Turns<'_>, parent_conversation: Uuid) {
    assert_eq!(
        run_state(p, turns.parent.fence.scope.run).await,
        "cancelled"
    );
    assert_eq!(run_state(p, turns.child.fence.scope.run).await, "cancelled");
    assert_ne!(conversation(p, turns.child).await, parent_conversation);
    assert_eq!(conversation(p, turns.other).await, parent_conversation);
    let owner: Option<Uuid> = sqlx::query_scalar(
        "SELECT active_exchange_id FROM agent_history_conversations WHERE company_id=$1 AND id=$2",
    )
    .bind(turns.parent.fence.scope.company.as_uuid())
    .bind(parent_conversation)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(owner, Some(turns.other.fence.scope.execution.as_uuid()));
    let child_models: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM task_harness_runs WHERE company_id=$1 AND task_id=$2",
    )
    .bind(turns.child.fence.scope.company.as_uuid())
    .bind(turns.child.fence.scope.job.0)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(child_models, 0);
    let private:bool=sqlx::query_scalar("SELECT scope_kind='child_branch' AND child_run_id=$3 FROM agent_history_conversations WHERE company_id=$1 AND id=$2")
        .bind(turns.child.fence.scope.company.as_uuid()).bind(conversation(p,turns.child).await)
        .bind(turns.child.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(
        private,
        "actual child conversation must be its private branch"
    );
}

struct ParkedParent {
    parent: ClaimedWorkflow,
    llm: crate::services::test_support::ScriptedLlm,
}
async fn park_parent(setup: &ToolSetup) -> ParkedParent {
    let parent = setup.claim("history-parent").await;
    let llm = scripted_llm(vec![LlmTurn::tool_call(
        DECLARED,
        json!({"question":"Review?"}),
    )])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    assert!(matches!(
        Box::pin(setup.runtime().execute(&kind(), &parent)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    ParkedParent { parent, llm }
}

struct CascadePending {
    child: tokio::task::JoinHandle<AppResult<AgentPreparationResult>>,
    cancel: tokio::task::JoinHandle<AppResult<CancelResult>>,
    other: tokio::task::JoinHandle<AppResult<AgentPreparationResult>>,
}
async fn finish(pending: CascadePending) {
    // Join every actual operation before inspecting results, including failed contenders.
    let child = joined(pending.child).await;
    let cancel = joined(pending.cancel).await;
    let other = joined(pending.other).await;
    assert!(matches!(
        child.unwrap().unwrap(),
        AgentPreparationResult::Ready(_)
    ));
    assert!(matches!(
        cancel.unwrap().unwrap(),
        CancelResult::Applied { .. }
    ));
    assert!(matches!(
        other.unwrap().unwrap(),
        AgentPreparationResult::Ready(_)
    ));
}
async fn joined<T>(mut pending: tokio::task::JoinHandle<T>) -> Result<T, &'static str> {
    match tokio::time::timeout(Duration::from_secs(5), &mut pending).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err("native contender panicked"),
        Err(_) => {
            pending.abort();
            let _ = pending.await;
            Err("native contender exceeded its hang guard")
        }
    }
}
