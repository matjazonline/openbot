-- Link the already-supported execution-child admission. Runtime child invocation
-- and parent wait consumption stay with their phase07 owner.
ALTER TABLE workflow_runs
    ADD COLUMN parent_run_id uuid,
    ADD COLUMN parent_execution_id uuid;

DO $$
DECLARE cause record;
BEGIN
    FOR cause IN
        SELECT admission.company_id,admission.run_id,
               substring(admission.source_key FROM 4)::jsonb AS source
        FROM workflow_admissions AS admission
        WHERE admission.source_key LIKE 'v1:%'
    LOOP
        IF cause.source->>0 = 'child' THEN
            IF jsonb_array_length(cause.source) <> 5 OR cause.source->4 <> 'null'::jsonb
               OR NOT EXISTS (
                   SELECT 1 FROM workflow_executions AS parent
                   WHERE parent.company_id=cause.company_id
                     AND parent.run_id=(cause.source->>1)::uuid
                     AND parent.id=(cause.source->>2)::uuid
                     AND parent.step_id=cause.source->>3
               ) THEN
                RAISE EXCEPTION 'Invalid admitted execution-child cause' USING ERRCODE='23514';
            END IF;
            UPDATE workflow_runs SET parent_run_id=(cause.source->>1)::uuid,
                parent_execution_id=(cause.source->>2)::uuid
            WHERE company_id=cause.company_id AND id=cause.run_id;
        END IF;
    END LOOP;
END $$;

ALTER TABLE workflow_runs
    ADD CONSTRAINT workflow_parent_shape CHECK (
        (parent_run_id IS NULL AND parent_execution_id IS NULL)
        OR (parent_run_id IS NOT NULL AND parent_execution_id IS NOT NULL AND parent_run_id<>id)),
    ADD CONSTRAINT workflow_parent_execution_fk
        FOREIGN KEY (company_id,parent_run_id,parent_execution_id)
        REFERENCES workflow_executions(company_id,run_id,id);

CREATE FUNCTION preserve_workflow_parent() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.parent_run_id,NEW.parent_execution_id) IS DISTINCT FROM
       (OLD.parent_run_id,OLD.parent_execution_id) THEN
        RAISE EXCEPTION 'Admitted workflow parent is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_parent_immutable BEFORE UPDATE OF parent_run_id,parent_execution_id
    ON workflow_runs FOR EACH ROW EXECUTE FUNCTION preserve_workflow_parent();

CREATE FUNCTION check_workflow_parent_source() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE source jsonb; parent_run uuid; parent_execution uuid;
BEGIN
    IF TG_OP='UPDATE' AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow admission identity is immutable' USING ERRCODE='23514';
    END IF;
    source := substring(NEW.source_key FROM 4)::jsonb;
    SELECT run.parent_run_id,run.parent_execution_id INTO parent_run,parent_execution
      FROM workflow_runs AS run WHERE run.company_id=NEW.company_id AND run.id=NEW.run_id;
    IF source->>0='child' THEN
        IF jsonb_array_length(source)<>5 OR source->4<>'null'::jsonb
           OR parent_run IS DISTINCT FROM (source->>1)::uuid
           OR parent_execution IS DISTINCT FROM (source->>2)::uuid
           OR NOT EXISTS (SELECT 1 FROM workflow_executions AS execution
                WHERE execution.company_id=NEW.company_id AND execution.run_id=parent_run
                  AND execution.id=parent_execution AND execution.step_id=source->>3) THEN
            RAISE EXCEPTION 'Workflow admission parent mismatch' USING ERRCODE='23514';
        END IF;
    ELSIF parent_run IS NOT NULL THEN
        RAISE EXCEPTION 'Non-child admission has parent' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_admission_parent BEFORE INSERT OR UPDATE ON workflow_admissions
    FOR EACH ROW EXECUTE FUNCTION check_workflow_parent_source();

ALTER TABLE workflow_run_events
    ADD COLUMN terminal_state text,
    ADD COLUMN terminal_execution_id uuid,
    ADD CONSTRAINT workflow_wakeup_shape CHECK (
        (event_kind='parent_wakeup' AND terminal_state IS NOT NULL
            AND terminal_state IN ('succeeded','failed','cancelled') AND execution_id IS NULL)
        OR (event_kind<>'parent_wakeup' AND terminal_state IS NULL AND terminal_execution_id IS NULL)),
    ADD CONSTRAINT workflow_wakeup_execution_fk FOREIGN KEY (company_id,run_id,terminal_execution_id)
        REFERENCES workflow_executions(company_id,run_id,id);

CREATE FUNCTION preserve_workflow_wakeup() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.event_kind='parent_wakeup' OR NEW.event_kind='parent_wakeup') AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow parent wakeup is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_wakeup_immutable BEFORE UPDATE ON workflow_run_events
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_wakeup();

CREATE FUNCTION schedule_workflow_parent_wakeup() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.parent_run_id IS NOT NULL AND NEW.state IS DISTINCT FROM OLD.state
       AND NEW.state IN ('succeeded','failed','cancelled') THEN
        -- The UPDATE already owns this child run. Every FK below targets only
        -- its own rows: even a parent KEY SHARE here would invert tree ordering.
        INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,
                                        terminal_state,terminal_execution_id)
        SELECT NEW.company_id,NEW.id,COALESCE(MAX(event.sequence),0)+1,
               'parent_wakeup',NEW.actor_id,NEW.state,NEW.terminal_execution_id
        FROM workflow_run_events AS event WHERE event.company_id=NEW.company_id AND event.run_id=NEW.id;
    END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER workflow_terminal_parent_wakeup AFTER UPDATE OF state ON workflow_runs
    FOR EACH ROW EXECUTE FUNCTION schedule_workflow_parent_wakeup();

COMMENT ON COLUMN workflow_run_events.terminal_state IS
    'Immutable child terminal transition fact. Parent owner consumes in its own run-first transaction.';
