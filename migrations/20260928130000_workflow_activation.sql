-- Activation is a run-wide sequence, not an attempt counter or per-step round.
-- Retain the existing (company, run, step, activation) identity constraint too.
ALTER TABLE workflow_executions
    ADD CONSTRAINT workflow_execution_run_ordinal UNIQUE (company_id, run_id, activation),
    ADD COLUMN activated_at timestamptz,
    ADD COLUMN frozen_inputs jsonb,
    ADD COLUMN completed_at timestamptz,
    ADD COLUMN committed_output jsonb,
    ADD CONSTRAINT workflow_execution_activation_shape CHECK (
        (activated_at IS NULL AND frozen_inputs IS NULL)
        OR (activated_at IS NOT NULL AND frozen_inputs IS NOT NULL
            AND jsonb_typeof(frozen_inputs) = 'object'
            AND octet_length(frozen_inputs::text) <= 1048576)),
    ADD CONSTRAINT workflow_execution_output_shape CHECK (
        (completed_at IS NULL AND committed_output IS NULL)
        OR (completed_at IS NOT NULL AND committed_output IS NOT NULL
            AND activated_at IS NOT NULL AND completed_at >= activated_at
            AND octet_length(committed_output::text) <= 1048576));

COMMENT ON COLUMN workflow_executions.activation IS
    'Positive run-wide logical ordinal. Allocate successors under the run lock; retries reuse it.';
COMMENT ON COLUMN workflow_executions.committed_output IS
    'Execution-owned immutable committed result; written with progression by the runtime result owner.';

CREATE FUNCTION preserve_workflow_execution_facts() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.activated_at IS NOT NULL AND
       (NEW.activated_at, NEW.frozen_inputs) IS DISTINCT FROM
       (OLD.activated_at, OLD.frozen_inputs) THEN
        RAISE EXCEPTION 'workflow activation inputs are immutable'
            USING ERRCODE = '23514', CONSTRAINT = 'workflow_activation_immutable';
    END IF;
    IF OLD.completed_at IS NOT NULL AND
       (NEW.completed_at, NEW.committed_output) IS DISTINCT FROM
       (OLD.completed_at, OLD.committed_output) THEN
        RAISE EXCEPTION 'workflow committed output is immutable'
            USING ERRCODE = '23514', CONSTRAINT = 'workflow_output_immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER workflow_executions_preserve_facts
BEFORE UPDATE OF activated_at, frozen_inputs, completed_at, committed_output
ON workflow_executions FOR EACH ROW EXECUTE FUNCTION preserve_workflow_execution_facts();
