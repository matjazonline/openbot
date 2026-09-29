-- Administrative expiry is fenced by the run deadline and the exact current attempt.
ALTER TABLE task_attempts DROP CONSTRAINT workflow_attempt_failure_shape,
    ADD CONSTRAINT workflow_attempt_failure_shape CHECK (
        (workflow_failure_class IS NULL AND workflow_failure_code IS NULL
            AND workflow_retry_safety IS NULL AND workflow_retirement IS NULL)
        OR (status='failed' AND finished_at IS NOT NULL
            AND workflow_failure_class IS NOT NULL AND workflow_failure_class IN ('retryable','terminal')
            AND workflow_failure_code IS NOT NULL AND octet_length(workflow_failure_code) BETWEEN 1 AND 128
            AND workflow_retry_safety IS NOT NULL AND workflow_retry_safety IN ('safe','unknown')
            AND workflow_retirement IS NOT NULL AND workflow_retirement IN ('live','expired','deadline')));

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
