-- 04.11 provider-neutral messaging (decisions logs/04.11 D2 + D3 + D4).
-- No backward compatibility: reset the database when this migration does not apply cleanly.
-- One canonical message per destination; the `message` marker is the publication transaction's
-- first write; delivery rows own provider truth; the action row is the durable per-invocation wait.

-- 1. Vocabulary shared with accepted 04.1-04.10 facts.
ALTER TABLE workflow_action_dispatches
    DROP CONSTRAINT workflow_action_dispatches_effect_kind_check,
    ADD CONSTRAINT workflow_action_dispatches_effect_kind_check
        CHECK (effect_kind IN ('local', 'remote', 'message'));
ALTER TABLE workflow_action_receipts
    DROP CONSTRAINT workflow_action_receipt_kind,
    ADD CONSTRAINT workflow_action_receipt_kind CHECK (
        (effect_kind='local' AND remote_entry_id IS NULL AND reconciliation_evidence_id IS NULL)
        OR (effect_kind='remote' AND ((remote_entry_id IS NOT NULL)::integer
            +(reconciliation_evidence_id IS NOT NULL)::integer)=1)
        OR (effect_kind='message' AND remote_entry_id IS NULL AND reconciliation_evidence_id IS NULL));
ALTER TABLE workflow_action_reconciliations
    DROP CONSTRAINT workflow_action_reconciliations_effect_kind_check,
    ADD CONSTRAINT workflow_action_reconciliations_effect_kind_check
        CHECK (effect_kind IN ('remote', 'message'));
ALTER TABLE task_attempts
    DROP CONSTRAINT task_attempts_stop_reason_check,
    ADD CONSTRAINT task_attempts_stop_reason_check CHECK (stop_reason = ANY (ARRAY[
        'completed', 'retryable_failure', 'terminal_failure', 'timed_out', 'shutdown',
        'lease_lost', 'ownership_transferred', 'agent_instruction', 'delegation_cancelled',
        'operator_stopped', 'approval_rejected', 'channel_agent_removed', 'effect_parked']));
ALTER TABLE workflow_waits
    DROP CONSTRAINT workflow_wait_failure,
    ADD CONSTRAINT workflow_wait_failure CHECK ((state='failed') = (failure_code IS NOT NULL)
        AND (failure_code IS NULL
            OR failure_code IN ('activation_limit', 'invalid_output', 'evidence_conflict')));
ALTER TABLE message_deliveries
    DROP CONSTRAINT message_deliveries_purpose_check,
    ADD CONSTRAINT message_deliveries_purpose_check CHECK (purpose = ANY (ARRAY[
        'reply', 'mirror', 'outreach', 'notification', 'workflow_send', 'workflow_reply'])),
    DROP CONSTRAINT message_deliveries_cancellation_check,
    ADD CONSTRAINT message_deliveries_cancellation_check CHECK (
        ((cancellation_requested_at IS NULL) = (cancellation_reason IS NULL))
        AND (cancellation_reason IS NULL OR cancellation_reason = ANY (ARRAY[
            'channel_agent_removed', 'task_stopped', 'workflow_cancelled', 'workflow_expired',
            'workflow_revoked', 'recipient_revoked', 'evidence_conflict']))
        AND (cancellation_requested_at IS NULL OR status <> ALL (ARRAY['pending', 'retryable']))),
    ADD CONSTRAINT message_deliveries_company_message_key UNIQUE (company_id, id, message_id);
CREATE OR REPLACE FUNCTION public.valid_delivery_failure_class(class text) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN (class = ANY (ARRAY['authentication', 'rate_limited', 'invalid_payload',
        'destination_unavailable', 'network', 'timeout', 'provider_fault', 'internal',
        'dependency_failed', 'superseded', 'lease_expired', 'reconciled_not_applied',
        'not_applied_exhausted', 'reconciliation_proof_expired']));

-- 2. The durable per-invocation wait and its immutable destinations.
CREATE TABLE workflow_message_actions (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    effect_kind text NOT NULL DEFAULT 'message' CHECK (effect_kind='message'),
    origin text NOT NULL CHECK (origin IN ('step', 'model_tool')),
    wait_id uuid,
    -- Captured by the insert trigger from the live job; a writer cannot choose it.
    fence_expires_at timestamptz NOT NULL,
    destination_count smallint NOT NULL CHECK (destination_count BETWEEN 1 AND 20),
    state text NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'uncertain', 'accepted', 'failed', 'stalled')),
    consumed_through bigint NOT NULL DEFAULT 0 CHECK (consumed_through >= 0),
    consume_failures smallint NOT NULL DEFAULT 0 CHECK (consume_failures BETWEEN 0 AND 5),
    consume_after timestamptz NOT NULL DEFAULT clock_timestamp(),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, invocation_id),
    UNIQUE (company_id, run_id, execution_id, invocation_id),
    CHECK (origin='step' OR wait_id IS NULL),
    CHECK (state<>'stalled' OR consume_failures=5),
    CONSTRAINT workflow_message_action_marker FOREIGN KEY
        (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind)
        REFERENCES workflow_action_dispatches
            (company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind)
        ON DELETE CASCADE,
    CONSTRAINT workflow_message_action_owner FOREIGN KEY (company_id, run_id)
        REFERENCES workflow_runs(company_id, id) ON DELETE CASCADE,
    CONSTRAINT workflow_message_action_wait FOREIGN KEY (company_id, wait_id)
        REFERENCES workflow_waits(company_id, id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED
);
-- Receipt discovery probes open actions per execution (polling CANDIDATES_SQL, the consumer page).
CREATE INDEX workflow_message_actions_open
    ON workflow_message_actions (company_id, execution_id, consume_after, invocation_id)
    WHERE state IN ('pending', 'uncertain');
CREATE INDEX workflow_message_actions_wait ON workflow_message_actions (company_id, execution_id, wait_id)
    WHERE wait_id IS NOT NULL;

CREATE TABLE workflow_message_destinations (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    ordinal smallint NOT NULL CHECK (ordinal BETWEEN 0 AND 19),
    kind text NOT NULL CHECK (kind IN ('channel', 'user', 'reply')),
    destination_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    message_id uuid NOT NULL,
    delivery_id uuid,
    principal_id uuid,
    recipient_address text CHECK (recipient_address IS NULL
        OR (btrim(recipient_address)<>'' AND octet_length(recipient_address)<=998)),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, invocation_id, ordinal),
    UNIQUE (company_id, invocation_id, ordinal, delivery_id),
    UNIQUE (company_id, delivery_id),
    UNIQUE (company_id, message_id),
    UNIQUE (company_id, invocation_id, kind, destination_id),
    CHECK ((kind='channel' AND delivery_id IS NULL AND principal_id IS NULL AND recipient_address IS NULL)
        OR (kind='user' AND delivery_id IS NOT NULL AND principal_id IS NOT NULL AND recipient_address IS NOT NULL)
        OR (kind='reply' AND principal_id IS NULL
            AND (delivery_id IS NULL) = (recipient_address IS NULL))),
    CONSTRAINT workflow_message_destination_action FOREIGN KEY (company_id, invocation_id)
        REFERENCES workflow_message_actions(company_id, invocation_id) ON DELETE CASCADE,
    CONSTRAINT workflow_message_destination_owner FOREIGN KEY (company_id, run_id)
        REFERENCES workflow_runs(company_id, id) ON DELETE CASCADE,
    CONSTRAINT workflow_message_destination_message FOREIGN KEY (company_id, message_id)
        REFERENCES messages(company_id, id),
    CONSTRAINT workflow_message_destination_thread FOREIGN KEY (company_id, thread_id)
        REFERENCES threads(company_id, id),
    CONSTRAINT workflow_message_destination_delivery FOREIGN KEY (company_id, delivery_id, message_id)
        REFERENCES message_deliveries(company_id, id, message_id),
    CONSTRAINT workflow_message_destination_principal FOREIGN KEY (company_id, principal_id)
        REFERENCES principals(company_id, id)
);
-- Run-scoped workflow cancellation and expiry lock a run's linked deliveries.
CREATE INDEX workflow_message_destinations_run ON workflow_message_destinations (company_id, run_id)
    WHERE delivery_id IS NOT NULL;
CREATE TRIGGER workflow_message_destination_immutable BEFORE UPDATE OR DELETE ON workflow_message_destinations
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

-- 3. Receipt events: written only by the parent status trigger while the parent row lock is held.
CREATE TABLE workflow_message_delivery_events (
    seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    ordinal smallint NOT NULL,
    delivery_id uuid NOT NULL,
    old_status text NOT NULL,
    status text NOT NULL,
    failure_class text,
    observed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT workflow_message_event_destination FOREIGN KEY (company_id, invocation_id, ordinal, delivery_id)
        REFERENCES workflow_message_destinations(company_id, invocation_id, ordinal, delivery_id)
        ON DELETE CASCADE,
    CONSTRAINT workflow_message_event_owner FOREIGN KEY (company_id, run_id)
        REFERENCES workflow_runs(company_id, id) ON DELETE CASCADE
);
CREATE INDEX workflow_message_delivery_events_cursor
    ON workflow_message_delivery_events (company_id, invocation_id, seq);
CREATE TRIGGER workflow_message_event_immutable BEFORE UPDATE OR DELETE ON workflow_message_delivery_events
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

-- 4. Per-delivery evidence subjects (04.7 extension) and one-use reopen consumption.
ALTER TABLE workflow_action_evidence
    ADD COLUMN delivery_ordinal smallint,
    ADD COLUMN delivery_id uuid,
    ADD CONSTRAINT workflow_action_evidence_delivery_shape
        CHECK ((delivery_ordinal IS NULL) = (delivery_id IS NULL)),
    ADD CONSTRAINT workflow_action_evidence_delivery_subject FOREIGN KEY
        (company_id, invocation_id, delivery_ordinal, delivery_id)
        REFERENCES workflow_message_destinations(company_id, invocation_id, ordinal, delivery_id)
        ON DELETE CASCADE,
    DROP CONSTRAINT workflow_action_evidence_applied_request_check,
    ADD CONSTRAINT workflow_action_evidence_applied_request_check
        CHECK (applied_request IN ('remote_entry', 'marker_reservation', 'unattributed', 'delivery')),
    DROP CONSTRAINT workflow_action_applied_request_shape,
    -- The NULL alternative stays subject-neutral (R005 F014.1): message final-not-applied and
    -- unknown evidence carries a delivery subject and no attribution. The effect-kind <=> subject
    -- rule is enforced by workflow_action_evidence_commit_guard.
    ADD CONSTRAINT workflow_action_applied_request_shape CHECK (
        (applied_request IS NULL AND applied_remote_entry_id IS NULL)
        OR (disposition='applied' AND delivery_id IS NULL
            AND ((applied_request='remote_entry' AND applied_remote_entry_id IS NOT NULL)
                OR (applied_request IN ('marker_reservation', 'unattributed') AND applied_remote_entry_id IS NULL)))
        OR (disposition='applied' AND applied_request='delivery'
            AND applied_remote_entry_id IS NULL AND delivery_id IS NOT NULL));

CREATE TABLE workflow_message_delivery_reopens (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    id uuid NOT NULL,
    evidence_id uuid NOT NULL,
    ordinal smallint NOT NULL,
    delivery_id uuid NOT NULL,
    attempt_count integer NOT NULL CHECK (attempt_count >= 0),
    valid_until timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, evidence_id),
    CONSTRAINT workflow_message_reopen_evidence FOREIGN KEY
        (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, evidence_id)
        REFERENCES workflow_action_evidence
            (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id)
        ON DELETE CASCADE,
    CONSTRAINT workflow_message_reopen_destination FOREIGN KEY (company_id, invocation_id, ordinal, delivery_id)
        REFERENCES workflow_message_destinations(company_id, invocation_id, ordinal, delivery_id)
        ON DELETE CASCADE,
    CONSTRAINT workflow_message_reopen_owner FOREIGN KEY (company_id, run_id)
        REFERENCES workflow_runs(company_id, id) ON DELETE CASCADE
);
CREATE TRIGGER workflow_message_reopen_immutable BEFORE UPDATE OR DELETE ON workflow_message_delivery_reopens
    FOR EACH ROW EXECUTE FUNCTION workflow_action_fact_immutable();

ALTER TABLE message_deliveries
    ADD COLUMN settled_evidence_id uuid,
    ADD COLUMN reopen_id uuid,
    ADD CONSTRAINT message_deliveries_settled_evidence FOREIGN KEY (company_id, settled_evidence_id)
        REFERENCES workflow_action_evidence(company_id, id) ON DELETE SET NULL (settled_evidence_id),
    ADD CONSTRAINT message_deliveries_reopen FOREIGN KEY (company_id, reopen_id)
        REFERENCES workflow_message_delivery_reopens(company_id, id) ON DELETE SET NULL (reopen_id);

-- 5. Admission: at most 128 markers per execution through messaging (D2 2.2, D3 A).
CREATE FUNCTION workflow_message_admission_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- MAX_MESSAGE_ACTIONS_PER_EXECUTION = 128, the workflow_action_retry_safe proof ceiling.
    -- Lock the fenced attempt first so two SQL-only writers under one fence serialize here.
    PERFORM 1 FROM task_attempts WHERE task_id=NEW.job_id AND attempt_number=NEW.attempt_number
        AND execution_generation=NEW.execution_generation AND worker_id=NEW.worker_id FOR UPDATE;
    IF (SELECT count(*) FROM (SELECT 1 FROM workflow_action_dispatches
            WHERE company_id=NEW.company_id AND execution_id=NEW.execution_id LIMIT 128) AS bounded)>=128
    THEN RAISE EXCEPTION 'workflow message action limit' USING ERRCODE='54000'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_message_admission_guard BEFORE INSERT ON workflow_action_dispatches
    FOR EACH ROW WHEN (NEW.effect_kind='message') EXECUTE FUNCTION workflow_message_admission_guard();

-- 6. Fence capture: the database, not the writer, records the live lease expiry (F007).
CREATE FUNCTION workflow_message_action_capture() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE captured timestamptz;
BEGIN
    SELECT job.lock_expires_at INTO captured
    FROM workflow_action_dispatches AS marker
    JOIN background_tasks AS job ON job.company_id=marker.company_id AND job.id=marker.job_id
    JOIN task_attempts AS attempt ON attempt.task_id=marker.job_id AND attempt.attempt_number=marker.attempt_number
    WHERE marker.company_id=NEW.company_id AND marker.invocation_id=NEW.invocation_id
        AND marker.id=NEW.dispatch_id AND marker.effect_kind='message'
        AND job.status='processing' AND job.worker_id=marker.worker_id
        AND job.execution_generation=marker.execution_generation
        AND job.retry_count+1=marker.attempt_number AND job.lock_expires_at>clock_timestamp()
        AND attempt.status='processing' AND attempt.worker_id=marker.worker_id
        AND attempt.execution_generation=marker.execution_generation;
    IF captured IS NULL THEN
        RAISE EXCEPTION 'workflow message action requires a live fence' USING ERRCODE='23514';
    END IF;
    IF NEW.fence_expires_at IS NOT NULL AND NEW.fence_expires_at IS DISTINCT FROM captured THEN
        RAISE EXCEPTION 'workflow message fence expiry is database owned' USING ERRCODE='23514';
    END IF;
    NEW.fence_expires_at:=captured;
    NEW.state:='pending';
    NEW.consumed_through:=0;
    NEW.consume_failures:=0;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_message_action_capture BEFORE INSERT ON workflow_message_actions
    FOR EACH ROW EXECUTE FUNCTION workflow_message_action_capture();

-- 7. Destination aggregate shared by the state guard, the receipt guard and the consumer.
CREATE FUNCTION workflow_message_destination_state(delivery_id uuid, delivery_status text)
RETURNS text LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE WHEN delivery_id IS NULL OR delivery_status='delivered' THEN 'accepted'
        WHEN delivery_status='dead_letter' THEN 'failed'
        WHEN delivery_status='outcome_unknown' THEN 'unknown'
        ELSE 'pending' END
$$;
CREATE FUNCTION workflow_message_aggregate(company uuid, invocation uuid)
RETURNS TABLE(state text, result jsonb, destinations integer) LANGUAGE sql STABLE AS $$
    WITH destination AS (
        SELECT target.ordinal, target.kind, target.destination_id, target.message_id,
            workflow_message_destination_state(target.delivery_id, delivery.status) AS status
        FROM workflow_message_destinations AS target
        LEFT JOIN message_deliveries AS delivery
            ON delivery.company_id=target.company_id AND delivery.id=target.delivery_id
        WHERE target.company_id=company AND target.invocation_id=invocation
    )
    SELECT CASE WHEN bool_or(status='pending') THEN 'pending'
            WHEN bool_or(status='unknown') THEN 'uncertain'
            WHEN bool_and(status='accepted') THEN 'accepted' ELSE 'failed' END,
        jsonb_build_object(
            'status', CASE WHEN bool_and(status='accepted') THEN 'accepted' ELSE 'failed' END,
            'messages', jsonb_agg(jsonb_build_object(
                'destination', jsonb_build_object('kind', kind, 'id', destination_id::text),
                'message_id', message_id::text,
                'status', CASE WHEN status='accepted' THEN 'accepted' ELSE 'failed' END)
                ORDER BY ordinal)),
        count(*)::integer
    FROM destination
$$;

-- 8. Action-state guard: immutable linkage, monotone cursor, state equals the SQL aggregate.
CREATE FUNCTION workflow_message_action_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE aggregate record;
BEGIN
    IF TG_OP='DELETE' THEN
        IF pg_trigger_depth()>1 AND NOT EXISTS(SELECT 1 FROM workflow_runs
            WHERE company_id=OLD.company_id AND id=OLD.run_id) THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'workflow message action is retained with its run' USING ERRCODE='23514';
    END IF;
    IF (to_jsonb(OLD)-'state'-'consumed_through'-'consume_failures'-'consume_after'-'updated_at')
        IS DISTINCT FROM (to_jsonb(NEW)-'state'-'consumed_through'-'consume_failures'-'consume_after'-'updated_at')
        OR NEW.consumed_through<OLD.consumed_through OR NEW.consume_failures<OLD.consume_failures
    THEN RAISE EXCEPTION 'workflow message action linkage is immutable' USING ERRCODE='23514'; END IF;
    IF NEW.state IS DISTINCT FROM OLD.state THEN
        IF OLD.state IN ('accepted', 'failed', 'stalled') THEN
            RAISE EXCEPTION 'workflow message action state is final' USING ERRCODE='23514';
        END IF;
        IF NEW.state='stalled' THEN
            IF NEW.consume_failures<>5 THEN
                RAISE EXCEPTION 'workflow message action stalls only at five failures' USING ERRCODE='23514';
            END IF;
            RETURN NEW;
        END IF;
        SELECT * INTO aggregate FROM workflow_message_aggregate(NEW.company_id, NEW.invocation_id);
        IF aggregate.destinations<>NEW.destination_count OR aggregate.state IS DISTINCT FROM NEW.state
            OR (NEW.state IN ('accepted', 'failed') AND NOT EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt
                WHERE receipt.company_id=NEW.company_id AND receipt.invocation_id=NEW.invocation_id
                    AND receipt.effect_kind='message' AND receipt.result->>'status'=NEW.state))
        THEN RAISE EXCEPTION 'workflow message action state disagrees with its deliveries' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_message_action_guard BEFORE UPDATE OR DELETE ON workflow_message_actions
    FOR EACH ROW EXECUTE FUNCTION workflow_message_action_guard();

-- 9. Receipt guard: the result is derived from authoritative final delivery states (F007).
CREATE FUNCTION workflow_message_receipt_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE aggregate record; expected integer;
BEGIN
    SELECT destination_count INTO expected FROM workflow_message_actions
        WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id AND dispatch_id=NEW.dispatch_id;
    SELECT * INTO aggregate FROM workflow_message_aggregate(NEW.company_id, NEW.invocation_id);
    IF expected IS NULL OR aggregate.destinations<>expected
        OR aggregate.state NOT IN ('accepted', 'failed') OR NEW.result IS DISTINCT FROM aggregate.result
    THEN RAISE EXCEPTION 'workflow message receipt disagrees with its deliveries' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_message_receipt_guard BEFORE INSERT ON workflow_action_receipts
    FOR EACH ROW WHEN (NEW.effect_kind='message') EXECUTE FUNCTION workflow_message_receipt_guard();

-- 10. Effect waits: deadline is exactly the run deadline and the wait belongs to one action.
CREATE FUNCTION workflow_message_effect_wait_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.event_name IS NOT NULL OR NEW.correlation IS NOT NULL OR NEW.notification_intent IS NOT NULL
        OR NOT EXISTS(SELECT 1 FROM workflow_runs AS run WHERE run.company_id=NEW.company_id
            AND run.id=NEW.run_id AND run.deadline=NEW.deadline)
        OR NOT EXISTS(SELECT 1 FROM workflow_message_actions AS action WHERE action.company_id=NEW.company_id
            AND action.run_id=NEW.run_id AND action.execution_id=NEW.execution_id AND action.wait_id=NEW.id)
    THEN RAISE EXCEPTION 'workflow effect wait must belong to one message action' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_message_effect_wait_guard BEFORE INSERT ON workflow_waits
    FOR EACH ROW WHEN (NEW.reason='effect') EXECUTE FUNCTION workflow_message_effect_wait_guard();

-- 11. Deferred dispatch guard: local/remote branches unchanged; new message branch (D2 2.5, D3 B).
CREATE OR REPLACE FUNCTION workflow_action_dispatch_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE action workflow_message_actions%ROWTYPE; has_delivery boolean;
BEGIN
    -- This deferred trigger checks wall time at COMMIT, including all lock waits.
    -- Lock run first so SQL-only writers cannot race cancellation or claim replacement.
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    PERFORM id FROM workflow_executions WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND id=NEW.execution_id FOR UPDATE;
    PERFORM id FROM background_tasks WHERE company_id=NEW.company_id AND id=NEW.job_id FOR UPDATE;
    PERFORM id FROM task_attempts WHERE task_id=NEW.job_id AND attempt_number=NEW.attempt_number FOR UPDATE;
    IF NEW.effect_kind='message' THEN
        SELECT * INTO action FROM workflow_message_actions
            WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id AND dispatch_id=NEW.id;
        IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM workflow_message_destinations AS target
                WHERE target.company_id=NEW.company_id AND target.invocation_id=NEW.invocation_id)
            OR (SELECT count(*) FROM workflow_message_destinations AS target
                WHERE target.company_id=NEW.company_id AND target.invocation_id=NEW.invocation_id)<>action.destination_count
        THEN RAISE EXCEPTION 'workflow message dispatch requires its linkage'; END IF;
        has_delivery:=EXISTS(SELECT 1 FROM workflow_message_destinations AS target
            WHERE target.company_id=NEW.company_id AND target.invocation_id=NEW.invocation_id
                AND target.delivery_id IS NOT NULL);
        IF action.wait_id IS NOT NULL THEN
            IF NOT has_delivery OR action.origin<>'step' OR action.fence_expires_at<=clock_timestamp()
                OR NOT EXISTS (
                    SELECT 1 FROM workflow_runs AS run
                    JOIN workflow_waits AS wait ON wait.company_id=run.company_id AND wait.run_id=run.id
                    JOIN background_tasks AS job ON job.company_id=run.company_id AND job.id=NEW.job_id
                    JOIN task_attempts AS attempt ON attempt.task_id=job.id AND attempt.attempt_number=NEW.attempt_number
                    WHERE run.company_id=NEW.company_id AND run.id=NEW.run_id
                      AND run.state='waiting' AND run.waiting_reason='effect' AND run.deadline>clock_timestamp()
                      AND wait.id=action.wait_id AND wait.execution_id=NEW.execution_id
                      AND wait.reason='effect' AND wait.state='waiting' AND wait.deadline=run.deadline
                      AND job.queue_kind='workflow' AND job.status<>'processing'
                      AND attempt.worker_id=NEW.worker_id AND attempt.execution_generation=NEW.execution_generation
                      AND attempt.status='completed' AND attempt.stop_reason='effect_parked')
            THEN RAISE EXCEPTION 'workflow message park lost its fence'; END IF;
            RETURN NEW;
        END IF;
        IF action.origin='step' AND has_delivery THEN
            RAISE EXCEPTION 'workflow message step with deliveries must park';
        END IF;
        IF NOT has_delivery AND NOT EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt
            WHERE receipt.company_id=NEW.company_id AND receipt.invocation_id=NEW.invocation_id
              AND receipt.dispatch_id=NEW.id)
        THEN RAISE EXCEPTION 'in-app workflow message requires atomic receipt'; END IF;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM workflow_runs AS run
        JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=job.id
        WHERE run.company_id=NEW.company_id AND run.id=NEW.run_id
          AND run.state IN ('queued','running') AND run.deadline>clock_timestamp()
          AND execution.id=NEW.execution_id AND execution.completed_at IS NULL
          AND job.id=NEW.job_id AND job.queue_kind='workflow' AND job.status='processing'
          AND job.worker_id=NEW.worker_id AND job.execution_generation=NEW.execution_generation
          AND job.retry_count+1=NEW.attempt_number AND job.lock_expires_at>clock_timestamp()
          AND attempt.attempt_number=NEW.attempt_number AND attempt.worker_id=NEW.worker_id
          AND attempt.execution_generation=NEW.execution_generation AND attempt.status='processing'
    ) THEN RAISE EXCEPTION 'workflow action dispatch lost live fence'; END IF;
    IF NEW.effect_kind='local' AND NOT EXISTS (
        SELECT 1 FROM workflow_action_receipts AS receipt
        WHERE receipt.company_id=NEW.company_id AND receipt.invocation_id=NEW.invocation_id
          AND receipt.dispatch_id=NEW.id
    ) THEN RAISE EXCEPTION 'local workflow action requires atomic receipt'; END IF;
    RETURN NEW;
END
$$;

-- 11b. Fenced completion: a message step parked in its effect wait closes its job and attempt
-- (EffectParked) without completing the execution (D2 3, D3 B). Every other close is unchanged.
CREATE OR REPLACE FUNCTION check_workflow_fenced_completion() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.queue_kind = 'workflow' AND OLD.status = 'processing' AND NEW.status = 'completed' THEN
        IF OLD.lock_expires_at IS NULL OR OLD.lock_expires_at <= clock_timestamp()
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id = execution.company_id AND run.id = execution.run_id
                JOIN task_attempts AS attempt ON attempt.task_id = NEW.id
                    AND attempt.attempt_number = OLD.retry_count + 1
                    AND attempt.worker_id = OLD.worker_id
                    AND attempt.execution_generation = OLD.execution_generation
                WHERE execution.company_id = NEW.company_id AND execution.id = NEW.workflow_execution_id
                    AND attempt.status = 'completed' AND run.deadline > clock_timestamp()
                    AND ((execution.completed_at IS NOT NULL AND execution.committed_route IS NOT NULL)
                        OR (attempt.stop_reason = 'effect_parked' AND execution.completed_at IS NULL
                            AND run.state = 'waiting' AND run.waiting_reason = 'effect'
                            AND EXISTS (SELECT 1 FROM workflow_waits AS wait
                                JOIN workflow_message_actions AS action ON action.company_id = wait.company_id
                                    AND action.execution_id = wait.execution_id AND action.wait_id = wait.id
                                WHERE wait.company_id = execution.company_id AND wait.execution_id = execution.id
                                    AND wait.reason = 'effect' AND wait.state = 'waiting')))
            ) THEN
            RAISE EXCEPTION 'workflow completion lost ownership before commit' USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NULL;
END $$;

-- 12. Delivery provenance: delivered/dead_letter are final; outcome_unknown exits only by
-- evidence; a reopened part re-enters unknown only by the expired-proof refusal (D3 C, D).
CREATE FUNCTION workflow_message_delivery_transition_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE evidence workflow_action_evidence%ROWTYPE; reopen workflow_message_delivery_reopens%ROWTYPE;
BEGIN
    IF NOT EXISTS(SELECT 1 FROM workflow_message_destinations AS target
        WHERE target.company_id=NEW.company_id AND target.delivery_id=NEW.id) THEN RETURN NEW; END IF;
    IF OLD.status IN ('delivered', 'dead_letter') THEN
        RAISE EXCEPTION 'workflow message delivery outcome is final' USING ERRCODE='23514';
    END IF;
    IF OLD.status<>'outcome_unknown' THEN RETURN NEW; END IF;
    IF NEW.status IN ('delivered', 'dead_letter') THEN
        SELECT * INTO evidence FROM workflow_action_evidence
            WHERE company_id=NEW.company_id AND id=NEW.settled_evidence_id AND delivery_id=NEW.id;
        IF NOT FOUND OR (NEW.status='delivered' AND evidence.disposition<>'applied')
            OR (NEW.status='dead_letter' AND evidence.disposition<>'final_not_applied')
        THEN RAISE EXCEPTION 'workflow message delivery leaves unknown only by evidence' USING ERRCODE='23514'; END IF;
        RETURN NEW;
    END IF;
    IF NEW.status='retryable' THEN
        SELECT * INTO reopen FROM workflow_message_delivery_reopens
            WHERE company_id=NEW.company_id AND id=NEW.reopen_id AND delivery_id=NEW.id;
        IF NOT FOUND OR reopen.attempt_count<>OLD.attempt_count THEN
            RAISE EXCEPTION 'workflow message delivery reopens only by consumed proof' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'workflow message delivery leaves unknown only by evidence' USING ERRCODE='23514';
END $$;
CREATE TRIGGER workflow_message_delivery_transition_guard BEFORE UPDATE OF status ON message_deliveries
    FOR EACH ROW WHEN (OLD.status IS DISTINCT FROM NEW.status)
    EXECUTE FUNCTION workflow_message_delivery_transition_guard();

CREATE FUNCTION workflow_message_part_transition_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE parent message_deliveries%ROWTYPE; reopen workflow_message_delivery_reopens%ROWTYPE;
BEGIN
    IF NOT EXISTS(SELECT 1 FROM workflow_message_destinations AS target
        WHERE target.company_id=NEW.company_id AND target.delivery_id=NEW.delivery_id) THEN RETURN NEW; END IF;
    SELECT * INTO parent FROM message_deliveries WHERE id=NEW.delivery_id;
    IF OLD.status IN ('delivered', 'dead') THEN
        RAISE EXCEPTION 'workflow message part outcome is final' USING ERRCODE='23514';
    END IF;
    IF OLD.status='outcome_unknown' THEN
        IF (NEW.status='delivered' AND parent.settled_evidence_id IS NOT NULL AND parent.status='delivered')
            OR (NEW.status='dead' AND parent.settled_evidence_id IS NOT NULL AND parent.status='dead_letter')
            OR (NEW.status='retryable' AND parent.reopen_id IS NOT NULL AND parent.status='retryable')
        THEN RETURN NEW; END IF;
        RAISE EXCEPTION 'workflow message part leaves unknown only by evidence' USING ERRCODE='23514';
    END IF;
    IF OLD.status='retryable' AND NEW.status='outcome_unknown' THEN
        SELECT * INTO reopen FROM workflow_message_delivery_reopens
            WHERE company_id=parent.company_id AND id=parent.reopen_id;
        IF NOT FOUND OR NEW.last_error_class IS DISTINCT FROM 'reconciliation_proof_expired'
            OR (OLD.request_started_at IS NOT NULL AND OLD.request_started_at>=reopen.created_at)
        THEN RAISE EXCEPTION 'workflow message part re-enters unknown only by expired proof' USING ERRCODE='23514'; END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_message_part_transition_guard BEFORE UPDATE OF status ON message_delivery_parts
    FOR EACH ROW WHEN (OLD.status IS DISTINCT FROM NEW.status)
    EXECUTE FUNCTION workflow_message_part_transition_guard();

CREATE FUNCTION workflow_message_delivery_event() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO workflow_message_delivery_events
        (company_id, run_id, invocation_id, ordinal, delivery_id, old_status, status, failure_class)
    SELECT target.company_id, target.run_id, target.invocation_id, target.ordinal, target.delivery_id,
        OLD.status, NEW.status, NEW.last_error_class
    FROM workflow_message_destinations AS target
    WHERE target.company_id=NEW.company_id AND target.delivery_id=NEW.id;
    RETURN NULL;
END $$;
CREATE TRIGGER workflow_message_delivery_event AFTER UPDATE OF status ON message_deliveries
    FOR EACH ROW WHEN (OLD.status IS DISTINCT FROM NEW.status
        AND (NEW.status IN ('delivered', 'dead_letter', 'outcome_unknown') OR OLD.status='outcome_unknown'))
    EXECUTE FUNCTION workflow_message_delivery_event();

-- 13. Reopen guard: one-use, unexpired, final, conflict-free proof for an unknown delivery (D4 O).
CREATE FUNCTION workflow_message_reopen_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE evidence workflow_action_evidence%ROWTYPE;
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    SELECT * INTO evidence FROM workflow_action_evidence WHERE company_id=NEW.company_id AND id=NEW.evidence_id;
    IF NOT FOUND OR evidence.disposition<>'final_not_applied' OR NOT evidence.grant_eligible
        OR evidence.registration IS NULL OR evidence.valid_until<=clock_timestamp()
        OR evidence.invocation_id<>NEW.invocation_id OR evidence.delivery_id IS DISTINCT FROM NEW.delivery_id
        OR evidence.delivery_ordinal IS DISTINCT FROM NEW.ordinal OR NEW.valid_until<>evidence.valid_until
        OR workflow_action_has_evidence_conflict(NEW.company_id, NEW.execution_id)
        OR NOT EXISTS(SELECT 1 FROM message_deliveries AS delivery WHERE delivery.company_id=NEW.company_id
            AND delivery.id=NEW.delivery_id AND delivery.status='outcome_unknown'
            AND delivery.attempt_count=NEW.attempt_count)
    THEN RAISE EXCEPTION 'workflow message reopen requires one unexpired final proof' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_message_reopen_guard BEFORE INSERT ON workflow_message_delivery_reopens
    FOR EACH ROW EXECUTE FUNCTION workflow_message_reopen_guard();

-- 14. Evidence provenance: remote branch unchanged; message evidence names one delivery subject.
CREATE OR REPLACE FUNCTION workflow_action_evidence_commit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE marker workflow_action_dispatches%ROWTYPE; started timestamptz;
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id;
    IF NEW.verified_at>clock_timestamp()
        OR (NEW.registration IS NOT NULL AND NEW.valid_until<=clock_timestamp())
        OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
            WHERE command.company_id=NEW.company_id AND command.command_key=NEW.command_key
            AND command.evidence_id=NEW.id AND command.actor_id=NEW.actor_id
            AND command.id=NEW.command_id AND command.request_digest=NEW.request_digest
            AND command.run_id=NEW.run_id AND command.execution_id=NEW.execution_id
            AND command.invocation_id=NEW.invocation_id AND command.argument_digest=NEW.argument_digest
            AND command.dispatch_id=NEW.dispatch_id)
    THEN RAISE EXCEPTION 'invalid workflow action evidence provenance' USING ERRCODE='23514'; END IF;
    IF marker.effect_kind='message' THEN
        SELECT part.request_started_at INTO started FROM message_delivery_parts AS part
            WHERE part.delivery_id=NEW.delivery_id ORDER BY part.part_index LIMIT 1;
        IF NEW.delivery_id IS NULL OR NEW.observed_at<marker.created_at
            OR (NEW.disposition='applied') <> (NEW.applied_request IS NOT DISTINCT FROM 'delivery')
            OR EXISTS(SELECT 1 FROM workflow_action_evidence_coverage WHERE company_id=NEW.company_id AND evidence_id=NEW.id)
            OR started IS NULL OR NEW.observed_at<started
        THEN RAISE EXCEPTION 'invalid workflow message evidence provenance' USING ERRCODE='23514'; END IF;
        RETURN NULL;
    END IF;
    IF marker.effect_kind<>'remote' OR NEW.observed_at<marker.created_at
        OR NEW.delivery_id IS NOT NULL OR NEW.applied_request IS NOT DISTINCT FROM 'delivery'
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

-- 15. First-uncertainty observations: message markers may be recorded in any run state.
CREATE OR REPLACE FUNCTION workflow_action_reconciliation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE owner workflow_runs%ROWTYPE;
BEGIN
    -- Serialize with receipt insertion and terminal transitions using the existing run lock.
    SELECT * INTO owner FROM workflow_runs
        WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF NOT FOUND OR (NEW.effect_kind='remote' AND NOT (owner.state IN ('failed','cancelled')
            OR (owner.state='waiting' AND owner.waiting_reason='reconciliation')))
        OR EXISTS (SELECT 1 FROM workflow_action_receipts AS receipt
            WHERE receipt.company_id=NEW.company_id AND receipt.invocation_id=NEW.invocation_id)
    THEN
        RAISE EXCEPTION 'workflow reconciliation requires parked or terminal unresolved effect' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;

-- 16. Projection: message markers derive from the action state ahead of generic evidence (D4 P).
CREATE OR REPLACE VIEW workflow_action_effect_states AS
SELECT intent.company_id,intent.run_id,intent.execution_id,intent.id AS invocation_id,intent.argument_digest,
    CASE WHEN receipt.invocation_id IS NOT NULL THEN 'committed'
        WHEN marker.effect_kind='message' THEN
            CASE WHEN message_action.state='pending' THEN 'possible_dispatch' ELSE 'needs_reconciliation' END
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
LEFT JOIN workflow_action_reconciliations AS reconciliation ON reconciliation.company_id=intent.company_id AND reconciliation.invocation_id=intent.id
LEFT JOIN workflow_message_actions AS message_action ON message_action.company_id=intent.company_id AND message_action.invocation_id=intent.id;
