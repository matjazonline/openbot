-- Preparation records are content, never grants or dispatch permission.
CREATE TABLE workflow_action_intents (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    id uuid NOT NULL,
    operation_key text NOT NULL CHECK (octet_length(operation_key) BETWEEN 1 AND 128),
    operation_digest text NOT NULL CHECK (operation_digest ~ '^[0-9a-f]{64}$'),
    argument_digest text NOT NULL CHECK (argument_digest ~ '^[0-9a-f]{64}$'),
    idempotency_key text NOT NULL CHECK (idempotency_key = 'workflow-action:v1:' || operation_digest),
    operation jsonb NOT NULL CHECK (jsonb_typeof(operation) = 'object' AND octet_length(operation::text) <= 524288),
    policy_decision text NOT NULL CHECK (policy_decision IN ('unevaluated', 'approval_required')),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, run_id, execution_id, operation_digest),
    UNIQUE (company_id, run_id, execution_id, id, argument_digest),
    UNIQUE (company_id, idempotency_key),
    FOREIGN KEY (company_id, run_id, execution_id)
        REFERENCES workflow_executions(company_id, run_id, id) ON DELETE CASCADE
);

CREATE TABLE workflow_action_model_calls (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    model_call_id text NOT NULL CHECK (octet_length(model_call_id) BETWEEN 1 AND 128),
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, run_id, execution_id, model_call_id),
    FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest)
        REFERENCES workflow_action_intents(company_id, run_id, execution_id, id, argument_digest)
        ON DELETE CASCADE
);

CREATE FUNCTION workflow_action_intent_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'workflow action intent is immutable';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER workflow_action_intent_immutable BEFORE UPDATE ON workflow_action_intents
    FOR EACH ROW EXECUTE FUNCTION workflow_action_intent_immutable();
CREATE TRIGGER workflow_action_model_call_immutable BEFORE UPDATE ON workflow_action_model_calls
    FOR EACH ROW EXECUTE FUNCTION workflow_action_intent_immutable();
