-- Additive tightening: nullable evidence has exactly one actual-truth meaning.
ALTER TABLE workflow_action_evidence_conflicts ADD CONSTRAINT workflow_action_conflict_nullable_evidence_shape
    CHECK(evidence_id IS NOT NULL OR (actual_observation_id IS NOT NULL AND reason='actual_receipt_disagreement'));
CREATE OR REPLACE FUNCTION workflow_action_receipt_finality_conflict() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE applied workflow_action_evidence%ROWTYPE;
BEGIN
    IF NEW.remote_entry_id IS NOT NULL THEN
        INSERT INTO workflow_action_actual_receipt_observations(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,result)
            VALUES(NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,NEW.remote_entry_id,NEW.result)
            ON CONFLICT(company_id,remote_entry_id,result_digest) DO NOTHING;
        -- A SQL transaction may have recorded other bounded incoming truth
        -- before its first canonical receipt. Compare it even on the actual branch.
        INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason)
            SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,
                NULL,observation.remote_entry_id,observation.id,'actual_receipt_disagreement'
            FROM workflow_action_actual_receipt_observations AS observation
            WHERE observation.company_id=NEW.company_id AND observation.invocation_id=NEW.invocation_id
                AND observation.result<>NEW.result AND NOT EXISTS(
                    SELECT 1 FROM workflow_action_evidence_conflicts AS conflict
                    WHERE conflict.company_id=NEW.company_id AND conflict.actual_observation_id=observation.id
                        AND conflict.reason='actual_receipt_disagreement');
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

