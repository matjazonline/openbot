-- Every evidence-bearing command owns its exact actor audit. Other event kinds
-- retain execution/kind uniqueness, and historical identities are never rewritten.
LOCK TABLE workflow_run_events, workflow_action_evidence_commands, workflow_action_evidence
    IN ACCESS EXCLUSIVE MODE;

-- Guard installation does not recheck old rows. Validate their immutable linkage
-- under the migration's table locks, without expiring legitimate historical proof.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM workflow_action_evidence_commands AS command
        WHERE command.evidence_id IS NOT NULL AND (
            NOT EXISTS (
                SELECT 1 FROM workflow_action_evidence AS evidence
                WHERE evidence.company_id=command.company_id AND evidence.id=command.evidence_id
                    AND evidence.command_key=command.command_key AND evidence.command_id=command.id
                    AND evidence.request_digest=command.request_digest AND evidence.actor_id=command.actor_id
                    AND evidence.run_id=command.run_id AND evidence.execution_id=command.execution_id
                    AND evidence.invocation_id=command.invocation_id AND evidence.argument_digest=command.argument_digest
                    AND evidence.dispatch_id=command.dispatch_id
            ) OR NOT EXISTS (
                SELECT 1 FROM workflow_run_events AS audit
                WHERE audit.company_id=command.company_id AND audit.run_id=command.run_id
                    AND audit.sequence=command.audit_sequence AND audit.execution_id=command.execution_id
                    AND audit.actor_id=command.actor_id AND audit.event_kind='action_reconciled'
            )
        )
    ) OR EXISTS (
        SELECT 1 FROM workflow_run_events AS audit
        WHERE audit.event_kind='action_reconciled' AND (
            audit.execution_id IS NULL OR (SELECT count(*) FROM workflow_action_evidence_commands AS command
                WHERE command.company_id=audit.company_id AND command.run_id=audit.run_id
                    AND command.audit_sequence=audit.sequence AND command.execution_id=audit.execution_id
                    AND command.actor_id=audit.actor_id AND command.evidence_id IS NOT NULL)<>1
        )
    ) OR EXISTS (
        SELECT 1 FROM workflow_action_evidence AS evidence
        WHERE NOT EXISTS (
            SELECT 1 FROM workflow_action_evidence_commands AS command
            WHERE command.company_id=evidence.company_id AND command.evidence_id=evidence.id
                AND command.command_key=evidence.command_key AND command.id=evidence.command_id
                AND command.request_digest=evidence.request_digest AND command.actor_id=evidence.actor_id
                AND command.run_id=evidence.run_id AND command.execution_id=evidence.execution_id
                AND command.invocation_id=evidence.invocation_id AND command.argument_digest=evidence.argument_digest
                AND command.dispatch_id=evidence.dispatch_id
        )
    ) THEN
        RAISE EXCEPTION 'invalid historical workflow reconciliation audit linkage' USING ERRCODE='23514';
    END IF;
END $$;

ALTER TABLE workflow_run_events DROP CONSTRAINT workflow_run_event_execution_kind;
CREATE UNIQUE INDEX workflow_run_event_execution_kind ON workflow_run_events
    (company_id,run_id,execution_id,event_kind) WHERE event_kind<>'action_reconciled';
ALTER TABLE workflow_run_events ADD CONSTRAINT workflow_action_audit_execution
    CHECK(event_kind<>'action_reconciled' OR execution_id IS NOT NULL);
ALTER TABLE workflow_action_evidence_commands ADD CONSTRAINT workflow_action_command_audit_unique
    UNIQUE(company_id,run_id,audit_sequence);

CREATE OR REPLACE FUNCTION workflow_action_command_scope_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.evidence_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
        WHERE evidence.company_id=NEW.company_id AND evidence.id=NEW.evidence_id
            AND evidence.command_key=NEW.command_key AND evidence.command_id=NEW.id
            AND evidence.request_digest=NEW.request_digest AND evidence.actor_id=NEW.actor_id
            AND evidence.run_id=NEW.run_id AND evidence.execution_id=NEW.execution_id
            AND evidence.invocation_id=NEW.invocation_id AND evidence.argument_digest=NEW.argument_digest
            AND evidence.dispatch_id=NEW.dispatch_id)
    THEN RAISE EXCEPTION 'workflow action command evidence scope mismatch' USING ERRCODE='23514'; END IF;
    IF NEW.evidence_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM workflow_run_events AS audit
        WHERE audit.company_id=NEW.company_id AND audit.run_id=NEW.run_id
            AND audit.sequence=NEW.audit_sequence AND audit.execution_id=NEW.execution_id
            AND audit.actor_id=NEW.actor_id AND audit.event_kind='action_reconciled'
    ) THEN
        RAISE EXCEPTION 'workflow action command audit scope mismatch' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END $$;

CREATE FUNCTION workflow_action_audit_command_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (SELECT count(*) FROM workflow_action_evidence_commands AS command
        WHERE command.company_id=NEW.company_id AND command.run_id=NEW.run_id
            AND command.audit_sequence=NEW.sequence AND command.execution_id=NEW.execution_id
            AND command.actor_id=NEW.actor_id AND command.evidence_id IS NOT NULL)<>1 THEN
        RAISE EXCEPTION 'workflow reconciliation audit requires one exact evidence command' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_action_audit_command_guard AFTER INSERT ON workflow_run_events
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (NEW.event_kind='action_reconciled')
    EXECUTE FUNCTION workflow_action_audit_command_guard();

CREATE FUNCTION workflow_action_audit_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.event_kind='action_reconciled' THEN
        RAISE EXCEPTION 'workflow reconciliation actor audit is append-only' USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    IF NEW.event_kind='action_reconciled' THEN
        RAISE EXCEPTION 'workflow event cannot become a reconciliation actor audit' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_audit_immutable BEFORE UPDATE OR DELETE ON workflow_run_events
    FOR EACH ROW EXECUTE FUNCTION workflow_action_audit_immutable();

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
                    AND receipt.result_revision=owner.revision AND audit.execution_id=receipt.execution_id
                    AND audit.actor_id=receipt.actor_id AND audit.event_kind='action_reconciled');
        IF NEW.retry_count<>OLD.retry_count OR NEW.max_retries<>OLD.max_retries
            OR NEW.workflow_execution_id<>OLD.workflow_execution_id OR NEW.id<>OLD.id
            OR owner.state<>'running' OR owner.terminal_execution_id IS NOT NULL
            OR NOT (ordinary OR reconciled)
        THEN RAISE EXCEPTION 'invalid explicit workflow retry' USING ERRCODE='23514'; END IF;
    END IF;
    RETURN NULL;
END $$;
