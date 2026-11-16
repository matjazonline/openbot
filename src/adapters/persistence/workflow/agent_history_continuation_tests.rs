use super::*;
use crate::application::workflow::decisions::{
    DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions,
};
use tokio::sync::Barrier;

struct Reviewers {
    setup: Box<AgentSetup>,
    admin: Uuid,
}

async fn reviewed(origin: &str) -> Reviewers {
    let mut setup = Box::new(Box::pin(agent_setup_full(origin, None, true)).await);
    let admin = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &setup.fixture,
        "admin",
    )
    .await;
    let mut source = model_source(&AgentKey::parse("assistant").unwrap(), ModelStep::AgentRun);
    source["steps"]["task"] = source["steps"]["start"].clone();
    source["steps"]["start"]["with"]["context"] = json!({"literal":"prior task"});
    source["steps"]["start"]["routes"]["success"] = json!("task");
    source["steps"]["task"]["with"]["context"] = json!({"literal":"current task"});
    source["steps"]["task"]["with"]["approval"] = json!({"literal":{
        "reviewers":[{"kind":"principal","id":setup.fixture.principal(setup.fixture.target.actor.user_id()).await},
            {"kind":"principal","id":setup.fixture.principal(admin).await}],
        "deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()}});
    let graph = publish_graph(&setup, source, json!({})).await;
    let AdmissionResult::Created(run) = admit_graph(&setup, &graph, "history-human-rounds")
        .await
        .unwrap()
    else {
        panic!("new reviewed root")
    };
    setup.claim = claim_run(&setup, run).await;
    Reviewers { setup, admin }
}

async fn pending_command(setup: &AgentSetup, actor: WorkflowActor, key: &str) -> SubmitDecision {
    let scope = setup.claim.fence.scope;
    let id:Uuid=sqlx::query_scalar("SELECT decision.id FROM workflow_decisions AS decision JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id WHERE decision.company_id=$1 AND decision.execution_id=$2 AND wait.state='waiting' AND decision.submitted_at IS NULL")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(setup.fixture.persistence.pool()).await.unwrap();
    let view = setup
        .fixture
        .persistence
        .read_decision(scope.company, actor, DecisionId(id))
        .await
        .unwrap();
    SubmitDecision {
        company: scope.company,
        actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse(key).unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest,
        choice: "approve".into(),
        feedback: String::new(),
        data: json!({}),
    }
}

async fn inspect_waiting_history(setup: &AgentSetup) {
    use crate::application::workflow::{
        agent_history::AgentExchangeId,
        agent_history_views::HistoryActivity,
        run_views::{RunReadScope, RunViews},
        storage::{HistoryCandidate, WorkflowStorageMaintenance},
    };
    let scope = setup.claim.fence.scope;
    let p = &setup.fixture.persistence;
    let detail = p
        .agent_history_exchange(
            RunReadScope {
                company: scope.company,
                run: scope.run,
                viewer: setup.fixture.target.actor,
            },
            scope.execution.as_uuid(),
            AgentExchangeId::for_execution(scope.execution),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail.entry.activity,
        Some(HistoryActivity::WaitingForHuman)
    );
    let before = frozen(setup).await;
    assert!(
        !p.expire_history(HistoryCandidate {
            company: scope.company,
            finished_at: chrono::Utc::now(),
            exchange: AgentExchangeId::for_execution(scope.execution)
        })
        .await
        .unwrap()
    );
    assert_eq!(
        frozen(setup).await,
        before,
        "real human continuation keeps the exact retained frozen history"
    );
}

async fn competing_approval(reviewers: &Reviewers) {
    let setup = &reviewers.setup;
    inspect_waiting_history(setup).await;
    let owner = pending_command(setup, setup.fixture.target.actor, "history-owner-choice").await;
    let admin = pending_command(
        setup,
        WorkflowActor::authenticated(reviewers.admin).unwrap(),
        "history-admin-choice",
    )
    .await;
    let p = &setup.fixture.persistence;
    let barrier = Barrier::new(2);
    let first = async {
        barrier.wait().await;
        p.submit_decision(owner).await
    };
    let second = async {
        barrier.wait().await;
        p.submit_decision(admin).await
    };
    let (first, second) = tokio::join!(first, second);
    let outcomes = [first, second];
    assert_eq!(outcomes.iter().filter(|o| o.is_ok()).count(), 1);
    for outcome in outcomes {
        if let Err(error) = outcome {
            assert!(matches!(error, AppError::Conflict(_)), "{error:?}");
        }
    }
}

async fn frozen(setup: &AgentSetup) -> Value {
    sqlx::query_scalar("SELECT frozen_preparation FROM agent_history_exchanges WHERE company_id=$1 AND execution_id=$2")
        .bind(setup.claim.fence.scope.company.as_uuid()).bind(setup.claim.fence.scope.execution.as_uuid())
        .fetch_one(setup.fixture.persistence.pool()).await.unwrap()
}

async fn start_task(setup: &mut AgentSetup) {
    let output =
        Box::pin(agent_runtime(setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim))
            .await
            .completed_result()
            .unwrap();
    assert_eq!(output, json!("prior accepted"));
    let next = setup
        .fixture
        .persistence
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: setup.claim.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .unwrap()
        .successor
        .unwrap();
    setup.claim = setup
        .fixture
        .persistence
        .claim_io(
            next,
            crate::application::workflow::lease::WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn agent_history_continuation_two_real_approval_rounds_and_competing_reviewers_preserve_one_exchange()
 {
    let (origin, server) = servers(2).await;
    let mut reviewers = Box::pin(reviewed(&origin)).await;
    let llm = scripted_llm(vec![
        LlmTurn::text("\"prior accepted\""),
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/first"}),
        ),
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/second"}),
        ),
        LlmTurn::text("\"both accepted\""),
    ])
    .await;
    register_scripted_agent_base_url(reviewers.setup.agent, &llm.base_url);
    start_task(&mut reviewers.setup).await;
    let kind = WorkflowStepKind("agent.run".into());
    assert!(matches!(
        Box::pin(agent_runtime(&reviewers.setup).execute(&kind, &reviewers.setup.claim)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let before = frozen(&reviewers.setup).await;
    assert_eq!(
        before["history"]["seed"],
        json!([{"input":"prior task","output":"prior accepted"}])
    );
    competing_approval(&reviewers).await;
    finish_approval_rounds(&reviewers, before, server, llm).await;
}

async fn assert_history_counts(setup: &AgentSetup) {
    let scope = setup.claim.fence.scope;
    let p = &setup.fixture.persistence;
    let facts:Value=sqlx::query_scalar("SELECT jsonb_build_object('exchanges',(SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2),'completed',(SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2 AND state='completed'),'decisions',(SELECT count(*) FROM workflow_decisions WHERE company_id=$1 AND execution_id=$3 AND submitted_at IS NOT NULL),'dispatches',(SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1 AND execution_id=$3))")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        facts,
        json!({"exchanges":2,"completed":2,"decisions":2,"dispatches":2})
    );
}

async fn finish_approval_rounds(
    reviewers: &Reviewers,
    before: Value,
    server: tokio::task::JoinHandle<Vec<String>>,
    mut llm: crate::services::test_support::ScriptedLlm,
) {
    let kind = WorkflowStepKind("agent.run".into());
    let second = reclaim(&reviewers.setup).await;
    assert_eq!(
        second.fence.scope.execution,
        reviewers.setup.claim.fence.scope.execution
    );
    assert!(matches!(
        Box::pin(agent_runtime(&reviewers.setup).execute(&kind, &second)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    assert_eq!(frozen(&reviewers.setup).await, before);
    approve_open(&reviewers.setup, "history-second-round").await;
    let third = reclaim(&reviewers.setup).await;
    assert_eq!(third.fence.scope.execution, second.fence.scope.execution);
    assert_eq!(
        Box::pin(agent_runtime(&reviewers.setup).execute(&kind, &third))
            .await
            .completed_result()
            .unwrap(),
        json!("both accepted")
    );
    assert_eq!(frozen(&reviewers.setup).await, before);
    let sent = server.await.unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent[0].starts_with("POST /first") && sent[1].starts_with("POST /second"));
    assert_history_counts(&reviewers.setup).await;
    let requests = llm.observed();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3].to_string().matches("prior accepted").count(), 1);
    assert_eq!(requests[3].to_string().matches("current task").count(), 1);
    assert_eq!(llm.finish().await.unwrap(), 4);
}

#[tokio::test]
async fn agent_history_retention_actual_human_submission_owner_competes_with_cleanup_and_replays_expired_selected_source()
 {
    use crate::adapters::persistence::workflow::{
        agent_history_race_fixture::{OwnerGate, Target},
        agent_history_retention_fixture as clock,
    };
    use crate::application::workflow::storage::WorkflowStorageMaintenance;
    let (origin, server) = servers(2).await;
    let mut reviewers = Box::pin(reviewed(&origin)).await;
    let llm = scripted_llm(vec![
        LlmTurn::text("\"prior accepted\""),
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/first"}),
        ),
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/second"}),
        ),
        LlmTurn::text("\"both accepted\""),
    ])
    .await;
    register_scripted_agent_base_url(reviewers.setup.agent, &llm.base_url);
    let prior = reviewers.setup.claim.fence.scope;
    start_task(&mut reviewers.setup).await;
    let setup = &reviewers.setup;
    let p = &setup.fixture.persistence;
    // This fixture clock changes only the completed source copy; the genuine human deadline stays live.
    clock::age_completed_copy(p, prior).await;
    let candidate = clock::candidate(p, prior).await;
    let kind = WorkflowStepKind("agent.run".into());
    assert!(matches!(
        Box::pin(agent_runtime(setup).execute(&kind, &setup.claim)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    inspect_waiting_history(setup).await;
    let before = frozen(setup).await;
    assert_eq!(
        before["history"]["seed"],
        json!([{"input":"prior task","output":"prior accepted"}])
    );
    let command = pending_command(
        setup,
        setup.fixture.target.actor,
        "history-cleanup-race-choice",
    )
    .await;
    let gate = OwnerGate::install(p.pool(), Target::Decision(command.decision.0)).await;
    let human_owner = p.submit_decision(command);
    let cleanup = async {
        gate.arrived(prior.company.as_uuid()).await;
        assert!(!p.expire_history(candidate).await.unwrap());
        gate.release().await;
    };
    let (submitted, ()) = tokio::join!(human_owner, cleanup);
    submitted.unwrap();
    assert_eq!(frozen(setup).await, before);
    assert!(
        p.expire_history(candidate).await.unwrap(),
        "ordinary completed own job may expire while its root has a live continuation"
    );
    assert_eq!(
        frozen(setup).await,
        before,
        "active human preparation keeps its exact copied pair"
    );
    finish_approval_rounds(&reviewers, before, server, llm).await;
}
