use super::*;
use crate::adapters::persistence::workflow::agent_history_retention_fixture as clock;
use crate::application::workflow::{
    agent_history_views::*,
    run_views::{RunReadScope, RunViews},
    storage::WorkflowStorageMaintenance,
};

async fn conversation(f: &HistoryThread, claim: &ClaimedWorkflow) -> Uuid {
    sqlx::query_scalar(
        "SELECT conversation_id FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(claim.fence.scope.execution.as_uuid())
    .fetch_one(f.s.pool())
    .await
    .unwrap()
}
async fn complete(f: &HistoryThread, expected: &str) -> ClaimedWorkflow {
    let claim = f.next().await;
    assert_eq!(
        Box::pin(runtime(f).execute(&WorkflowStepKind("agent.run".into()), &claim))
            .await
            .completed_result()
            .unwrap(),
        json!(expected)
    );
    cancel(
        f,
        &claim,
        &format!("history-window-{}", claim.fence.scope.run.as_uuid()),
    )
    .await;
    claim
}

async fn assert_scope_pins(
    f: &HistoryThread,
    old: &ClaimedWorkflow,
    old_conversation: Uuid,
    old_scope: RunReadScope,
) {
    let p = f.s.p();
    for query in [
        "DELETE FROM agent_history_conversations WHERE company_id=$1 AND id=$2",
        "DELETE FROM agent_history_sessions WHERE company_id=$1 AND id=$2",
    ] {
        let target = if query.contains("conversations") {
            old_conversation
        } else {
            old.fence.scope.run.as_uuid()
        };
        assert!(
            sqlx::query(query)
                .bind(old.fence.scope.company.as_uuid())
                .bind(target)
                .execute(f.s.pool())
                .await
                .is_err()
        );
    }
    assert!(
        sqlx::query("DELETE FROM threads WHERE company_id=$1 AND id=$2")
            .bind(old.fence.scope.company.as_uuid())
            .bind(f.s.thread)
            .execute(f.s.pool())
            .await
            .is_err()
    );
    let retirement = crate::application::use_cases::agent::AgentPersistence::delete(p, f.agent)
        .await
        .unwrap_err();
    assert!(
        matches!(retirement, AppError::Conflict(_)),
        "published source/revision pins refuse destructive agent retirement: {retirement:?}"
    );
    assert!(
        p.agent_history_exchange(
            old_scope,
            old.fence.scope.execution.as_uuid(),
            AgentExchangeId::for_execution(old.fence.scope.execution)
        )
        .await
        .unwrap()
        .unwrap()
        .payload
        .is_some()
    );
}

#[tokio::test]
async fn agent_history_controls_native_scope_deletions_keep_retained_history_and_recreated_thread_window_rejects_old_cursor()
 {
    let f = Box::pin(HistoryThread::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"old\""), LlmTurn::text("\"new\"")]).await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    let old = complete(&f, "old").await;
    let old_conversation = conversation(&f, &old).await;
    let cursor = HistoryCursor::after(AgentConversationId::new(old_conversation), 1).unwrap();
    let p = f.s.p();
    let old_scope = RunReadScope {
        company: old.fence.scope.company,
        run: old.fence.scope.run,
        viewer: f.s.owner(),
    };
    assert_scope_pins(&f, &old, old_conversation, old_scope).await;
    clock::retire(p, old.fence.scope).await;
    assert!(
        p.agent_history_page(
            old_scope,
            old.fence.scope.execution.as_uuid(),
            HistoryPageRequest::new(None, None).unwrap()
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(p.prune_history(old.fence.scope.company, 32).await.unwrap() > 0);
    let gone: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_history_conversations WHERE company_id=$1 AND id=$2",
    )
    .bind(old.fence.scope.company.as_uuid())
    .bind(old_conversation)
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    assert_eq!(gone, 0);
    assert_new_window(&f, old_conversation, cursor).await;
    assert_eq!(
        llm.finish().await.unwrap(),
        2,
        "neither cleanup nor stale-anchor inspection regenerates output"
    );
}

async fn assert_new_window(f: &HistoryThread, old_conversation: Uuid, cursor: HistoryCursor) {
    let p = f.s.p();
    let current = complete(f, "new").await;
    assert_ne!(
        conversation(f, &current).await,
        old_conversation,
        "same actual agent/thread starts a fresh retained window"
    );
    let current_scope = RunReadScope {
        company: current.fence.scope.company,
        run: current.fence.scope.run,
        viewer: f.s.owner(),
    };
    assert!(
        p.agent_history_page(
            current_scope,
            current.fence.scope.execution.as_uuid(),
            HistoryPageRequest::new(Some(cursor.as_str().into()), None).unwrap()
        )
        .await
        .unwrap()
        .is_none()
    );
    let current_detail = p
        .agent_history_exchange(
            current_scope,
            current.fence.scope.execution.as_uuid(),
            AgentExchangeId::for_execution(current.fence.scope.execution),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(
        current_detail
            .payload
            .as_ref()
            .unwrap()
            .frozen
            .as_ref()
            .unwrap()
            .history
            .selected
            .is_empty()
    );
}
