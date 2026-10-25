//! The legacy task runtime's schema is gone, and nothing left in the catalog names it (p08 D4.8).
//!
//! The scan is the D4.3/D4.8 catalog check: a function body, constraint, index, trigger or view
//! that still names a dropped table or column would fail only when it runs, so it is caught here
//! instead.
use sqlx::PgConnection;

use super::test_support::test_pool;

/// The legacy runtime's tables, dropped in B4c-3a, and the channel-agent tables B4c-2 dropped.
const RETIRED_TABLES: &[&str] = &[
    "attention_source_events",
    "delegation_control_commands",
    "human_approvals",
    "human_task_completions",
    "manual_handoffs",
    "response_draft_evidence",
    "response_draft_publications",
    "response_drafts",
    "response_review_commands",
    "response_reviews",
    "start_agent_task_commands",
    "task_agent_instruction_notes",
    "task_agent_instructions",
    "task_approval_waits",
    "task_channel_targets",
    "task_harness_invocations",
    "task_outreach_replies",
    "task_outreach_targets",
    "task_outreaches",
    "task_ownership_events",
    "task_status_events",
    "thread_handoff_events",
    "thread_handoff_runs",
    "thread_handoffs",
    "agent_sub_agents",
    "channel_agents",
];

/// Columns whose names nothing live reuses, so a mention anywhere in the catalog is a leftover.
const RETIRED_COLUMN_NAMES: &[&str] = &[
    "source_message_uuid",
    "wait_expires_at",
    "transition_reason",
    "transition_actor_kind",
    "transition_actor_id",
    "transition_approval_id",
    "transition_outreach_id",
    "owner_principal_id",
    "owner_principal_kind",
    "ownership_version",
    "business_priority",
    "business_due_at",
    "attention_version",
    "awaited_outreach_id",
    "prompt_tokens",
    "completion_tokens",
    "assignment_email_enabled",
    "response_review_email_enabled",
    "delegation_timeout_email_enabled",
    "task_failure_email_enabled",
    "owner_agent_id",
    "response_trigger",
    "preferred_reviewer_principal_id",
    "external_response_review_override",
    "external_reply_handling_override",
    "default_participant_emails",
    "config_json",
    "run_timeout_secs",
];

const RETIRED_FUNCTIONS: &[&str] = &[
    "attention_source_events_are_immutable",
    "bump_handoff_version_for_responsibility_cleanup",
    "bump_task_attention_version_for_owner_change",
    "delete_channel_target_tasks",
    "enforce_outreach_status_transition",
    "enforce_outreach_target_status_transition",
    "enforce_response_draft_evidence_immutability",
    "enforce_response_draft_update",
    "notification_from_attention_source",
    "notification_from_outreach",
    "notification_from_review",
    "notification_from_task_ownership",
    "notification_from_task_status",
    "notification_withdraw_deleted_source",
    "notify_agent_instruction",
    "notify_task_chain_changed",
    "notify_task_ownership",
    "record_initial_task_ownership",
    "record_task_status_event",
    "release_tasks_for_removed_principal",
    "require_legacy_auxiliary_task",
    "require_legacy_handoff_event_task",
    "require_legacy_source_task",
    "task_ownership_events_are_immutable",
    "thread_handoff_events_are_immutable",
];

/// Stored values only the legacy runtime wrote, and distinctive enough that a mention anywhere is a
/// leftover. The generic ones (`stopped`, `dead_letter`, `task`) are live elsewhere and are left
/// to the narrowed checks.
const RETIRED_VALUES: &[&str] = &[
    "legacy",
    "pending_approval",
    "waiting_for_third_party_reply",
    "channel_agent_removed",
    "task_stopped",
    "ownership_transferred",
    "agent_instruction",
    "delegation_cancelled",
    "operator_stopped",
    "approval_rejected",
    "retryable_failure",
    "terminal_failure",
    "delegation_timeout",
    "task_failure",
    // The distinctive legacy `transition_reason`s; the generic ones (`claimed`, `completed`) are
    // live elsewhere.
    "approval_requested",
    "approval_accepted",
    "outreach_started",
    "outreach_reply_received",
    "outreach_timed_out",
    "outreach_extended",
    "operator_resumed",
    "delegation_target_cancelled",
    "delegation_reassigned",
    "delegation_partial",
    // The legacy-task link of an agent's creation provenance (p08 B4d).
    "source_task_id",
];

/// Every catalog object in `public` that names a retired table, column or function, or spells a
/// retired stored value, as `kind name: mention`.
async fn retired_mentions(connection: &mut PgConnection) -> Vec<String> {
    let names: Vec<&str> = RETIRED_TABLES
        .iter()
        .chain(RETIRED_COLUMN_NAMES)
        .chain(RETIRED_FUNCTIONS)
        .copied()
        .collect();
    let pattern = format!(
        r"\m({})\M|'({})'",
        names.join("|"),
        RETIRED_VALUES.join("|")
    );
    // The full definition rather than `prosrc`, which is empty for a SQL-standard body.
    sqlx::query_scalar(
        r#"WITH objects(kind, name, body) AS (
               SELECT 'function', proname::text, pg_get_functiondef(oid) FROM pg_proc
                WHERE pronamespace = 'public'::regnamespace AND prokind IN ('f', 'p')
               UNION ALL
               SELECT 'constraint', conname::text, pg_get_constraintdef(oid) FROM pg_constraint
                WHERE connamespace = 'public'::regnamespace
               UNION ALL
               SELECT 'index', indexname::text, indexdef FROM pg_indexes WHERE schemaname = 'public'
               UNION ALL
               SELECT 'trigger', tgname::text, pg_get_triggerdef(oid) FROM pg_trigger
                WHERE NOT tgisinternal
               UNION ALL
               SELECT 'view', viewname::text, definition FROM pg_views WHERE schemaname = 'public'
           )
           SELECT DISTINCT kind || ' ' || name || ': ' || COALESCE(mention[1], mention[2])
             FROM objects, regexp_matches(body, $1, 'g') AS found(mention)
            ORDER BY 1"#,
    )
    .bind(&pattern)
    .fetch_all(connection)
    .await
    .unwrap()
}

#[tokio::test]
async fn the_legacy_runtime_tables_columns_and_functions_are_gone() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = ANY($1) ORDER BY 1",
    )
    .bind(RETIRED_TABLES)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(tables.is_empty(), "{tables:?}");

    // The generic names (`task_id`, `result`, `error`) are live elsewhere, so they are checked on
    // the table they left rather than scanned for.
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT table_name || '.' || column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND (column_name = ANY($1) \
            OR (table_name, column_name) IN (('message_deliveries', 'task_id'), \
                ('task_attempts', 'result'), ('task_attempts', 'error'))) \
         ORDER BY 1",
    )
    .bind(RETIRED_COLUMN_NAMES)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(columns.is_empty(), "{columns:?}");

    let functions: Vec<String> = sqlx::query_scalar(
        "SELECT proname::text FROM pg_proc \
         WHERE pronamespace = 'public'::regnamespace AND proname = ANY($1) ORDER BY 1",
    )
    .bind(RETIRED_FUNCTIONS)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(functions.is_empty(), "{functions:?}");
}

#[tokio::test]
async fn no_catalog_object_names_the_legacy_runtime() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let mut connection = pool.acquire().await.unwrap();
    let mentions = retired_mentions(&mut connection).await;
    assert!(mentions.is_empty(), "{mentions:#?}");

    // The scan can see what it looks for, planted in a transaction no other test can observe and
    // rolled back: a PL/pgSQL function naming a dropped table -- its body is not resolved until it
    // runs, which is exactly the leftover this scan exists to catch -- and a SQL-standard body,
    // which `prosrc` leaves empty, spelling a retired value.
    let mut planted = pool.begin().await.unwrap();
    for canary in [
        "CREATE FUNCTION public.legacy_runtime_scan_canary() RETURNS text LANGUAGE plpgsql \
         AS $$ BEGIN RETURN (SELECT 'x' FROM task_outreaches LIMIT 1); END $$",
        "CREATE FUNCTION public.legacy_runtime_value_canary() RETURNS text LANGUAGE sql \
         RETURN 'pending_approval'",
    ] {
        sqlx::query(canary).execute(&mut *planted).await.unwrap();
    }
    assert_eq!(
        retired_mentions(&mut planted).await,
        vec![
            "function legacy_runtime_scan_canary: task_outreaches".to_owned(),
            "function legacy_runtime_value_canary: pending_approval".to_owned(),
        ]
    );
    planted.rollback().await.unwrap();
}
