-- Reconciliation records truth. Only a host-verified, exact, one-use final absence
-- may enable another remote entry; provider signatures remain application-owned.
ALTER TABLE workflow_action_dispatches ADD CONSTRAINT workflow_action_marker_evidence_key
    UNIQUE(company_id,run_id,execution_id,invocation_id,argument_digest,id);
CREATE TABLE workflow_action_evidence (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL, argument_digest text NOT NULL, dispatch_id uuid NOT NULL,
    id uuid NOT NULL, actor_id uuid NOT NULL, command_id uuid NOT NULL,
    command_key text NOT NULL CHECK(octet_length(command_key) BETWEEN 1 AND 128),
    request_digest text NOT NULL CHECK(request_digest ~ '^[0-9a-f]{64}$'),
    coverage_digest text NOT NULL CHECK(coverage_digest ~ '^[0-9a-f]{64}$'),
    disposition text NOT NULL CHECK(disposition IN ('applied','final_not_applied','unknown')),
    registration text CHECK(registration ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'),
    verifier_version text CHECK(verifier_version ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'),
    provider text CHECK(provider ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'),
    authoritative_reference text CHECK(authoritative_reference ~ '^[A-Za-z0-9][A-Za-z0-9_./:-]{0,127}$'),
    operation_signature text CHECK(operation_signature ~ '^[0-9a-f]{64}$'),
    observed_at timestamptz NOT NULL, verified_at timestamptz NOT NULL,
    valid_until timestamptz NOT NULL, created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    grant_eligible boolean NOT NULL DEFAULT false,
    diagnostic jsonb NOT NULL CHECK(jsonb_typeof(diagnostic)='object' AND octet_length(diagnostic::text)<=16384),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id),
    UNIQUE(company_id,command_key,command_id,request_digest,id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id)
        REFERENCES workflow_action_dispatches(company_id,run_id,execution_id,invocation_id,argument_digest,id),
    CHECK(observed_at<=verified_at AND verified_at<=created_at),
    CHECK((registration IS NULL AND verifier_version IS NULL AND provider IS NULL
        AND authoritative_reference IS NULL AND operation_signature IS NULL
        AND disposition='unknown' AND valid_until=verified_at AND NOT grant_eligible)
        OR (registration IS NOT NULL AND verifier_version IS NOT NULL AND provider IS NOT NULL
        AND authoritative_reference IS NOT NULL AND operation_signature IS NOT NULL
        AND valid_until>verified_at AND valid_until<=verified_at+interval '24 hours')),
    CHECK(NOT grant_eligible OR disposition='final_not_applied')
);
CREATE UNIQUE INDEX workflow_action_one_final_coverage ON workflow_action_evidence
    (company_id,invocation_id,coverage_digest) WHERE grant_eligible;
CREATE TABLE workflow_action_evidence_coverage (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL, argument_digest text NOT NULL, dispatch_id uuid NOT NULL,
    evidence_id uuid NOT NULL, remote_entry_id uuid NOT NULL,
    PRIMARY KEY(company_id,evidence_id,remote_entry_id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id)
        REFERENCES workflow_action_evidence(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id)
        REFERENCES workflow_action_remote_entries(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id)
);
CREATE TABLE workflow_action_evidence_commands (
    company_id uuid NOT NULL, command_key text NOT NULL CHECK(octet_length(command_key) BETWEEN 1 AND 128),
    id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL, invocation_id uuid NOT NULL,
    argument_digest text NOT NULL, dispatch_id uuid NOT NULL, actor_id uuid NOT NULL,
    request_digest text NOT NULL CHECK(request_digest ~ '^[0-9a-f]{64}$'),
    expected_revision bigint NOT NULL CHECK(expected_revision>0),
    result_revision bigint NOT NULL CHECK(result_revision>0),
    outcome jsonb NOT NULL CHECK(jsonb_typeof(outcome)='object' AND octet_length(outcome::text)<=1024),
    evidence_id uuid, audit_sequence bigint, scheduled_job_id uuid,
    previous_state text CHECK(previous_state IN ('queued','running','waiting','succeeded','failed','cancelled')),
    previous_waiting_reason text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,command_key),
    UNIQUE(company_id,command_key,id,request_digest),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id)
        REFERENCES workflow_action_dispatches(company_id,run_id,execution_id,invocation_id,argument_digest,id),
    FOREIGN KEY(company_id,command_key,id,request_digest,evidence_id)
        REFERENCES workflow_action_evidence(company_id,command_key,command_id,request_digest,id)
        DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(company_id,run_id,audit_sequence) REFERENCES workflow_run_events(company_id,run_id,sequence),
    FOREIGN KEY(company_id,execution_id,scheduled_job_id) REFERENCES background_tasks(company_id,workflow_execution_id,id),
    CHECK((evidence_id IS NULL AND audit_sequence IS NULL AND scheduled_job_id IS NULL)
        OR (evidence_id IS NOT NULL AND audit_sequence IS NOT NULL)),
    CHECK(scheduled_job_id IS NULL OR (previous_state='waiting' AND previous_waiting_reason='reconciliation'
        AND outcome->>'kind'='scheduled'))
);
ALTER TABLE workflow_action_evidence ADD CONSTRAINT workflow_action_evidence_command_link
    FOREIGN KEY(company_id,command_key,command_id,request_digest)
    REFERENCES workflow_action_evidence_commands(company_id,command_key,id,request_digest)
    DEFERRABLE INITIALLY DEFERRED;
CREATE TABLE workflow_action_evidence_consumptions (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL, argument_digest text NOT NULL, dispatch_id uuid NOT NULL,
    evidence_id uuid NOT NULL, remote_entry_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,evidence_id), UNIQUE(company_id,remote_entry_id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id)
        REFERENCES workflow_action_evidence(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id)
        REFERENCES workflow_action_remote_entries(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id)
);
CREATE TABLE workflow_action_evidence_conflicts (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL, argument_digest text NOT NULL, dispatch_id uuid NOT NULL,
    id uuid NOT NULL DEFAULT gen_random_uuid(), evidence_id uuid NOT NULL,
    contradictory_evidence_id uuid, remote_entry_id uuid,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id)
        REFERENCES workflow_action_evidence(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,contradictory_evidence_id)
        REFERENCES workflow_action_evidence(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id),
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id)
        REFERENCES workflow_action_remote_entries(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id),
    CHECK(contradictory_evidence_id IS NOT NULL OR remote_entry_id IS NOT NULL)
);
ALTER TABLE workflow_action_receipts ADD COLUMN reconciliation_evidence_id uuid;
ALTER TABLE workflow_action_receipts DROP CONSTRAINT workflow_action_receipt_kind;
ALTER TABLE workflow_action_receipts ADD CONSTRAINT workflow_action_receipt_kind CHECK (
    (effect_kind='local' AND remote_entry_id IS NULL AND reconciliation_evidence_id IS NULL)
    OR (effect_kind='remote' AND ((remote_entry_id IS NOT NULL)::integer
        +(reconciliation_evidence_id IS NOT NULL)::integer)=1));
ALTER TABLE workflow_action_receipts ADD CONSTRAINT workflow_action_receipt_evidence_source
    FOREIGN KEY(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,reconciliation_evidence_id)
    REFERENCES workflow_action_evidence(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id);

CREATE TRIGGER workflow_action_evidence_immutable BEFORE UPDATE OR DELETE ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();
CREATE TRIGGER workflow_action_evidence_coverage_immutable BEFORE UPDATE OR DELETE ON workflow_action_evidence_coverage
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();
CREATE TRIGGER workflow_action_evidence_command_immutable BEFORE UPDATE OR DELETE ON workflow_action_evidence_commands
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();
CREATE TRIGGER workflow_action_evidence_consumption_immutable BEFORE UPDATE OR DELETE ON workflow_action_evidence_consumptions
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();
CREATE TRIGGER workflow_action_evidence_conflict_immutable BEFORE UPDATE OR DELETE ON workflow_action_evidence_conflicts
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();
CREATE TRIGGER workflow_action_evidence_revision AFTER INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION advance_workflow_owned_revision();
CREATE TRIGGER workflow_action_evidence_conflict_revision AFTER INSERT ON workflow_action_evidence_conflicts
    FOR EACH ROW EXECUTE FUNCTION advance_workflow_owned_revision();

CREATE FUNCTION workflow_action_has_evidence_conflict(company uuid, execution uuid)
RETURNS boolean LANGUAGE sql AS $$
    SELECT EXISTS(SELECT 1 FROM workflow_action_evidence_conflicts AS conflict
        WHERE conflict.company_id=company AND conflict.execution_id=execution)
$$;

-- excluded_entry is permitted ONLY for its exact committed consumption. The new
-- processing attempt does not disqualify itself; every earlier processing one does.
CREATE FUNCTION workflow_action_not_applied_available(company uuid, invocation uuid, excluded_entry uuid DEFAULT NULL)
RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE proof workflow_action_evidence%ROWTYPE; marker workflow_action_dispatches%ROWTYPE;
BEGIN
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=company AND invocation_id=invocation;
    IF NOT FOUND OR marker.effect_kind<>'remote'
        OR workflow_action_has_evidence_conflict(company,marker.execution_id)
        OR EXISTS(SELECT 1 FROM workflow_action_receipts WHERE company_id=company AND invocation_id=invocation)
        OR EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=company AND invocation_id=invocation AND disposition='applied')
        OR (SELECT count(*) FROM (SELECT id FROM workflow_action_remote_entries
            WHERE company_id=company AND invocation_id=invocation LIMIT 129) AS bounded)>128
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
            IF NOT EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions
                WHERE company_id=company AND evidence_id=proof.id AND remote_entry_id=excluded_entry)
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

CREATE OR REPLACE FUNCTION workflow_action_retry_safe(company uuid, execution uuid)
RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE fact record; scanned integer:=0; safe boolean:=true;
BEGIN
    IF workflow_action_has_evidence_conflict(company,execution) THEN RETURN false; END IF;
    FOR fact IN SELECT invocation_id FROM workflow_action_dispatches
        WHERE company_id=company AND execution_id=execution ORDER BY invocation_id LIMIT 129
    LOOP
        scanned:=scanned+1;
        IF scanned>128 THEN RETURN false; END IF;
        IF EXISTS(SELECT 1 FROM workflow_action_receipts WHERE company_id=company AND invocation_id=fact.invocation_id)
        THEN CONTINUE; END IF;
        IF EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=company
            AND invocation_id=fact.invocation_id AND disposition='applied')
            OR NOT (COALESCE(workflow_action_replay_supported(company,fact.invocation_id),false)
                OR workflow_action_not_applied_available(company,fact.invocation_id) IS NOT NULL)
        THEN safe:=false; END IF;
    END LOOP;
    IF scanned=0 THEN RETURN NULL; END IF;
    RETURN safe;
END $$;

CREATE FUNCTION workflow_action_evidence_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE marker workflow_action_dispatches%ROWTYPE;
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id;
    IF marker.effect_kind<>'remote' OR NEW.observed_at<marker.created_at
        OR NEW.verified_at>clock_timestamp()
        OR (NEW.registration IS NOT NULL AND NEW.valid_until<=clock_timestamp())
        OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
            WHERE command.company_id=NEW.company_id AND command.command_key=NEW.command_key
            AND command.evidence_id=NEW.id AND command.actor_id=NEW.actor_id)
        OR (SELECT count(*) FROM workflow_action_evidence_coverage WHERE company_id=NEW.company_id AND evidence_id=NEW.id)>128
        OR EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=NEW.company_id AND entry.invocation_id=NEW.invocation_id
                AND entry.created_at<=NEW.created_at AND (entry.created_at>NEW.observed_at
                OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                    WHERE coverage.company_id=NEW.company_id AND coverage.evidence_id=NEW.id AND coverage.remote_entry_id=entry.id)))
    THEN RAISE EXCEPTION 'invalid workflow action evidence provenance' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_action_evidence_commit_guard AFTER INSERT ON workflow_action_evidence
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_action_evidence_commit_guard();
CREATE FUNCTION workflow_action_receipt_evidence_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.reconciliation_evidence_id IS NOT NULL AND NOT EXISTS(
        SELECT 1 FROM workflow_action_evidence WHERE company_id=NEW.company_id
            AND id=NEW.reconciliation_evidence_id AND disposition='applied' AND registration IS NOT NULL)
    THEN RAISE EXCEPTION 'workflow action receipt requires verified applied evidence' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_receipt_evidence_guard BEFORE INSERT ON workflow_action_receipts
    FOR EACH ROW EXECUTE FUNCTION workflow_action_receipt_evidence_guard();

-- Late bounded truth is retained; a finality breach is immutable and permanently
-- vetoes automatic continuation even if a usable receipt now exists.
CREATE FUNCTION workflow_action_receipt_finality_conflict() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,contradictory_evidence_id,remote_entry_id)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,proof.id,
            NEW.reconciliation_evidence_id,NEW.remote_entry_id
        FROM workflow_action_evidence AS proof WHERE proof.company_id=NEW.company_id
            AND proof.invocation_id=NEW.invocation_id AND proof.disposition='final_not_applied'
            AND (NEW.reconciliation_evidence_id IS NOT NULL OR EXISTS(
                SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=NEW.company_id
                    AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=NEW.remote_entry_id));
    RETURN NULL;
END $$;
CREATE TRIGGER workflow_action_receipt_finality_conflict AFTER INSERT ON workflow_action_receipts
    FOR EACH ROW EXECUTE FUNCTION workflow_action_receipt_finality_conflict();
CREATE FUNCTION workflow_action_conflict_park_pending() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    UPDATE background_tasks SET status='failed',updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND workflow_execution_id=NEW.execution_id
            AND queue_kind='workflow' AND status='pending';
    UPDATE workflow_runs SET state='waiting',waiting_reason='reconciliation'
        WHERE company_id=NEW.company_id AND id=NEW.run_id AND state IN ('queued','running')
            AND NOT EXISTS(SELECT 1 FROM background_tasks WHERE company_id=NEW.company_id
                AND workflow_execution_id=NEW.execution_id AND status='processing');
    RETURN NULL;
END $$;
CREATE TRIGGER workflow_action_conflict_park_pending AFTER INSERT ON workflow_action_evidence_conflicts
    FOR EACH ROW EXECUTE FUNCTION workflow_action_conflict_park_pending();

CREATE OR REPLACE FUNCTION workflow_action_remote_entry_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE marker workflow_action_dispatches%ROWTYPE; proof uuid;
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    PERFORM id FROM workflow_executions WHERE company_id=NEW.company_id AND id=NEW.execution_id FOR UPDATE;
    PERFORM id FROM background_tasks WHERE company_id=NEW.company_id AND id=NEW.job_id FOR UPDATE;
    PERFORM id FROM task_attempts WHERE task_id=NEW.job_id AND attempt_number=NEW.attempt_number FOR UPDATE;
    IF NOT EXISTS(SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=job.id
        WHERE owner.company_id=NEW.company_id AND owner.id=NEW.run_id
            AND owner.state IN ('queued','running') AND owner.deadline>clock_timestamp()
            AND execution.id=NEW.execution_id AND execution.completed_at IS NULL
            AND job.id=NEW.job_id AND job.queue_kind='workflow' AND job.status='processing'
            AND job.worker_id=NEW.worker_id AND job.execution_generation=NEW.execution_generation
            AND job.retry_count+1=NEW.attempt_number AND job.lock_expires_at>clock_timestamp()
            AND attempt.attempt_number=NEW.attempt_number AND attempt.worker_id=NEW.worker_id
            AND attempt.execution_generation=NEW.execution_generation AND attempt.status='processing')
    THEN RAISE EXCEPTION 'workflow action remote entry lost live fence'; END IF;
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id;
    proof:=workflow_action_not_applied_available(NEW.company_id,NEW.invocation_id,NEW.id);
    IF workflow_action_has_evidence_conflict(NEW.company_id,NEW.execution_id)
        OR EXISTS(SELECT 1 FROM workflow_action_receipts WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id)
        OR EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id AND disposition='applied')
        OR EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            JOIN task_attempts AS attempt ON attempt.task_id=entry.job_id AND attempt.attempt_number=entry.attempt_number
            WHERE entry.company_id=NEW.company_id AND entry.invocation_id=NEW.invocation_id
                AND entry.id<>NEW.id AND attempt.status='processing')
        OR (marker.attempt_number<>NEW.attempt_number AND (marker.attempt_number>=NEW.attempt_number
            OR (proof IS NULL AND NOT COALESCE(workflow_action_replay_supported(NEW.company_id,NEW.invocation_id),false))))
        OR (EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions WHERE company_id=NEW.company_id AND remote_entry_id=NEW.id) AND proof IS NULL)
    THEN RAISE EXCEPTION 'workflow action remote entry lacks supported replay'; END IF;
    RETURN NEW;
END $$;

CREATE FUNCTION workflow_action_consumption_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF workflow_action_not_applied_available(NEW.company_id,NEW.invocation_id,NEW.remote_entry_id)
        IS DISTINCT FROM NEW.evidence_id
    THEN RAISE EXCEPTION 'workflow action final proof cannot be consumed' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_action_consumption_commit_guard AFTER INSERT ON workflow_action_evidence_consumptions
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_action_consumption_commit_guard();

-- Reconciliation preserves ordinary retry gates except its independent historical
-- safety proof; it neither rewrites old attempts nor replenishes shared budgets.
CREATE FUNCTION workflow_action_reconciliation_reopen_safe(company uuid, run uuid, job uuid)
RETURNS boolean LANGUAGE sql AS $$
    SELECT EXISTS(SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
        JOIN workflow_run_budgets AS link ON link.company_id=owner.company_id AND link.run_id=owner.id
        JOIN workflow_root_budgets AS budget ON budget.company_id=link.company_id AND budget.root_run_id=link.root_run_id
        JOIN workflow_root_budget_usage AS usage ON usage.company_id=budget.company_id AND usage.root_run_id=budget.root_run_id
        WHERE owner.company_id=company AND owner.id=run AND task.id=job AND task.queue_kind='workflow'
            AND owner.deadline>clock_timestamp() AND execution.activation<=owner.max_steps
            AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
            AND execution.committed_output IS NULL AND execution.committed_route IS NULL AND execution.successor_execution_id IS NULL
            AND task.retry_count>0 AND task.retry_count<task.max_retries
            AND task.worker_id IS NULL AND task.execution_generation IS NULL AND task.lock_expires_at IS NULL
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND attempt.workflow_retirement IN ('live','expired')
            AND attempt.workflow_failure_code NOT IN ('workflow.activation_limit','workflow.invalid_result','workflow.run_deadline','workflow.root_budget_exhausted')
            AND workflow_action_retry_safe(company,execution.id) IS TRUE
            AND NOT EXISTS(SELECT 1 FROM workflow_budget_receipts WHERE company_id=company AND run_id=run AND disposition='exhausted')
            AND budget.provenance='frozen_v2' AND usage.activations<=budget.activations
            AND usage.model_calls<budget.model_calls AND usage.repetitions<=budget.repetitions
            AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
            AND NOT EXISTS(SELECT 1 FROM workflow_executions AS sibling
                JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                WHERE sibling.company_id=company AND sibling.run_id=run AND other.queue_kind='workflow'
                    AND other.id<>job AND other.status IN ('pending','processing')))
$$;
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
                    AND receipt.result_revision=owner.revision AND audit.actor_id=receipt.actor_id AND audit.event_kind='action_reconciled');
        IF NEW.retry_count<>OLD.retry_count OR NEW.max_retries<>OLD.max_retries
            OR NEW.workflow_execution_id<>OLD.workflow_execution_id OR NEW.id<>OLD.id
            OR owner.state<>'running' OR owner.terminal_execution_id IS NOT NULL
            OR NOT (ordinary OR reconciled)
        THEN RAISE EXCEPTION 'invalid explicit workflow retry' USING ERRCODE='23514'; END IF;
    END IF;
    RETURN NULL;
END $$;
