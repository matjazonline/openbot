-- Exact reconciliation episodes are provenance, not a second runnable queue.
-- Existing applied migrations and historical commands are immutable.
LOCK TABLE workflow_action_evidence_commands, background_tasks IN SHARE ROW EXCLUSIVE MODE;
DO $$ BEGIN
    IF EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
        JOIN background_tasks AS job ON job.company_id=command.company_id AND job.id=command.scheduled_job_id
        WHERE command.outcome->>'kind'='scheduled' AND job.status='pending'
            AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL)
    THEN RAISE EXCEPTION 'workflow reconciliation pending episode provenance is ambiguous' USING ERRCODE='23514'; END IF;
END $$;

CREATE FUNCTION workflow_action_reconciliation_budget_eligible(company uuid, run uuid)
RETURNS boolean LANGUAGE sql AS $$
    SELECT EXISTS(SELECT 1 FROM workflow_run_budgets AS link
        JOIN workflow_root_budgets AS budget ON budget.company_id=link.company_id AND budget.root_run_id=link.root_run_id
        JOIN workflow_root_budget_usage AS usage ON usage.company_id=budget.company_id AND usage.root_run_id=budget.root_run_id
        WHERE link.company_id=company AND link.run_id=run AND budget.provenance='frozen_v2'
            AND usage.activations<=budget.activations AND usage.model_calls<budget.model_calls
            AND usage.repetitions<=budget.repetitions
            AND NOT EXISTS(SELECT 1 FROM workflow_budget_receipts AS receipt
                WHERE receipt.company_id=company AND receipt.run_id=run AND receipt.disposition='exhausted'))
$$;

CREATE TABLE workflow_action_schedule_witnesses (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL, job_id uuid NOT NULL,
    retired_attempt integer NOT NULL CHECK(retired_attempt>0), transaction_id xid8 NOT NULL,
    PRIMARY KEY(company_id,job_id,retired_attempt,transaction_id),
    FOREIGN KEY(company_id,run_id,execution_id) REFERENCES workflow_executions(company_id,run_id,id),
    FOREIGN KEY(company_id,execution_id,job_id) REFERENCES background_tasks(company_id,workflow_execution_id,id),
    FOREIGN KEY(job_id,retired_attempt) REFERENCES task_attempts(task_id,attempt_number)
);
CREATE TRIGGER workflow_action_schedule_witness_owner BEFORE INSERT ON workflow_action_schedule_witnesses
    FOR EACH ROW EXECUTE FUNCTION workflow_action_state_witness_owner();
CREATE TRIGGER workflow_action_schedule_witness_immutable BEFORE UPDATE OR DELETE ON workflow_action_schedule_witnesses
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

ALTER TABLE workflow_action_evidence_commands ADD CONSTRAINT workflow_action_command_episode_scope
    UNIQUE(company_id,run_id,execution_id,scheduled_job_id,command_key);

CREATE TABLE workflow_action_claim_episodes (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL, job_id uuid NOT NULL,
    command_key text NOT NULL, retired_attempt integer NOT NULL CHECK(retired_attempt>0),
    PRIMARY KEY(company_id,command_key),
    UNIQUE(company_id,job_id,retired_attempt),
    UNIQUE(company_id,run_id,execution_id,job_id,command_key,retired_attempt),
    FOREIGN KEY(company_id,run_id,execution_id) REFERENCES workflow_executions(company_id,run_id,id),
    FOREIGN KEY(company_id,execution_id,job_id) REFERENCES background_tasks(company_id,workflow_execution_id,id),
    FOREIGN KEY(company_id,run_id,execution_id,job_id,command_key)
        REFERENCES workflow_action_evidence_commands(company_id,run_id,execution_id,scheduled_job_id,command_key),
    FOREIGN KEY(job_id,retired_attempt) REFERENCES task_attempts(task_id,attempt_number)
);
CREATE TRIGGER workflow_action_claim_episode_immutable BEFORE UPDATE OR DELETE ON workflow_action_claim_episodes
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

CREATE FUNCTION workflow_action_capture_schedule() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE run uuid;
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='failed' AND NEW.status='pending' THEN
        SELECT execution.run_id INTO run FROM workflow_executions AS execution
            WHERE execution.company_id=OLD.company_id AND execution.id=OLD.workflow_execution_id;
        INSERT INTO workflow_action_schedule_witnesses(company_id,run_id,execution_id,job_id,retired_attempt,transaction_id)
            VALUES(OLD.company_id,run,OLD.workflow_execution_id,OLD.id,OLD.retry_count,pg_current_xact_id());
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_capture_schedule BEFORE UPDATE ON background_tasks
    FOR EACH ROW EXECUTE FUNCTION workflow_action_capture_schedule();

CREATE FUNCTION workflow_action_bind_schedule() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.outcome->>'kind'='scheduled' THEN
        INSERT INTO workflow_action_claim_episodes(company_id,run_id,execution_id,job_id,command_key,retired_attempt)
            SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.scheduled_job_id,NEW.command_key,witness.retired_attempt
            FROM workflow_action_schedule_witnesses AS witness
            WHERE witness.company_id=NEW.company_id AND witness.run_id=NEW.run_id
                AND witness.execution_id=NEW.execution_id AND witness.job_id=NEW.scheduled_job_id
                AND witness.transaction_id=pg_current_xact_id();
        IF NOT FOUND THEN RAISE EXCEPTION 'workflow reconciliation schedule lacks current episode witness' USING ERRCODE='23514'; END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_bind_schedule AFTER INSERT ON workflow_action_evidence_commands
    FOR EACH ROW EXECUTE FUNCTION workflow_action_bind_schedule();

CREATE FUNCTION workflow_action_claim_episode_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
        JOIN workflow_runs AS owner ON owner.company_id=command.company_id AND owner.id=command.run_id
        JOIN background_tasks AS job ON job.company_id=command.company_id AND job.id=command.scheduled_job_id
        JOIN task_attempts AS attempt ON attempt.task_id=job.id AND attempt.attempt_number=NEW.retired_attempt
        JOIN workflow_action_schedule_witnesses AS schedule ON schedule.company_id=NEW.company_id
            AND schedule.run_id=NEW.run_id AND schedule.execution_id=NEW.execution_id AND schedule.job_id=NEW.job_id
            AND schedule.retired_attempt=NEW.retired_attempt AND schedule.transaction_id=pg_current_xact_id()
        JOIN workflow_action_state_witnesses AS state ON state.company_id=NEW.company_id AND state.run_id=NEW.run_id
            AND state.transaction_id=pg_current_xact_id()
        WHERE command.company_id=NEW.company_id AND command.command_key=NEW.command_key
            AND command.run_id=NEW.run_id AND command.execution_id=NEW.execution_id AND command.scheduled_job_id=NEW.job_id
            AND command.outcome->>'kind'='scheduled' AND command.result_revision=owner.revision
            AND state.initial_state='waiting' AND state.initial_waiting_reason='reconciliation'
            AND job.retry_count=NEW.retired_attempt AND job.status='pending'
            AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=NEW.job_id AND later.attempt_number>NEW.retired_attempt))
    THEN RAISE EXCEPTION 'invalid current workflow reconciliation claim episode' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_action_claim_episode_guard AFTER INSERT ON workflow_action_claim_episodes
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_action_claim_episode_guard();

CREATE FUNCTION workflow_action_pending_claim_episode(company uuid, run uuid, execution uuid, job uuid)
RETURNS text LANGUAGE sql AS $$
    SELECT episode.command_key FROM workflow_action_claim_episodes AS episode
        JOIN background_tasks AS task ON task.company_id=episode.company_id AND task.id=episode.job_id
        JOIN workflow_executions AS step ON step.company_id=episode.company_id AND step.run_id=episode.run_id AND step.id=episode.execution_id
        WHERE episode.company_id=company AND episode.run_id=run AND episode.execution_id=execution AND episode.job_id=job
            AND task.queue_kind='workflow' AND task.status='pending' AND task.retry_count=episode.retired_attempt
            AND task.worker_id IS NULL AND task.execution_generation IS NULL AND task.lock_expires_at IS NULL
            AND step.activated_at IS NOT NULL AND step.completed_at IS NULL
            AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=job AND later.attempt_number>episode.retired_attempt)
$$;

-- Database-owned OLD state and current-xid retirement/audit linkage. No client
-- previous-state label can create or rewrite these witnesses.
CREATE TABLE workflow_action_claim_retirement_witnesses (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL, job_id uuid NOT NULL,
    command_key text NOT NULL, retired_attempt integer NOT NULL, transaction_id xid8 NOT NULL,
    deadline timestamptz NOT NULL, retirement_confirmed boolean NOT NULL DEFAULT false, audit_sequence bigint,
    PRIMARY KEY(company_id,command_key,transaction_id),
    FOREIGN KEY(company_id,run_id,execution_id,job_id,command_key,retired_attempt)
        REFERENCES workflow_action_claim_episodes(company_id,run_id,execution_id,job_id,command_key,retired_attempt),
    FOREIGN KEY(company_id,run_id,audit_sequence) REFERENCES workflow_run_events(company_id,run_id,sequence)
);
CREATE FUNCTION workflow_action_claim_retirement_witness_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF pg_trigger_depth()<2 OR TG_OP='DELETE' OR NEW.transaction_id<>pg_current_xact_id()
    THEN RAISE EXCEPTION 'workflow claim retirement witness is database owned' USING ERRCODE='23514'; END IF;
    IF TG_OP='UPDATE' AND (
        (to_jsonb(OLD)-'retirement_confirmed'-'audit_sequence') IS DISTINCT FROM (to_jsonb(NEW)-'retirement_confirmed'-'audit_sequence')
        OR (OLD.retirement_confirmed AND NOT NEW.retirement_confirmed)
        OR (OLD.audit_sequence IS NOT NULL AND OLD.audit_sequence IS DISTINCT FROM NEW.audit_sequence))
    THEN RAISE EXCEPTION 'workflow claim retirement witness is append-once' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_claim_retirement_witness_owner BEFORE INSERT OR UPDATE OR DELETE ON workflow_action_claim_retirement_witnesses
    FOR EACH ROW EXECUTE FUNCTION workflow_action_claim_retirement_witness_owner();

CREATE FUNCTION workflow_action_capture_claim_retirement() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE episode workflow_action_claim_episodes%ROWTYPE; expected_state text; candidates bigint;
BEGIN
    IF OLD.state IN ('queued','running') AND NEW.state IN ('failed','waiting') AND OLD.deadline>clock_timestamp() THEN
        SELECT count(*) INTO candidates FROM workflow_action_claim_episodes AS binding
            WHERE binding.company_id=OLD.company_id AND binding.run_id=OLD.id
                AND workflow_action_pending_claim_episode(binding.company_id,binding.run_id,binding.execution_id,binding.job_id)=binding.command_key;
        IF candidates>1 THEN RAISE EXCEPTION 'multiple current workflow reconciliation claim episodes' USING ERRCODE='23514'; END IF;
        SELECT binding.* INTO episode FROM workflow_action_claim_episodes AS binding
            WHERE binding.company_id=OLD.company_id AND binding.run_id=OLD.id
                AND workflow_action_pending_claim_episode(binding.company_id,binding.run_id,binding.execution_id,binding.job_id)=binding.command_key;
        IF FOUND THEN
            -- Same order as claim: requesting run -> execution -> job -> shared usage.
            PERFORM step.id FROM workflow_executions AS step WHERE step.company_id=episode.company_id AND step.id=episode.execution_id FOR UPDATE;
            PERFORM job.id FROM background_tasks AS job WHERE job.company_id=episode.company_id AND job.id=episode.job_id FOR UPDATE;
            PERFORM usage.root_run_id FROM workflow_root_budget_usage AS usage
                JOIN workflow_run_budgets AS link ON link.company_id=usage.company_id AND link.root_run_id=usage.root_run_id
                WHERE link.company_id=OLD.company_id AND link.run_id=OLD.id FOR UPDATE OF usage;
            expected_state:=CASE WHEN workflow_action_retry_safe(episode.company_id,episode.execution_id) IS FALSE THEN 'waiting' ELSE 'failed' END;
            IF NOT workflow_action_reconciliation_budget_eligible(OLD.company_id,OLD.id)
                AND OLD.deadline>clock_timestamp() AND NEW.state=expected_state
                AND ((expected_state='waiting' AND NEW.waiting_reason='reconciliation' AND NEW.terminal_execution_id IS NULL)
                    OR (expected_state='failed' AND NEW.waiting_reason IS NULL AND NEW.terminal_execution_id=episode.execution_id)) THEN
                INSERT INTO workflow_action_claim_retirement_witnesses(company_id,run_id,execution_id,job_id,command_key,retired_attempt,transaction_id,deadline)
                    VALUES(episode.company_id,episode.run_id,episode.execution_id,episode.job_id,episode.command_key,episode.retired_attempt,pg_current_xact_id(),OLD.deadline);
            END IF;
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_capture_claim_retirement BEFORE UPDATE ON workflow_runs
    FOR EACH ROW EXECUTE FUNCTION workflow_action_capture_claim_retirement();

CREATE FUNCTION workflow_action_confirm_claim_retirement() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='pending' AND NEW.status='failed'
        AND OLD.worker_id IS NULL AND OLD.execution_generation IS NULL AND OLD.lock_expires_at IS NULL
        AND NEW.worker_id IS NULL AND NEW.execution_generation IS NULL AND NEW.lock_expires_at IS NULL
        AND NEW.retry_count=OLD.retry_count AND NEW.workflow_execution_id=OLD.workflow_execution_id THEN
        UPDATE workflow_action_claim_retirement_witnesses AS witness SET retirement_confirmed=true
            WHERE witness.company_id=OLD.company_id AND witness.job_id=OLD.id AND witness.execution_id=OLD.workflow_execution_id
                AND witness.retired_attempt=OLD.retry_count AND witness.transaction_id=pg_current_xact_id()
                AND workflow_action_pending_claim_episode(witness.company_id,witness.run_id,witness.execution_id,witness.job_id)=witness.command_key;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_confirm_claim_retirement BEFORE UPDATE ON background_tasks
    FOR EACH ROW EXECUTE FUNCTION workflow_action_confirm_claim_retirement();

CREATE FUNCTION workflow_action_link_claim_retirement_audit() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.event_kind='workflow.root_budget_exhausted' AND NEW.execution_id IS NOT NULL THEN
        UPDATE workflow_action_claim_retirement_witnesses AS witness SET audit_sequence=NEW.sequence
            WHERE witness.company_id=NEW.company_id AND witness.run_id=NEW.run_id AND witness.execution_id=NEW.execution_id
                AND witness.transaction_id=pg_current_xact_id() AND witness.retirement_confirmed AND witness.audit_sequence IS NULL;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_link_claim_retirement_audit AFTER INSERT ON workflow_run_events
    FOR EACH ROW EXECUTE FUNCTION workflow_action_link_claim_retirement_audit();

CREATE TABLE workflow_action_claim_budget_refusals (
    company_id uuid NOT NULL, run_id uuid NOT NULL, execution_id uuid NOT NULL, job_id uuid NOT NULL,
    command_key text NOT NULL, retired_attempt integer NOT NULL, audit_sequence bigint NOT NULL,
    PRIMARY KEY(company_id,command_key),
    FOREIGN KEY(company_id,run_id,execution_id,job_id,command_key,retired_attempt)
        REFERENCES workflow_action_claim_episodes(company_id,run_id,execution_id,job_id,command_key,retired_attempt),
    FOREIGN KEY(company_id,run_id,audit_sequence) REFERENCES workflow_run_events(company_id,run_id,sequence)
);
CREATE TRIGGER workflow_action_claim_budget_refusal_immutable BEFORE UPDATE OR DELETE ON workflow_action_claim_budget_refusals
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

CREATE FUNCTION workflow_action_claim_budget_refusal_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- Raw SQL obtains the same serialization; historical witnesses cannot qualify.
    PERFORM owner.id FROM workflow_runs AS owner WHERE owner.company_id=NEW.company_id AND owner.id=NEW.run_id FOR UPDATE;
    PERFORM step.id FROM workflow_executions AS step WHERE step.company_id=NEW.company_id AND step.run_id=NEW.run_id AND step.id=NEW.execution_id FOR UPDATE;
    PERFORM job.id FROM background_tasks AS job WHERE job.company_id=NEW.company_id AND job.id=NEW.job_id FOR UPDATE;
    PERFORM usage.root_run_id FROM workflow_root_budget_usage AS usage
        JOIN workflow_run_budgets AS link ON link.company_id=usage.company_id AND link.root_run_id=usage.root_run_id
        WHERE link.company_id=NEW.company_id AND link.run_id=NEW.run_id FOR UPDATE OF usage;
    IF workflow_action_reconciliation_budget_eligible(NEW.company_id,NEW.run_id)
        OR NOT EXISTS(SELECT 1 FROM workflow_action_claim_retirement_witnesses AS witness
            JOIN workflow_runs AS owner ON owner.company_id=witness.company_id AND owner.id=witness.run_id
            JOIN background_tasks AS job ON job.company_id=witness.company_id AND job.id=witness.job_id
            JOIN workflow_executions AS step ON step.company_id=witness.company_id AND step.run_id=witness.run_id AND step.id=witness.execution_id
            JOIN workflow_run_events AS audit ON audit.company_id=witness.company_id AND audit.run_id=witness.run_id AND audit.sequence=witness.audit_sequence
            WHERE witness.company_id=NEW.company_id AND witness.run_id=NEW.run_id AND witness.execution_id=NEW.execution_id
                AND witness.job_id=NEW.job_id AND witness.command_key=NEW.command_key AND witness.retired_attempt=NEW.retired_attempt
                AND witness.transaction_id=pg_current_xact_id() AND witness.retirement_confirmed AND witness.audit_sequence=NEW.audit_sequence
                AND witness.deadline>clock_timestamp() AND owner.deadline>clock_timestamp()
                AND ((owner.state='failed' AND owner.terminal_execution_id=NEW.execution_id AND owner.waiting_reason IS NULL
                    AND workflow_action_retry_safe(NEW.company_id,NEW.execution_id) IS DISTINCT FROM false)
                    OR (owner.state='waiting' AND owner.waiting_reason='reconciliation' AND owner.terminal_execution_id IS NULL
                        AND workflow_action_retry_safe(NEW.company_id,NEW.execution_id) IS FALSE))
                AND job.status='failed' AND job.queue_kind='workflow' AND job.workflow_execution_id=NEW.execution_id AND job.retry_count=NEW.retired_attempt
                AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL
                AND step.activated_at IS NOT NULL AND step.completed_at IS NULL
                AND audit.execution_id=NEW.execution_id AND audit.event_kind='workflow.root_budget_exhausted'
                AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=NEW.job_id AND later.attempt_number>NEW.retired_attempt))
    THEN RAISE EXCEPTION 'invalid current workflow reconciliation budget refusal' USING ERRCODE='23514'; END IF;
    IF TG_WHEN='BEFORE' THEN RETURN NEW; END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER workflow_action_claim_budget_refusal_guard BEFORE INSERT ON workflow_action_claim_budget_refusals
    FOR EACH ROW EXECUTE FUNCTION workflow_action_claim_budget_refusal_guard();
CREATE CONSTRAINT TRIGGER workflow_action_claim_budget_refusal_commit_guard AFTER INSERT ON workflow_action_claim_budget_refusals
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_action_claim_budget_refusal_guard();

CREATE FUNCTION workflow_action_claim_retirement_atomic_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.audit_sequence IS NOT NULL AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
        WHERE refusal.company_id=NEW.company_id AND refusal.run_id=NEW.run_id AND refusal.execution_id=NEW.execution_id
            AND refusal.job_id=NEW.job_id AND refusal.command_key=NEW.command_key AND refusal.retired_attempt=NEW.retired_attempt
            AND refusal.audit_sequence=NEW.audit_sequence)
    THEN RAISE EXCEPTION 'workflow claim budget retirement requires atomic refusal' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_action_claim_retirement_atomic_guard AFTER INSERT OR UPDATE ON workflow_action_claim_retirement_witnesses
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION workflow_action_claim_retirement_atomic_guard();

CREATE FUNCTION workflow_action_claim_refusal_audit_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM workflow_action_claim_retirement_witnesses AS witness
        WHERE witness.company_id=OLD.company_id AND witness.run_id=OLD.run_id AND witness.audit_sequence=OLD.sequence)
        OR EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
            WHERE refusal.company_id=OLD.company_id AND refusal.run_id=OLD.run_id AND refusal.audit_sequence=OLD.sequence)
    THEN RAISE EXCEPTION 'workflow claim budget retirement audit is immutable' USING ERRCODE='23514'; END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_action_claim_refusal_audit_immutable BEFORE UPDATE OR DELETE ON workflow_run_events
    FOR EACH ROW EXECUTE FUNCTION workflow_action_claim_refusal_audit_immutable();

CREATE OR REPLACE FUNCTION workflow_action_reconciliation_reopen_safe(company uuid, run uuid, job uuid)
RETURNS boolean LANGUAGE sql AS $$
    SELECT EXISTS(SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
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
            AND workflow_action_reconciliation_budget_eligible(company,run)
            AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
                WHERE refusal.company_id=company AND refusal.run_id=run AND refusal.execution_id=execution.id AND refusal.job_id=job)
            AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
            AND NOT EXISTS(SELECT 1 FROM workflow_executions AS sibling
                JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                WHERE sibling.company_id=company AND sibling.run_id=run AND other.queue_kind='workflow'
                    AND other.id<>job AND other.status IN ('pending','processing')))
$$;

CREATE OR REPLACE FUNCTION workflow_control_retry_safe(company uuid, run uuid, job uuid)
RETURNS boolean LANGUAGE sql AS $$
    SELECT EXISTS (
        SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
        WHERE owner.company_id=company AND owner.id=run AND task.id=job AND task.queue_kind='workflow'
            AND owner.deadline>clock_timestamp() AND execution.activation<=owner.max_steps
            AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
            AND execution.committed_output IS NULL AND execution.committed_route IS NULL
            AND execution.successor_execution_id IS NULL
            AND task.retry_count>0 AND task.retry_count<task.max_retries
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND workflow_action_retry_safe(company,execution.id) IS DISTINCT FROM false
            AND attempt.workflow_retry_safety='safe' AND attempt.workflow_retirement IN ('live','expired')
            AND attempt.workflow_failure_code NOT IN ('workflow.activation_limit','workflow.invalid_result','workflow.run_deadline')
            AND NOT EXISTS (SELECT 1 FROM workflow_budget_receipts AS receipt
                WHERE receipt.company_id=company AND receipt.run_id=run AND receipt.disposition='exhausted')
            AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
                WHERE refusal.company_id=company AND refusal.run_id=run AND refusal.execution_id=execution.id AND refusal.job_id=job)
            AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
            AND NOT EXISTS (SELECT 1 FROM workflow_executions AS sibling
                JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                WHERE sibling.company_id=company AND sibling.run_id=run AND other.queue_kind='workflow'
                    AND other.id<>job AND other.status IN ('pending','processing'))
    )
$$;
