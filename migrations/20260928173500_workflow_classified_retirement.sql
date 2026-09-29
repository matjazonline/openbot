-- Failures belong to the existing fenced attempt ledger; diagnostics never enter it.
ALTER TABLE task_attempts
    ADD COLUMN workflow_failure_class text,
    ADD COLUMN workflow_failure_code text,
    ADD COLUMN workflow_retry_safety text,
    ADD COLUMN workflow_retirement text,
    ADD CONSTRAINT workflow_attempt_failure_shape CHECK (
        (workflow_failure_class IS NULL AND workflow_failure_code IS NULL
            AND workflow_retry_safety IS NULL AND workflow_retirement IS NULL)
        OR (status='failed' AND finished_at IS NOT NULL
            AND workflow_failure_class IS NOT NULL AND workflow_failure_class IN ('retryable','terminal')
            AND workflow_failure_code IS NOT NULL AND octet_length(workflow_failure_code) BETWEEN 1 AND 128
            AND workflow_retry_safety IS NOT NULL AND workflow_retry_safety IN ('safe','unknown')
            AND workflow_retirement IS NOT NULL AND workflow_retirement IN ('live','expired')));

CREATE FUNCTION check_workflow_fenced_retirement() RETURNS trigger LANGUAGE plpgsql AS $$
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
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_fenced_retirement_guard
    AFTER UPDATE ON background_tasks DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION check_workflow_fenced_retirement();
