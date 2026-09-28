-- A wakeup is a generated terminal transition, not a caller-provided event.
ALTER TABLE workflow_runs ADD COLUMN terminal_wakeup_sequence bigint
    CHECK (terminal_wakeup_sequence > 0);

-- Reconcile supported admitted children completed before parent wakeups existed.
-- Existing immutable facts remain untouched. Unsupported pre-v1/invalid source
-- shapes are not compatibility formats; the preceding migration fails on them.
UPDATE workflow_runs AS run SET terminal_wakeup_sequence=latest.sequence
FROM (SELECT event.company_id,event.run_id,MAX(event.sequence) AS sequence
      FROM workflow_run_events AS event WHERE event.event_kind='parent_wakeup'
      GROUP BY event.company_id,event.run_id) AS latest
WHERE run.company_id=latest.company_id AND run.id=latest.run_id;

DO $$
DECLARE child record; next_sequence bigint;
BEGIN
    FOR child IN SELECT run.company_id,run.id,run.actor_id,run.state,run.terminal_execution_id
        FROM workflow_runs AS run
        WHERE run.parent_run_id IS NOT NULL AND run.state IN ('succeeded','failed','cancelled')
          AND run.terminal_wakeup_sequence IS NULL
    LOOP
        SELECT COALESCE(MAX(event.sequence),0)+1 INTO next_sequence FROM workflow_run_events AS event
            WHERE event.company_id=child.company_id AND event.run_id=child.id;
        UPDATE workflow_runs SET terminal_wakeup_sequence=next_sequence
            WHERE company_id=child.company_id AND id=child.id;
        INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,
                                        terminal_state,terminal_execution_id)
        VALUES(child.company_id,child.id,next_sequence,'parent_wakeup',child.actor_id,
               child.state,child.terminal_execution_id);
    END LOOP;
END $$;

CREATE FUNCTION allocate_workflow_wakeup_sequence() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.terminal_wakeup_sequence IS NOT NULL THEN
            RAISE EXCEPTION 'Workflow wakeup sequence is generated' USING ERRCODE='23514';
        END IF;
    ELSE
        IF NEW.terminal_wakeup_sequence IS DISTINCT FROM OLD.terminal_wakeup_sequence THEN
            RAISE EXCEPTION 'Workflow wakeup sequence is generated' USING ERRCODE='23514';
        END IF;
        IF NEW.parent_run_id IS NOT NULL AND NEW.state IS DISTINCT FROM OLD.state
           AND NEW.state IN ('succeeded','failed','cancelled') THEN
            SELECT COALESCE(MAX(event.sequence),0)+1 INTO NEW.terminal_wakeup_sequence
                FROM workflow_run_events AS event
                WHERE event.company_id=NEW.company_id AND event.run_id=NEW.id;
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_allocate_wakeup BEFORE INSERT OR UPDATE ON workflow_runs
    FOR EACH ROW EXECUTE FUNCTION allocate_workflow_wakeup_sequence();

CREATE FUNCTION validate_workflow_wakeup_insert() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.event_kind='parent_wakeup' AND NOT EXISTS (
        SELECT 1 FROM workflow_runs AS child
        WHERE child.company_id=NEW.company_id AND child.id=NEW.run_id
          AND child.parent_run_id IS NOT NULL AND child.terminal_wakeup_sequence=NEW.sequence
          AND child.state=NEW.terminal_state AND child.actor_id=NEW.actor_id
          AND child.terminal_execution_id IS NOT DISTINCT FROM NEW.terminal_execution_id
    ) THEN
        RAISE EXCEPTION 'Workflow wakeup does not match generated terminal transition' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_validate_wakeup BEFORE INSERT ON workflow_run_events
    FOR EACH ROW EXECUTE FUNCTION validate_workflow_wakeup_insert();

CREATE OR REPLACE FUNCTION schedule_workflow_parent_wakeup() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.parent_run_id IS NOT NULL AND NEW.state IS DISTINCT FROM OLD.state
       AND NEW.state IN ('succeeded','failed','cancelled') THEN
        -- No parent lookup/FK/lock: this transaction owns only the child tree.
        INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,
                                        terminal_state,terminal_execution_id)
        VALUES(NEW.company_id,NEW.id,NEW.terminal_wakeup_sequence,'parent_wakeup',NEW.actor_id,
               NEW.state,NEW.terminal_execution_id);
    END IF;
    RETURN NULL;
END $$;
