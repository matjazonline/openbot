-- A status event's optional approval/outreach links are SET NULL during aggregate
-- deletion. That update keeps the already validated task association unchanged,
-- even when the cascading parent task is no longer visible in this transaction.
-- Only new/reassigned associations require visibility; queue identity is immutable.
CREATE OR REPLACE FUNCTION public.require_legacy_auxiliary_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF NEW.task_id IS NOT DISTINCT FROM OLD.task_id
           AND NEW.company_id IS NOT DISTINCT FROM OLD.company_id THEN
            RETURN NEW;
        END IF;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM public.background_tasks AS task
        WHERE task.id = NEW.task_id AND task.company_id = NEW.company_id
          AND task.queue_kind = 'legacy'
    ) THEN
        RAISE EXCEPTION 'Legacy auxiliary association requires a visible scoped legacy task'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_auxiliary_task_queue_kind';
    END IF;
    RETURN NEW;
END;
$$;
