-- Fail closed if the source writer cannot see a legacy task. Checking only for
-- visible workflow rows could miss a concurrent uncommitted workflow INSERT,
-- then let the FK wait for that row and accept it after commit.
CREATE OR REPLACE FUNCTION public.require_legacy_source_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.task_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.background_tasks AS task
        WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy'
    ) THEN
        RAISE EXCEPTION 'Legacy notification source requires a visible legacy task'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_source_task_queue_kind';
    END IF;
    -- Scoped FKs still reject foreign tasks and serialize concurrent deletion.
    -- Existing task queue identity cannot change after this visibility check.
    RETURN NEW;
END;
$$;
