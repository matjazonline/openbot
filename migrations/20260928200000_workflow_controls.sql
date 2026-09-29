-- Existing writers participate in one monotonic control revision. Heartbeats and
-- timestamp-only bookkeeping do not change logical state; claim/fence, retry
-- debit, activation/result, and wait settlement do. Owners already lock run first.
ALTER TABLE workflow_runs ADD COLUMN revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0);
CREATE FUNCTION advance_workflow_run_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.revision<>OLD.revision AND pg_trigger_depth()=1 THEN
        RAISE EXCEPTION 'workflow revision is database generated' USING ERRCODE='23514';
    END IF;
    IF (to_jsonb(NEW)-'revision') IS DISTINCT FROM (to_jsonb(OLD)-'revision')
        OR NEW.revision<>OLD.revision THEN
        IF OLD.revision=9223372036854775807 THEN
            RAISE EXCEPTION 'workflow revision exhausted' USING ERRCODE='23514';
        END IF;
        NEW.revision=OLD.revision+1;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_run_revision BEFORE UPDATE ON workflow_runs
    FOR EACH ROW EXECUTE FUNCTION advance_workflow_run_revision();

CREATE FUNCTION advance_workflow_owned_revision() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE owner_company uuid; owner_run uuid;
BEGIN
    IF TG_OP='UPDATE' AND (to_jsonb(NEW)-ARRAY['updated_at','locked_at','lock_expires_at'])
        IS NOT DISTINCT FROM (to_jsonb(OLD)-ARRAY['updated_at','locked_at','lock_expires_at']) THEN
        RETURN NULL;
    END IF;
    IF TG_TABLE_NAME='background_tasks' THEN
        IF NEW.queue_kind<>'workflow' THEN RETURN NULL; END IF;
        SELECT company_id,run_id INTO owner_company,owner_run FROM workflow_executions
            WHERE company_id=NEW.company_id AND id=NEW.workflow_execution_id;
    ELSE
        owner_company=NEW.company_id; owner_run=NEW.run_id;
    END IF;
    UPDATE workflow_runs SET revision=revision+1 WHERE company_id=owner_company AND id=owner_run;
    RETURN NULL;
END $$;
CREATE TRIGGER workflow_execution_revision AFTER INSERT OR UPDATE ON workflow_executions
    FOR EACH ROW EXECUTE FUNCTION advance_workflow_owned_revision();
CREATE TRIGGER workflow_job_revision AFTER INSERT OR UPDATE ON background_tasks
    FOR EACH ROW EXECUTE FUNCTION advance_workflow_owned_revision();
CREATE TRIGGER workflow_wait_revision AFTER INSERT OR UPDATE ON workflow_waits
    FOR EACH ROW EXECUTE FUNCTION advance_workflow_owned_revision();

CREATE TABLE workflow_control_commands (
    company_id uuid NOT NULL,
    command_key text NOT NULL CHECK (octet_length(command_key) BETWEEN 1 AND 128),
    run_id uuid NOT NULL,
    actor_id uuid NOT NULL,
    operation text NOT NULL CHECK (operation IN ('cancel','retry')),
    expected_revision bigint NOT NULL CHECK (expected_revision > 0),
    result text NOT NULL CHECK (result IN ('applied','terminal','unsafe','conflict')),
    result_revision bigint NOT NULL CHECK (result_revision > 0),
    audit_sequence bigint,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,command_key),
    FOREIGN KEY(company_id,run_id) REFERENCES workflow_runs(company_id,id) ON DELETE CASCADE,
    FOREIGN KEY(company_id,run_id,audit_sequence) REFERENCES workflow_run_events(company_id,run_id,sequence),
    CHECK ((result='applied')=(audit_sequence IS NOT NULL)),
    CHECK ((result<>'terminal' OR operation='cancel') AND (result<>'unsafe' OR operation='retry'))
);
CREATE FUNCTION preserve_workflow_control_receipt() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'workflow control receipts are immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_control_receipt_immutable BEFORE UPDATE ON workflow_control_commands
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_control_receipt();
ALTER TABLE workflow_waits DROP CONSTRAINT workflow_wait_state,
    ADD CONSTRAINT workflow_wait_state CHECK (state IN ('waiting','completed','expired','failed','cancelled'));

ALTER TABLE task_attempts DROP CONSTRAINT workflow_attempt_failure_shape,
    ADD CONSTRAINT workflow_attempt_failure_shape CHECK (
        (workflow_failure_class IS NULL AND workflow_failure_code IS NULL
            AND workflow_retry_safety IS NULL AND workflow_retirement IS NULL)
        OR (status='failed' AND finished_at IS NOT NULL
            AND workflow_failure_class IS NOT NULL AND workflow_failure_class IN ('retryable','terminal')
            AND workflow_failure_code IS NOT NULL AND octet_length(workflow_failure_code) BETWEEN 1 AND 128
            AND workflow_retry_safety IS NOT NULL AND workflow_retry_safety IN ('safe','unknown')
            AND workflow_retirement IS NOT NULL AND workflow_retirement IN ('live','expired','deadline','cancel')));

CREATE OR REPLACE FUNCTION check_workflow_fenced_retirement() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE retirement task_attempts%ROWTYPE;
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='processing' AND NEW.status IN ('pending','failed') THEN
        SELECT * INTO retirement FROM task_attempts WHERE task_id=OLD.id
            AND attempt_number=OLD.retry_count+1 AND execution_generation=OLD.execution_generation
            AND worker_id=OLD.worker_id AND status='failed';
        IF NOT FOUND OR retirement.workflow_retirement IS NULL OR NEW.retry_count<>OLD.retry_count+1
            OR (retirement.workflow_retirement='live' AND OLD.lock_expires_at<=clock_timestamp())
            OR (retirement.workflow_retirement='expired' AND OLD.lock_expires_at>clock_timestamp())
            OR (NEW.status='pending' AND (retirement.workflow_retry_safety<>'safe'
                OR retirement.workflow_failure_class<>'retryable' OR NEW.retry_count>=NEW.max_retries
                OR NEW.run_at<=retirement.finished_at)) THEN
            RAISE EXCEPTION 'workflow failure lost ownership or retry eligibility' USING ERRCODE='23514';
        END IF;
        IF retirement.workflow_retirement='deadline' AND (
            NEW.status<>'failed' OR retirement.workflow_failure_class<>'terminal'
            OR retirement.workflow_failure_code<>'workflow.run_deadline'
            OR retirement.workflow_retry_safety<>'unknown'
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.deadline<=clock_timestamp() AND run.state='failed'
                    AND execution.successor_execution_id IS NULL)) THEN
            RAISE EXCEPTION 'invalid workflow deadline retirement' USING ERRCODE='23514';
        END IF;
        IF retirement.workflow_retirement='cancel' AND (
            NEW.status<>'failed' OR retirement.workflow_failure_class<>'terminal'
            OR retirement.workflow_failure_code<>'workflow.cancelled'
            OR retirement.workflow_retry_safety<>'unknown'
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.state='cancelled' AND execution.completed_at IS NULL
                    AND execution.successor_execution_id IS NULL)) THEN
            RAISE EXCEPTION 'invalid workflow cancellation retirement' USING ERRCODE='23514';
        END IF;
        -- A retirement may fail or suspend an expired run, but cannot schedule
        -- a retry or successor after its original deadline, including commit delay.
        IF (NEW.status='pending' OR EXISTS (
                SELECT 1 FROM workflow_executions WHERE company_id=NEW.company_id
                    AND id=NEW.workflow_execution_id AND successor_execution_id IS NOT NULL))
            AND EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.deadline<=clock_timestamp()) THEN
            RAISE EXCEPTION 'workflow retirement scheduled work after deadline' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NULL;
END $$;

-- Shared predicate for the writer and deferred guard. Status changes during the
-- control; immutable activation and last-attempt evidence remain the authority.
CREATE FUNCTION workflow_control_retry_safe(company uuid, run uuid, job uuid)
RETURNS boolean LANGUAGE sql AS $$
    SELECT EXISTS (
        SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
        WHERE owner.company_id=company AND owner.id=run AND task.id=job AND task.queue_kind='workflow'
            AND owner.deadline>clock_timestamp() AND execution.activation<=owner.max_steps
            AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
            AND execution.committed_output IS NULL AND execution.committed_route IS NULL
            AND execution.successor_execution_id IS NULL
            AND task.retry_count>0 AND task.retry_count<task.max_retries
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND attempt.workflow_retry_safety='safe' AND attempt.workflow_retirement IN ('live','expired')
            AND attempt.workflow_failure_code NOT IN ('workflow.activation_limit','workflow.invalid_result','workflow.run_deadline')
            AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
            AND NOT EXISTS (SELECT 1 FROM workflow_executions AS sibling
                JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                WHERE sibling.company_id=company AND sibling.run_id=run AND other.queue_kind='workflow'
                    AND other.id<>job AND other.status IN ('pending','processing'))
    )
$$;
CREATE FUNCTION check_workflow_control_retry() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE owner workflow_runs%ROWTYPE;
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='failed' AND NEW.status='pending' THEN
        SELECT run.* INTO owner FROM workflow_runs AS run JOIN workflow_executions AS execution
            ON execution.company_id=run.company_id AND execution.run_id=run.id
            WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id;
        IF NEW.retry_count<>OLD.retry_count OR NEW.max_retries<>OLD.max_retries
            OR NEW.workflow_execution_id<>OLD.workflow_execution_id OR NEW.id<>OLD.id
            OR owner.state<>'running' OR owner.terminal_execution_id IS NOT NULL
            OR NOT workflow_control_retry_safe(NEW.company_id,owner.id,NEW.id)
            OR NOT EXISTS (SELECT 1 FROM workflow_control_commands AS receipt
                WHERE receipt.company_id=owner.company_id AND receipt.run_id=owner.id
                    AND receipt.operation='retry' AND receipt.result='applied'
                    AND receipt.result_revision=owner.revision) THEN
            RAISE EXCEPTION 'invalid explicit workflow retry' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_control_retry_guard AFTER UPDATE ON background_tasks
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION check_workflow_control_retry();
