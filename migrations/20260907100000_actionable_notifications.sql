-- Actionable notifications are a projection of durable source transitions. The source triggers
-- write identifier-only outbox rows in the same transaction as the business change; a worker
-- resolves current responsibility and authorization before materializing recipient records.
ALTER TABLE user_notification_preferences
    RENAME COLUMN task_assignment_email_enabled TO assignment_email_enabled;

ALTER TABLE user_notification_preferences
    ADD COLUMN response_review_email_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN delegation_timeout_email_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN task_failure_email_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN delivery_failure_email_enabled BOOLEAN NOT NULL DEFAULT TRUE;

-- The reviewer transition and the actor who caused it must be one write. This is attribution for
-- notification routing only; review decisions remain in response_review_commands.
ALTER TABLE response_reviews
    ADD COLUMN notification_actor_principal_id UUID,
    ADD CONSTRAINT response_reviews_notification_actor_fk
        FOREIGN KEY (company_id, notification_actor_principal_id)
        REFERENCES principals(company_id, id) ON DELETE SET NULL (notification_actor_principal_id);

CREATE TABLE notification_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    notification_id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL,
    source_id UUID NOT NULL,
    action_kind TEXT NOT NULL,
    source_generation BIGINT NOT NULL,
    actor_principal_id UUID,
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 10,
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    execution_id UUID,
    owner_worker_id UUID,
    locked_at TIMESTAMPTZ,
    lock_expires_at TIMESTAMPTZ,
    last_error_class TEXT,
    last_error_detail TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    projected_at TIMESTAMPTZ,
    CONSTRAINT notification_events_identity_key
        UNIQUE (company_id, source_kind, source_id, action_kind, source_generation),
    CONSTRAINT notification_events_notification_id_key UNIQUE (notification_id),
    CONSTRAINT notification_events_actor_fk
        FOREIGN KEY (company_id, actor_principal_id)
        REFERENCES principals(company_id, id) ON DELETE SET NULL (actor_principal_id),
    CONSTRAINT notification_events_source_kind_check CHECK (
        source_kind IN ('task', 'handoff', 'response_review', 'delegation', 'delivery')
    ),
    CONSTRAINT notification_events_action_kind_check CHECK (
        action_kind IN (
            'assignment', 'response_review', 'delegation_timeout',
            'task_failure', 'delivery_failure'
        )
    ),
    CONSTRAINT notification_events_source_generation_check CHECK (source_generation > 0),
    CONSTRAINT notification_events_status_check CHECK (
        status IN ('pending', 'processing', 'projected', 'dead_letter')
    ),
    CONSTRAINT notification_events_attempt_check CHECK (
        attempt_count >= 0 AND max_attempts > 0 AND attempt_count <= max_attempts
    ),
    CONSTRAINT notification_events_error_check CHECK (
        last_error_class IS NULL OR last_error_class IN (
            'database', 'composition', 'invalid_source', 'lease_expired', 'internal'
        )
    ),
    CONSTRAINT notification_events_lease_check CHECK (
        (status = 'processing'
         AND execution_id IS NOT NULL AND owner_worker_id IS NOT NULL
         AND locked_at IS NOT NULL AND lock_expires_at IS NOT NULL
         AND lock_expires_at > locked_at)
        OR
        (status <> 'processing'
         AND execution_id IS NULL AND owner_worker_id IS NULL
         AND locked_at IS NULL AND lock_expires_at IS NULL)
    ),
    CONSTRAINT notification_events_projection_check CHECK (
        (status = 'projected') = (projected_at IS NOT NULL)
    )
);

CREATE INDEX notification_events_claimable_idx
    ON notification_events (available_at, occurred_at, id)
    WHERE status = 'pending';
CREATE INDEX notification_events_processing_lease_idx
    ON notification_events (lock_expires_at, id) WHERE status = 'processing';
CREATE INDEX notification_events_source_idx
    ON notification_events (company_id, source_kind, source_id, action_kind, source_generation DESC);

CREATE TABLE notifications (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    recipient_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recipient_principal_id UUID,
    event_id UUID NOT NULL REFERENCES notification_events(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL,
    source_id UUID NOT NULL,
    action_kind TEXT NOT NULL,
    source_generation BIGINT NOT NULL,
    channel_id UUID NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    read_at TIMESTAMPTZ,
    state_changed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    email_delivery_id UUID REFERENCES message_deliveries(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT notifications_identity_key UNIQUE (
        company_id, recipient_user_id, source_kind, source_id, action_kind, source_generation
    ),
    CONSTRAINT notifications_recipient_fk
        FOREIGN KEY (company_id, recipient_principal_id)
        REFERENCES principals(company_id, id) ON DELETE SET NULL (recipient_principal_id),
    CONSTRAINT notifications_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT notifications_event_identity_fk
        FOREIGN KEY (
            company_id, source_kind, source_id, action_kind, source_generation
        ) REFERENCES notification_events (
            company_id, source_kind, source_id, action_kind, source_generation
        ) ON DELETE CASCADE,
    CONSTRAINT notifications_email_delivery_key UNIQUE (email_delivery_id),
    CONSTRAINT notifications_source_kind_check CHECK (
        source_kind IN ('task', 'handoff', 'response_review', 'delegation', 'delivery')
    ),
    CONSTRAINT notifications_action_kind_check CHECK (
        action_kind IN (
            'assignment', 'response_review', 'delegation_timeout',
            'task_failure', 'delivery_failure'
        )
    ),
    CONSTRAINT notifications_source_generation_check CHECK (source_generation > 0),
    CONSTRAINT notifications_state_check CHECK (
        state IN ('active', 'resolved', 'withdrawn')
    )
);

CREATE UNIQUE INDEX notifications_one_active_action_idx
    ON notifications (company_id, recipient_user_id, source_kind, source_id, action_kind)
    WHERE state = 'active';
CREATE INDEX notifications_recipient_list_idx
    ON notifications (company_id, recipient_user_id, state, created_at DESC, id DESC);
CREATE INDEX notifications_active_age_idx
    ON notifications (created_at, id) WHERE state = 'active';

-- SQL and the application use this same authorization rule. It deliberately grants no access
-- from company role alone when the channel is allowlisted (apart from the company owner).
CREATE FUNCTION notification_principal_can_view(
    checked_company UUID, checked_channel UUID, checked_principal UUID
) RETURNS BOOLEAN
LANGUAGE SQL
STABLE
RETURN EXISTS (
    SELECT 1
    FROM principals AS principal
    JOIN company_members AS member
      ON member.company_id = principal.company_id AND member.user_id = principal.user_id
    JOIN channels AS channel
      ON channel.company_id = principal.company_id AND channel.id = checked_channel
    WHERE principal.company_id = checked_company AND principal.id = checked_principal
      AND principal.kind = 'person'
      AND (
          member.role = 'owner'
          OR channel.access_mode IN ('team', 'public')
          OR EXISTS (
              SELECT 1 FROM channel_principal_grants AS permission
              WHERE permission.company_id = checked_company
                AND permission.channel_id = checked_channel
                AND permission.principal_id = checked_principal
                AND permission.capability = 'view'
          )
      )
);

CREATE FUNCTION enqueue_actionable_notification_event(
    event_company UUID,
    event_source_kind TEXT,
    event_source_id UUID,
    event_action_kind TEXT,
    event_actor UUID
) RETURNS VOID
LANGUAGE plpgsql AS $$
DECLARE
    next_generation BIGINT;
BEGIN
    -- Cascading company deletion may invoke child-table triggers after the parent row has become
    -- invisible. Cleanup owns no new work, and the company cascade removes existing events.
    IF NOT EXISTS (SELECT 1 FROM companies WHERE id = event_company) THEN
        RETURN;
    END IF;
    IF event_actor IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM principals
         WHERE company_id = event_company AND id = event_actor
    ) THEN
        event_actor := NULL;
    END IF;
    SELECT COALESCE(MAX(source_generation), 0) + 1
      INTO next_generation
      FROM notification_events
     WHERE company_id = event_company
       AND source_kind = event_source_kind
       AND source_id = event_source_id
       AND action_kind = event_action_kind;

    INSERT INTO notification_events (
        company_id, source_kind, source_id, action_kind, source_generation, actor_principal_id
    ) VALUES (
        event_company, event_source_kind, event_source_id, event_action_kind,
        next_generation, event_actor
    );
END;
$$;

CREATE FUNCTION notification_from_task_ownership() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.new_owner_kind = 'human'
       AND (NEW.previous_owner_principal_id, NEW.previous_owner_kind)
           IS DISTINCT FROM (NEW.new_owner_principal_id, NEW.new_owner_kind) THEN
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'task', NEW.task_id, 'assignment', NEW.actor_principal_id
        );
    ELSIF NEW.previous_owner_kind = 'human' AND NEW.new_owner_kind <> 'human' THEN
        -- Releasing a formerly personal item must withdraw the old alert.
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'task', NEW.task_id, 'assignment', NEW.actor_principal_id
        );
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER task_ownership_actionable_notification
AFTER INSERT ON task_ownership_events
FOR EACH ROW EXECUTE FUNCTION notification_from_task_ownership();

CREATE FUNCTION notification_from_attention_source() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.source_kind = 'handoff'
       AND NEW.operation IN ('created', 'reassigned', 'resolved', 'withdrawn') THEN
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'handoff', NEW.source_id, 'assignment', NEW.actor_principal_id
        );
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER handoff_actionable_notification
AFTER INSERT ON attention_source_events
FOR EACH ROW EXECUTE FUNCTION notification_from_attention_source();

CREATE FUNCTION notification_from_review() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'response_review', NEW.draft_id, 'response_review',
            NEW.notification_actor_principal_id
        );
    ELSIF OLD.status IS DISTINCT FROM NEW.status
          OR OLD.reviewer_principal_id IS DISTINCT FROM NEW.reviewer_principal_id THEN
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'response_review', NEW.draft_id, 'response_review',
            NEW.notification_actor_principal_id
        );
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER response_review_actionable_notification
AFTER INSERT OR UPDATE OF status, reviewer_principal_id ON response_reviews
FOR EACH ROW EXECUTE FUNCTION notification_from_review();

CREATE FUNCTION notification_withdraw_deleted_source() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    row_data JSONB := to_jsonb(OLD);
    deleted_source_id UUID := COALESCE(
        (row_data->>'id')::UUID, (row_data->>'draft_id')::UUID
    );
BEGIN
    UPDATE notifications
       SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
           updated_at = CURRENT_TIMESTAMP
     WHERE source_kind = TG_ARGV[0] AND source_id = deleted_source_id
       AND state = 'active';
    RETURN OLD;
END;
$$;

CREATE TRIGGER response_review_notification_delete
AFTER DELETE ON response_reviews
FOR EACH ROW EXECUTE FUNCTION notification_withdraw_deleted_source('response_review');

CREATE TRIGGER handoff_notification_delete
AFTER DELETE ON manual_handoffs
FOR EACH ROW EXECUTE FUNCTION notification_withdraw_deleted_source('handoff');

CREATE FUNCTION notification_from_outreach() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    event_company UUID;
BEGIN
    IF TG_OP = 'DELETE' THEN
        UPDATE notifications
           SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
               updated_at = CURRENT_TIMESTAMP
         WHERE source_kind = 'delegation' AND source_id = OLD.id AND state = 'active';
    ELSIF OLD.status IS DISTINCT FROM NEW.status THEN
        SELECT company_id INTO event_company FROM background_tasks WHERE id = NEW.task_id;
        IF event_company IS NOT NULL AND (
            OLD.status = 'timeout_pending_approval' OR NEW.status = 'timeout_pending_approval'
        ) THEN
            PERFORM enqueue_actionable_notification_event(
                event_company, 'delegation', NEW.id, 'delegation_timeout', NULL
            );
        END IF;
    END IF;
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE TRIGGER outreach_actionable_notification
AFTER UPDATE OF status OR DELETE ON task_outreaches
FOR EACH ROW EXECUTE FUNCTION notification_from_outreach();

CREATE FUNCTION notification_from_task_status() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        UPDATE notifications
           SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
               updated_at = CURRENT_TIMESTAMP
         WHERE company_id = OLD.company_id AND source_kind = 'task'
           AND source_id = OLD.id AND state = 'active';
    ELSIF OLD.status IS DISTINCT FROM NEW.status THEN
        IF OLD.status = 'dead_letter' OR NEW.status = 'dead_letter' THEN
            PERFORM enqueue_actionable_notification_event(
                NEW.company_id, 'task', NEW.id, 'task_failure', NULL
            );
        END IF;
        IF NEW.status IN ('completed', 'stopped', 'dead_letter')
           OR OLD.status IN ('completed', 'stopped', 'dead_letter') THEN
            PERFORM enqueue_actionable_notification_event(
                NEW.company_id, 'task', NEW.id, 'assignment', NULL
            );
        END IF;
    END IF;
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE TRIGGER task_status_actionable_notification
AFTER UPDATE OF status OR DELETE ON background_tasks
FOR EACH ROW EXECUTE FUNCTION notification_from_task_status();

CREATE FUNCTION notification_from_delivery() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    event_company UUID := COALESCE(NEW.company_id, OLD.company_id);
BEGIN
    IF event_company IS NULL THEN
        RETURN COALESCE(NEW, OLD);
    END IF;
    IF TG_OP = 'DELETE' THEN
        UPDATE notifications
           SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
               updated_at = CURRENT_TIMESTAMP
         WHERE company_id = event_company AND source_kind = 'delivery'
           AND source_id = OLD.id AND state = 'active';
    ELSIF OLD.status IS DISTINCT FROM NEW.status
          AND (OLD.status IN ('dead_letter', 'outcome_unknown')
               OR NEW.status IN ('dead_letter', 'outcome_unknown')) THEN
        PERFORM enqueue_actionable_notification_event(
            event_company, 'delivery', NEW.id, 'delivery_failure', NULL
        );
    END IF;
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE TRIGGER delivery_actionable_notification
AFTER UPDATE OF status OR DELETE ON message_deliveries
FOR EACH ROW EXECUTE FUNCTION notification_from_delivery();

CREATE FUNCTION withdraw_unauthorized_notifications(
    changed_company UUID, changed_channel UUID
) RETURNS VOID
LANGUAGE SQL AS $$
    UPDATE notifications AS notification
       SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
           updated_at = CURRENT_TIMESTAMP
     WHERE notification.company_id = changed_company
       AND notification.channel_id = changed_channel
       AND notification.state = 'active'
       AND (
           notification.recipient_principal_id IS NULL
           OR NOT notification_principal_can_view(
               notification.company_id,
               notification.channel_id,
               notification.recipient_principal_id
           )
       );
$$;

CREATE FUNCTION notification_recheck_channel_access() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    row_data JSONB := CASE WHEN TG_OP = 'DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END;
BEGIN
    PERFORM withdraw_unauthorized_notifications(
        (row_data->>'company_id')::UUID,
        COALESCE((row_data->>'channel_id')::UUID, (row_data->>'id')::UUID)
    );
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE TRIGGER notification_channel_grant_recheck
AFTER INSERT OR UPDATE OR DELETE ON channel_principal_grants
FOR EACH ROW EXECUTE FUNCTION notification_recheck_channel_access();

CREATE TRIGGER notification_channel_policy_recheck
AFTER UPDATE OF access_mode ON channels
FOR EACH ROW EXECUTE FUNCTION notification_recheck_channel_access();

CREATE FUNCTION notification_withdraw_deleted_principal() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    UPDATE notifications
       SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
           updated_at = CURRENT_TIMESTAMP
     WHERE company_id = OLD.company_id AND recipient_principal_id = OLD.id
       AND state = 'active';
    RETURN OLD;
END;
$$;

CREATE TRIGGER notification_principal_delete_recheck
BEFORE DELETE ON principals
FOR EACH ROW EXECUTE FUNCTION notification_withdraw_deleted_principal();

CREATE FUNCTION notify_actionable_notification_changed() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify(
        'actionable_notification_changed',
        json_build_object(
            'company_id', NEW.company_id,
            'recipient_user_id', NEW.recipient_user_id,
            'notification_id', NEW.id
        )::TEXT
    );
    RETURN NEW;
END;
$$;

CREATE TRIGGER actionable_notifications_notify
AFTER INSERT OR UPDATE OF state, read_at ON notifications
FOR EACH ROW EXECUTE FUNCTION notify_actionable_notification_changed();
