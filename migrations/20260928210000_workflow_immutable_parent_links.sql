-- Repeated UPDATEs of a run in one transaction can recheck even an unchanged
-- PostgreSQL FK. Keep the cross-tree FK on immutable child-owned identity, so
-- revision/result updates never acquire a parent execution lock.
ALTER TABLE workflow_runs ADD CONSTRAINT workflow_run_parent_identity
    UNIQUE (company_id,id,parent_run_id,parent_execution_id);

CREATE TABLE workflow_run_parents (
    company_id uuid NOT NULL,
    child_run_id uuid NOT NULL,
    parent_run_id uuid NOT NULL,
    parent_execution_id uuid NOT NULL,
    PRIMARY KEY (company_id,child_run_id),
    UNIQUE (company_id,child_run_id,parent_run_id,parent_execution_id),
    CONSTRAINT workflow_parent_execution_fk
        FOREIGN KEY (company_id,parent_run_id,parent_execution_id)
        REFERENCES workflow_executions(company_id,run_id,id),
    CONSTRAINT workflow_parent_owner_fk
        FOREIGN KEY (company_id,child_run_id,parent_run_id,parent_execution_id)
        REFERENCES workflow_runs(company_id,id,parent_run_id,parent_execution_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED
);

INSERT INTO workflow_run_parents(company_id,child_run_id,parent_run_id,parent_execution_id)
    SELECT company_id,id,parent_run_id,parent_execution_id FROM workflow_runs
    WHERE parent_run_id IS NOT NULL;

ALTER TABLE workflow_runs
    ADD CONSTRAINT workflow_run_parent_link_fk
        FOREIGN KEY (company_id,id,parent_run_id,parent_execution_id)
        REFERENCES workflow_run_parents(company_id,child_run_id,parent_run_id,parent_execution_id),
    DROP CONSTRAINT workflow_parent_execution_fk;

CREATE FUNCTION insert_workflow_parent_link() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.parent_run_id IS NOT NULL THEN
        -- Owner FK is deferred until this run INSERT has finished. Both rows
        -- roll back together on any admission/commit failure.
        INSERT INTO workflow_run_parents(company_id,child_run_id,parent_run_id,parent_execution_id)
            VALUES(NEW.company_id,NEW.id,NEW.parent_run_id,NEW.parent_execution_id);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_parent_link BEFORE INSERT ON workflow_runs
    FOR EACH ROW EXECUTE FUNCTION insert_workflow_parent_link();

CREATE FUNCTION preserve_workflow_parent_link() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow parent link is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_parent_link_immutable BEFORE UPDATE ON workflow_run_parents
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_parent_link();

COMMENT ON TABLE workflow_run_parents IS
    'Immutable child-owned lineage identity; parent FK is checked at admission, never by run progression.';
