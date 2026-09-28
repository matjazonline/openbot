-- Complete indirect source isolation; preserve taskless legacy notifications.
CREATE OR REPLACE FUNCTION public.enqueue_actionable_notification_event(event_company uuid, event_source_kind text, event_source_id uuid, event_action_kind text, event_actor uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
DECLARE
    next_generation BIGINT;
BEGIN
    -- Existing actionable notifications belong to the legacy task runtime. Preserve
    -- missing-source withdrawal events, but never enqueue for a workflow-owned job.
    IF EXISTS (
        SELECT 1 FROM background_tasks AS task
         WHERE task.company_id = event_company AND task.queue_kind = 'workflow'
           AND ((event_source_kind = 'task' AND task.id = event_source_id)
             OR (event_source_kind = 'delegation' AND EXISTS (
                 SELECT 1 FROM task_outreaches AS outreach
                  WHERE outreach.id = event_source_id AND outreach.task_id = task.id))
             OR (event_source_kind = 'delivery' AND EXISTS (
                 SELECT 1 FROM message_deliveries AS delivery
                  WHERE delivery.company_id = event_company AND delivery.id = event_source_id
                    AND delivery.task_id = task.id))
             OR (event_source_kind = 'response_review' AND EXISTS (
                 SELECT 1 FROM response_drafts AS draft
                  WHERE draft.company_id = event_company AND draft.id = event_source_id
                    AND draft.task_id = task.id)))
    ) THEN
        RETURN;
    END IF;
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

CREATE OR REPLACE FUNCTION public.notify_attention_changed() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    row_data JSONB := CASE WHEN TG_OP = 'DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END;
    source_id UUID;
    company UUID := NULLIF(row_data->>'company_id', '')::UUID;
    channel UUID := NULLIF(row_data->>'channel_id', '')::UUID;
BEGIN
    -- Approvals/deliveries carry their task directly; reviews resolve through the draft.
    IF TG_ARGV[0] IN ('approval', 'delivery') AND EXISTS (
        SELECT 1 FROM background_tasks AS task
         WHERE task.company_id = company
           AND task.id = NULLIF(row_data->>'task_id', '')::UUID
           AND task.queue_kind = 'workflow'
    ) THEN
        RETURN COALESCE(NEW, OLD);
    END IF;
    source_id := COALESCE(
        NULLIF(row_data->>'id', '')::UUID,
        NULLIF(row_data->>'draft_id', '')::UUID
    );

    IF TG_ARGV[0] = 'response_review' THEN
        SELECT draft.company_id, draft.channel_id
          INTO company, channel
          FROM response_drafts AS draft
         WHERE draft.id = NULLIF(row_data->>'draft_id', '')::UUID
           AND draft.version = NULLIF(row_data->>'draft_version', '')::INTEGER
           AND NOT EXISTS (
               SELECT 1 FROM background_tasks AS task
                WHERE task.company_id = draft.company_id AND task.id = draft.task_id
                  AND task.queue_kind = 'workflow'
           );
    ELSIF TG_ARGV[0] = 'delegation' THEN
        SELECT task.company_id, task.channel_id
          INTO company, channel
          FROM background_tasks AS task
         WHERE task.id = NULLIF(row_data->>'task_id', '')::UUID
           AND task.queue_kind = 'legacy';
    END IF;

    IF company IS NOT NULL AND channel IS NOT NULL AND source_id IS NOT NULL THEN
        PERFORM pg_notify(
            'attention_changed',
            json_build_object(
                'company_id', company,
                'channel_id', channel,
                'source_kind', TG_ARGV[0],
                'source_id', source_id
            )::TEXT
        );
    END IF;
    RETURN COALESCE(NEW, OLD);
END;
$$;
