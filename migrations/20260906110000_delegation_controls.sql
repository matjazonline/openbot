-- Versioned, auditable recovery controls for delegated work.

ALTER TABLE task_outreaches
    ADD COLUMN version BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN created_by_principal_id UUID,
    ADD COLUMN created_by_principal_kind TEXT,
    ADD CONSTRAINT task_outreaches_version_check CHECK (version > 0),
    ADD CONSTRAINT task_outreaches_creator_shape_check CHECK (
        (created_by_principal_id IS NULL AND created_by_principal_kind IS NULL)
        OR
        (created_by_principal_id IS NOT NULL AND created_by_principal_kind = 'agent')
    ),
    ADD CONSTRAINT task_outreaches_creator_fk
        FOREIGN KEY (company_id, created_by_principal_id, created_by_principal_kind)
        REFERENCES principals(company_id, id, kind) ON DELETE RESTRICT;

ALTER TABLE background_tasks
    DROP CONSTRAINT background_tasks_transition_reason_check,
    ADD CONSTRAINT background_tasks_transition_reason_check CHECK (
        transition_reason IS NULL OR transition_reason IN (
            'enqueued', 'claimed', 'completed', 'retryable_failure', 'terminal_failure',
            'timed_out', 'shutdown', 'lease_lost', 'approval_requested', 'approval_accepted',
            'approval_rejected', 'outreach_started', 'outreach_reply_received',
            'outreach_timed_out', 'outreach_extended', 'operator_stopped', 'operator_resumed',
            'ownership_transferred', 'agent_instruction', 'delegation_target_cancelled',
            'delegation_cancelled', 'delegation_reassigned', 'delegation_partial', 'unknown'
        )
    );

ALTER TABLE task_attempts
    DROP CONSTRAINT task_attempts_stop_reason_check,
    ADD CONSTRAINT task_attempts_stop_reason_check CHECK (stop_reason IN (
        'completed', 'retryable_failure', 'terminal_failure', 'timed_out', 'shutdown',
        'lease_lost', 'ownership_transferred', 'agent_instruction', 'delegation_cancelled'
    ));

ALTER TABLE task_status_events
    DROP CONSTRAINT task_status_events_reason_check,
    ADD CONSTRAINT task_status_events_reason_check CHECK (reason IN (
        'enqueued', 'claimed', 'completed', 'retryable_failure', 'terminal_failure',
        'timed_out', 'shutdown', 'lease_lost', 'approval_requested', 'approval_accepted',
        'approval_rejected', 'outreach_started', 'outreach_reply_received',
        'outreach_timed_out', 'outreach_extended', 'operator_stopped', 'operator_resumed',
        'ownership_transferred', 'agent_instruction', 'delegation_target_cancelled',
        'delegation_cancelled', 'delegation_reassigned', 'delegation_partial', 'unknown'
    ));

ALTER TABLE task_outreach_targets
    ADD COLUMN status TEXT NOT NULL DEFAULT 'active',
    ADD COLUMN replaces_target_id UUID;

UPDATE task_outreach_targets
SET status = 'responded'
WHERE responded_at IS NOT NULL;

ALTER TABLE task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_status_check CHECK (
        status IN ('active', 'responded', 'cancelled', 'superseded', 'expired')
    ),
    ADD CONSTRAINT task_outreach_targets_response_state_check CHECK (
        (status = 'responded' AND responded_at IS NOT NULL AND response_association_id IS NOT NULL)
        OR
        (status <> 'responded' AND responded_at IS NULL AND response_association_id IS NULL)
    ),
    ADD CONSTRAINT task_outreach_targets_replacement_fk
        FOREIGN KEY (company_id, replaces_target_id)
        REFERENCES task_outreach_targets(company_id, id) ON DELETE RESTRICT;

-- Every correlated reply is retained. Only the first eligible reply advances the target; later
-- replies remain immutable internal context and cannot reopen a terminal target or outreach.
CREATE TABLE task_outreach_replies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    outreach_id UUID NOT NULL,
    target_id UUID NOT NULL,
    response_association_id UUID NOT NULL,
    disposition TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT task_outreach_replies_target_fk
        FOREIGN KEY (company_id, target_id)
        REFERENCES task_outreach_targets(company_id, id) ON DELETE CASCADE,
    CONSTRAINT task_outreach_replies_outreach_fk
        FOREIGN KEY (company_id, outreach_id)
        REFERENCES task_outreaches(company_id, id) ON DELETE CASCADE,
    CONSTRAINT task_outreach_replies_response_fk
        FOREIGN KEY (company_id, response_association_id)
        REFERENCES thread_messages(company_id, id) ON DELETE CASCADE,
    CONSTRAINT task_outreach_replies_response_key UNIQUE (response_association_id),
    CONSTRAINT task_outreach_replies_disposition_check CHECK (
        disposition IN ('counted', 'late', 'duplicate')
    )
);

CREATE INDEX task_outreach_replies_target_received_idx
    ON task_outreach_replies (target_id, received_at, id);

-- The result is stored as versioned JSON because each operation returns different consequence
-- data. Its structural version is constrained so a future reader never guesses at an old shape.
CREATE TABLE delegation_control_commands (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    task_id UUID NOT NULL,
    outreach_id UUID NOT NULL,
    target_id UUID,
    command_id UUID NOT NULL,
    command_fingerprint TEXT NOT NULL,
    operation TEXT NOT NULL,
    actor_principal_id UUID NOT NULL,
    actor_kind TEXT NOT NULL,
    authority TEXT NOT NULL,
    reason TEXT NOT NULL,
    reason_detail TEXT,
    from_version BIGINT NOT NULL,
    to_version BIGINT NOT NULL,
    result JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT delegation_control_commands_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE,
    CONSTRAINT delegation_control_commands_outreach_fk
        FOREIGN KEY (company_id, outreach_id)
        REFERENCES task_outreaches(company_id, id) ON DELETE CASCADE,
    CONSTRAINT delegation_control_commands_target_fk
        FOREIGN KEY (company_id, target_id)
        REFERENCES task_outreach_targets(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT delegation_control_commands_actor_fk
        FOREIGN KEY (company_id, actor_principal_id, actor_kind)
        REFERENCES principals(company_id, id, kind) ON DELETE RESTRICT,
    CONSTRAINT delegation_control_commands_task_command_key UNIQUE (task_id, command_id),
    CONSTRAINT delegation_control_commands_version_check CHECK (
        from_version > 0 AND to_version = from_version + 1
    ),
    CONSTRAINT delegation_control_commands_operation_check CHECK (
        operation IN ('extend_outreach', 'cancel_target', 'cancel_outreach',
                      'reassign_internal_target', 'proceed_with_partial', 'stop_task')
    ),
    CONSTRAINT delegation_control_commands_actor_kind_check CHECK (
        actor_kind IN ('person', 'agent')
    ),
    CONSTRAINT delegation_control_commands_authority_check CHECK (
        authority IN ('human_owner', 'company_manager', 'owning_agent')
    ),
    CONSTRAINT delegation_control_commands_reason_check CHECK (
        reason IN ('deadline_changed', 'no_longer_needed', 'target_unavailable',
                   'incorrect_target', 'partial_results_accepted', 'task_stopped', 'other')
    ),
    CONSTRAINT delegation_control_commands_reason_detail_check CHECK (
        reason_detail IS NULL OR octet_length(reason_detail) <= 512
    ),
    CONSTRAINT delegation_control_commands_result_version_check CHECK (
        result->>'version' = '1'
    )
);

CREATE INDEX delegation_control_commands_outreach_idx
    ON delegation_control_commands (outreach_id, occurred_at, id);

-- Database-owned transition matrices keep direct SQL and Rust commands from drifting apart.
CREATE FUNCTION enforce_outreach_status_transition() RETURNS TRIGGER AS $$
BEGIN
    IF OLD.status = NEW.status THEN
        RETURN NEW;
    END IF;
    IF NOT (CASE OLD.status
        WHEN 'waiting' THEN NEW.status IN (
            'threshold_met', 'timeout_pending_approval', 'proceed_partial', 'cancelled'
        )
        WHEN 'timeout_pending_approval' THEN NEW.status IN (
            'waiting', 'threshold_met', 'proceed_partial', 'cancelled'
        )
        WHEN 'threshold_met' THEN NEW.status IN ('completed', 'cancelled')
        WHEN 'proceed_partial' THEN NEW.status IN ('completed', 'cancelled')
        WHEN 'cancelled' THEN FALSE
        WHEN 'completed' THEN FALSE
        ELSE FALSE
    END) THEN
        RAISE EXCEPTION 'invalid outreach status transition: % -> %', OLD.status, NEW.status
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_outreaches_status_transition
BEFORE UPDATE OF status ON task_outreaches
FOR EACH ROW EXECUTE FUNCTION enforce_outreach_status_transition();

CREATE FUNCTION enforce_outreach_target_status_transition() RETURNS TRIGGER AS $$
BEGIN
    IF OLD.status = NEW.status THEN
        RETURN NEW;
    END IF;
    IF OLD.status <> 'active'
       OR NEW.status NOT IN ('responded', 'cancelled', 'superseded', 'expired') THEN
        RAISE EXCEPTION 'invalid outreach target status transition: % -> %', OLD.status, NEW.status
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_outreach_targets_status_transition
BEFORE UPDATE OF status ON task_outreach_targets
FOR EACH ROW EXECUTE FUNCTION enforce_outreach_target_status_transition();
