-- Reconciliation is an immutable observation, never another scheduler or replay grant.
CREATE TABLE workflow_action_reconciliations (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    effect_kind text NOT NULL CHECK (effect_kind='remote'),
    failure_code text NOT NULL CHECK (failure_code ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$' AND octet_length(failure_code)<=128),
    observed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, invocation_id),
    FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind)
        REFERENCES workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind)
);

CREATE TRIGGER workflow_action_reconciliation_immutable BEFORE UPDATE OR DELETE ON workflow_action_reconciliations
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

CREATE FUNCTION workflow_action_reconciliation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE owner workflow_runs%ROWTYPE;
BEGIN
    -- Serialize with receipt insertion and terminal transitions using the existing run lock.
    SELECT * INTO owner FROM workflow_runs
        WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF NOT FOUND OR NOT (owner.state IN ('failed','cancelled')
            OR (owner.state='waiting' AND owner.waiting_reason='reconciliation'))
        OR EXISTS (SELECT 1 FROM workflow_action_receipts AS receipt
            WHERE receipt.company_id=NEW.company_id AND receipt.invocation_id=NEW.invocation_id)
    THEN
        RAISE EXCEPTION 'workflow reconciliation requires parked or terminal unresolved effect' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_reconciliation_guard BEFORE INSERT ON workflow_action_reconciliations
    FOR EACH ROW EXECUTE FUNCTION workflow_action_reconciliation_guard();

CREATE VIEW workflow_action_effect_states AS
SELECT intent.company_id, intent.run_id, intent.execution_id, intent.id AS invocation_id,
    intent.argument_digest,
    CASE WHEN receipt.invocation_id IS NOT NULL THEN 'committed'
        WHEN reconciliation.invocation_id IS NOT NULL THEN 'needs_reconciliation'
        WHEN marker.invocation_id IS NOT NULL THEN 'possible_dispatch'
        ELSE 'prepared' END AS effect_state
FROM workflow_action_intents AS intent
LEFT JOIN workflow_action_dispatches AS marker
    ON marker.company_id=intent.company_id AND marker.invocation_id=intent.id
LEFT JOIN workflow_action_receipts AS receipt
    ON receipt.company_id=intent.company_id AND receipt.invocation_id=intent.id
LEFT JOIN workflow_action_reconciliations AS reconciliation
    ON reconciliation.company_id=intent.company_id AND reconciliation.invocation_id=intent.id;
