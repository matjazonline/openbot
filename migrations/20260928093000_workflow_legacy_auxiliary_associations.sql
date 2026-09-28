-- These auxiliary owners only describe legacy work. Their scoped FKs preserve
-- tenant ownership and serialize deletion; attempts deliberately remain shared.
-- Requiring a visible legacy parent rejects invisible concurrent INSERTs before
-- an FK can wait for a row whose discriminator this statement never observed.
CREATE FUNCTION public.require_legacy_auxiliary_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
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

CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.task_outreaches
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.task_harness_runs
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.task_agent_instructions
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.start_agent_task_commands
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.task_channel_targets
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.task_ownership_events
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.task_status_events
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.delegation_control_commands
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.human_task_completions
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.task_approval_waits
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
CREATE TRIGGER require_legacy_auxiliary_task
    BEFORE INSERT OR UPDATE ON public.thread_handoff_runs
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
