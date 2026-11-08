use super::*;
use crate::adapters::persistence::workflow::{
    agent_history_race_fixture::{OwnerGate, Target},
    agent_history_retention_fixture as clock,
};
use crate::application::workflow::storage::WorkflowStorageMaintenance;

async fn snapshot(f: &ManualHistory, claim: &ClaimedWorkflow) -> Value {
    sqlx::query_scalar(
        "SELECT frozen_preparation FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(claim.fence.scope.execution.as_uuid())
    .fetch_one(f.f.s.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn agent_history_retention_actual_preparation_owner_skips_competing_cleanup_and_replays_exact_copy()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"prior accepted\"")]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let prior = f
        .complete(json!("prior input"), json!("prior accepted"))
        .await;
    clock::age(f.f.s.p(), prior.fence.scope).await;
    let candidate = clock::candidate(f.f.s.p(), prior.fence.scope).await;
    let current = f.start(json!("current input")).await;
    let p = f.f.s.p();
    let gate = OwnerGate::install(
        p.pool(),
        Target::Preparation(current.fence.scope.execution.as_uuid()),
    )
    .await;
    let producer = p.prepare_agent(&current, policy());
    let competing_cleanup = async {
        gate.arrived(current.fence.scope.company.as_uuid()).await;
        assert!(!p.expire_history(candidate).await.unwrap());
        gate.release().await;
    };
    let (prepared, ()) = tokio::join!(producer, competing_cleanup);
    let prepared = prepared.unwrap().expect_ready().prepared;
    let frozen = snapshot(&f, &current).await;
    assert_eq!(
        frozen["history"]["seed"],
        json!([{"input":"prior input","output":"prior accepted"}])
    );
    assert!(p.expire_history(candidate).await.unwrap());
    let replay = p
        .prepare_agent(&current, policy())
        .await
        .unwrap()
        .expect_ready()
        .prepared;
    assert_eq!(replay.frozen_history(), prepared.frozen_history());
    assert_eq!(snapshot(&f, &current).await, frozen);
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_retention_actual_expiry_owner_precedes_blocked_fresh_selection_without_mixed_seed()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"expired answer\"")]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let prior = f
        .complete(json!("expired input"), json!("expired answer"))
        .await;
    clock::age(f.f.s.p(), prior.fence.scope).await;
    let candidate = clock::candidate(f.f.s.p(), prior.fence.scope).await;
    let current = f.start(json!("fresh input")).await;
    let p = f.f.s.p();
    let gate = OwnerGate::install(
        p.pool(),
        Target::Expiry(prior.fence.scope.execution.as_uuid()),
    )
    .await;
    let producer = p.expire_history(candidate);
    let competing_preparation = async {
        let expiry_owner = gate.arrived(current.fence.scope.company.as_uuid()).await;
        let preparation = p.prepare_agent(&current, policy());
        let release = async {
            OwnerGate::blocked_by(p.pool(), expiry_owner).await;
            gate.release().await;
        };
        let (prepared, ()) = tokio::join!(preparation, release);
        prepared.unwrap().expect_ready()
    };
    let (expired, _prepared) = tokio::join!(producer, competing_preparation);
    assert!(expired.unwrap());
    let frozen = snapshot(&f, &current).await;
    assert_eq!(frozen["history"]["seed"], json!([]));
    assert_eq!(frozen["history"]["selected"], json!([]));
    let facts: Value = sqlx::query_scalar("SELECT jsonb_build_object('last',last_exchange_order,'expired',expired_count,'through',expired_through_order) FROM agent_history_conversations WHERE company_id=$1")
        .bind(current.fence.scope.company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(facts, json!({"last":2,"expired":1,"through":1}));
    assert_eq!(snapshot(&f, &current).await, frozen);
    assert_eq!(llm.finish().await.unwrap(), 1);
}
