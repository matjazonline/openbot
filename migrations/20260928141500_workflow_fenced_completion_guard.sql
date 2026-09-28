-- The ownership used to close a workflow job must still be live at commit,
-- including when commit is delayed after the application's last clock check.
CREATE FUNCTION check_workflow_fenced_completion() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.queue_kind = 'workflow' AND OLD.status = 'processing' AND NEW.status = 'completed' THEN
        IF OLD.lock_expires_at IS NULL OR OLD.lock_expires_at <= clock_timestamp()
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id = execution.company_id AND run.id = execution.run_id
                JOIN task_attempts AS attempt ON attempt.task_id = NEW.id
                    AND attempt.attempt_number = OLD.retry_count + 1
                    AND attempt.worker_id = OLD.worker_id
                    AND attempt.execution_generation = OLD.execution_generation
                WHERE execution.company_id = NEW.company_id AND execution.id = NEW.workflow_execution_id
                    AND execution.completed_at IS NOT NULL AND execution.committed_route IS NOT NULL
                    AND attempt.status = 'completed' AND run.deadline > clock_timestamp()
            ) THEN
            RAISE EXCEPTION 'workflow completion lost ownership before commit' USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_fenced_completion_guard
    AFTER UPDATE ON background_tasks DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION check_workflow_fenced_completion();
