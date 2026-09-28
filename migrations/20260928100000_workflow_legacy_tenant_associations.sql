-- Scope keys close the single-column FK gap and protect against parent updates.
-- Existing incompatible rows fail migration validation; never silently repair ownership.
ALTER TABLE public.agent_channel_provisions ADD COLUMN company_id uuid;
UPDATE public.agent_channel_provisions AS provision
SET company_id = task.company_id
FROM public.background_tasks AS task WHERE task.id = provision.task_id;
ALTER TABLE public.schedule_runs ADD COLUMN company_id uuid, ADD COLUMN channel_id uuid;
UPDATE public.schedule_runs AS run
SET company_id = schedule.company_id, channel_id = schedule.channel_id
FROM public.channel_schedules AS schedule WHERE schedule.id = run.schedule_id;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM public.agent_channel_provisions AS provision
        JOIN public.background_tasks AS task ON task.id = provision.task_id
        WHERE task.queue_kind <> 'legacy'
    ) OR EXISTS (
        SELECT 1 FROM public.schedule_runs AS run
        JOIN public.background_tasks AS task ON task.id = run.task_id
        WHERE task.queue_kind <> 'legacy'
    ) THEN
        RAISE EXCEPTION 'Existing provisioning/schedule task association is not legacy';
    END IF;
END;
$$;

ALTER TABLE public.agent_channel_provisions
    ALTER COLUMN company_id SET NOT NULL,
    ADD CONSTRAINT agent_channel_provisions_task_scope_fk FOREIGN KEY (company_id, task_id)
        REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE,
    ADD CONSTRAINT agent_channel_provisions_agent_scope_fk FOREIGN KEY (company_id, agent_id)
        REFERENCES public.agents(company_id, id) ON DELETE CASCADE,
    ADD CONSTRAINT agent_channel_provisions_channel_scope_fk FOREIGN KEY (company_id, channel_id)
        REFERENCES public.channels(company_id, id) ON DELETE CASCADE;

-- These keys exist only to support the association FKs, not query optimization.
ALTER TABLE public.channel_schedules ADD CONSTRAINT channel_schedules_run_scope_key
    UNIQUE (company_id, channel_id, id);
ALTER TABLE public.background_tasks ADD CONSTRAINT background_tasks_schedule_scope_key
    UNIQUE (company_id, channel_id, thread_id, id);
ALTER TABLE public.schedule_runs
    ALTER COLUMN company_id SET NOT NULL, ALTER COLUMN channel_id SET NOT NULL,
    ADD CONSTRAINT schedule_runs_schedule_scope_fk FOREIGN KEY (company_id, channel_id, schedule_id)
        REFERENCES public.channel_schedules(company_id, channel_id, id) ON DELETE CASCADE,
    ADD CONSTRAINT schedule_runs_thread_scope_fk FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES public.threads(company_id, channel_id, id) ON DELETE SET NULL (thread_id),
    ADD CONSTRAINT schedule_runs_task_scope_fk FOREIGN KEY (company_id, channel_id, thread_id, task_id)
        REFERENCES public.background_tasks(company_id, channel_id, thread_id, id)
        ON DELETE SET NULL (task_id);

CREATE FUNCTION public.require_legacy_provision_task() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE task_company uuid;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF (NEW.company_id, NEW.task_id, NEW.agent_id, NEW.channel_id)
            IS NOT DISTINCT FROM (OLD.company_id, OLD.task_id, OLD.agent_id, OLD.channel_id) THEN
            RETURN NEW;
        END IF;
    END IF;
    SELECT task.company_id INTO task_company FROM public.background_tasks AS task
    WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy';
    IF task_company IS NULL
       OR (NEW.company_id IS NOT NULL AND NEW.company_id <> task_company)
       OR NOT EXISTS (SELECT 1 FROM public.agents AS agent
                      WHERE agent.id = NEW.agent_id AND agent.company_id = task_company)
       OR NOT EXISTS (SELECT 1 FROM public.channels AS channel
                      WHERE channel.id = NEW.channel_id AND channel.company_id = task_company) THEN
        RAISE EXCEPTION 'Provision requires a visible legacy task and same-company owners'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_provision_task_scope';
    END IF;
    NEW.company_id := task_company;
    RETURN NEW;
END;
$$;
CREATE TRIGGER require_legacy_provision_task BEFORE INSERT OR UPDATE
    ON public.agent_channel_provisions FOR EACH ROW
    EXECUTE FUNCTION public.require_legacy_provision_task();

CREATE FUNCTION public.require_legacy_schedule_task() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE owner_company uuid; owner_channel uuid;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF (NEW.company_id, NEW.channel_id, NEW.schedule_id, NEW.thread_id, NEW.task_id)
            IS NOT DISTINCT FROM
           (OLD.company_id, OLD.channel_id, OLD.schedule_id, OLD.thread_id, OLD.task_id) THEN
            RETURN NEW;
        END IF;
    END IF;
    SELECT schedule.company_id, schedule.channel_id INTO owner_company, owner_channel
    FROM public.channel_schedules AS schedule WHERE schedule.id = NEW.schedule_id;
    IF owner_company IS NULL
       OR (NEW.company_id IS NOT NULL AND NEW.company_id <> owner_company)
       OR (NEW.channel_id IS NOT NULL AND NEW.channel_id <> owner_channel) THEN
        RAISE EXCEPTION 'Schedule occurrence requires its actual owner scope'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_schedule_owner_scope';
    END IF;
    NEW.company_id := owner_company;
    NEW.channel_id := owner_channel;
    IF NEW.task_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.background_tasks AS task
        WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy'
          AND task.company_id = owner_company AND task.channel_id = owner_channel
          AND task.thread_id = NEW.thread_id
    ) THEN
        RAISE EXCEPTION 'Schedule result requires a visible legacy task in the occurrence scope'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_schedule_task_scope';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER require_legacy_schedule_task BEFORE INSERT OR UPDATE
    ON public.schedule_runs FOR EACH ROW EXECUTE FUNCTION public.require_legacy_schedule_task();
