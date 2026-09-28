-- A commit delayed after the application deadline check must still roll back.
CREATE FUNCTION check_workflow_progression_deadline() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.committed_route IS NOT NULL AND (TG_OP = 'INSERT' OR OLD.committed_route IS NULL)
        AND EXISTS (SELECT 1 FROM workflow_runs AS run
            WHERE run.company_id = NEW.company_id AND run.id = NEW.run_id
                AND run.deadline <= clock_timestamp()) THEN
        RAISE EXCEPTION 'workflow deadline reached before progression commit' USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_progression_commit_deadline
    AFTER INSERT OR UPDATE ON workflow_executions DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION check_workflow_progression_deadline();

ALTER TABLE workflow_run_events
    ADD COLUMN execution_id uuid,
    ADD CONSTRAINT workflow_run_event_execution_fk FOREIGN KEY (company_id,run_id,execution_id)
        REFERENCES workflow_executions(company_id,run_id,id),
    ADD CONSTRAINT workflow_run_event_execution_kind UNIQUE (company_id,run_id,execution_id,event_kind);
