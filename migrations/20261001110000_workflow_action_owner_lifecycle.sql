-- Owned provenance disappears only with its owning run. Keep all fact-to-fact
-- NO ACTION keys: deleting an individual execution, job, command or audit must
-- not erase history while the run still exists. No historical row is rewritten.
ALTER TABLE workflow_action_state_witnesses
    DROP CONSTRAINT workflow_action_state_witnesses_company_id_run_id_fkey,
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY(company_id,run_id)
        REFERENCES workflow_runs(company_id,id) ON DELETE CASCADE;

-- Existing effects/evidence participate in the same deletion as their new claim
-- provenance. Direct run ownership avoids cascading through mutable job state or
-- through the circular command/evidence linkage.
DO $$
DECLARE fact_table text;
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
        EXECUTE format('ALTER TABLE %I ADD CONSTRAINT workflow_action_owner_lifecycle
            FOREIGN KEY(company_id,run_id) REFERENCES workflow_runs(company_id,id) ON DELETE CASCADE', fact_table);
    END LOOP;
END $$;

CREATE OR REPLACE FUNCTION workflow_action_fact_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' AND pg_trigger_depth()>1
        AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
    THEN RETURN OLD; END IF;
    RAISE EXCEPTION 'workflow action effect fact is append-only';
END $$;

CREATE OR REPLACE FUNCTION workflow_action_claim_retirement_witness_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        IF pg_trigger_depth()>1
            AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
        THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'workflow claim retirement witness is database owned' USING ERRCODE='23514';
    END IF;
    IF pg_trigger_depth()<2 OR NEW.transaction_id<>pg_current_xact_id()
    THEN RAISE EXCEPTION 'workflow claim retirement witness is database owned' USING ERRCODE='23514'; END IF;
    IF TG_OP='UPDATE' AND (
        (to_jsonb(OLD)-'retirement_confirmed'-'audit_sequence') IS DISTINCT FROM (to_jsonb(NEW)-'retirement_confirmed'-'audit_sequence')
        OR (OLD.retirement_confirmed AND NOT NEW.retirement_confirmed)
        OR (OLD.audit_sequence IS NOT NULL AND OLD.audit_sequence IS DISTINCT FROM NEW.audit_sequence))
    THEN RAISE EXCEPTION 'workflow claim retirement witness is append-once' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION workflow_action_audit_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' AND pg_trigger_depth()>1
        AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
    THEN RETURN OLD; END IF;
    IF OLD.event_kind='action_reconciled' THEN
        RAISE EXCEPTION 'workflow reconciliation actor audit is append-only' USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    IF NEW.event_kind='action_reconciled' THEN
        RAISE EXCEPTION 'workflow event cannot become a reconciliation actor audit' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION workflow_action_claim_refusal_audit_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' AND pg_trigger_depth()>1
        AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
    THEN RETURN OLD; END IF;
    IF EXISTS(SELECT 1 FROM workflow_action_claim_retirement_witnesses AS witness
        WHERE witness.company_id=OLD.company_id AND witness.run_id=OLD.run_id AND witness.audit_sequence=OLD.sequence)
        OR EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
            WHERE refusal.company_id=OLD.company_id AND refusal.run_id=OLD.run_id AND refusal.audit_sequence=OLD.sequence)
    THEN RAISE EXCEPTION 'workflow claim budget retirement audit is immutable' USING ERRCODE='23514'; END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
