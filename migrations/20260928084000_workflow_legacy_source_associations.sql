-- Legacy source notifications cannot recover queue provenance after task deletion:
-- drafts/deliveries SET NULL(task_id), while approvals cascade. Prevent these
-- associations in the first place. task_attempts deliberately remains shared.
-- The deployment guard still excludes workflow rows, so no backfill is needed.
CREATE FUNCTION public.require_legacy_source_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.task_id IS NOT NULL AND EXISTS (
        SELECT 1 FROM public.background_tasks AS task
        WHERE task.id = NEW.task_id AND task.queue_kind <> 'legacy'
    ) THEN
        RAISE EXCEPTION 'Legacy notification source cannot reference a workflow task'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_source_task_queue_kind';
    END IF;
    -- Existing scoped FKs reject missing/foreign tasks and serialize deletion.
    -- Queue identity is immutable, so no later reclassification can race this check.
    RETURN NEW;
END;
$$;

CREATE TRIGGER require_legacy_source_task
    BEFORE INSERT OR UPDATE ON public.response_drafts
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_source_task();
CREATE TRIGGER require_legacy_source_task
    BEFORE INSERT OR UPDATE ON public.message_deliveries
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_source_task();
CREATE TRIGGER require_legacy_source_task
    BEFORE INSERT OR UPDATE ON public.human_approvals
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_source_task();
