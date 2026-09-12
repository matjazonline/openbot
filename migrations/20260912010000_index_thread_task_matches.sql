-- Match one task per displayed message without scanning/sorting its whole correlation history.
-- Keep this ordering aligned with THREAD_TASK_LOOKUP_SQL, including the ascending UUID tie-breaker.
-- The source-message unique key continues to serve exact matches; other correlation consumers
-- retain background_tasks_correlation_idx.
CREATE INDEX background_tasks_thread_correlation_match_idx
    ON public.background_tasks (
        company_id, thread_id, correlation_id,
        (task_type IN ('email_agent_dispatch', 'scheduled_agent_run')) DESC,
        created_at DESC, id ASC
    )
    -- PostgreSQL also needs the expression's input available for an index-only projection.
    -- Without this, it can prefer the smaller correlation index followed by a history-wide sort.
    INCLUDE (task_type)
    WHERE thread_id IS NOT NULL;
