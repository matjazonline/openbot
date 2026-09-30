-- Effect facts reference the existing task attempt; they never own another lease.
ALTER TABLE background_tasks ADD CONSTRAINT workflow_action_job_scope_key
    UNIQUE (company_id, workflow_execution_id, id);
ALTER TABLE task_attempts ADD CONSTRAINT workflow_action_attempt_fence_key
    UNIQUE (task_id, attempt_number, execution_generation, worker_id);

CREATE TABLE workflow_action_dispatches (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    id uuid NOT NULL,
    job_id uuid NOT NULL,
    attempt_number integer NOT NULL CHECK (attempt_number > 0),
    execution_generation uuid NOT NULL,
    worker_id uuid NOT NULL,
    effect_kind text NOT NULL CHECK (effect_kind IN ('local', 'remote')),
    current_policy jsonb NOT NULL CHECK (jsonb_typeof(current_policy)='object' AND octet_length(current_policy::text)<=524288),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, invocation_id),
    UNIQUE (company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind),
    FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest)
        REFERENCES workflow_action_intents(company_id, run_id, execution_id, id, argument_digest),
    FOREIGN KEY (company_id, execution_id, job_id)
        REFERENCES background_tasks(company_id, workflow_execution_id, id),
    FOREIGN KEY (job_id, attempt_number, execution_generation, worker_id)
        REFERENCES task_attempts(task_id, attempt_number, execution_generation, worker_id)
);

CREATE TABLE workflow_action_receipts (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    effect_kind text NOT NULL CHECK (effect_kind='local'),
    result jsonb NOT NULL CHECK (octet_length(result::text)<=131072),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, invocation_id),
    FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind)
        REFERENCES workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind)
);

CREATE FUNCTION workflow_action_fact_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'workflow action effect fact is append-only';
END
$$;
CREATE TRIGGER workflow_action_dispatch_immutable BEFORE UPDATE OR DELETE ON workflow_action_dispatches
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();
CREATE TRIGGER workflow_action_receipt_immutable BEFORE UPDATE OR DELETE ON workflow_action_receipts
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

CREATE FUNCTION workflow_action_dispatch_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- This deferred trigger checks wall time at COMMIT, including all lock waits.
    -- Lock run first so SQL-only writers cannot race cancellation or claim replacement.
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    PERFORM id FROM workflow_executions WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND id=NEW.execution_id FOR UPDATE;
    PERFORM id FROM background_tasks WHERE company_id=NEW.company_id AND id=NEW.job_id FOR UPDATE;
    PERFORM id FROM task_attempts WHERE task_id=NEW.job_id AND attempt_number=NEW.attempt_number FOR UPDATE;
    IF NOT EXISTS (
        SELECT 1 FROM workflow_runs AS run
        JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=job.id
        WHERE run.company_id=NEW.company_id AND run.id=NEW.run_id
          AND run.state IN ('queued','running') AND run.deadline>clock_timestamp()
          AND execution.id=NEW.execution_id AND execution.completed_at IS NULL
          AND job.id=NEW.job_id AND job.queue_kind='workflow' AND job.status='processing'
          AND job.worker_id=NEW.worker_id AND job.execution_generation=NEW.execution_generation
          AND job.retry_count+1=NEW.attempt_number AND job.lock_expires_at>clock_timestamp()
          AND attempt.attempt_number=NEW.attempt_number AND attempt.worker_id=NEW.worker_id
          AND attempt.execution_generation=NEW.execution_generation AND attempt.status='processing'
    ) THEN RAISE EXCEPTION 'workflow action dispatch lost live fence'; END IF;
    IF NEW.effect_kind='local' AND NOT EXISTS (
        SELECT 1 FROM workflow_action_receipts AS receipt
        WHERE receipt.company_id=NEW.company_id AND receipt.invocation_id=NEW.invocation_id
          AND receipt.dispatch_id=NEW.id
    ) THEN RAISE EXCEPTION 'local workflow action requires atomic receipt'; END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER workflow_action_dispatch_commit_guard
    AFTER INSERT ON workflow_action_dispatches DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION workflow_action_dispatch_commit_guard();
