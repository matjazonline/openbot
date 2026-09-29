-- Coordination/configuration only. Jobs and attempts remain the execution ledger.
CREATE TABLE workflow_capacity_policy (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    global_limit integer NOT NULL CHECK (global_limit BETWEEN 2 AND 1024),
    company_limit integer NOT NULL CHECK (company_limit >= 1 AND company_limit < global_limit)
);

CREATE FUNCTION guard_workflow_capacity_policy() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'workflow capacity policy is immutable' USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER workflow_capacity_policy_immutable
BEFORE UPDATE OR DELETE ON workflow_capacity_policy
FOR EACH ROW EXECUTE FUNCTION guard_workflow_capacity_policy();
