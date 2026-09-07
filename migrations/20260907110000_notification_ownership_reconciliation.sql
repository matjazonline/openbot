-- Allocate source generations under a transaction-scoped lock. Source transitions normally
-- serialize on their own row, but independent transitions can reconcile the same notification
-- identity (for example ownership and delivery state).
CREATE OR REPLACE FUNCTION enqueue_actionable_notification_event(
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
    IF NOT EXISTS (
        SELECT 1 FROM companies AS company WHERE company.id = event_company
    ) THEN
        RETURN;
    END IF;
    IF event_actor IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM principals AS principal
         WHERE principal.company_id = event_company AND principal.id = event_actor
    ) THEN
        event_actor := NULL;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended(
        event_company::TEXT || ':' || event_source_kind || ':' || event_source_id::TEXT
            || ':' || event_action_kind,
        0
    ));
    SELECT COALESCE(MAX(source_generation), 0) + 1
      INTO next_generation
      FROM notification_events AS event
     WHERE event.company_id = event_company
       AND event.source_kind = event_source_kind
       AND event.source_id = event_source_id
       AND event.action_kind = event_action_kind;

    INSERT INTO notification_events (
        company_id, source_kind, source_id, action_kind, source_generation, actor_principal_id
    ) VALUES (
        event_company, event_source_kind, event_source_id, event_action_kind,
        next_generation, event_actor
    );
END;
$$;

-- Responsibility for outstanding failures and timeout decisions follows task ownership. These
-- remain identifier-only reconciliation events; the projector resolves current source state,
-- recipient and authorization after commit.
CREATE OR REPLACE FUNCTION notification_from_task_ownership() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    related_source UUID;
BEGIN
    IF NEW.new_owner_kind = 'human'
       AND (NEW.previous_owner_principal_id, NEW.previous_owner_kind)
           IS DISTINCT FROM (NEW.new_owner_principal_id, NEW.new_owner_kind) THEN
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'task', NEW.task_id, 'assignment', NEW.actor_principal_id
        );
    ELSIF NEW.previous_owner_kind = 'human' AND NEW.new_owner_kind <> 'human' THEN
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'task', NEW.task_id, 'assignment', NEW.actor_principal_id
        );
    END IF;

    IF EXISTS (
        SELECT 1 FROM background_tasks AS task
         WHERE task.company_id = NEW.company_id AND task.id = NEW.task_id
           AND task.status = 'dead_letter'
    ) THEN
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'task', NEW.task_id, 'task_failure', NEW.actor_principal_id
        );
    END IF;

    FOR related_source IN
        SELECT outreach.id FROM task_outreaches AS outreach
         WHERE outreach.task_id = NEW.task_id AND outreach.status = 'timeout_pending_approval'
    LOOP
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'delegation', related_source, 'delegation_timeout',
            NEW.actor_principal_id
        );
    END LOOP;

    FOR related_source IN
        SELECT delivery.id FROM message_deliveries AS delivery
         WHERE delivery.company_id = NEW.company_id AND delivery.task_id = NEW.task_id
           AND delivery.status IN ('dead_letter', 'outcome_unknown')
    LOOP
        PERFORM enqueue_actionable_notification_event(
            NEW.company_id, 'delivery', related_source, 'delivery_failure',
            NEW.actor_principal_id
        );
    END LOOP;
    RETURN NEW;
END;
$$;
