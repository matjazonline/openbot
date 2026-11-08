use super::*;
use crate::application::workflow::{
    agent::AgentPreparationResult, storage::WorkflowStorageMaintenance,
};
#[path = "agent_history_capacity_prefix_fixture.rs"]
mod prefix;
use prefix::PayloadPrefix;

struct Contenders {
    first: ClaimedWorkflow,
    second: ClaimedWorkflow,
}
async fn contend(f: &mut ManualHistory) -> Contenders {
    let first = f.start(json!("metadata contender A")).await;
    let second = f.start(json!("metadata contender B")).await;
    let p = f.f.s.p();
    let (one, two) = tokio::join!(
        p.prepare_agent(&first, policy()),
        p.prepare_agent(&second, policy())
    );
    assert!(matches!(
        (one.unwrap(), two.unwrap()),
        (
            AgentPreparationResult::Ready(_),
            AgentPreparationResult::AtCapacity
        ) | (
            AgentPreparationResult::AtCapacity,
            AgentPreparationResult::Ready(_)
        )
    ));
    let calls: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_harness_runs WHERE company_id=$1")
            .bind(first.fence.scope.company.as_uuid())
            .fetch_one(f.f.s.pool())
            .await
            .unwrap();
    assert_eq!(
        calls, 0,
        "historical counts and capacity contenders never open a model checkpoint"
    );
    Contenders { first, second }
}
async fn assert_native_capacity(f: &ManualHistory) {
    // Deliberate whole-row replay attempt: assert the native capacity guard itself,
    // rather than accepting a duplicate-key or initial-state failure as capacity proof.
    let error=sqlx::query("INSERT INTO agent_history_exchanges SELECT exchange.* FROM agent_history_exchanges AS exchange WHERE company_id=$1 ORDER BY position LIMIT 1")
        .bind(f.f.s.fixture.target.company.as_uuid()).execute(f.f.s.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("agent_history_capacity")
    );
}

async fn count(f: &ManualHistory) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1")
        .bind(f.f.s.fixture.target.company.as_uuid())
        .fetch_one(f.f.s.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn agent_history_retention_conversation_metadata_actual_limit_blocks_competitors_with_expired_payload_prefix()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let prefix = prefix::seed(&mut f, 4095, PayloadPrefix::Expired).await;
    f.from_run = Some(prefix.scope.run);
    let race = contend(&mut f).await;
    assert_eq!(count(&f).await, 4096);
    assert_native_capacity(&f).await;
    let slots:i64=sqlx::query_scalar("SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND conversation_id=$2 AND payload_retained")
        .bind(prefix.scope.company.as_uuid()).bind(prefix.conversation).fetch_one(f.f.s.pool()).await.unwrap();
    assert_eq!(
        slots, 1,
        "expired metadata continues to consume metadata capacity, not payload capacity"
    );
    assert_ne!(
        race.first.fence.scope.execution,
        race.second.fence.scope.execution
    );
}

#[tokio::test]
async fn agent_history_retention_company_metadata_actual_limit_is_independent_of_payload_and_conversation_limits()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    for index in 0..8 {
        prefix::seed(
            &mut f,
            if index == 7 { 4095 } else { 4096 },
            PayloadPrefix::Expired,
        )
        .await;
    }
    assert_eq!(count(&f).await, 32767);
    // Separate fresh scope for each genuine contender; their private identities cannot share a stream.
    f.from_run = None;
    let first = f.start(json!("company contender A")).await;
    f.from_run = None;
    let second = f.start(json!("company contender B")).await;
    let p = f.f.s.p();
    let (one, two) = tokio::join!(
        p.prepare_agent(&first, policy()),
        p.prepare_agent(&second, policy())
    );
    assert!(matches!(
        (one.unwrap(), two.unwrap()),
        (
            AgentPreparationResult::Ready(_),
            AgentPreparationResult::AtCapacity
        ) | (
            AgentPreparationResult::AtCapacity,
            AgentPreparationResult::Ready(_)
        )
    ));
    assert_eq!(count(&f).await, 32768);
    assert_native_capacity(&f).await;
    let payloads: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND payload_retained",
    )
    .bind(first.fence.scope.company.as_uuid())
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    assert_eq!(payloads, 1);
}

#[tokio::test]
async fn agent_history_retention_company_payload_actual_limit_and_pressure_expire_sparse_streams_to_soft_ceiling()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    for index in 0..33 {
        prefix::seed(
            &mut f,
            if index == 32 { 63 } else { 62 },
            PayloadPrefix::Retained,
        )
        .await;
    }
    assert_eq!(count(&f).await, 2047);
    f.from_run = None;
    let first = f.start(json!("company payload A")).await;
    f.from_run = None;
    let second = f.start(json!("company payload B")).await;
    let p = f.f.s.p();
    let (one, two) = tokio::join!(
        p.prepare_agent(&first, policy()),
        p.prepare_agent(&second, policy())
    );
    assert!(matches!(
        (one.unwrap(), two.unwrap()),
        (
            AgentPreparationResult::Ready(_),
            AgentPreparationResult::AtCapacity
        ) | (
            AgentPreparationResult::AtCapacity,
            AgentPreparationResult::Ready(_)
        )
    ));
    assert_eq!(count(&f).await, 2048);
    assert_native_capacity(&f).await;
    let mut cursor = None;
    let mut expired = 0;
    for _ in 0..4 {
        let page = p.history_page(cursor).await.unwrap();
        assert_eq!(page.candidates.len(), 128);
        for candidate in page.candidates {
            cursor = Some(candidate);
            expired += usize::from(p.expire_history(candidate).await.unwrap());
        }
    }
    assert_eq!(
        expired, 512,
        "company pressure frees space even though every stream has fewer than64 terminal pairs"
    );
    let retained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND payload_retained",
    )
    .bind(first.fence.scope.company.as_uuid())
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    assert_eq!(retained, 1536);
    let page = p.history_page(cursor).await.unwrap();
    for candidate in page.candidates {
        assert!(
            !p.expire_history(candidate).await.unwrap(),
            "pressure expiry stops at its real soft ceiling"
        );
    }
}
