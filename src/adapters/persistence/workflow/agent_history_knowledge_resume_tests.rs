use super::*;
use crate::application::workflow::{
    decisions::WorkflowDecisions,
    lease::{WorkflowHandlerOutcome, WorkflowWorkerId},
};
use crate::services::test_support::ScriptedLlm;

async fn snapshot(f: &SearchFixture) -> Value {
    sqlx::query_scalar("SELECT frozen_preparation FROM agent_history_exchanges WHERE company_id=$1 AND execution_id=$2")
        .bind(f.claim.fence.scope.company.as_uuid()).bind(f.claim.fence.scope.execution.as_uuid()).fetch_one(f.p().pool()).await.unwrap()
}

async fn park(f: &SearchFixture) -> crate::domain::workflow::WaitId {
    let result = Box::pin(
        f.runtime()
            .execute(&WorkflowStepKind("agent.run".into()), &f.claim),
    )
    .await;
    let WorkflowHandlerOutcome::Suspended(wait) = result else {
        panic!("real token review must suspend before next provider call")
    };
    wait.wait
}

async fn approve(f: &mut SearchFixture) {
    use crate::application::workflow::decisions::{DecisionCommandKey, DecisionId, SubmitDecision};
    let scope = f.claim.fence.scope;
    let p = f.p().clone();
    let actor = f.fixture.target.actor;
    let id:Uuid=sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2 AND submitted_at IS NULL ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let view = p
        .read_decision(scope.company, actor, DecisionId(id))
        .await
        .unwrap();
    p.submit_decision(SubmitDecision {
        company: scope.company,
        actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse(format!("human-token-{id}")).unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest,
        choice: "approve".into(),
        feedback: String::new(),
        data: json!({}),
    })
    .await
    .unwrap();
    // Approval itself reopens the same job; the next operation is its genuine claim.
    f.claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
}

struct WaitingKnowledge {
    f: SearchFixture,
    preparation: Value,
    llm: ScriptedLlm,
}
impl WaitingKnowledge {
    async fn new(turns: Vec<LlmTurn>) -> Self {
        let mut f = Box::pin(SearchFixture::with_budget(AccessLevel::L9, Some(1000))).await;
        f.upload_sales().await;
        let mut llm = scripted_llm(turns).await;
        register_scripted_agent_base_url(f.agent, &llm.base_url);
        let first = park(&f).await;
        assert!(llm.observed().is_empty());
        let preparation = snapshot(&f).await;
        assert_eq!(preparation["history"]["seed"], json!([]));
        assert_eq!(preparation["history"]["content_level"], 0);
        approve(&mut f).await;
        let second = park(&f).await;
        assert_ne!(first, second);
        assert_eq!(llm.observed().len(), 1);
        assert_eq!(f.snapshot().await["content"], 5);
        assert_eq!(f.model.query_count(), 1);
        let checkpoint = f.checkpoint().await;
        assert_eq!(checkpoint.turns.len(), 1);
        assert!(
            checkpoint.invocations[0]
                .result
                .as_ref()
                .unwrap()
                .to_string()
                .contains("PRIVATE FLOOR 60")
        );
        assert_eq!(
            snapshot(&f).await,
            preparation,
            "later observation does not rewrite frozen seed/classification"
        );
        Self {
            f,
            preparation,
            llm,
        }
    }
}

fn knowledge_turn() -> LlmTurn {
    LlmTurn::tool_call(
        SEARCH_KNOWLEDGE,
        json!({"query":"What is the AX-4410 discount floor?"}),
    )
}

#[tokio::test]
async fn agent_history_level_later_real_knowledge_then_human_wait_refuses_downgraded_resume_before_provider()
 {
    let mut case = Box::pin(WaitingKnowledge::new(vec![knowledge_turn()])).await;
    assert_eq!(sqlx::query("UPDATE company_members AS member SET access_level=0 FROM workflow_runs AS run WHERE member.company_id=run.company_id AND member.user_id=run.actor_id AND run.company_id=$1 AND run.id=$2")
        .bind(case.f.claim.fence.scope.company.as_uuid()).bind(case.f.claim.fence.scope.run.as_uuid()).execute(case.f.p().pool()).await.unwrap().rows_affected(),1);
    approve(&mut case.f).await;
    let result = Box::pin(
        case.f
            .runtime()
            .execute(&WorkflowStepKind("agent.run".into()), &case.f.claim),
    )
    .await;
    let failure = result.completed_result().unwrap_err();
    assert_eq!(
        failure.failure.code().as_str(),
        "knowledge.audience_below_content"
    );
    let refusal = failure.failure.knowledge_refusal().unwrap();
    assert_eq!(refusal.content, AccessLevel::L5);
    assert_eq!(refusal.destination, AccessLevel::PUBLIC);
    assert_eq!(snapshot(&case.f).await, case.preparation);
    assert_eq!(case.f.model.query_count(), 1);
    assert_eq!(case.llm.finish().await.unwrap(), 1);
    case.f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn agent_history_level_later_real_knowledge_then_human_wait_allows_unchanged_audience() {
    let mut case = Box::pin(WaitingKnowledge::new(vec![
        knowledge_turn(),
        LlmTurn::text("\"allowed\""),
    ]))
    .await;
    approve(&mut case.f).await;
    let result = Box::pin(
        case.f
            .runtime()
            .execute(&WorkflowStepKind("agent.run".into()), &case.f.claim),
    )
    .await;
    assert_eq!(result.completed_result().unwrap(), json!("allowed"));
    assert_eq!(snapshot(&case.f).await, case.preparation);
    assert_eq!(case.f.model.query_count(), 1);
    assert_eq!(case.llm.finish().await.unwrap(), 2);
    case.f.library.shutdown().await.unwrap();
}
