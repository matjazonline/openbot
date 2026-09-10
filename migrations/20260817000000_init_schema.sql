-- Squashed baseline for a newly created database (through 2026-09-10).
-- Incremental upgrades and data backfills are intentionally unsupported; reset existing databases.

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: citext; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS citext WITH SCHEMA public;


--
-- Name: EXTENSION citext; Type: COMMENT; Schema: -; Owner: -
--

COMMENT ON EXTENSION citext IS 'data type for case-insensitive character strings';


--
-- Name: pg_stat_statements; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS pg_stat_statements WITH SCHEMA public;


--
-- Name: EXTENSION pg_stat_statements; Type: COMMENT; Schema: -; Owner: -
--

COMMENT ON EXTENSION pg_stat_statements IS 'track planning and execution statistics of all SQL statements executed';


--
-- Name: attention_source_events_are_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.attention_source_events_are_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'DELETE'
       AND NOT EXISTS (SELECT 1 FROM companies WHERE id = OLD.company_id) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'attention source events are immutable' USING ERRCODE = '55000';
END;
$$;


--
-- Name: bump_handoff_version_for_responsibility_cleanup(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.bump_handoff_version_for_responsibility_cleanup() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.responsible_principal_id IS DISTINCT FROM OLD.responsible_principal_id
       AND NEW.version = OLD.version THEN
        NEW.version := OLD.version + 1;
        NEW.updated_at := CURRENT_TIMESTAMP;
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: bump_task_attention_version_for_owner_change(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.bump_task_attention_version_for_owner_change() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.owner_principal_id, NEW.owner_principal_kind)
       IS DISTINCT FROM (OLD.owner_principal_id, OLD.owner_principal_kind) THEN
        NEW.attention_version := OLD.attention_version + 1;
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: create_memory_lifecycle_for_legacy_connection(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.create_memory_lifecycle_for_legacy_connection() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    INSERT INTO memory_remote_resource_lifecycles
        (provider, remote_database_id, company_id, desired_state)
    VALUES (NEW.provider, NEW.remote_database_id, NEW.company_id, 'present')
    ON CONFLICT (provider, remote_database_id) DO UPDATE
    SET company_id = EXCLUDED.company_id,
        desired_state = 'present',
        quiesce_until = CURRENT_TIMESTAMP,
        last_error = NULL,
        updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$;


--
-- Name: delete_channel_target_tasks(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.delete_channel_target_tasks() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    DELETE FROM background_tasks task
    WHERE EXISTS (
        SELECT 1 FROM task_channel_targets target
        WHERE target.task_id = task.id AND target.channel_id = OLD.id
    );
    RETURN OLD;
END;
$$;


--
-- Name: delete_orphan_message(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.delete_orphan_message() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    DELETE FROM messages message
    WHERE message.id = OLD.message_id
      AND NOT EXISTS (
          SELECT 1 FROM thread_messages association
          WHERE association.message_id = OLD.message_id
      );
    RETURN NULL;
END;
$$;


--
-- Name: enforce_agent_skill_scope(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enforce_agent_skill_scope() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM agents AS agent
        WHERE agent.id = NEW.agent_id
          AND agent.company_id IS NOT DISTINCT FROM NEW.company_id
    ) THEN
        RAISE EXCEPTION 'agent must match the relationship company scope'
            USING ERRCODE = '23514', CONSTRAINT = 'agent_skills_agent_scope_check';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM skills AS skill
        WHERE skill.id = NEW.skill_id
          AND skill.company_id IS NOT DISTINCT FROM NEW.company_id
    ) THEN
        RAISE EXCEPTION 'skill must match the relationship company scope'
            USING ERRCODE = '23514', CONSTRAINT = 'agent_skills_skill_scope_check';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: enforce_channel_agent_scope(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enforce_channel_agent_scope() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM agents AS agent
        WHERE agent.id = NEW.agent_id
          AND (agent.company_id IS NULL OR agent.company_id = NEW.company_id)
    ) THEN
        RAISE EXCEPTION 'agent must belong to the channel company or the global library'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: enforce_enabled_channel_has_active_agent(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enforce_enabled_channel_has_active_agent() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    checked_channel_id UUID;
BEGIN
    IF TG_TABLE_NAME = 'channels' THEN
        checked_channel_id := COALESCE(NEW.id, OLD.id);
    ELSE
        checked_channel_id := COALESCE(NEW.channel_id, OLD.channel_id);
    END IF;
    IF EXISTS (
        SELECT 1 FROM channels AS channel
        WHERE channel.id = checked_channel_id AND channel.enabled
    ) AND NOT EXISTS (
        SELECT 1 FROM channel_agents AS assignment
        WHERE assignment.channel_id = checked_channel_id AND assignment.position = 0
    ) THEN
        RAISE EXCEPTION 'enabled channel must have an active agent at position 0'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;


--
-- Name: enforce_outreach_status_transition(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enforce_outreach_status_transition() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
$$;


--
-- Name: enforce_outreach_target_status_transition(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enforce_outreach_target_status_transition() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
$$;


--
-- Name: enforce_owned_channel_position_zero(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enforce_owned_channel_position_zero() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    checked_channel_id UUID;
BEGIN
    IF TG_TABLE_NAME = 'channels' THEN
        checked_channel_id := COALESCE(NEW.id, OLD.id);
    ELSE
        checked_channel_id := COALESCE(NEW.channel_id, OLD.channel_id);
    END IF;

    IF EXISTS (
        SELECT 1
        FROM channels AS channel
        WHERE channel.id = checked_channel_id
          AND channel.owner_agent_id IS NOT NULL
          AND NOT EXISTS (
              SELECT 1
              FROM channel_agents AS assignment
              WHERE assignment.channel_id = channel.id
                AND assignment.agent_id = channel.owner_agent_id
                AND assignment.position = 0
          )
    ) THEN
        RAISE EXCEPTION 'owned channel must assign its owner agent at position 0'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;


--
-- Name: enforce_response_draft_evidence_immutability(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enforce_response_draft_evidence_immutability() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF EXISTS (
            SELECT 1 FROM response_reviews
            WHERE company_id = NEW.company_id AND draft_id = NEW.draft_id
              AND draft_version = NEW.draft_version
        ) THEN
            RAISE EXCEPTION 'response draft evidence is already sealed' USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM response_drafts
        WHERE company_id = OLD.company_id AND id = OLD.draft_id AND version = OLD.draft_version
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'response draft evidence is immutable' USING ERRCODE = '23514';
END;
$$;


--
-- Name: enforce_response_draft_update(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enforce_response_draft_update() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (OLD.id, OLD.version, OLD.company_id, OLD.channel_id, OLD.thread_id,
        OLD.author_principal_id, OLD.proposed_message_id,
        OLD.subject, OLD.body, OLD.attachment_snapshot, OLD.recipient_snapshot,
        OLD.transport_snapshot, OLD.publication_snapshot, OLD.created_by_principal_id,
        OLD.created_at, OLD.source_handoff_generation) IS DISTINCT FROM
       (NEW.id, NEW.version, NEW.company_id, NEW.channel_id, NEW.thread_id,
        NEW.author_principal_id, NEW.proposed_message_id,
        NEW.subject, NEW.body, NEW.attachment_snapshot, NEW.recipient_snapshot,
        NEW.transport_snapshot, NEW.publication_snapshot, NEW.created_by_principal_id,
        NEW.created_at, NEW.source_handoff_generation) THEN
        RAISE EXCEPTION 'response draft versions are immutable' USING ERRCODE = '23514';
    END IF;
    IF OLD.task_id IS DISTINCT FROM NEW.task_id
       AND NOT (OLD.task_id IS NOT NULL AND NEW.task_id IS NULL) THEN
        RAISE EXCEPTION 'a response draft task source cannot be replaced' USING ERRCODE = '23514';
    END IF;
    IF OLD.status <> NEW.status AND NOT (
        OLD.status = 'pending_review'
        AND NEW.status IN ('rejected', 'expired', 'superseded', 'published')
    ) THEN
        RAISE EXCEPTION 'invalid response draft status transition' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: enqueue_actionable_notification_event(uuid, text, uuid, text, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enqueue_actionable_notification_event(event_company uuid, event_source_kind text, event_source_id uuid, event_action_kind text, event_actor uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: guard_active_agent_harness_change(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.guard_active_agent_harness_change() RETURNS trigger
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
          AND task.status IN ('pending', 'processing', 'pending_approval',
                              'waiting_for_third_party_reply', 'stopped', 'failed', 'dead_letter')
    ) OR EXISTS (
        SELECT 1 FROM channel_agents AS assignment
        JOIN background_tasks AS task
          ON task.company_id = assignment.company_id AND task.channel_id = assignment.channel_id
        WHERE assignment.agent_id = OLD.id
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


--
-- Name: guard_agent_mcp_harness(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.guard_agent_mcp_harness() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.harness_kind <> 'rig' AND NEW.company_id IS NOT NULL THEN
        PERFORM id FROM companies WHERE id = NEW.company_id FOR SHARE;
        IF EXISTS (
            SELECT 1 FROM agent_mcp_selections AS selection
            JOIN company_mcp_connections AS connection
              ON connection.company_id = selection.company_id AND connection.id = selection.connection_id
            JOIN company_mcp_tool_grants AS grant_row
              ON grant_row.company_id = connection.company_id AND grant_row.connection_id = connection.id
            WHERE selection.company_id = NEW.company_id AND selection.agent_id = NEW.id
              AND connection.enabled AND connection.deleted_at IS NULL
        ) THEN
            RAISE EXCEPTION USING ERRCODE = '23514', CONSTRAINT = 'agent_mcp_requires_rig',
                MESSAGE = 'Remove enabled MCP grants before changing harness to ai_agents';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: initialize_task_ownership(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.initialize_task_ownership() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
$$;


--
-- Name: internal_note_tombstones_are_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.internal_note_tombstones_are_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM internal_notes WHERE company_id = OLD.company_id AND id = OLD.note_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'internal note tombstones are immutable' USING ERRCODE = '55000';
END;
$$;


--
-- Name: internal_notes_are_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.internal_notes_are_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM threads
         WHERE company_id = OLD.company_id AND channel_id = OLD.channel_id AND id = OLD.thread_id
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'internal note audit rows are immutable' USING ERRCODE = '55000';
END;
$$;


--
-- Name: lock_task_agent_harnesses(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.lock_task_agent_harnesses() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.status IN ('pending', 'processing', 'pending_approval',
                      'waiting_for_third_party_reply', 'stopped', 'failed', 'dead_letter') THEN
        PERFORM agent.id FROM agents AS agent
        JOIN (
            SELECT principal.agent_id AS id FROM principals AS principal
            WHERE principal.id = NEW.owner_principal_id AND principal.company_id = NEW.company_id
            UNION
            SELECT assignment.agent_id AS id FROM channel_agents AS assignment
            WHERE assignment.channel_id = NEW.channel_id AND assignment.company_id = NEW.company_id
        ) AS affected ON affected.id = agent.id
        -- Preserve stable lock order and shared/library agents selected by this channel.
        ORDER BY agent.id FOR SHARE OF agent;
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: notification_from_attention_source(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_from_attention_source() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: notification_from_delivery(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_from_delivery() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: notification_from_outreach(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_from_outreach() RETURNS trigger
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


--
-- Name: notification_from_review(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_from_review() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: notification_from_task_ownership(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_from_task_ownership() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: notification_from_task_status(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_from_task_status() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: valid_creation_provenance(jsonb); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_creation_provenance(provenance jsonb) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN ((jsonb_typeof(provenance) = 'object'::text) AND ((provenance - ARRAY['actor_type'::text, 'actor_id'::text, 'actor_name'::text, 'source_channel_id'::text, 'source_task_id'::text]) = '{}'::jsonb) AND (jsonb_typeof((provenance -> 'actor_type'::text)) = 'string'::text) AND ((provenance ->> 'actor_type'::text) = ANY (ARRAY['user'::text, 'agent'::text, 'system'::text])) AND (jsonb_typeof((provenance -> 'actor_name'::text)) = 'string'::text) AND (btrim((provenance ->> 'actor_name'::text)) <> ''::text) AND CASE (provenance ->> 'actor_type'::text) WHEN 'system'::text THEN ((provenance ? 'actor_id'::text) AND ((provenance -> 'actor_id'::text) = 'null'::jsonb) AND COALESCE(((provenance -> 'source_channel_id'::text) = 'null'::jsonb), true) AND COALESCE(((provenance -> 'source_task_id'::text) = 'null'::jsonb), true)) WHEN 'user'::text THEN ((jsonb_typeof((provenance -> 'actor_id'::text)) = 'string'::text) AND ((provenance ->> 'actor_id'::text) ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'::text) AND COALESCE(((provenance -> 'source_channel_id'::text) = 'null'::jsonb), true) AND COALESCE(((provenance -> 'source_task_id'::text) = 'null'::jsonb), true)) WHEN 'agent'::text THEN ((jsonb_typeof((provenance -> 'actor_id'::text)) = 'string'::text) AND ((provenance ->> 'actor_id'::text) ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'::text) AND (jsonb_typeof((provenance -> 'source_channel_id'::text)) = 'string'::text) AND ((provenance ->> 'source_channel_id'::text) ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'::text) AND (jsonb_typeof((provenance -> 'source_task_id'::text)) = 'string'::text) AND ((provenance ->> 'source_task_id'::text) ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'::text)) ELSE false END);


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: channel_principal_grants; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.channel_principal_grants (
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    capability text NOT NULL,
    provenance text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT channel_principal_grants_capability_check CHECK ((capability = ANY (ARRAY['participate'::text, 'view'::text]))),
    CONSTRAINT channel_principal_grants_provenance_check CHECK ((provenance = ANY (ARRAY['configured_allowlist'::text, 'manager'::text, 'conversation_membership'::text, 'system'::text])))
);


--
-- Name: channels; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.channels (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    name text NOT NULL,
    access_mode text DEFAULT 'team'::text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    add_3rd_party boolean DEFAULT true NOT NULL,
    created_by jsonb NOT NULL,
    retrieve_company_memory boolean DEFAULT false NOT NULL,
    retrieve_agent_memory boolean DEFAULT false NOT NULL,
    retrieve_user_memory boolean DEFAULT false NOT NULL,
    persist_company_memory boolean DEFAULT false NOT NULL,
    persist_agent_memory boolean DEFAULT false NOT NULL,
    persist_user_memory boolean DEFAULT false NOT NULL,
    description text,
    owner_agent_id uuid,
    external_response_review_override text,
    preferred_reviewer_principal_id uuid,
    CONSTRAINT channels_access_mode_check CHECK ((access_mode = ANY (ARRAY['team'::text, 'allowlist'::text, 'public'::text]))),
    CONSTRAINT channels_created_by_shape_check CHECK (public.valid_creation_provenance(created_by)),
    CONSTRAINT channels_external_response_review_override_check CHECK (((external_response_review_override IS NULL) OR (external_response_review_override = ANY (ARRAY['autonomous'::text, 'review_all_external'::text])))),
    CONSTRAINT channels_name_not_blank CHECK ((btrim(name) <> ''::text))
);


--
-- Name: company_members; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.company_members (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    user_id uuid NOT NULL,
    role text DEFAULT 'member'::text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT company_members_role_check CHECK ((role = ANY (ARRAY['owner'::text, 'member'::text, 'admin'::text])))
);


--
-- Name: principals; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.principals (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    kind text NOT NULL,
    user_id uuid,
    agent_id uuid,
    display_label text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT principals_display_label_check CHECK (((btrim(display_label) <> ''::text) AND (octet_length(display_label) <= 255))),
    CONSTRAINT principals_kind_check CHECK ((kind = ANY (ARRAY['person'::text, 'agent'::text, 'external'::text, 'system'::text]))),
    CONSTRAINT principals_shape_check CHECK ((((kind = 'person'::text) AND (user_id IS NOT NULL) AND (agent_id IS NULL)) OR ((kind = 'agent'::text) AND (user_id IS NULL) AND (agent_id IS NOT NULL)) OR ((kind = ANY (ARRAY['external'::text, 'system'::text])) AND (user_id IS NULL) AND (agent_id IS NULL))))
);


--
-- Name: notification_principal_can_view(uuid, uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_principal_can_view(checked_company uuid, checked_channel uuid, checked_principal uuid) RETURNS boolean
    LANGUAGE sql STABLE
    RETURN (EXISTS (SELECT 1 FROM ((public.principals principal JOIN public.company_members member ON (((member.company_id = principal.company_id) AND (member.user_id = principal.user_id)))) JOIN public.channels channel ON (((channel.company_id = principal.company_id) AND (channel.id = notification_principal_can_view.checked_channel)))) WHERE ((principal.company_id = notification_principal_can_view.checked_company) AND (principal.id = notification_principal_can_view.checked_principal) AND (principal.kind = 'person'::text) AND ((member.role = 'owner'::text) OR (channel.access_mode = ANY (ARRAY['team'::text, 'public'::text])) OR (EXISTS (SELECT 1 FROM public.channel_principal_grants permission WHERE ((permission.company_id = notification_principal_can_view.checked_company) AND (permission.channel_id = notification_principal_can_view.checked_channel) AND (permission.principal_id = notification_principal_can_view.checked_principal) AND (permission.capability = 'view'::text))))))));


--
-- Name: notification_recheck_channel_access(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_recheck_channel_access() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    row_data JSONB := CASE WHEN TG_OP = 'DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END;
    changed_company UUID := (row_data->>'company_id')::UUID;
    changed_channel UUID := COALESCE(
        (row_data->>'channel_id')::UUID,
        (row_data->>'id')::UUID
    );
BEGIN
    PERFORM 1 FROM channels AS channel
     WHERE channel.company_id = changed_company AND channel.id = changed_channel
     FOR UPDATE;
    PERFORM withdraw_unauthorized_notifications(changed_company, changed_channel);
    RETURN COALESCE(NEW, OLD);
END;
$$;


--
-- Name: notification_recheck_membership(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_recheck_membership() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    changed_company UUID := COALESCE(NEW.company_id, OLD.company_id);
    changed_user UUID := COALESCE(NEW.user_id, OLD.user_id);
BEGIN
    UPDATE notifications AS notification
       SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
           updated_at = CURRENT_TIMESTAMP
     WHERE notification.company_id = changed_company
       AND notification.recipient_user_id = changed_user
       AND notification.state = 'active'
       AND (
           notification.recipient_principal_id IS NULL
           OR NOT notification_principal_can_view(
               notification.company_id,
               notification.channel_id,
               notification.recipient_principal_id
           )
       );
    RETURN COALESCE(NEW, OLD);
END;
$$;


--
-- Name: notification_withdraw_deleted_principal(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_withdraw_deleted_principal() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    UPDATE notifications
       SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
           updated_at = CURRENT_TIMESTAMP
     WHERE company_id = OLD.company_id AND recipient_principal_id = OLD.id
       AND state = 'active';
    RETURN OLD;
END;
$$;


--
-- Name: notification_withdraw_deleted_source(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notification_withdraw_deleted_source() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: notify_actionable_notification_changed(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_actionable_notification_changed() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: notify_agent_instruction(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_agent_instruction() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
$$;


--
-- Name: notify_attention_changed(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_attention_changed() RETURNS trigger
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
         WHERE task.id = NULLIF(row_data->>'task_id', '')::UUID;
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


--
-- Name: notify_inbound_event_ready(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_inbound_event_ready() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.status IN ('pending', 'retryable')
       AND (TG_OP = 'INSERT' OR OLD.status IS DISTINCT FROM NEW.status
            OR OLD.available_at IS DISTINCT FROM NEW.available_at) THEN
        PERFORM pg_notify('inbound_event_ready', NEW.id::TEXT);
    END IF;
    RETURN NULL;
END;
$$;


--
-- Name: notify_internal_note_change(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_internal_note_change() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
$$;


--
-- Name: notify_task_chain_changed(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_task_chain_changed() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    notified_company_id UUID;
    notified_correlation_id UUID;
BEGIN
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
$$;


--
-- Name: notify_task_ownership(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_task_ownership() RETURNS trigger
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
$$;


--
-- Name: notify_thread_activity(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_thread_activity() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM pg_notify(
        'thread_activity',
        json_build_object(
            'thread_id', NEW.thread_id,
            'channel_id', NEW.channel_id,
            'company_id', NEW.company_id
        )::text
    );
    RETURN NULL;
END;
$$;


--
-- Name: notify_thread_message(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_thread_message() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM pg_notify(
        'thread_message',
        json_build_object(
            'thread_id', NEW.thread_id,
            'channel_id', NEW.channel_id,
            'company_id', NEW.company_id
        )::text
    );
    RETURN NULL;
END;
$$;


--
-- Name: prevent_assigned_library_agent_delete(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.prevent_assigned_library_agent_delete() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.company_id IS NULL
       AND EXISTS (SELECT 1 FROM channel_agents WHERE agent_id = OLD.id) THEN
        RAISE EXCEPTION 'library agent is assigned to one or more channels'
            USING ERRCODE = '23503';
    END IF;
    RETURN OLD;
END;
$$;


--
-- Name: prevent_assigned_library_skill_delete(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.prevent_assigned_library_skill_delete() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.company_id IS NULL
       AND EXISTS (SELECT 1 FROM agent_skills WHERE skill_id = OLD.id) THEN
        RAISE EXCEPTION 'library skill is assigned to one or more agents'
            USING ERRCODE = '23503', CONSTRAINT = 'library_skill_delete_guard';
    END IF;
    RETURN OLD;
END;
$$;


--
-- Name: prevent_message_audience_widening(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.prevent_message_audience_widening() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.audience <> 'external_conversation'
       AND NEW.audience = 'external_conversation' THEN
        RAISE EXCEPTION 'message audience cannot be widened in place'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: prevent_owned_channel_delete(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.prevent_owned_channel_delete() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.owner_agent_id IS NOT NULL
       AND EXISTS (SELECT 1 FROM agents WHERE id = OLD.owner_agent_id) THEN
        RAISE EXCEPTION 'owned channel must be deleted through its owner agent'
            USING ERRCODE = '23503';
    END IF;
    RETURN OLD;
END;
$$;


--
-- Name: record_initial_task_ownership(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.record_initial_task_ownership() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
$$;


--
-- Name: record_task_status_event(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.record_task_status_event() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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

    transition_reason := NEW.transition_reason;
    transition_actor_kind := NEW.transition_actor_kind;
    transition_actor_id := NEW.transition_actor_id;
    approval_id := NEW.transition_approval_id;
    outreach_id := NEW.transition_outreach_id;

    IF transition_reason IS NULL THEN
        transition_reason := CASE
            WHEN TG_OP = 'INSERT' THEN 'enqueued'
            WHEN OLD.status = 'pending' AND NEW.status = 'processing' THEN 'claimed'
            WHEN OLD.status = 'processing' AND NEW.status = 'completed' THEN 'completed'
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
            -- Nothing above established a cause, so none is claimed. `retryable_failure` used to
            -- stand here, which turned every unattributed transition into a fabricated worker
            -- failure -- an operator resuming a dead-lettered task was filed as the worker failing
            -- it again. A row that says "unclassified" is greppable; a row that says the wrong
            -- thing is not.
            ELSE 'unknown'
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
$$;


--
-- Name: reject_binding_audit_rewrite(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.reject_binding_audit_rewrite() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'UPDATE' OR pg_trigger_depth() <= 1 THEN
        RAISE EXCEPTION 'binding_audit_events is append-only' USING ERRCODE = '23514';
    END IF;
    RETURN OLD;
END;
$$;


--
-- Name: release_tasks_for_removed_principal(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.release_tasks_for_removed_principal() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
$$;


--
-- Name: retire_memory_lifecycle_for_legacy_connection(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.retire_memory_lifecycle_for_legacy_connection() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    cleanup_available_at TIMESTAMPTZ;
BEGIN
    UPDATE memory_remote_resource_lifecycles
    SET company_id = NULL,
        desired_state = 'absent',
        quiesce_until = GREATEST(
            quiesce_until,
            CURRENT_TIMESTAMP + INTERVAL '180 seconds'
        ),
        last_error = NULL,
        updated_at = CURRENT_TIMESTAMP
    WHERE provider = OLD.provider
      AND remote_database_id = OLD.remote_database_id
      AND (desired_state <> 'absent' OR company_id IS NOT NULL)
    RETURNING quiesce_until INTO cleanup_available_at;

    IF FOUND THEN
        INSERT INTO memory_cleanup_jobs
            (id, provider, remote_database_id, available_at)
        VALUES (
            md5(OLD.provider || ':' || OLD.remote_database_id)::uuid,
            OLD.provider,
            OLD.remote_database_id,
            cleanup_available_at
        )
        ON CONFLICT (provider, remote_database_id) DO UPDATE
        SET status = 'pending',
            attempts = 0,
            available_at = EXCLUDED.available_at,
            lease_token = NULL,
            lease_expires_at = NULL,
            operation_generation = NULL,
            last_error = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE memory_cleanup_jobs.status <> 'leased';
    END IF;
    RETURN OLD;
END;
$$;


--
-- Name: synchronize_legacy_memory_provisioning_phase(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.synchronize_legacy_memory_provisioning_phase() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.status = 'completed' AND NEW.phase NOT IN ('ready', 'failed') THEN
        NEW.phase = 'ready';
    ELSIF NEW.status = 'failed' AND NEW.phase <> 'failed' THEN
        NEW.phase = 'failed';
    ELSIF NEW.status = 'pending' AND OLD.status IN ('completed', 'failed')
            AND NEW.phase IN ('ready', 'failed') THEN
        NEW.phase = 'create_pending';
        NEW.failure_attempts = 0;
        NEW.readiness_deadline = NULL;
        NEW.next_poll_at = NULL;
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: task_ownership_events_are_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.task_ownership_events_are_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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
$$;


--
-- Name: transport_requires_installation(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.transport_requires_installation(transport text) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN (transport = 'slack'::text);


--
-- Name: valid_binding_change_reason(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_binding_change_reason(reason text) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN (reason = ANY (ARRAY['manager_request'::text, 'installation_revoked'::text, 'endpoint_removed'::text, 'access_revoked'::text, 'channel_disabled'::text, 'provider_drift'::text]));


--
-- Name: valid_delivery_failure_class(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_delivery_failure_class(class text) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN (class = ANY (ARRAY['authentication'::text, 'rate_limited'::text, 'invalid_payload'::text, 'destination_unavailable'::text, 'network'::text, 'timeout'::text, 'provider_fault'::text, 'internal'::text, 'dependency_failed'::text, 'superseded'::text, 'lease_expired'::text]));


--
-- Name: valid_inbound_event_error_class(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_inbound_event_error_class(class text) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN (class = ANY (ARRAY['decode'::text, 'invalid_payload'::text, 'routing'::text, 'dependency'::text, 'rate_limited'::text, 'provider_fault'::text, 'deadline'::text, 'internal'::text, 'unsupported_transport'::text, 'lease_expired'::text]));


--
-- Name: valid_inbound_event_ignore_reason(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_inbound_event_ignore_reason(reason text) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN (reason = ANY (ARRAY['not_message'::text, 'unsupported_event'::text, 'unsupported_subtype'::text, 'automated_sender'::text, 'empty_content'::text, 'inactive_binding'::text, 'delivery_confirmation'::text]));


--
-- Name: valid_inbound_safe_header_facts(jsonb); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_inbound_safe_header_facts(facts jsonb) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN ((jsonb_typeof(facts) = 'object'::text) AND (SELECT (count(*) <= 16) FROM jsonb_object_keys(valid_inbound_safe_header_facts.facts) jsonb_object_keys(jsonb_object_keys)) AND (octet_length((facts)::text) <= 4096) AND (NOT (EXISTS (SELECT 1 FROM jsonb_each(valid_inbound_safe_header_facts.facts) fact(name, value) WHERE ((fact.name !~ '^[a-z0-9_]{1,64}$'::text) OR (jsonb_typeof(fact.value) <> 'string'::text) OR ((octet_length((fact.value #>> '{}'::text[])) < 1) OR (octet_length((fact.value #>> '{}'::text[])) > 256)) OR ((fact.value #>> '{}'::text[]) ~ '[[:cntrl:]]'::text))))));


--
-- Name: valid_tool_id_array(text[]); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_tool_id_array(ids text[]) RETURNS boolean
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    AS $$
    SELECT cardinality(ids) <= 32
       AND array_position(ids, NULL) IS NULL
       AND COALESCE(bool_and(btrim(id) <> '' AND char_length(id) <= 120), TRUE)
    FROM unnest(ids) AS id;
$$;


--
-- Name: withdraw_unauthorized_notifications(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.withdraw_unauthorized_notifications(changed_company uuid, changed_channel uuid) RETURNS void
    LANGUAGE sql
    AS $$
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


--
-- Name: agent_channel_provisions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.agent_channel_provisions (
    task_id uuid NOT NULL,
    request_hash text NOT NULL,
    agent_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    warnings jsonb DEFAULT '[]'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);


--
-- Name: agent_mcp_selection_revisions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.agent_mcp_selection_revisions (
    company_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    revision bigint DEFAULT 1 NOT NULL,
    CONSTRAINT agent_mcp_selection_revisions_revision_check CHECK ((revision > 0))
);


--
-- Name: agent_mcp_selections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.agent_mcp_selections (
    company_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    connection_id uuid NOT NULL
);


--
-- Name: agent_skills; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.agent_skills (
    company_id uuid,
    agent_id uuid NOT NULL,
    skill_id uuid NOT NULL,
    "position" integer NOT NULL,
    CONSTRAINT agent_skills_position_check CHECK ((("position" >= 0) AND ("position" <= 15)))
);


--
-- Name: agent_sub_agents; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.agent_sub_agents (
    company_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    sub_agent_id uuid NOT NULL,
    "position" integer NOT NULL,
    CONSTRAINT agent_sub_agents_not_self CHECK ((agent_id <> sub_agent_id)),
    CONSTRAINT agent_sub_agents_position_check CHECK ((("position" >= 0) AND ("position" <= 63)))
);


--
-- Name: agents; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.agents (
    id uuid NOT NULL,
    company_id uuid,
    name text NOT NULL,
    slug public.citext NOT NULL,
    provider text,
    model text,
    system_prompt text,
    description text,
    config_json jsonb,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    avatar_url text,
    created_by jsonb NOT NULL,
    run_timeout_secs integer,
    memory_recall_mode text DEFAULT 'fast'::text NOT NULL,
    memory_max_results smallint DEFAULT 5 NOT NULL,
    memory_persistence_mode text DEFAULT 'audience_only'::text NOT NULL,
    memory_enabled boolean DEFAULT false NOT NULL,
    harness_kind text DEFAULT 'rig'::text NOT NULL,
    granted_tool_ids text[] DEFAULT '{}'::text[] NOT NULL,
    native_tool_policy jsonb DEFAULT '{"version": 1}'::jsonb NOT NULL,
    response_contract jsonb,
    CONSTRAINT agents_avatar_url_scheme_check CHECK (((avatar_url IS NULL) OR (avatar_url ~ '^https?://'::text))),
    CONSTRAINT agents_config_v1_shape_check CHECK (((config_json IS NULL) OR ((jsonb_typeof(config_json) = 'object'::text) AND ((config_json ->> 'version'::text) = '1'::text) AND (octet_length((config_json)::text) <= 65536)))),
    CONSTRAINT agents_created_by_shape_check CHECK (public.valid_creation_provenance(created_by)),
    CONSTRAINT agents_granted_tool_ids_bounded CHECK (public.valid_tool_id_array(granted_tool_ids)),
    CONSTRAINT agents_harness_kind_check CHECK ((harness_kind = ANY (ARRAY['ai_agents'::text, 'rig'::text]))),
    CONSTRAINT agents_memory_max_results_check CHECK (((memory_max_results >= 1) AND (memory_max_results <= 20))),
    CONSTRAINT agents_memory_persistence_mode_check CHECK ((memory_persistence_mode = ANY (ARRAY['audience_only'::text, 'scope_specific_facts'::text]))),
    CONSTRAINT agents_memory_recall_mode_check CHECK ((memory_recall_mode = ANY (ARRAY['fast'::text, 'thinking'::text]))),
    CONSTRAINT agents_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT agents_native_tool_policy_shape CHECK (((jsonb_typeof(native_tool_policy) = 'object'::text) AND ((native_tool_policy ->> 'version'::text) = '1'::text) AND (octet_length((native_tool_policy)::text) <= 16384))),
    CONSTRAINT agents_response_contract CHECK (((response_contract IS NULL) OR COALESCE(((harness_kind = 'rig'::text) AND (jsonb_typeof(response_contract) = 'object'::text) AND ((response_contract ->> 'version'::text) = '1'::text) AND ((response_contract ->> 'format'::text) = 'json_schema'::text) AND (response_contract ? 'schema'::text) AND ((response_contract - ARRAY['version'::text, 'format'::text, 'schema'::text]) = '{}'::jsonb) AND (octet_length((response_contract)::text) <= 131072)), false))),
    CONSTRAINT agents_run_timeout_secs_check CHECK (((run_timeout_secs >= 1) AND (run_timeout_secs <= 3600))),
    CONSTRAINT agents_slug_format CHECK ((((slug)::text = lower((slug)::text)) AND ((slug)::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'::text)))
);


--
-- Name: attention_source_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.attention_source_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    company_id uuid NOT NULL,
    source_kind text NOT NULL,
    source_id uuid NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    operation text NOT NULL,
    actor_principal_id uuid NOT NULL,
    from_version bigint NOT NULL,
    to_version bigint NOT NULL,
    previous_priority text,
    new_priority text,
    previous_due_at timestamp with time zone,
    new_due_at timestamp with time zone,
    previous_responsible_principal_id uuid,
    new_responsible_principal_id uuid,
    occurred_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT attention_source_events_operation_check CHECK ((operation = ANY (ARRAY['created'::text, 'attributes_changed'::text, 'reassigned'::text, 'resolved'::text, 'withdrawn'::text]))),
    CONSTRAINT attention_source_events_priority_check CHECK ((((previous_priority IS NULL) OR (previous_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))) AND ((new_priority IS NULL) OR (new_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))))),
    CONSTRAINT attention_source_events_source_kind_check CHECK ((source_kind = ANY (ARRAY['task'::text, 'handoff'::text]))),
    CONSTRAINT attention_source_events_version_check CHECK (((from_version >= 0) AND (to_version = (from_version + 1))))
);


--
-- Name: background_tasks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.background_tasks (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid,
    source_message_uuid uuid,
    source_schedule_run_id uuid,
    correlation_id uuid NOT NULL,
    task_type text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    retry_count integer DEFAULT 0 NOT NULL,
    max_retries integer DEFAULT 3 NOT NULL,
    last_error text,
    worker_id uuid,
    execution_generation uuid,
    locked_at timestamp with time zone,
    lock_expires_at timestamp with time zone,
    wait_expires_at timestamp with time zone,
    run_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    transition_reason text,
    transition_actor_kind text,
    transition_actor_id uuid,
    transition_approval_id uuid,
    transition_outreach_id uuid,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    owner_principal_id uuid,
    owner_principal_kind text,
    ownership_version bigint DEFAULT 1 NOT NULL,
    business_priority text DEFAULT 'normal'::text NOT NULL,
    business_due_at timestamp with time zone,
    attention_version bigint DEFAULT 1 NOT NULL,
    awaited_outreach_id uuid,
    CONSTRAINT background_tasks_attention_version_check CHECK ((attention_version > 0)),
    CONSTRAINT background_tasks_business_priority_check CHECK ((business_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))),
    CONSTRAINT background_tasks_lease_check CHECK ((((status = 'processing'::text) AND (worker_id IS NOT NULL) AND (execution_generation IS NOT NULL) AND (locked_at IS NOT NULL) AND (lock_expires_at IS NOT NULL) AND (lock_expires_at > locked_at)) OR ((status <> 'processing'::text) AND (worker_id IS NULL) AND (execution_generation IS NULL) AND (locked_at IS NULL) AND (lock_expires_at IS NULL)))),
    CONSTRAINT background_tasks_max_retries_check CHECK ((max_retries > 0)),
    CONSTRAINT background_tasks_owner_shape_check CHECK ((((owner_principal_id IS NULL) AND (owner_principal_kind IS NULL)) OR ((owner_principal_id IS NOT NULL) AND (owner_principal_kind = ANY (ARRAY['person'::text, 'agent'::text]))))),
    CONSTRAINT background_tasks_ownership_version_check CHECK ((ownership_version > 0)),
    CONSTRAINT background_tasks_payload_object_check CHECK ((jsonb_typeof(payload) = 'object'::text)),
    CONSTRAINT background_tasks_retry_count_check CHECK ((retry_count >= 0)),
    CONSTRAINT background_tasks_single_source_check CHECK (((source_message_uuid IS NULL) OR (source_schedule_run_id IS NULL))),
    CONSTRAINT background_tasks_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'processing'::text, 'pending_approval'::text, 'waiting_for_third_party_reply'::text, 'completed'::text, 'failed'::text, 'dead_letter'::text, 'stopped'::text]))),
    CONSTRAINT background_tasks_transition_actor_kind_check CHECK (((transition_actor_kind IS NULL) OR (transition_actor_kind = ANY (ARRAY['system'::text, 'worker'::text, 'operator'::text, 'human'::text, 'agent'::text, 'approval'::text, 'outreach'::text])))),
    CONSTRAINT background_tasks_transition_reason_check CHECK (((transition_reason IS NULL) OR (transition_reason = ANY (ARRAY['enqueued'::text, 'claimed'::text, 'completed'::text, 'retryable_failure'::text, 'terminal_failure'::text, 'timed_out'::text, 'shutdown'::text, 'lease_lost'::text, 'approval_requested'::text, 'approval_accepted'::text, 'approval_rejected'::text, 'outreach_started'::text, 'outreach_reply_received'::text, 'outreach_timed_out'::text, 'outreach_extended'::text, 'operator_stopped'::text, 'operator_resumed'::text, 'ownership_transferred'::text, 'agent_instruction'::text, 'delegation_target_cancelled'::text, 'delegation_cancelled'::text, 'delegation_reassigned'::text, 'delegation_partial'::text, 'unknown'::text])))),
    CONSTRAINT background_tasks_transition_shape_check CHECK ((((transition_reason IS NULL) AND (transition_actor_kind IS NULL) AND (transition_actor_id IS NULL) AND (transition_approval_id IS NULL) AND (transition_outreach_id IS NULL)) OR ((transition_reason IS NOT NULL) AND
CASE transition_actor_kind
    WHEN 'system'::text THEN ((transition_actor_id IS NULL) AND (transition_approval_id IS NULL) AND (transition_outreach_id IS NULL))
    WHEN 'worker'::text THEN ((transition_actor_id IS NOT NULL) AND (transition_approval_id IS NULL) AND (transition_outreach_id IS NULL))
    WHEN 'operator'::text THEN ((transition_actor_id IS NOT NULL) AND (transition_approval_id IS NULL) AND (transition_outreach_id IS NULL))
    WHEN 'human'::text THEN ((transition_actor_id IS NOT NULL) AND (transition_approval_id IS NULL) AND (transition_outreach_id IS NULL))
    WHEN 'agent'::text THEN ((transition_actor_id IS NOT NULL) AND (transition_approval_id IS NULL) AND (transition_outreach_id IS NULL))
    WHEN 'approval'::text THEN ((transition_actor_id IS NULL) AND (transition_approval_id IS NOT NULL) AND (transition_outreach_id IS NULL))
    WHEN 'outreach'::text THEN ((transition_actor_id IS NULL) AND (transition_approval_id IS NULL) AND (transition_outreach_id IS NOT NULL))
    ELSE false
END)))
);


--
-- Name: binding_audit_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.binding_audit_events (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    action text NOT NULL,
    reason text,
    actor jsonb NOT NULL,
    metadata jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT binding_audit_events_action_check CHECK ((action = ANY (ARRAY['linked'::text, 'endpoint_changed'::text, 'enabled'::text, 'paused'::text, 'disabled'::text, 'drift_detected'::text, 'unlinked'::text]))),
    CONSTRAINT binding_audit_events_actor_check CHECK (public.valid_creation_provenance(actor)),
    CONSTRAINT binding_audit_events_metadata_check CHECK (((jsonb_typeof(metadata) = 'object'::text) AND ((metadata -> 'version'::text) = '1'::jsonb) AND (jsonb_typeof((metadata -> 'transport'::text)) = 'string'::text) AND (octet_length((metadata)::text) <= 4096))),
    CONSTRAINT binding_audit_events_reason_check CHECK (((reason IS NULL) OR public.valid_binding_change_reason(reason)))
);


--
-- Name: channel_agents; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.channel_agents (
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    "position" integer NOT NULL,
    CONSTRAINT channel_agents_position_check CHECK (("position" >= 0))
);


--
-- Name: channel_bindings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.channel_bindings (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    installation_id uuid,
    transport text NOT NULL,
    namespace text NOT NULL,
    external_endpoint_key text NOT NULL,
    display_label text NOT NULL,
    access_policy text NOT NULL,
    delivery_policy text NOT NULL,
    status text NOT NULL,
    disabled_reason text,
    created_by jsonb NOT NULL,
    access_snapshot jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT channel_bindings_access_policy_check CHECK ((access_policy = ANY (ARRAY['channel_acl'::text, 'conversation_members_read_and_participate'::text]))),
    CONSTRAINT channel_bindings_access_snapshot_check CHECK (((jsonb_typeof(access_snapshot) = 'object'::text) AND ((access_snapshot -> 'version'::text) = '1'::jsonb) AND (jsonb_typeof((access_snapshot -> 'kind'::text)) = 'string'::text) AND ((access_snapshot ->> 'kind'::text) = ANY (ARRAY['deployment_endpoint'::text, 'provider_conversation'::text])) AND (octet_length((access_snapshot)::text) <= 4096))),
    CONSTRAINT channel_bindings_created_by_check CHECK (public.valid_creation_provenance(created_by)),
    CONSTRAINT channel_bindings_delivery_policy_check CHECK ((delivery_policy = ANY (ARRAY['reply_only'::text, 'reply_and_initiate'::text]))),
    CONSTRAINT channel_bindings_disabled_reason_check CHECK ((((status = ANY (ARRAY['disabled'::text, 'orphaned'::text])) = (disabled_reason IS NOT NULL)) AND ((disabled_reason IS NULL) OR public.valid_binding_change_reason(disabled_reason)))),
    CONSTRAINT channel_bindings_display_label_check CHECK (((btrim(display_label) <> ''::text) AND (octet_length(display_label) <= 255))),
    CONSTRAINT channel_bindings_endpoint_key_check CHECK (((btrim(external_endpoint_key) <> ''::text) AND (octet_length(external_endpoint_key) <= 512))),
    CONSTRAINT channel_bindings_installation_coherence_check CHECK ((public.transport_requires_installation(transport) = (installation_id IS NOT NULL))),
    CONSTRAINT channel_bindings_namespace_check CHECK (((btrim(namespace) <> ''::text) AND (octet_length(namespace) <= 255))),
    CONSTRAINT channel_bindings_status_check CHECK ((status = ANY (ARRAY['active'::text, 'paused'::text, 'disabled'::text, 'orphaned'::text]))),
    CONSTRAINT channel_bindings_transport_check CHECK ((transport = ANY (ARRAY['email'::text, 'slack'::text])))
);


--
-- Name: channel_schedules; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.channel_schedules (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    name text NOT NULL,
    schedule_type text NOT NULL,
    interval_seconds bigint,
    subject_template text NOT NULL,
    prompt_template text NOT NULL,
    delivery_mode text DEFAULT 'mailbox_only'::text NOT NULL,
    recipient_emails public.citext[] DEFAULT '{}'::public.citext[] NOT NULL,
    timezone text DEFAULT 'UTC'::text NOT NULL,
    run_as_user_id uuid,
    enabled boolean DEFAULT true NOT NULL,
    last_run_at timestamp with time zone,
    next_run_at timestamp with time zone,
    last_error text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT channel_schedules_delivery_mode_check CHECK ((delivery_mode = ANY (ARRAY['mailbox_only'::text, 'email_participants'::text, 'email_custom'::text]))),
    CONSTRAINT channel_schedules_interval_check CHECK ((((schedule_type = 'interval'::text) AND (interval_seconds IS NOT NULL) AND (interval_seconds >= 60)) OR ((schedule_type = 'one_off'::text) AND (interval_seconds IS NULL)))),
    CONSTRAINT channel_schedules_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT channel_schedules_timezone_check CHECK (((now() AT TIME ZONE timezone) IS NOT NULL)),
    CONSTRAINT channel_schedules_type_check CHECK ((schedule_type = ANY (ARRAY['interval'::text, 'one_off'::text])))
);


--
-- Name: channel_slugs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.channel_slugs (
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    slug public.citext NOT NULL,
    is_primary boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT channel_slugs_format CHECK ((((slug)::text = lower((slug)::text)) AND ((slug)::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'::text)))
);


--
-- Name: companies; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.companies (
    id uuid NOT NULL,
    user_id uuid NOT NULL,
    name text NOT NULL,
    slug public.citext NOT NULL,
    enable_llm_spam_guardrail boolean,
    default_add_3rd_party boolean DEFAULT true NOT NULL,
    default_participant_emails public.citext[],
    default_retrieve_company_memory boolean DEFAULT false NOT NULL,
    default_retrieve_agent_memory boolean DEFAULT false NOT NULL,
    default_retrieve_user_memory boolean DEFAULT false NOT NULL,
    default_persist_company_memory boolean DEFAULT false NOT NULL,
    default_persist_agent_memory boolean DEFAULT false NOT NULL,
    default_persist_user_memory boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    avatar_url text,
    memory_provider text,
    external_response_review text DEFAULT 'autonomous'::text NOT NULL,
    CONSTRAINT companies_avatar_url_scheme_check CHECK (((avatar_url IS NULL) OR (avatar_url ~ '^https?://'::text))),
    CONSTRAINT companies_default_participants_bounded CHECK (((default_participant_emails IS NULL) OR ((cardinality(default_participant_emails) <= 64) AND (array_position(default_participant_emails, NULL::public.citext) IS NULL) AND (array_position(default_participant_emails, ''::public.citext) IS NULL)))),
    CONSTRAINT companies_external_response_review_check CHECK ((external_response_review = ANY (ARRAY['autonomous'::text, 'review_all_external'::text]))),
    CONSTRAINT companies_memory_provider_check CHECK (((memory_provider IS NULL) OR (memory_provider = ANY (ARRAY['hydradb'::text, 'hindsight'::text])))),
    CONSTRAINT companies_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT companies_slug_format CHECK ((((slug)::text = lower((slug)::text)) AND ((slug)::text ~ '^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$'::text)))
);


--
-- Name: company_invites; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.company_invites (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    email public.citext NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    role text DEFAULT 'member'::text NOT NULL,
    CONSTRAINT company_invites_role_check CHECK ((role = ANY (ARRAY['member'::text, 'admin'::text]))),
    CONSTRAINT company_invites_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'accepted'::text, 'declined'::text])))
);


--
-- Name: company_mcp_connections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.company_mcp_connections (
    company_id uuid NOT NULL,
    id uuid NOT NULL,
    slug text NOT NULL,
    endpoint_url text NOT NULL,
    transport text DEFAULT 'streamable_http'::text NOT NULL,
    enabled boolean DEFAULT false NOT NULL,
    auth_type text NOT NULL,
    revision bigint DEFAULT 1 NOT NULL,
    credential_revision bigint DEFAULT 1 NOT NULL,
    discovery_json jsonb DEFAULT '[]'::jsonb NOT NULL,
    deleted_at timestamp with time zone,
    CONSTRAINT company_mcp_connections_auth_type_check CHECK ((auth_type = ANY (ARRAY['none'::text, 'bearer'::text]))),
    CONSTRAINT company_mcp_connections_credential_revision_check CHECK ((credential_revision > 0)),
    CONSTRAINT company_mcp_connections_discovery_json_check CHECK (((jsonb_typeof(discovery_json) = 'array'::text) AND (jsonb_array_length(discovery_json) <= 100) AND (octet_length((discovery_json)::text) <= 1048576))),
    CONSTRAINT company_mcp_connections_endpoint_url_check CHECK (((octet_length(endpoint_url) >= 1) AND (octet_length(endpoint_url) <= 2048))),
    CONSTRAINT company_mcp_connections_revision_check CHECK ((revision > 0)),
    CONSTRAINT company_mcp_connections_slug_check CHECK (((length(slug) >= 1) AND (length(slug) <= 100))),
    CONSTRAINT company_mcp_connections_transport_check CHECK ((transport = 'streamable_http'::text))
);


--
-- Name: company_mcp_credentials; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.company_mcp_credentials (
    company_id uuid NOT NULL,
    connection_id uuid NOT NULL,
    envelope text NOT NULL,
    CONSTRAINT company_mcp_credentials_envelope_check CHECK (((envelope ~~ 'enc:v2:%'::text) AND (octet_length(envelope) <= 24000)))
);


--
-- Name: company_mcp_tool_grants; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.company_mcp_tool_grants (
    company_id uuid NOT NULL,
    connection_id uuid NOT NULL,
    tool_name text NOT NULL,
    CONSTRAINT company_mcp_tool_grants_tool_name_check CHECK (((octet_length(tool_name) >= 1) AND (octet_length(tool_name) <= 256)))
);


--
-- Name: company_model_connections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.company_model_connections (
    company_id uuid NOT NULL,
    provider text NOT NULL,
    api_key text NOT NULL,
    models text[] NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT company_model_connections_api_key_check CHECK (((btrim(api_key) <> ''::text) AND (octet_length(api_key) <= 16384))),
    CONSTRAINT company_model_connections_models_count_check CHECK (((cardinality(models) >= 1) AND (cardinality(models) <= 32))),
    CONSTRAINT company_model_connections_models_have_no_nulls CHECK (((array_position(models, NULL::text) IS NULL) AND (array_position(models, ''::text) IS NULL))),
    CONSTRAINT company_model_connections_provider_check CHECK (((provider = ANY (ARRAY['google'::text, 'openai'::text, 'anthropic'::text, 'groq'::text])) AND (length(provider) <= 64)))
);


--
-- Name: company_resend_api_integrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.company_resend_api_integrations (
    company_id uuid NOT NULL,
    webhook_token text NOT NULL,
    api_key text NOT NULL,
    signing_secret text NOT NULL,
    authserv_id text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT company_resend_api_integrations_api_key_check CHECK (((api_key ~ '^enc:v2:[1-9][0-9]{0,8}(:[A-Za-z0-9+/]+={0,2}){4}$'::text) AND (octet_length(api_key) <= 8192))),
    CONSTRAINT company_resend_api_integrations_authserv_id_check CHECK (((btrim(authserv_id) <> ''::text) AND (authserv_id !~ '[[:space:]]'::text) AND (octet_length(authserv_id) <= 255))),
    CONSTRAINT company_resend_api_integrations_signing_secret_check CHECK (((signing_secret ~ '^enc:v2:[1-9][0-9]{0,8}(:[A-Za-z0-9+/]+={0,2}){4}$'::text) AND (octet_length(signing_secret) <= 8192))),
    CONSTRAINT company_resend_api_integrations_webhook_token_check CHECK ((webhook_token ~ '^[a-z0-9]{32}$'::text))
);


--
-- Name: delegation_control_commands; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.delegation_control_commands (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    company_id uuid NOT NULL,
    task_id uuid NOT NULL,
    outreach_id uuid NOT NULL,
    target_id uuid,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    operation text NOT NULL,
    actor_principal_id uuid NOT NULL,
    actor_kind text NOT NULL,
    authority text NOT NULL,
    reason text NOT NULL,
    reason_detail text,
    from_version bigint NOT NULL,
    to_version bigint NOT NULL,
    result jsonb NOT NULL,
    occurred_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT delegation_control_commands_actor_kind_check CHECK ((actor_kind = ANY (ARRAY['person'::text, 'agent'::text]))),
    CONSTRAINT delegation_control_commands_authority_check CHECK ((authority = ANY (ARRAY['human_owner'::text, 'company_manager'::text, 'owning_agent'::text]))),
    CONSTRAINT delegation_control_commands_operation_check CHECK ((operation = ANY (ARRAY['extend_outreach'::text, 'cancel_target'::text, 'cancel_outreach'::text, 'reassign_internal_target'::text, 'proceed_with_partial'::text, 'stop_task'::text]))),
    CONSTRAINT delegation_control_commands_reason_check CHECK ((reason = ANY (ARRAY['deadline_changed'::text, 'no_longer_needed'::text, 'target_unavailable'::text, 'incorrect_target'::text, 'partial_results_accepted'::text, 'task_stopped'::text, 'other'::text]))),
    CONSTRAINT delegation_control_commands_reason_detail_check CHECK (((reason_detail IS NULL) OR (octet_length(reason_detail) <= 512))),
    CONSTRAINT delegation_control_commands_result_version_check CHECK (((result ->> 'version'::text) = '1'::text)),
    CONSTRAINT delegation_control_commands_version_check CHECK (((from_version > 0) AND (to_version = (from_version + 1))))
);


--
-- Name: email_message_metadata; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.email_message_metadata (
    company_id uuid NOT NULL,
    message_id uuid NOT NULL,
    rfc_message_id text NOT NULL,
    in_reply_to text,
    references_list text[] DEFAULT '{}'::text[] NOT NULL,
    thread_index text,
    raw_text_body text,
    raw_html_body text,
    CONSTRAINT email_message_metadata_in_reply_to_check CHECK (((in_reply_to IS NULL) OR (octet_length(in_reply_to) <= 998))),
    CONSTRAINT email_message_metadata_references_check CHECK (((array_length(references_list, 1) IS NULL) OR (array_length(references_list, 1) <= 100))),
    CONSTRAINT email_message_metadata_rfc_message_id_check CHECK (((btrim(rfc_message_id) <> ''::text) AND (octet_length(rfc_message_id) <= 998))),
    CONSTRAINT email_message_metadata_thread_index_check CHECK (((thread_index IS NULL) OR (octet_length(thread_index) <= 998)))
);


--
-- Name: external_messages; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.external_messages (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    external_message_key text NOT NULL,
    message_id uuid NOT NULL,
    delivery_part_id uuid,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT external_messages_key_check CHECK (((btrim(external_message_key) <> ''::text) AND (octet_length(external_message_key) <= 998)))
);


--
-- Name: external_threads; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.external_threads (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    external_thread_key text NOT NULL,
    thread_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT external_threads_key_check CHECK (((btrim(external_thread_key) <> ''::text) AND (octet_length(external_thread_key) <= 998)))
);


--
-- Name: human_approvals; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.human_approvals (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    task_id uuid,
    step_key text NOT NULL,
    approver_email public.citext NOT NULL,
    action_type text NOT NULL,
    action_title text NOT NULL,
    action_summary text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    token uuid NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    approver_principal_id uuid,
    expiry_retry_at timestamp with time zone,
    CONSTRAINT human_approvals_expiry_check CHECK ((expires_at > created_at)),
    CONSTRAINT human_approvals_payload_object_check CHECK ((jsonb_typeof(payload) = 'object'::text)),
    CONSTRAINT human_approvals_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'approved'::text, 'rejected'::text, 'expired'::text])))
);


--
-- Name: human_task_completions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.human_task_completions (
    task_id uuid NOT NULL,
    company_id uuid NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    owner_principal_id uuid NOT NULL,
    ownership_version bigint NOT NULL,
    message_id uuid NOT NULL,
    completed_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT human_task_completions_ownership_version_check CHECK ((ownership_version > 0))
);


--
-- Name: inbound_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.inbound_events (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    installation_id uuid,
    transport text NOT NULL,
    external_event_key text NOT NULL,
    correlation_id uuid NOT NULL,
    raw_payload bytea NOT NULL,
    content_type text,
    content_hash bytea NOT NULL,
    safe_header_facts jsonb DEFAULT '{}'::jsonb NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    attempt_count integer DEFAULT 0 NOT NULL,
    max_attempts integer DEFAULT 5 NOT NULL,
    available_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    last_error_class text,
    last_error_detail text,
    ignore_reason text,
    execution_id uuid,
    owner_worker_id uuid,
    locked_at timestamp with time zone,
    lock_expires_at timestamp with time zone,
    received_at timestamp with time zone NOT NULL,
    processed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT inbound_events_attempt_check CHECK (((attempt_count >= 0) AND (max_attempts > 0) AND (attempt_count <= max_attempts))),
    CONSTRAINT inbound_events_content_hash_check CHECK ((octet_length(content_hash) = 32)),
    CONSTRAINT inbound_events_content_type_check CHECK (((content_type IS NULL) OR ((btrim(content_type) <> ''::text) AND (octet_length(content_type) <= 255) AND (content_type !~ '[[:cntrl:]]'::text)))),
    CONSTRAINT inbound_events_error_check CHECK ((((last_error_class IS NULL) OR public.valid_inbound_event_error_class(last_error_class)) AND ((last_error_detail IS NULL) OR (octet_length(last_error_detail) <= 512)) AND ((last_error_detail IS NULL) OR (last_error_class IS NOT NULL)) AND ((status = ANY (ARRAY['retryable'::text, 'dead_letter'::text])) = (last_error_class IS NOT NULL)))),
    CONSTRAINT inbound_events_external_event_key_check CHECK (((btrim(external_event_key) <> ''::text) AND (octet_length(external_event_key) <= 512))),
    CONSTRAINT inbound_events_ignore_check CHECK ((((status = 'ignored'::text) = (ignore_reason IS NOT NULL)) AND ((ignore_reason IS NULL) OR public.valid_inbound_event_ignore_reason(ignore_reason)))),
    CONSTRAINT inbound_events_installation_check CHECK ((public.transport_requires_installation(transport) = (installation_id IS NOT NULL))),
    CONSTRAINT inbound_events_lease_check CHECK ((((status = 'processing'::text) AND (execution_id IS NOT NULL) AND (owner_worker_id IS NOT NULL) AND (locked_at IS NOT NULL) AND (lock_expires_at IS NOT NULL) AND (lock_expires_at > locked_at)) OR ((status <> 'processing'::text) AND (execution_id IS NULL) AND (owner_worker_id IS NULL) AND (locked_at IS NULL) AND (lock_expires_at IS NULL)))),
    CONSTRAINT inbound_events_payload_check CHECK (((octet_length(raw_payload) >= 1) AND (octet_length(raw_payload) <= 1048576))),
    CONSTRAINT inbound_events_processed_check CHECK (((status = ANY (ARRAY['completed'::text, 'ignored'::text, 'dead_letter'::text])) = (processed_at IS NOT NULL))),
    CONSTRAINT inbound_events_safe_header_facts_check CHECK (public.valid_inbound_safe_header_facts(safe_header_facts)),
    CONSTRAINT inbound_events_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'processing'::text, 'retryable'::text, 'completed'::text, 'ignored'::text, 'dead_letter'::text]))),
    CONSTRAINT inbound_events_transport_check CHECK ((transport = ANY (ARRAY['email'::text, 'slack'::text])))
);


--
-- Name: integration_credentials; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.integration_credentials (
    company_id uuid NOT NULL,
    installation_id uuid NOT NULL,
    credential_kind text NOT NULL,
    envelope text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT integration_credentials_envelope_check CHECK (((envelope ~ '^enc:v2:[1-9][0-9]{0,8}(:[A-Za-z0-9+/]+={0,2}){4}$'::text) AND (octet_length(envelope) <= 8192))),
    CONSTRAINT integration_credentials_kind_check CHECK ((credential_kind = ANY (ARRAY['bot_access_token'::text, 'bot_refresh_token'::text, 'user_access_token'::text])))
);


--
-- Name: integration_installations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.integration_installations (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    transport text NOT NULL,
    external_tenant_key text NOT NULL,
    display_name text NOT NULL,
    status text NOT NULL,
    granted_scopes text[] DEFAULT '{}'::text[] NOT NULL,
    installed_by jsonb NOT NULL,
    installed_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_by jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    revoked_by jsonb,
    revoked_at timestamp with time zone,
    CONSTRAINT integration_installations_display_name_check CHECK (((btrim(display_name) <> ''::text) AND (octet_length(display_name) <= 255))),
    CONSTRAINT integration_installations_installed_by_check CHECK (public.valid_creation_provenance(installed_by)),
    CONSTRAINT integration_installations_revocation_check CHECK ((((status = 'revoked'::text) = (revoked_at IS NOT NULL)) AND ((revoked_at IS NULL) = (revoked_by IS NULL)) AND ((revoked_by IS NULL) OR public.valid_creation_provenance(revoked_by)))),
    CONSTRAINT integration_installations_scopes_check CHECK (((array_position(granted_scopes, NULL::text) IS NULL) AND (NOT (''::text = ANY (granted_scopes))) AND (COALESCE(array_length(granted_scopes, 1), 0) <= 64) AND (octet_length(array_to_string(granted_scopes, ','::text)) <= 4096))),
    CONSTRAINT integration_installations_status_check CHECK ((status = ANY (ARRAY['active'::text, 'reauthorization_required'::text, 'revoked'::text, 'disabled'::text]))),
    CONSTRAINT integration_installations_tenant_key_check CHECK (((btrim(external_tenant_key) <> ''::text) AND (octet_length(external_tenant_key) <= 255))),
    CONSTRAINT integration_installations_transport_check CHECK (public.transport_requires_installation(transport)),
    CONSTRAINT integration_installations_updated_by_check CHECK (public.valid_creation_provenance(updated_by))
);


--
-- Name: internal_note_tombstones; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.internal_note_tombstones (
    note_id uuid NOT NULL,
    company_id uuid NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    actor_principal_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);


--
-- Name: internal_notes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.internal_notes (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    message_id uuid NOT NULL,
    message_audience text DEFAULT 'internal_only'::text NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    author_principal_id uuid NOT NULL,
    provenance text NOT NULL,
    supersedes_note_id uuid,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT internal_notes_message_audience_check CHECK ((message_audience = 'internal_only'::text)),
    CONSTRAINT internal_notes_no_self_supersession CHECK ((id IS DISTINCT FROM supersedes_note_id)),
    CONSTRAINT internal_notes_provenance_check CHECK ((provenance = ANY (ARRAY['human_ui'::text, 'api'::text, 'integration'::text, 'email_quiet_ingress'::text])))
);


--
-- Name: manual_handoffs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.manual_handoffs (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid,
    correlation_id uuid,
    title text NOT NULL,
    next_action text NOT NULL,
    status text DEFAULT 'open'::text NOT NULL,
    responsible_principal_id uuid,
    business_priority text DEFAULT 'normal'::text NOT NULL,
    business_due_at timestamp with time zone,
    version bigint DEFAULT 1 NOT NULL,
    created_by_principal_id uuid,
    resolved_by_principal_id uuid,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    resolved_at timestamp with time zone,
    CONSTRAINT manual_handoffs_next_action_check CHECK (((btrim(next_action) <> ''::text) AND (octet_length(next_action) <= 2048))),
    CONSTRAINT manual_handoffs_priority_check CHECK ((business_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))),
    CONSTRAINT manual_handoffs_resolution_check CHECK ((((status = 'open'::text) AND (resolved_at IS NULL) AND (resolved_by_principal_id IS NULL)) OR ((status <> 'open'::text) AND (resolved_at IS NOT NULL)))),
    CONSTRAINT manual_handoffs_status_check CHECK ((status = ANY (ARRAY['open'::text, 'resolved'::text, 'withdrawn'::text]))),
    CONSTRAINT manual_handoffs_title_check CHECK (((btrim(title) <> ''::text) AND (octet_length(title) <= 512))),
    CONSTRAINT manual_handoffs_version_check CHECK ((version > 0))
);


--
-- Name: memory_cleanup_jobs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memory_cleanup_jobs (
    id uuid NOT NULL,
    provider text NOT NULL,
    remote_database_id text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    available_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    lease_expires_at timestamp with time zone,
    last_error text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    lease_token uuid,
    operation_generation bigint,
    CONSTRAINT memory_cleanup_jobs_generation_state_check CHECK ((((status = 'leased'::text) AND (operation_generation IS NOT NULL)) OR ((status <> 'leased'::text) AND (operation_generation IS NULL)))),
    CONSTRAINT memory_cleanup_jobs_lease_state_check CHECK ((((status = 'leased'::text) AND (lease_token IS NOT NULL) AND (lease_expires_at IS NOT NULL)) OR ((status <> 'leased'::text) AND (lease_token IS NULL) AND (lease_expires_at IS NULL)))),
    CONSTRAINT memory_cleanup_jobs_provider_check CHECK ((provider = ANY (ARRAY['hydradb'::text, 'hindsight'::text]))),
    CONSTRAINT memory_cleanup_jobs_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'leased'::text, 'completed'::text, 'failed'::text])))
);


--
-- Name: memory_provider_connections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memory_provider_connections (
    company_id uuid NOT NULL,
    provider text NOT NULL,
    remote_database_id text NOT NULL,
    readiness text DEFAULT 'pending'::text NOT NULL,
    last_error text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT memory_provider_connections_provider_check CHECK ((provider = ANY (ARRAY['hydradb'::text, 'hindsight'::text]))),
    CONSTRAINT memory_provider_connections_readiness_check CHECK ((readiness = ANY (ARRAY['pending'::text, 'provisioning'::text, 'ready'::text, 'failed'::text])))
);


--
-- Name: memory_provisioning_jobs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memory_provisioning_jobs (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    provider text NOT NULL,
    remote_database_id text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    available_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    lease_token uuid,
    lease_expires_at timestamp with time zone,
    last_error text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    operation_generation bigint,
    phase text DEFAULT 'create_pending'::text NOT NULL,
    failure_attempts integer DEFAULT 0 NOT NULL,
    readiness_deadline timestamp with time zone,
    next_poll_at timestamp with time zone,
    CONSTRAINT memory_provisioning_jobs_attempts_check CHECK ((attempts >= 0)),
    CONSTRAINT memory_provisioning_jobs_check CHECK ((((status = 'leased'::text) AND (lease_token IS NOT NULL) AND (lease_expires_at IS NOT NULL)) OR ((status <> 'leased'::text) AND (lease_token IS NULL) AND (lease_expires_at IS NULL)))),
    CONSTRAINT memory_provisioning_jobs_failure_attempts_check CHECK ((failure_attempts >= 0)),
    CONSTRAINT memory_provisioning_jobs_generation_state_check CHECK ((((status = 'leased'::text) AND (operation_generation IS NOT NULL)) OR ((status <> 'leased'::text) AND (operation_generation IS NULL)))),
    CONSTRAINT memory_provisioning_jobs_phase_check CHECK ((phase = ANY (ARRAY['create_pending'::text, 'waiting_ready'::text, 'ready'::text, 'failed'::text]))),
    CONSTRAINT memory_provisioning_jobs_phase_state_check CHECK ((((status = ANY (ARRAY['pending'::text, 'leased'::text])) AND (phase = ANY (ARRAY['create_pending'::text, 'waiting_ready'::text]))) OR ((status = 'completed'::text) AND (phase = 'ready'::text)) OR ((status = 'failed'::text) AND (phase = 'failed'::text)))),
    CONSTRAINT memory_provisioning_jobs_provider_check CHECK ((provider = ANY (ARRAY['hydradb'::text, 'hindsight'::text]))),
    CONSTRAINT memory_provisioning_jobs_readiness_window_check CHECK ((((phase = 'create_pending'::text) AND (readiness_deadline IS NULL) AND (next_poll_at IS NULL)) OR ((phase = 'waiting_ready'::text) AND (readiness_deadline IS NOT NULL) AND (next_poll_at IS NOT NULL)) OR (phase = ANY (ARRAY['ready'::text, 'failed'::text])))),
    CONSTRAINT memory_provisioning_jobs_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'leased'::text, 'completed'::text, 'failed'::text])))
);


--
-- Name: memory_remote_resource_lifecycles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.memory_remote_resource_lifecycles (
    provider text NOT NULL,
    remote_database_id text NOT NULL,
    company_id uuid,
    desired_state text NOT NULL,
    operation_generation bigint DEFAULT 0 NOT NULL,
    operation_lease_token uuid,
    operation_lease_expires_at timestamp with time zone,
    quiesce_until timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    last_error text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT memory_remote_resource_lifecycles_check CHECK ((((operation_lease_token IS NULL) AND (operation_lease_expires_at IS NULL)) OR ((operation_lease_token IS NOT NULL) AND (operation_lease_expires_at IS NOT NULL)))),
    CONSTRAINT memory_remote_resource_lifecycles_check1 CHECK (((desired_state = 'absent'::text) OR (company_id IS NOT NULL))),
    CONSTRAINT memory_remote_resource_lifecycles_desired_state_check CHECK ((desired_state = ANY (ARRAY['present'::text, 'absent'::text]))),
    CONSTRAINT memory_remote_resource_lifecycles_operation_generation_check CHECK ((operation_generation >= 0)),
    CONSTRAINT memory_remote_resource_lifecycles_provider_check CHECK ((provider = ANY (ARRAY['hydradb'::text, 'hindsight'::text])))
);


--
-- Name: message_deliveries; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.message_deliveries (
    id uuid NOT NULL,
    company_id uuid,
    channel_id uuid,
    message_id uuid,
    source_binding_id uuid,
    destination_binding_id uuid,
    external_destination text,
    task_id uuid,
    depends_on_delivery_id uuid,
    correlation_id uuid NOT NULL,
    transport text NOT NULL,
    purpose text NOT NULL,
    idempotency_key text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    attempt_count integer DEFAULT 0 NOT NULL,
    max_attempts integer NOT NULL,
    available_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    last_error_class text,
    last_error_detail text,
    execution_id uuid,
    owner_worker_id uuid,
    locked_at timestamp with time zone,
    lock_expires_at timestamp with time zone,
    delivered_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    message_audience text,
    CONSTRAINT message_deliveries_attempt_check CHECK (((attempt_count >= 0) AND (max_attempts > 0) AND (attempt_count <= max_attempts))),
    CONSTRAINT message_deliveries_attribution_check CHECK ((((company_id IS NOT NULL) AND (channel_id IS NOT NULL) AND (message_id IS NOT NULL) AND (source_binding_id IS NOT NULL) AND (destination_binding_id IS NOT NULL)) OR ((company_id IS NULL) AND (channel_id IS NULL) AND (message_id IS NULL) AND (source_binding_id IS NULL) AND (destination_binding_id IS NULL) AND (task_id IS NULL) AND (depends_on_delivery_id IS NULL) AND (external_destination IS NOT NULL) AND (purpose = 'notification'::text)))),
    CONSTRAINT message_deliveries_delivered_at_check CHECK (((status = 'delivered'::text) = (delivered_at IS NOT NULL))),
    CONSTRAINT message_deliveries_error_check CHECK ((((last_error_class IS NULL) OR public.valid_delivery_failure_class(last_error_class)) AND ((last_error_detail IS NULL) OR (octet_length(last_error_detail) <= 512)) AND ((last_error_detail IS NULL) OR (last_error_class IS NOT NULL)))),
    CONSTRAINT message_deliveries_external_destination_check CHECK (((external_destination IS NULL) OR ((btrim(external_destination) <> ''::text) AND (octet_length(external_destination) <= 998)))),
    CONSTRAINT message_deliveries_idempotency_key_check CHECK (((btrim(idempotency_key) <> ''::text) AND (octet_length(idempotency_key) <= 512))),
    CONSTRAINT message_deliveries_lease_check CHECK ((((status = 'sending'::text) AND (execution_id IS NOT NULL) AND (owner_worker_id IS NOT NULL) AND (locked_at IS NOT NULL) AND (lock_expires_at IS NOT NULL) AND (lock_expires_at > locked_at)) OR ((status <> 'sending'::text) AND (execution_id IS NULL) AND (owner_worker_id IS NULL) AND (locked_at IS NULL) AND (lock_expires_at IS NULL)))),
    CONSTRAINT message_deliveries_message_audience_check CHECK ((((message_id IS NULL) AND (message_audience IS NULL)) OR ((message_id IS NOT NULL) AND (message_audience = 'external_conversation'::text)))),
    CONSTRAINT message_deliveries_no_self_dependency_check CHECK (((depends_on_delivery_id IS NULL) OR (depends_on_delivery_id <> id))),
    CONSTRAINT message_deliveries_purpose_check CHECK ((purpose = ANY (ARRAY['reply'::text, 'mirror'::text, 'outreach'::text, 'notification'::text]))),
    CONSTRAINT message_deliveries_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'sending'::text, 'retryable'::text, 'delivered'::text, 'outcome_unknown'::text, 'dead_letter'::text]))),
    CONSTRAINT message_deliveries_transport_check CHECK ((transport = ANY (ARRAY['email'::text, 'slack'::text])))
);


--
-- Name: message_delivery_parts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.message_delivery_parts (
    id uuid NOT NULL,
    company_id uuid,
    delivery_id uuid NOT NULL,
    part_index integer NOT NULL,
    part_key text NOT NULL,
    payload jsonb NOT NULL,
    status text DEFAULT 'prepared'::text NOT NULL,
    provider_message_key text,
    content_digest text NOT NULL,
    attempt_count integer DEFAULT 0 NOT NULL,
    last_error_class text,
    last_error_detail text,
    request_started_at timestamp with time zone,
    delivered_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT message_delivery_parts_attempt_check CHECK ((attempt_count >= 0)),
    CONSTRAINT message_delivery_parts_delivered_at_check CHECK (((status = 'delivered'::text) = (delivered_at IS NOT NULL))),
    CONSTRAINT message_delivery_parts_digest_check CHECK (((btrim(content_digest) <> ''::text) AND (octet_length(content_digest) <= 128))),
    CONSTRAINT message_delivery_parts_error_check CHECK ((((last_error_class IS NULL) OR public.valid_delivery_failure_class(last_error_class)) AND ((last_error_detail IS NULL) OR (octet_length(last_error_detail) <= 512)) AND ((last_error_detail IS NULL) OR (last_error_class IS NOT NULL)))),
    CONSTRAINT message_delivery_parts_index_check CHECK (((part_index >= 0) AND (part_index < 50))),
    CONSTRAINT message_delivery_parts_key_check CHECK (((btrim(part_key) <> ''::text) AND (octet_length(part_key) <= 200))),
    CONSTRAINT message_delivery_parts_payload_check CHECK (((jsonb_typeof(payload) = 'object'::text) AND (jsonb_typeof((payload -> 'transport'::text)) = 'string'::text) AND (jsonb_typeof((payload -> 'version'::text)) = 'number'::text) AND (octet_length((payload)::text) <= 262144))),
    CONSTRAINT message_delivery_parts_provider_key_check CHECK (((provider_message_key IS NULL) OR ((btrim(provider_message_key) <> ''::text) AND (octet_length(provider_message_key) <= 998)))),
    CONSTRAINT message_delivery_parts_started_check CHECK (((status <> 'delivered'::text) OR (request_started_at IS NOT NULL))),
    CONSTRAINT message_delivery_parts_status_check CHECK ((status = ANY (ARRAY['prepared'::text, 'sending'::text, 'delivered'::text, 'outcome_unknown'::text, 'retryable'::text, 'dead'::text])))
);


--
-- Name: message_participants; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.message_participants (
    company_id uuid NOT NULL,
    message_id uuid NOT NULL,
    participant_identity_id uuid NOT NULL,
    kind text NOT NULL,
    "position" integer NOT NULL,
    CONSTRAINT message_participants_kind_check CHECK ((kind = ANY (ARRAY['sender'::text, 'to'::text, 'cc'::text]))),
    CONSTRAINT message_participants_position_check CHECK (("position" >= 0))
);


--
-- Name: messages; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.messages (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    author_principal_id uuid NOT NULL,
    authored_identity_id uuid,
    subject text NOT NULL,
    clean_text_body text NOT NULL,
    attachments jsonb,
    direction text NOT NULL,
    role text NOT NULL,
    correlation_id uuid NOT NULL,
    content_hash bytea NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    audience text DEFAULT 'legacy_unclassified'::text NOT NULL,
    structured_response jsonb,
    CONSTRAINT messages_attachments_check CHECK (((attachments IS NULL) OR ((jsonb_typeof(attachments) = 'object'::text) AND ((attachments -> 'version'::text) = '"1"'::jsonb) AND (jsonb_typeof((attachments -> 'items'::text)) = 'array'::text) AND (octet_length((attachments)::text) <= 262144)))),
    CONSTRAINT messages_audience_check CHECK ((audience = ANY (ARRAY['external_conversation'::text, 'internal_only'::text, 'legacy_unclassified'::text]))),
    CONSTRAINT messages_content_hash_check CHECK ((octet_length(content_hash) = 32)),
    CONSTRAINT messages_direction_check CHECK ((direction = ANY (ARRAY['inbound'::text, 'outbound'::text]))),
    CONSTRAINT messages_role_check CHECK ((role = ANY (ARRAY['human'::text, 'agent'::text, 'system'::text]))),
    CONSTRAINT messages_structured_response_body CHECK (((structured_response IS NULL) OR COALESCE(((jsonb_typeof(structured_response) = 'object'::text) AND ((structured_response ->> 'body'::text) = clean_text_body) AND (((structured_response -> 'contract'::text) ->> 'version'::text) = '1'::text) AND (length((structured_response ->> 'fingerprint'::text)) = 64)), false))),
    CONSTRAINT messages_subject_check CHECK ((octet_length(subject) <= 2048))
);


--
-- Name: notification_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.notification_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    notification_id uuid DEFAULT gen_random_uuid() NOT NULL,
    company_id uuid NOT NULL,
    source_kind text NOT NULL,
    source_id uuid NOT NULL,
    action_kind text NOT NULL,
    source_generation bigint NOT NULL,
    actor_principal_id uuid,
    status text DEFAULT 'pending'::text NOT NULL,
    attempt_count integer DEFAULT 0 NOT NULL,
    max_attempts integer DEFAULT 10 NOT NULL,
    available_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    execution_id uuid,
    owner_worker_id uuid,
    locked_at timestamp with time zone,
    lock_expires_at timestamp with time zone,
    last_error_class text,
    last_error_detail text,
    occurred_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    projected_at timestamp with time zone,
    CONSTRAINT notification_events_action_kind_check CHECK ((action_kind = ANY (ARRAY['assignment'::text, 'response_review'::text, 'delegation_timeout'::text, 'task_failure'::text, 'delivery_failure'::text]))),
    CONSTRAINT notification_events_attempt_check CHECK (((attempt_count >= 0) AND (max_attempts > 0) AND (attempt_count <= max_attempts))),
    CONSTRAINT notification_events_error_check CHECK (((last_error_class IS NULL) OR (last_error_class = ANY (ARRAY['database'::text, 'composition'::text, 'invalid_source'::text, 'lease_expired'::text, 'internal'::text])))),
    CONSTRAINT notification_events_lease_check CHECK ((((status = 'processing'::text) AND (execution_id IS NOT NULL) AND (owner_worker_id IS NOT NULL) AND (locked_at IS NOT NULL) AND (lock_expires_at IS NOT NULL) AND (lock_expires_at > locked_at)) OR ((status <> 'processing'::text) AND (execution_id IS NULL) AND (owner_worker_id IS NULL) AND (locked_at IS NULL) AND (lock_expires_at IS NULL)))),
    CONSTRAINT notification_events_projection_check CHECK (((status = 'projected'::text) = (projected_at IS NOT NULL))),
    CONSTRAINT notification_events_source_generation_check CHECK ((source_generation > 0)),
    CONSTRAINT notification_events_source_kind_check CHECK ((source_kind = ANY (ARRAY['task'::text, 'handoff'::text, 'response_review'::text, 'delegation'::text, 'delivery'::text]))),
    CONSTRAINT notification_events_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'processing'::text, 'projected'::text, 'dead_letter'::text])))
);


--
-- Name: notifications; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.notifications (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    company_id uuid NOT NULL,
    recipient_user_id uuid NOT NULL,
    recipient_principal_id uuid,
    event_id uuid NOT NULL,
    source_kind text NOT NULL,
    source_id uuid NOT NULL,
    action_kind text NOT NULL,
    source_generation bigint NOT NULL,
    channel_id uuid NOT NULL,
    state text DEFAULT 'active'::text NOT NULL,
    read_at timestamp with time zone,
    state_changed_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    email_delivery_id uuid,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT notifications_action_kind_check CHECK ((action_kind = ANY (ARRAY['assignment'::text, 'response_review'::text, 'delegation_timeout'::text, 'task_failure'::text, 'delivery_failure'::text]))),
    CONSTRAINT notifications_source_generation_check CHECK ((source_generation > 0)),
    CONSTRAINT notifications_source_kind_check CHECK ((source_kind = ANY (ARRAY['task'::text, 'handoff'::text, 'response_review'::text, 'delegation'::text, 'delivery'::text]))),
    CONSTRAINT notifications_state_check CHECK ((state = ANY (ARRAY['active'::text, 'resolved'::text, 'withdrawn'::text])))
);


--
-- Name: participant_identities; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.participant_identities (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    transport text NOT NULL,
    namespace text NOT NULL,
    subject text NOT NULL,
    display_label text,
    status text NOT NULL,
    claim_metadata jsonb NOT NULL,
    provenance text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT participant_identities_claim_metadata_check CHECK (((jsonb_typeof(claim_metadata) = 'object'::text) AND ((claim_metadata -> 'version'::text) = '1'::jsonb) AND (jsonb_typeof((claim_metadata -> 'kind'::text)) = 'string'::text) AND ((claim_metadata ->> 'kind'::text) = ANY (ARRAY['observation'::text, 'account'::text, 'provider_profile'::text])) AND (octet_length((claim_metadata)::text) <= 8192))),
    CONSTRAINT participant_identities_display_label_check CHECK (((display_label IS NULL) OR (octet_length(display_label) <= 255))),
    CONSTRAINT participant_identities_namespace_check CHECK (((btrim(namespace) <> ''::text) AND (octet_length(namespace) <= 255))),
    CONSTRAINT participant_identities_provenance_check CHECK ((provenance = ANY (ARRAY['account'::text, 'agent'::text, 'channel_allowlist'::text, 'transport_ingress'::text, 'provider_profile_claim'::text, 'system'::text]))),
    CONSTRAINT participant_identities_status_check CHECK ((status = ANY (ARRAY['observed'::text, 'verified'::text, 'disabled'::text]))),
    CONSTRAINT participant_identities_subject_check CHECK (((btrim(subject) <> ''::text) AND (octet_length(subject) <= 320))),
    CONSTRAINT participant_identities_transport_check CHECK ((transport = ANY (ARRAY['email'::text, 'slack'::text])))
);


--
-- Name: pending_account_changes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.pending_account_changes (
    user_id uuid NOT NULL,
    kind text NOT NULL,
    new_email public.citext,
    new_password_hash text,
    confirmation_code_hash text NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT pending_account_changes_email_not_blank CHECK (((new_email IS NULL) OR (btrim((new_email)::text) <> ''::text))),
    CONSTRAINT pending_account_changes_kind_check CHECK ((kind = ANY (ARRAY['email'::text, 'password'::text]))),
    CONSTRAINT pending_account_changes_payload_matches_kind CHECK ((((kind = 'email'::text) AND (new_email IS NOT NULL) AND (new_password_hash IS NULL)) OR ((kind = 'password'::text) AND (new_password_hash IS NOT NULL) AND (new_email IS NULL))))
);


--
-- Name: pending_user_registrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.pending_user_registrations (
    email public.citext NOT NULL,
    username public.citext NOT NULL,
    password_hash text NOT NULL,
    confirmation_code_hash text NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT pending_user_registrations_email_not_blank CHECK ((btrim((email)::text) <> ''::text)),
    CONSTRAINT pending_user_registrations_username_not_blank CHECK ((btrim((username)::text) <> ''::text))
);


--
-- Name: response_draft_evidence; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.response_draft_evidence (
    company_id uuid NOT NULL,
    draft_id uuid NOT NULL,
    draft_version integer NOT NULL,
    id uuid NOT NULL,
    "position" integer NOT NULL,
    source_reference jsonb NOT NULL,
    source_version text NOT NULL,
    content_digest text NOT NULL,
    audience text NOT NULL,
    support text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT response_draft_evidence_audience_check CHECK ((audience = ANY (ARRAY['external_conversation'::text, 'internal_only'::text, 'company_restricted'::text]))),
    CONSTRAINT response_draft_evidence_digest_check CHECK (((btrim(content_digest) <> ''::text) AND (octet_length(content_digest) <= 128))),
    CONSTRAINT response_draft_evidence_position_check CHECK ((("position" >= 0) AND ("position" < 128))),
    CONSTRAINT response_draft_evidence_source_check CHECK (((jsonb_typeof(source_reference) = 'object'::text) AND (jsonb_typeof((source_reference -> 'kind'::text)) = 'string'::text) AND ((source_reference ->> 'kind'::text) = ANY (ARRAY['message'::text, 'note'::text, 'attachment'::text, 'delegated_result'::text, 'retained_tool_result'::text, 'external_url'::text])) AND (octet_length((source_reference)::text) <= 8192))),
    CONSTRAINT response_draft_evidence_support_check CHECK ((support = ANY (ARRAY['direct_evidence'::text, 'inference'::text]))),
    CONSTRAINT response_draft_evidence_version_check CHECK (((btrim(source_version) <> ''::text) AND (octet_length(source_version) <= 256)))
);


--
-- Name: response_draft_publications; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.response_draft_publications (
    company_id uuid NOT NULL,
    draft_id uuid NOT NULL,
    draft_version integer NOT NULL,
    message_id uuid NOT NULL,
    message_audience text DEFAULT 'external_conversation'::text NOT NULL,
    delivery_id uuid NOT NULL,
    published_by_principal_id uuid NOT NULL,
    published_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT response_draft_publications_audience_check CHECK ((message_audience = 'external_conversation'::text))
);


--
-- Name: response_drafts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.response_drafts (
    id uuid NOT NULL,
    version integer NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    task_id uuid,
    source_handoff_generation uuid,
    author_principal_id uuid NOT NULL,
    reviewer_principal_id uuid NOT NULL,
    proposed_message_id uuid NOT NULL,
    subject text NOT NULL,
    body text NOT NULL,
    attachment_snapshot jsonb NOT NULL,
    recipient_snapshot jsonb NOT NULL,
    transport_snapshot jsonb NOT NULL,
    publication_snapshot jsonb NOT NULL,
    status text DEFAULT 'pending_review'::text NOT NULL,
    created_by_principal_id uuid NOT NULL,
    updated_by_principal_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    response_contract jsonb GENERATED ALWAYS AS ((((publication_snapshot -> 'message'::text) -> 'structured'::text) -> 'contract'::text)) STORED,
    CONSTRAINT response_drafts_attachment_snapshot_check CHECK (((jsonb_typeof(attachment_snapshot) = 'object'::text) AND ((attachment_snapshot -> 'version'::text) = '"1"'::jsonb) AND (jsonb_typeof((attachment_snapshot -> 'items'::text)) = 'array'::text) AND (octet_length((attachment_snapshot)::text) <= 262144))),
    CONSTRAINT response_drafts_body_check CHECK (((btrim(body) <> ''::text) AND (octet_length(body) <= 262144))),
    CONSTRAINT response_drafts_publication_snapshot_check CHECK (((jsonb_typeof(publication_snapshot) = 'object'::text) AND ((publication_snapshot -> 'version'::text) = '"1"'::jsonb) AND (octet_length((publication_snapshot)::text) <= 16777216))),
    CONSTRAINT response_drafts_recipient_snapshot_check CHECK (((jsonb_typeof(recipient_snapshot) = 'object'::text) AND ((recipient_snapshot -> 'version'::text) = '"1"'::jsonb) AND (jsonb_typeof((recipient_snapshot -> 'to'::text)) = 'array'::text) AND (jsonb_typeof((recipient_snapshot -> 'cc'::text)) = 'array'::text) AND (octet_length((recipient_snapshot)::text) <= 32768))),
    CONSTRAINT response_drafts_status_check CHECK ((status = ANY (ARRAY['pending_review'::text, 'rejected'::text, 'expired'::text, 'superseded'::text, 'published'::text]))),
    CONSTRAINT response_drafts_structured_body CHECK (((response_contract IS NULL) OR COALESCE((((((publication_snapshot -> 'message'::text) -> 'structured'::text) ->> 'body'::text) = ((publication_snapshot -> 'message'::text) ->> 'clean_text_body'::text)) AND ((response_contract ->> 'version'::text) = '1'::text) AND (length((((publication_snapshot -> 'message'::text) -> 'structured'::text) ->> 'fingerprint'::text)) = 64)), false))),
    CONSTRAINT response_drafts_subject_check CHECK ((octet_length(subject) <= 2048)),
    CONSTRAINT response_drafts_transport_snapshot_check CHECK (((jsonb_typeof(transport_snapshot) = 'object'::text) AND ((transport_snapshot -> 'version'::text) = '"1"'::jsonb) AND (octet_length((transport_snapshot)::text) <= 8192))),
    CONSTRAINT response_drafts_version_check CHECK ((version > 0))
);


--
-- Name: response_review_commands; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.response_review_commands (
    company_id uuid NOT NULL,
    command_id uuid NOT NULL,
    draft_id uuid NOT NULL,
    expected_draft_version integer NOT NULL,
    action text NOT NULL,
    command_fingerprint text NOT NULL,
    resulting_draft_version integer NOT NULL,
    published_message_id uuid,
    published_delivery_id uuid,
    published_delivery_created boolean,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT response_review_commands_action_check CHECK ((action = ANY (ARRAY['approve'::text, 'edit'::text, 'reject'::text, 'reassign'::text]))),
    CONSTRAINT response_review_commands_fingerprint_check CHECK (((btrim(command_fingerprint) <> ''::text) AND (octet_length(command_fingerprint) <= 128))),
    CONSTRAINT response_review_commands_publication_result_check CHECK ((((published_delivery_id IS NULL) AND (published_delivery_created IS NULL)) OR ((published_delivery_id IS NOT NULL) AND (published_delivery_created IS NOT NULL)))),
    CONSTRAINT response_review_commands_version_check CHECK (((expected_draft_version > 0) AND (resulting_draft_version > 0)))
);


--
-- Name: response_reviews; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.response_reviews (
    company_id uuid NOT NULL,
    draft_id uuid NOT NULL,
    draft_version integer NOT NULL,
    reviewer_principal_id uuid NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    feedback text,
    reviewer_rationale text,
    decided_by_principal_id uuid,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    notification_actor_principal_id uuid,
    CONSTRAINT response_reviews_decision_shape_check CHECK ((((status = 'pending'::text) AND (decided_by_principal_id IS NULL) AND (feedback IS NULL) AND (reviewer_rationale IS NULL)) OR ((status = 'rejected'::text) AND (decided_by_principal_id IS NOT NULL) AND (feedback IS NOT NULL)) OR ((status = 'published'::text) AND (decided_by_principal_id IS NOT NULL)) OR (status = ANY (ARRAY['expired'::text, 'superseded'::text])))),
    CONSTRAINT response_reviews_expiry_check CHECK ((expires_at > created_at)),
    CONSTRAINT response_reviews_feedback_check CHECK (((feedback IS NULL) OR ((btrim(feedback) <> ''::text) AND (octet_length(feedback) <= 8192)))),
    CONSTRAINT response_reviews_rationale_check CHECK (((reviewer_rationale IS NULL) OR ((btrim(reviewer_rationale) <> ''::text) AND (octet_length(reviewer_rationale) <= 2048)))),
    CONSTRAINT response_reviews_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'rejected'::text, 'expired'::text, 'superseded'::text, 'published'::text])))
);


--
-- Name: runtime_metric_samples; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.runtime_metric_samples (
    machine_id text NOT NULL,
    machine_region text,
    sampled_at timestamp with time zone NOT NULL,
    process_rss_bytes bigint,
    memory_limit_bytes bigint,
    cpu_utilization_percent double precision,
    cpu_steal_percent double precision,
    cpu_throttle_percent double precision,
    database_acquire_duration_ms double precision NOT NULL,
    database_acquire_succeeded boolean NOT NULL,
    pool_size integer NOT NULL,
    pool_idle integer NOT NULL,
    pool_active integer NOT NULL,
    active_task_executions integer DEFAULT 0 NOT NULL,
    task_worker_concurrency_limit integer DEFAULT 1 NOT NULL,
    hydradb_calls integer DEFAULT 0 NOT NULL,
    hydradb_failures integer DEFAULT 0 NOT NULL,
    hydradb_duration_ms double precision DEFAULT 0 NOT NULL,
    CONSTRAINT runtime_metric_samples_acquire_duration_nonnegative CHECK ((database_acquire_duration_ms >= (0)::double precision)),
    CONSTRAINT runtime_metric_samples_active_tasks_nonnegative CHECK ((active_task_executions >= 0)),
    CONSTRAINT runtime_metric_samples_active_tasks_within_limit CHECK ((active_task_executions <= task_worker_concurrency_limit)),
    CONSTRAINT runtime_metric_samples_cpu_steal_nonnegative CHECK (((cpu_steal_percent IS NULL) OR (cpu_steal_percent >= (0)::double precision))),
    CONSTRAINT runtime_metric_samples_cpu_throttle_nonnegative CHECK (((cpu_throttle_percent IS NULL) OR (cpu_throttle_percent >= (0)::double precision))),
    CONSTRAINT runtime_metric_samples_cpu_utilization_nonnegative CHECK (((cpu_utilization_percent IS NULL) OR (cpu_utilization_percent >= (0)::double precision))),
    CONSTRAINT runtime_metric_samples_hydradb_calls_nonnegative CHECK ((hydradb_calls >= 0)),
    CONSTRAINT runtime_metric_samples_hydradb_duration_needs_calls CHECK (((hydradb_calls > 0) OR (hydradb_duration_ms = (0)::double precision))),
    CONSTRAINT runtime_metric_samples_hydradb_duration_nonnegative CHECK ((hydradb_duration_ms >= (0)::double precision)),
    CONSTRAINT runtime_metric_samples_hydradb_failures_within_calls CHECK (((hydradb_failures >= 0) AND (hydradb_failures <= hydradb_calls))),
    CONSTRAINT runtime_metric_samples_memory_limit_nonnegative CHECK (((memory_limit_bytes IS NULL) OR (memory_limit_bytes >= 0))),
    CONSTRAINT runtime_metric_samples_pool_active_nonnegative CHECK ((pool_active >= 0)),
    CONSTRAINT runtime_metric_samples_pool_idle_nonnegative CHECK ((pool_idle >= 0)),
    CONSTRAINT runtime_metric_samples_pool_parts_fit CHECK (((pool_idle + pool_active) = pool_size)),
    CONSTRAINT runtime_metric_samples_pool_size_nonnegative CHECK ((pool_size >= 0)),
    CONSTRAINT runtime_metric_samples_rss_nonnegative CHECK (((process_rss_bytes IS NULL) OR (process_rss_bytes >= 0))),
    CONSTRAINT runtime_metric_samples_worker_limit_positive CHECK ((task_worker_concurrency_limit > 0))
);


--
-- Name: schedule_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.schedule_runs (
    id uuid NOT NULL,
    schedule_id uuid NOT NULL,
    scheduled_for timestamp with time zone NOT NULL,
    schedule_snapshot jsonb NOT NULL,
    thread_id uuid,
    task_id uuid,
    last_error text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    materialization_status text DEFAULT 'pending'::text NOT NULL,
    materialization_attempts integer DEFAULT 0 NOT NULL,
    materialization_available_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    materialization_worker_id uuid,
    materialization_generation uuid,
    materialization_locked_at timestamp with time zone,
    materialization_lock_expires_at timestamp with time zone,
    CONSTRAINT schedule_runs_materialization_attempts_check CHECK (((materialization_attempts >= 0) AND (materialization_attempts <= 5))),
    CONSTRAINT schedule_runs_materialization_state_check CHECK ((((materialization_status = 'pending'::text) AND (task_id IS NULL) AND (materialization_attempts < 5) AND (materialization_worker_id IS NULL) AND (materialization_generation IS NULL) AND (materialization_locked_at IS NULL) AND (materialization_lock_expires_at IS NULL)) OR ((materialization_status = 'materializing'::text) AND (task_id IS NULL) AND ((materialization_attempts >= 1) AND (materialization_attempts <= 5)) AND (materialization_worker_id IS NOT NULL) AND (materialization_generation IS NOT NULL) AND (materialization_locked_at IS NOT NULL) AND (materialization_lock_expires_at IS NOT NULL) AND (materialization_lock_expires_at > materialization_locked_at)) OR ((materialization_status = 'materialized'::text) AND (task_id IS NOT NULL) AND (materialization_worker_id IS NULL) AND (materialization_generation IS NULL) AND (materialization_locked_at IS NULL) AND (materialization_lock_expires_at IS NULL)) OR ((materialization_status = 'failed'::text) AND (task_id IS NULL) AND (materialization_attempts = 5) AND (materialization_worker_id IS NULL) AND (materialization_generation IS NULL) AND (materialization_locked_at IS NULL) AND (materialization_lock_expires_at IS NULL)))),
    CONSTRAINT schedule_runs_snapshot_object_check CHECK ((jsonb_typeof(schedule_snapshot) = 'object'::text)),
    CONSTRAINT schedule_runs_task_requires_thread_check CHECK (((task_id IS NULL) OR (thread_id IS NOT NULL)))
);


--
-- Name: skills; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.skills (
    id uuid NOT NULL,
    company_id uuid,
    slug public.citext NOT NULL,
    name text NOT NULL,
    description text NOT NULL,
    trigger text NOT NULL,
    instructions jsonb NOT NULL,
    created_by jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT skills_created_by_shape_check CHECK (public.valid_creation_provenance(created_by)),
    CONSTRAINT skills_description_bounded CHECK (((btrim(description) <> ''::text) AND (char_length(description) <= 500))),
    CONSTRAINT skills_instructions_shape CHECK (((jsonb_typeof(instructions) = 'array'::text) AND ((jsonb_array_length(instructions) >= 1) AND (jsonb_array_length(instructions) <= 32)) AND (octet_length((instructions)::text) <= 524288))),
    CONSTRAINT skills_name_bounded CHECK (((btrim(name) <> ''::text) AND (char_length(name) <= 120))),
    CONSTRAINT skills_slug_format CHECK (((char_length((slug)::text) <= 120) AND ((slug)::text = lower((slug)::text)) AND ((slug)::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'::text))),
    CONSTRAINT skills_trigger_bounded CHECK (((btrim(trigger) <> ''::text) AND (char_length(trigger) <= 500)))
);


--
-- Name: start_agent_task_commands; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.start_agent_task_commands (
    company_id uuid NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    task_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);


--
-- Name: task_agent_instruction_notes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_agent_instruction_notes (
    instruction_id uuid NOT NULL,
    company_id uuid NOT NULL,
    note_id uuid NOT NULL,
    "position" integer NOT NULL,
    CONSTRAINT task_agent_instruction_notes_position_check CHECK ((("position" >= 0) AND ("position" < 50)))
);


--
-- Name: task_agent_instructions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_agent_instructions (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    task_id uuid NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    requested_by_principal_id uuid NOT NULL,
    requested_ownership_version bigint NOT NULL,
    wake_outcome text NOT NULL,
    consumed_execution_generation uuid,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    consumed_at timestamp with time zone,
    CONSTRAINT task_agent_instructions_consumption_check CHECK ((((consumed_execution_generation IS NULL) AND (consumed_at IS NULL)) OR ((consumed_execution_generation IS NOT NULL) AND (consumed_at IS NOT NULL)))),
    CONSTRAINT task_agent_instructions_requested_ownership_version_check CHECK ((requested_ownership_version > 0)),
    CONSTRAINT task_agent_instructions_wake_outcome_check CHECK ((wake_outcome = ANY (ARRAY['queued'::text, 'requeued'::text, 'parked'::text])))
);


--
-- Name: task_approval_waits; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_approval_waits (
    task_id uuid NOT NULL,
    company_id uuid NOT NULL,
    approval_id uuid NOT NULL,
    cycle_id uuid NOT NULL,
    owner_principal_id uuid,
    ownership_version bigint NOT NULL,
    state text NOT NULL,
    run_id uuid,
    invocation_id uuid,
    checkpoint_revision bigint,
    CONSTRAINT task_approval_waits_check CHECK ((((run_id IS NULL) AND (invocation_id IS NULL) AND (checkpoint_revision IS NULL)) OR ((run_id IS NOT NULL) AND (invocation_id IS NOT NULL) AND (checkpoint_revision >= 0)))),
    CONSTRAINT task_approval_waits_ownership_version_check CHECK ((ownership_version >= 0)),
    CONSTRAINT task_approval_waits_state_check CHECK ((state = ANY (ARRAY['waiting'::text, 'approved'::text, 'rejected'::text, 'expired'::text])))
);


--
-- Name: task_attempts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_attempts (
    id uuid NOT NULL,
    task_id uuid NOT NULL,
    attempt_number integer NOT NULL,
    status text NOT NULL,
    error text,
    stop_reason text,
    prompt_tokens integer,
    completion_tokens integer,
    result jsonb,
    started_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    finished_at timestamp with time zone,
    execution_generation uuid NOT NULL,
    worker_id uuid NOT NULL,
    machine_id text NOT NULL,
    machine_region text,
    CONSTRAINT task_attempts_machine_id_check CHECK ((length(TRIM(BOTH FROM machine_id)) > 0)),
    CONSTRAINT task_attempts_status_check CHECK ((status = ANY (ARRAY['processing'::text, 'completed'::text, 'failed'::text]))),
    CONSTRAINT task_attempts_stop_reason_check CHECK ((stop_reason = ANY (ARRAY['completed'::text, 'retryable_failure'::text, 'terminal_failure'::text, 'timed_out'::text, 'shutdown'::text, 'lease_lost'::text, 'ownership_transferred'::text, 'agent_instruction'::text, 'delegation_cancelled'::text]))),
    CONSTRAINT task_attempts_token_check CHECK ((((prompt_tokens IS NULL) OR (prompt_tokens >= 0)) AND ((completion_tokens IS NULL) OR (completion_tokens >= 0))))
);


--
-- Name: task_channel_targets; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_channel_targets (
    task_id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    recipient_role text NOT NULL,
    "position" integer NOT NULL,
    CONSTRAINT task_channel_targets_position_check CHECK (("position" >= 0)),
    CONSTRAINT task_channel_targets_role_check CHECK ((recipient_role = ANY (ARRAY['to'::text, 'cc'::text])))
);


--
-- Name: task_harness_invocations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_harness_invocations (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    task_id uuid NOT NULL,
    run_id uuid NOT NULL,
    model_turn smallint NOT NULL,
    call_ordinal smallint NOT NULL,
    state text NOT NULL,
    invocation jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT task_harness_invocations_call_ordinal_check CHECK (((call_ordinal >= 0) AND (call_ordinal <= 63))),
    CONSTRAINT task_harness_invocations_invocation_check CHECK (((jsonb_typeof(invocation) = 'object'::text) AND (octet_length((invocation)::text) <= 262144))),
    CONSTRAINT task_harness_invocations_ledger_identity CHECK (COALESCE(((((invocation -> 'call'::text) ->> 'invocation_id'::text) = (id)::text) AND (((invocation ->> 'turn'::text))::integer = model_turn) AND (((invocation ->> 'ordinal'::text))::integer = call_ordinal) AND ((invocation ->> 'state'::text) = state) AND (invocation ? 'result'::text) AND ((state = 'completed'::text) = ((invocation -> 'result'::text) <> 'null'::jsonb))), false)),
    CONSTRAINT task_harness_invocations_model_turn_check CHECK (((model_turn >= 0) AND (model_turn <= 15))),
    CONSTRAINT task_harness_invocations_state_check CHECK ((state = ANY (ARRAY['prepared'::text, 'ready'::text, 'waiting'::text, 'completed'::text, 'failed'::text, 'indeterminate'::text])))
);


--
-- Name: task_harness_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_harness_runs (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    task_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    owner_principal_id uuid NOT NULL,
    ownership_version bigint NOT NULL,
    schema_version smallint NOT NULL,
    revision bigint NOT NULL,
    state text NOT NULL,
    checkpoint jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    final_output_consumed_at timestamp with time zone,
    CONSTRAINT task_harness_runs_check CHECK ((((checkpoint ->> 'schema_version'::text))::integer = schema_version)),
    CONSTRAINT task_harness_runs_check1 CHECK ((((checkpoint ->> 'revision'::text))::bigint = revision)),
    CONSTRAINT task_harness_runs_check2 CHECK (((checkpoint ->> 'state'::text) = state)),
    CONSTRAINT task_harness_runs_check3 CHECK ((((checkpoint ->> 'run_id'::text) = (id)::text) AND (((checkpoint -> 'identity'::text) ->> 'company_id'::text) = (company_id)::text) AND (((checkpoint -> 'identity'::text) ->> 'task_id'::text) = (task_id)::text) AND (((checkpoint -> 'identity'::text) ->> 'agent_id'::text) = (agent_id)::text))),
    CONSTRAINT task_harness_runs_checkpoint_check CHECK (((jsonb_typeof(checkpoint) = 'object'::text) AND (octet_length((checkpoint)::text) <= 2097152))),
    CONSTRAINT task_harness_runs_checkpoint_check1 CHECK ((jsonb_array_length((checkpoint -> 'reservations'::text)) <= 16)),
    CONSTRAINT task_harness_runs_checkpoint_check2 CHECK ((jsonb_array_length((checkpoint -> 'invocations'::text)) <= 64)),
    CONSTRAINT task_harness_runs_invalid_output CHECK (((state <> 'invalid_output'::text) OR COALESCE((((checkpoint -> 'final_output'::text) = 'null'::jsonb) AND (((checkpoint -> 'identity'::text) -> 'response_contract'::text) <> 'null'::jsonb) AND (jsonb_array_length(jsonb_path_query_array(checkpoint, '$."reservations"[*]?(@."repair" != null)."repair"'::jsonpath)) = 2) AND ((((checkpoint -> 'turns'::text) -> '-1'::integer) ->> 'invalid_response'::text) = ANY (ARRAY['malformed_json'::text, 'schema_mismatch'::text, 'output_limit'::text, 'tool_call'::text]))), false))),
    CONSTRAINT task_harness_runs_ownership_version_check CHECK ((ownership_version >= 0)),
    CONSTRAINT task_harness_runs_required_checkpoint_fields CHECK (COALESCE(((checkpoint ?& ARRAY['schema_version'::text, 'run_id'::text, 'identity'::text, 'revision'::text, 'state'::text, 'policy'::text, 'messages'::text, 'reservations'::text, 'turns'::text, 'invocations'::text, 'executions'::text]) AND (((checkpoint -> 'identity'::text) ->> 'harness'::text) = 'rig'::text) AND (jsonb_typeof((checkpoint -> 'messages'::text)) = 'array'::text) AND ((jsonb_array_length((checkpoint -> 'messages'::text)) >= 1) AND (jsonb_array_length((checkpoint -> 'messages'::text)) <= 81)) AND (jsonb_typeof((checkpoint -> 'turns'::text)) = 'array'::text) AND (jsonb_array_length((checkpoint -> 'turns'::text)) <= 16) AND (jsonb_typeof((checkpoint -> 'executions'::text)) = 'array'::text) AND (jsonb_array_length((checkpoint -> 'executions'::text)) <= 64)), false)),
    CONSTRAINT task_harness_runs_response_contract CHECK (COALESCE((((checkpoint -> 'identity'::text) ? 'response_contract'::text) AND (checkpoint ? 'contract_fingerprint'::text) AND
CASE
    WHEN (((checkpoint -> 'identity'::text) -> 'response_contract'::text) = 'null'::jsonb) THEN ((checkpoint -> 'contract_fingerprint'::text) = 'null'::jsonb)
    ELSE (((((checkpoint -> 'identity'::text) -> 'response_contract'::text) ->> 'version'::text) = '1'::text) AND ((((checkpoint -> 'identity'::text) -> 'response_contract'::text) ->> 'format'::text) = 'json_schema'::text) AND (length((checkpoint ->> 'contract_fingerprint'::text)) = 64) AND
    CASE
        WHEN (state = 'completed'::text) THEN (((((checkpoint -> 'final_output'::text) -> 'structured'::text) -> 'contract'::text) = ((checkpoint -> 'identity'::text) -> 'response_contract'::text)) AND ((((checkpoint -> 'final_output'::text) -> 'structured'::text) ->> 'fingerprint'::text) = (checkpoint ->> 'contract_fingerprint'::text)) AND ((((checkpoint -> 'final_output'::text) -> 'structured'::text) ->> 'body'::text) = ((checkpoint -> 'final_output'::text) ->> 'content'::text)))
        ELSE true
    END)
END AND (jsonb_array_length(jsonb_path_query_array(checkpoint, '$."reservations"[*]?(@."repair" != null)."repair"'::jsonpath)) <= 2)), false)),
    CONSTRAINT task_harness_runs_revision_check CHECK ((revision >= 0)),
    CONSTRAINT task_harness_runs_schema_version_check CHECK ((schema_version = 1)),
    CONSTRAINT task_harness_runs_state_check CHECK ((state = ANY (ARRAY['active'::text, 'waiting'::text, 'completed'::text, 'superseded'::text, 'invalid_output'::text])))
);


--
-- Name: task_outreach_replies; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_outreach_replies (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    company_id uuid NOT NULL,
    outreach_id uuid NOT NULL,
    target_id uuid NOT NULL,
    response_association_id uuid NOT NULL,
    disposition text NOT NULL,
    received_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT task_outreach_replies_disposition_check CHECK ((disposition = ANY (ARRAY['counted'::text, 'late'::text, 'duplicate'::text])))
);


--
-- Name: task_outreach_targets; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_outreach_targets (
    outreach_id uuid NOT NULL,
    email public.citext NOT NULL,
    responded_at timestamp with time zone,
    response_association_id uuid,
    delivery_id uuid,
    request_message_id uuid,
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    company_id uuid NOT NULL,
    target_kind text DEFAULT 'external'::text NOT NULL,
    internal_channel_id uuid,
    external_transport text,
    external_namespace text,
    external_subject text,
    status text DEFAULT 'active'::text NOT NULL,
    replaces_target_id uuid,
    CONSTRAINT task_outreach_targets_identity_check CHECK ((((target_kind = 'internal_channel'::text) AND (internal_channel_id IS NOT NULL) AND (external_transport IS NULL) AND (external_namespace IS NULL) AND (external_subject IS NULL)) OR ((target_kind = 'external'::text) AND (internal_channel_id IS NULL) AND (external_transport IS NOT NULL) AND (external_namespace IS NOT NULL) AND (external_subject IS NOT NULL) AND (btrim(external_namespace) <> ''::text) AND (btrim(external_subject) <> ''::text)))),
    CONSTRAINT task_outreach_targets_kind_check CHECK ((target_kind = ANY (ARRAY['internal_channel'::text, 'external'::text]))),
    CONSTRAINT task_outreach_targets_response_check CHECK (((response_association_id IS NULL) OR (responded_at IS NOT NULL))),
    CONSTRAINT task_outreach_targets_response_state_check CHECK ((((status = 'responded'::text) AND (responded_at IS NOT NULL) AND (response_association_id IS NOT NULL)) OR ((status <> 'responded'::text) AND (responded_at IS NULL) AND (response_association_id IS NULL)))),
    CONSTRAINT task_outreach_targets_status_check CHECK ((status = ANY (ARRAY['active'::text, 'responded'::text, 'cancelled'::text, 'superseded'::text, 'expired'::text]))),
    CONSTRAINT task_outreach_targets_transport_check CHECK (((external_transport IS NULL) OR (external_transport = ANY (ARRAY['email'::text, 'slack'::text]))))
);


--
-- Name: task_outreaches; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_outreaches (
    id uuid NOT NULL,
    task_id uuid NOT NULL,
    status text NOT NULL,
    required_threshold_percent numeric(5,2) NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    outreach_key text NOT NULL,
    subject text NOT NULL,
    body text NOT NULL,
    company_id uuid NOT NULL,
    version bigint DEFAULT 1 NOT NULL,
    created_by_principal_id uuid,
    created_by_principal_kind text,
    ownership_version bigint DEFAULT 1 NOT NULL,
    harness_run_id uuid,
    harness_invocation_id uuid,
    checkpoint_revision bigint,
    CONSTRAINT task_outreaches_body_check CHECK ((length(btrim(body)) > 0)),
    CONSTRAINT task_outreaches_check CHECK ((((harness_run_id IS NULL) AND (harness_invocation_id IS NULL) AND (checkpoint_revision IS NULL)) OR ((harness_run_id IS NOT NULL) AND (harness_invocation_id IS NOT NULL) AND (checkpoint_revision >= 0)))),
    CONSTRAINT task_outreaches_creator_shape_check CHECK ((((created_by_principal_id IS NULL) AND (created_by_principal_kind IS NULL)) OR ((created_by_principal_id IS NOT NULL) AND (created_by_principal_kind = 'agent'::text)))),
    CONSTRAINT task_outreaches_expiry_check CHECK ((expires_at > created_at)),
    CONSTRAINT task_outreaches_ownership_version_check CHECK ((ownership_version >= 0)),
    CONSTRAINT task_outreaches_status_check CHECK ((status = ANY (ARRAY['waiting'::text, 'threshold_met'::text, 'timeout_pending_approval'::text, 'proceed_partial'::text, 'cancelled'::text, 'completed'::text]))),
    CONSTRAINT task_outreaches_subject_check CHECK ((length(btrim(subject)) > 0)),
    CONSTRAINT task_outreaches_threshold_check CHECK (((required_threshold_percent > (0)::numeric) AND (required_threshold_percent <= (100)::numeric))),
    CONSTRAINT task_outreaches_version_check CHECK ((version > 0))
);


--
-- Name: task_ownership_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_ownership_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    task_id uuid NOT NULL,
    company_id uuid NOT NULL,
    sequence bigint NOT NULL,
    from_version bigint NOT NULL,
    to_version bigint NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    operation text NOT NULL,
    actor_principal_id uuid,
    actor_kind text NOT NULL,
    previous_owner_principal_id uuid,
    previous_owner_kind text,
    previous_owner_label text,
    new_owner_principal_id uuid,
    new_owner_kind text,
    new_owner_label text,
    reason text NOT NULL,
    reason_detail text,
    handoff_instruction text,
    occurred_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT task_ownership_events_actor_kind_check CHECK ((actor_kind = ANY (ARRAY['system'::text, 'human'::text, 'agent'::text]))),
    CONSTRAINT task_ownership_events_handoff_check CHECK (((handoff_instruction IS NULL) OR ((btrim(handoff_instruction) <> ''::text) AND (octet_length(handoff_instruction) <= 8192)))),
    CONSTRAINT task_ownership_events_new_owner_check CHECK ((((new_owner_principal_id IS NULL) AND (new_owner_kind = 'unassigned'::text)) OR ((new_owner_principal_id IS NOT NULL) AND (new_owner_kind = ANY (ARRAY['human'::text, 'agent'::text]))))),
    CONSTRAINT task_ownership_events_operation_check CHECK ((operation = ANY (ARRAY['initial_assignment'::text, 'claim'::text, 'assign'::text, 'transfer'::text, 'release'::text, 'owner_removed'::text]))),
    CONSTRAINT task_ownership_events_previous_owner_check CHECK ((((previous_owner_principal_id IS NULL) AND (previous_owner_kind = 'unassigned'::text)) OR ((previous_owner_principal_id IS NOT NULL) AND (previous_owner_kind = ANY (ARRAY['human'::text, 'agent'::text]))))),
    CONSTRAINT task_ownership_events_reason_check CHECK ((reason = ANY (ARRAY['initial_assignment'::text, 'self_claim'::text, 'manual_assignment'::text, 'delegated'::text, 'workload_rebalance'::text, 'owner_unavailable'::text, 'released'::text, 'owner_removed'::text]))),
    CONSTRAINT task_ownership_events_reason_detail_check CHECK (((reason_detail IS NULL) OR (octet_length(reason_detail) <= 512))),
    CONSTRAINT task_ownership_events_transfer_handoff_check CHECK ((((operation = 'transfer'::text) AND (handoff_instruction IS NOT NULL)) OR ((operation <> 'transfer'::text) AND (handoff_instruction IS NULL)))),
    CONSTRAINT task_ownership_events_version_check CHECK (((from_version >= 0) AND (to_version = (from_version + 1)) AND (sequence = to_version)))
);


--
-- Name: task_status_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_status_events (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    task_id uuid NOT NULL,
    correlation_id uuid NOT NULL,
    sequence integer NOT NULL,
    from_status text,
    to_status text NOT NULL,
    reason text NOT NULL,
    actor_kind text NOT NULL,
    actor_id uuid,
    related_approval_id uuid,
    related_outreach_id uuid,
    retry_count integer NOT NULL,
    run_at timestamp with time zone NOT NULL,
    execution_generation uuid,
    transitioned_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT task_status_events_actor_kind_check CHECK ((actor_kind = ANY (ARRAY['system'::text, 'worker'::text, 'operator'::text, 'human'::text, 'agent'::text, 'approval'::text, 'outreach'::text]))),
    CONSTRAINT task_status_events_from_status_check CHECK (((from_status IS NULL) OR (from_status = ANY (ARRAY['pending'::text, 'processing'::text, 'pending_approval'::text, 'waiting_for_third_party_reply'::text, 'completed'::text, 'failed'::text, 'dead_letter'::text, 'stopped'::text])))),
    CONSTRAINT task_status_events_reason_check CHECK ((reason = ANY (ARRAY['enqueued'::text, 'claimed'::text, 'completed'::text, 'retryable_failure'::text, 'terminal_failure'::text, 'timed_out'::text, 'shutdown'::text, 'lease_lost'::text, 'approval_requested'::text, 'approval_accepted'::text, 'approval_rejected'::text, 'outreach_started'::text, 'outreach_reply_received'::text, 'outreach_timed_out'::text, 'outreach_extended'::text, 'operator_stopped'::text, 'operator_resumed'::text, 'ownership_transferred'::text, 'agent_instruction'::text, 'delegation_target_cancelled'::text, 'delegation_cancelled'::text, 'delegation_reassigned'::text, 'delegation_partial'::text, 'unknown'::text]))),
    CONSTRAINT task_status_events_related_source_check CHECK (((related_approval_id IS NULL) OR (related_outreach_id IS NULL))),
    CONSTRAINT task_status_events_retry_count_check CHECK ((retry_count >= 0)),
    CONSTRAINT task_status_events_sequence_check CHECK ((sequence > 0)),
    CONSTRAINT task_status_events_to_status_check CHECK ((to_status = ANY (ARRAY['pending'::text, 'processing'::text, 'pending_approval'::text, 'waiting_for_third_party_reply'::text, 'completed'::text, 'failed'::text, 'dead_letter'::text, 'stopped'::text])))
);


--
-- Name: thread_messages; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.thread_messages (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    message_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    entry_kind text NOT NULL,
    CONSTRAINT thread_messages_entry_kind_check CHECK ((entry_kind = ANY (ARRAY['conversation'::text, 'note'::text, 'delegation'::text, 'system_event'::text])))
);


--
-- Name: thread_principals; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.thread_principals (
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    role text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT thread_principals_role_check CHECK ((role = ANY (ARRAY['author'::text, 'participant'::text])))
);


--
-- Name: threads; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.threads (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    subject text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);


--
-- Name: user_login_methods; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_login_methods (
    user_id uuid NOT NULL,
    provider text NOT NULL,
    provider_subject text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT user_login_methods_provider_check CHECK ((provider = ANY (ARRAY['password'::text, 'google'::text, 'apple'::text]))),
    CONSTRAINT user_login_methods_subject_check CHECK ((((provider = 'password'::text) AND (provider_subject IS NULL)) OR ((provider = ANY (ARRAY['google'::text, 'apple'::text])) AND (provider_subject IS NOT NULL) AND (btrim(provider_subject) <> ''::text))))
);


--
-- Name: user_notification_preferences; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_notification_preferences (
    user_id uuid NOT NULL,
    assignment_email_enabled boolean DEFAULT true NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    response_review_email_enabled boolean DEFAULT true NOT NULL,
    delegation_timeout_email_enabled boolean DEFAULT true NOT NULL,
    task_failure_email_enabled boolean DEFAULT true NOT NULL,
    delivery_failure_email_enabled boolean DEFAULT true NOT NULL
);


--
-- Name: users; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.users (
    id uuid NOT NULL,
    username public.citext NOT NULL,
    email public.citext NOT NULL,
    password_hash text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    avatar_url text,
    CONSTRAINT users_avatar_url_scheme_check CHECK (((avatar_url IS NULL) OR (avatar_url ~ '^https?://'::text))),
    CONSTRAINT users_email_not_blank CHECK ((btrim((email)::text) <> ''::text)),
    CONSTRAINT users_username_not_blank CHECK ((btrim((username)::text) <> ''::text))
);


--
-- Name: agent_channel_provisions agent_channel_provisions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_pkey PRIMARY KEY (task_id, request_hash);


--
-- Name: agent_mcp_selection_revisions agent_mcp_selection_revisions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_mcp_selection_revisions
    ADD CONSTRAINT agent_mcp_selection_revisions_pkey PRIMARY KEY (company_id, agent_id);


--
-- Name: agent_mcp_selections agent_mcp_selections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_mcp_selections
    ADD CONSTRAINT agent_mcp_selections_pkey PRIMARY KEY (company_id, agent_id, connection_id);


--
-- Name: agent_skills agent_skills_agent_position_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_skills
    ADD CONSTRAINT agent_skills_agent_position_key UNIQUE (agent_id, "position");


--
-- Name: agent_skills agent_skills_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_skills
    ADD CONSTRAINT agent_skills_pkey PRIMARY KEY (agent_id, skill_id);


--
-- Name: agent_sub_agents agent_sub_agents_agent_position_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_sub_agents
    ADD CONSTRAINT agent_sub_agents_agent_position_key UNIQUE (agent_id, "position");


--
-- Name: agent_sub_agents agent_sub_agents_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_sub_agents
    ADD CONSTRAINT agent_sub_agents_pkey PRIMARY KEY (agent_id, sub_agent_id);


--
-- Name: agents agents_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agents
    ADD CONSTRAINT agents_company_id_id_key UNIQUE (company_id, id);


--
-- Name: agents agents_company_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agents
    ADD CONSTRAINT agents_company_slug_key UNIQUE (company_id, slug);


--
-- Name: agents agents_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agents
    ADD CONSTRAINT agents_pkey PRIMARY KEY (id);


--
-- Name: attention_source_events attention_source_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.attention_source_events
    ADD CONSTRAINT attention_source_events_pkey PRIMARY KEY (id);


--
-- Name: attention_source_events attention_source_events_source_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.attention_source_events
    ADD CONSTRAINT attention_source_events_source_command_key UNIQUE (company_id, source_kind, source_id, command_id);


--
-- Name: background_tasks background_tasks_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_company_id_id_key UNIQUE (company_id, id);


--
-- Name: background_tasks background_tasks_company_source_message_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_company_source_message_key UNIQUE (company_id, source_message_uuid);


--
-- Name: background_tasks background_tasks_company_source_schedule_run_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_company_source_schedule_run_key UNIQUE (company_id, source_schedule_run_id);


--
-- Name: background_tasks background_tasks_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_pkey PRIMARY KEY (id);


--
-- Name: binding_audit_events binding_audit_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.binding_audit_events
    ADD CONSTRAINT binding_audit_events_pkey PRIMARY KEY (id);


--
-- Name: channel_agents channel_agents_channel_position_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_agents
    ADD CONSTRAINT channel_agents_channel_position_key UNIQUE (channel_id, "position");


--
-- Name: channel_agents channel_agents_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_agents
    ADD CONSTRAINT channel_agents_pkey PRIMARY KEY (channel_id, agent_id);


--
-- Name: channel_bindings channel_bindings_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_bindings
    ADD CONSTRAINT channel_bindings_company_id_id_key UNIQUE (company_id, id);


--
-- Name: channel_bindings channel_bindings_company_transport_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_bindings
    ADD CONSTRAINT channel_bindings_company_transport_key UNIQUE (company_id, id, transport);


--
-- Name: channel_bindings channel_bindings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_bindings
    ADD CONSTRAINT channel_bindings_pkey PRIMARY KEY (id);


--
-- Name: channel_principal_grants channel_principal_grants_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_principal_grants
    ADD CONSTRAINT channel_principal_grants_pkey PRIMARY KEY (company_id, channel_id, principal_id, capability);


--
-- Name: channel_schedules channel_schedules_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_schedules
    ADD CONSTRAINT channel_schedules_pkey PRIMARY KEY (id);


--
-- Name: channel_slugs channel_slugs_company_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_slugs
    ADD CONSTRAINT channel_slugs_company_slug_key UNIQUE (company_id, slug);


--
-- Name: channel_slugs channel_slugs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_slugs
    ADD CONSTRAINT channel_slugs_pkey PRIMARY KEY (channel_id, slug);


--
-- Name: channels channels_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channels
    ADD CONSTRAINT channels_company_id_id_key UNIQUE (company_id, id);


--
-- Name: channels channels_owner_agent_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channels
    ADD CONSTRAINT channels_owner_agent_key UNIQUE (owner_agent_id);


--
-- Name: channels channels_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channels
    ADD CONSTRAINT channels_pkey PRIMARY KEY (id);


--
-- Name: companies companies_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.companies
    ADD CONSTRAINT companies_pkey PRIMARY KEY (id);


--
-- Name: companies companies_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.companies
    ADD CONSTRAINT companies_slug_key UNIQUE (slug);


--
-- Name: company_invites company_invites_company_email_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_invites
    ADD CONSTRAINT company_invites_company_email_key UNIQUE (company_id, email);


--
-- Name: company_invites company_invites_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_invites
    ADD CONSTRAINT company_invites_pkey PRIMARY KEY (id);


--
-- Name: company_mcp_connections company_mcp_connections_company_id_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_mcp_connections
    ADD CONSTRAINT company_mcp_connections_company_id_slug_key UNIQUE (company_id, slug);


--
-- Name: company_mcp_connections company_mcp_connections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_mcp_connections
    ADD CONSTRAINT company_mcp_connections_pkey PRIMARY KEY (company_id, id);


--
-- Name: company_mcp_credentials company_mcp_credentials_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_mcp_credentials
    ADD CONSTRAINT company_mcp_credentials_pkey PRIMARY KEY (company_id, connection_id);


--
-- Name: company_mcp_tool_grants company_mcp_tool_grants_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_mcp_tool_grants
    ADD CONSTRAINT company_mcp_tool_grants_pkey PRIMARY KEY (company_id, connection_id, tool_name);


--
-- Name: company_members company_members_company_user_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_members
    ADD CONSTRAINT company_members_company_user_key UNIQUE (company_id, user_id);


--
-- Name: company_members company_members_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_members
    ADD CONSTRAINT company_members_pkey PRIMARY KEY (id);


--
-- Name: company_model_connections company_model_connections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_model_connections
    ADD CONSTRAINT company_model_connections_pkey PRIMARY KEY (company_id, provider);


--
-- Name: company_resend_api_integrations company_resend_api_integrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_resend_api_integrations
    ADD CONSTRAINT company_resend_api_integrations_pkey PRIMARY KEY (company_id);


--
-- Name: company_resend_api_integrations company_resend_api_integrations_webhook_token_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_resend_api_integrations
    ADD CONSTRAINT company_resend_api_integrations_webhook_token_key UNIQUE (webhook_token);


--
-- Name: delegation_control_commands delegation_control_commands_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.delegation_control_commands
    ADD CONSTRAINT delegation_control_commands_pkey PRIMARY KEY (id);


--
-- Name: delegation_control_commands delegation_control_commands_task_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.delegation_control_commands
    ADD CONSTRAINT delegation_control_commands_task_command_key UNIQUE (task_id, command_id);


--
-- Name: email_message_metadata email_message_metadata_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.email_message_metadata
    ADD CONSTRAINT email_message_metadata_pkey PRIMARY KEY (message_id);


--
-- Name: external_messages external_messages_binding_key_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_messages
    ADD CONSTRAINT external_messages_binding_key_key UNIQUE (binding_id, external_message_key);


--
-- Name: external_messages external_messages_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_messages
    ADD CONSTRAINT external_messages_company_id_id_key UNIQUE (company_id, id);


--
-- Name: external_messages external_messages_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_messages
    ADD CONSTRAINT external_messages_pkey PRIMARY KEY (id);


--
-- Name: external_threads external_threads_binding_key_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_threads
    ADD CONSTRAINT external_threads_binding_key_key UNIQUE (binding_id, external_thread_key);


--
-- Name: external_threads external_threads_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_threads
    ADD CONSTRAINT external_threads_company_id_id_key UNIQUE (company_id, id);


--
-- Name: external_threads external_threads_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_threads
    ADD CONSTRAINT external_threads_pkey PRIMARY KEY (id);


--
-- Name: human_approvals human_approvals_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_company_id_id_key UNIQUE (company_id, id);


--
-- Name: human_approvals human_approvals_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_pkey PRIMARY KEY (id);


--
-- Name: human_approvals human_approvals_task_scope; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_task_scope UNIQUE (company_id, task_id, id);


--
-- Name: human_approvals human_approvals_thread_step_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_thread_step_key UNIQUE (company_id, channel_id, thread_id, step_key);


--
-- Name: human_approvals human_approvals_token_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_token_key UNIQUE (token);


--
-- Name: human_task_completions human_task_completions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_task_completions
    ADD CONSTRAINT human_task_completions_pkey PRIMARY KEY (task_id);


--
-- Name: human_task_completions human_task_completions_task_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_task_completions
    ADD CONSTRAINT human_task_completions_task_command_key UNIQUE (task_id, command_id);


--
-- Name: inbound_events inbound_events_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inbound_events
    ADD CONSTRAINT inbound_events_company_id_id_key UNIQUE (company_id, id);


--
-- Name: inbound_events inbound_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inbound_events
    ADD CONSTRAINT inbound_events_pkey PRIMARY KEY (id);


--
-- Name: inbound_events inbound_events_transport_external_event_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inbound_events
    ADD CONSTRAINT inbound_events_transport_external_event_key UNIQUE (transport, external_event_key);


--
-- Name: integration_credentials integration_credentials_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.integration_credentials
    ADD CONSTRAINT integration_credentials_pkey PRIMARY KEY (company_id, installation_id, credential_kind);


--
-- Name: integration_installations integration_installations_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.integration_installations
    ADD CONSTRAINT integration_installations_company_id_id_key UNIQUE (company_id, id);


--
-- Name: integration_installations integration_installations_company_transport_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.integration_installations
    ADD CONSTRAINT integration_installations_company_transport_key UNIQUE (company_id, id, transport);


--
-- Name: integration_installations integration_installations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.integration_installations
    ADD CONSTRAINT integration_installations_pkey PRIMARY KEY (id);


--
-- Name: integration_installations integration_installations_tenant_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.integration_installations
    ADD CONSTRAINT integration_installations_tenant_key UNIQUE (transport, external_tenant_key);


--
-- Name: internal_note_tombstones internal_note_tombstones_company_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_note_tombstones
    ADD CONSTRAINT internal_note_tombstones_company_command_key UNIQUE (company_id, command_id);


--
-- Name: internal_note_tombstones internal_note_tombstones_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_note_tombstones
    ADD CONSTRAINT internal_note_tombstones_pkey PRIMARY KEY (note_id);


--
-- Name: internal_notes internal_notes_company_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_company_command_key UNIQUE (company_id, command_id);


--
-- Name: internal_notes internal_notes_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_company_id_id_key UNIQUE (company_id, id);


--
-- Name: internal_notes internal_notes_message_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_message_key UNIQUE (company_id, message_id);


--
-- Name: internal_notes internal_notes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_pkey PRIMARY KEY (id);


--
-- Name: internal_notes internal_notes_supersedes_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_supersedes_key UNIQUE (supersedes_note_id);


--
-- Name: manual_handoffs manual_handoffs_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.manual_handoffs
    ADD CONSTRAINT manual_handoffs_company_id_id_key UNIQUE (company_id, id);


--
-- Name: manual_handoffs manual_handoffs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.manual_handoffs
    ADD CONSTRAINT manual_handoffs_pkey PRIMARY KEY (id);


--
-- Name: memory_cleanup_jobs memory_cleanup_jobs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_cleanup_jobs
    ADD CONSTRAINT memory_cleanup_jobs_pkey PRIMARY KEY (id);


--
-- Name: memory_cleanup_jobs memory_cleanup_jobs_provider_remote_database_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_cleanup_jobs
    ADD CONSTRAINT memory_cleanup_jobs_provider_remote_database_id_key UNIQUE (provider, remote_database_id);


--
-- Name: memory_provider_connections memory_provider_connections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_provider_connections
    ADD CONSTRAINT memory_provider_connections_pkey PRIMARY KEY (company_id, provider);


--
-- Name: memory_provider_connections memory_provider_connections_provider_remote_database_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_provider_connections
    ADD CONSTRAINT memory_provider_connections_provider_remote_database_id_key UNIQUE (provider, remote_database_id);


--
-- Name: memory_provisioning_jobs memory_provisioning_jobs_company_id_provider_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_provisioning_jobs
    ADD CONSTRAINT memory_provisioning_jobs_company_id_provider_key UNIQUE (company_id, provider);


--
-- Name: memory_provisioning_jobs memory_provisioning_jobs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_provisioning_jobs
    ADD CONSTRAINT memory_provisioning_jobs_pkey PRIMARY KEY (id);


--
-- Name: memory_provisioning_jobs memory_provisioning_jobs_provider_remote_database_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_provisioning_jobs
    ADD CONSTRAINT memory_provisioning_jobs_provider_remote_database_id_key UNIQUE (provider, remote_database_id);


--
-- Name: memory_remote_resource_lifecycles memory_remote_resource_lifecycles_company_id_provider_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_remote_resource_lifecycles
    ADD CONSTRAINT memory_remote_resource_lifecycles_company_id_provider_key UNIQUE (company_id, provider);


--
-- Name: memory_remote_resource_lifecycles memory_remote_resource_lifecycles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_remote_resource_lifecycles
    ADD CONSTRAINT memory_remote_resource_lifecycles_pkey PRIMARY KEY (provider, remote_database_id);


--
-- Name: message_deliveries message_deliveries_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_company_id_id_key UNIQUE (company_id, id);


--
-- Name: message_deliveries message_deliveries_destination_key_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_destination_key_key UNIQUE (destination_binding_id, idempotency_key);


--
-- Name: message_deliveries message_deliveries_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_pkey PRIMARY KEY (id);


--
-- Name: message_delivery_parts message_delivery_parts_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_delivery_parts
    ADD CONSTRAINT message_delivery_parts_company_id_id_key UNIQUE (company_id, id);


--
-- Name: message_delivery_parts message_delivery_parts_delivery_index_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_delivery_parts
    ADD CONSTRAINT message_delivery_parts_delivery_index_key UNIQUE (delivery_id, part_index);


--
-- Name: message_delivery_parts message_delivery_parts_delivery_key_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_delivery_parts
    ADD CONSTRAINT message_delivery_parts_delivery_key_key UNIQUE (delivery_id, part_key);


--
-- Name: message_delivery_parts message_delivery_parts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_delivery_parts
    ADD CONSTRAINT message_delivery_parts_pkey PRIMARY KEY (id);


--
-- Name: message_participants message_participants_identity_kind_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_participants
    ADD CONSTRAINT message_participants_identity_kind_key UNIQUE (message_id, kind, participant_identity_id);


--
-- Name: message_participants message_participants_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_participants
    ADD CONSTRAINT message_participants_pkey PRIMARY KEY (message_id, kind, "position");


--
-- Name: messages messages_company_id_id_audience_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_company_id_id_audience_key UNIQUE (company_id, id, audience);


--
-- Name: messages messages_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_company_id_id_key UNIQUE (company_id, id);


--
-- Name: messages messages_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_pkey PRIMARY KEY (id);


--
-- Name: notification_events notification_events_identity_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notification_events
    ADD CONSTRAINT notification_events_identity_key UNIQUE (company_id, source_kind, source_id, action_kind, source_generation);


--
-- Name: notification_events notification_events_notification_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notification_events
    ADD CONSTRAINT notification_events_notification_id_key UNIQUE (notification_id);


--
-- Name: notification_events notification_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notification_events
    ADD CONSTRAINT notification_events_pkey PRIMARY KEY (id);


--
-- Name: notifications notifications_email_delivery_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_email_delivery_key UNIQUE (email_delivery_id);


--
-- Name: notifications notifications_identity_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_identity_key UNIQUE (company_id, recipient_user_id, source_kind, source_id, action_kind, source_generation);


--
-- Name: notifications notifications_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_pkey PRIMARY KEY (id);


--
-- Name: participant_identities participant_identities_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.participant_identities
    ADD CONSTRAINT participant_identities_company_id_id_key UNIQUE (company_id, id);


--
-- Name: participant_identities participant_identities_company_principal_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.participant_identities
    ADD CONSTRAINT participant_identities_company_principal_id_key UNIQUE (company_id, principal_id, id);


--
-- Name: participant_identities participant_identities_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.participant_identities
    ADD CONSTRAINT participant_identities_pkey PRIMARY KEY (id);


--
-- Name: participant_identities participant_identities_qualified_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.participant_identities
    ADD CONSTRAINT participant_identities_qualified_key UNIQUE (company_id, transport, namespace, subject);


--
-- Name: pending_account_changes pending_account_changes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pending_account_changes
    ADD CONSTRAINT pending_account_changes_pkey PRIMARY KEY (user_id, kind);


--
-- Name: pending_user_registrations pending_user_registrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pending_user_registrations
    ADD CONSTRAINT pending_user_registrations_pkey PRIMARY KEY (email);


--
-- Name: principals principals_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.principals
    ADD CONSTRAINT principals_company_id_id_key UNIQUE (company_id, id);


--
-- Name: principals principals_company_id_id_kind_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.principals
    ADD CONSTRAINT principals_company_id_id_kind_key UNIQUE (company_id, id, kind);


--
-- Name: principals principals_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.principals
    ADD CONSTRAINT principals_pkey PRIMARY KEY (id);


--
-- Name: response_draft_evidence response_draft_evidence_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_draft_evidence
    ADD CONSTRAINT response_draft_evidence_pkey PRIMARY KEY (draft_id, draft_version, id);


--
-- Name: response_draft_evidence response_draft_evidence_position_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_draft_evidence
    ADD CONSTRAINT response_draft_evidence_position_key UNIQUE (draft_id, draft_version, "position");


--
-- Name: response_draft_publications response_draft_publications_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_draft_publications
    ADD CONSTRAINT response_draft_publications_pkey PRIMARY KEY (draft_id);


--
-- Name: response_drafts response_drafts_company_id_id_version_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_company_id_id_version_key UNIQUE (company_id, id, version);


--
-- Name: response_drafts response_drafts_company_message_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_company_message_key UNIQUE (company_id, proposed_message_id);


--
-- Name: response_drafts response_drafts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_pkey PRIMARY KEY (id, version);


--
-- Name: response_review_commands response_review_commands_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_review_commands
    ADD CONSTRAINT response_review_commands_pkey PRIMARY KEY (company_id, command_id);


--
-- Name: response_reviews response_reviews_company_id_draft_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_reviews
    ADD CONSTRAINT response_reviews_company_id_draft_key UNIQUE (company_id, draft_id, draft_version);


--
-- Name: response_reviews response_reviews_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_reviews
    ADD CONSTRAINT response_reviews_pkey PRIMARY KEY (draft_id, draft_version);


--
-- Name: runtime_metric_samples runtime_metric_samples_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.runtime_metric_samples
    ADD CONSTRAINT runtime_metric_samples_pkey PRIMARY KEY (machine_id, sampled_at);


--
-- Name: CONSTRAINT runtime_metric_samples_pkey ON runtime_metric_samples; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON CONSTRAINT runtime_metric_samples_pkey ON public.runtime_metric_samples IS 'Supports runtime history reads on (machine_id, sampled_at)';


--
-- Name: schedule_runs schedule_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_pkey PRIMARY KEY (id);


--
-- Name: schedule_runs schedule_runs_schedule_slot_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_schedule_slot_key UNIQUE (schedule_id, scheduled_for);


--
-- Name: skills skills_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.skills
    ADD CONSTRAINT skills_company_id_id_key UNIQUE (company_id, id);


--
-- Name: skills skills_company_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.skills
    ADD CONSTRAINT skills_company_slug_key UNIQUE (company_id, slug);


--
-- Name: skills skills_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.skills
    ADD CONSTRAINT skills_pkey PRIMARY KEY (id);


--
-- Name: start_agent_task_commands start_agent_task_commands_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.start_agent_task_commands
    ADD CONSTRAINT start_agent_task_commands_pkey PRIMARY KEY (company_id, command_id);


--
-- Name: task_agent_instruction_notes task_agent_instruction_notes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instruction_notes
    ADD CONSTRAINT task_agent_instruction_notes_pkey PRIMARY KEY (instruction_id, note_id);


--
-- Name: task_agent_instruction_notes task_agent_instruction_notes_position_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instruction_notes
    ADD CONSTRAINT task_agent_instruction_notes_position_key UNIQUE (instruction_id, "position");


--
-- Name: task_agent_instructions task_agent_instructions_company_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instructions
    ADD CONSTRAINT task_agent_instructions_company_command_key UNIQUE (company_id, command_id);


--
-- Name: task_agent_instructions task_agent_instructions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instructions
    ADD CONSTRAINT task_agent_instructions_pkey PRIMARY KEY (id);


--
-- Name: task_approval_waits task_approval_waits_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_approval_waits
    ADD CONSTRAINT task_approval_waits_pkey PRIMARY KEY (task_id);


--
-- Name: task_attempts task_attempts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_attempts
    ADD CONSTRAINT task_attempts_pkey PRIMARY KEY (id);


--
-- Name: task_attempts task_attempts_task_attempt_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_attempts
    ADD CONSTRAINT task_attempts_task_attempt_key UNIQUE (task_id, attempt_number);


--
-- Name: task_channel_targets task_channel_targets_channel_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_channel_targets
    ADD CONSTRAINT task_channel_targets_channel_key UNIQUE (task_id, channel_id);


--
-- Name: task_channel_targets task_channel_targets_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_channel_targets
    ADD CONSTRAINT task_channel_targets_pkey PRIMARY KEY (task_id, "position");


--
-- Name: task_harness_invocations task_harness_invocations_company_id_task_id_run_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_invocations
    ADD CONSTRAINT task_harness_invocations_company_id_task_id_run_id_id_key UNIQUE (company_id, task_id, run_id, id);


--
-- Name: task_harness_invocations task_harness_invocations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_invocations
    ADD CONSTRAINT task_harness_invocations_pkey PRIMARY KEY (id);


--
-- Name: task_harness_invocations task_harness_invocations_run_id_model_turn_call_ordinal_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_invocations
    ADD CONSTRAINT task_harness_invocations_run_id_model_turn_call_ordinal_key UNIQUE (run_id, model_turn, call_ordinal);


--
-- Name: task_harness_runs task_harness_runs_company_id_task_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_runs
    ADD CONSTRAINT task_harness_runs_company_id_task_id_id_key UNIQUE (company_id, task_id, id);


--
-- Name: task_harness_runs task_harness_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_runs
    ADD CONSTRAINT task_harness_runs_pkey PRIMARY KEY (id);


--
-- Name: task_outreach_replies task_outreach_replies_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_replies
    ADD CONSTRAINT task_outreach_replies_pkey PRIMARY KEY (id);


--
-- Name: task_outreach_replies task_outreach_replies_response_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_replies
    ADD CONSTRAINT task_outreach_replies_response_key UNIQUE (response_association_id);


--
-- Name: task_outreach_targets task_outreach_targets_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_company_id_id_key UNIQUE (company_id, id);


--
-- Name: task_outreach_targets task_outreach_targets_company_outreach_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_company_outreach_id_key UNIQUE (company_id, outreach_id, id);


--
-- Name: task_outreach_targets task_outreach_targets_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_pkey PRIMARY KEY (outreach_id, email);


--
-- Name: task_outreaches task_outreaches_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreaches
    ADD CONSTRAINT task_outreaches_company_id_id_key UNIQUE (company_id, id);


--
-- Name: task_outreaches task_outreaches_company_task_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreaches
    ADD CONSTRAINT task_outreaches_company_task_id_key UNIQUE (company_id, task_id, id);


--
-- Name: task_outreaches task_outreaches_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreaches
    ADD CONSTRAINT task_outreaches_pkey PRIMARY KEY (id);


--
-- Name: task_outreaches task_outreaches_task_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreaches
    ADD CONSTRAINT task_outreaches_task_key UNIQUE (task_id, outreach_key);


--
-- Name: task_ownership_events task_ownership_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_ownership_events
    ADD CONSTRAINT task_ownership_events_pkey PRIMARY KEY (id);


--
-- Name: task_ownership_events task_ownership_events_task_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_ownership_events
    ADD CONSTRAINT task_ownership_events_task_command_key UNIQUE (task_id, command_id);


--
-- Name: task_ownership_events task_ownership_events_task_sequence_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_ownership_events
    ADD CONSTRAINT task_ownership_events_task_sequence_key UNIQUE (task_id, sequence);


--
-- Name: task_status_events task_status_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_status_events
    ADD CONSTRAINT task_status_events_pkey PRIMARY KEY (id);


--
-- Name: task_status_events task_status_events_task_sequence_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_status_events
    ADD CONSTRAINT task_status_events_task_sequence_key UNIQUE (task_id, sequence);


--
-- Name: thread_messages thread_messages_channel_message_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_messages
    ADD CONSTRAINT thread_messages_channel_message_key UNIQUE (channel_id, message_id);


--
-- Name: thread_messages thread_messages_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_messages
    ADD CONSTRAINT thread_messages_company_id_id_key UNIQUE (company_id, id);


--
-- Name: thread_messages thread_messages_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_messages
    ADD CONSTRAINT thread_messages_pkey PRIMARY KEY (id);


--
-- Name: thread_messages thread_messages_thread_message_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_messages
    ADD CONSTRAINT thread_messages_thread_message_key UNIQUE (thread_id, message_id);


--
-- Name: thread_principals thread_principals_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_principals
    ADD CONSTRAINT thread_principals_pkey PRIMARY KEY (company_id, channel_id, thread_id, principal_id, role);


--
-- Name: threads threads_channel_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.threads
    ADD CONSTRAINT threads_channel_id_key UNIQUE (channel_id, id);


--
-- Name: threads threads_company_channel_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.threads
    ADD CONSTRAINT threads_company_channel_id_key UNIQUE (company_id, channel_id, id);


--
-- Name: threads threads_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.threads
    ADD CONSTRAINT threads_company_id_id_key UNIQUE (company_id, id);


--
-- Name: threads threads_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.threads
    ADD CONSTRAINT threads_pkey PRIMARY KEY (id);


--
-- Name: user_login_methods user_login_methods_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_login_methods
    ADD CONSTRAINT user_login_methods_pkey PRIMARY KEY (user_id, provider);


--
-- Name: user_notification_preferences user_notification_preferences_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_notification_preferences
    ADD CONSTRAINT user_notification_preferences_pkey PRIMARY KEY (user_id);


--
-- Name: users users_email_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_email_key UNIQUE (email);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);


--
-- Name: users users_username_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_username_key UNIQUE (username);


--
-- Name: agent_skills_skill_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX agent_skills_skill_idx ON public.agent_skills USING btree (skill_id, agent_id);


--
-- Name: agent_sub_agents_sub_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX agent_sub_agents_sub_idx ON public.agent_sub_agents USING btree (sub_agent_id, agent_id);


--
-- Name: agents_company_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX agents_company_created_idx ON public.agents USING btree (company_id, created_at DESC, id DESC);


--
-- Name: agents_library_slug_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX agents_library_slug_key ON public.agents USING btree (slug) WHERE (company_id IS NULL);


--
-- Name: background_tasks_company_channel_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_company_channel_created_idx ON public.background_tasks USING btree (company_id, channel_id, created_at DESC, id DESC);


--
-- Name: background_tasks_company_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_company_created_idx ON public.background_tasks USING btree (company_id, created_at DESC, id DESC);


--
-- Name: background_tasks_company_status_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_company_status_created_idx ON public.background_tasks USING btree (company_id, status, created_at DESC, id DESC);


--
-- Name: background_tasks_company_updated_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_company_updated_idx ON public.background_tasks USING btree (company_id, updated_at DESC);


--
-- Name: background_tasks_correlation_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_correlation_idx ON public.background_tasks USING btree (correlation_id, created_at);


--
-- Name: background_tasks_pending_ready_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_pending_ready_idx ON public.background_tasks USING btree (run_at, created_at, id) WHERE (status = 'pending'::text);


--
-- Name: background_tasks_processing_lease_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_processing_lease_idx ON public.background_tasks USING btree (lock_expires_at, id) WHERE (status = 'processing'::text);


--
-- Name: background_tasks_schedule_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_schedule_idx ON public.background_tasks USING btree (((payload ->> 'schedule_id'::text))) WHERE (task_type = 'scheduled_agent_run'::text);


--
-- Name: background_tasks_thread_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_thread_idx ON public.background_tasks USING btree (thread_id) WHERE (thread_id IS NOT NULL);


--
-- Name: background_tasks_unsettled_channel_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_unsettled_channel_idx ON public.background_tasks USING btree (company_id, channel_id) WHERE (status = ANY (ARRAY['pending'::text, 'processing'::text, 'pending_approval'::text, 'waiting_for_third_party_reply'::text, 'stopped'::text, 'failed'::text, 'dead_letter'::text]));


--
-- Name: background_tasks_unsettled_owner_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_unsettled_owner_idx ON public.background_tasks USING btree (company_id, owner_principal_id) WHERE (status = ANY (ARRAY['pending'::text, 'processing'::text, 'pending_approval'::text, 'waiting_for_third_party_reply'::text, 'stopped'::text, 'failed'::text, 'dead_letter'::text]));


--
-- Name: background_tasks_waiting_due_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_waiting_due_idx ON public.background_tasks USING btree (wait_expires_at, id) WHERE ((status = 'waiting_for_third_party_reply'::text) AND (wait_expires_at IS NOT NULL));


--
-- Name: binding_audit_events_binding_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX binding_audit_events_binding_idx ON public.binding_audit_events USING btree (company_id, binding_id, created_at DESC, id DESC);


--
-- Name: channel_agents_agent_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channel_agents_agent_idx ON public.channel_agents USING btree (agent_id, channel_id);


--
-- Name: channel_bindings_canonical_deployment_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX channel_bindings_canonical_deployment_idx ON public.channel_bindings USING btree (company_id, channel_id, transport) WHERE ((installation_id IS NULL) AND (status = ANY (ARRAY['active'::text, 'paused'::text])));


--
-- Name: channel_bindings_channel_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channel_bindings_channel_idx ON public.channel_bindings USING btree (company_id, channel_id, transport, status);


--
-- Name: channel_bindings_deployment_endpoint_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX channel_bindings_deployment_endpoint_idx ON public.channel_bindings USING btree (transport, namespace, external_endpoint_key) WHERE ((installation_id IS NULL) AND (status = ANY (ARRAY['active'::text, 'paused'::text])));


--
-- Name: channel_bindings_installation_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channel_bindings_installation_idx ON public.channel_bindings USING btree (installation_id, status) WHERE (installation_id IS NOT NULL);


--
-- Name: channel_bindings_installed_endpoint_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX channel_bindings_installed_endpoint_idx ON public.channel_bindings USING btree (installation_id, transport, namespace, external_endpoint_key) WHERE ((installation_id IS NOT NULL) AND (status = ANY (ARRAY['active'::text, 'paused'::text])));


--
-- Name: channel_principal_grants_principal_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channel_principal_grants_principal_idx ON public.channel_principal_grants USING btree (company_id, principal_id, channel_id, capability);


--
-- Name: channel_schedules_channel_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channel_schedules_channel_idx ON public.channel_schedules USING btree (channel_id, created_at DESC, id DESC);


--
-- Name: channel_schedules_company_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channel_schedules_company_idx ON public.channel_schedules USING btree (company_id, created_at DESC, id DESC);


--
-- Name: channel_schedules_due_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channel_schedules_due_idx ON public.channel_schedules USING btree (next_run_at, id) WHERE ((enabled = true) AND (next_run_at IS NOT NULL));


--
-- Name: channel_slugs_primary_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX channel_slugs_primary_idx ON public.channel_slugs USING btree (channel_id) WHERE is_primary;


--
-- Name: channels_company_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channels_company_created_idx ON public.channels USING btree (company_id, created_at DESC, id DESC);


--
-- Name: companies_user_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX companies_user_created_idx ON public.companies USING btree (user_id, created_at DESC, id DESC);


--
-- Name: company_invites_company_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX company_invites_company_created_idx ON public.company_invites USING btree (company_id, created_at DESC, id DESC);


--
-- Name: company_invites_email_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX company_invites_email_created_idx ON public.company_invites USING btree (email, created_at DESC, id DESC);


--
-- Name: company_members_company_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX company_members_company_created_idx ON public.company_members USING btree (company_id, created_at, id);


--
-- Name: company_members_one_owner_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX company_members_one_owner_idx ON public.company_members USING btree (company_id) WHERE (role = 'owner'::text);


--
-- Name: company_members_user_company_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX company_members_user_company_idx ON public.company_members USING btree (user_id, company_id);


--
-- Name: company_model_connections_one_default_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX company_model_connections_one_default_idx ON public.company_model_connections USING btree (company_id) WHERE is_default;


--
-- Name: delegation_control_commands_outreach_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX delegation_control_commands_outreach_idx ON public.delegation_control_commands USING btree (outreach_id, occurred_at, id);


--
-- Name: email_message_metadata_company_rfc_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX email_message_metadata_company_rfc_idx ON public.email_message_metadata USING btree (company_id, rfc_message_id);


--
-- Name: email_message_metadata_in_reply_to_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX email_message_metadata_in_reply_to_idx ON public.email_message_metadata USING btree (company_id, in_reply_to) WHERE (in_reply_to IS NOT NULL);


--
-- Name: email_message_metadata_thread_index_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX email_message_metadata_thread_index_idx ON public.email_message_metadata USING btree (company_id, thread_index) WHERE (thread_index IS NOT NULL);


--
-- Name: external_messages_delivery_part_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX external_messages_delivery_part_idx ON public.external_messages USING btree (delivery_part_id) WHERE (delivery_part_id IS NOT NULL);


--
-- Name: external_messages_message_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX external_messages_message_idx ON public.external_messages USING btree (company_id, message_id, binding_id);


--
-- Name: external_threads_thread_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX external_threads_thread_idx ON public.external_threads USING btree (company_id, thread_id, binding_id);


--
-- Name: human_approvals_channel_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX human_approvals_channel_created_idx ON public.human_approvals USING btree (company_id, channel_id, created_at DESC, id DESC);


--
-- Name: human_approvals_expiry_due; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX human_approvals_expiry_due ON public.human_approvals USING btree (expires_at, id) WHERE (status = 'pending'::text);


--
-- Name: human_approvals_pending_expiry_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX human_approvals_pending_expiry_idx ON public.human_approvals USING btree (expires_at, id) WHERE (status = 'pending'::text);


--
-- Name: human_approvals_task_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX human_approvals_task_idx ON public.human_approvals USING btree (task_id) WHERE (task_id IS NOT NULL);


--
-- Name: inbound_events_claimable_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX inbound_events_claimable_idx ON public.inbound_events USING btree (available_at, received_at, id) WHERE (status = ANY (ARRAY['pending'::text, 'retryable'::text]));


--
-- Name: inbound_events_company_installation_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX inbound_events_company_installation_created_idx ON public.inbound_events USING btree (company_id, installation_id, created_at DESC, id DESC);


--
-- Name: inbound_events_processed_retention_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX inbound_events_processed_retention_idx ON public.inbound_events USING btree (status, processed_at, id) WHERE (status = ANY (ARRAY['completed'::text, 'ignored'::text, 'dead_letter'::text]));


--
-- Name: inbound_events_processing_lease_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX inbound_events_processing_lease_idx ON public.inbound_events USING btree (lock_expires_at, id) WHERE (status = 'processing'::text);


--
-- Name: integration_installations_company_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX integration_installations_company_idx ON public.integration_installations USING btree (company_id, transport, installed_at DESC, id DESC);


--
-- Name: memory_cleanup_jobs_due_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX memory_cleanup_jobs_due_idx ON public.memory_cleanup_jobs USING btree (available_at, created_at, id) WHERE (status = 'pending'::text);


--
-- Name: memory_provisioning_jobs_due_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX memory_provisioning_jobs_due_idx ON public.memory_provisioning_jobs USING btree ((
CASE phase
    WHEN 'waiting_ready'::text THEN next_poll_at
    ELSE available_at
END), created_at, id) WHERE (status = 'pending'::text);


--
-- Name: message_deliveries_claimable_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_claimable_idx ON public.message_deliveries USING btree (available_at, id) WHERE (status = ANY (ARRAY['pending'::text, 'retryable'::text]));


--
-- Name: message_deliveries_company_channel_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_company_channel_created_idx ON public.message_deliveries USING btree (company_id, channel_id, created_at DESC, id DESC);


--
-- Name: message_deliveries_company_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_company_created_idx ON public.message_deliveries USING btree (company_id, created_at DESC, id DESC);


--
-- Name: message_deliveries_company_status_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_company_status_idx ON public.message_deliveries USING btree (company_id, status);


--
-- Name: message_deliveries_company_updated_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_company_updated_idx ON public.message_deliveries USING btree (company_id, updated_at DESC);


--
-- Name: message_deliveries_correlation_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_correlation_idx ON public.message_deliveries USING btree (correlation_id, created_at);


--
-- Name: message_deliveries_dependency_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_dependency_idx ON public.message_deliveries USING btree (depends_on_delivery_id) WHERE (depends_on_delivery_id IS NOT NULL);


--
-- Name: message_deliveries_message_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_message_idx ON public.message_deliveries USING btree (company_id, message_id);


--
-- Name: message_deliveries_sending_lease_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_sending_lease_idx ON public.message_deliveries USING btree (lock_expires_at, id) WHERE (status = 'sending'::text);


--
-- Name: message_deliveries_standalone_key_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX message_deliveries_standalone_key_key ON public.message_deliveries USING btree (transport, idempotency_key) WHERE (destination_binding_id IS NULL);


--
-- Name: message_deliveries_task_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_deliveries_task_idx ON public.message_deliveries USING btree (task_id) WHERE (task_id IS NOT NULL);


--
-- Name: message_delivery_parts_delivery_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_delivery_parts_delivery_idx ON public.message_delivery_parts USING btree (delivery_id, part_index);


--
-- Name: message_delivery_parts_provider_key_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX message_delivery_parts_provider_key_idx ON public.message_delivery_parts USING btree (company_id, provider_message_key) WHERE (provider_message_key IS NOT NULL);


--
-- Name: message_delivery_parts_unfinished_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_delivery_parts_unfinished_idx ON public.message_delivery_parts USING btree (delivery_id, part_index) WHERE (status = ANY (ARRAY['prepared'::text, 'retryable'::text]));


--
-- Name: message_participants_identity_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX message_participants_identity_idx ON public.message_participants USING btree (company_id, participant_identity_id, message_id);


--
-- Name: messages_author_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX messages_author_idx ON public.messages USING btree (company_id, author_principal_id);


--
-- Name: messages_company_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX messages_company_created_idx ON public.messages USING btree (company_id, created_at DESC, id DESC);


--
-- Name: notification_events_claimable_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX notification_events_claimable_idx ON public.notification_events USING btree (available_at, occurred_at, id) WHERE (status = 'pending'::text);


--
-- Name: notification_events_processing_lease_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX notification_events_processing_lease_idx ON public.notification_events USING btree (lock_expires_at, id) WHERE (status = 'processing'::text);


--
-- Name: notification_events_source_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX notification_events_source_idx ON public.notification_events USING btree (company_id, source_kind, source_id, action_kind, source_generation DESC);


--
-- Name: notifications_active_age_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX notifications_active_age_idx ON public.notifications USING btree (created_at, id) WHERE (state = 'active'::text);


--
-- Name: notifications_one_active_action_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX notifications_one_active_action_idx ON public.notifications USING btree (company_id, recipient_user_id, source_kind, source_id, action_kind) WHERE (state = 'active'::text);


--
-- Name: notifications_recipient_list_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX notifications_recipient_list_idx ON public.notifications USING btree (company_id, recipient_user_id, state, created_at DESC, id DESC);


--
-- Name: participant_identities_principal_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX participant_identities_principal_idx ON public.participant_identities USING btree (company_id, principal_id, created_at, id);


--
-- Name: pending_user_registrations_username_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX pending_user_registrations_username_key ON public.pending_user_registrations USING btree (username);


--
-- Name: principals_company_agent_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX principals_company_agent_key ON public.principals USING btree (company_id, agent_id) WHERE (agent_id IS NOT NULL);


--
-- Name: principals_company_system_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX principals_company_system_key ON public.principals USING btree (company_id) WHERE (kind = 'system'::text);


--
-- Name: principals_company_user_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX principals_company_user_key ON public.principals USING btree (company_id, user_id) WHERE (user_id IS NOT NULL);


--
-- Name: response_drafts_one_pending_version_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX response_drafts_one_pending_version_idx ON public.response_drafts USING btree (id) WHERE (status = 'pending_review'::text);


--
-- Name: response_drafts_reviewer_pending_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX response_drafts_reviewer_pending_idx ON public.response_drafts USING btree (company_id, reviewer_principal_id, created_at, id) WHERE (status = 'pending_review'::text);


--
-- Name: response_drafts_scope_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX response_drafts_scope_idx ON public.response_drafts USING btree (company_id, channel_id, thread_id, id, version DESC);


--
-- Name: response_reviews_pending_expiry_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX response_reviews_pending_expiry_idx ON public.response_reviews USING btree (expires_at, draft_id) WHERE (status = 'pending'::text);


--
-- Name: schedule_runs_materialization_expired_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX schedule_runs_materialization_expired_idx ON public.schedule_runs USING btree (materialization_lock_expires_at, created_at, id) WHERE (materialization_status = 'materializing'::text);


--
-- Name: schedule_runs_materialization_ready_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX schedule_runs_materialization_ready_idx ON public.schedule_runs USING btree (materialization_available_at, created_at, id) WHERE (materialization_status = 'pending'::text);


--
-- Name: schedule_runs_schedule_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX schedule_runs_schedule_created_idx ON public.schedule_runs USING btree (schedule_id, created_at DESC, id DESC);


--
-- Name: skills_library_slug_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX skills_library_slug_key ON public.skills USING btree (slug) WHERE (company_id IS NULL);


--
-- Name: task_channel_targets_channel_task_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_channel_targets_channel_task_idx ON public.task_channel_targets USING btree (company_id, channel_id, task_id);


--
-- Name: task_channel_targets_thread_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_channel_targets_thread_idx ON public.task_channel_targets USING btree (company_id, channel_id, thread_id);


--
-- Name: task_harness_runs_scope; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX task_harness_runs_scope ON public.task_harness_runs USING btree (task_id, agent_id, ownership_version);


--
-- Name: task_harness_runs_task; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_harness_runs_task ON public.task_harness_runs USING btree (company_id, task_id);


--
-- Name: task_outreach_replies_target_received_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_outreach_replies_target_received_idx ON public.task_outreach_replies USING btree (target_id, received_at, id);


--
-- Name: task_outreach_targets_delivery_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX task_outreach_targets_delivery_idx ON public.task_outreach_targets USING btree (delivery_id) WHERE (delivery_id IS NOT NULL);


--
-- Name: task_outreach_targets_email_waiting_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_outreach_targets_email_waiting_idx ON public.task_outreach_targets USING btree (email, outreach_id) WHERE (responded_at IS NULL);


--
-- Name: task_outreach_targets_request_message_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_outreach_targets_request_message_idx ON public.task_outreach_targets USING btree (request_message_id) WHERE (request_message_id IS NOT NULL);


--
-- Name: task_outreach_targets_response_association_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_outreach_targets_response_association_idx ON public.task_outreach_targets USING btree (response_association_id) WHERE (response_association_id IS NOT NULL);


--
-- Name: task_outreaches_due_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_outreaches_due_idx ON public.task_outreaches USING btree (expires_at, id) WHERE ((status = 'waiting'::text) AND (expires_at IS NOT NULL));


--
-- Name: task_outreaches_task_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_outreaches_task_idx ON public.task_outreaches USING btree (task_id);


--
-- Name: task_outreaches_task_status_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_outreaches_task_status_idx ON public.task_outreaches USING btree (task_id, status);


--
-- Name: task_status_events_company_correlation_timeline_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_status_events_company_correlation_timeline_idx ON public.task_status_events USING btree (company_id, correlation_id, transitioned_at, task_id, sequence, id);


--
-- Name: task_status_events_task_history_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_status_events_task_history_idx ON public.task_status_events USING btree (task_id, transitioned_at, sequence, id);


--
-- Name: thread_messages_message_thread_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX thread_messages_message_thread_idx ON public.thread_messages USING btree (message_id, thread_id);


--
-- Name: thread_messages_thread_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX thread_messages_thread_created_idx ON public.thread_messages USING btree (thread_id, created_at, id);


--
-- Name: thread_principals_principal_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX thread_principals_principal_idx ON public.thread_principals USING btree (company_id, principal_id, thread_id, role);


--
-- Name: threads_channel_updated_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX threads_channel_updated_idx ON public.threads USING btree (channel_id, updated_at DESC, id DESC);


--
-- Name: user_login_methods_provider_subject_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX user_login_methods_provider_subject_key ON public.user_login_methods USING btree (provider, provider_subject) WHERE (provider_subject IS NOT NULL);


--
-- Name: notifications actionable_notifications_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER actionable_notifications_notify AFTER INSERT OR UPDATE OF state, read_at ON public.notifications FOR EACH ROW EXECUTE FUNCTION public.notify_actionable_notification_changed();


--
-- Name: agent_skills agent_skills_scope_check; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER agent_skills_scope_check BEFORE INSERT OR UPDATE ON public.agent_skills FOR EACH ROW EXECUTE FUNCTION public.enforce_agent_skill_scope();


--
-- Name: attention_source_events attention_source_events_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER attention_source_events_immutable BEFORE DELETE OR UPDATE ON public.attention_source_events FOR EACH ROW EXECUTE FUNCTION public.attention_source_events_are_immutable();


--
-- Name: background_tasks background_tasks_bump_attention_version; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_bump_attention_version BEFORE UPDATE OF owner_principal_id, owner_principal_kind ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.bump_task_attention_version_for_owner_change();


--
-- Name: background_tasks background_tasks_initialize_ownership; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_initialize_ownership BEFORE INSERT ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.initialize_task_ownership();


--
-- Name: background_tasks background_tasks_notify_activity; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_notify_activity AFTER INSERT OR UPDATE OF status ON public.background_tasks FOR EACH ROW WHEN ((new.thread_id IS NOT NULL)) EXECUTE FUNCTION public.notify_thread_activity();


--
-- Name: background_tasks background_tasks_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_notify_attention AFTER INSERT OR DELETE OR UPDATE OF status, owner_principal_id, owner_principal_kind, business_priority, business_due_at, attention_version ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.notify_attention_changed('task');


--
-- Name: background_tasks background_tasks_record_initial_ownership; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_record_initial_ownership AFTER INSERT ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.record_initial_task_ownership();


--
-- Name: background_tasks background_tasks_record_status_event; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_record_status_event AFTER INSERT OR UPDATE OF status ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.record_task_status_event();


--
-- Name: binding_audit_events binding_audit_events_append_only; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER binding_audit_events_append_only BEFORE DELETE OR UPDATE ON public.binding_audit_events FOR EACH ROW EXECUTE FUNCTION public.reject_binding_audit_rewrite();


--
-- Name: channel_agents channel_agents_scope_check; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER channel_agents_scope_check BEFORE INSERT OR UPDATE ON public.channel_agents FOR EACH ROW EXECUTE FUNCTION public.enforce_channel_agent_scope();


--
-- Name: channel_agents channel_assignment_active_agent_check; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER channel_assignment_active_agent_check AFTER INSERT OR DELETE OR UPDATE ON public.channel_agents DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.enforce_enabled_channel_has_active_agent();


--
-- Name: channels channels_delete_target_tasks; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER channels_delete_target_tasks BEFORE DELETE ON public.channels FOR EACH ROW EXECUTE FUNCTION public.delete_channel_target_tasks();


--
-- Name: message_deliveries delivery_actionable_notification; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER delivery_actionable_notification AFTER DELETE OR UPDATE OF status ON public.message_deliveries FOR EACH ROW EXECUTE FUNCTION public.notification_from_delivery();


--
-- Name: channels enabled_channel_active_agent_check; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER enabled_channel_active_agent_check AFTER INSERT OR UPDATE OF enabled ON public.channels DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.enforce_enabled_channel_has_active_agent();


--
-- Name: agents guard_active_agent_harness_change; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER guard_active_agent_harness_change BEFORE UPDATE OF harness_kind, response_contract ON public.agents FOR EACH ROW EXECUTE FUNCTION public.guard_active_agent_harness_change();


--
-- Name: agents guard_agent_mcp_harness; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER guard_agent_mcp_harness BEFORE UPDATE OF harness_kind ON public.agents FOR EACH ROW EXECUTE FUNCTION public.guard_agent_mcp_harness();


--
-- Name: attention_source_events handoff_actionable_notification; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER handoff_actionable_notification AFTER INSERT ON public.attention_source_events FOR EACH ROW EXECUTE FUNCTION public.notification_from_attention_source();


--
-- Name: manual_handoffs handoff_notification_delete; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER handoff_notification_delete AFTER DELETE ON public.manual_handoffs FOR EACH ROW EXECUTE FUNCTION public.notification_withdraw_deleted_source('handoff');


--
-- Name: human_approvals human_approvals_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER human_approvals_notify_attention AFTER INSERT OR DELETE OR UPDATE OF status, approver_principal_id, expires_at ON public.human_approvals FOR EACH ROW EXECUTE FUNCTION public.notify_attention_changed('approval');


--
-- Name: human_approvals human_approvals_notify_chain; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER human_approvals_notify_chain AFTER INSERT OR UPDATE OF status ON public.human_approvals FOR EACH ROW WHEN ((new.task_id IS NOT NULL)) EXECUTE FUNCTION public.notify_task_chain_changed();


--
-- Name: inbound_events inbound_events_notify_ready; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER inbound_events_notify_ready AFTER INSERT OR UPDATE OF status, available_at ON public.inbound_events FOR EACH ROW EXECUTE FUNCTION public.notify_inbound_event_ready();


--
-- Name: internal_note_tombstones internal_note_tombstones_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER internal_note_tombstones_immutable BEFORE DELETE OR UPDATE ON public.internal_note_tombstones FOR EACH ROW EXECUTE FUNCTION public.internal_note_tombstones_are_immutable();


--
-- Name: internal_note_tombstones internal_note_tombstones_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER internal_note_tombstones_notify AFTER INSERT ON public.internal_note_tombstones FOR EACH ROW EXECUTE FUNCTION public.notify_internal_note_change();


--
-- Name: internal_notes internal_notes_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER internal_notes_immutable BEFORE DELETE OR UPDATE ON public.internal_notes FOR EACH ROW EXECUTE FUNCTION public.internal_notes_are_immutable();


--
-- Name: internal_notes internal_notes_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER internal_notes_notify AFTER INSERT ON public.internal_notes FOR EACH ROW EXECUTE FUNCTION public.notify_internal_note_change();


--
-- Name: agents library_agent_delete_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER library_agent_delete_guard BEFORE DELETE ON public.agents FOR EACH ROW EXECUTE FUNCTION public.prevent_assigned_library_agent_delete();


--
-- Name: skills library_skill_delete_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER library_skill_delete_guard BEFORE DELETE ON public.skills FOR EACH ROW EXECUTE FUNCTION public.prevent_assigned_library_skill_delete();


--
-- Name: background_tasks lock_task_agent_harnesses; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER lock_task_agent_harnesses BEFORE INSERT OR UPDATE OF status, owner_principal_id, channel_id ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.lock_task_agent_harnesses();


--
-- Name: manual_handoffs manual_handoffs_bump_cleanup_version; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER manual_handoffs_bump_cleanup_version BEFORE UPDATE OF responsible_principal_id ON public.manual_handoffs FOR EACH ROW EXECUTE FUNCTION public.bump_handoff_version_for_responsibility_cleanup();


--
-- Name: manual_handoffs manual_handoffs_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER manual_handoffs_notify_attention AFTER INSERT OR DELETE OR UPDATE OF status, responsible_principal_id, business_priority, business_due_at, version ON public.manual_handoffs FOR EACH ROW EXECUTE FUNCTION public.notify_attention_changed('handoff');


--
-- Name: memory_provider_connections memory_connection_lifecycle_compatibility_delete; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER memory_connection_lifecycle_compatibility_delete BEFORE DELETE ON public.memory_provider_connections FOR EACH ROW EXECUTE FUNCTION public.retire_memory_lifecycle_for_legacy_connection();


--
-- Name: memory_provider_connections memory_connection_lifecycle_compatibility_insert; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER memory_connection_lifecycle_compatibility_insert AFTER INSERT ON public.memory_provider_connections FOR EACH ROW EXECUTE FUNCTION public.create_memory_lifecycle_for_legacy_connection();


--
-- Name: memory_provisioning_jobs memory_provisioning_phase_compatibility_update; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER memory_provisioning_phase_compatibility_update BEFORE UPDATE ON public.memory_provisioning_jobs FOR EACH ROW EXECUTE FUNCTION public.synchronize_legacy_memory_provisioning_phase();


--
-- Name: message_deliveries message_deliveries_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER message_deliveries_notify_attention AFTER INSERT OR DELETE OR UPDATE OF status ON public.message_deliveries FOR EACH ROW EXECUTE FUNCTION public.notify_attention_changed('delivery');


--
-- Name: message_deliveries message_deliveries_notify_chain; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER message_deliveries_notify_chain AFTER INSERT OR UPDATE OF status ON public.message_deliveries FOR EACH ROW EXECUTE FUNCTION public.notify_task_chain_changed();


--
-- Name: messages messages_audience_no_widen; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER messages_audience_no_widen BEFORE UPDATE OF audience ON public.messages FOR EACH ROW EXECUTE FUNCTION public.prevent_message_audience_widening();


--
-- Name: channel_principal_grants notification_channel_grant_recheck; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER notification_channel_grant_recheck AFTER INSERT OR DELETE OR UPDATE ON public.channel_principal_grants FOR EACH ROW EXECUTE FUNCTION public.notification_recheck_channel_access();


--
-- Name: channels notification_channel_policy_recheck; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER notification_channel_policy_recheck AFTER UPDATE OF access_mode ON public.channels FOR EACH ROW EXECUTE FUNCTION public.notification_recheck_channel_access();


--
-- Name: company_members notification_membership_recheck; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER notification_membership_recheck AFTER DELETE OR UPDATE OF role ON public.company_members FOR EACH ROW EXECUTE FUNCTION public.notification_recheck_membership();


--
-- Name: principals notification_principal_delete_recheck; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER notification_principal_delete_recheck BEFORE DELETE ON public.principals FOR EACH ROW EXECUTE FUNCTION public.notification_withdraw_deleted_principal();


--
-- Name: task_outreaches outreach_actionable_notification; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER outreach_actionable_notification AFTER DELETE OR UPDATE OF status ON public.task_outreaches FOR EACH ROW EXECUTE FUNCTION public.notification_from_outreach();


--
-- Name: channel_agents owned_channel_assignment_position_zero_check; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER owned_channel_assignment_position_zero_check AFTER INSERT OR DELETE OR UPDATE ON public.channel_agents DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.enforce_owned_channel_position_zero();


--
-- Name: channels owned_channel_delete_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER owned_channel_delete_guard BEFORE DELETE ON public.channels FOR EACH ROW EXECUTE FUNCTION public.prevent_owned_channel_delete();


--
-- Name: channels owned_channel_position_zero_check; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER owned_channel_position_zero_check AFTER INSERT OR UPDATE OF owner_agent_id ON public.channels DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.enforce_owned_channel_position_zero();


--
-- Name: principals principals_release_owned_tasks; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER principals_release_owned_tasks BEFORE DELETE ON public.principals FOR EACH ROW WHEN ((old.kind = ANY (ARRAY['person'::text, 'agent'::text]))) EXECUTE FUNCTION public.release_tasks_for_removed_principal();


--
-- Name: response_draft_evidence response_draft_evidence_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER response_draft_evidence_immutable BEFORE INSERT OR DELETE OR UPDATE ON public.response_draft_evidence FOR EACH ROW EXECUTE FUNCTION public.enforce_response_draft_evidence_immutability();


--
-- Name: response_drafts response_drafts_immutable_versions; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER response_drafts_immutable_versions BEFORE UPDATE ON public.response_drafts FOR EACH ROW EXECUTE FUNCTION public.enforce_response_draft_update();


--
-- Name: response_reviews response_review_actionable_notification; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER response_review_actionable_notification AFTER INSERT OR UPDATE OF status, reviewer_principal_id ON public.response_reviews FOR EACH ROW EXECUTE FUNCTION public.notification_from_review();


--
-- Name: response_reviews response_review_notification_delete; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER response_review_notification_delete AFTER DELETE ON public.response_reviews FOR EACH ROW EXECUTE FUNCTION public.notification_withdraw_deleted_source('response_review');


--
-- Name: response_reviews response_reviews_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER response_reviews_notify_attention AFTER INSERT OR DELETE OR UPDATE OF status, reviewer_principal_id, expires_at ON public.response_reviews FOR EACH ROW EXECUTE FUNCTION public.notify_attention_changed('response_review');


--
-- Name: task_agent_instructions task_agent_instructions_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_agent_instructions_notify AFTER INSERT ON public.task_agent_instructions FOR EACH ROW EXECUTE FUNCTION public.notify_agent_instruction();


--
-- Name: task_outreach_targets task_outreach_targets_notify_chain; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_outreach_targets_notify_chain AFTER UPDATE OF responded_at ON public.task_outreach_targets FOR EACH ROW WHEN ((old.responded_at IS DISTINCT FROM new.responded_at)) EXECUTE FUNCTION public.notify_task_chain_changed();


--
-- Name: task_outreach_targets task_outreach_targets_status_transition; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_outreach_targets_status_transition BEFORE UPDATE OF status ON public.task_outreach_targets FOR EACH ROW EXECUTE FUNCTION public.enforce_outreach_target_status_transition();


--
-- Name: task_outreaches task_outreaches_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_outreaches_notify_attention AFTER INSERT OR DELETE OR UPDATE OF status, expires_at, version ON public.task_outreaches FOR EACH ROW EXECUTE FUNCTION public.notify_attention_changed('delegation');


--
-- Name: task_outreaches task_outreaches_notify_chain; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_outreaches_notify_chain AFTER INSERT OR UPDATE OF status ON public.task_outreaches FOR EACH ROW EXECUTE FUNCTION public.notify_task_chain_changed();


--
-- Name: task_outreaches task_outreaches_status_transition; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_outreaches_status_transition BEFORE UPDATE OF status ON public.task_outreaches FOR EACH ROW EXECUTE FUNCTION public.enforce_outreach_status_transition();


--
-- Name: task_ownership_events task_ownership_actionable_notification; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_ownership_actionable_notification AFTER INSERT ON public.task_ownership_events FOR EACH ROW EXECUTE FUNCTION public.notification_from_task_ownership();


--
-- Name: task_ownership_events task_ownership_events_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_ownership_events_immutable BEFORE DELETE OR UPDATE ON public.task_ownership_events FOR EACH ROW EXECUTE FUNCTION public.task_ownership_events_are_immutable();


--
-- Name: task_ownership_events task_ownership_events_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_ownership_events_notify AFTER INSERT ON public.task_ownership_events FOR EACH ROW EXECUTE FUNCTION public.notify_task_ownership();


--
-- Name: background_tasks task_status_actionable_notification; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_status_actionable_notification AFTER DELETE OR UPDATE OF status ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.notification_from_task_status();


--
-- Name: task_status_events task_status_events_notify_chain; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_status_events_notify_chain AFTER INSERT ON public.task_status_events FOR EACH ROW EXECUTE FUNCTION public.notify_task_chain_changed();


--
-- Name: thread_messages thread_messages_delete_orphan_message; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER thread_messages_delete_orphan_message AFTER DELETE ON public.thread_messages FOR EACH ROW EXECUTE FUNCTION public.delete_orphan_message();


--
-- Name: thread_messages thread_messages_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER thread_messages_notify AFTER INSERT ON public.thread_messages FOR EACH ROW EXECUTE FUNCTION public.notify_thread_message();


--
-- Name: agent_channel_provisions agent_channel_provisions_agent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_agent_id_fkey FOREIGN KEY (agent_id) REFERENCES public.agents(id) ON DELETE CASCADE;


--
-- Name: agent_channel_provisions agent_channel_provisions_channel_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels(id) ON DELETE CASCADE;


--
-- Name: agent_channel_provisions agent_channel_provisions_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.background_tasks(id) ON DELETE CASCADE;


--
-- Name: agent_mcp_selection_revisions agent_mcp_selection_revisions_company_id_agent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_mcp_selection_revisions
    ADD CONSTRAINT agent_mcp_selection_revisions_company_id_agent_id_fkey FOREIGN KEY (company_id, agent_id) REFERENCES public.agents(company_id, id) ON DELETE CASCADE;


--
-- Name: agent_mcp_selections agent_mcp_selections_company_id_agent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_mcp_selections
    ADD CONSTRAINT agent_mcp_selections_company_id_agent_id_fkey FOREIGN KEY (company_id, agent_id) REFERENCES public.agents(company_id, id) ON DELETE CASCADE;


--
-- Name: agent_mcp_selections agent_mcp_selections_company_id_connection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_mcp_selections
    ADD CONSTRAINT agent_mcp_selections_company_id_connection_id_fkey FOREIGN KEY (company_id, connection_id) REFERENCES public.company_mcp_connections(company_id, id) ON DELETE RESTRICT;


--
-- Name: agent_skills agent_skills_agent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_skills
    ADD CONSTRAINT agent_skills_agent_id_fkey FOREIGN KEY (agent_id) REFERENCES public.agents(id) ON DELETE CASCADE;


--
-- Name: agent_skills agent_skills_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_skills
    ADD CONSTRAINT agent_skills_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: agent_skills agent_skills_skill_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_skills
    ADD CONSTRAINT agent_skills_skill_id_fkey FOREIGN KEY (skill_id) REFERENCES public.skills(id) ON DELETE CASCADE;


--
-- Name: agent_sub_agents agent_sub_agents_agent_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_sub_agents
    ADD CONSTRAINT agent_sub_agents_agent_fk FOREIGN KEY (company_id, agent_id) REFERENCES public.agents(company_id, id) ON DELETE CASCADE;


--
-- Name: agent_sub_agents agent_sub_agents_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_sub_agents
    ADD CONSTRAINT agent_sub_agents_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: agent_sub_agents agent_sub_agents_sub_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_sub_agents
    ADD CONSTRAINT agent_sub_agents_sub_fk FOREIGN KEY (company_id, sub_agent_id) REFERENCES public.agents(company_id, id) ON DELETE CASCADE;


--
-- Name: agents agents_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agents
    ADD CONSTRAINT agents_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: attention_source_events attention_source_events_company_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.attention_source_events
    ADD CONSTRAINT attention_source_events_company_fk FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: background_tasks background_tasks_awaited_outreach_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_awaited_outreach_fk FOREIGN KEY (company_id, id, awaited_outreach_id) REFERENCES public.task_outreaches(company_id, task_id, id) ON DELETE SET NULL (awaited_outreach_id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: background_tasks background_tasks_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: background_tasks background_tasks_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: background_tasks background_tasks_owner_principal_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_owner_principal_fk FOREIGN KEY (company_id, owner_principal_id, owner_principal_kind) REFERENCES public.principals(company_id, id, kind) ON DELETE RESTRICT;


--
-- Name: background_tasks background_tasks_source_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_source_message_fk FOREIGN KEY (company_id, source_message_uuid) REFERENCES public.messages(company_id, id) ON DELETE SET NULL (source_message_uuid);


--
-- Name: background_tasks background_tasks_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE SET NULL (thread_id);


--
-- Name: background_tasks background_tasks_transition_approval_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_transition_approval_fk FOREIGN KEY (transition_approval_id) REFERENCES public.human_approvals(id);


--
-- Name: background_tasks background_tasks_transition_outreach_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_transition_outreach_fk FOREIGN KEY (transition_outreach_id) REFERENCES public.task_outreaches(id);


--
-- Name: binding_audit_events binding_audit_events_binding_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.binding_audit_events
    ADD CONSTRAINT binding_audit_events_binding_fk FOREIGN KEY (company_id, binding_id) REFERENCES public.channel_bindings(company_id, id) ON DELETE CASCADE;


--
-- Name: channel_agents channel_agents_agent_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_agents
    ADD CONSTRAINT channel_agents_agent_fk FOREIGN KEY (agent_id) REFERENCES public.agents(id) ON DELETE CASCADE;


--
-- Name: channel_agents channel_agents_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_agents
    ADD CONSTRAINT channel_agents_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: channel_bindings channel_bindings_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_bindings
    ADD CONSTRAINT channel_bindings_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: channel_bindings channel_bindings_installation_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_bindings
    ADD CONSTRAINT channel_bindings_installation_fk FOREIGN KEY (company_id, installation_id, transport) REFERENCES public.integration_installations(company_id, id, transport) ON DELETE CASCADE;


--
-- Name: channel_principal_grants channel_principal_grants_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_principal_grants
    ADD CONSTRAINT channel_principal_grants_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: channel_principal_grants channel_principal_grants_principal_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_principal_grants
    ADD CONSTRAINT channel_principal_grants_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id) ON DELETE CASCADE;


--
-- Name: channel_schedules channel_schedules_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_schedules
    ADD CONSTRAINT channel_schedules_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: channel_schedules channel_schedules_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_schedules
    ADD CONSTRAINT channel_schedules_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: channel_schedules channel_schedules_run_as_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_schedules
    ADD CONSTRAINT channel_schedules_run_as_user_id_fkey FOREIGN KEY (run_as_user_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: channel_slugs channel_slugs_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_slugs
    ADD CONSTRAINT channel_slugs_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: channels channels_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channels
    ADD CONSTRAINT channels_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: channels channels_owner_agent_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channels
    ADD CONSTRAINT channels_owner_agent_fk FOREIGN KEY (company_id, owner_agent_id) REFERENCES public.agents(company_id, id) ON DELETE CASCADE;


--
-- Name: channels channels_preferred_reviewer_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channels
    ADD CONSTRAINT channels_preferred_reviewer_fk FOREIGN KEY (company_id, preferred_reviewer_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (preferred_reviewer_principal_id);


--
-- Name: companies companies_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.companies
    ADD CONSTRAINT companies_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE RESTRICT;


--
-- Name: company_invites company_invites_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_invites
    ADD CONSTRAINT company_invites_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: company_mcp_connections company_mcp_connections_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_mcp_connections
    ADD CONSTRAINT company_mcp_connections_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: company_mcp_credentials company_mcp_credentials_company_id_connection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_mcp_credentials
    ADD CONSTRAINT company_mcp_credentials_company_id_connection_id_fkey FOREIGN KEY (company_id, connection_id) REFERENCES public.company_mcp_connections(company_id, id) ON DELETE CASCADE;


--
-- Name: company_mcp_tool_grants company_mcp_tool_grants_company_id_connection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_mcp_tool_grants
    ADD CONSTRAINT company_mcp_tool_grants_company_id_connection_id_fkey FOREIGN KEY (company_id, connection_id) REFERENCES public.company_mcp_connections(company_id, id) ON DELETE CASCADE;


--
-- Name: company_members company_members_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_members
    ADD CONSTRAINT company_members_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: company_members company_members_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_members
    ADD CONSTRAINT company_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: company_model_connections company_model_connections_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_model_connections
    ADD CONSTRAINT company_model_connections_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: company_resend_api_integrations company_resend_api_integrations_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.company_resend_api_integrations
    ADD CONSTRAINT company_resend_api_integrations_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: delegation_control_commands delegation_control_commands_actor_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.delegation_control_commands
    ADD CONSTRAINT delegation_control_commands_actor_fk FOREIGN KEY (company_id, actor_principal_id, actor_kind) REFERENCES public.principals(company_id, id, kind) ON DELETE RESTRICT;


--
-- Name: delegation_control_commands delegation_control_commands_outreach_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.delegation_control_commands
    ADD CONSTRAINT delegation_control_commands_outreach_fk FOREIGN KEY (company_id, outreach_id) REFERENCES public.task_outreaches(company_id, id) ON DELETE CASCADE;


--
-- Name: delegation_control_commands delegation_control_commands_target_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.delegation_control_commands
    ADD CONSTRAINT delegation_control_commands_target_fk FOREIGN KEY (company_id, outreach_id, target_id) REFERENCES public.task_outreach_targets(company_id, outreach_id, id) ON DELETE RESTRICT;


--
-- Name: delegation_control_commands delegation_control_commands_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.delegation_control_commands
    ADD CONSTRAINT delegation_control_commands_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: email_message_metadata email_message_metadata_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.email_message_metadata
    ADD CONSTRAINT email_message_metadata_message_fk FOREIGN KEY (company_id, message_id) REFERENCES public.messages(company_id, id) ON DELETE CASCADE;


--
-- Name: external_messages external_messages_binding_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_messages
    ADD CONSTRAINT external_messages_binding_fk FOREIGN KEY (company_id, binding_id) REFERENCES public.channel_bindings(company_id, id) ON DELETE CASCADE;


--
-- Name: external_messages external_messages_delivery_part_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_messages
    ADD CONSTRAINT external_messages_delivery_part_fk FOREIGN KEY (company_id, delivery_part_id) REFERENCES public.message_delivery_parts(company_id, id) ON DELETE SET NULL (delivery_part_id);


--
-- Name: external_messages external_messages_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_messages
    ADD CONSTRAINT external_messages_message_fk FOREIGN KEY (company_id, message_id) REFERENCES public.messages(company_id, id) ON DELETE CASCADE;


--
-- Name: external_threads external_threads_binding_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_threads
    ADD CONSTRAINT external_threads_binding_fk FOREIGN KEY (company_id, binding_id) REFERENCES public.channel_bindings(company_id, id) ON DELETE CASCADE;


--
-- Name: external_threads external_threads_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_threads
    ADD CONSTRAINT external_threads_thread_fk FOREIGN KEY (company_id, thread_id) REFERENCES public.threads(company_id, id) ON DELETE CASCADE;


--
-- Name: human_approvals human_approvals_approver_principal_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_approver_principal_fk FOREIGN KEY (company_id, approver_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (approver_principal_id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: human_approvals human_approvals_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: human_approvals human_approvals_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: human_approvals human_approvals_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_approvals
    ADD CONSTRAINT human_approvals_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


--
-- Name: human_task_completions human_task_completions_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_task_completions
    ADD CONSTRAINT human_task_completions_message_fk FOREIGN KEY (company_id, message_id) REFERENCES public.messages(company_id, id) ON DELETE RESTRICT;


--
-- Name: human_task_completions human_task_completions_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.human_task_completions
    ADD CONSTRAINT human_task_completions_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: inbound_events inbound_events_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inbound_events
    ADD CONSTRAINT inbound_events_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: inbound_events inbound_events_installation_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.inbound_events
    ADD CONSTRAINT inbound_events_installation_fk FOREIGN KEY (company_id, installation_id, transport) REFERENCES public.integration_installations(company_id, id, transport) ON DELETE CASCADE;


--
-- Name: integration_credentials integration_credentials_installation_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.integration_credentials
    ADD CONSTRAINT integration_credentials_installation_fk FOREIGN KEY (company_id, installation_id) REFERENCES public.integration_installations(company_id, id) ON DELETE CASCADE;


--
-- Name: integration_installations integration_installations_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.integration_installations
    ADD CONSTRAINT integration_installations_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: internal_note_tombstones internal_note_tombstones_actor_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_note_tombstones
    ADD CONSTRAINT internal_note_tombstones_actor_fk FOREIGN KEY (company_id, actor_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: internal_note_tombstones internal_note_tombstones_note_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_note_tombstones
    ADD CONSTRAINT internal_note_tombstones_note_fk FOREIGN KEY (company_id, note_id) REFERENCES public.internal_notes(company_id, id) ON DELETE CASCADE;


--
-- Name: internal_notes internal_notes_author_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_author_fk FOREIGN KEY (company_id, author_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: internal_notes internal_notes_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_message_fk FOREIGN KEY (company_id, message_id, message_audience) REFERENCES public.messages(company_id, id, audience) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: internal_notes internal_notes_supersedes_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_supersedes_fk FOREIGN KEY (company_id, supersedes_note_id) REFERENCES public.internal_notes(company_id, id) ON DELETE CASCADE;


--
-- Name: internal_notes internal_notes_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.internal_notes
    ADD CONSTRAINT internal_notes_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


--
-- Name: manual_handoffs manual_handoffs_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.manual_handoffs
    ADD CONSTRAINT manual_handoffs_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: manual_handoffs manual_handoffs_creator_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.manual_handoffs
    ADD CONSTRAINT manual_handoffs_creator_fk FOREIGN KEY (company_id, created_by_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (created_by_principal_id);


--
-- Name: manual_handoffs manual_handoffs_resolver_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.manual_handoffs
    ADD CONSTRAINT manual_handoffs_resolver_fk FOREIGN KEY (company_id, resolved_by_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (resolved_by_principal_id);


--
-- Name: manual_handoffs manual_handoffs_responsible_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.manual_handoffs
    ADD CONSTRAINT manual_handoffs_responsible_fk FOREIGN KEY (company_id, responsible_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (responsible_principal_id);


--
-- Name: manual_handoffs manual_handoffs_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.manual_handoffs
    ADD CONSTRAINT manual_handoffs_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


--
-- Name: memory_cleanup_jobs memory_cleanup_jobs_lifecycle_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_cleanup_jobs
    ADD CONSTRAINT memory_cleanup_jobs_lifecycle_fkey FOREIGN KEY (provider, remote_database_id) REFERENCES public.memory_remote_resource_lifecycles(provider, remote_database_id);


--
-- Name: memory_provider_connections memory_provider_connections_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_provider_connections
    ADD CONSTRAINT memory_provider_connections_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: memory_provisioning_jobs memory_provisioning_jobs_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_provisioning_jobs
    ADD CONSTRAINT memory_provisioning_jobs_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: memory_provisioning_jobs memory_provisioning_jobs_lifecycle_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_provisioning_jobs
    ADD CONSTRAINT memory_provisioning_jobs_lifecycle_fkey FOREIGN KEY (provider, remote_database_id) REFERENCES public.memory_remote_resource_lifecycles(provider, remote_database_id);


--
-- Name: memory_remote_resource_lifecycles memory_remote_resource_lifecycles_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.memory_remote_resource_lifecycles
    ADD CONSTRAINT memory_remote_resource_lifecycles_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE SET NULL;


--
-- Name: message_deliveries message_deliveries_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: message_deliveries message_deliveries_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: message_deliveries message_deliveries_dependency_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_dependency_fk FOREIGN KEY (company_id, depends_on_delivery_id) REFERENCES public.message_deliveries(company_id, id) ON DELETE SET NULL (depends_on_delivery_id);


--
-- Name: message_deliveries message_deliveries_destination_binding_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_destination_binding_fk FOREIGN KEY (company_id, destination_binding_id, transport) REFERENCES public.channel_bindings(company_id, id, transport) ON DELETE CASCADE;


--
-- Name: message_deliveries message_deliveries_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_message_fk FOREIGN KEY (company_id, message_id, message_audience) REFERENCES public.messages(company_id, id, audience) ON DELETE CASCADE;


--
-- Name: message_deliveries message_deliveries_source_binding_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_source_binding_fk FOREIGN KEY (company_id, source_binding_id) REFERENCES public.channel_bindings(company_id, id) ON DELETE CASCADE;


--
-- Name: message_deliveries message_deliveries_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE SET NULL (task_id);


--
-- Name: message_delivery_parts message_delivery_parts_delivery_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_delivery_parts
    ADD CONSTRAINT message_delivery_parts_delivery_fk FOREIGN KEY (company_id, delivery_id) REFERENCES public.message_deliveries(company_id, id) ON DELETE CASCADE;


--
-- Name: message_delivery_parts message_delivery_parts_delivery_id_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_delivery_parts
    ADD CONSTRAINT message_delivery_parts_delivery_id_fk FOREIGN KEY (delivery_id) REFERENCES public.message_deliveries(id) ON DELETE CASCADE;


--
-- Name: message_participants message_participants_identity_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_participants
    ADD CONSTRAINT message_participants_identity_fk FOREIGN KEY (company_id, participant_identity_id) REFERENCES public.participant_identities(company_id, id) ON DELETE CASCADE;


--
-- Name: message_participants message_participants_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_participants
    ADD CONSTRAINT message_participants_message_fk FOREIGN KEY (company_id, message_id) REFERENCES public.messages(company_id, id) ON DELETE CASCADE;


--
-- Name: messages messages_author_principal_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_author_principal_fk FOREIGN KEY (company_id, author_principal_id) REFERENCES public.principals(company_id, id) ON DELETE CASCADE;


--
-- Name: messages messages_authored_identity_author_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_authored_identity_author_fk FOREIGN KEY (company_id, author_principal_id, authored_identity_id) REFERENCES public.participant_identities(company_id, principal_id, id) ON DELETE SET NULL (authored_identity_id);


--
-- Name: messages messages_authored_identity_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_authored_identity_fk FOREIGN KEY (company_id, authored_identity_id) REFERENCES public.participant_identities(company_id, id) ON DELETE SET NULL (authored_identity_id);


--
-- Name: messages messages_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.messages
    ADD CONSTRAINT messages_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: notification_events notification_events_actor_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notification_events
    ADD CONSTRAINT notification_events_actor_fk FOREIGN KEY (company_id, actor_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (actor_principal_id);


--
-- Name: notification_events notification_events_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notification_events
    ADD CONSTRAINT notification_events_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: notifications notifications_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: notifications notifications_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: notifications notifications_email_delivery_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_email_delivery_id_fkey FOREIGN KEY (email_delivery_id) REFERENCES public.message_deliveries(id) ON DELETE SET NULL;


--
-- Name: notifications notifications_event_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_event_id_fkey FOREIGN KEY (event_id) REFERENCES public.notification_events(id) ON DELETE CASCADE;


--
-- Name: notifications notifications_event_identity_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_event_identity_fk FOREIGN KEY (company_id, source_kind, source_id, action_kind, source_generation) REFERENCES public.notification_events(company_id, source_kind, source_id, action_kind, source_generation) ON DELETE CASCADE;


--
-- Name: notifications notifications_recipient_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_recipient_fk FOREIGN KEY (company_id, recipient_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (recipient_principal_id);


--
-- Name: notifications notifications_recipient_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notifications
    ADD CONSTRAINT notifications_recipient_user_id_fkey FOREIGN KEY (recipient_user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: participant_identities participant_identities_principal_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.participant_identities
    ADD CONSTRAINT participant_identities_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id) ON DELETE CASCADE;


--
-- Name: pending_account_changes pending_account_changes_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pending_account_changes
    ADD CONSTRAINT pending_account_changes_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: principals principals_company_agent_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.principals
    ADD CONSTRAINT principals_company_agent_fk FOREIGN KEY (company_id, agent_id) REFERENCES public.agents(company_id, id) ON DELETE CASCADE;


--
-- Name: principals principals_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.principals
    ADD CONSTRAINT principals_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: principals principals_company_user_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.principals
    ADD CONSTRAINT principals_company_user_fk FOREIGN KEY (company_id, user_id) REFERENCES public.company_members(company_id, user_id) ON DELETE CASCADE;


--
-- Name: response_draft_evidence response_draft_evidence_draft_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_draft_evidence
    ADD CONSTRAINT response_draft_evidence_draft_fk FOREIGN KEY (company_id, draft_id, draft_version) REFERENCES public.response_drafts(company_id, id, version) ON DELETE CASCADE;


--
-- Name: response_draft_publications response_draft_publications_delivery_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_draft_publications
    ADD CONSTRAINT response_draft_publications_delivery_fk FOREIGN KEY (company_id, delivery_id) REFERENCES public.message_deliveries(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_draft_publications response_draft_publications_draft_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_draft_publications
    ADD CONSTRAINT response_draft_publications_draft_fk FOREIGN KEY (company_id, draft_id, draft_version) REFERENCES public.response_drafts(company_id, id, version) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_draft_publications response_draft_publications_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_draft_publications
    ADD CONSTRAINT response_draft_publications_message_fk FOREIGN KEY (company_id, message_id, message_audience) REFERENCES public.messages(company_id, id, audience) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_draft_publications response_draft_publications_publisher_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_draft_publications
    ADD CONSTRAINT response_draft_publications_publisher_fk FOREIGN KEY (company_id, published_by_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_drafts response_drafts_author_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_author_fk FOREIGN KEY (company_id, author_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_drafts response_drafts_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: response_drafts response_drafts_created_by_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_created_by_fk FOREIGN KEY (company_id, created_by_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_drafts response_drafts_reviewer_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_reviewer_fk FOREIGN KEY (company_id, reviewer_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_drafts response_drafts_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE SET NULL (task_id);


--
-- Name: response_drafts response_drafts_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


--
-- Name: response_drafts response_drafts_updated_by_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_drafts
    ADD CONSTRAINT response_drafts_updated_by_fk FOREIGN KEY (company_id, updated_by_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_review_commands response_review_commands_delivery_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_review_commands
    ADD CONSTRAINT response_review_commands_delivery_fk FOREIGN KEY (company_id, published_delivery_id) REFERENCES public.message_deliveries(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_review_commands response_review_commands_draft_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_review_commands
    ADD CONSTRAINT response_review_commands_draft_fk FOREIGN KEY (company_id, draft_id, expected_draft_version) REFERENCES public.response_drafts(company_id, id, version) ON DELETE CASCADE;


--
-- Name: response_reviews response_reviews_decider_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_reviews
    ADD CONSTRAINT response_reviews_decider_fk FOREIGN KEY (company_id, decided_by_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: response_reviews response_reviews_draft_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_reviews
    ADD CONSTRAINT response_reviews_draft_fk FOREIGN KEY (company_id, draft_id, draft_version) REFERENCES public.response_drafts(company_id, id, version) ON DELETE CASCADE;


--
-- Name: response_reviews response_reviews_notification_actor_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_reviews
    ADD CONSTRAINT response_reviews_notification_actor_fk FOREIGN KEY (company_id, notification_actor_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (notification_actor_principal_id);


--
-- Name: response_reviews response_reviews_reviewer_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.response_reviews
    ADD CONSTRAINT response_reviews_reviewer_fk FOREIGN KEY (company_id, reviewer_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: schedule_runs schedule_runs_schedule_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_schedule_id_fkey FOREIGN KEY (schedule_id) REFERENCES public.channel_schedules(id) ON DELETE CASCADE;


--
-- Name: schedule_runs schedule_runs_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.background_tasks(id) ON DELETE SET NULL;


--
-- Name: schedule_runs schedule_runs_thread_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_thread_id_fkey FOREIGN KEY (thread_id) REFERENCES public.threads(id) ON DELETE SET NULL;


--
-- Name: skills skills_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.skills
    ADD CONSTRAINT skills_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: start_agent_task_commands start_agent_task_commands_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.start_agent_task_commands
    ADD CONSTRAINT start_agent_task_commands_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: task_agent_instruction_notes task_agent_instruction_notes_instruction_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instruction_notes
    ADD CONSTRAINT task_agent_instruction_notes_instruction_id_fkey FOREIGN KEY (instruction_id) REFERENCES public.task_agent_instructions(id) ON DELETE CASCADE;


--
-- Name: task_agent_instruction_notes task_agent_instruction_notes_note_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instruction_notes
    ADD CONSTRAINT task_agent_instruction_notes_note_fk FOREIGN KEY (company_id, note_id) REFERENCES public.internal_notes(company_id, id) ON DELETE CASCADE;


--
-- Name: task_agent_instructions task_agent_instructions_actor_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instructions
    ADD CONSTRAINT task_agent_instructions_actor_fk FOREIGN KEY (company_id, requested_by_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: task_agent_instructions task_agent_instructions_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instructions
    ADD CONSTRAINT task_agent_instructions_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: task_agent_instructions task_agent_instructions_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_agent_instructions
    ADD CONSTRAINT task_agent_instructions_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


--
-- Name: task_approval_waits task_approval_waits_approval_task; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_approval_waits
    ADD CONSTRAINT task_approval_waits_approval_task FOREIGN KEY (company_id, task_id, approval_id) REFERENCES public.human_approvals(company_id, task_id, id) ON DELETE CASCADE;


--
-- Name: task_approval_waits task_approval_waits_company_id_approval_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_approval_waits
    ADD CONSTRAINT task_approval_waits_company_id_approval_id_fkey FOREIGN KEY (company_id, approval_id) REFERENCES public.human_approvals(company_id, id) ON DELETE CASCADE;


--
-- Name: task_approval_waits task_approval_waits_company_id_owner_principal_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_approval_waits
    ADD CONSTRAINT task_approval_waits_company_id_owner_principal_id_fkey FOREIGN KEY (company_id, owner_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: task_approval_waits task_approval_waits_company_id_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_approval_waits
    ADD CONSTRAINT task_approval_waits_company_id_task_id_fkey FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: task_approval_waits task_approval_waits_company_id_task_id_run_id_invocation_i_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_approval_waits
    ADD CONSTRAINT task_approval_waits_company_id_task_id_run_id_invocation_i_fkey FOREIGN KEY (company_id, task_id, run_id, invocation_id) REFERENCES public.task_harness_invocations(company_id, task_id, run_id, id);


--
-- Name: task_attempts task_attempts_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_attempts
    ADD CONSTRAINT task_attempts_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.background_tasks(id) ON DELETE CASCADE;


--
-- Name: task_channel_targets task_channel_targets_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_channel_targets
    ADD CONSTRAINT task_channel_targets_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: task_channel_targets task_channel_targets_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_channel_targets
    ADD CONSTRAINT task_channel_targets_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


--
-- Name: task_harness_invocations task_harness_invocations_company_id_task_id_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_invocations
    ADD CONSTRAINT task_harness_invocations_company_id_task_id_run_id_fkey FOREIGN KEY (company_id, task_id, run_id) REFERENCES public.task_harness_runs(company_id, task_id, id) ON DELETE CASCADE;


--
-- Name: task_harness_runs task_harness_runs_company_id_agent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_runs
    ADD CONSTRAINT task_harness_runs_company_id_agent_id_fkey FOREIGN KEY (company_id, agent_id) REFERENCES public.agents(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: task_harness_runs task_harness_runs_company_id_owner_principal_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_runs
    ADD CONSTRAINT task_harness_runs_company_id_owner_principal_id_fkey FOREIGN KEY (company_id, owner_principal_id) REFERENCES public.principals(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: task_harness_runs task_harness_runs_company_id_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_runs
    ADD CONSTRAINT task_harness_runs_company_id_task_id_fkey FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: task_outreach_replies task_outreach_replies_outreach_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_replies
    ADD CONSTRAINT task_outreach_replies_outreach_fk FOREIGN KEY (company_id, outreach_id) REFERENCES public.task_outreaches(company_id, id) ON DELETE CASCADE;


--
-- Name: task_outreach_replies task_outreach_replies_response_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_replies
    ADD CONSTRAINT task_outreach_replies_response_fk FOREIGN KEY (company_id, response_association_id) REFERENCES public.thread_messages(company_id, id) ON DELETE CASCADE;


--
-- Name: task_outreach_replies task_outreach_replies_target_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_replies
    ADD CONSTRAINT task_outreach_replies_target_fk FOREIGN KEY (company_id, outreach_id, target_id) REFERENCES public.task_outreach_targets(company_id, outreach_id, id) ON DELETE CASCADE;


--
-- Name: task_outreach_targets task_outreach_targets_delivery_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_delivery_fk FOREIGN KEY (company_id, delivery_id) REFERENCES public.message_deliveries(company_id, id) ON DELETE SET NULL (delivery_id);


--
-- Name: task_outreach_targets task_outreach_targets_internal_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_internal_channel_fk FOREIGN KEY (company_id, internal_channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: task_outreach_targets task_outreach_targets_outreach_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_outreach_fk FOREIGN KEY (company_id, outreach_id) REFERENCES public.task_outreaches(company_id, id) ON DELETE CASCADE;


--
-- Name: task_outreach_targets task_outreach_targets_replacement_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_replacement_fk FOREIGN KEY (company_id, outreach_id, replaces_target_id) REFERENCES public.task_outreach_targets(company_id, outreach_id, id) ON DELETE RESTRICT;


--
-- Name: task_outreach_targets task_outreach_targets_request_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_request_message_fk FOREIGN KEY (company_id, request_message_id) REFERENCES public.messages(company_id, id) ON DELETE SET NULL (request_message_id);


--
-- Name: task_outreach_targets task_outreach_targets_response_association_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreach_targets
    ADD CONSTRAINT task_outreach_targets_response_association_fk FOREIGN KEY (company_id, response_association_id) REFERENCES public.thread_messages(company_id, id) ON DELETE SET NULL (response_association_id);


--
-- Name: task_outreaches task_outreaches_creator_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreaches
    ADD CONSTRAINT task_outreaches_creator_fk FOREIGN KEY (company_id, created_by_principal_id, created_by_principal_kind) REFERENCES public.principals(company_id, id, kind) ON DELETE RESTRICT;


--
-- Name: task_outreaches task_outreaches_harness_invocation_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreaches
    ADD CONSTRAINT task_outreaches_harness_invocation_fk FOREIGN KEY (company_id, task_id, harness_run_id, harness_invocation_id) REFERENCES public.task_harness_invocations(company_id, task_id, run_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: task_outreaches task_outreaches_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_outreaches
    ADD CONSTRAINT task_outreaches_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: task_ownership_events task_ownership_events_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_ownership_events
    ADD CONSTRAINT task_ownership_events_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: task_status_events task_status_events_related_approval_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_status_events
    ADD CONSTRAINT task_status_events_related_approval_id_fkey FOREIGN KEY (related_approval_id) REFERENCES public.human_approvals(id) ON DELETE SET NULL;


--
-- Name: task_status_events task_status_events_related_outreach_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_status_events
    ADD CONSTRAINT task_status_events_related_outreach_id_fkey FOREIGN KEY (related_outreach_id) REFERENCES public.task_outreaches(id) ON DELETE SET NULL;


--
-- Name: task_status_events task_status_events_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_status_events
    ADD CONSTRAINT task_status_events_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: thread_messages thread_messages_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_messages
    ADD CONSTRAINT thread_messages_message_fk FOREIGN KEY (company_id, message_id) REFERENCES public.messages(company_id, id) ON DELETE CASCADE;


--
-- Name: thread_messages thread_messages_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_messages
    ADD CONSTRAINT thread_messages_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


--
-- Name: thread_principals thread_principals_principal_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_principals
    ADD CONSTRAINT thread_principals_principal_fk FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id) ON DELETE CASCADE;


--
-- Name: thread_principals thread_principals_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_principals
    ADD CONSTRAINT thread_principals_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


--
-- Name: threads threads_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.threads
    ADD CONSTRAINT threads_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: user_login_methods user_login_methods_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_login_methods
    ADD CONSTRAINT user_login_methods_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: user_notification_preferences user_notification_preferences_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_notification_preferences
    ADD CONSTRAINT user_notification_preferences_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--
