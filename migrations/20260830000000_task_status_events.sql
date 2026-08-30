-- Immutable, metadata-only history for every background-task status transition.
CREATE TABLE task_status_events (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    task_id UUID NOT NULL,
    correlation_id UUID NOT NULL,
    sequence INTEGER NOT NULL,
    from_status TEXT,
    to_status TEXT NOT NULL,
    reason TEXT NOT NULL,
    actor_kind TEXT NOT NULL,
    actor_id UUID,
    related_approval_id UUID REFERENCES human_approvals(id) ON DELETE SET NULL,
    related_outreach_id UUID REFERENCES task_outreaches(id) ON DELETE SET NULL,
    retry_count INTEGER NOT NULL,
    run_at TIMESTAMPTZ NOT NULL,
    execution_generation UUID,
    transitioned_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT task_status_events_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE,
    CONSTRAINT task_status_events_task_sequence_key UNIQUE (task_id, sequence),
    CONSTRAINT task_status_events_sequence_check CHECK (sequence > 0),
    CONSTRAINT task_status_events_retry_count_check CHECK (retry_count >= 0),
    CONSTRAINT task_status_events_from_status_check CHECK (
        from_status IS NULL OR from_status IN (
            'pending', 'processing', 'pending_approval',
            'waiting_for_third_party_reply', 'completed', 'failed',
            'dead_letter', 'stopped'
        )
    ),
    CONSTRAINT task_status_events_to_status_check CHECK (to_status IN (
        'pending', 'processing', 'pending_approval',
        'waiting_for_third_party_reply', 'completed', 'failed',
        'dead_letter', 'stopped'
    )),
    CONSTRAINT task_status_events_reason_check CHECK (reason IN (
        'enqueued', 'claimed', 'completed',
        'retryable_failure', 'terminal_failure', 'timed_out', 'shutdown',
        'lease_lost', 'approval_requested', 'approval_accepted',
        'outreach_started', 'outreach_reply_received', 'outreach_timed_out',
        'outreach_extended', 'operator_stopped', 'operator_resumed'
    )),
    CONSTRAINT task_status_events_actor_kind_check CHECK (actor_kind IN (
        'system', 'worker', 'operator', 'approval', 'outreach'
    )),
    CONSTRAINT task_status_events_related_source_check CHECK (
        related_approval_id IS NULL OR related_outreach_id IS NULL
    )
);

CREATE INDEX task_status_events_task_history_idx
    ON task_status_events (task_id, transitioned_at, sequence, id);
CREATE INDEX task_status_events_company_correlation_timeline_idx
    ON task_status_events (company_id, correlation_id, transitioned_at, task_id, sequence, id);

-- A persistence operation may set these transaction-local values when old/new status alone is
-- not expressive enough. Empty/missing settings fall back to the deterministic mapping below.
CREATE FUNCTION record_task_status_event() RETURNS TRIGGER AS $$
DECLARE
    transition_reason TEXT;
    transition_actor_kind TEXT;
    transition_actor_id UUID;
    approval_id UUID;
    outreach_id UUID;
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.status = OLD.status THEN
        RETURN NULL;
    END IF;

    transition_reason := NULLIF(current_setting('mail_agents.task_transition_reason', TRUE), '');
    transition_actor_kind := NULLIF(current_setting('mail_agents.task_transition_actor_kind', TRUE), '');
    transition_actor_id := NULLIF(current_setting('mail_agents.task_transition_actor_id', TRUE), '')::uuid;
    approval_id := NULLIF(current_setting('mail_agents.task_transition_approval_id', TRUE), '')::uuid;
    outreach_id := NULLIF(current_setting('mail_agents.task_transition_outreach_id', TRUE), '')::uuid;

    IF transition_reason IS NULL THEN
        transition_reason := CASE
            WHEN TG_OP = 'INSERT' THEN 'enqueued'
            WHEN OLD.status = 'pending' AND NEW.status = 'processing' THEN 'claimed'
            WHEN OLD.status = 'processing' AND NEW.status = 'completed' THEN 'completed'
            WHEN OLD.status = 'processing' AND NEW.status = 'pending'
                 AND NEW.last_error = 'Task lease expired without the run reporting a result'
                THEN 'lease_lost'
            WHEN OLD.status = 'processing' AND NEW.status = 'dead_letter'
                 AND NEW.last_error = 'Task lease expired without the run reporting a result'
                THEN 'lease_lost'
            WHEN OLD.status = 'processing' AND NEW.status = 'pending' THEN 'retryable_failure'
            WHEN OLD.status = 'processing' AND NEW.status = 'dead_letter' THEN 'terminal_failure'
            WHEN OLD.status = 'processing' AND NEW.status = 'pending_approval' THEN 'approval_requested'
            WHEN OLD.status = 'processing' AND NEW.status = 'waiting_for_third_party_reply'
                THEN 'outreach_started'
            WHEN OLD.status = 'pending_approval' AND NEW.status = 'pending' THEN 'approval_accepted'
            WHEN OLD.status = 'waiting_for_third_party_reply' AND NEW.status = 'pending'
                THEN 'outreach_reply_received'
            WHEN OLD.status = 'waiting_for_third_party_reply' AND NEW.status = 'pending_approval'
                THEN 'outreach_timed_out'
            WHEN OLD.status = 'pending_approval'
                 AND NEW.status = 'waiting_for_third_party_reply' THEN 'outreach_extended'
            WHEN NEW.status = 'stopped' THEN 'operator_stopped'
            WHEN OLD.status = 'stopped' AND NEW.status = 'pending' THEN 'operator_resumed'
            ELSE 'retryable_failure'
        END;
    END IF;

    IF transition_actor_kind IS NULL THEN
        transition_actor_kind := CASE
            WHEN transition_reason IN ('claimed', 'completed', 'retryable_failure',
                                       'terminal_failure', 'timed_out', 'shutdown', 'lease_lost')
                THEN 'worker'
            WHEN transition_reason IN ('approval_requested', 'approval_accepted') THEN 'approval'
            WHEN transition_reason IN ('outreach_started', 'outreach_reply_received',
                                       'outreach_timed_out', 'outreach_extended') THEN 'outreach'
            WHEN transition_reason IN ('operator_stopped', 'operator_resumed') THEN 'operator'
            ELSE 'system'
        END;
    END IF;

    IF transition_actor_id IS NULL AND transition_actor_kind = 'worker' THEN
        transition_actor_id := COALESCE(NEW.worker_id, CASE WHEN TG_OP = 'UPDATE' THEN OLD.worker_id END);
    END IF;

    INSERT INTO task_status_events (
        id, company_id, task_id, correlation_id, sequence, from_status, to_status,
        reason, actor_kind, actor_id, related_approval_id, related_outreach_id,
        retry_count, run_at, execution_generation, transitioned_at
    ) VALUES (
        gen_random_uuid(), NEW.company_id, NEW.id, NEW.correlation_id,
        COALESCE((SELECT MAX(event.sequence) + 1
                  FROM task_status_events AS event WHERE event.task_id = NEW.id), 1),
        CASE WHEN TG_OP = 'UPDATE' THEN OLD.status ELSE NULL END,
        NEW.status, transition_reason, transition_actor_kind, transition_actor_id,
        approval_id, outreach_id, NEW.retry_count, NEW.run_at,
        COALESCE(NEW.execution_generation,
                 CASE WHEN TG_OP = 'UPDATE' THEN OLD.execution_generation END),
        CURRENT_TIMESTAMP
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER background_tasks_record_status_event
AFTER INSERT OR UPDATE OF status ON background_tasks
FOR EACH ROW EXECUTE FUNCTION record_task_status_event();

-- One small, identifier-only notification wakes every Tasks board that may need to reconcile.
CREATE FUNCTION notify_task_chain_changed() RETURNS TRIGGER AS $$
DECLARE
    notified_company_id UUID;
    notified_correlation_id UUID;
BEGIN
    IF TG_TABLE_NAME = 'task_status_events' THEN
        notified_company_id := NEW.company_id;
        notified_correlation_id := NEW.correlation_id;
    ELSIF TG_TABLE_NAME = 'email_outbox' THEN
        notified_company_id := NEW.company_id;
        notified_correlation_id := NEW.correlation_id;
    ELSIF TG_TABLE_NAME = 'human_approvals' THEN
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM background_tasks AS task WHERE task.id = NEW.task_id;
    ELSIF TG_TABLE_NAME = 'task_outreaches' THEN
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM background_tasks AS task WHERE task.id = NEW.task_id;
    ELSE
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM task_outreaches AS outreach
          JOIN background_tasks AS task ON task.id = outreach.task_id
         WHERE outreach.id = NEW.outreach_id;
    END IF;

    IF notified_company_id IS NOT NULL AND notified_correlation_id IS NOT NULL THEN
        PERFORM pg_notify(
            'task_chain_changed',
            json_build_object(
                'company_id', notified_company_id,
                'correlation_id', notified_correlation_id
            )::text
        );
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_status_events_notify_chain
AFTER INSERT ON task_status_events
FOR EACH ROW EXECUTE FUNCTION notify_task_chain_changed();

CREATE TRIGGER email_outbox_notify_chain
AFTER INSERT OR UPDATE OF status ON email_outbox
FOR EACH ROW EXECUTE FUNCTION notify_task_chain_changed();

CREATE TRIGGER human_approvals_notify_chain
AFTER INSERT OR UPDATE OF status ON human_approvals
FOR EACH ROW WHEN (NEW.task_id IS NOT NULL)
EXECUTE FUNCTION notify_task_chain_changed();

CREATE TRIGGER task_outreaches_notify_chain
AFTER INSERT OR UPDATE OF status ON task_outreaches
FOR EACH ROW EXECUTE FUNCTION notify_task_chain_changed();

CREATE TRIGGER task_outreach_targets_notify_chain
AFTER UPDATE OF responded_at ON task_outreach_targets
FOR EACH ROW WHEN (OLD.responded_at IS DISTINCT FROM NEW.responded_at)
EXECUTE FUNCTION notify_task_chain_changed();
