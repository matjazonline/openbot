-- Handoff history intentionally outlives tasks. Validate new references without
-- adding a task FK or making historical reads depend on a currently live task.
CREATE FUNCTION public.require_legacy_handoff_event_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.task_id IS NULL THEN
        RETURN NEW;
    END IF;
    -- Unlike the live auxiliary owners, events have no FK to serialize deletion.
    -- Lock the visible scoped parent through commit, then retain only its identity.
    -- Queue identity is immutable; KEY SHARE also prevents a company-key change.
    PERFORM 1 FROM public.background_tasks AS task
    WHERE task.id = NEW.task_id AND task.company_id = NEW.company_id
      AND task.queue_kind = 'legacy'
    FOR KEY SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Handoff event requires a visible scoped legacy task'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_handoff_event_task_provenance';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER require_legacy_handoff_event_task
    BEFORE INSERT ON public.thread_handoff_events
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_handoff_event_task();
