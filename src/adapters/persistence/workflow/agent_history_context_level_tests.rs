use super::*;
use crate::application::workflow::checkpoints::WorkflowCheckpointStore;
use crate::entities::access_level::AccessLevel;

pub(super) async fn team(f: &HistoryThread) {
    let company = f.s.fixture.target.company.as_uuid();
    sqlx::query("DELETE FROM thread_principals WHERE company_id=$1 AND thread_id=$2")
        .bind(company)
        .bind(f.s.thread)
        .execute(f.s.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT $1,$2,$3,id,'participant' FROM principals WHERE company_id=$1 AND user_id=$4")
        .bind(company).bind(f.s.channel).bind(f.s.thread).bind(f.s.owner().user_id()).execute(f.s.pool()).await.unwrap();
}

pub(super) async fn high_next(f: &HistoryThread) -> ClaimedWorkflow {
    let intent = f.s.intent(TO).await;
    team(f).await;
    Box::pin(f.claim_intent(intent)).await
}

pub(super) async fn public(f: &HistoryThread, claim: &ClaimedWorkflow) {
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT run.company_id,run.channel_id,run.thread_id,message.author_principal_id,'participant' FROM workflow_runs AS run JOIN workflow_message_intents AS intent ON intent.company_id=run.company_id AND intent.id=run.trigger_id JOIN messages AS message ON message.company_id=intent.company_id AND message.id=intent.message_id WHERE run.company_id=$1 AND run.id=$2 ON CONFLICT DO NOTHING")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.run.as_uuid()).execute(f.s.pool()).await.unwrap();
}

pub(super) async fn raise(f: &HistoryThread, claim: &ClaimedWorkflow, level: AccessLevel) {
    let mut tx = f.s.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        claim.fence.scope.company,
        claim.fence.scope.run,
        level,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

pub(super) async fn replace(f: &HistoryThread, claim: &ClaimedWorkflow) -> ClaimedWorkflow {
    assert!(
        f.s.p()
            .release_io(claim.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp() WHERE company_id=$1 AND id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(claim.fence.scope.job.0)
    .execute(f.s.pool())
    .await
    .unwrap();
    f.s.p()
        .claim_io(
            claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            policy(),
        )
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn agent_history_level_classified_current_input_empty_seed_refuses_initial_and_restarted_entry()
 {
    for stage in [Stage::BeforeCheckpoint, Stage::Replacement] {
        let f = Box::pin(HistoryThread::new()).await;
        let llm = scripted_llm(vec![]).await;
        register_scripted_agent_base_url(f.agent, &llm.base_url);
        let claim = high_next(&f).await;
        // The native run classification covers mandatory current input independently of history.
        raise(&f, &claim, AccessLevel::L5).await;
        let prepared =
            f.s.p()
                .prepare_agent(&claim, policy())
                .await
                .unwrap()
                .expect_ready();
        let before = frozen(&f, &claim).await;
        assert_eq!(before["history"]["seed"], json!([]));
        assert_eq!(before["history"]["selected_level"], 0);
        assert_eq!(before["history"]["content_level"], 5);
        let claim = match stage {
            Stage::BeforeCheckpoint => claim,
            Stage::Replacement => {
                f.s.p()
                    .open_workflow_checkpoint(
                        claim.fence,
                        &prepared.prepared.spec(claim.fence).unwrap(),
                        policy(),
                    )
                    .await
                    .unwrap();
                replace(&f, &claim).await
            }
        };
        public(&f, &claim).await;
        let result =
            Box::pin(runtime(&f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
        let refusal = result.completed_result().unwrap_err();
        assert_eq!(
            refusal.failure.code().as_str(),
            "knowledge.audience_below_content"
        );
        let levels = refusal
            .failure
            .knowledge_refusal()
            .expect("typed refusal facts");
        assert_eq!(levels.content, AccessLevel::L5);
        assert_eq!(levels.destination, AccessLevel::PUBLIC);
        assert_eq!(frozen(&f, &claim).await, before);
        assert_eq!(llm.finish().await.unwrap(), 0);
    }
}

enum Stage {
    BeforeCheckpoint,
    Replacement,
}

#[tokio::test]
async fn agent_history_level_classified_current_input_allows_unchanged_team_audience() {
    let f = Box::pin(HistoryThread::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"allowed\"")]).await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    let claim = high_next(&f).await;
    raise(&f, &claim, AccessLevel::L5).await;
    let prepared =
        f.s.p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready();
    let before = frozen(&f, &claim).await;
    assert_eq!(
        prepared.prepared.frozen_history().unwrap().selected_level,
        AccessLevel::PUBLIC
    );
    let result = Box::pin(runtime(&f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
    assert_eq!(result.completed_result().unwrap(), json!("allowed"));
    assert_eq!(frozen(&f, &claim).await, before);
    assert_eq!(llm.finish().await.unwrap(), 1);
}
