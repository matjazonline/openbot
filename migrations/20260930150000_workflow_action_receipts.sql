-- The first marker is immutable; NULL legacy evidence remains unknown forever.
ALTER TABLE workflow_action_dispatches ADD COLUMN replay_subject bytea
    CHECK (octet_length(replay_subject) BETWEEN 1 AND 524288);

CREATE TABLE workflow_action_remote_entries (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    id uuid NOT NULL,
    job_id uuid NOT NULL,
    attempt_number integer NOT NULL CHECK (attempt_number > 0),
    execution_generation uuid NOT NULL,
    worker_id uuid NOT NULL,
    effect_kind text NOT NULL DEFAULT 'remote' CHECK (effect_kind='remote'),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, invocation_id, job_id, attempt_number),
    UNIQUE (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id),
    FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind)
        REFERENCES workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind),
    FOREIGN KEY (company_id, execution_id, job_id)
        REFERENCES background_tasks(company_id, workflow_execution_id, id),
    FOREIGN KEY (job_id, attempt_number, execution_generation, worker_id)
        REFERENCES task_attempts(task_id, attempt_number, execution_generation, worker_id)
);
CREATE TRIGGER workflow_action_remote_entry_immutable BEFORE UPDATE OR DELETE ON workflow_action_remote_entries
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

ALTER TABLE workflow_action_receipts DROP CONSTRAINT workflow_action_receipts_effect_kind_check;
ALTER TABLE workflow_action_receipts ADD COLUMN remote_entry_id uuid;
ALTER TABLE workflow_action_receipts ADD CONSTRAINT workflow_action_receipt_kind CHECK (
    (effect_kind='local' AND remote_entry_id IS NULL) OR
    (effect_kind='remote' AND remote_entry_id IS NOT NULL));
ALTER TABLE workflow_action_receipts ADD CONSTRAINT workflow_action_receipt_remote_entry FOREIGN KEY
    (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id)
    REFERENCES workflow_action_remote_entries(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id);

-- Historical facts establish scheduling eligibility only, never trusted provider
-- registration or an I/O grant. Decode the exact bytes; hash those bytes, not JSONB text.
CREATE FUNCTION workflow_action_replay_supported(company uuid, invocation uuid)
RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE marker workflow_action_dispatches%ROWTYPE;
    operation jsonb; subject jsonb; proof jsonb; guarantee jsonb;
    seconds numeric; nanos numeric; horizon timestamptz;
BEGIN
    SELECT * INTO marker FROM workflow_action_dispatches
        WHERE company_id=company AND invocation_id=invocation;
    IF NOT FOUND OR marker.effect_kind<>'remote' OR marker.replay_subject IS NULL THEN RETURN false; END IF;
    SELECT intent.operation, owner.deadline INTO operation,horizon
        FROM workflow_action_intents AS intent
        JOIN workflow_runs AS owner ON owner.company_id=intent.company_id AND owner.id=intent.run_id
        WHERE intent.company_id=company AND intent.id=invocation;
    subject := convert_from(marker.replay_subject,'UTF8')::jsonb;
    proof := marker.current_policy->'provider_replay';
    guarantee := proof->'guarantee';
    IF subject IS DISTINCT FROM jsonb_build_object('tool',operation->'contract','target',operation->'target')
        OR operation->'contract'->>'company_id' IS DISTINCT FROM company::text
        OR marker.current_policy->'tool' IS DISTINCT FROM operation->'contract'
        OR marker.current_policy->'approval_required' IS DISTINCT FROM 'false'::jsonb
        OR jsonb_typeof(proof) IS DISTINCT FROM 'object'
        OR proof->'version' IS DISTINCT FROM '1'::jsonb
        OR jsonb_typeof(proof->'registration') IS DISTINCT FROM 'string'
        OR (proof->>'registration') !~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'
        OR jsonb_typeof(proof->'operation') IS DISTINCT FROM 'string'
        OR proof->>'operation' IS DISTINCT FROM encode(sha256(marker.replay_subject),'hex')
        OR proof - ARRAY['version','registration','operation','guarantee'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee) IS DISTINCT FROM 'object'
        OR COALESCE((operation->'contract'->'policy'->>'policy_revision')::bigint,0) <= 0
    THEN RETURN false; END IF;
    IF operation->'contract'->'policy'->>'recovery'='safe_repeat' THEN
        RETURN guarantee = '{"mode":"safe_repeat"}'::jsonb;
    END IF;
    IF operation->'contract'->'policy'->>'recovery' IS DISTINCT FROM 'provider_idempotency'
        OR guarantee->>'mode' IS DISTINCT FROM 'provider_idempotency'
        OR guarantee - ARRAY['mode','retention'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee->'retention') IS DISTINCT FROM 'object'
        OR guarantee->'retention' - ARRAY['secs','nanos'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee->'retention'->'secs') IS DISTINCT FROM 'number'
        OR jsonb_typeof(guarantee->'retention'->'nanos') IS DISTINCT FROM 'number'
    THEN RETURN false; END IF;
    seconds := (guarantee->'retention'->>'secs')::numeric;
    nanos := (guarantee->'retention'->>'nanos')::numeric;
    IF seconds <> trunc(seconds) OR nanos <> trunc(nanos) OR seconds < 0
        OR nanos < 0 OR nanos >= 1000000000 OR seconds+nanos/1000000000 <= 0
        OR seconds+nanos/1000000000 > 31536000 THEN RETURN false; END IF;
    -- Round down sub-microsecond guarantees, conservatively. Both first creation
    -- and the run horizon are authoritative DB timestamps, immune to lock waits.
    RETURN horizon>clock_timestamp() AND marker.created_at
        + make_interval(secs => (seconds+floor(nanos/1000)/1000000)::double precision) >= horizon;
EXCEPTION WHEN OTHERS THEN RETURN false;
END $$;

-- NULL preserves phase03's no-marker decision. Scan at most 129 facts; overflow
-- is unknown. Every unsafe sibling vetoes a supplied or historical safe label.
CREATE FUNCTION workflow_action_retry_safe(company uuid, execution uuid)
RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE fact record; scanned integer := 0; safe boolean := true;
BEGIN
    FOR fact IN SELECT marker.invocation_id FROM workflow_action_dispatches AS marker
        WHERE marker.company_id=company AND marker.execution_id=execution
        ORDER BY marker.invocation_id LIMIT 129
    LOOP
        scanned := scanned+1;
        IF scanned>128 THEN RETURN false; END IF;
        IF NOT EXISTS (SELECT 1 FROM workflow_action_receipts AS receipt
            WHERE receipt.company_id=company AND receipt.invocation_id=fact.invocation_id)
            AND NOT COALESCE(workflow_action_replay_supported(company,fact.invocation_id),false)
        THEN safe := false; END IF;
    END LOOP;
    IF scanned=0 THEN RETURN NULL; END IF;
    RETURN safe;
END $$;

CREATE FUNCTION workflow_action_remote_entry_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE marker workflow_action_dispatches%ROWTYPE;
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    PERFORM id FROM workflow_executions WHERE company_id=NEW.company_id AND id=NEW.execution_id FOR UPDATE;
    PERFORM id FROM background_tasks WHERE company_id=NEW.company_id AND id=NEW.job_id FOR UPDATE;
    PERFORM id FROM task_attempts WHERE task_id=NEW.job_id AND attempt_number=NEW.attempt_number FOR UPDATE;
    IF NOT EXISTS (
        SELECT 1 FROM workflow_runs AS owner
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
          AND attempt.execution_generation=NEW.execution_generation AND attempt.status='processing'
    ) THEN RAISE EXCEPTION 'workflow action remote entry lost live fence'; END IF;
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id;
    IF EXISTS (SELECT 1 FROM workflow_action_receipts WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id)
        OR EXISTS (SELECT 1 FROM workflow_action_remote_entries AS entry
            JOIN task_attempts AS attempt ON attempt.task_id=entry.job_id AND attempt.attempt_number=entry.attempt_number
            WHERE entry.company_id=NEW.company_id AND entry.invocation_id=NEW.invocation_id
                AND entry.id<>NEW.id AND attempt.status='processing')
        OR (marker.attempt_number<>NEW.attempt_number AND (
            marker.attempt_number>=NEW.attempt_number
            OR NOT COALESCE(workflow_action_replay_supported(NEW.company_id,NEW.invocation_id),false)))
    THEN RAISE EXCEPTION 'workflow action remote entry lacks supported replay'; END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workflow_action_remote_entry_commit_guard
    AFTER INSERT ON workflow_action_remote_entries DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION workflow_action_remote_entry_commit_guard();

-- Preserve every phase03 retirement and control constraint, adding action truth.
CREATE OR REPLACE FUNCTION check_workflow_fenced_retirement() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE retirement task_attempts%ROWTYPE;
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='processing' AND NEW.status IN ('pending','failed') THEN
        SELECT * INTO retirement FROM task_attempts WHERE task_id=OLD.id
            AND attempt_number=OLD.retry_count+1 AND execution_generation=OLD.execution_generation
            AND worker_id=OLD.worker_id AND status='failed';
        IF NOT FOUND OR retirement.workflow_retirement IS NULL OR NEW.retry_count<>OLD.retry_count+1
            OR (retirement.workflow_retirement='live' AND OLD.lock_expires_at<=clock_timestamp())
            OR (retirement.workflow_retirement='expired' AND OLD.lock_expires_at>clock_timestamp())
            OR (NEW.status='pending' AND (retirement.workflow_retry_safety<>'safe'
                OR retirement.workflow_failure_class<>'retryable' OR NEW.retry_count>=NEW.max_retries
                OR NEW.run_at<=retirement.finished_at)) THEN
            RAISE EXCEPTION 'workflow failure lost ownership or retry eligibility' USING ERRCODE='23514';
        END IF;
        IF retirement.workflow_retry_safety='safe' AND
            workflow_action_retry_safe(NEW.company_id,NEW.workflow_execution_id) IS FALSE THEN
            RAISE EXCEPTION 'workflow action outcome unknown at retirement' USING ERRCODE='23514';
        END IF;
        IF retirement.workflow_retirement='deadline' AND (
            NEW.status<>'failed' OR retirement.workflow_failure_class<>'terminal'
            OR retirement.workflow_failure_code<>'workflow.run_deadline'
            OR retirement.workflow_retry_safety<>'unknown'
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.deadline<=clock_timestamp() AND run.state='failed'
                    AND execution.successor_execution_id IS NULL)) THEN
            RAISE EXCEPTION 'invalid workflow deadline retirement' USING ERRCODE='23514';
        END IF;
        IF retirement.workflow_retirement='cancel' AND (
            NEW.status<>'failed' OR retirement.workflow_failure_class<>'terminal'
            OR retirement.workflow_failure_code<>'workflow.cancelled'
            OR retirement.workflow_retry_safety<>'unknown'
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.state='cancelled' AND execution.completed_at IS NULL
                    AND execution.successor_execution_id IS NULL)) THEN
            RAISE EXCEPTION 'invalid workflow cancellation retirement' USING ERRCODE='23514';
        END IF;
        -- A retirement may fail or suspend an expired run, but cannot schedule
        -- a retry or successor after its original deadline, including commit delay.
        IF (NEW.status='pending' OR EXISTS (
                SELECT 1 FROM workflow_executions WHERE company_id=NEW.company_id
                    AND id=NEW.workflow_execution_id AND successor_execution_id IS NOT NULL))
            AND EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.deadline<=clock_timestamp()) THEN
            RAISE EXCEPTION 'workflow retirement scheduled work after deadline' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NULL;
END $$;

-- Durable refusals cannot be reopened by operator retry, including direct SQL.
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
            AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
            AND NOT EXISTS (SELECT 1 FROM workflow_executions AS sibling
                JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                WHERE sibling.company_id=company AND sibling.run_id=run AND other.queue_kind='workflow'
                    AND other.id<>job AND other.status IN ('pending','processing'))
    )
$$;
