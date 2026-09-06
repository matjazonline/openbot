-- Business ownership is independent of the worker process that temporarily leases a task.
-- `owner_principal_kind` is deliberately stored beside the principal id: it lets a composite
-- foreign key prove both tenant membership and the only two principal kinds eligible to own work.
ALTER TABLE principals
    ADD CONSTRAINT principals_company_id_id_kind_key UNIQUE (company_id, id, kind);

ALTER TABLE background_tasks
    ADD COLUMN owner_principal_id UUID,
    ADD COLUMN owner_principal_kind TEXT,
    ADD COLUMN ownership_version BIGINT NOT NULL DEFAULT 1,
    ADD CONSTRAINT background_tasks_owner_shape_check CHECK (
        (owner_principal_id IS NULL AND owner_principal_kind IS NULL)
        OR
        (owner_principal_id IS NOT NULL AND owner_principal_kind IN ('person', 'agent'))
    ),
    ADD CONSTRAINT background_tasks_ownership_version_check CHECK (ownership_version > 0),
    ADD CONSTRAINT background_tasks_owner_principal_fk
        FOREIGN KEY (company_id, owner_principal_id, owner_principal_kind)
        REFERENCES principals(company_id, id, kind) ON DELETE RESTRICT;

ALTER TABLE task_attempts
    DROP CONSTRAINT task_attempts_stop_reason_check,
    ADD CONSTRAINT task_attempts_stop_reason_check CHECK (stop_reason IN (
        'completed', 'retryable_failure', 'terminal_failure',
        'timed_out', 'shutdown', 'lease_lost', 'ownership_transferred'
    ));

ALTER TABLE background_tasks
    DROP CONSTRAINT background_tasks_transition_reason_check,
    ADD CONSTRAINT background_tasks_transition_reason_check CHECK (
        transition_reason IS NULL OR transition_reason IN (
            'enqueued', 'claimed', 'completed', 'retryable_failure', 'terminal_failure',
            'timed_out', 'shutdown', 'lease_lost', 'approval_requested', 'approval_accepted',
            'approval_rejected', 'outreach_started', 'outreach_reply_received',
            'outreach_timed_out', 'outreach_extended', 'operator_stopped', 'operator_resumed',
            'ownership_transferred', 'unknown'
        )
    );

ALTER TABLE background_tasks
    DROP CONSTRAINT background_tasks_transition_actor_kind_check,
    DROP CONSTRAINT background_tasks_transition_shape_check,
    ADD CONSTRAINT background_tasks_transition_actor_kind_check CHECK (
        transition_actor_kind IS NULL OR transition_actor_kind IN (
            'system', 'worker', 'operator', 'human', 'agent', 'approval', 'outreach'
        )
    ),
    ADD CONSTRAINT background_tasks_transition_shape_check CHECK (
        (transition_reason IS NULL
         AND transition_actor_kind IS NULL
         AND transition_actor_id IS NULL
         AND transition_approval_id IS NULL
         AND transition_outreach_id IS NULL)
        OR
        (transition_reason IS NOT NULL
         AND CASE transition_actor_kind
             WHEN 'system' THEN transition_actor_id IS NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NULL
             WHEN 'worker' THEN transition_actor_id IS NOT NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NULL
             WHEN 'operator' THEN transition_actor_id IS NOT NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NULL
             WHEN 'human' THEN transition_actor_id IS NOT NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NULL
             WHEN 'agent' THEN transition_actor_id IS NOT NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NULL
             WHEN 'approval' THEN transition_actor_id IS NULL
                 AND transition_approval_id IS NOT NULL AND transition_outreach_id IS NULL
             WHEN 'outreach' THEN transition_actor_id IS NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NOT NULL
             ELSE FALSE
         END)
    );

ALTER TABLE task_status_events
    DROP CONSTRAINT task_status_events_reason_check,
    DROP CONSTRAINT task_status_events_actor_kind_check,
    ADD CONSTRAINT task_status_events_reason_check CHECK (reason IN (
        'enqueued', 'claimed', 'completed', 'retryable_failure', 'terminal_failure',
        'timed_out', 'shutdown', 'lease_lost', 'approval_requested', 'approval_accepted',
        'approval_rejected', 'outreach_started', 'outreach_reply_received',
        'outreach_timed_out', 'outreach_extended', 'operator_stopped', 'operator_resumed',
        'ownership_transferred', 'unknown'
    )),
    ADD CONSTRAINT task_status_events_actor_kind_check CHECK (actor_kind IN (
        'system', 'worker', 'operator', 'human', 'agent', 'approval', 'outreach'
    ));

-- Existing work belongs to the position-zero agent on its primary channel. A disabled or
-- otherwise unresolved channel is intentionally left unassigned instead of manufacturing an
-- owner that could not execute it.
UPDATE background_tasks AS task
SET owner_principal_id = (
    SELECT principal.id AS principal_id
    FROM channel_agents AS assignment
    JOIN agents AS agent
      ON agent.id = assignment.agent_id AND agent.company_id = task.company_id
    JOIN principals AS principal
      ON principal.company_id = task.company_id
     AND principal.agent_id = agent.id
     AND principal.kind = 'agent'
    WHERE assignment.company_id = task.company_id
      AND assignment.channel_id = task.channel_id
      AND assignment.position = 0
    LIMIT 1
);

UPDATE background_tasks
SET owner_principal_kind = CASE WHEN owner_principal_id IS NULL THEN NULL ELSE 'agent' END;

CREATE TABLE task_ownership_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL,
    company_id UUID NOT NULL,
    sequence BIGINT NOT NULL,
    from_version BIGINT NOT NULL,
    to_version BIGINT NOT NULL,
    command_id UUID NOT NULL,
    -- A digest of every semantic command field. It distinguishes a harmless retry from reuse of
    -- an idempotency key for a different operation without retaining an operational payload.
    command_fingerprint TEXT NOT NULL,
    operation TEXT NOT NULL,
    actor_principal_id UUID,
    actor_kind TEXT NOT NULL,
    previous_owner_principal_id UUID,
    previous_owner_kind TEXT,
    previous_owner_label TEXT,
    new_owner_principal_id UUID,
    new_owner_kind TEXT,
    new_owner_label TEXT,
    reason TEXT NOT NULL,
    reason_detail TEXT,
    handoff_instruction TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT task_ownership_events_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE,
    CONSTRAINT task_ownership_events_task_sequence_key UNIQUE (task_id, sequence),
    CONSTRAINT task_ownership_events_task_command_key UNIQUE (task_id, command_id),
    CONSTRAINT task_ownership_events_version_check CHECK (
        from_version >= 0 AND to_version = from_version + 1 AND sequence = to_version
    ),
    CONSTRAINT task_ownership_events_operation_check CHECK (
        operation IN ('initial_assignment', 'claim', 'assign', 'transfer', 'release', 'owner_removed')
    ),
    CONSTRAINT task_ownership_events_actor_kind_check CHECK (
        actor_kind IN ('system', 'human', 'agent')
    ),
    CONSTRAINT task_ownership_events_previous_owner_check CHECK (
        (previous_owner_principal_id IS NULL AND previous_owner_kind = 'unassigned')
        OR
        (previous_owner_principal_id IS NOT NULL AND previous_owner_kind IN ('human', 'agent'))
    ),
    CONSTRAINT task_ownership_events_new_owner_check CHECK (
        (new_owner_principal_id IS NULL AND new_owner_kind = 'unassigned')
        OR
        (new_owner_principal_id IS NOT NULL AND new_owner_kind IN ('human', 'agent'))
    ),
    CONSTRAINT task_ownership_events_reason_check CHECK (
        reason IN ('initial_assignment', 'self_claim', 'manual_assignment', 'delegated',
                   'workload_rebalance', 'owner_unavailable', 'released', 'owner_removed')
    ),
    CONSTRAINT task_ownership_events_reason_detail_check CHECK (
        reason_detail IS NULL OR octet_length(reason_detail) <= 512
    ),
    CONSTRAINT task_ownership_events_handoff_check CHECK (
        handoff_instruction IS NULL OR (
            btrim(handoff_instruction) <> '' AND octet_length(handoff_instruction) <= 8192
        )
    ),
    CONSTRAINT task_ownership_events_transfer_handoff_check CHECK (
        (operation = 'transfer' AND handoff_instruction IS NOT NULL)
        OR (operation <> 'transfer' AND handoff_instruction IS NULL)
    )
);

-- Backfill an origin event for every row, including deliberately unassigned work. Snapshot labels
-- are copied now so a later rename or removal cannot rewrite history.
INSERT INTO task_ownership_events (
    task_id, company_id, sequence, from_version, to_version, command_id, command_fingerprint,
    operation, actor_kind, previous_owner_kind, new_owner_principal_id, new_owner_kind,
    new_owner_label, reason
)
SELECT task.id, task.company_id, 1, 0, 1, gen_random_uuid(),
       'migration:' || task.id::text, 'initial_assignment', 'system', 'unassigned',
       task.owner_principal_id,
       CASE task.owner_principal_kind WHEN 'person' THEN 'human'
            WHEN 'agent' THEN 'agent' ELSE 'unassigned' END,
       principal.display_label, 'initial_assignment'
FROM background_tasks AS task
LEFT JOIN principals AS principal
  ON principal.company_id = task.company_id AND principal.id = task.owner_principal_id;

-- New tasks resolve their initial owner inside the insert transaction. This also covers future
-- producers: no enqueue path can forget the invariant or the origin event.
CREATE FUNCTION initialize_task_ownership() RETURNS TRIGGER AS $$
DECLARE
    resolved RECORD;
BEGIN
    IF NEW.owner_principal_id IS NULL THEN
        SELECT principal.id, principal.display_label
          INTO resolved
          FROM channel_agents AS assignment
          JOIN agents AS agent
            ON agent.id = assignment.agent_id AND agent.company_id = NEW.company_id
          JOIN principals AS principal
            ON principal.company_id = NEW.company_id
           AND principal.agent_id = agent.id
           AND principal.kind = 'agent'
         WHERE assignment.company_id = NEW.company_id
           AND assignment.channel_id = NEW.channel_id
           AND assignment.position = 0
         LIMIT 1;
        NEW.owner_principal_id := resolved.id;
        NEW.owner_principal_kind := CASE WHEN resolved.id IS NULL THEN NULL ELSE 'agent' END;
    END IF;
    NEW.ownership_version := 1;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER background_tasks_initialize_ownership
BEFORE INSERT ON background_tasks
FOR EACH ROW EXECUTE FUNCTION initialize_task_ownership();

CREATE FUNCTION record_initial_task_ownership() RETURNS TRIGGER AS $$
DECLARE
    label TEXT;
BEGIN
    SELECT display_label INTO label
      FROM principals
     WHERE company_id = NEW.company_id AND id = NEW.owner_principal_id;
    INSERT INTO task_ownership_events (
        task_id, company_id, sequence, from_version, to_version, command_id,
        command_fingerprint, operation, actor_kind, previous_owner_kind,
        new_owner_principal_id, new_owner_kind, new_owner_label, reason
    ) VALUES (
        NEW.id, NEW.company_id, 1, 0, 1, gen_random_uuid(), 'enqueue:' || NEW.id::text,
        'initial_assignment', 'system', 'unassigned', NEW.owner_principal_id,
        CASE NEW.owner_principal_kind WHEN 'person' THEN 'human'
             WHEN 'agent' THEN 'agent' ELSE 'unassigned' END,
        label, 'initial_assignment'
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER background_tasks_record_initial_ownership
AFTER INSERT ON background_tasks
FOR EACH ROW EXECUTE FUNCTION record_initial_task_ownership();

CREATE FUNCTION task_ownership_events_are_immutable() RETURNS TRIGGER AS $$
BEGIN
    -- Tenant/task deletion may remove the ledger with its aggregate root. Direct mutation while
    -- the task exists is forbidden. A principal deletion never changes snapshot rows.
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM background_tasks WHERE id = OLD.task_id AND company_id = OLD.company_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'task ownership events are immutable' USING ERRCODE = '55000';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_ownership_events_immutable
BEFORE UPDATE OR DELETE ON task_ownership_events
FOR EACH ROW EXECUTE FUNCTION task_ownership_events_are_immutable();

-- Principal removal cannot bypass ownership cleanup. Active work is released and a running lease
-- is revoked; terminal work only clears the live reference. The immutable snapshot remains.
CREATE FUNCTION release_tasks_for_removed_principal() RETURNS TRIGGER AS $$
DECLARE
    owned RECORD;
BEGIN
    FOR owned IN
        SELECT task.*, OLD.display_label AS old_label
          FROM background_tasks AS task
         WHERE task.company_id = OLD.company_id AND task.owner_principal_id = OLD.id
         FOR UPDATE
    LOOP
        UPDATE background_tasks
           SET owner_principal_id = NULL,
               owner_principal_kind = NULL,
               ownership_version = ownership_version + 1,
               status = CASE WHEN status = 'processing' THEN 'pending' ELSE status END,
               worker_id = NULL,
               execution_generation = NULL,
               locked_at = NULL,
               lock_expires_at = NULL,
               run_at = CASE WHEN status = 'processing' THEN CURRENT_TIMESTAMP ELSE run_at END,
               transition_reason = CASE
                   WHEN status = 'processing' THEN 'ownership_transferred'
                   ELSE transition_reason
               END,
               transition_actor_kind = CASE
                   WHEN status = 'processing' THEN 'system'
                   ELSE transition_actor_kind
               END,
               transition_actor_id = CASE
                   WHEN status = 'processing' THEN NULL
                   ELSE transition_actor_id
               END,
               updated_at = CURRENT_TIMESTAMP
         WHERE id = owned.id;

        IF owned.status = 'processing' THEN
            UPDATE task_attempts
               SET status = 'failed', stop_reason = 'ownership_transferred',
                   error = 'Task ownership was removed', finished_at = CURRENT_TIMESTAMP
             WHERE task_id = owned.id
               AND execution_generation = owned.execution_generation
               AND status = 'processing';
        END IF;

        INSERT INTO task_ownership_events (
            task_id, company_id, sequence, from_version, to_version, command_id,
            command_fingerprint, operation, actor_kind, previous_owner_principal_id,
            previous_owner_kind, previous_owner_label, new_owner_kind, reason
        ) VALUES (
            owned.id, owned.company_id, owned.ownership_version + 1,
            owned.ownership_version, owned.ownership_version + 1, gen_random_uuid(),
            'owner-removed:' || OLD.id::text || ':' || owned.ownership_version::text,
            'owner_removed', 'system', OLD.id,
            CASE OLD.kind WHEN 'person' THEN 'human' ELSE 'agent' END,
            owned.old_label, 'unassigned', 'owner_removed'
        );
    END LOOP;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER principals_release_owned_tasks
BEFORE DELETE ON principals
FOR EACH ROW WHEN (OLD.kind IN ('person', 'agent'))
EXECUTE FUNCTION release_tasks_for_removed_principal();

-- Ownership changes are mailbox/task reconciliation wake-ups. Only identifiers leave Postgres;
-- viewers re-read scoped state and handoff text never enters the notification payload.
CREATE FUNCTION notify_task_ownership() RETURNS TRIGGER AS $$
DECLARE
    affected_thread UUID;
    affected_channel UUID;
    affected_correlation UUID;
    affected_status TEXT;
    affected_owner_kind TEXT;
BEGIN
    SELECT thread_id, channel_id, correlation_id, status, owner_principal_kind
      INTO affected_thread, affected_channel, affected_correlation, affected_status,
           affected_owner_kind
      FROM background_tasks WHERE id = NEW.task_id;

    IF affected_thread IS NOT NULL THEN
        PERFORM pg_notify('thread_activity', json_build_object(
            'thread_id', affected_thread,
            'channel_id', affected_channel,
            'company_id', NEW.company_id
        )::text);
    END IF;
    IF affected_correlation IS NOT NULL THEN
        PERFORM pg_notify('task_chain_changed', json_build_object(
            'company_id', NEW.company_id,
            'correlation_id', affected_correlation
        )::text);
    END IF;
    PERFORM pg_notify('task_ownership_changed', json_build_object(
        'task_id', NEW.task_id
    )::text);
    IF affected_status = 'pending' AND affected_owner_kind = 'agent' THEN
        PERFORM pg_notify('task_ready', json_build_object(
            'task_id', NEW.task_id
        )::text);
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_ownership_events_notify
AFTER INSERT ON task_ownership_events
FOR EACH ROW EXECUTE FUNCTION notify_task_ownership();

CREATE TABLE user_notification_preferences (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    task_assignment_email_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE human_task_completions (
    task_id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    command_id UUID NOT NULL,
    command_fingerprint TEXT NOT NULL,
    owner_principal_id UUID NOT NULL,
    ownership_version BIGINT NOT NULL CHECK (ownership_version > 0),
    message_id UUID NOT NULL,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT human_task_completions_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE,
    CONSTRAINT human_task_completions_message_fk
        FOREIGN KEY (company_id, message_id)
        REFERENCES messages(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT human_task_completions_task_command_key UNIQUE (task_id, command_id)
);
