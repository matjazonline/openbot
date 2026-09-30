-- Preserve every valid FailureCode, including single-part and mixed-case names.
ALTER TABLE workflow_action_reconciliations
    DROP CONSTRAINT workflow_action_reconciliations_failure_code_check,
    ADD CONSTRAINT workflow_action_reconciliations_failure_code_check
        CHECK (octet_length(failure_code) BETWEEN 1 AND 128
            AND failure_code ~ '^[A-Za-z_][A-Za-z0-9_-]*(\.[A-Za-z_][A-Za-z0-9_-]*)*$');
