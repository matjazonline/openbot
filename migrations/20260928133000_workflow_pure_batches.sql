-- Pure progression extends the execution owner and the existing job queue.
ALTER TABLE workflow_runs
    ADD COLUMN state text NOT NULL DEFAULT 'queued',
    ADD COLUMN waiting_reason text,
    ADD COLUMN terminal_execution_id uuid,
    ADD CONSTRAINT workflow_run_state CHECK (state IN ('queued','running','waiting','succeeded','failed','cancelled')),
    ADD CONSTRAINT workflow_run_waiting_shape CHECK (
        (state = 'waiting' AND waiting_reason IS NOT NULL AND waiting_reason IN ('decision','event','timer','child_run','effect','reconciliation'))
        OR (state <> 'waiting' AND waiting_reason IS NULL)),
    ADD CONSTRAINT workflow_run_terminal_execution_fk FOREIGN KEY (company_id,id,terminal_execution_id)
        REFERENCES workflow_executions(company_id,run_id,id);

ALTER TABLE workflow_executions
    ADD COLUMN frozen_choice text,
    ADD COLUMN committed_route text,
    ADD COLUMN route_target text,
    ADD COLUMN successor_execution_id uuid,
    ADD CONSTRAINT workflow_choice_shape CHECK (frozen_choice IS NULL OR
        (activated_at IS NOT NULL AND octet_length(frozen_choice) BETWEEN 1 AND 128)),
    ADD CONSTRAINT workflow_route_shape CHECK (
        (committed_route IS NULL AND route_target IS NULL AND successor_execution_id IS NULL)
        OR (completed_at IS NOT NULL AND committed_route IS NOT NULL
            AND octet_length(committed_route) BETWEEN 1 AND 140 AND route_target IS NOT NULL
            AND ((route_target = '$end' AND successor_execution_id IS NULL)
                OR (route_target <> '$end' AND octet_length(route_target) BETWEEN 1 AND 128
                    AND successor_execution_id IS NOT NULL)))),
    ADD CONSTRAINT workflow_successor_fk FOREIGN KEY (company_id,run_id,successor_execution_id)
        REFERENCES workflow_executions(company_id,run_id,id),
    ADD CONSTRAINT workflow_one_predecessor UNIQUE (company_id,run_id,successor_execution_id);

-- Audit existing violations before installing uniqueness (no duplicate is repaired silently).
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM background_tasks WHERE queue_kind = 'workflow'
        GROUP BY company_id,workflow_execution_id HAVING count(*) > 1) THEN
        RAISE EXCEPTION 'multiple jobs for one workflow execution';
    END IF;
END $$;
CREATE UNIQUE INDEX workflow_one_execution_job ON background_tasks(company_id,workflow_execution_id)
    WHERE queue_kind = 'workflow';

CREATE FUNCTION preserve_workflow_progression() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.activated_at IS NOT NULL AND NEW.frozen_choice IS DISTINCT FROM OLD.frozen_choice THEN
        RAISE EXCEPTION 'workflow rule choice is immutable' USING ERRCODE = '23514';
    END IF;
    IF OLD.committed_route IS NOT NULL AND
        (NEW.committed_route,NEW.route_target,NEW.successor_execution_id) IS DISTINCT FROM
        (OLD.committed_route,OLD.route_target,OLD.successor_execution_id) THEN
        RAISE EXCEPTION 'workflow route is immutable' USING ERRCODE = '23514';
    END IF;
    IF NEW.successor_execution_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM workflow_executions AS successor
        WHERE successor.company_id = NEW.company_id AND successor.run_id = NEW.run_id
            AND successor.id = NEW.successor_execution_id AND successor.step_id = NEW.route_target
            AND successor.activation = NEW.activation + 1) THEN
        RAISE EXCEPTION 'invalid workflow successor' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_preserve_progression BEFORE INSERT OR UPDATE ON workflow_executions
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_progression();
