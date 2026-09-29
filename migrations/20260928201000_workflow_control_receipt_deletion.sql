-- Retain command identity until its owner is deleted. Cascading cleanup is
-- allowed only once the run/company is actually absent, not by trigger depth.
CREATE OR REPLACE FUNCTION preserve_workflow_control_receipt() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        IF EXISTS (SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
            AND EXISTS (SELECT 1 FROM companies WHERE id=OLD.company_id) THEN
            RAISE EXCEPTION 'workflow control receipts are immutable' USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'workflow control receipts are immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
DROP TRIGGER workflow_control_receipt_immutable ON workflow_control_commands;
CREATE TRIGGER workflow_control_receipt_immutable BEFORE UPDATE OR DELETE ON workflow_control_commands
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_control_receipt();
