use super::*;
use crate::application::workflow::{
    CancelCommand, CancelResult, WorkflowInspection, WorkflowRunTransitions,
    agent::{AgentPreparation, AgentPreparationResult},
    waits::{WaitProgress, WorkflowWaits},
};
use crate::domain::workflow::WaitId;

fn policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(60)).unwrap()
}

async fn cancel(f: &HistoryThread, claim: &ClaimedWorkflow, key: &str) {
    let p = f.s.p();
    let scope = claim.fence.scope;
    let head = p.head(scope.company, scope.run).await.unwrap().unwrap();
    assert!(matches!(
        p.cancel(CancelCommand {
            company_id: scope.company,
            run_id: scope.run,
            actor: f.s.owner(),
            command_key: IdempotencyKey::parse(key).unwrap(),
            expected_revision: head.revision,
        })
        .await
        .unwrap(),
        CancelResult::Applied { .. }
    ));
}

async fn pointer(f: &HistoryThread) -> Option<Uuid> {
    sqlx::query_scalar("SELECT active_exchange_id FROM agent_history_conversations WHERE company_id=$1 AND agent_id=$2 AND thread_id=$3")
        .bind(f.s.fixture.target.company.as_uuid()).bind(f.agent).bind(f.s.thread)
        .fetch_one(f.s.pool()).await.unwrap()
}

async fn parked(f: &HistoryThread, claim: &ClaimedWorkflow) -> WaitId {
    let AgentPreparationResult::Parked(wait) =
        f.s.p().prepare_agent(claim, policy()).await.unwrap()
    else {
        panic!("turn must park behind the owner")
    };
    wait
}

#[tokio::test]
async fn agent_history_ordering_competing_preparations_park_without_checkpoint_or_retry_and_resume_once()
 {
    let f = Box::pin(HistoryThread::new()).await;
    let a = f.next().await;
    let b = f.next().await;
    let p = f.s.p();
    let (left, right) = tokio::join!(p.prepare_agent(&a, policy()), p.prepare_agent(&b, policy()));
    let (owner, queued, wait) = match (left.unwrap(), right.unwrap()) {
        (AgentPreparationResult::Ready(_), AgentPreparationResult::Parked(wait)) => (&a, &b, wait),
        (AgentPreparationResult::Parked(wait), AgentPreparationResult::Ready(_)) => (&b, &a, wait),
        _ => panic!("exactly one competing turn must own the stream"),
    };
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
    let states: Vec<String> = sqlx::query_scalar(
        "SELECT state FROM agent_history_exchanges WHERE company_id=$1 ORDER BY position",
    )
    .bind(a.fence.scope.company.as_uuid())
    .fetch_all(f.s.pool())
    .await
    .unwrap();
    assert_eq!(states, vec!["queued", "queued"]);
    let saved:Value=sqlx::query_scalar("SELECT jsonb_build_object('status',job.status,'retry',job.retry_count,'continuation',job.continuation_count,'leased',job.worker_id IS NOT NULL,'attempt_status',attempt.status,'stop',attempt.stop_reason,'checkpoints',(SELECT count(*) FROM task_harness_runs WHERE company_id=job.company_id)) FROM background_tasks AS job JOIN task_attempts AS attempt ON attempt.task_id=job.id AND attempt.attempt_number=job.attempt_ordinal WHERE job.company_id=$1 AND job.id=$2")
        .bind(queued.fence.scope.company.as_uuid()).bind(queued.fence.scope.job.0).fetch_one(f.s.pool()).await.unwrap();
    assert_eq!(
        saved,
        json!({"status":"completed","retry":0,"continuation":0,"leased":false,"attempt_status":"completed","stop":"agent_turn_parked","checkpoints":0})
    );
    assert!(p.confirm_suspension(queued.fence, wait).await.unwrap());
    let mut forged = queued.fence;
    forged.generation.0 = Uuid::new_v4();
    assert!(!p.confirm_suspension(forged, wait).await.unwrap());
    assert!(matches!(
        p.resume_wait(queued.fence.scope).await.unwrap(),
        WaitProgress::Pending
    ));
    cancel(&f, owner, "cancel-turn-owner").await;
    assert_eq!(pointer(&f).await, None);
    assert!(matches!(
        p.resume_wait(queued.fence.scope).await.unwrap(),
        WaitProgress::Continued
    ));
    assert_eq!(
        pointer(&f).await,
        Some(queued.fence.scope.execution.as_uuid())
    );
    // A fast wake does not invalidate the old park acknowledgement or manufacture a step result.
    assert!(p.confirm_suspension(queued.fence, wait).await.unwrap());
    assert!(matches!(
        p.resume_wait(queued.fence.scope).await.unwrap(),
        WaitProgress::Refused
    ));
    let resumed = p
        .claim_io(
            queued.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            policy(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        p.prepare_agent(&resumed, policy()).await.unwrap(),
        AgentPreparationResult::Ready(_)
    ));
    let counters: (i32, i16) = sqlx::query_as(
        "SELECT retry_count,continuation_count FROM background_tasks WHERE company_id=$1 AND id=$2",
    )
    .bind(resumed.fence.scope.company.as_uuid())
    .bind(resumed.fence.scope.job.0)
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    assert_eq!(counters, (0, 1));
}

#[tokio::test]
async fn agent_history_ordering_cancelled_queued_turn_never_releases_owner_or_blocks_next_position()
{
    let f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let cancelled = f.next().await;
    let wait = parked(&f, &cancelled).await;
    let next = f.next().await;
    parked(&f, &next).await;
    cancel(&f, &cancelled, "cancel-queued-turn").await;
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
    assert!(
        f.s.p()
            .confirm_suspension(cancelled.fence, wait)
            .await
            .unwrap()
    );
    assert!(matches!(
        f.s.p().resume_wait(next.fence.scope).await.unwrap(),
        WaitProgress::Pending
    ));
    cancel(&f, &owner, "cancel-first-turn").await;
    assert!(matches!(
        f.s.p().resume_wait(next.fence.scope).await.unwrap(),
        WaitProgress::Continued
    ));
    assert_eq!(
        pointer(&f).await,
        Some(next.fence.scope.execution.as_uuid())
    );
}

struct NoTools;
#[async_trait]
impl crate::application::workflow::agent::AgentToolDispatch for NoTools {
    async fn dispatch(
        &self,
        _: crate::application::workflow::lease::WorkflowFence,
        _: &crate::application::workflow::agent::PreparedTool,
        _: crate::application::workflow::actions::ModelToolCallId,
        _: Value,
    ) -> AppResult<crate::application::workflow::agent::AgentToolObservation> {
        panic!("this ordering fixture has no tools")
    }
}

fn runtime(
    f: &HistoryThread,
) -> crate::adapters::harness::rig::WorkflowAgentRuntime<PostgresPersistence, NoTools> {
    crate::adapters::harness::rig::WorkflowAgentRuntime::new(
        crate::adapters::harness::rig::providers::ProviderRegistry::standard().unwrap(),
        f.s.p().clone(),
        NoTools,
        None,
        policy(),
    )
}

#[tokio::test]
async fn agent_history_recovery_final_commit_releases_before_workflow_finish_and_poll_recovers_lost_wake()
 {
    use crate::application::workflow::{
        polling::{PollWork, WorkflowPolling, WorkflowStepKind},
        worker::WorkflowHandler,
    };
    use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
    let f = Box::pin(HistoryThread::new()).await;
    let llm = scripted_llm(vec![
        LlmTurn::text("\"first\""),
        LlmTurn::text("\"second\""),
    ])
    .await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    let queued = f.next().await;
    let p = f.s.p();
    let parked =
        Box::pin(runtime(&f).execute(&WorkflowStepKind("agent.run".into()), &queued)).await;
    let crate::application::workflow::lease::WorkflowHandlerOutcome::Suspended(parked) = parked
    else {
        panic!("queued handler must suspend without entering the provider")
    };
    let requests: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_harness_runs WHERE company_id=$1")
            .bind(owner.fence.scope.company.as_uuid())
            .fetch_one(f.s.pool())
            .await
            .unwrap();
    assert_eq!(requests, 0);
    let result = Box::pin(runtime(&f).execute(&WorkflowStepKind("agent.run".into()), &owner)).await;
    assert_eq!(crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome::completed_result(result).unwrap(),json!("first"));
    assert_eq!(pointer(&f).await, None);
    // No workflow completion or notification is needed. The accepted checkpoint owns release.
    let first:Value=sqlx::query_scalar("SELECT output FROM agent_history_exchanges WHERE company_id=$1 AND id=$2 AND state='completed'")
        .bind(owner.fence.scope.company.as_uuid()).bind(owner.fence.scope.execution.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    assert_eq!(first, json!("first"));
    let page = p.poll_work(None, 128).await.unwrap();
    assert!(
        page.candidates
            .iter()
            .any(|candidate| candidate.scope == queued.fence.scope
                && candidate.work == PollWork::Wait)
    );
    assert_eq!(p.sweep_waits(128).await.unwrap(), 1);
    assert!(
        p.confirm_suspension(queued.fence, parked.wait)
            .await
            .unwrap()
    );
    let resumed = p
        .claim_io(
            queued.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            policy(),
        )
        .await
        .unwrap()
        .unwrap();
    let result =
        Box::pin(runtime(&f).execute(&WorkflowStepKind("agent.run".into()), &resumed)).await;
    assert_eq!(crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome::completed_result(result).unwrap(),json!("second"));
    assert_eq!(pointer(&f).await, None);
    assert_eq!(llm.finish().await.unwrap(), 2);
}

#[path = "agent_history_capacity_tests.rs"]
mod capacity;

#[path = "agent_history_turn_ack_tests.rs"]
mod acknowledgement;

#[path = "agent_history_turn_guard_tests.rs"]
mod guards;

#[path = "agent_history_supervision_tests.rs"]
mod supervision;

#[path = "agent_history_selection_tests.rs"]
mod selection;
