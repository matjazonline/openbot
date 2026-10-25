//! What is left of agent and channel runtime configuration: none. An agent is a system prompt,
//! a model and its grants, run as a workflow model step; a channel is an address whose automation
//! is a workflow binding (p08 B4c).
use super::test_support::test_pool;

#[tokio::test]
async fn retired_agent_and_channel_runtime_columns_and_tables_are_gone() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT table_name || '.' || column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND (table_name, column_name) IN ( \
             ('agents', 'harness_kind'), ('agents', 'granted_tool_ids'), \
             ('agents', 'native_tool_policy'), ('agents', 'config_json'), \
             ('agents', 'run_timeout_secs'), ('agents', 'response_contract'), \
             ('channels', 'owner_agent_id'), ('channels', 'response_trigger'), \
             ('channels', 'external_response_review_override'), \
             ('channels', 'external_reply_handling_override'), \
             ('channels', 'preferred_reviewer_principal_id'), \
             ('companies', 'external_response_review'), \
             ('companies', 'external_reply_handling'), \
             ('companies', 'default_participant_emails')) \
         ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(columns.is_empty(), "{columns:?}");
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables WHERE table_schema = 'public' \
           AND table_name IN ('agent_mcp_selections', 'agent_mcp_selection_revisions', \
                              'agent_channel_provisions', 'agent_sub_agents', 'channel_agents') \
         ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(tables.is_empty(), "{tables:?}");
    let functions: Vec<String> = sqlx::query_scalar(
        "SELECT proname::text FROM pg_proc WHERE pronamespace = 'public'::regnamespace \
           AND proname IN ('enforce_channel_agent_scope', 'enforce_owned_channel_position_zero', \
                           'prevent_assigned_library_agent_delete', 'prevent_owned_channel_delete', \
                           'initialize_task_ownership') \
         ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(functions.is_empty(), "{functions:?}");
}
