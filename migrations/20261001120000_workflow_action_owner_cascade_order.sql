-- Follow-up to the applied owner-lifecycle migration. Coordinated fact cascades
-- avoid immediate NO ACTION checks observing other owner cascades still queued.
-- Live-owner DELETE guards installed by 110000 remain in force; no rows change.
-- Cascades must traverse the complete owned effect/evidence/episode graph.
-- NO ACTION checks run before separately queued owner cascades have necessarily
-- drained. Every deleting fact still checks that its owning run is already gone,
-- so this cannot make a live owner's job, command or audit a deletion authority.
DO $$
DECLARE fact_table text; owned_key record; definition text;
BEGIN
    FOREACH fact_table IN ARRAY ARRAY[
        'workflow_action_dispatches', 'workflow_action_receipts',
        'workflow_action_remote_entries', 'workflow_action_reconciliations',
        'workflow_action_evidence', 'workflow_action_evidence_coverage',
        'workflow_action_evidence_commands', 'workflow_action_evidence_consumptions',
        'workflow_action_evidence_conflicts', 'workflow_action_actual_receipt_observations',
        'workflow_action_schedule_witnesses', 'workflow_action_claim_episodes',
        'workflow_action_claim_retirement_witnesses', 'workflow_action_claim_budget_refusals'
    ] LOOP
        FOR owned_key IN SELECT conname, pg_get_constraintdef(oid) AS definition
            FROM pg_constraint WHERE conrelid=fact_table::regclass
                AND contype='f' AND confdeltype='a' ORDER BY conname
        LOOP
            -- Preserve every column, tenant scope and original deferral mode.
            definition:=replace(owned_key.definition, ' DEFERRABLE', ' ON DELETE CASCADE DEFERRABLE');
            IF definition=owned_key.definition THEN definition:=definition || ' ON DELETE CASCADE'; END IF;
            EXECUTE format('ALTER TABLE %I DROP CONSTRAINT %I, ADD CONSTRAINT %I %s',
                fact_table, owned_key.conname, owned_key.conname, definition);
        END LOOP;
    END LOOP;
END $$;

