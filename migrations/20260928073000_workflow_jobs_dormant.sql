-- Dormant workflow jobs: admission is enabled only after all legacy consumers are isolated.
ALTER TABLE background_tasks
    ADD COLUMN queue_kind text NOT NULL DEFAULT 'legacy',
    ADD COLUMN workflow_execution_id uuid,
    ADD CONSTRAINT background_tasks_queue_kind_check CHECK (queue_kind IN ('legacy', 'workflow')),
    ADD CONSTRAINT background_tasks_workflow_disabled CHECK (queue_kind = 'legacy'),
    ADD CONSTRAINT background_tasks_workflow_execution_fk
        FOREIGN KEY (company_id, workflow_execution_id)
        REFERENCES workflow_executions(company_id, id),
    ADD CONSTRAINT background_tasks_workflow_shape_check CHECK (
        (queue_kind = 'legacy' AND workflow_execution_id IS NULL)
        OR (queue_kind = 'workflow' AND workflow_execution_id IS NOT NULL
            AND task_type = 'workflow_execution'
            AND payload = jsonb_build_object('version', 1,
                'execution_id', workflow_execution_id::text)
            AND owner_principal_id IS NULL AND owner_principal_kind IS NULL
            AND source_message_uuid IS NULL AND source_schedule_run_id IS NULL
            AND awaited_outreach_id IS NULL AND wait_expires_at IS NULL
            AND transition_reason IS NULL AND transition_actor_kind IS NULL
            AND transition_actor_id IS NULL AND transition_approval_id IS NULL
            AND transition_outreach_id IS NULL)
    );

COMMENT ON CONSTRAINT background_tasks_workflow_disabled ON background_tasks IS
    'Temporary deployment gate: remove only after complete legacy query and nullable-channel isolation.';
COMMENT ON COLUMN background_tasks.workflow_execution_id IS
    'Logical workflow execution; attempts and fenced leases remain owned by background_tasks/task_attempts.';

CREATE FUNCTION preserve_background_task_queue_identity() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.queue_kind, NEW.workflow_execution_id) IS DISTINCT FROM
       (OLD.queue_kind, OLD.workflow_execution_id) THEN
        RAISE EXCEPTION 'background task queue identity is immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER background_tasks_preserve_queue_identity
BEFORE UPDATE OF queue_kind, workflow_execution_id ON background_tasks
FOR EACH ROW EXECUTE FUNCTION preserve_background_task_queue_identity();

-- Preserve the exact legacy trigger bodies; workflow jobs must not acquire legacy ownership,
-- create legacy audit records, wake legacy projections, or lock channel agent harnesses.
-- Channel nullability is deliberately unchanged until all legacy consumers are isolated.
DROP TRIGGER background_tasks_bump_attention_version ON background_tasks;
CREATE TRIGGER background_tasks_bump_attention_version BEFORE UPDATE OF owner_principal_id, owner_principal_kind ON public.background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'legacy') EXECUTE FUNCTION public.bump_task_attention_version_for_owner_change();
DROP TRIGGER background_tasks_initialize_ownership ON background_tasks;
CREATE TRIGGER background_tasks_initialize_ownership BEFORE INSERT ON public.background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'legacy') EXECUTE FUNCTION public.initialize_task_ownership();
DROP TRIGGER background_tasks_notify_activity ON background_tasks;
CREATE TRIGGER background_tasks_notify_activity AFTER INSERT OR UPDATE OF status ON public.background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'legacy' AND NEW.thread_id IS NOT NULL) EXECUTE FUNCTION public.notify_thread_activity();
DROP TRIGGER background_tasks_notify_attention ON background_tasks;
CREATE TRIGGER background_tasks_notify_attention AFTER INSERT OR UPDATE OF status, owner_principal_id, owner_principal_kind, business_priority, business_due_at, attention_version ON public.background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'legacy') EXECUTE FUNCTION public.notify_attention_changed('task');
CREATE TRIGGER background_tasks_notify_attention_delete AFTER DELETE ON background_tasks
FOR EACH ROW WHEN (OLD.queue_kind = 'legacy') EXECUTE FUNCTION public.notify_attention_changed('task');
DROP TRIGGER background_tasks_record_initial_ownership ON background_tasks;
CREATE TRIGGER background_tasks_record_initial_ownership AFTER INSERT ON public.background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'legacy') EXECUTE FUNCTION public.record_initial_task_ownership();
DROP TRIGGER background_tasks_record_status_event ON background_tasks;
CREATE TRIGGER background_tasks_record_status_event AFTER INSERT OR UPDATE OF status ON public.background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'legacy') EXECUTE FUNCTION public.record_task_status_event();
DROP TRIGGER lock_task_agent_harnesses ON background_tasks;
CREATE TRIGGER lock_task_agent_harnesses BEFORE INSERT OR UPDATE OF status, owner_principal_id, channel_id ON public.background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'legacy') EXECUTE FUNCTION public.lock_task_agent_harnesses();
DROP TRIGGER task_status_actionable_notification ON background_tasks;
CREATE TRIGGER task_status_actionable_notification AFTER UPDATE OF status ON public.background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'legacy') EXECUTE FUNCTION public.notification_from_task_status();
CREATE TRIGGER task_status_actionable_notification_delete AFTER DELETE ON background_tasks
FOR EACH ROW WHEN (OLD.queue_kind = 'legacy') EXECUTE FUNCTION public.notification_from_task_status();
