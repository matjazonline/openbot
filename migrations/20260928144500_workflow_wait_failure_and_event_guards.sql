ALTER TABLE workflow_waits
    DROP CONSTRAINT workflow_waits_state_check,
    ADD CONSTRAINT workflow_wait_state CHECK (state IN ('waiting','completed','expired','failed')),
    ADD COLUMN failure_code text,
    ADD CONSTRAINT workflow_wait_failure CHECK ((state='failed') = (failure_code IS NOT NULL)
        AND (failure_code IS NULL OR failure_code IN ('activation_limit','invalid_output')));
CREATE FUNCTION immutable_workflow_wait_event() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'workflow event facts are immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_wait_event_immutable BEFORE UPDATE ON workflow_wait_events
    FOR EACH ROW EXECUTE FUNCTION immutable_workflow_wait_event();
