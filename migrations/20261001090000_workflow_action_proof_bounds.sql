-- Proof authority bounds prior coverage, excluding only its matched consuming entry.
-- Keep ordinary dispatch/replay independent; all existing proof callers share this owner.
CREATE OR REPLACE FUNCTION workflow_action_not_applied_available(company uuid, invocation uuid, excluded_entry uuid DEFAULT NULL)
RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE proof workflow_action_evidence%ROWTYPE; marker workflow_action_dispatches%ROWTYPE;
BEGIN
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=company AND invocation_id=invocation;
    IF NOT FOUND OR marker.effect_kind<>'remote'
        OR workflow_action_has_evidence_conflict(company,marker.execution_id)
        OR EXISTS(SELECT 1 FROM workflow_action_receipts WHERE company_id=company AND invocation_id=invocation)
        OR EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=company AND invocation_id=invocation AND disposition='applied')
        OR (SELECT count(*) FROM (SELECT id FROM workflow_action_dispatches
            WHERE company_id=company AND execution_id=marker.execution_id LIMIT 129) AS bounded)>128
        OR (SELECT count(*) FROM (SELECT id FROM workflow_action_remote_entries
            WHERE company_id=company AND invocation_id=invocation
                AND id IS DISTINCT FROM excluded_entry LIMIT 129) AS bounded)>128
    THEN RETURN NULL; END IF;
    FOR proof IN SELECT * FROM workflow_action_evidence
        WHERE company_id=company AND invocation_id=invocation AND grant_eligible
        ORDER BY created_at,id
    LOOP
        IF proof.valid_until<=clock_timestamp() THEN CONTINUE; END IF;
        IF excluded_entry IS NULL THEN
            IF EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions
                WHERE company_id=company AND evidence_id=proof.id) THEN CONTINUE; END IF;
        ELSE
            IF NOT EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions AS consumption
                JOIN workflow_action_remote_entries AS consuming
                    ON (consuming.company_id,consuming.run_id,consuming.execution_id,consuming.invocation_id,
                        consuming.argument_digest,consuming.dispatch_id,consuming.id)
                    = (consumption.company_id,consumption.run_id,consumption.execution_id,consumption.invocation_id,
                        consumption.argument_digest,consumption.dispatch_id,consumption.remote_entry_id)
                WHERE (consumption.company_id,consumption.run_id,consumption.execution_id,consumption.invocation_id,
                        consumption.argument_digest,consumption.dispatch_id,consumption.evidence_id)
                    = (proof.company_id,proof.run_id,proof.execution_id,proof.invocation_id,
                        proof.argument_digest,proof.dispatch_id,proof.id)
                    AND consuming.id=excluded_entry)
            THEN CONTINUE; END IF;
        END IF;
        IF EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            JOIN task_attempts AS attempt ON attempt.task_id=entry.job_id AND attempt.attempt_number=entry.attempt_number
            WHERE entry.company_id=company AND entry.invocation_id=invocation
                AND entry.id IS DISTINCT FROM excluded_entry AND attempt.status='processing')
            OR EXISTS(SELECT 1 FROM task_attempts AS attempt WHERE attempt.task_id=marker.job_id
                AND attempt.attempt_number=marker.attempt_number AND attempt.status='processing'
                AND (excluded_entry IS NULL OR NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS consuming
                    WHERE consuming.company_id=company AND consuming.id=excluded_entry
                    AND consuming.job_id=attempt.task_id AND consuming.attempt_number=attempt.attempt_number)))
        THEN CONTINUE; END IF;
        IF EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=company AND entry.invocation_id=invocation
                AND entry.id IS DISTINCT FROM excluded_entry AND NOT EXISTS(
                    SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                    WHERE coverage.company_id=company AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=entry.id))
            OR EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                WHERE coverage.company_id=company AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=excluded_entry)
        THEN CONTINUE; END IF;
        RETURN proof.id;
    END LOOP;
    RETURN NULL;
END $$;
