-- Additive correction: applied evidence names the request it proves; historical
-- entries remain unattributed and cannot acquire a synthetic transaction identity.
ALTER TABLE workflow_action_evidence ADD COLUMN applied_request text
    CHECK(applied_request IN ('remote_entry','marker_reservation','unattributed'));
ALTER TABLE workflow_action_evidence ADD COLUMN applied_remote_entry_id uuid;
ALTER TABLE workflow_action_evidence ADD CONSTRAINT workflow_action_applied_request_shape CHECK (
    (applied_request IS NULL AND applied_remote_entry_id IS NULL)
    OR (disposition='applied' AND ((applied_request='remote_entry' AND applied_remote_entry_id IS NOT NULL)
        OR (applied_request IN ('marker_reservation','unattributed') AND applied_remote_entry_id IS NULL))));
ALTER TABLE workflow_action_evidence ADD CONSTRAINT workflow_action_applied_request_scope
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,applied_remote_entry_id)
    REFERENCES workflow_action_remote_entries(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id);
ALTER TABLE workflow_action_evidence_conflicts ADD COLUMN reason text NOT NULL DEFAULT 'finality_breach'
    CHECK(reason IN ('finality_breach','unattributed_applied','evidence_disagreement'));
CREATE FUNCTION workflow_action_applied_request_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.disposition='applied' AND NEW.applied_request IS NULL
    THEN RAISE EXCEPTION 'applied evidence requires request attribution' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_applied_request_guard BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION workflow_action_applied_request_guard();

-- No existing remote entry is assigned a reservation identity. The default applies
-- only to future inserts and the trigger rejects explicitly spoofed identities.
ALTER TABLE workflow_action_remote_entries ADD COLUMN reservation_xid xid8;
ALTER TABLE workflow_action_remote_entries ALTER COLUMN reservation_xid SET DEFAULT pg_current_xact_id();
ALTER TABLE workflow_action_evidence_consumptions ADD COLUMN reservation_xid xid8;
ALTER TABLE workflow_action_evidence_consumptions ALTER COLUMN reservation_xid SET DEFAULT pg_current_xact_id();
CREATE FUNCTION workflow_action_reservation_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.reservation_xid IS NOT NULL AND NEW.reservation_xid<>pg_current_xact_id()
    THEN RAISE EXCEPTION 'invalid workflow action reservation transaction' USING ERRCODE='23514'; END IF;
    NEW.reservation_xid:=pg_current_xact_id();
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_entry_reservation_identity BEFORE INSERT ON workflow_action_remote_entries
    FOR EACH ROW EXECUTE FUNCTION workflow_action_reservation_identity();
CREATE TRIGGER workflow_action_consumption_reservation_identity BEFORE INSERT ON workflow_action_evidence_consumptions
    FOR EACH ROW EXECUTE FUNCTION workflow_action_reservation_identity();
CREATE OR REPLACE FUNCTION workflow_action_consumption_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.reservation_xid IS DISTINCT FROM pg_current_xact_id()
        OR NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=NEW.company_id AND entry.id=NEW.remote_entry_id
                AND entry.reservation_xid=NEW.reservation_xid)
        OR workflow_action_not_applied_available(NEW.company_id,NEW.invocation_id,NEW.remote_entry_id)
            IS DISTINCT FROM NEW.evidence_id
    THEN RAISE EXCEPTION 'workflow action final proof cannot be consumed' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;

-- Capture the first real OLD state/reason before any change in this transaction.
-- Caller command labels cannot turn terminal or human-wait work into reconciliation.
CREATE TABLE workflow_action_state_witnesses (
    company_id uuid NOT NULL, run_id uuid NOT NULL, transaction_id xid8 NOT NULL,
    initial_state text NOT NULL, initial_waiting_reason text,
    PRIMARY KEY(company_id,run_id,transaction_id),
    FOREIGN KEY(company_id,run_id) REFERENCES workflow_runs(company_id,id)
);
CREATE FUNCTION workflow_action_state_witness_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF pg_trigger_depth()<2 OR NEW.transaction_id<>pg_current_xact_id()
    THEN RAISE EXCEPTION 'workflow action state witness is database owned' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_state_witness_owner BEFORE INSERT ON workflow_action_state_witnesses
    FOR EACH ROW EXECUTE FUNCTION workflow_action_state_witness_owner();
CREATE TRIGGER workflow_action_state_witness_immutable BEFORE UPDATE OR DELETE ON workflow_action_state_witnesses
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();
CREATE FUNCTION workflow_action_capture_state_witness() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state IS DISTINCT FROM NEW.state OR OLD.waiting_reason IS DISTINCT FROM NEW.waiting_reason THEN
        INSERT INTO workflow_action_state_witnesses(company_id,run_id,transaction_id,initial_state,initial_waiting_reason)
            VALUES(OLD.company_id,OLD.id,pg_current_xact_id(),OLD.state,OLD.waiting_reason)
            ON CONFLICT DO NOTHING;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_capture_state_witness BEFORE UPDATE ON workflow_runs
    FOR EACH ROW EXECUTE FUNCTION workflow_action_capture_state_witness();

CREATE OR REPLACE FUNCTION workflow_action_receipt_finality_conflict() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE applied workflow_action_evidence%ROWTYPE;
BEGIN
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
    RETURN NULL;
END $$;
CREATE OR REPLACE FUNCTION workflow_action_conflict_park_pending() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF NOT EXISTS(SELECT 1 FROM workflow_executions AS execution
        WHERE execution.company_id=NEW.company_id AND execution.run_id=NEW.run_id AND execution.id=NEW.execution_id
            AND execution.completed_at IS NULL AND execution.committed_output IS NULL
            AND execution.committed_route IS NULL AND execution.successor_execution_id IS NULL)
    THEN RETURN NULL; END IF;
    UPDATE background_tasks SET status='failed',updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND workflow_execution_id=NEW.execution_id
            AND queue_kind='workflow' AND status='pending';
    UPDATE workflow_runs SET state='waiting',waiting_reason='reconciliation'
        WHERE company_id=NEW.company_id AND id=NEW.run_id AND state IN ('queued','running')
            AND NOT EXISTS(SELECT 1 FROM background_tasks WHERE company_id=NEW.company_id
                AND workflow_execution_id=NEW.execution_id AND status='processing');
    RETURN NULL;
END $$;
CREATE OR REPLACE FUNCTION workflow_action_evidence_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE marker workflow_action_dispatches%ROWTYPE;
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id;
    IF marker.effect_kind<>'remote' OR NEW.observed_at<marker.created_at
        OR NEW.verified_at>clock_timestamp()
        OR (NEW.registration IS NOT NULL AND NEW.valid_until<=clock_timestamp())
        OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
            WHERE command.company_id=NEW.company_id AND command.command_key=NEW.command_key
            AND command.evidence_id=NEW.id AND command.actor_id=NEW.actor_id
            AND command.id=NEW.command_id AND command.request_digest=NEW.request_digest
            AND command.run_id=NEW.run_id AND command.execution_id=NEW.execution_id
            AND command.invocation_id=NEW.invocation_id AND command.argument_digest=NEW.argument_digest
            AND command.dispatch_id=NEW.dispatch_id)
        OR (NEW.applied_request='remote_entry' AND NOT EXISTS(
            SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=NEW.company_id
                AND coverage.evidence_id=NEW.id AND coverage.remote_entry_id=NEW.applied_remote_entry_id))
        OR (NEW.applied_request='marker_reservation' AND EXISTS(
            SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=NEW.company_id AND coverage.evidence_id=NEW.id))
        OR (SELECT count(*) FROM workflow_action_evidence_coverage WHERE company_id=NEW.company_id AND evidence_id=NEW.id)>128
        OR EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=NEW.company_id AND entry.invocation_id=NEW.invocation_id
                AND entry.created_at<=NEW.created_at AND (entry.created_at>NEW.observed_at
                OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                    WHERE coverage.company_id=NEW.company_id AND coverage.evidence_id=NEW.id AND coverage.remote_entry_id=entry.id)))
    THEN RAISE EXCEPTION 'invalid workflow action evidence provenance' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;

CREATE FUNCTION workflow_action_command_scope_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.evidence_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
        WHERE evidence.company_id=NEW.company_id AND evidence.id=NEW.evidence_id
            AND evidence.command_key=NEW.command_key AND evidence.command_id=NEW.id
            AND evidence.request_digest=NEW.request_digest AND evidence.actor_id=NEW.actor_id
            AND evidence.run_id=NEW.run_id AND evidence.execution_id=NEW.execution_id
            AND evidence.invocation_id=NEW.invocation_id AND evidence.argument_digest=NEW.argument_digest
            AND evidence.dispatch_id=NEW.dispatch_id)
    THEN RAISE EXCEPTION 'workflow action command evidence scope mismatch' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_action_command_scope_guard AFTER INSERT ON workflow_action_evidence_commands
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_action_command_scope_guard();
CREATE OR REPLACE FUNCTION check_workflow_control_retry() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE owner workflow_runs%ROWTYPE; ordinary boolean; reconciled boolean;
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='failed' AND NEW.status='pending' THEN
        SELECT run.* INTO owner FROM workflow_runs AS run JOIN workflow_executions AS execution
            ON execution.company_id=run.company_id AND execution.run_id=run.id
            WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id;
        ordinary:=workflow_control_retry_safe(NEW.company_id,owner.id,NEW.id)
            AND EXISTS(SELECT 1 FROM workflow_control_commands AS receipt WHERE receipt.company_id=owner.company_id
                AND receipt.run_id=owner.id AND receipt.operation='retry' AND receipt.result='applied' AND receipt.result_revision=owner.revision);
        reconciled:=workflow_action_reconciliation_reopen_safe(NEW.company_id,owner.id,NEW.id)
            AND EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS receipt
                JOIN workflow_action_evidence AS evidence ON evidence.company_id=receipt.company_id AND evidence.id=receipt.evidence_id
                JOIN workflow_run_events AS audit ON audit.company_id=receipt.company_id AND audit.run_id=receipt.run_id AND audit.sequence=receipt.audit_sequence
                WHERE receipt.company_id=owner.company_id AND receipt.run_id=owner.id AND receipt.execution_id=NEW.workflow_execution_id
                    AND receipt.scheduled_job_id=NEW.id AND receipt.outcome->>'kind'='scheduled'
                    AND receipt.previous_state='waiting' AND receipt.previous_waiting_reason='reconciliation'
                    AND evidence.run_id=receipt.run_id AND evidence.execution_id=receipt.execution_id
                    AND evidence.invocation_id=receipt.invocation_id AND evidence.argument_digest=receipt.argument_digest
                    AND evidence.dispatch_id=receipt.dispatch_id
                    AND EXISTS(SELECT 1 FROM workflow_action_state_witnesses AS witness
                        WHERE witness.company_id=owner.company_id AND witness.run_id=owner.id
                            AND witness.transaction_id=pg_current_xact_id()
                            AND witness.initial_state='waiting' AND witness.initial_waiting_reason='reconciliation')
                    AND receipt.result_revision=owner.revision AND audit.actor_id=receipt.actor_id AND audit.event_kind='action_reconciled');
        IF NEW.retry_count<>OLD.retry_count OR NEW.max_retries<>OLD.max_retries
            OR NEW.workflow_execution_id<>OLD.workflow_execution_id OR NEW.id<>OLD.id
            OR owner.state<>'running' OR owner.terminal_execution_id IS NOT NULL
            OR NOT (ordinary OR reconciled)
        THEN RAISE EXCEPTION 'invalid explicit workflow retry' USING ERRCODE='23514'; END IF;
    END IF;
    RETURN NULL;
END $$;
