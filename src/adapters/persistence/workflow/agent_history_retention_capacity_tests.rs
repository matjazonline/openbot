use super::*;
use crate::adapters::persistence::jsonb_test_support::LogCapture;
use crate::application::workflow::{
    agent::AgentPreparationResult,
    storage::{HISTORY_EXPIRY_SWEEP_LIMIT, WorkflowStorageMaintenance},
};
use tracing::instrument::WithSubscriber;

async fn fill_payload_slots(f: &mut ManualHistory) {
    // Keep each strict HTTP script within its existing scenario bound; all 127 calls are real.
    let mut actual_calls = 0;
    for start in (0..127).step_by(16) {
        let end = (start + 16).min(127);
        let llm = scripted_llm(
            (start..end)
                .map(|index| LlmTurn::text(index.to_string()))
                .collect(),
        )
        .await;
        register_scripted_agent_base_url(f.f.agent, &llm.base_url);
        for index in start..end {
            f.complete_ordinary(json!(index), json!(index)).await;
        }
        actual_calls += llm.finish().await.unwrap();
    }
    assert_eq!(actual_calls, 127);
}

#[tokio::test]
async fn agent_history_retention_actual_last_payload_slot_has_one_competing_producer_and_replay_survives_capacity()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    Box::pin(fill_payload_slots(&mut f)).await;
    let first = f.start(json!("last slot A")).await;
    let second = f.start(json!("last slot B")).await;
    let p = f.f.s.p();
    let logs = LogCapture::default();
    let (one, two) = async {
        tokio::join!(
            p.prepare_agent(&first, policy()),
            p.prepare_agent(&second, policy())
        )
    }
    .with_subscriber(logs.dispatch())
    .await;
    let events = logs.text();
    assert_eq!(events.matches("agent_history.refused").count(), 1);
    assert!(events.contains("history_capacity") && events.contains("not_applied"));
    assert!(!events.contains("last slot A") && !events.contains("last slot B"));
    let one = one.unwrap();
    let two = two.unwrap();
    let winner = match (&one, &two) {
        (AgentPreparationResult::Ready(_), AgentPreparationResult::AtCapacity) => &first,
        (AgentPreparationResult::AtCapacity, AgentPreparationResult::Ready(_)) => &second,
        _ => panic!("exactly one native producer must consume the final stored-payload slot"),
    };
    let before: Value=sqlx::query_scalar("SELECT jsonb_build_object('last',last_exchange_order,'payloads',
        (SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND payload_retained),
        'metadata',(SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1)) FROM agent_history_conversations WHERE company_id=$1")
        .bind(winner.fence.scope.company.as_uuid()).fetch_one(f.f.s.pool()).await.unwrap();
    assert_eq!(before, json!({"last":128,"payloads":128,"metadata":128}));
    let prepared = p
        .prepare_agent(winner, policy())
        .await
        .unwrap()
        .expect_ready()
        .prepared;
    assert_eq!(
        prepared.history_exchange(),
        Some(AgentExchangeId::for_execution(winner.fence.scope.execution))
    );
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM task_harness_runs WHERE company_id=$1 AND workflow_execution_id=ANY($2)")
        .bind(winner.fence.scope.company.as_uuid()).bind(vec![first.fence.scope.execution.as_uuid(),second.fence.scope.execution.as_uuid()])
        .fetch_one(f.f.s.pool()).await.unwrap();
    assert_eq!(
        count, 0,
        "capacity never opens a checkpoint or enters the model"
    );
    let page = p.history_page(None).await.unwrap();
    assert!(page.candidates.len() <= usize::from(HISTORY_EXPIRY_SWEEP_LIMIT));
    let mut expired = 0;
    for candidate in &page.candidates {
        expired += usize::from(p.expire_history(*candidate).await.unwrap());
    }
    assert_eq!(
        expired, 63,
        "retain the newest 64 terminal pairs plus the live winning activation"
    );
    let restored = p
        .prepare_agent(winner, policy())
        .await
        .unwrap()
        .expect_ready()
        .prepared;
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(prepared).unwrap(),
        "active frozen copy survives old-source payload expiry"
    );
    let after:Value=sqlx::query_scalar("SELECT jsonb_build_object('last',last_exchange_order,'expired',expired_count,'through',expired_through_order) FROM agent_history_conversations WHERE company_id=$1")
        .bind(winner.fence.scope.company.as_uuid()).fetch_one(f.f.s.pool()).await.unwrap();
    assert_eq!(after, json!({"last":128,"expired":63,"through":63}));
    let calls: i64 =
        sqlx::query_scalar("SELECT count(*) FROM model_usage_calls WHERE company_id=$1")
            .bind(winner.fence.scope.company.as_uuid())
            .fetch_one(f.f.s.pool())
            .await
            .unwrap();
    assert_eq!(
        calls, 127,
        "capacity contenders and frozen replay made zero provider calls"
    );
}

async fn fill_conversation_slots(f: &ManualHistory, session: Uuid) {
    use crate::application::use_cases::agent::{AgentPersistence, AgentWrite};
    let company = f.f.s.fixture.target.company.as_uuid();
    for index in 0..2047 {
        // Real native agents and scope FKs; no disabled guards or reduced policy limits.
        let agent = AgentPersistence::create(
            f.f.s.p(),
            company,
            AgentWrite {
                name: format!("Stored identity {index}"),
                slug: format!("stored-identity-{index}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO agent_history_conversations(company_id,id,agent_id,scope_kind,session_id) VALUES($1,$2,$3,'private_session',$4)")
            .bind(company).bind(Uuid::new_v4()).bind(agent.id).bind(session).execute(f.f.s.pool()).await.unwrap();
    }
}

struct IdentityWinner<'a> {
    identity: AgentConversationIdentity,
    owner: &'a ClaimedWorkflow,
    rejected: &'a ClaimedWorkflow,
}

async fn fill_identity_slots(f: &mut ManualHistory) -> ClaimedWorkflow {
    let seed = f.start(json!("seed session")).await;
    cancel(&f.f, &seed, "retire-empty-session-root").await;
    let session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM workflow_agent_sessions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(seed.fence.scope.company.as_uuid())
    .bind(seed.fence.scope.run.as_uuid())
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    fill_conversation_slots(f, session).await;
    seed
}

#[tokio::test]
async fn agent_history_controls_empty_scope_capacity_is_native_and_pruning_keeps_live_resolved_identity()
 {
    use crate::application::workflow::agent_history::ResolveAgentHistoryIdentity;
    let mut f = Box::pin(ManualHistory::new()).await;
    let seed = fill_identity_slots(&mut f).await;
    f.from_run = None;
    let first = f.start(json!("identity A")).await;
    f.from_run = None;
    let second = f.start(json!("identity B")).await;
    let p = f.f.s.p();
    let request = |claim: &ClaimedWorkflow| ResolveAgentHistoryIdentity {
        fence: claim.fence,
        lease_policy: policy(),
    };
    let (one, two) = tokio::join!(
        p.resolve_agent_history_identity(request(&first)),
        p.resolve_agent_history_identity(request(&second))
    );
    let race = match (one, two) {
        (Ok(Some(identity)), Err(AppError::Conflict(_))) => IdentityWinner {
            identity,
            owner: &first,
            rejected: &second,
        },
        (Err(AppError::Conflict(_)), Ok(Some(identity))) => IdentityWinner {
            identity,
            owner: &second,
            rejected: &first,
        },
        other => panic!("one genuine identity must consume the final conversation slot: {other:?}"),
    };
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_history_conversations WHERE company_id=$1")
            .bind(seed.fence.scope.company.as_uuid())
            .fetch_one(f.f.s.pool())
            .await
            .unwrap();
    assert_eq!(count, 2048);
    let session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM workflow_agent_sessions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(race.rejected.fence.scope.company.as_uuid())
    .bind(race.rejected.fence.scope.run.as_uuid())
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    let error=sqlx::query("INSERT INTO agent_history_conversations(company_id,id,agent_id,scope_kind,session_id) VALUES($1,$2,$3,'private_session',$4)")
        .bind(race.rejected.fence.scope.company.as_uuid()).bind(Uuid::new_v4()).bind(f.f.agent).bind(session).execute(f.f.s.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("agent_history_capacity")
    );
    assert_eq!(
        p.prune_history(seed.fence.scope.company, 32).await.unwrap(),
        32
    );
    let replay = p
        .resolve_agent_history_identity(request(race.owner))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        replay, race.identity,
        "an identity resolved before enqueue remains pinned by its actual live scope"
    );
    let lost_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_history_conversations WHERE company_id=$1 AND session_id=$2",
    )
    .bind(race.rejected.fence.scope.company.as_uuid())
    .bind(session)
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    assert_eq!(
        lost_count, 0,
        "failed identity allocation did not fabricate a successful empty window"
    );
}
