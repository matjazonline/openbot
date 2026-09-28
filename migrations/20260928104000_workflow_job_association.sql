-- Keep workflow_disabled and channel NOT NULL until the coordinated isolation gate passes.
-- Runs pin their admission association; executions never move between runs.
CREATE FUNCTION preserve_workflow_run_association() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.company_id, NEW.id, NEW.channel_id, NEW.thread_id) IS DISTINCT FROM
       (OLD.company_id, OLD.id, OLD.channel_id, OLD.thread_id) THEN
        RAISE EXCEPTION 'workflow run association is immutable'
            USING ERRCODE = '23514', CONSTRAINT = 'workflow_run_association_immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER workflow_runs_preserve_association
BEFORE UPDATE OF company_id, id, channel_id, thread_id ON workflow_runs
FOR EACH ROW EXECUTE FUNCTION preserve_workflow_run_association();

CREATE FUNCTION preserve_workflow_execution_identity() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.company_id, NEW.id, NEW.run_id, NEW.step_id, NEW.activation) IS DISTINCT FROM
       (OLD.company_id, OLD.id, OLD.run_id, OLD.step_id, OLD.activation) THEN
        RAISE EXCEPTION 'workflow execution identity is immutable'
            USING ERRCODE = '23514', CONSTRAINT = 'workflow_execution_identity_immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER workflow_executions_preserve_identity
BEFORE UPDATE OF company_id, id, run_id, step_id, activation ON workflow_executions
FOR EACH ROW EXECUTE FUNCTION preserve_workflow_execution_identity();

CREATE FUNCTION validate_workflow_job_association() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    owning_run uuid;
    run_channel uuid;
    run_thread uuid;
BEGIN
    -- Link identity is immutable. Discover it before taking locks in run-first order.
    SELECT execution.run_id INTO owning_run
    FROM workflow_executions AS execution
    WHERE execution.company_id = NEW.company_id AND execution.id = NEW.workflow_execution_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'workflow job requires a visible scoped execution'
            USING ERRCODE = '23503', CONSTRAINT = 'background_tasks_workflow_execution_fk';
    END IF;
    SELECT run.channel_id, run.thread_id INTO run_channel, run_thread
    FROM workflow_runs AS run
    WHERE run.company_id = NEW.company_id AND run.id = owning_run
    FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'workflow job requires a visible scoped run'
            USING ERRCODE = '23503', CONSTRAINT = 'background_tasks_workflow_execution_fk';
    END IF;
    PERFORM 1 FROM workflow_executions AS execution
    WHERE execution.company_id = NEW.company_id AND execution.id = NEW.workflow_execution_id
      AND execution.run_id = owning_run
    FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'workflow job requires a visible scoped execution'
            USING ERRCODE = '23503', CONSTRAINT = 'background_tasks_workflow_execution_fk';
    END IF;
    IF NEW.channel_id IS DISTINCT FROM run_channel OR NEW.thread_id IS DISTINCT FROM run_thread THEN
        RAISE EXCEPTION 'workflow job association must equal its execution run association'
            USING ERRCODE = '23514', CONSTRAINT = 'background_tasks_workflow_association';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER background_tasks_validate_workflow_association
BEFORE INSERT OR UPDATE OF company_id, channel_id, thread_id, workflow_execution_id, queue_kind
ON background_tasks FOR EACH ROW WHEN (NEW.queue_kind = 'workflow')
EXECUTE FUNCTION validate_workflow_job_association();

-- Installed before dropping NOT NULL: legacy domain entities always retain their channel.
ALTER TABLE background_tasks ADD CONSTRAINT background_tasks_legacy_channel_required
    CHECK (queue_kind <> 'legacy' OR channel_id IS NOT NULL);
