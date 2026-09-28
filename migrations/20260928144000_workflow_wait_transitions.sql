-- Wait registration is execution-owned. It is not a dispatch queue.
ALTER TABLE workflow_waits
    DROP CONSTRAINT workflow_waits_check,
    ADD CONSTRAINT workflow_wait_deadline CHECK (reason='timer' OR deadline>created_at),
    ADD COLUMN state text NOT NULL DEFAULT 'waiting' CHECK (state IN ('waiting','completed','expired')),
    ADD COLUMN event_name text,
    ADD COLUMN correlation text,
    ADD COLUMN notification_intent jsonb,
    ADD COLUMN settled_at timestamptz,
    ADD COLUMN consumed_event_id uuid,
    ADD CONSTRAINT workflow_wait_execution UNIQUE (company_id,run_id,execution_id),
    ADD CONSTRAINT workflow_wait_settlement CHECK ((state='waiting') = (settled_at IS NULL)),
    ADD CONSTRAINT workflow_wait_event_name CHECK (event_name IS NULL OR octet_length(event_name) BETWEEN 1 AND 128),
    ADD CONSTRAINT workflow_wait_correlation CHECK (correlation IS NULL OR octet_length(correlation) BETWEEN 1 AND 256),
    ADD CONSTRAINT workflow_wait_intent CHECK (notification_intent IS NULL OR
        (jsonb_typeof(notification_intent)='object' AND notification_intent->>'version'='1'
        AND notification_intent->>'wait_id'=id::text
        AND octet_length(notification_intent::text)<=2048));
CREATE TABLE workflow_wait_events (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    id uuid NOT NULL,
    event_name text NOT NULL CHECK (octet_length(event_name) BETWEEN 1 AND 128),
    correlation text NOT NULL CHECK (octet_length(correlation) BETWEEN 1 AND 256),
    payload jsonb NOT NULL CHECK (octet_length(payload::text)<=1048576),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id,id),
    UNIQUE (company_id,run_id,execution_id,id),
    FOREIGN KEY (company_id,run_id,execution_id)
        REFERENCES workflow_executions(company_id,run_id,id) ON DELETE CASCADE
);
ALTER TABLE workflow_waits ADD CONSTRAINT workflow_wait_consumed_event
    FOREIGN KEY (company_id,run_id,execution_id,consumed_event_id)
        REFERENCES workflow_wait_events(company_id,run_id,execution_id,id);
CREATE FUNCTION check_workflow_wait_commit() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.notification_intent IS NOT NULL AND
       (TG_OP='INSERT' OR (OLD.state='waiting' AND NEW.state='completed')) THEN
        IF EXISTS (SELECT 1 FROM workflow_runs AS run WHERE run.company_id=NEW.company_id
            AND run.id=NEW.run_id AND run.deadline<=clock_timestamp()) OR
           (NEW.reason='event' AND NEW.deadline<=clock_timestamp()) THEN
            RAISE EXCEPTION 'workflow wait deadline reached before commit' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_wait_commit_deadline AFTER INSERT OR UPDATE ON workflow_waits
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION check_workflow_wait_commit();
