-- The legacy discriminator must be an equality key, not a post-scan filter: a workflow-heavy
-- correlation must not be read in full before LIMIT 1. Keep the preference order aligned with
-- THREAD_TASK_LOOKUP_SQL and retain task_type for PostgreSQL's index-only expression projection.
-- This transactional rebuild, like the original index build, blocks writes while it runs.
DROP INDEX public.background_tasks_thread_correlation_match_idx;
CREATE INDEX background_tasks_thread_correlation_match_idx
    ON public.background_tasks (
        company_id, thread_id, correlation_id, queue_kind,
        (task_type IN ('email_agent_dispatch', 'scheduled_agent_run')) DESC,
        created_at DESC, id ASC
    )
    INCLUDE (task_type)
    WHERE thread_id IS NOT NULL;
