-- First-class, immutable private notes. The body remains the canonical message payload; this
-- ledger records creation provenance and correction/removal history without rewriting it.
CREATE TABLE internal_notes (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    message_id UUID NOT NULL,
    message_audience TEXT NOT NULL DEFAULT 'internal_only'
        CHECK (message_audience = 'internal_only'),
    command_id UUID NOT NULL,
    command_fingerprint TEXT NOT NULL,
    author_principal_id UUID NOT NULL,
    provenance TEXT NOT NULL,
    supersedes_note_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT internal_notes_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT internal_notes_company_command_key UNIQUE (company_id, command_id),
    CONSTRAINT internal_notes_message_key UNIQUE (company_id, message_id),
    CONSTRAINT internal_notes_supersedes_key UNIQUE (supersedes_note_id),
    CONSTRAINT internal_notes_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE,
    CONSTRAINT internal_notes_message_fk
        FOREIGN KEY (company_id, message_id, message_audience)
        REFERENCES messages(company_id, id, audience) ON DELETE RESTRICT,
    CONSTRAINT internal_notes_author_fk
        FOREIGN KEY (company_id, author_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT internal_notes_supersedes_fk
        FOREIGN KEY (company_id, supersedes_note_id)
        REFERENCES internal_notes(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT internal_notes_no_self_supersession CHECK (id IS DISTINCT FROM supersedes_note_id),
    CONSTRAINT internal_notes_provenance_check CHECK (
        provenance IN ('human_ui', 'api', 'integration', 'email_quiet_ingress')
    )
);

CREATE TABLE internal_note_tombstones (
    note_id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    command_id UUID NOT NULL,
    command_fingerprint TEXT NOT NULL,
    actor_principal_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT internal_note_tombstones_company_command_key UNIQUE (company_id, command_id),
    CONSTRAINT internal_note_tombstones_note_fk
        FOREIGN KEY (company_id, note_id)
        REFERENCES internal_notes(company_id, id) ON DELETE CASCADE,
    CONSTRAINT internal_note_tombstones_actor_fk
        FOREIGN KEY (company_id, actor_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT
);

CREATE TABLE task_agent_instructions (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    task_id UUID NOT NULL,
    command_id UUID NOT NULL,
    command_fingerprint TEXT NOT NULL,
    requested_by_principal_id UUID NOT NULL,
    requested_ownership_version BIGINT NOT NULL CHECK (requested_ownership_version > 0),
    wake_outcome TEXT NOT NULL CHECK (wake_outcome IN ('queued', 'requeued', 'parked')),
    consumed_execution_generation UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    consumed_at TIMESTAMPTZ,
    CONSTRAINT task_agent_instructions_company_command_key UNIQUE (company_id, command_id),
    CONSTRAINT task_agent_instructions_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE,
    CONSTRAINT task_agent_instructions_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE,
    CONSTRAINT task_agent_instructions_actor_fk
        FOREIGN KEY (company_id, requested_by_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT task_agent_instructions_consumption_check CHECK (
        (consumed_execution_generation IS NULL AND consumed_at IS NULL)
        OR (consumed_execution_generation IS NOT NULL AND consumed_at IS NOT NULL)
    )
);

CREATE TABLE task_agent_instruction_notes (
    instruction_id UUID NOT NULL REFERENCES task_agent_instructions(id) ON DELETE CASCADE,
    company_id UUID NOT NULL,
    note_id UUID NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0 AND position < 50),
    PRIMARY KEY (instruction_id, note_id),
    CONSTRAINT task_agent_instruction_notes_position_key UNIQUE (instruction_id, position),
    CONSTRAINT task_agent_instruction_notes_note_fk
        FOREIGN KEY (company_id, note_id)
        REFERENCES internal_notes(company_id, id) ON DELETE RESTRICT
);

CREATE TABLE start_agent_task_commands (
    company_id UUID NOT NULL,
    command_id UUID NOT NULL,
    command_fingerprint TEXT NOT NULL,
    task_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, command_id),
    CONSTRAINT start_agent_task_commands_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE
);

ALTER TABLE task_attempts
    DROP CONSTRAINT task_attempts_stop_reason_check,
    ADD CONSTRAINT task_attempts_stop_reason_check CHECK (stop_reason IN (
        'completed', 'retryable_failure', 'terminal_failure', 'timed_out', 'shutdown',
        'lease_lost', 'ownership_transferred', 'agent_instruction'
    ));

ALTER TABLE background_tasks
    DROP CONSTRAINT background_tasks_transition_reason_check,
    ADD CONSTRAINT background_tasks_transition_reason_check CHECK (
        transition_reason IS NULL OR transition_reason IN (
            'enqueued', 'claimed', 'completed', 'retryable_failure', 'terminal_failure',
            'timed_out', 'shutdown', 'lease_lost', 'approval_requested', 'approval_accepted',
            'approval_rejected', 'outreach_started', 'outreach_reply_received',
            'outreach_timed_out', 'outreach_extended', 'operator_stopped', 'operator_resumed',
            'ownership_transferred', 'agent_instruction', 'unknown'
        )
    );

ALTER TABLE task_status_events
    DROP CONSTRAINT task_status_events_reason_check,
    ADD CONSTRAINT task_status_events_reason_check CHECK (reason IN (
        'enqueued', 'claimed', 'completed', 'retryable_failure', 'terminal_failure',
        'timed_out', 'shutdown', 'lease_lost', 'approval_requested', 'approval_accepted',
        'approval_rejected', 'outreach_started', 'outreach_reply_received',
        'outreach_timed_out', 'outreach_extended', 'operator_stopped', 'operator_resumed',
        'ownership_transferred', 'agent_instruction', 'unknown'
    ));

CREATE FUNCTION internal_notes_are_immutable() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM threads
         WHERE company_id = OLD.company_id AND channel_id = OLD.channel_id AND id = OLD.thread_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'internal note audit rows are immutable' USING ERRCODE = '55000';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER internal_notes_immutable
BEFORE UPDATE OR DELETE ON internal_notes
FOR EACH ROW EXECUTE FUNCTION internal_notes_are_immutable();

CREATE FUNCTION internal_note_tombstones_are_immutable() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM internal_notes WHERE company_id = OLD.company_id AND id = OLD.note_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'internal note tombstones are immutable' USING ERRCODE = '55000';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER internal_note_tombstones_immutable
BEFORE UPDATE OR DELETE ON internal_note_tombstones
FOR EACH ROW EXECUTE FUNCTION internal_note_tombstones_are_immutable();

-- A quiet note wakes readers, never workers. Ask-agent writes change task state when necessary;
-- this notification only tells open mailboxes to re-read the thread.
CREATE FUNCTION notify_internal_note_change() RETURNS TRIGGER AS $$
DECLARE
    note_thread UUID;
    note_channel UUID;
    note_company UUID;
BEGIN
    IF TG_TABLE_NAME = 'internal_notes' THEN
        note_thread := NEW.thread_id;
        note_channel := NEW.channel_id;
        note_company := NEW.company_id;
    ELSE
        SELECT thread_id, channel_id, company_id
          INTO note_thread, note_channel, note_company
          FROM internal_notes WHERE id = NEW.note_id;
    END IF;
    PERFORM pg_notify('thread_messages', json_build_object(
        'thread_id', note_thread, 'channel_id', note_channel, 'company_id', note_company
    )::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER internal_notes_notify
AFTER INSERT ON internal_notes
FOR EACH ROW EXECUTE FUNCTION notify_internal_note_change();

CREATE TRIGGER internal_note_tombstones_notify
AFTER INSERT ON internal_note_tombstones
FOR EACH ROW EXECUTE FUNCTION notify_internal_note_change();

CREATE FUNCTION notify_agent_instruction() RETURNS TRIGGER AS $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM background_tasks
         WHERE id = NEW.task_id AND status = 'pending' AND owner_principal_kind = 'agent'
    ) THEN
        PERFORM pg_notify('task_ready', json_build_object('task_id', NEW.task_id)::text);
    END IF;
    PERFORM pg_notify('thread_activity', json_build_object(
        'thread_id', NEW.thread_id, 'channel_id', NEW.channel_id, 'company_id', NEW.company_id
    )::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_agent_instructions_notify
AFTER INSERT ON task_agent_instructions
FOR EACH ROW EXECUTE FUNCTION notify_agent_instruction();
