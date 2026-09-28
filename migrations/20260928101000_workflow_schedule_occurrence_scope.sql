-- A schedule may move channels; an already captured occurrence still executes
-- its immutable snapshot. Preserve that existing behavior, retaining tenant FK
-- and task/thread scope protection. This plan targets a fresh-database cutover;
-- it does not migrate preexisting business history through the earlier guard.
ALTER TABLE public.schedule_runs DROP CONSTRAINT schedule_runs_schedule_scope_fk;
ALTER TABLE public.channel_schedules DROP CONSTRAINT channel_schedules_run_scope_key;
ALTER TABLE public.channel_schedules ADD CONSTRAINT channel_schedules_company_id_key
    UNIQUE (company_id, id);
ALTER TABLE public.schedule_runs
    ADD CONSTRAINT schedule_runs_schedule_company_fk FOREIGN KEY (company_id, schedule_id)
        REFERENCES public.channel_schedules(company_id, id) ON DELETE CASCADE,
    ADD CONSTRAINT schedule_runs_channel_scope_fk FOREIGN KEY (company_id, channel_id)
        REFERENCES public.channels(company_id, id) ON DELETE CASCADE;

CREATE OR REPLACE FUNCTION public.require_legacy_schedule_task() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE owner_company uuid; owner_channel uuid;
BEGIN
    IF TG_OP = 'INSERT' THEN
        -- SHARE also conflicts with a non-key channel UPDATE: either we capture
        -- before the move or observe its committed scope, never a mixed snapshot.
        SELECT schedule.company_id, schedule.channel_id INTO owner_company, owner_channel
        FROM public.channel_schedules AS schedule WHERE schedule.id = NEW.schedule_id
        FOR SHARE;
        IF owner_company IS NULL
           OR (NEW.company_id IS NOT NULL AND NEW.company_id <> owner_company)
           OR (NEW.channel_id IS NOT NULL AND NEW.channel_id <> owner_channel) THEN
            RAISE EXCEPTION 'Schedule occurrence requires its actual owner scope at capture'
                USING ERRCODE = '23514', CONSTRAINT = 'legacy_schedule_owner_scope';
        END IF;
        NEW.company_id := owner_company;
        NEW.channel_id := owner_channel;
    ELSE
        IF (NEW.company_id, NEW.channel_id, NEW.schedule_id, NEW.schedule_snapshot)
            IS DISTINCT FROM
           (OLD.company_id, OLD.channel_id, OLD.schedule_id, OLD.schedule_snapshot) THEN
            RAISE EXCEPTION 'Captured schedule occurrence identity is immutable'
                USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_identity_immutable';
        END IF;
        IF (NEW.thread_id, NEW.task_id) IS NOT DISTINCT FROM (OLD.thread_id, OLD.task_id) THEN
            RETURN NEW;
        END IF;
    END IF;
    -- Incomplete snapshots already fail the application decoder. When an identity
    -- is present it must agree with the relational occurrence, including raw SQL.
    IF (NEW.schedule_snapshot ? 'id' AND NEW.schedule_snapshot->>'id' IS DISTINCT FROM NEW.schedule_id::text)
       OR (NEW.schedule_snapshot ? 'company_id' AND NEW.schedule_snapshot->>'company_id' IS DISTINCT FROM NEW.company_id::text)
       OR (NEW.schedule_snapshot ? 'channel_id' AND NEW.schedule_snapshot->>'channel_id' IS DISTINCT FROM NEW.channel_id::text) THEN
        RAISE EXCEPTION 'Schedule snapshot identity must match captured occurrence scope'
            USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_snapshot_scope';
    END IF;
    IF NEW.task_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.background_tasks AS task
        WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy'
          AND task.company_id = NEW.company_id AND task.channel_id = NEW.channel_id
          AND task.thread_id = NEW.thread_id
    ) THEN
        RAISE EXCEPTION 'Schedule result requires a visible legacy task in the occurrence scope'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_schedule_task_scope';
    END IF;
    RETURN NEW;
END;
$$;
