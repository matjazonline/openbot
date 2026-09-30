-- Actual incoming truth must remain visible even when the first receipt wins.
-- These observations are bounded append-only audit, never another receipt owner.
CREATE TABLE workflow_action_actual_receipt_observations (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL, argument_digest text NOT NULL, dispatch_id uuid NOT NULL,
    remote_entry_id uuid NOT NULL, id uuid NOT NULL DEFAULT gen_random_uuid(),
    result jsonb NOT NULL CHECK(octet_length(result::text)<=131072),
    result_digest text GENERATED ALWAYS AS (encode(sha256(jsonb_send(result)),'hex')) STORED,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,remote_entry_id,result_digest),
    UNIQUE(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id)
        REFERENCES workflow_action_remote_entries(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id)
);
CREATE TRIGGER workflow_action_actual_observation_immutable BEFORE UPDATE OR DELETE
    ON workflow_action_actual_receipt_observations FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();
ALTER TABLE workflow_action_evidence_conflicts ADD COLUMN actual_observation_id uuid;
ALTER TABLE workflow_action_evidence_conflicts ALTER COLUMN evidence_id DROP NOT NULL;
ALTER TABLE workflow_action_evidence_conflicts ADD CONSTRAINT workflow_action_conflict_truth_anchor
    CHECK(evidence_id IS NOT NULL OR actual_observation_id IS NOT NULL);
ALTER TABLE workflow_action_evidence_conflicts ADD CONSTRAINT workflow_action_conflict_actual_observation
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,actual_observation_id)
    REFERENCES workflow_action_actual_receipt_observations(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,id);
ALTER TABLE workflow_action_evidence_conflicts ADD CONSTRAINT workflow_action_conflict_observation_shape
    CHECK(actual_observation_id IS NULL OR remote_entry_id IS NOT NULL);
ALTER TABLE workflow_action_evidence_conflicts DROP CONSTRAINT workflow_action_evidence_conflicts_reason_check;
ALTER TABLE workflow_action_evidence_conflicts ADD CONSTRAINT workflow_action_evidence_conflicts_reason_check
    CHECK(reason IN ('finality_breach','unattributed_applied','evidence_disagreement','actual_receipt_disagreement'));

CREATE FUNCTION workflow_action_actual_observation_conflicts() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,
            proof.id,NEW.remote_entry_id,NEW.id,'finality_breach'
        FROM workflow_action_evidence AS proof WHERE proof.company_id=NEW.company_id
            AND proof.invocation_id=NEW.invocation_id AND proof.disposition='final_not_applied'
            AND EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                WHERE coverage.company_id=NEW.company_id AND coverage.evidence_id=proof.id
                    AND coverage.remote_entry_id=NEW.remote_entry_id);
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,
            receipt.reconciliation_evidence_id,NEW.remote_entry_id,NEW.id,'actual_receipt_disagreement'
        FROM workflow_action_receipts AS receipt WHERE receipt.company_id=NEW.company_id
            AND receipt.invocation_id=NEW.invocation_id AND receipt.result<>NEW.result;
    RETURN NULL;
END $$;
CREATE TRIGGER workflow_action_actual_observation_conflicts AFTER INSERT
    ON workflow_action_actual_receipt_observations FOR EACH ROW EXECUTE FUNCTION workflow_action_actual_observation_conflicts();
CREATE FUNCTION workflow_action_actual_observation_receipt_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt
        WHERE receipt.company_id=NEW.company_id AND receipt.run_id=NEW.run_id
            AND receipt.execution_id=NEW.execution_id AND receipt.invocation_id=NEW.invocation_id
            AND receipt.argument_digest=NEW.argument_digest AND receipt.dispatch_id=NEW.dispatch_id)
    THEN RAISE EXCEPTION 'workflow action actual observation requires canonical receipt' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_action_actual_observation_receipt_guard AFTER INSERT
    ON workflow_action_actual_receipt_observations DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION workflow_action_actual_observation_receipt_guard();
CREATE OR REPLACE FUNCTION workflow_action_receipt_finality_conflict() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE applied workflow_action_evidence%ROWTYPE;
BEGIN
    IF NEW.remote_entry_id IS NOT NULL THEN
        INSERT INTO workflow_action_actual_receipt_observations(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,result)
            VALUES(NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,NEW.remote_entry_id,NEW.result)
            ON CONFLICT(company_id,remote_entry_id,result_digest) DO NOTHING;
        RETURN NULL;
    END IF;
    IF NEW.reconciliation_evidence_id IS NOT NULL THEN
        SELECT * INTO applied FROM workflow_action_evidence
            WHERE company_id=NEW.company_id AND id=NEW.reconciliation_evidence_id;
    END IF;
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,contradictory_evidence_id,remote_entry_id,reason)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,proof.id,
            NEW.reconciliation_evidence_id,NEW.remote_entry_id,
            CASE WHEN applied.applied_request='unattributed' OR (NEW.reconciliation_evidence_id IS NOT NULL AND applied.applied_request IS NULL)
                THEN 'unattributed_applied' ELSE 'finality_breach' END
        FROM workflow_action_evidence AS proof WHERE proof.company_id=NEW.company_id
            AND proof.invocation_id=NEW.invocation_id AND proof.disposition='final_not_applied'
            AND ((NEW.remote_entry_id IS NOT NULL AND EXISTS(
                SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=NEW.company_id
                    AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=NEW.remote_entry_id))
                OR (NEW.reconciliation_evidence_id IS NOT NULL AND (
                    applied.applied_request IS NULL OR applied.applied_request='unattributed'
                    OR (applied.applied_request='remote_entry' AND EXISTS(
                        SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=NEW.company_id
                            AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=applied.applied_remote_entry_id))
                    OR (applied.applied_request='marker_reservation' AND NOT EXISTS(
                        SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                            WHERE coverage.company_id=NEW.company_id AND coverage.evidence_id=proof.id)))));
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,
            NEW.reconciliation_evidence_id,observation.remote_entry_id,observation.id,'actual_receipt_disagreement'
        FROM workflow_action_actual_receipt_observations AS observation WHERE observation.company_id=NEW.company_id
            AND observation.invocation_id=NEW.invocation_id AND observation.result<>NEW.result;
    RETURN NULL;
END $$;

CREATE OR REPLACE VIEW workflow_action_effect_states AS
SELECT intent.company_id,intent.run_id,intent.execution_id,intent.id AS invocation_id,intent.argument_digest,
    CASE WHEN receipt.invocation_id IS NOT NULL THEN 'committed'
        WHEN EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence WHERE evidence.company_id=intent.company_id
            AND evidence.invocation_id=intent.id AND evidence.disposition='applied') THEN 'applied_without_result'
        WHEN workflow_action_has_evidence_conflict(intent.company_id,intent.execution_id) THEN 'needs_reconciliation'
        WHEN workflow_action_not_applied_available(intent.company_id,intent.id) IS NOT NULL THEN 'not_applied'
        WHEN reconciliation.invocation_id IS NOT NULL OR EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
            WHERE evidence.company_id=intent.company_id AND evidence.invocation_id=intent.id)
            THEN 'needs_reconciliation'
        WHEN marker.invocation_id IS NOT NULL THEN 'possible_dispatch'
        ELSE 'prepared' END AS effect_state,
    workflow_action_has_evidence_conflict(intent.company_id,intent.execution_id) AS evidence_conflict
FROM workflow_action_intents AS intent
LEFT JOIN workflow_action_dispatches AS marker ON marker.company_id=intent.company_id AND marker.invocation_id=intent.id
LEFT JOIN workflow_action_receipts AS receipt ON receipt.company_id=intent.company_id AND receipt.invocation_id=intent.id
LEFT JOIN workflow_action_reconciliations AS reconciliation ON reconciliation.company_id=intent.company_id AND reconciliation.invocation_id=intent.id;
