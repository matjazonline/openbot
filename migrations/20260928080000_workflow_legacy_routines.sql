-- Isolate indirect legacy consumers while workflow admission remains disabled.
-- The queue discriminator is immutable; every selected legacy row stays legacy.
-- Ownership-ledger immutability still protects every task and is intentionally unchanged.

CREATE OR REPLACE FUNCTION public.delete_channel_target_tasks() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    DELETE FROM background_tasks AS task
    WHERE task.queue_kind = 'legacy' AND EXISTS (
        SELECT 1 FROM task_channel_targets AS target
        WHERE target.task_id = task.id AND target.channel_id = OLD.id
    );
    RETURN OLD;
END;
$$;

CREATE OR REPLACE FUNCTION public.guard_active_agent_harness_change() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.harness_kind IS NOT DISTINCT FROM OLD.harness_kind
       AND NEW.response_contract IS NOT DISTINCT FROM OLD.response_contract THEN
        RETURN NEW;
    END IF;
    IF EXISTS (
        SELECT 1 FROM principals AS principal
        JOIN background_tasks AS task
          ON task.company_id = principal.company_id AND task.owner_principal_id = principal.id
        WHERE principal.company_id = OLD.company_id AND principal.agent_id = OLD.id
          AND task.queue_kind = 'legacy'
          AND task.status IN ('pending', 'processing', 'pending_approval',
                              'waiting_for_third_party_reply', 'stopped', 'failed', 'dead_letter')
    ) OR EXISTS (
        SELECT 1 FROM channel_agents AS assignment
        JOIN background_tasks AS task
          ON task.company_id = assignment.company_id AND task.channel_id = assignment.channel_id
        WHERE assignment.agent_id = OLD.id
          AND task.queue_kind = 'legacy'
          AND task.status IN ('pending', 'processing', 'pending_approval',
                              'waiting_for_third_party_reply', 'stopped', 'failed', 'dead_letter')
    ) THEN
        RAISE EXCEPTION USING ERRCODE = '23514',
            CONSTRAINT = 'agent_harness_has_unsettled_tasks',
            MESSAGE = 'Agent harness cannot change while tasks are active or suspended';
    END IF;
    RETURN NEW;
END;
$$;

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
                    AND delivery.task_id = task.id)))
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

CREATE OR REPLACE FUNCTION public.notification_from_outreach() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    event_company UUID;
BEGIN
    IF TG_OP = 'DELETE' THEN
        UPDATE notifications
           SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
               updated_at = CURRENT_TIMESTAMP
         WHERE source_kind = 'delegation' AND source_id = OLD.id AND state = 'active';
    ELSIF OLD.status IS DISTINCT FROM NEW.status THEN
        SELECT company_id INTO event_company FROM background_tasks WHERE id = NEW.task_id AND queue_kind = 'legacy';
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

CREATE OR REPLACE FUNCTION public.notification_from_task_ownership() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    related_source UUID;
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM background_tasks AS task
         WHERE task.company_id = NEW.company_id AND task.id = NEW.task_id
           AND task.queue_kind = 'legacy'
    ) THEN
        RETURN NEW;
    END IF;
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

CREATE OR REPLACE FUNCTION public.notify_agent_instruction() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM background_tasks AS task
         WHERE task.company_id = NEW.company_id AND task.id = NEW.task_id
           AND task.queue_kind = 'legacy'
    ) THEN
        RETURN NULL;
    END IF;
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
    source_id := COALESCE(
        NULLIF(row_data->>'id', '')::UUID,
        NULLIF(row_data->>'draft_id', '')::UUID
    );

    IF TG_ARGV[0] = 'response_review' THEN
        SELECT draft.company_id, draft.channel_id
          INTO company, channel
          FROM response_drafts AS draft
         WHERE draft.id = NULLIF(row_data->>'draft_id', '')::UUID
           AND draft.version = NULLIF(row_data->>'draft_version', '')::INTEGER;
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

CREATE OR REPLACE FUNCTION public.notify_task_chain_changed() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    notified_company_id UUID;
    notified_correlation_id UUID;
BEGIN
    IF TG_TABLE_NAME IN ('task_status_events', 'message_deliveries') THEN
        IF EXISTS (
            SELECT 1 FROM background_tasks AS task
             WHERE task.company_id = NEW.company_id AND task.id = NEW.task_id
               AND task.queue_kind = 'workflow'
        ) THEN
            RETURN NULL;
        END IF;
    END IF;
    -- `UPDATE OF status` fires whenever the column appears in a SET list, whether or not the value
    -- moved. A write that leaves the status alone changes nothing the board draws, so it must not
    -- wake every connected viewer of the company. This suppresses no real transition:
    -- `pending -> sending -> delivered` is three material changes and still emits three
    -- notifications,
    -- which the stream coalesces on its own. The checks are per table because these are the only
    -- notifying tables that have a `status` column at all.
    IF TG_OP = 'UPDATE' THEN
        IF TG_TABLE_NAME = 'message_deliveries' THEN
            IF NEW.status IS NOT DISTINCT FROM OLD.status THEN
                RETURN NULL;
            END IF;
        ELSIF TG_TABLE_NAME = 'human_approvals' THEN
            IF NEW.status IS NOT DISTINCT FROM OLD.status THEN
                RETURN NULL;
            END IF;
        ELSIF TG_TABLE_NAME = 'task_outreaches' THEN
            IF NEW.status IS NOT DISTINCT FROM OLD.status THEN
                RETURN NULL;
            END IF;
        END IF;
    END IF;

    IF TG_TABLE_NAME = 'task_status_events' THEN
        notified_company_id := NEW.company_id;
        notified_correlation_id := NEW.correlation_id;
    ELSIF TG_TABLE_NAME = 'message_deliveries' THEN
        notified_company_id := NEW.company_id;
        notified_correlation_id := NEW.correlation_id;
    ELSIF TG_TABLE_NAME = 'human_approvals' THEN
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM background_tasks AS task WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy';
    ELSIF TG_TABLE_NAME = 'task_outreaches' THEN
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM background_tasks AS task WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy';
    ELSE
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM task_outreaches AS outreach
          JOIN background_tasks AS task ON task.id = outreach.task_id
         WHERE outreach.id = NEW.outreach_id AND task.queue_kind = 'legacy';
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
$$;

CREATE OR REPLACE FUNCTION public.notify_task_ownership() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
      FROM background_tasks WHERE id = NEW.task_id AND company_id = NEW.company_id AND queue_kind = 'legacy';
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;

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
$$;

CREATE OR REPLACE FUNCTION public.release_tasks_for_removed_principal() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- One statement per affected relation, not one per owned task. `owned` locks the row set and
    -- carries the values the other two arms need from *before* the release: `task_attempts` is
    -- fenced on the generation the task is about to lose, and the ownership event's `sequence` is
    -- the version the task is about to leave behind, per task rather than a running count. Row
    -- triggers still fire once per row -- that is PostgreSQL, not the statement shape -- but the
    -- row set is planned, locked and computed once instead of once per task.
    --
    -- The `task_attempts` arm is never read by the final `INSERT`, and does not need to be: a
    -- data-modifying `WITH` clause always runs to completion whether or not the primary query
    -- selects from it.
    WITH owned AS (
        SELECT task.id, task.company_id, task.status, task.ownership_version,
               task.execution_generation
          FROM background_tasks AS task
         WHERE task.company_id = OLD.company_id AND task.owner_principal_id = OLD.id
           AND task.queue_kind = 'legacy'
         FOR UPDATE
    ),
    released AS (
        UPDATE background_tasks AS task
           SET owner_principal_id = NULL,
               owner_principal_kind = NULL,
               ownership_version = owned.ownership_version + 1,
               status = CASE WHEN owned.status = 'processing' THEN 'pending' ELSE owned.status END,
               worker_id = NULL,
               execution_generation = NULL,
               locked_at = NULL,
               lock_expires_at = NULL,
               run_at = CASE
                   WHEN owned.status = 'processing' THEN CURRENT_TIMESTAMP
                   ELSE task.run_at
               END,
               transition_reason = CASE
                   WHEN owned.status = 'processing' THEN 'ownership_transferred'
                   ELSE task.transition_reason
               END,
               transition_actor_kind = CASE
                   WHEN owned.status = 'processing' THEN 'system'
                   ELSE task.transition_actor_kind
               END,
               transition_actor_id = CASE
                   WHEN owned.status = 'processing' THEN NULL
                   ELSE task.transition_actor_id
               END,
               updated_at = CURRENT_TIMESTAMP
          FROM owned
         WHERE task.company_id = owned.company_id AND task.id = owned.id
        RETURNING task.id, task.company_id, owned.status AS released_status,
                  owned.ownership_version AS released_version,
                  owned.execution_generation AS released_generation
    ),
    fenced AS (
        UPDATE task_attempts AS attempt
           SET status = 'failed', stop_reason = 'ownership_transferred',
               error = 'Task ownership was removed', finished_at = CURRENT_TIMESTAMP
          FROM released
         WHERE attempt.task_id = released.id
           AND released.released_status = 'processing'
           AND attempt.execution_generation = released.released_generation
           AND attempt.status = 'processing'
        RETURNING attempt.id
    )
    INSERT INTO task_ownership_events (
        task_id, company_id, sequence, from_version, to_version, command_id,
        command_fingerprint, operation, actor_kind, previous_owner_principal_id,
        previous_owner_kind, previous_owner_label, new_owner_kind, reason
    )
    SELECT released.id, released.company_id, released.released_version + 1,
           released.released_version, released.released_version + 1, gen_random_uuid(),
           'owner-removed:' || OLD.id::text || ':' || released.released_version::text,
           'owner_removed', 'system', OLD.id,
           CASE OLD.kind WHEN 'person' THEN 'human' ELSE 'agent' END,
           OLD.display_label, 'unassigned', 'owner_removed'
      FROM released;

    -- Two triggers share this body. An agent's principal is deleted outright, cascaded from the
    -- agent row; a person's principal is demoted to 'external' by team removal, which is an
    -- UPDATE whose row must be returned as NEW -- returning OLD there would quietly write the
    -- pre-demotion row back, and returning NULL would skip the update this clears the way for.
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;
