--
-- PostgreSQL database dump
--

\restrict qFwGhfq2cOwzNkKiMUdK0K54svFLzmVHJd6sT7yjkUx8MzDwPwJCpMshLQof2hx

-- Dumped from database version 18.6 (Homebrew)
-- Dumped by pg_dump version 18.6 (Homebrew)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
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
-- Name: advance_workflow_owned_revision(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.advance_workflow_owned_revision() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE owner_company uuid; owner_run uuid;
BEGIN
    IF TG_OP='UPDATE' AND (to_jsonb(NEW)-ARRAY['updated_at','locked_at','lock_expires_at'])
        IS NOT DISTINCT FROM (to_jsonb(OLD)-ARRAY['updated_at','locked_at','lock_expires_at']) THEN
        RETURN NULL;
    END IF;
    IF TG_TABLE_NAME='background_tasks' THEN
        IF NEW.queue_kind<>'workflow' THEN RETURN NULL; END IF;
        SELECT company_id,run_id INTO owner_company,owner_run FROM workflow_executions
            WHERE company_id=NEW.company_id AND id=NEW.workflow_execution_id;
    ELSE
        owner_company=NEW.company_id; owner_run=NEW.run_id;
    END IF;
    UPDATE workflow_runs SET revision=revision+1 WHERE company_id=owner_company AND id=owner_run;
    RETURN NULL;
END $$;


--
-- Name: advance_workflow_run_revision(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.advance_workflow_run_revision() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.revision<>OLD.revision AND pg_trigger_depth()=1 THEN
        RAISE EXCEPTION 'workflow revision is database generated' USING ERRCODE='23514';
    END IF;
    IF (to_jsonb(NEW)-'revision') IS DISTINCT FROM (to_jsonb(OLD)-'revision')
        OR NEW.revision<>OLD.revision THEN
        IF OLD.revision=9223372036854775807 THEN
            RAISE EXCEPTION 'workflow revision exhausted' USING ERRCODE='23514';
        END IF;
        NEW.revision=OLD.revision+1;
    END IF;
    RETURN NEW;
END $$;


--
-- Name: allocate_workflow_wakeup_sequence(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.allocate_workflow_wakeup_sequence() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.terminal_wakeup_sequence IS NOT NULL THEN
            RAISE EXCEPTION 'Workflow wakeup sequence is generated' USING ERRCODE='23514';
        END IF;
    ELSE
        IF NEW.terminal_wakeup_sequence IS DISTINCT FROM OLD.terminal_wakeup_sequence THEN
            RAISE EXCEPTION 'Workflow wakeup sequence is generated' USING ERRCODE='23514';
        END IF;
        IF NEW.parent_run_id IS NOT NULL AND NEW.state IS DISTINCT FROM OLD.state
           AND NEW.state IN ('succeeded','failed','cancelled') THEN
            SELECT COALESCE(MAX(event.sequence),0)+1 INTO NEW.terminal_wakeup_sequence
                FROM workflow_run_events AS event
                WHERE event.company_id=NEW.company_id AND event.run_id=NEW.id;
        END IF;
    END IF;
    RETURN NEW;
END $$;


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
-- Name: check_workflow_control_retry(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workflow_control_retry() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE owner workflow_runs%ROWTYPE; ordinary boolean; reconciled boolean;
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='failed' AND NEW.status='pending' THEN
        SELECT run.* INTO owner FROM workflow_runs AS run JOIN workflow_executions AS execution
            ON execution.company_id=run.company_id AND execution.run_id=run.id
            WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id;
        ordinary:=workflow_control_retry_safe(NEW.company_id,owner.id,NEW.id)
            AND EXISTS(SELECT 1 FROM workflow_control_commands AS receipt WHERE receipt.company_id=owner.company_id
                AND receipt.run_id=owner.id AND receipt.operation='retry' AND receipt.result='applied' AND receipt.result_revision=owner.revision);
        reconciled:=workflow_action_reconciliation_reopen_safe(NEW.company_id,owner.id,NEW.id)
            AND EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS receipt
                JOIN workflow_action_evidence AS evidence ON evidence.company_id=receipt.company_id AND evidence.id=receipt.evidence_id
                JOIN workflow_run_events AS audit ON audit.company_id=receipt.company_id AND audit.run_id=receipt.run_id AND audit.sequence=receipt.audit_sequence
                WHERE receipt.company_id=owner.company_id AND receipt.run_id=owner.id AND receipt.execution_id=NEW.workflow_execution_id
                    AND receipt.scheduled_job_id=NEW.id AND receipt.outcome->>'kind'='scheduled'
                    AND receipt.previous_state='waiting' AND receipt.previous_waiting_reason='reconciliation'
                    AND evidence.run_id=receipt.run_id AND evidence.execution_id=receipt.execution_id
                    AND evidence.invocation_id=receipt.invocation_id AND evidence.argument_digest=receipt.argument_digest
                    AND evidence.dispatch_id=receipt.dispatch_id
                    AND EXISTS(SELECT 1 FROM workflow_action_state_witnesses AS witness
                        WHERE witness.company_id=owner.company_id AND witness.run_id=owner.id
                            AND witness.transaction_id=pg_current_xact_id()
                            AND witness.initial_state='waiting' AND witness.initial_waiting_reason='reconciliation')
                    AND receipt.result_revision=owner.revision AND audit.execution_id=receipt.execution_id
                    AND audit.actor_id=receipt.actor_id AND audit.event_kind='action_reconciled');
        IF NEW.retry_count<>OLD.retry_count OR NEW.max_retries<>OLD.max_retries
            OR NEW.workflow_execution_id<>OLD.workflow_execution_id OR NEW.id<>OLD.id
            OR owner.state<>'running' OR owner.terminal_execution_id IS NOT NULL
            OR NOT (ordinary OR reconciled)
        THEN RAISE EXCEPTION 'invalid explicit workflow retry' USING ERRCODE='23514'; END IF;
    END IF;
    RETURN NULL;
END $$;


--
-- Name: check_workflow_fenced_completion(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workflow_fenced_completion() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: check_workflow_fenced_retirement(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workflow_fenced_retirement() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE retirement task_attempts%ROWTYPE;
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='processing' AND NEW.status IN ('pending','failed') THEN
        SELECT * INTO retirement FROM task_attempts WHERE task_id=OLD.id
            AND attempt_number=OLD.retry_count+1 AND execution_generation=OLD.execution_generation
            AND worker_id=OLD.worker_id AND status='failed';
        IF NOT FOUND OR retirement.workflow_retirement IS NULL OR NEW.retry_count<>OLD.retry_count+1
            OR (retirement.workflow_retirement='live' AND OLD.lock_expires_at<=clock_timestamp())
            OR (retirement.workflow_retirement='expired' AND OLD.lock_expires_at>clock_timestamp())
            OR (NEW.status='pending' AND (retirement.workflow_retry_safety<>'safe'
                OR retirement.workflow_failure_class<>'retryable' OR NEW.retry_count>=NEW.max_retries
                OR NEW.run_at<=retirement.finished_at)) THEN
            RAISE EXCEPTION 'workflow failure lost ownership or retry eligibility' USING ERRCODE='23514';
        END IF;
        IF retirement.workflow_retry_safety='safe' AND
            workflow_action_retry_safe(NEW.company_id,NEW.workflow_execution_id) IS FALSE THEN
            RAISE EXCEPTION 'workflow action outcome unknown at retirement' USING ERRCODE='23514';
        END IF;
        IF retirement.workflow_retirement='deadline' AND (
            NEW.status<>'failed' OR retirement.workflow_failure_class<>'terminal'
            OR retirement.workflow_failure_code<>'workflow.run_deadline'
            OR retirement.workflow_retry_safety<>'unknown'
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.deadline<=clock_timestamp() AND run.state='failed'
                    AND execution.successor_execution_id IS NULL)) THEN
            RAISE EXCEPTION 'invalid workflow deadline retirement' USING ERRCODE='23514';
        END IF;
        IF retirement.workflow_retirement='cancel' AND (
            NEW.status<>'failed' OR retirement.workflow_failure_class<>'terminal'
            OR retirement.workflow_failure_code<>'workflow.cancelled'
            OR retirement.workflow_retry_safety<>'unknown'
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.state='cancelled' AND execution.completed_at IS NULL
                    AND execution.successor_execution_id IS NULL)) THEN
            RAISE EXCEPTION 'invalid workflow cancellation retirement' USING ERRCODE='23514';
        END IF;
        -- A retirement may fail or suspend an expired run, but cannot schedule
        -- a retry or successor after its original deadline, including commit delay.
        IF (NEW.status='pending' OR EXISTS (
                SELECT 1 FROM workflow_executions WHERE company_id=NEW.company_id
                    AND id=NEW.workflow_execution_id AND successor_execution_id IS NOT NULL))
            AND EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND run.deadline<=clock_timestamp()) THEN
            RAISE EXCEPTION 'workflow retirement scheduled work after deadline' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NULL;
END $$;


--
-- Name: check_workflow_parent_source(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workflow_parent_source() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE source jsonb; parent_run uuid; parent_execution uuid;
BEGIN
    IF TG_OP='UPDATE' AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow admission identity is immutable' USING ERRCODE='23514';
    END IF;
    source := substring(NEW.source_key FROM 4)::jsonb;
    SELECT run.parent_run_id,run.parent_execution_id INTO parent_run,parent_execution
      FROM workflow_runs AS run WHERE run.company_id=NEW.company_id AND run.id=NEW.run_id;
    IF source->>0='child' THEN
        IF jsonb_array_length(source)<>5 OR source->4<>'null'::jsonb
           OR parent_run IS DISTINCT FROM (source->>1)::uuid
           OR parent_execution IS DISTINCT FROM (source->>2)::uuid
           OR NOT EXISTS (SELECT 1 FROM workflow_executions AS execution
                WHERE execution.company_id=NEW.company_id AND execution.run_id=parent_run
                  AND execution.id=parent_execution AND execution.step_id=source->>3) THEN
            RAISE EXCEPTION 'Workflow admission parent mismatch' USING ERRCODE='23514';
        END IF;
    ELSIF parent_run IS NOT NULL THEN
        RAISE EXCEPTION 'Non-child admission has parent' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: check_workflow_progression_deadline(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workflow_progression_deadline() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.committed_route IS NOT NULL AND (TG_OP = 'INSERT' OR OLD.committed_route IS NULL)
        AND EXISTS (SELECT 1 FROM workflow_runs AS run
            WHERE run.company_id = NEW.company_id AND run.id = NEW.run_id
                AND run.deadline <= clock_timestamp()) THEN
        RAISE EXCEPTION 'workflow deadline reached before progression commit' USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END $$;


--
-- Name: check_workflow_wait_commit(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workflow_wait_commit() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.notification_intent IS NOT NULL AND
       (TG_OP='INSERT' OR (OLD.state='waiting' AND NEW.state='completed')) THEN
        IF EXISTS (SELECT 1 FROM workflow_runs AS run WHERE run.company_id=NEW.company_id
            AND run.id=NEW.run_id AND run.deadline<=clock_timestamp()) OR
           (NEW.reason='event' AND NEW.deadline<=clock_timestamp()) THEN
            RAISE EXCEPTION 'workflow wait deadline reached before commit' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NULL;
END $$;


--
-- Name: company_mcp_endpoint_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.company_mcp_endpoint_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.company_id, NEW.id, NEW.endpoint_url, NEW.transport)
        IS DISTINCT FROM (OLD.company_id, OLD.id, OLD.endpoint_url, OLD.transport) THEN
        RAISE EXCEPTION 'MCP connection endpoint is immutable';
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
-- Name: debit_workflow_budget_receipt(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.debit_workflow_budget_receipt() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE usage workflow_root_budget_usage%ROWTYPE; budget workflow_root_budgets%ROWTYPE;
    receipt workflow_budget_receipts%ROWTYPE; consumed bigint; ceiling integer;
BEGIN
    PERFORM 1 FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    SELECT * INTO receipt FROM workflow_budget_receipts WHERE company_id=NEW.company_id
        AND run_id=NEW.run_id AND execution_id=NEW.execution_id AND resource=NEW.resource AND reservation_key=NEW.reservation_key;
    IF FOUND THEN
        IF (NEW.root_run_id,NEW.quantity) IS DISTINCT FROM (receipt.root_run_id,receipt.quantity) THEN
            RAISE EXCEPTION 'Workflow budget reservation payload conflicts' USING ERRCODE='23514';
        END IF;
        NEW.disposition=receipt.disposition;
        RETURN NEW;
    END IF;
    SELECT * INTO STRICT usage FROM workflow_root_budget_usage
        WHERE company_id=NEW.company_id AND root_run_id=NEW.root_run_id FOR UPDATE;
    SELECT * INTO STRICT budget FROM workflow_root_budgets
        WHERE company_id=NEW.company_id AND root_run_id=NEW.root_run_id;
    consumed=CASE NEW.resource WHEN 'activation' THEN usage.activations WHEN 'model_call' THEN usage.model_calls ELSE usage.repetitions END;
    ceiling=CASE NEW.resource WHEN 'activation' THEN budget.activations WHEN 'model_call' THEN budget.model_calls ELSE budget.repetitions END;
    NEW.disposition=CASE WHEN budget.provenance='frozen_v2' AND NEW.quantity>0
        AND consumed<=ceiling::bigint-NEW.quantity THEN 'granted' ELSE 'exhausted' END;
    IF NEW.disposition='granted' THEN
        UPDATE workflow_root_budget_usage SET
            activations=activations+CASE WHEN NEW.resource='activation' THEN NEW.quantity ELSE 0 END,
            model_calls=model_calls+CASE WHEN NEW.resource='model_call' THEN NEW.quantity ELSE 0 END,
            repetitions=repetitions+CASE WHEN NEW.resource='repetition' THEN NEW.quantity ELSE 0 END
            WHERE company_id=NEW.company_id AND root_run_id=NEW.root_run_id;
    END IF;
    RETURN NEW;
END $$;


--
-- Name: delete_channel_target_tasks(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.delete_channel_target_tasks() RETURNS trigger
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
-- Name: guard_workflow_capacity_policy(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.guard_workflow_capacity_policy() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    RAISE EXCEPTION 'workflow capacity policy is immutable' USING ERRCODE = '23514';
END;
$$;


--
-- Name: immutable_workflow_wait_event(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.immutable_workflow_wait_event() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'workflow event facts are immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


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
-- Name: insert_workflow_budget_link(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.insert_workflow_budget_link() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE root uuid; stored jsonb; limits jsonb;
BEGIN
    IF NEW.parent_run_id IS NOT NULL THEN
        SELECT root_run_id INTO STRICT root FROM workflow_run_budgets
            WHERE company_id=NEW.company_id AND run_id=NEW.parent_run_id;
    ELSE
        root=NEW.id;
        -- Invalid/old raw schema fixtures may remain as sealed records. They do
        -- not become executable or receive defaults through this SQL boundary.
        BEGIN
            stored=convert_from(NEW.bundle,'UTF8')::jsonb;
            IF stored->>'compiler_revision'='2' THEN
                SELECT version->'compiled'->'root_budget' INTO limits
                    FROM jsonb_array_elements(stored->'versions') AS version
                    WHERE version->>'version'=stored->>'root';
            END IF;
        EXCEPTION WHEN data_exception THEN
            limits=NULL;
        END;
        INSERT INTO workflow_root_budgets(company_id,root_run_id,provenance,activations,model_calls,repetitions)
            VALUES(NEW.company_id,root,CASE WHEN limits IS NULL THEN 'sealed_legacy' ELSE 'frozen_v2' END,
                (limits->>'activations')::integer,(limits->>'model_calls')::integer,(limits->>'repetitions')::integer);
        INSERT INTO workflow_root_budget_usage(company_id,root_run_id) VALUES(NEW.company_id,root);
    END IF;
    INSERT INTO workflow_run_budgets(company_id,run_id,root_run_id,parent_run_id)
        VALUES(NEW.company_id,NEW.id,root,NEW.parent_run_id);
    RETURN NEW;
END $$;


--
-- Name: insert_workflow_parent_link(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.insert_workflow_parent_link() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.parent_run_id IS NOT NULL THEN
        -- Owner FK is deferred until this run INSERT has finished. Both rows
        -- roll back together on any admission/commit failure.
        INSERT INTO workflow_run_parents(company_id,child_run_id,parent_run_id,parent_execution_id)
            VALUES(NEW.company_id,NEW.id,NEW.parent_run_id,NEW.parent_execution_id);
    END IF;
    RETURN NEW;
END $$;


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
               OR NEW.status IN ('dead_letter', 'outcome_unknown'))
          -- Work somebody deliberately cancelled is not presented as a delivery that failed. An
          -- ambiguous outcome still notifies: the provider may hold it, and a human must know.
          AND NOT (NEW.status = 'dead_letter' AND NEW.last_error_class = 'superseded'
                   AND NEW.cancellation_requested_at IS NOT NULL) THEN
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
    response_trigger text DEFAULT 'always'::text NOT NULL,
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
    external_reply_handling_override text,
    CONSTRAINT channels_access_mode_check CHECK ((access_mode = ANY (ARRAY['team'::text, 'allowlist'::text, 'public'::text]))),
    CONSTRAINT channels_created_by_shape_check CHECK (public.valid_creation_provenance(created_by)),
    CONSTRAINT channels_external_reply_handling_override_check CHECK (((external_reply_handling_override IS NULL) OR (external_reply_handling_override = ANY (ARRAY['automatic'::text, 'manual_handoff'::text])))),
    CONSTRAINT channels_external_response_review_override_check CHECK (((external_response_review_override IS NULL) OR (external_response_review_override = ANY (ARRAY['autonomous'::text, 'review_all_external'::text])))),
    CONSTRAINT channels_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT channels_response_trigger_check CHECK ((response_trigger = ANY (ARRAY['always'::text, 'mentioned'::text, 'mentioned_or_reply'::text])))
);


--
-- Name: COLUMN channels.external_reply_handling_override; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.channels.external_reply_handling_override IS 'NULL inherits the company policy live rather than meaning automatic: a company that switches its default moves every inheriting channel with it, so the effective value is resolved with COALESCE at read time and never copied into the channel row.';


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
-- Name: preserve_background_task_queue_identity(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_background_task_queue_identity() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.queue_kind, NEW.workflow_execution_id) IS DISTINCT FROM
       (OLD.queue_kind, OLD.workflow_execution_id) THEN
        RAISE EXCEPTION 'background task queue identity is immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: preserve_delivery_cancellation(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_delivery_cancellation() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.cancellation_requested_at IS NOT NULL
       AND (NEW.cancellation_requested_at IS DISTINCT FROM OLD.cancellation_requested_at
            OR NEW.cancellation_reason IS DISTINCT FROM OLD.cancellation_reason) THEN
        RAISE EXCEPTION 'a delivery cancellation intent cannot be cleared or replaced'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: preserve_workflow_budget_identity(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_budget_identity() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        IF EXISTS (SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id
            AND id=CASE WHEN TG_TABLE_NAME='workflow_run_budgets' THEN (to_jsonb(OLD)->>'run_id')::uuid ELSE OLD.root_run_id END) THEN
            RAISE EXCEPTION 'Workflow budget identity cannot be removed while its owner exists' USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow budget identity is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: preserve_workflow_budget_receipt(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_budget_receipt() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        IF EXISTS (SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id) THEN
            RAISE EXCEPTION 'Workflow budget receipt cannot be removed while its owner exists' USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow budget receipt is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: preserve_workflow_budget_usage(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_budget_usage() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.activations<>0 OR NEW.model_calls<>0 OR NEW.repetitions<>0 THEN
            RAISE EXCEPTION 'New workflow budget usage starts at zero' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP='DELETE' THEN
        IF EXISTS (SELECT 1 FROM workflow_root_budgets WHERE company_id=OLD.company_id AND root_run_id=OLD.root_run_id) THEN
            RAISE EXCEPTION 'Workflow budget usage cannot be removed while its owner exists' USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF (NEW.company_id,NEW.root_run_id) IS DISTINCT FROM (OLD.company_id,OLD.root_run_id)
        OR NEW.activations<OLD.activations OR NEW.model_calls<OLD.model_calls OR NEW.repetitions<OLD.repetitions
        OR (NEW IS DISTINCT FROM OLD AND pg_trigger_depth()<2) THEN
        RAISE EXCEPTION 'Workflow budget usage changes require a receipt' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: preserve_workflow_control_receipt(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_control_receipt() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        IF EXISTS (SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
            AND EXISTS (SELECT 1 FROM companies WHERE id=OLD.company_id) THEN
            RAISE EXCEPTION 'workflow control receipts are immutable' USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'workflow control receipts are immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: preserve_workflow_execution_facts(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_execution_facts() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.activated_at IS NOT NULL AND
       (NEW.activated_at, NEW.frozen_inputs) IS DISTINCT FROM
       (OLD.activated_at, OLD.frozen_inputs) THEN
        RAISE EXCEPTION 'workflow activation inputs are immutable'
            USING ERRCODE = '23514', CONSTRAINT = 'workflow_activation_immutable';
    END IF;
    IF OLD.completed_at IS NOT NULL AND
       (NEW.completed_at, NEW.committed_output) IS DISTINCT FROM
       (OLD.completed_at, OLD.committed_output) THEN
        RAISE EXCEPTION 'workflow committed output is immutable'
            USING ERRCODE = '23514', CONSTRAINT = 'workflow_output_immutable';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: preserve_workflow_execution_identity(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_execution_identity() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.company_id, NEW.id, NEW.run_id, NEW.step_id, NEW.activation) IS DISTINCT FROM
       (OLD.company_id, OLD.id, OLD.run_id, OLD.step_id, OLD.activation) THEN
        RAISE EXCEPTION 'workflow execution identity is immutable'
            USING ERRCODE = '23514', CONSTRAINT = 'workflow_execution_identity_immutable';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: preserve_workflow_parent(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_parent() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.parent_run_id,NEW.parent_execution_id) IS DISTINCT FROM
       (OLD.parent_run_id,OLD.parent_execution_id) THEN
        RAISE EXCEPTION 'Admitted workflow parent is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: preserve_workflow_parent_link(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_parent_link() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow parent link is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: preserve_workflow_progression(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_progression() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.activated_at IS NOT NULL AND NEW.frozen_choice IS DISTINCT FROM OLD.frozen_choice THEN
        RAISE EXCEPTION 'workflow rule choice is immutable' USING ERRCODE = '23514';
    END IF;
    IF OLD.committed_route IS NOT NULL AND
        (NEW.committed_route,NEW.route_target,NEW.successor_execution_id) IS DISTINCT FROM
        (OLD.committed_route,OLD.route_target,OLD.successor_execution_id) THEN
        RAISE EXCEPTION 'workflow route is immutable' USING ERRCODE = '23514';
    END IF;
    IF NEW.successor_execution_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM workflow_executions AS successor
        WHERE successor.company_id = NEW.company_id AND successor.run_id = NEW.run_id
            AND successor.id = NEW.successor_execution_id AND successor.step_id = NEW.route_target
            AND successor.activation = NEW.activation + 1) THEN
        RAISE EXCEPTION 'invalid workflow successor' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: preserve_workflow_run_association(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_run_association() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.company_id, NEW.id, NEW.channel_id, NEW.thread_id) IS DISTINCT FROM
       (OLD.company_id, OLD.id, OLD.channel_id, OLD.thread_id) THEN
        RAISE EXCEPTION 'workflow run association is immutable'
            USING ERRCODE = '23514', CONSTRAINT = 'workflow_run_association_immutable';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: preserve_workflow_wakeup(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_wakeup() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (OLD.event_kind='parent_wakeup' OR NEW.event_kind='parent_wakeup') AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow parent wakeup is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


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
            WHEN transition_reason IN ('operator_stopped', 'operator_resumed',
                                       'channel_agent_removed') THEN 'operator'
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


--
-- Name: require_immutable_schedule_channel(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.require_immutable_schedule_channel() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.channel_id IS DISTINCT FROM OLD.channel_id THEN
        RAISE EXCEPTION 'A schedule cannot change its channel'
            USING ERRCODE = '23514', CONSTRAINT = 'schedule_channel_immutable';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: require_legacy_auxiliary_task(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.require_legacy_auxiliary_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF NEW.task_id IS NOT DISTINCT FROM OLD.task_id
           AND NEW.company_id IS NOT DISTINCT FROM OLD.company_id THEN
            RETURN NEW;
        END IF;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM public.background_tasks AS task
        WHERE task.id = NEW.task_id AND task.company_id = NEW.company_id
          AND task.queue_kind = 'legacy'
    ) THEN
        RAISE EXCEPTION 'Legacy auxiliary association requires a visible scoped legacy task'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_auxiliary_task_queue_kind';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: require_legacy_handoff_event_task(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.require_legacy_handoff_event_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.task_id IS NULL THEN
        RETURN NEW;
    END IF;
    -- Unlike the live auxiliary owners, events have no FK to serialize deletion.
    -- Lock the visible scoped parent through commit, then retain only its identity.
    -- Queue identity is immutable; KEY SHARE also prevents a company-key change.
    PERFORM 1 FROM public.background_tasks AS task
    WHERE task.id = NEW.task_id AND task.company_id = NEW.company_id
      AND task.queue_kind = 'legacy'
    FOR KEY SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Handoff event requires a visible scoped legacy task'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_handoff_event_task_provenance';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: require_legacy_provision_task(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.require_legacy_provision_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE task_company uuid;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF (NEW.company_id, NEW.task_id, NEW.agent_id, NEW.channel_id)
            IS NOT DISTINCT FROM (OLD.company_id, OLD.task_id, OLD.agent_id, OLD.channel_id) THEN
            RETURN NEW;
        END IF;
    END IF;
    SELECT task.company_id INTO task_company FROM public.background_tasks AS task
    WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy';
    IF task_company IS NULL
       OR (NEW.company_id IS NOT NULL AND NEW.company_id <> task_company)
       OR NOT EXISTS (SELECT 1 FROM public.agents AS agent
                      WHERE agent.id = NEW.agent_id AND agent.company_id = task_company)
       OR NOT EXISTS (SELECT 1 FROM public.channels AS channel
                      WHERE channel.id = NEW.channel_id AND channel.company_id = task_company) THEN
        RAISE EXCEPTION 'Provision requires a visible legacy task and same-company owners'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_provision_task_scope';
    END IF;
    NEW.company_id := task_company;
    RETURN NEW;
END;
$$;


--
-- Name: require_legacy_schedule_task(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.require_legacy_schedule_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE owner_company uuid; owner_channel uuid;
BEGIN
    IF TG_OP = 'INSERT' THEN
        -- SHARE also conflicts with a non-key channel UPDATE: either we capture
        -- before the move or observe its committed scope, never a mixed snapshot.
        SELECT schedule.company_id, schedule.channel_id INTO owner_company, owner_channel
        FROM public.channel_schedules AS schedule WHERE schedule.id = NEW.schedule_id
        FOR SHARE;
        IF owner_company IS NULL
           OR (NEW.company_id IS NOT NULL AND NEW.company_id <> owner_company)
           OR (NEW.channel_id IS NOT NULL AND NEW.channel_id <> owner_channel) THEN
            RAISE EXCEPTION 'Schedule occurrence requires its actual owner scope at capture'
                USING ERRCODE = '23514', CONSTRAINT = 'legacy_schedule_owner_scope';
        END IF;
        NEW.company_id := owner_company;
        NEW.channel_id := owner_channel;
    ELSE
        IF (NEW.company_id, NEW.channel_id, NEW.schedule_id, NEW.schedule_snapshot)
            IS DISTINCT FROM
           (OLD.company_id, OLD.channel_id, OLD.schedule_id, OLD.schedule_snapshot) THEN
            RAISE EXCEPTION 'Captured schedule occurrence identity is immutable'
                USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_identity_immutable';
        END IF;
        IF (NEW.thread_id, NEW.task_id) IS NOT DISTINCT FROM (OLD.thread_id, OLD.task_id) THEN
            RETURN NEW;
        END IF;
    END IF;
    -- Incomplete snapshots already fail the application decoder. When an identity
    -- is present it must agree with the relational occurrence, including raw SQL.
    IF (NEW.schedule_snapshot ? 'id' AND NEW.schedule_snapshot->>'id' IS DISTINCT FROM NEW.schedule_id::text)
       OR (NEW.schedule_snapshot ? 'company_id' AND NEW.schedule_snapshot->>'company_id' IS DISTINCT FROM NEW.company_id::text)
       OR (NEW.schedule_snapshot ? 'channel_id' AND NEW.schedule_snapshot->>'channel_id' IS DISTINCT FROM NEW.channel_id::text) THEN
        RAISE EXCEPTION 'Schedule snapshot identity must match captured occurrence scope'
            USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_snapshot_scope';
    END IF;
    IF NEW.task_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.background_tasks AS task
        WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy'
          AND task.company_id = NEW.company_id AND task.channel_id = NEW.channel_id
          AND task.thread_id = NEW.thread_id
    ) THEN
        RAISE EXCEPTION 'Schedule result requires a visible legacy task in the occurrence scope'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_schedule_task_scope';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: require_legacy_source_task(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.require_legacy_source_task() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.task_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.background_tasks AS task
        WHERE task.id = NEW.task_id AND task.queue_kind = 'legacy'
    ) THEN
        RAISE EXCEPTION 'Legacy notification source requires a visible legacy task'
            USING ERRCODE = '23514', CONSTRAINT = 'legacy_source_task_queue_kind';
    END IF;
    -- Scoped FKs still reject foreign tasks and serialize concurrent deletion.
    -- Existing task queue identity cannot change after this visibility check.
    RETURN NEW;
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
-- Name: schedule_workflow_parent_wakeup(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.schedule_workflow_parent_wakeup() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.parent_run_id IS NOT NULL AND NEW.state IS DISTINCT FROM OLD.state
       AND NEW.state IN ('succeeded','failed','cancelled') THEN
        -- No parent lookup/FK/lock: this transaction owns only the child tree.
        INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,
                                        terminal_state,terminal_execution_id)
        VALUES(NEW.company_id,NEW.id,NEW.terminal_wakeup_sequence,'parent_wakeup',NEW.actor_id,
               NEW.state,NEW.terminal_execution_id);
    END IF;
    RETURN NULL;
END $$;


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
-- Name: thread_handoff_events_are_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.thread_handoff_events_are_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'DELETE'
       AND NOT EXISTS (SELECT 1 FROM companies WHERE id = OLD.company_id) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'thread handoff events are immutable' USING ERRCODE = '55000';
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
    RETURN (class = ANY (ARRAY['authentication'::text, 'rate_limited'::text, 'invalid_payload'::text, 'destination_unavailable'::text, 'network'::text, 'timeout'::text, 'provider_fault'::text, 'internal'::text, 'dependency_failed'::text, 'superseded'::text, 'lease_expired'::text, 'reconciled_not_applied'::text, 'not_applied_exhausted'::text, 'reconciliation_proof_expired'::text]));


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
-- Name: validate_workflow_job_association(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.validate_workflow_job_association() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    owning_run uuid;
    run_channel uuid;
    run_thread uuid;
BEGIN
    -- Link identity is immutable. Discover it before taking locks in run-first order.
    SELECT execution.run_id INTO owning_run
    FROM workflow_executions AS execution
    WHERE execution.company_id = NEW.company_id AND execution.id = NEW.workflow_execution_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'workflow job requires a visible scoped execution'
            USING ERRCODE = '23503', CONSTRAINT = 'background_tasks_workflow_execution_fk';
    END IF;
    SELECT run.channel_id, run.thread_id INTO run_channel, run_thread
    FROM workflow_runs AS run
    WHERE run.company_id = NEW.company_id AND run.id = owning_run
    FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'workflow job requires a visible scoped run'
            USING ERRCODE = '23503', CONSTRAINT = 'background_tasks_workflow_execution_fk';
    END IF;
    PERFORM 1 FROM workflow_executions AS execution
    WHERE execution.company_id = NEW.company_id AND execution.id = NEW.workflow_execution_id
      AND execution.run_id = owning_run
    FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'workflow job requires a visible scoped execution'
            USING ERRCODE = '23503', CONSTRAINT = 'background_tasks_workflow_execution_fk';
    END IF;
    IF NEW.channel_id IS DISTINCT FROM run_channel OR NEW.thread_id IS DISTINCT FROM run_thread THEN
        RAISE EXCEPTION 'workflow job association must equal its execution run association'
            USING ERRCODE = '23514', CONSTRAINT = 'background_tasks_workflow_association';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: validate_workflow_wakeup_insert(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.validate_workflow_wakeup_insert() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.event_kind='parent_wakeup' AND NOT EXISTS (
        SELECT 1 FROM workflow_runs AS child
        WHERE child.company_id=NEW.company_id AND child.id=NEW.run_id
          AND child.parent_run_id IS NOT NULL AND child.terminal_wakeup_sequence=NEW.sequence
          AND child.state=NEW.terminal_state AND child.actor_id=NEW.actor_id
          AND child.terminal_execution_id IS NOT DISTINCT FROM NEW.terminal_execution_id
    ) THEN
        RAISE EXCEPTION 'Workflow wakeup does not match generated terminal transition' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


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
-- Name: workflow_action_actual_observation_conflicts(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_actual_observation_conflicts() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,
            proof.id,NEW.remote_entry_id,NEW.id,'finality_breach'
        FROM workflow_action_evidence AS proof WHERE proof.company_id=NEW.company_id
            AND proof.invocation_id=NEW.invocation_id AND proof.disposition='final_not_applied'
            AND EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                WHERE coverage.company_id=NEW.company_id AND coverage.evidence_id=proof.id
                    AND coverage.remote_entry_id=NEW.remote_entry_id);
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,
            receipt.reconciliation_evidence_id,NEW.remote_entry_id,NEW.id,'actual_receipt_disagreement'
        FROM workflow_action_receipts AS receipt WHERE receipt.company_id=NEW.company_id
            AND receipt.invocation_id=NEW.invocation_id AND receipt.result<>NEW.result;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_actual_observation_receipt_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_actual_observation_receipt_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt
        WHERE receipt.company_id=NEW.company_id AND receipt.run_id=NEW.run_id
            AND receipt.execution_id=NEW.execution_id AND receipt.invocation_id=NEW.invocation_id
            AND receipt.argument_digest=NEW.argument_digest AND receipt.dispatch_id=NEW.dispatch_id)
    THEN RAISE EXCEPTION 'workflow action actual observation requires canonical receipt' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_applied_request_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_applied_request_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.disposition='applied' AND NEW.applied_request IS NULL
    THEN RAISE EXCEPTION 'applied evidence requires request attribution' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_audit_command_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_audit_command_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (SELECT count(*) FROM workflow_action_evidence_commands AS command
        WHERE command.company_id=NEW.company_id AND command.run_id=NEW.run_id
            AND command.audit_sequence=NEW.sequence AND command.execution_id=NEW.execution_id
            AND command.actor_id=NEW.actor_id AND command.evidence_id IS NOT NULL)<>1 THEN
        RAISE EXCEPTION 'workflow reconciliation audit requires one exact evidence command' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_audit_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_audit_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='DELETE' AND pg_trigger_depth()>1
        AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
    THEN RETURN OLD; END IF;
    IF OLD.event_kind='action_reconciled' THEN
        RAISE EXCEPTION 'workflow reconciliation actor audit is append-only' USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    IF NEW.event_kind='action_reconciled' THEN
        RAISE EXCEPTION 'workflow event cannot become a reconciliation actor audit' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_bind_schedule(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_bind_schedule() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.outcome->>'kind'='scheduled' THEN
        INSERT INTO workflow_action_claim_episodes(company_id,run_id,execution_id,job_id,command_key,retired_attempt)
            SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.scheduled_job_id,NEW.command_key,witness.retired_attempt
            FROM workflow_action_schedule_witnesses AS witness
            WHERE witness.company_id=NEW.company_id AND witness.run_id=NEW.run_id
                AND witness.execution_id=NEW.execution_id AND witness.job_id=NEW.scheduled_job_id
                AND witness.transaction_id=pg_current_xact_id();
        IF NOT FOUND THEN RAISE EXCEPTION 'workflow reconciliation schedule lacks current episode witness' USING ERRCODE='23514'; END IF;
    END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_capture_claim_retirement(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_capture_claim_retirement() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE episode workflow_action_claim_episodes%ROWTYPE; expected_state text; candidates bigint;
BEGIN
    IF OLD.state IN ('queued','running') AND NEW.state IN ('failed','waiting') AND OLD.deadline>clock_timestamp() THEN
        SELECT count(*) INTO candidates FROM workflow_action_claim_episodes AS binding
            WHERE binding.company_id=OLD.company_id AND binding.run_id=OLD.id
                AND workflow_action_pending_claim_episode(binding.company_id,binding.run_id,binding.execution_id,binding.job_id)=binding.command_key;
        IF candidates>1 THEN RAISE EXCEPTION 'multiple current workflow reconciliation claim episodes' USING ERRCODE='23514'; END IF;
        SELECT binding.* INTO episode FROM workflow_action_claim_episodes AS binding
            WHERE binding.company_id=OLD.company_id AND binding.run_id=OLD.id
                AND workflow_action_pending_claim_episode(binding.company_id,binding.run_id,binding.execution_id,binding.job_id)=binding.command_key;
        IF FOUND THEN
            -- Same order as claim: requesting run -> execution -> job -> shared usage.
            PERFORM step.id FROM workflow_executions AS step WHERE step.company_id=episode.company_id AND step.id=episode.execution_id FOR UPDATE;
            PERFORM job.id FROM background_tasks AS job WHERE job.company_id=episode.company_id AND job.id=episode.job_id FOR UPDATE;
            PERFORM usage.root_run_id FROM workflow_root_budget_usage AS usage
                JOIN workflow_run_budgets AS link ON link.company_id=usage.company_id AND link.root_run_id=usage.root_run_id
                WHERE link.company_id=OLD.company_id AND link.run_id=OLD.id FOR UPDATE OF usage;
            expected_state:=CASE WHEN workflow_action_retry_safe(episode.company_id,episode.execution_id) IS FALSE THEN 'waiting' ELSE 'failed' END;
            IF NOT workflow_action_reconciliation_budget_eligible(OLD.company_id,OLD.id)
                AND OLD.deadline>clock_timestamp() AND NEW.state=expected_state
                AND ((expected_state='waiting' AND NEW.waiting_reason='reconciliation' AND NEW.terminal_execution_id IS NULL)
                    OR (expected_state='failed' AND NEW.waiting_reason IS NULL AND NEW.terminal_execution_id=episode.execution_id)) THEN
                INSERT INTO workflow_action_claim_retirement_witnesses(company_id,run_id,execution_id,job_id,command_key,retired_attempt,transaction_id,deadline)
                    VALUES(episode.company_id,episode.run_id,episode.execution_id,episode.job_id,episode.command_key,episode.retired_attempt,pg_current_xact_id(),OLD.deadline);
            END IF;
        END IF;
    END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_capture_schedule(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_capture_schedule() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE run uuid;
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='failed' AND NEW.status='pending' THEN
        SELECT execution.run_id INTO run FROM workflow_executions AS execution
            WHERE execution.company_id=OLD.company_id AND execution.id=OLD.workflow_execution_id;
        INSERT INTO workflow_action_schedule_witnesses(company_id,run_id,execution_id,job_id,retired_attempt,transaction_id)
            VALUES(OLD.company_id,run,OLD.workflow_execution_id,OLD.id,OLD.retry_count,pg_current_xact_id());
    END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_capture_state_witness(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_capture_state_witness() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.state IS DISTINCT FROM NEW.state OR OLD.waiting_reason IS DISTINCT FROM NEW.waiting_reason THEN
        INSERT INTO workflow_action_state_witnesses(company_id,run_id,transaction_id,initial_state,initial_waiting_reason)
            VALUES(OLD.company_id,OLD.id,pg_current_xact_id(),OLD.state,OLD.waiting_reason)
            ON CONFLICT DO NOTHING;
    END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_claim_budget_refusal_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_claim_budget_refusal_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- Raw SQL obtains the same serialization; historical witnesses cannot qualify.
    PERFORM owner.id FROM workflow_runs AS owner WHERE owner.company_id=NEW.company_id AND owner.id=NEW.run_id FOR UPDATE;
    PERFORM step.id FROM workflow_executions AS step WHERE step.company_id=NEW.company_id AND step.run_id=NEW.run_id AND step.id=NEW.execution_id FOR UPDATE;
    PERFORM job.id FROM background_tasks AS job WHERE job.company_id=NEW.company_id AND job.id=NEW.job_id FOR UPDATE;
    PERFORM usage.root_run_id FROM workflow_root_budget_usage AS usage
        JOIN workflow_run_budgets AS link ON link.company_id=usage.company_id AND link.root_run_id=usage.root_run_id
        WHERE link.company_id=NEW.company_id AND link.run_id=NEW.run_id FOR UPDATE OF usage;
    IF workflow_action_reconciliation_budget_eligible(NEW.company_id,NEW.run_id)
        OR NOT EXISTS(SELECT 1 FROM workflow_action_claim_retirement_witnesses AS witness
            JOIN workflow_runs AS owner ON owner.company_id=witness.company_id AND owner.id=witness.run_id
            JOIN background_tasks AS job ON job.company_id=witness.company_id AND job.id=witness.job_id
            JOIN workflow_executions AS step ON step.company_id=witness.company_id AND step.run_id=witness.run_id AND step.id=witness.execution_id
            JOIN workflow_run_events AS audit ON audit.company_id=witness.company_id AND audit.run_id=witness.run_id AND audit.sequence=witness.audit_sequence
            WHERE witness.company_id=NEW.company_id AND witness.run_id=NEW.run_id AND witness.execution_id=NEW.execution_id
                AND witness.job_id=NEW.job_id AND witness.command_key=NEW.command_key AND witness.retired_attempt=NEW.retired_attempt
                AND witness.transaction_id=pg_current_xact_id() AND witness.retirement_confirmed AND witness.audit_sequence=NEW.audit_sequence
                AND witness.deadline>clock_timestamp() AND owner.deadline>clock_timestamp()
                AND ((owner.state='failed' AND owner.terminal_execution_id=NEW.execution_id AND owner.waiting_reason IS NULL
                    AND workflow_action_retry_safe(NEW.company_id,NEW.execution_id) IS DISTINCT FROM false)
                    OR (owner.state='waiting' AND owner.waiting_reason='reconciliation' AND owner.terminal_execution_id IS NULL
                        AND workflow_action_retry_safe(NEW.company_id,NEW.execution_id) IS FALSE))
                AND job.status='failed' AND job.queue_kind='workflow' AND job.workflow_execution_id=NEW.execution_id AND job.retry_count=NEW.retired_attempt
                AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL
                AND step.activated_at IS NOT NULL AND step.completed_at IS NULL
                AND audit.execution_id=NEW.execution_id AND audit.event_kind='workflow.root_budget_exhausted'
                AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=NEW.job_id AND later.attempt_number>NEW.retired_attempt))
    THEN RAISE EXCEPTION 'invalid current workflow reconciliation budget refusal' USING ERRCODE='23514'; END IF;
    IF TG_WHEN='BEFORE' THEN RETURN NEW; END IF;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_claim_episode_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_claim_episode_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
        JOIN workflow_runs AS owner ON owner.company_id=command.company_id AND owner.id=command.run_id
        JOIN background_tasks AS job ON job.company_id=command.company_id AND job.id=command.scheduled_job_id
        JOIN task_attempts AS attempt ON attempt.task_id=job.id AND attempt.attempt_number=NEW.retired_attempt
        JOIN workflow_action_schedule_witnesses AS schedule ON schedule.company_id=NEW.company_id
            AND schedule.run_id=NEW.run_id AND schedule.execution_id=NEW.execution_id AND schedule.job_id=NEW.job_id
            AND schedule.retired_attempt=NEW.retired_attempt AND schedule.transaction_id=pg_current_xact_id()
        JOIN workflow_action_state_witnesses AS state ON state.company_id=NEW.company_id AND state.run_id=NEW.run_id
            AND state.transaction_id=pg_current_xact_id()
        WHERE command.company_id=NEW.company_id AND command.command_key=NEW.command_key
            AND command.run_id=NEW.run_id AND command.execution_id=NEW.execution_id AND command.scheduled_job_id=NEW.job_id
            AND command.outcome->>'kind'='scheduled' AND command.result_revision=owner.revision
            AND state.initial_state='waiting' AND state.initial_waiting_reason='reconciliation'
            AND job.retry_count=NEW.retired_attempt AND job.status='pending'
            AND job.worker_id IS NULL AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=NEW.job_id AND later.attempt_number>NEW.retired_attempt))
    THEN RAISE EXCEPTION 'invalid current workflow reconciliation claim episode' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_claim_refusal_audit_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_claim_refusal_audit_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='DELETE' AND pg_trigger_depth()>1
        AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
    THEN RETURN OLD; END IF;
    IF EXISTS(SELECT 1 FROM workflow_action_claim_retirement_witnesses AS witness
        WHERE witness.company_id=OLD.company_id AND witness.run_id=OLD.run_id AND witness.audit_sequence=OLD.sequence)
        OR EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
            WHERE refusal.company_id=OLD.company_id AND refusal.run_id=OLD.run_id AND refusal.audit_sequence=OLD.sequence)
    THEN RAISE EXCEPTION 'workflow claim budget retirement audit is immutable' USING ERRCODE='23514'; END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_claim_retirement_atomic_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_claim_retirement_atomic_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.audit_sequence IS NOT NULL AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
        WHERE refusal.company_id=NEW.company_id AND refusal.run_id=NEW.run_id AND refusal.execution_id=NEW.execution_id
            AND refusal.job_id=NEW.job_id AND refusal.command_key=NEW.command_key AND refusal.retired_attempt=NEW.retired_attempt
            AND refusal.audit_sequence=NEW.audit_sequence)
    THEN RAISE EXCEPTION 'workflow claim budget retirement requires atomic refusal' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_claim_retirement_witness_owner(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_claim_retirement_witness_owner() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        IF pg_trigger_depth()>1
            AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
        THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'workflow claim retirement witness is database owned' USING ERRCODE='23514';
    END IF;
    IF pg_trigger_depth()<2 OR NEW.transaction_id<>pg_current_xact_id()
    THEN RAISE EXCEPTION 'workflow claim retirement witness is database owned' USING ERRCODE='23514'; END IF;
    IF TG_OP='UPDATE' AND (
        (to_jsonb(OLD)-'retirement_confirmed'-'audit_sequence') IS DISTINCT FROM (to_jsonb(NEW)-'retirement_confirmed'-'audit_sequence')
        OR (OLD.retirement_confirmed AND NOT NEW.retirement_confirmed)
        OR (OLD.audit_sequence IS NOT NULL AND OLD.audit_sequence IS DISTINCT FROM NEW.audit_sequence))
    THEN RAISE EXCEPTION 'workflow claim retirement witness is append-once' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_command_scope_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_command_scope_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.evidence_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
        WHERE evidence.company_id=NEW.company_id AND evidence.id=NEW.evidence_id
            AND evidence.command_key=NEW.command_key AND evidence.command_id=NEW.id
            AND evidence.request_digest=NEW.request_digest AND evidence.actor_id=NEW.actor_id
            AND evidence.run_id=NEW.run_id AND evidence.execution_id=NEW.execution_id
            AND evidence.invocation_id=NEW.invocation_id AND evidence.argument_digest=NEW.argument_digest
            AND evidence.dispatch_id=NEW.dispatch_id)
    THEN RAISE EXCEPTION 'workflow action command evidence scope mismatch' USING ERRCODE='23514'; END IF;
    IF NEW.evidence_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM workflow_run_events AS audit
        WHERE audit.company_id=NEW.company_id AND audit.run_id=NEW.run_id
            AND audit.sequence=NEW.audit_sequence AND audit.execution_id=NEW.execution_id
            AND audit.actor_id=NEW.actor_id AND audit.event_kind='action_reconciled'
    ) THEN
        RAISE EXCEPTION 'workflow action command audit scope mismatch' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_confirm_claim_retirement(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_confirm_claim_retirement() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF OLD.queue_kind='workflow' AND OLD.status='pending' AND NEW.status='failed'
        AND OLD.worker_id IS NULL AND OLD.execution_generation IS NULL AND OLD.lock_expires_at IS NULL
        AND NEW.worker_id IS NULL AND NEW.execution_generation IS NULL AND NEW.lock_expires_at IS NULL
        AND NEW.retry_count=OLD.retry_count AND NEW.workflow_execution_id=OLD.workflow_execution_id THEN
        UPDATE workflow_action_claim_retirement_witnesses AS witness SET retirement_confirmed=true
            WHERE witness.company_id=OLD.company_id AND witness.job_id=OLD.id AND witness.execution_id=OLD.workflow_execution_id
                AND witness.retired_attempt=OLD.retry_count AND witness.transaction_id=pg_current_xact_id()
                AND workflow_action_pending_claim_episode(witness.company_id,witness.run_id,witness.execution_id,witness.job_id)=witness.command_key;
    END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_conflict_park_pending(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_conflict_park_pending() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF NOT EXISTS(SELECT 1 FROM workflow_executions AS execution
        WHERE execution.company_id=NEW.company_id AND execution.run_id=NEW.run_id AND execution.id=NEW.execution_id
            AND execution.completed_at IS NULL AND execution.committed_output IS NULL
            AND execution.committed_route IS NULL AND execution.successor_execution_id IS NULL)
    THEN RETURN NULL; END IF;
    UPDATE background_tasks SET status='failed',updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND workflow_execution_id=NEW.execution_id
            AND queue_kind='workflow' AND status='pending';
    UPDATE workflow_runs SET state='waiting',waiting_reason='reconciliation'
        WHERE company_id=NEW.company_id AND id=NEW.run_id AND state IN ('queued','running')
            AND NOT EXISTS(SELECT 1 FROM background_tasks WHERE company_id=NEW.company_id
                AND workflow_execution_id=NEW.execution_id AND status='processing');
    RETURN NULL;
END $$;


--
-- Name: workflow_action_consumption_commit_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_consumption_commit_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.reservation_xid IS DISTINCT FROM pg_current_xact_id()
        OR NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=NEW.company_id AND entry.id=NEW.remote_entry_id
                AND entry.reservation_xid=NEW.reservation_xid)
        OR workflow_action_not_applied_available(NEW.company_id,NEW.invocation_id,NEW.remote_entry_id)
            IS DISTINCT FROM NEW.evidence_id
    THEN RAISE EXCEPTION 'workflow action final proof cannot be consumed' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_dispatch_commit_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_dispatch_commit_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_action_evidence_commit_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_evidence_commit_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_action_fact_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_fact_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='DELETE' AND pg_trigger_depth()>1
        AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
    THEN RETURN OLD; END IF;
    RAISE EXCEPTION 'workflow action effect fact is append-only';
END $$;


--
-- Name: workflow_action_has_evidence_conflict(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_has_evidence_conflict(company uuid, execution uuid) RETURNS boolean
    LANGUAGE sql
    AS $$
    SELECT EXISTS(SELECT 1 FROM workflow_action_evidence_conflicts AS conflict
        WHERE conflict.company_id=company AND conflict.execution_id=execution)
$$;


--
-- Name: workflow_action_intent_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_intent_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'workflow action intent is immutable';
    END IF;
    RETURN NEW;
END
$$;


--
-- Name: workflow_action_link_claim_retirement_audit(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_link_claim_retirement_audit() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.event_kind='workflow.root_budget_exhausted' AND NEW.execution_id IS NOT NULL THEN
        UPDATE workflow_action_claim_retirement_witnesses AS witness SET audit_sequence=NEW.sequence
            WHERE witness.company_id=NEW.company_id AND witness.run_id=NEW.run_id AND witness.execution_id=NEW.execution_id
                AND witness.transaction_id=pg_current_xact_id() AND witness.retirement_confirmed AND witness.audit_sequence IS NULL;
    END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_not_applied_available(uuid, uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_not_applied_available(company uuid, invocation uuid, excluded_entry uuid DEFAULT NULL::uuid) RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE proof workflow_action_evidence%ROWTYPE; marker workflow_action_dispatches%ROWTYPE;
BEGIN
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=company AND invocation_id=invocation;
    IF NOT FOUND OR marker.effect_kind<>'remote'
        OR workflow_action_has_evidence_conflict(company,marker.execution_id)
        OR EXISTS(SELECT 1 FROM workflow_action_receipts WHERE company_id=company AND invocation_id=invocation)
        OR EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=company AND invocation_id=invocation AND disposition='applied')
        OR (SELECT count(*) FROM (SELECT id FROM workflow_action_dispatches
            WHERE company_id=company AND execution_id=marker.execution_id LIMIT 129) AS bounded)>128
        OR (SELECT count(*) FROM (SELECT id FROM workflow_action_remote_entries
            WHERE company_id=company AND invocation_id=invocation
                AND id IS DISTINCT FROM excluded_entry LIMIT 129) AS bounded)>128
    THEN RETURN NULL; END IF;
    FOR proof IN SELECT * FROM workflow_action_evidence
        WHERE company_id=company AND invocation_id=invocation AND grant_eligible
        ORDER BY created_at,id
    LOOP
        IF proof.valid_until<=clock_timestamp() THEN CONTINUE; END IF;
        IF excluded_entry IS NULL THEN
            IF EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions
                WHERE company_id=company AND evidence_id=proof.id) THEN CONTINUE; END IF;
        ELSE
            IF NOT EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions AS consumption
                JOIN workflow_action_remote_entries AS consuming
                    ON (consuming.company_id,consuming.run_id,consuming.execution_id,consuming.invocation_id,
                        consuming.argument_digest,consuming.dispatch_id,consuming.id)
                    = (consumption.company_id,consumption.run_id,consumption.execution_id,consumption.invocation_id,
                        consumption.argument_digest,consumption.dispatch_id,consumption.remote_entry_id)
                WHERE (consumption.company_id,consumption.run_id,consumption.execution_id,consumption.invocation_id,
                        consumption.argument_digest,consumption.dispatch_id,consumption.evidence_id)
                    = (proof.company_id,proof.run_id,proof.execution_id,proof.invocation_id,
                        proof.argument_digest,proof.dispatch_id,proof.id)
                    AND consuming.id=excluded_entry)
            THEN CONTINUE; END IF;
        END IF;
        IF EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            JOIN task_attempts AS attempt ON attempt.task_id=entry.job_id AND attempt.attempt_number=entry.attempt_number
            WHERE entry.company_id=company AND entry.invocation_id=invocation
                AND entry.id IS DISTINCT FROM excluded_entry AND attempt.status='processing')
            OR EXISTS(SELECT 1 FROM task_attempts AS attempt WHERE attempt.task_id=marker.job_id
                AND attempt.attempt_number=marker.attempt_number AND attempt.status='processing'
                AND (excluded_entry IS NULL OR NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS consuming
                    WHERE consuming.company_id=company AND consuming.id=excluded_entry
                    AND consuming.job_id=attempt.task_id AND consuming.attempt_number=attempt.attempt_number)))
        THEN CONTINUE; END IF;
        IF EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=company AND entry.invocation_id=invocation
                AND entry.id IS DISTINCT FROM excluded_entry AND NOT EXISTS(
                    SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                    WHERE coverage.company_id=company AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=entry.id))
            OR EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                WHERE coverage.company_id=company AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=excluded_entry)
        THEN CONTINUE; END IF;
        RETURN proof.id;
    END LOOP;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_pending_claim_episode(uuid, uuid, uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_pending_claim_episode(company uuid, run uuid, execution uuid, job uuid) RETURNS text
    LANGUAGE sql
    AS $$
    SELECT episode.command_key FROM workflow_action_claim_episodes AS episode
        JOIN background_tasks AS task ON task.company_id=episode.company_id AND task.id=episode.job_id
        JOIN workflow_executions AS step ON step.company_id=episode.company_id AND step.run_id=episode.run_id AND step.id=episode.execution_id
        WHERE episode.company_id=company AND episode.run_id=run AND episode.execution_id=execution AND episode.job_id=job
            AND task.queue_kind='workflow' AND task.status='pending' AND task.retry_count=episode.retired_attempt
            AND task.worker_id IS NULL AND task.execution_generation IS NULL AND task.lock_expires_at IS NULL
            AND step.activated_at IS NOT NULL AND step.completed_at IS NULL
            AND NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=job AND later.attempt_number>episode.retired_attempt)
$$;


--
-- Name: workflow_action_receipt_evidence_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_receipt_evidence_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.reconciliation_evidence_id IS NOT NULL AND NOT EXISTS(
        SELECT 1 FROM workflow_action_evidence WHERE company_id=NEW.company_id
            AND id=NEW.reconciliation_evidence_id AND disposition='applied' AND registration IS NOT NULL)
    THEN RAISE EXCEPTION 'workflow action receipt requires verified applied evidence' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_receipt_finality_conflict(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_receipt_finality_conflict() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE applied workflow_action_evidence%ROWTYPE;
BEGIN
    IF NEW.remote_entry_id IS NOT NULL THEN
        INSERT INTO workflow_action_actual_receipt_observations(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,remote_entry_id,result)
            VALUES(NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,NEW.remote_entry_id,NEW.result)
            ON CONFLICT(company_id,remote_entry_id,result_digest) DO NOTHING;
        -- A SQL transaction may have recorded other bounded incoming truth
        -- before its first canonical receipt. Compare it even on the actual branch.
        INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason)
            SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,
                NULL,observation.remote_entry_id,observation.id,'actual_receipt_disagreement'
            FROM workflow_action_actual_receipt_observations AS observation
            WHERE observation.company_id=NEW.company_id AND observation.invocation_id=NEW.invocation_id
                AND observation.result<>NEW.result AND NOT EXISTS(
                    SELECT 1 FROM workflow_action_evidence_conflicts AS conflict
                    WHERE conflict.company_id=NEW.company_id AND conflict.actual_observation_id=observation.id
                        AND conflict.reason='actual_receipt_disagreement');
        RETURN NULL;
    END IF;
    IF NEW.reconciliation_evidence_id IS NOT NULL THEN
        SELECT * INTO applied FROM workflow_action_evidence
            WHERE company_id=NEW.company_id AND id=NEW.reconciliation_evidence_id;
    END IF;
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,contradictory_evidence_id,remote_entry_id,reason)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,proof.id,
            NEW.reconciliation_evidence_id,NEW.remote_entry_id,
            CASE WHEN applied.applied_request='unattributed' OR (NEW.reconciliation_evidence_id IS NOT NULL AND applied.applied_request IS NULL)
                THEN 'unattributed_applied' ELSE 'finality_breach' END
        FROM workflow_action_evidence AS proof WHERE proof.company_id=NEW.company_id
            AND proof.invocation_id=NEW.invocation_id AND proof.disposition='final_not_applied'
            AND ((NEW.remote_entry_id IS NOT NULL AND EXISTS(
                SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=NEW.company_id
                    AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=NEW.remote_entry_id))
                OR (NEW.reconciliation_evidence_id IS NOT NULL AND (
                    applied.applied_request IS NULL OR applied.applied_request='unattributed'
                    OR (applied.applied_request='remote_entry' AND EXISTS(
                        SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=NEW.company_id
                            AND coverage.evidence_id=proof.id AND coverage.remote_entry_id=applied.applied_remote_entry_id))
                    OR (applied.applied_request='marker_reservation' AND NOT EXISTS(
                        SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                            WHERE coverage.company_id=NEW.company_id AND coverage.evidence_id=proof.id)))));
    INSERT INTO workflow_action_evidence_conflicts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id,actual_observation_id,reason)
        SELECT NEW.company_id,NEW.run_id,NEW.execution_id,NEW.invocation_id,NEW.argument_digest,NEW.dispatch_id,
            NEW.reconciliation_evidence_id,observation.remote_entry_id,observation.id,'actual_receipt_disagreement'
        FROM workflow_action_actual_receipt_observations AS observation WHERE observation.company_id=NEW.company_id
            AND observation.invocation_id=NEW.invocation_id AND observation.result<>NEW.result;
    RETURN NULL;
END $$;


--
-- Name: workflow_action_reconciliation_budget_eligible(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_reconciliation_budget_eligible(company uuid, run uuid) RETURNS boolean
    LANGUAGE sql
    AS $$
    SELECT EXISTS(SELECT 1 FROM workflow_run_budgets AS link
        JOIN workflow_root_budgets AS budget ON budget.company_id=link.company_id AND budget.root_run_id=link.root_run_id
        JOIN workflow_root_budget_usage AS usage ON usage.company_id=budget.company_id AND usage.root_run_id=budget.root_run_id
        WHERE link.company_id=company AND link.run_id=run AND budget.provenance='frozen_v2'
            AND usage.activations<=budget.activations AND usage.model_calls<budget.model_calls
            AND usage.repetitions<=budget.repetitions
            AND NOT EXISTS(SELECT 1 FROM workflow_budget_receipts AS receipt
                WHERE receipt.company_id=company AND receipt.run_id=run AND receipt.disposition='exhausted'))
$$;


--
-- Name: workflow_action_reconciliation_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_reconciliation_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_action_reconciliation_reopen_safe(uuid, uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_reconciliation_reopen_safe(company uuid, run uuid, job uuid) RETURNS boolean
    LANGUAGE sql
    AS $$
    SELECT EXISTS(SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
        WHERE owner.company_id=company AND owner.id=run AND task.id=job AND task.queue_kind='workflow'
            AND owner.deadline>clock_timestamp() AND execution.activation<=owner.max_steps
            AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
            AND execution.committed_output IS NULL AND execution.committed_route IS NULL AND execution.successor_execution_id IS NULL
            AND task.retry_count>0 AND task.retry_count<task.max_retries
            AND task.worker_id IS NULL AND task.execution_generation IS NULL AND task.lock_expires_at IS NULL
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND attempt.workflow_retirement IN ('live','expired')
            AND attempt.workflow_failure_code NOT IN ('workflow.activation_limit','workflow.invalid_result','workflow.run_deadline','workflow.root_budget_exhausted')
            AND workflow_action_retry_safe(company,execution.id) IS TRUE
            AND workflow_action_reconciliation_budget_eligible(company,run)
            AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
                WHERE refusal.company_id=company AND refusal.run_id=run AND refusal.execution_id=execution.id AND refusal.job_id=job)
            AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
            AND NOT EXISTS(SELECT 1 FROM workflow_executions AS sibling
                JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                WHERE sibling.company_id=company AND sibling.run_id=run AND other.queue_kind='workflow'
                    AND other.id<>job AND other.status IN ('pending','processing')))
$$;


--
-- Name: workflow_action_remote_entry_commit_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_remote_entry_commit_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE marker workflow_action_dispatches%ROWTYPE; proof uuid;
BEGIN
    PERFORM id FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    PERFORM id FROM workflow_executions WHERE company_id=NEW.company_id AND id=NEW.execution_id FOR UPDATE;
    PERFORM id FROM background_tasks WHERE company_id=NEW.company_id AND id=NEW.job_id FOR UPDATE;
    PERFORM id FROM task_attempts WHERE task_id=NEW.job_id AND attempt_number=NEW.attempt_number FOR UPDATE;
    IF NOT EXISTS(SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=job.id
        WHERE owner.company_id=NEW.company_id AND owner.id=NEW.run_id
            AND owner.state IN ('queued','running') AND owner.deadline>clock_timestamp()
            AND execution.id=NEW.execution_id AND execution.completed_at IS NULL
            AND job.id=NEW.job_id AND job.queue_kind='workflow' AND job.status='processing'
            AND job.worker_id=NEW.worker_id AND job.execution_generation=NEW.execution_generation
            AND job.retry_count+1=NEW.attempt_number AND job.lock_expires_at>clock_timestamp()
            AND attempt.attempt_number=NEW.attempt_number AND attempt.worker_id=NEW.worker_id
            AND attempt.execution_generation=NEW.execution_generation AND attempt.status='processing')
    THEN RAISE EXCEPTION 'workflow action remote entry lost live fence'; END IF;
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id;
    proof:=workflow_action_not_applied_available(NEW.company_id,NEW.invocation_id,NEW.id);
    IF workflow_action_has_evidence_conflict(NEW.company_id,NEW.execution_id)
        OR EXISTS(SELECT 1 FROM workflow_action_receipts WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id)
        OR EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id AND disposition='applied')
        OR EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            JOIN task_attempts AS attempt ON attempt.task_id=entry.job_id AND attempt.attempt_number=entry.attempt_number
            WHERE entry.company_id=NEW.company_id AND entry.invocation_id=NEW.invocation_id
                AND entry.id<>NEW.id AND attempt.status='processing')
        OR (marker.attempt_number<>NEW.attempt_number AND (marker.attempt_number>=NEW.attempt_number
            OR (proof IS NULL AND NOT COALESCE(workflow_action_replay_supported(NEW.company_id,NEW.invocation_id),false))))
        OR (EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions WHERE company_id=NEW.company_id AND remote_entry_id=NEW.id) AND proof IS NULL)
    THEN RAISE EXCEPTION 'workflow action remote entry lacks supported replay'; END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_action_replay_supported(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_replay_supported(company uuid, invocation uuid) RETURNS boolean
    LANGUAGE plpgsql
    AS $_$
DECLARE marker workflow_action_dispatches%ROWTYPE;
    operation jsonb; subject jsonb; proof jsonb; guarantee jsonb;
    seconds numeric; nanos numeric; revision numeric; horizon timestamptz;
BEGIN
    SELECT * INTO marker FROM workflow_action_dispatches
        WHERE company_id=company AND invocation_id=invocation;
    IF NOT FOUND OR marker.effect_kind<>'remote' OR marker.replay_subject IS NULL THEN RETURN false; END IF;
    SELECT intent.operation, owner.deadline INTO operation,horizon
        FROM workflow_action_intents AS intent
        JOIN workflow_runs AS owner ON owner.company_id=intent.company_id AND owner.id=intent.run_id
        WHERE intent.company_id=company AND intent.id=invocation;
    subject := convert_from(marker.replay_subject,'UTF8')::jsonb;
    proof := marker.current_policy->'provider_replay';
    guarantee := proof->'guarantee';
    IF subject IS DISTINCT FROM jsonb_build_object('tool',operation->'contract','target',operation->'target')
        OR operation->'contract'->>'company_id' IS DISTINCT FROM company::text
        OR marker.current_policy->'tool' IS DISTINCT FROM operation->'contract'
        OR marker.current_policy->'approval_required' IS DISTINCT FROM 'false'::jsonb
        OR jsonb_typeof(proof) IS DISTINCT FROM 'object'
        OR proof->'version' IS DISTINCT FROM '1'::jsonb
        OR jsonb_typeof(proof->'registration') IS DISTINCT FROM 'string'
        OR octet_length(proof->>'registration') > 128
        OR (proof->>'registration') !~ '^[A-Za-z_][A-Za-z0-9_-]*(\.[A-Za-z_][A-Za-z0-9_-]*)*$'
        OR jsonb_typeof(proof->'operation') IS DISTINCT FROM 'string'
        OR proof->>'operation' IS DISTINCT FROM encode(sha256(marker.replay_subject),'hex')
        OR proof - ARRAY['version','registration','operation','guarantee'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee) IS DISTINCT FROM 'object'
        OR jsonb_typeof(operation->'contract'->'policy'->'policy_revision') IS DISTINCT FROM 'number'
    THEN RETURN false; END IF;
    revision := (operation->'contract'->'policy'->>'policy_revision')::numeric;
    IF revision <> trunc(revision) OR revision <= 0 OR revision > 18446744073709551615
    THEN RETURN false; END IF;
    IF operation->'contract'->'policy'->>'recovery'='safe_repeat' THEN
        RETURN guarantee = '{"mode":"safe_repeat"}'::jsonb;
    END IF;
    IF operation->'contract'->'policy'->>'recovery' IS DISTINCT FROM 'provider_idempotency'
        OR guarantee->>'mode' IS DISTINCT FROM 'provider_idempotency'
        OR guarantee - ARRAY['mode','retention'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee->'retention') IS DISTINCT FROM 'object'
        OR (guarantee->'retention') - ARRAY['secs','nanos'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee->'retention'->'secs') IS DISTINCT FROM 'number'
        OR jsonb_typeof(guarantee->'retention'->'nanos') IS DISTINCT FROM 'number'
    THEN RETURN false; END IF;
    seconds := (guarantee->'retention'->>'secs')::numeric;
    nanos := (guarantee->'retention'->>'nanos')::numeric;
    IF seconds <> trunc(seconds) OR nanos <> trunc(nanos) OR seconds < 0
        OR nanos < 0 OR nanos >= 1000000000 OR seconds+nanos/1000000000 <= 0
        OR seconds+nanos/1000000000 > 31536000 THEN RETURN false; END IF;
    -- Round down sub-microsecond guarantees, conservatively. Both first creation
    -- and the run horizon are authoritative DB timestamps, immune to lock waits.
    RETURN horizon>clock_timestamp() AND marker.created_at
        + make_interval(secs => (seconds+floor(nanos/1000)/1000000)::double precision) >= horizon;
EXCEPTION WHEN OTHERS THEN RETURN false;
END $_$;


--
-- Name: workflow_action_reservation_identity(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_reservation_identity() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.reservation_xid IS NOT NULL AND NEW.reservation_xid<>pg_current_xact_id()
    THEN RAISE EXCEPTION 'invalid workflow action reservation transaction' USING ERRCODE='23514'; END IF;
    NEW.reservation_xid:=pg_current_xact_id();
    RETURN NEW;
END $$;


--
-- Name: workflow_action_retry_safe(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_retry_safe(company uuid, execution uuid) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
DECLARE fact record; scanned integer:=0; safe boolean:=true;
BEGIN
    IF workflow_action_has_evidence_conflict(company,execution) THEN RETURN false; END IF;
    FOR fact IN SELECT invocation_id FROM workflow_action_dispatches
        WHERE company_id=company AND execution_id=execution ORDER BY invocation_id LIMIT 129
    LOOP
        scanned:=scanned+1;
        IF scanned>128 THEN RETURN false; END IF;
        IF EXISTS(SELECT 1 FROM workflow_action_receipts WHERE company_id=company AND invocation_id=fact.invocation_id)
        THEN CONTINUE; END IF;
        IF EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=company
            AND invocation_id=fact.invocation_id AND disposition='applied')
            OR NOT (COALESCE(workflow_action_replay_supported(company,fact.invocation_id),false)
                OR workflow_action_not_applied_available(company,fact.invocation_id) IS NOT NULL)
        THEN safe:=false; END IF;
    END LOOP;
    IF scanned=0 THEN RETURN NULL; END IF;
    RETURN safe;
END $$;


--
-- Name: workflow_action_state_witness_owner(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_action_state_witness_owner() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF pg_trigger_depth()<2 OR NEW.transaction_id<>pg_current_xact_id()
    THEN RAISE EXCEPTION 'workflow action state witness is database owned' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_control_retry_safe(uuid, uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_control_retry_safe(company uuid, run uuid, job uuid) RETURNS boolean
    LANGUAGE sql
    AS $$
    SELECT EXISTS (
        SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
        WHERE owner.company_id=company AND owner.id=run AND task.id=job AND task.queue_kind='workflow'
            AND owner.deadline>clock_timestamp() AND execution.activation<=owner.max_steps
            AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
            AND execution.committed_output IS NULL AND execution.committed_route IS NULL
            AND execution.successor_execution_id IS NULL
            AND task.retry_count>0 AND task.retry_count<task.max_retries
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND workflow_action_retry_safe(company,execution.id) IS DISTINCT FROM false
            AND attempt.workflow_retry_safety='safe' AND attempt.workflow_retirement IN ('live','expired')
            AND attempt.workflow_failure_code NOT IN ('workflow.activation_limit','workflow.invalid_result','workflow.run_deadline')
            AND NOT EXISTS (SELECT 1 FROM workflow_budget_receipts AS receipt
                WHERE receipt.company_id=company AND receipt.run_id=run AND receipt.disposition='exhausted')
            AND NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
                WHERE refusal.company_id=company AND refusal.run_id=run AND refusal.execution_id=execution.id AND refusal.job_id=job)
            AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
            AND NOT EXISTS (SELECT 1 FROM workflow_executions AS sibling
                JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                WHERE sibling.company_id=company AND sibling.run_id=run AND other.queue_kind='workflow'
                    AND other.id<>job AND other.status IN ('pending','processing'))
    )
$$;


--
-- Name: workflow_http_identity_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_http_identity_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.company_id, NEW.id, NEW.origin, NEW.bearer, NEW.created_at)
        IS DISTINCT FROM (OLD.company_id, OLD.id, OLD.origin, OLD.bearer, OLD.created_at) THEN
        RAISE EXCEPTION 'HTTP connection identity is immutable';
    END IF;
    RETURN NEW;
END;
$$;


--
-- Name: workflow_message_action_capture(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_action_capture() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_message_action_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_action_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_message_admission_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_admission_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_message_aggregate(uuid, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_aggregate(company uuid, invocation uuid) RETURNS TABLE(state text, result jsonb, destinations integer)
    LANGUAGE sql STABLE
    AS $$
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


--
-- Name: workflow_message_delivery_event(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_delivery_event() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    INSERT INTO workflow_message_delivery_events
        (company_id, run_id, invocation_id, ordinal, delivery_id, old_status, status, failure_class)
    SELECT target.company_id, target.run_id, target.invocation_id, target.ordinal, target.delivery_id,
        OLD.status, NEW.status, NEW.last_error_class
    FROM workflow_message_destinations AS target
    WHERE target.company_id=NEW.company_id AND target.delivery_id=NEW.id;
    RETURN NULL;
END $$;


--
-- Name: workflow_message_delivery_transition_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_delivery_transition_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_message_destination_state(uuid, text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_destination_state(delivery_id uuid, delivery_status text) RETURNS text
    LANGUAGE sql IMMUTABLE
    AS $$
    SELECT CASE WHEN delivery_id IS NULL OR delivery_status='delivered' THEN 'accepted'
        WHEN delivery_status='dead_letter' THEN 'failed'
        WHEN delivery_status='outcome_unknown' THEN 'unknown'
        ELSE 'pending' END
$$;


--
-- Name: workflow_message_effect_wait_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_effect_wait_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.event_name IS NOT NULL OR NEW.correlation IS NOT NULL OR NEW.notification_intent IS NOT NULL
        OR NOT EXISTS(SELECT 1 FROM workflow_runs AS run WHERE run.company_id=NEW.company_id
            AND run.id=NEW.run_id AND run.deadline=NEW.deadline)
        OR NOT EXISTS(SELECT 1 FROM workflow_message_actions AS action WHERE action.company_id=NEW.company_id
            AND action.run_id=NEW.run_id AND action.execution_id=NEW.execution_id AND action.wait_id=NEW.id)
    THEN RAISE EXCEPTION 'workflow effect wait must belong to one message action' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;


--
-- Name: workflow_message_part_transition_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_part_transition_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_message_receipt_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_receipt_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: workflow_message_reopen_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_reopen_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: agent_channel_provisions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.agent_channel_provisions (
    task_id uuid NOT NULL,
    request_hash text NOT NULL,
    agent_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    warnings jsonb DEFAULT '[]'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    company_id uuid NOT NULL
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
    CONSTRAINT agents_config_v1_shape_check CHECK (((config_json IS NULL) OR COALESCE(((jsonb_typeof(config_json) = 'object'::text) AND (jsonb_typeof((config_json -> 'version'::text)) = 'number'::text) AND ((config_json ->> 'version'::text) = '1'::text) AND (octet_length((config_json)::text) <= 65536)), false))),
    CONSTRAINT agents_created_by_shape_check CHECK (public.valid_creation_provenance(created_by)),
    CONSTRAINT agents_granted_tool_ids_bounded CHECK (public.valid_tool_id_array(granted_tool_ids)),
    CONSTRAINT agents_harness_kind_check CHECK ((harness_kind = ANY (ARRAY['ai_agents'::text, 'rig'::text]))),
    CONSTRAINT agents_memory_max_results_check CHECK (((memory_max_results >= 1) AND (memory_max_results <= 20))),
    CONSTRAINT agents_memory_persistence_mode_check CHECK ((memory_persistence_mode = ANY (ARRAY['audience_only'::text, 'scope_specific_facts'::text]))),
    CONSTRAINT agents_memory_recall_mode_check CHECK ((memory_recall_mode = ANY (ARRAY['fast'::text, 'thinking'::text]))),
    CONSTRAINT agents_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT agents_native_tool_policy_shape CHECK (COALESCE(((jsonb_typeof(native_tool_policy) = 'object'::text) AND (jsonb_typeof((native_tool_policy -> 'version'::text)) = 'number'::text) AND ((native_tool_policy ->> 'version'::text) = '1'::text) AND (octet_length((native_tool_policy)::text) <= 16384)), false)),
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
    channel_id uuid,
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
    queue_kind text DEFAULT 'legacy'::text NOT NULL,
    workflow_execution_id uuid,
    CONSTRAINT background_tasks_attention_version_check CHECK ((attention_version > 0)),
    CONSTRAINT background_tasks_business_priority_check CHECK ((business_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))),
    CONSTRAINT background_tasks_lease_check CHECK ((((status = 'processing'::text) AND (worker_id IS NOT NULL) AND (execution_generation IS NOT NULL) AND (locked_at IS NOT NULL) AND (lock_expires_at IS NOT NULL) AND (lock_expires_at > locked_at)) OR ((status <> 'processing'::text) AND (worker_id IS NULL) AND (execution_generation IS NULL) AND (locked_at IS NULL) AND (lock_expires_at IS NULL)))),
    CONSTRAINT background_tasks_legacy_channel_required CHECK (((queue_kind <> 'legacy'::text) OR (channel_id IS NOT NULL))),
    CONSTRAINT background_tasks_max_retries_check CHECK ((max_retries > 0)),
    CONSTRAINT background_tasks_owner_shape_check CHECK ((((owner_principal_id IS NULL) AND (owner_principal_kind IS NULL)) OR ((owner_principal_id IS NOT NULL) AND (owner_principal_kind IS NOT NULL) AND (owner_principal_kind = ANY (ARRAY['person'::text, 'agent'::text]))))),
    CONSTRAINT background_tasks_ownership_version_check CHECK ((ownership_version > 0)),
    CONSTRAINT background_tasks_payload_object_check CHECK ((jsonb_typeof(payload) = 'object'::text)),
    CONSTRAINT background_tasks_queue_kind_check CHECK ((queue_kind = ANY (ARRAY['legacy'::text, 'workflow'::text]))),
    CONSTRAINT background_tasks_retry_count_check CHECK ((retry_count >= 0)),
    CONSTRAINT background_tasks_single_source_check CHECK (((source_message_uuid IS NULL) OR (source_schedule_run_id IS NULL))),
    CONSTRAINT background_tasks_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'processing'::text, 'pending_approval'::text, 'waiting_for_third_party_reply'::text, 'completed'::text, 'failed'::text, 'dead_letter'::text, 'stopped'::text]))),
    CONSTRAINT background_tasks_transition_actor_kind_check CHECK (((transition_actor_kind IS NULL) OR (transition_actor_kind = ANY (ARRAY['system'::text, 'worker'::text, 'operator'::text, 'human'::text, 'agent'::text, 'approval'::text, 'outreach'::text])))),
    CONSTRAINT background_tasks_transition_reason_check CHECK (((transition_reason IS NULL) OR (transition_reason = ANY (ARRAY['enqueued'::text, 'claimed'::text, 'completed'::text, 'retryable_failure'::text, 'terminal_failure'::text, 'timed_out'::text, 'shutdown'::text, 'lease_lost'::text, 'approval_requested'::text, 'approval_accepted'::text, 'approval_rejected'::text, 'outreach_started'::text, 'outreach_reply_received'::text, 'outreach_timed_out'::text, 'outreach_extended'::text, 'operator_stopped'::text, 'operator_resumed'::text, 'ownership_transferred'::text, 'agent_instruction'::text, 'delegation_target_cancelled'::text, 'delegation_cancelled'::text, 'delegation_reassigned'::text, 'delegation_partial'::text, 'channel_agent_removed'::text, 'unknown'::text])))),
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
END))),
    CONSTRAINT background_tasks_workflow_shape_check CHECK ((((queue_kind = 'legacy'::text) AND (workflow_execution_id IS NULL)) OR ((queue_kind = 'workflow'::text) AND (workflow_execution_id IS NOT NULL) AND (task_type = 'workflow_execution'::text) AND (payload = jsonb_build_object('version', 1, 'execution_id', (workflow_execution_id)::text)) AND (owner_principal_id IS NULL) AND (owner_principal_kind IS NULL) AND (source_message_uuid IS NULL) AND (source_schedule_run_id IS NULL) AND (awaited_outreach_id IS NULL) AND (wait_expires_at IS NULL) AND (transition_reason IS NULL) AND (transition_actor_kind IS NULL) AND (transition_actor_id IS NULL) AND (transition_approval_id IS NULL) AND (transition_outreach_id IS NULL))))
);


--
-- Name: COLUMN background_tasks.workflow_execution_id; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.background_tasks.workflow_execution_id IS 'Logical workflow execution; attempts and fenced leases remain owned by background_tasks/task_attempts.';


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
    external_reply_handling text DEFAULT 'automatic'::text NOT NULL,
    CONSTRAINT companies_avatar_url_scheme_check CHECK (((avatar_url IS NULL) OR (avatar_url ~ '^https?://'::text))),
    CONSTRAINT companies_default_participants_bounded CHECK (((default_participant_emails IS NULL) OR ((cardinality(default_participant_emails) <= 64) AND (array_position(default_participant_emails, NULL::public.citext) IS NULL) AND (array_position(default_participant_emails, ''::public.citext) IS NULL)))),
    CONSTRAINT companies_external_reply_handling_check CHECK ((external_reply_handling = ANY (ARRAY['automatic'::text, 'manual_handoff'::text]))),
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
    CONSTRAINT delegation_control_commands_actor_kind_check CHECK ((actor_kind = ANY (ARRAY['person'::text, 'agent'::text, 'external'::text]))),
    CONSTRAINT delegation_control_commands_authority_check CHECK ((authority = ANY (ARRAY['human_owner'::text, 'company_manager'::text, 'owning_agent'::text]))),
    CONSTRAINT delegation_control_commands_operation_check CHECK ((operation = ANY (ARRAY['extend_outreach'::text, 'cancel_target'::text, 'cancel_outreach'::text, 'reassign_internal_target'::text, 'reassign_person_target'::text, 'proceed_with_partial'::text, 'stop_task'::text]))),
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
    cancellation_requested_at timestamp with time zone,
    cancellation_reason text,
    settled_evidence_id uuid,
    reopen_id uuid,
    CONSTRAINT message_deliveries_attempt_check CHECK (((attempt_count >= 0) AND (max_attempts > 0) AND (attempt_count <= max_attempts))),
    CONSTRAINT message_deliveries_attribution_check CHECK ((((company_id IS NOT NULL) AND (channel_id IS NOT NULL) AND (message_id IS NOT NULL) AND (source_binding_id IS NOT NULL) AND (destination_binding_id IS NOT NULL)) OR ((company_id IS NULL) AND (channel_id IS NULL) AND (message_id IS NULL) AND (source_binding_id IS NULL) AND (destination_binding_id IS NULL) AND (task_id IS NULL) AND (depends_on_delivery_id IS NULL) AND (external_destination IS NOT NULL) AND (purpose = 'notification'::text)))),
    CONSTRAINT message_deliveries_cancellation_check CHECK ((((cancellation_requested_at IS NULL) = (cancellation_reason IS NULL)) AND ((cancellation_reason IS NULL) OR (cancellation_reason = ANY (ARRAY['channel_agent_removed'::text, 'task_stopped'::text, 'workflow_cancelled'::text, 'workflow_expired'::text, 'workflow_revoked'::text, 'recipient_revoked'::text, 'evidence_conflict'::text]))) AND ((cancellation_requested_at IS NULL) OR (status <> ALL (ARRAY['pending'::text, 'retryable'::text]))))),
    CONSTRAINT message_deliveries_delivered_at_check CHECK (((status = 'delivered'::text) = (delivered_at IS NOT NULL))),
    CONSTRAINT message_deliveries_error_check CHECK ((((last_error_class IS NULL) OR public.valid_delivery_failure_class(last_error_class)) AND ((last_error_detail IS NULL) OR (octet_length(last_error_detail) <= 512)) AND ((last_error_detail IS NULL) OR (last_error_class IS NOT NULL)))),
    CONSTRAINT message_deliveries_external_destination_check CHECK (((external_destination IS NULL) OR ((btrim(external_destination) <> ''::text) AND (octet_length(external_destination) <= 998)))),
    CONSTRAINT message_deliveries_idempotency_key_check CHECK (((btrim(idempotency_key) <> ''::text) AND (octet_length(idempotency_key) <= 512))),
    CONSTRAINT message_deliveries_lease_check CHECK ((((status = 'sending'::text) AND (execution_id IS NOT NULL) AND (owner_worker_id IS NOT NULL) AND (locked_at IS NOT NULL) AND (lock_expires_at IS NOT NULL) AND (lock_expires_at > locked_at)) OR ((status <> 'sending'::text) AND (execution_id IS NULL) AND (owner_worker_id IS NULL) AND (locked_at IS NULL) AND (lock_expires_at IS NULL)))),
    CONSTRAINT message_deliveries_message_audience_check CHECK ((((message_id IS NULL) AND (message_audience IS NULL)) OR ((message_id IS NOT NULL) AND (message_audience = 'external_conversation'::text)))),
    CONSTRAINT message_deliveries_no_self_dependency_check CHECK (((depends_on_delivery_id IS NULL) OR (depends_on_delivery_id <> id))),
    CONSTRAINT message_deliveries_purpose_check CHECK ((purpose = ANY (ARRAY['reply'::text, 'mirror'::text, 'outreach'::text, 'notification'::text, 'workflow_send'::text, 'workflow_reply'::text]))),
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
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
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
    workflow_failure_class text,
    workflow_failure_code text,
    workflow_retry_safety text,
    workflow_retirement text,
    CONSTRAINT task_attempts_machine_id_check CHECK ((length(TRIM(BOTH FROM machine_id)) > 0)),
    CONSTRAINT task_attempts_status_check CHECK ((status = ANY (ARRAY['processing'::text, 'completed'::text, 'failed'::text]))),
    CONSTRAINT task_attempts_stop_reason_check CHECK ((stop_reason = ANY (ARRAY['completed'::text, 'retryable_failure'::text, 'terminal_failure'::text, 'timed_out'::text, 'shutdown'::text, 'lease_lost'::text, 'ownership_transferred'::text, 'agent_instruction'::text, 'delegation_cancelled'::text, 'operator_stopped'::text, 'approval_rejected'::text, 'channel_agent_removed'::text, 'effect_parked'::text]))),
    CONSTRAINT task_attempts_token_check CHECK ((((prompt_tokens IS NULL) OR (prompt_tokens >= 0)) AND ((completion_tokens IS NULL) OR (completion_tokens >= 0)))),
    CONSTRAINT workflow_attempt_failure_shape CHECK ((((workflow_failure_class IS NULL) AND (workflow_failure_code IS NULL) AND (workflow_retry_safety IS NULL) AND (workflow_retirement IS NULL)) OR ((status = 'failed'::text) AND (finished_at IS NOT NULL) AND (workflow_failure_class IS NOT NULL) AND (workflow_failure_class = ANY (ARRAY['retryable'::text, 'terminal'::text])) AND (workflow_failure_code IS NOT NULL) AND ((octet_length(workflow_failure_code) >= 1) AND (octet_length(workflow_failure_code) <= 128)) AND (workflow_retry_safety IS NOT NULL) AND (workflow_retry_safety = ANY (ARRAY['safe'::text, 'unknown'::text])) AND (workflow_retirement IS NOT NULL) AND (workflow_retirement = ANY (ARRAY['live'::text, 'expired'::text, 'deadline'::text, 'cancel'::text])))))
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
    CONSTRAINT task_outreaches_creator_shape_check CHECK ((((created_by_principal_id IS NULL) AND (created_by_principal_kind IS NULL)) OR ((created_by_principal_id IS NOT NULL) AND (created_by_principal_kind IS NOT NULL) AND (created_by_principal_kind = 'agent'::text)))),
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
    CONSTRAINT task_status_events_reason_check CHECK ((reason = ANY (ARRAY['enqueued'::text, 'claimed'::text, 'completed'::text, 'retryable_failure'::text, 'terminal_failure'::text, 'timed_out'::text, 'shutdown'::text, 'lease_lost'::text, 'approval_requested'::text, 'approval_accepted'::text, 'approval_rejected'::text, 'outreach_started'::text, 'outreach_reply_received'::text, 'outreach_timed_out'::text, 'outreach_extended'::text, 'operator_stopped'::text, 'operator_resumed'::text, 'ownership_transferred'::text, 'agent_instruction'::text, 'delegation_target_cancelled'::text, 'delegation_cancelled'::text, 'delegation_reassigned'::text, 'delegation_partial'::text, 'channel_agent_removed'::text, 'unknown'::text]))),
    CONSTRAINT task_status_events_related_source_check CHECK (((related_approval_id IS NULL) OR (related_outreach_id IS NULL))),
    CONSTRAINT task_status_events_retry_count_check CHECK ((retry_count >= 0)),
    CONSTRAINT task_status_events_sequence_check CHECK ((sequence > 0)),
    CONSTRAINT task_status_events_to_status_check CHECK ((to_status = ANY (ARRAY['pending'::text, 'processing'::text, 'pending_approval'::text, 'waiting_for_third_party_reply'::text, 'completed'::text, 'failed'::text, 'dead_letter'::text, 'stopped'::text])))
);


--
-- Name: thread_handoff_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.thread_handoff_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    company_id uuid NOT NULL,
    handoff_id uuid NOT NULL,
    generation uuid NOT NULL,
    command_id uuid NOT NULL,
    command_fingerprint text NOT NULL,
    operation text NOT NULL,
    actor_kind text NOT NULL,
    actor_principal_id uuid,
    from_state text,
    to_state text NOT NULL,
    from_version bigint NOT NULL,
    to_version bigint NOT NULL,
    previous_priority text,
    new_priority text,
    previous_due_at timestamp with time zone,
    new_due_at timestamp with time zone,
    previous_responsible_principal_id uuid,
    new_responsible_principal_id uuid,
    task_id uuid,
    draft_id uuid,
    draft_version integer,
    failure_reason text,
    occurred_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT thread_handoff_events_actor_check CHECK ((((actor_kind = 'system'::text) AND (actor_principal_id IS NULL)) OR ((actor_kind = ANY (ARRAY['human'::text, 'agent'::text])) AND (actor_principal_id IS NOT NULL)))),
    CONSTRAINT thread_handoff_events_failure_reason_check CHECK (((failure_reason IS NULL) OR ((btrim(failure_reason) <> ''::text) AND (octet_length(failure_reason) <= 2048)))),
    CONSTRAINT thread_handoff_events_operation_check CHECK ((operation = ANY (ARRAY['opened'::text, 'regenerated'::text, 'claimed'::text, 'released'::text, 'reassigned'::text, 'attributes_changed'::text, 'draft_requested'::text, 'draft_ready'::text, 'draft_failed'::text, 'resolved'::text, 'dismissed'::text]))),
    CONSTRAINT thread_handoff_events_priority_check CHECK ((((previous_priority IS NULL) OR (previous_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))) AND ((new_priority IS NULL) OR (new_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))))),
    CONSTRAINT thread_handoff_events_state_check CHECK ((((from_state IS NULL) OR (from_state = ANY (ARRAY['needs_instruction'::text, 'drafting'::text, 'draft_ready'::text, 'resolved'::text, 'dismissed'::text]))) AND (to_state = ANY (ARRAY['needs_instruction'::text, 'drafting'::text, 'draft_ready'::text, 'resolved'::text, 'dismissed'::text])))),
    CONSTRAINT thread_handoff_events_version_check CHECK (((from_version >= 0) AND (to_version = (from_version + 1))))
);


--
-- Name: thread_handoff_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.thread_handoff_runs (
    company_id uuid NOT NULL,
    task_id uuid NOT NULL,
    handoff_id uuid NOT NULL,
    generation uuid NOT NULL,
    requested_by_principal_id uuid NOT NULL,
    command_id uuid NOT NULL,
    draft_id uuid,
    draft_version integer,
    state text DEFAULT 'running'::text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT thread_handoff_runs_draft_check CHECK ((((draft_id IS NULL) AND (draft_version IS NULL)) OR ((draft_id IS NOT NULL) AND (draft_version > 0)))),
    CONSTRAINT thread_handoff_runs_state_check CHECK ((state = ANY (ARRAY['running'::text, 'drafted'::text, 'failed'::text, 'superseded'::text])))
);


--
-- Name: thread_handoffs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.thread_handoffs (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    generation uuid NOT NULL,
    state text DEFAULT 'needs_instruction'::text NOT NULL,
    source_message_id uuid NOT NULL,
    responsible_principal_id uuid,
    business_priority text DEFAULT 'normal'::text NOT NULL,
    business_due_at timestamp with time zone,
    version bigint DEFAULT 1 NOT NULL,
    generation_opened_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    closed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT thread_handoffs_closure_check CHECK ((((state = ANY (ARRAY['resolved'::text, 'dismissed'::text])) AND (closed_at IS NOT NULL)) OR ((state <> ALL (ARRAY['resolved'::text, 'dismissed'::text])) AND (closed_at IS NULL)))),
    CONSTRAINT thread_handoffs_priority_check CHECK ((business_priority = ANY (ARRAY['normal'::text, 'high'::text, 'urgent'::text]))),
    CONSTRAINT thread_handoffs_state_check CHECK ((state = ANY (ARRAY['needs_instruction'::text, 'drafting'::text, 'draft_ready'::text, 'resolved'::text, 'dismissed'::text]))),
    CONSTRAINT thread_handoffs_version_check CHECK ((version > 0))
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
    response_review_email_enabled boolean DEFAULT true CONSTRAINT user_notification_preferenc_response_review_email_enab_not_null NOT NULL,
    delegation_timeout_email_enabled boolean DEFAULT true CONSTRAINT user_notification_preferenc_delegation_timeout_email_e_not_null NOT NULL,
    task_failure_email_enabled boolean DEFAULT true CONSTRAINT user_notification_preferenc_task_failure_email_enabled_not_null NOT NULL,
    delivery_failure_email_enabled boolean DEFAULT true CONSTRAINT user_notification_preferenc_delivery_failure_email_ena_not_null NOT NULL
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
-- Name: workflow_action_actual_receipt_observations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_actual_receipt_observations (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid CONSTRAINT workflow_action_actual_receipt_observatio_execution_id_not_null NOT NULL,
    invocation_id uuid CONSTRAINT workflow_action_actual_receipt_observati_invocation_id_not_null NOT NULL,
    argument_digest text CONSTRAINT workflow_action_actual_receipt_observa_argument_digest_not_null NOT NULL,
    dispatch_id uuid CONSTRAINT workflow_action_actual_receipt_observation_dispatch_id_not_null NOT NULL,
    remote_entry_id uuid CONSTRAINT workflow_action_actual_receipt_observa_remote_entry_id_not_null NOT NULL,
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    result jsonb NOT NULL,
    result_digest text GENERATED ALWAYS AS (encode(sha256(jsonb_send(result)), 'hex'::text)) STORED,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_action_actual_receipt_observations_result_check CHECK ((octet_length((result)::text) <= 131072))
);


--
-- Name: workflow_action_claim_budget_refusals; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_claim_budget_refusals (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    job_id uuid NOT NULL,
    command_key text NOT NULL,
    retired_attempt integer NOT NULL,
    audit_sequence bigint NOT NULL
);


--
-- Name: workflow_action_claim_episodes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_claim_episodes (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    job_id uuid NOT NULL,
    command_key text NOT NULL,
    retired_attempt integer NOT NULL,
    CONSTRAINT workflow_action_claim_episodes_retired_attempt_check CHECK ((retired_attempt > 0))
);


--
-- Name: workflow_action_claim_retirement_witnesses; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_claim_retirement_witnesses (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid CONSTRAINT workflow_action_claim_retirement_witnesse_execution_id_not_null NOT NULL,
    job_id uuid NOT NULL,
    command_key text NOT NULL,
    retired_attempt integer CONSTRAINT workflow_action_claim_retirement_witne_retired_attempt_not_null NOT NULL,
    transaction_id xid8 CONSTRAINT workflow_action_claim_retirement_witnes_transaction_id_not_null NOT NULL,
    deadline timestamp with time zone NOT NULL,
    retirement_confirmed boolean DEFAULT false CONSTRAINT workflow_action_claim_retirement__retirement_confirmed_not_null NOT NULL,
    audit_sequence bigint
);


--
-- Name: workflow_action_dispatches; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_dispatches (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    id uuid NOT NULL,
    job_id uuid NOT NULL,
    attempt_number integer NOT NULL,
    execution_generation uuid NOT NULL,
    worker_id uuid NOT NULL,
    effect_kind text NOT NULL,
    current_policy jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    replay_subject bytea,
    CONSTRAINT workflow_action_dispatches_attempt_number_check CHECK ((attempt_number > 0)),
    CONSTRAINT workflow_action_dispatches_current_policy_check CHECK (((jsonb_typeof(current_policy) = 'object'::text) AND (octet_length((current_policy)::text) <= 524288))),
    CONSTRAINT workflow_action_dispatches_effect_kind_check CHECK ((effect_kind = ANY (ARRAY['local'::text, 'remote'::text, 'message'::text]))),
    CONSTRAINT workflow_action_dispatches_replay_subject_check CHECK (((octet_length(replay_subject) >= 1) AND (octet_length(replay_subject) <= 524288)))
);


--
-- Name: workflow_action_evidence; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_evidence (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    id uuid NOT NULL,
    actor_id uuid NOT NULL,
    command_id uuid NOT NULL,
    command_key text NOT NULL,
    request_digest text NOT NULL,
    coverage_digest text NOT NULL,
    disposition text NOT NULL,
    registration text,
    verifier_version text,
    provider text,
    authoritative_reference text,
    operation_signature text,
    observed_at timestamp with time zone NOT NULL,
    verified_at timestamp with time zone NOT NULL,
    valid_until timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    grant_eligible boolean DEFAULT false NOT NULL,
    diagnostic jsonb NOT NULL,
    applied_request text,
    applied_remote_entry_id uuid,
    delivery_ordinal smallint,
    delivery_id uuid,
    CONSTRAINT workflow_action_applied_request_shape CHECK ((((applied_request IS NULL) AND (applied_remote_entry_id IS NULL)) OR ((disposition = 'applied'::text) AND (delivery_id IS NULL) AND (((applied_request = 'remote_entry'::text) AND (applied_remote_entry_id IS NOT NULL)) OR ((applied_request = ANY (ARRAY['marker_reservation'::text, 'unattributed'::text])) AND (applied_remote_entry_id IS NULL)))) OR ((disposition = 'applied'::text) AND (applied_request = 'delivery'::text) AND (applied_remote_entry_id IS NULL) AND (delivery_id IS NOT NULL)))),
    CONSTRAINT workflow_action_evidence_applied_request_check CHECK ((applied_request = ANY (ARRAY['remote_entry'::text, 'marker_reservation'::text, 'unattributed'::text, 'delivery'::text]))),
    CONSTRAINT workflow_action_evidence_authoritative_reference_check CHECK ((authoritative_reference ~ '^[A-Za-z0-9][A-Za-z0-9_./:-]{0,127}$'::text)),
    CONSTRAINT workflow_action_evidence_check CHECK (((observed_at <= verified_at) AND (verified_at <= created_at))),
    CONSTRAINT workflow_action_evidence_check1 CHECK ((((registration IS NULL) AND (verifier_version IS NULL) AND (provider IS NULL) AND (authoritative_reference IS NULL) AND (operation_signature IS NULL) AND (disposition = 'unknown'::text) AND (valid_until = verified_at) AND (NOT grant_eligible)) OR ((registration IS NOT NULL) AND (verifier_version IS NOT NULL) AND (provider IS NOT NULL) AND (authoritative_reference IS NOT NULL) AND (operation_signature IS NOT NULL) AND (valid_until > verified_at) AND (valid_until <= (verified_at + '24:00:00'::interval))))),
    CONSTRAINT workflow_action_evidence_check2 CHECK (((NOT grant_eligible) OR (disposition = 'final_not_applied'::text))),
    CONSTRAINT workflow_action_evidence_command_key_check CHECK (((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128))),
    CONSTRAINT workflow_action_evidence_coverage_digest_check CHECK ((coverage_digest ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_action_evidence_delivery_shape CHECK (((delivery_ordinal IS NULL) = (delivery_id IS NULL))),
    CONSTRAINT workflow_action_evidence_diagnostic_check CHECK (((jsonb_typeof(diagnostic) = 'object'::text) AND (octet_length((diagnostic)::text) <= 16384))),
    CONSTRAINT workflow_action_evidence_disposition_check CHECK ((disposition = ANY (ARRAY['applied'::text, 'final_not_applied'::text, 'unknown'::text]))),
    CONSTRAINT workflow_action_evidence_operation_signature_check CHECK ((operation_signature ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_action_evidence_provider_check CHECK ((provider ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'::text)),
    CONSTRAINT workflow_action_evidence_registration_check CHECK ((registration ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'::text)),
    CONSTRAINT workflow_action_evidence_request_digest_check CHECK ((request_digest ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_action_evidence_verifier_version_check CHECK ((verifier_version ~ '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$'::text))
);


--
-- Name: workflow_action_intents; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_intents (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    id uuid NOT NULL,
    operation_key text NOT NULL,
    operation_digest text NOT NULL,
    argument_digest text NOT NULL,
    idempotency_key text NOT NULL,
    operation jsonb NOT NULL,
    policy_decision text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT workflow_action_intents_argument_digest_check CHECK ((argument_digest ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_action_intents_check CHECK ((idempotency_key = ('workflow-action:v1:'::text || operation_digest))),
    CONSTRAINT workflow_action_intents_operation_check CHECK (((jsonb_typeof(operation) = 'object'::text) AND (octet_length((operation)::text) <= 524288))),
    CONSTRAINT workflow_action_intents_operation_digest_check CHECK ((operation_digest ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_action_intents_operation_key_check CHECK (((octet_length(operation_key) >= 1) AND (octet_length(operation_key) <= 128))),
    CONSTRAINT workflow_action_intents_policy_decision_check CHECK ((policy_decision = ANY (ARRAY['unevaluated'::text, 'approval_required'::text])))
);


--
-- Name: workflow_action_receipts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_receipts (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    effect_kind text NOT NULL,
    result jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    remote_entry_id uuid,
    reconciliation_evidence_id uuid,
    CONSTRAINT workflow_action_receipt_kind CHECK ((((effect_kind = 'local'::text) AND (remote_entry_id IS NULL) AND (reconciliation_evidence_id IS NULL)) OR ((effect_kind = 'remote'::text) AND ((((remote_entry_id IS NOT NULL))::integer + ((reconciliation_evidence_id IS NOT NULL))::integer) = 1)) OR ((effect_kind = 'message'::text) AND (remote_entry_id IS NULL) AND (reconciliation_evidence_id IS NULL)))),
    CONSTRAINT workflow_action_receipts_result_check CHECK ((octet_length((result)::text) <= 131072))
);


--
-- Name: workflow_action_reconciliations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_reconciliations (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    effect_kind text NOT NULL,
    failure_code text NOT NULL,
    observed_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_action_reconciliations_effect_kind_check CHECK ((effect_kind = ANY (ARRAY['remote'::text, 'message'::text]))),
    CONSTRAINT workflow_action_reconciliations_failure_code_check CHECK ((((octet_length(failure_code) >= 1) AND (octet_length(failure_code) <= 128)) AND (failure_code ~ '^[A-Za-z_][A-Za-z0-9_-]*(\.[A-Za-z_][A-Za-z0-9_-]*)*$'::text)))
);


--
-- Name: workflow_message_actions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_message_actions (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    effect_kind text DEFAULT 'message'::text NOT NULL,
    origin text NOT NULL,
    wait_id uuid,
    fence_expires_at timestamp with time zone NOT NULL,
    destination_count smallint NOT NULL,
    state text DEFAULT 'pending'::text NOT NULL,
    consumed_through bigint DEFAULT 0 NOT NULL,
    consume_failures smallint DEFAULT 0 NOT NULL,
    consume_after timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_message_actions_check CHECK (((origin = 'step'::text) OR (wait_id IS NULL))),
    CONSTRAINT workflow_message_actions_check1 CHECK (((state <> 'stalled'::text) OR (consume_failures = 5))),
    CONSTRAINT workflow_message_actions_consume_failures_check CHECK (((consume_failures >= 0) AND (consume_failures <= 5))),
    CONSTRAINT workflow_message_actions_consumed_through_check CHECK ((consumed_through >= 0)),
    CONSTRAINT workflow_message_actions_destination_count_check CHECK (((destination_count >= 1) AND (destination_count <= 20))),
    CONSTRAINT workflow_message_actions_effect_kind_check CHECK ((effect_kind = 'message'::text)),
    CONSTRAINT workflow_message_actions_origin_check CHECK ((origin = ANY (ARRAY['step'::text, 'model_tool'::text]))),
    CONSTRAINT workflow_message_actions_state_check CHECK ((state = ANY (ARRAY['pending'::text, 'uncertain'::text, 'accepted'::text, 'failed'::text, 'stalled'::text])))
);


--
-- Name: workflow_action_effect_states; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.workflow_action_effect_states AS
 SELECT intent.company_id,
    intent.run_id,
    intent.execution_id,
    intent.id AS invocation_id,
    intent.argument_digest,
        CASE
            WHEN (receipt.invocation_id IS NOT NULL) THEN 'committed'::text
            WHEN (marker.effect_kind = 'message'::text) THEN
            CASE
                WHEN (message_action.state = 'pending'::text) THEN 'possible_dispatch'::text
                ELSE 'needs_reconciliation'::text
            END
            WHEN (EXISTS ( SELECT 1
               FROM public.workflow_action_evidence evidence
              WHERE ((evidence.company_id = intent.company_id) AND (evidence.invocation_id = intent.id) AND (evidence.disposition = 'applied'::text)))) THEN 'applied_without_result'::text
            WHEN public.workflow_action_has_evidence_conflict(intent.company_id, intent.execution_id) THEN 'needs_reconciliation'::text
            WHEN (public.workflow_action_not_applied_available(intent.company_id, intent.id) IS NOT NULL) THEN 'not_applied'::text
            WHEN ((reconciliation.invocation_id IS NOT NULL) OR (EXISTS ( SELECT 1
               FROM public.workflow_action_evidence evidence
              WHERE ((evidence.company_id = intent.company_id) AND (evidence.invocation_id = intent.id))))) THEN 'needs_reconciliation'::text
            WHEN (marker.invocation_id IS NOT NULL) THEN 'possible_dispatch'::text
            ELSE 'prepared'::text
        END AS effect_state,
    public.workflow_action_has_evidence_conflict(intent.company_id, intent.execution_id) AS evidence_conflict
   FROM ((((public.workflow_action_intents intent
     LEFT JOIN public.workflow_action_dispatches marker ON (((marker.company_id = intent.company_id) AND (marker.invocation_id = intent.id))))
     LEFT JOIN public.workflow_action_receipts receipt ON (((receipt.company_id = intent.company_id) AND (receipt.invocation_id = intent.id))))
     LEFT JOIN public.workflow_action_reconciliations reconciliation ON (((reconciliation.company_id = intent.company_id) AND (reconciliation.invocation_id = intent.id))))
     LEFT JOIN public.workflow_message_actions message_action ON (((message_action.company_id = intent.company_id) AND (message_action.invocation_id = intent.id))));


--
-- Name: workflow_action_evidence_commands; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_evidence_commands (
    company_id uuid NOT NULL,
    command_key text NOT NULL,
    id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    actor_id uuid NOT NULL,
    request_digest text NOT NULL,
    expected_revision bigint NOT NULL,
    result_revision bigint NOT NULL,
    outcome jsonb NOT NULL,
    evidence_id uuid,
    audit_sequence bigint,
    scheduled_job_id uuid,
    previous_state text,
    previous_waiting_reason text,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_action_evidence_commands_check CHECK ((((evidence_id IS NULL) AND (audit_sequence IS NULL) AND (scheduled_job_id IS NULL)) OR ((evidence_id IS NOT NULL) AND (audit_sequence IS NOT NULL)))),
    CONSTRAINT workflow_action_evidence_commands_check1 CHECK (((scheduled_job_id IS NULL) OR ((previous_state = 'waiting'::text) AND (previous_waiting_reason = 'reconciliation'::text) AND ((outcome ->> 'kind'::text) = 'scheduled'::text)))),
    CONSTRAINT workflow_action_evidence_commands_command_key_check CHECK (((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128))),
    CONSTRAINT workflow_action_evidence_commands_expected_revision_check CHECK ((expected_revision > 0)),
    CONSTRAINT workflow_action_evidence_commands_outcome_check CHECK (((jsonb_typeof(outcome) = 'object'::text) AND (octet_length((outcome)::text) <= 1024))),
    CONSTRAINT workflow_action_evidence_commands_previous_state_check CHECK ((previous_state = ANY (ARRAY['queued'::text, 'running'::text, 'waiting'::text, 'succeeded'::text, 'failed'::text, 'cancelled'::text]))),
    CONSTRAINT workflow_action_evidence_commands_request_digest_check CHECK ((request_digest ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_action_evidence_commands_result_revision_check CHECK ((result_revision > 0))
);


--
-- Name: workflow_action_evidence_conflicts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_evidence_conflicts (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    evidence_id uuid,
    contradictory_evidence_id uuid,
    remote_entry_id uuid,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    reason text DEFAULT 'finality_breach'::text NOT NULL,
    actual_observation_id uuid,
    CONSTRAINT workflow_action_conflict_nullable_evidence_shape CHECK (((evidence_id IS NOT NULL) OR ((actual_observation_id IS NOT NULL) AND (reason = 'actual_receipt_disagreement'::text)))),
    CONSTRAINT workflow_action_conflict_observation_shape CHECK (((actual_observation_id IS NULL) OR (remote_entry_id IS NOT NULL))),
    CONSTRAINT workflow_action_conflict_truth_anchor CHECK (((evidence_id IS NOT NULL) OR (actual_observation_id IS NOT NULL))),
    CONSTRAINT workflow_action_evidence_conflicts_check CHECK (((contradictory_evidence_id IS NOT NULL) OR (remote_entry_id IS NOT NULL))),
    CONSTRAINT workflow_action_evidence_conflicts_reason_check CHECK ((reason = ANY (ARRAY['finality_breach'::text, 'unattributed_applied'::text, 'evidence_disagreement'::text, 'actual_receipt_disagreement'::text])))
);


--
-- Name: workflow_action_evidence_consumptions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_evidence_consumptions (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    evidence_id uuid NOT NULL,
    remote_entry_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    reservation_xid xid8 DEFAULT pg_current_xact_id()
);


--
-- Name: workflow_action_evidence_coverage; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_evidence_coverage (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    evidence_id uuid NOT NULL,
    remote_entry_id uuid NOT NULL
);


--
-- Name: workflow_action_model_calls; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_model_calls (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    model_call_id text NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT workflow_action_model_calls_model_call_id_check CHECK (((octet_length(model_call_id) >= 1) AND (octet_length(model_call_id) <= 128)))
);


--
-- Name: workflow_action_remote_entries; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_remote_entries (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    argument_digest text NOT NULL,
    dispatch_id uuid NOT NULL,
    id uuid NOT NULL,
    job_id uuid NOT NULL,
    attempt_number integer NOT NULL,
    execution_generation uuid NOT NULL,
    worker_id uuid NOT NULL,
    effect_kind text DEFAULT 'remote'::text NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    reservation_xid xid8 DEFAULT pg_current_xact_id(),
    CONSTRAINT workflow_action_remote_entries_attempt_number_check CHECK ((attempt_number > 0)),
    CONSTRAINT workflow_action_remote_entries_effect_kind_check CHECK ((effect_kind = 'remote'::text))
);


--
-- Name: workflow_action_schedule_witnesses; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_schedule_witnesses (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    job_id uuid NOT NULL,
    retired_attempt integer NOT NULL,
    transaction_id xid8 NOT NULL,
    CONSTRAINT workflow_action_schedule_witnesses_retired_attempt_check CHECK ((retired_attempt > 0))
);


--
-- Name: workflow_action_state_witnesses; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_action_state_witnesses (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    transaction_id xid8 NOT NULL,
    initial_state text NOT NULL,
    initial_waiting_reason text
);


--
-- Name: workflow_admission_commands; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_admission_commands (
    company_id uuid NOT NULL,
    command_key text NOT NULL,
    run_id uuid NOT NULL,
    trigger_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT workflow_admission_commands_command_key_check CHECK (((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128)))
);


--
-- Name: workflow_admissions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_admissions (
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    source_key text NOT NULL,
    run_id uuid NOT NULL,
    CONSTRAINT workflow_admissions_source_key_check CHECK (((octet_length(source_key) >= 1) AND (octet_length(source_key) <= 512)))
);


--
-- Name: workflow_binding_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_binding_events (
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    state_revision bigint NOT NULL,
    configuration_revision bigint NOT NULL,
    event_kind text NOT NULL,
    actor_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT workflow_binding_events_configuration_revision_check CHECK ((configuration_revision > 0)),
    CONSTRAINT workflow_binding_events_event_kind_check CHECK ((event_kind = ANY (ARRAY['configured'::text, 'activated'::text, 'deactivated'::text]))),
    CONSTRAINT workflow_binding_events_state_revision_check CHECK ((state_revision > 0))
);


--
-- Name: workflow_binding_revisions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_binding_revisions (
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    revision bigint NOT NULL,
    version_id uuid NOT NULL,
    params jsonb NOT NULL,
    resources jsonb NOT NULL,
    actor_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT workflow_binding_revisions_params_check CHECK ((octet_length((params)::text) <= 4194304)),
    CONSTRAINT workflow_binding_revisions_resources_check CHECK (((jsonb_typeof(resources) = 'object'::text) AND (octet_length((resources)::text) <= 262144))),
    CONSTRAINT workflow_binding_revisions_revision_check CHECK ((revision > 0))
);


--
-- Name: workflow_bindings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_bindings (
    company_id uuid NOT NULL,
    id uuid NOT NULL,
    state_revision bigint NOT NULL,
    configuration_revision bigint NOT NULL,
    active boolean DEFAULT false NOT NULL,
    channel_id uuid,
    thread_id uuid,
    CONSTRAINT workflow_bindings_check CHECK (((thread_id IS NULL) OR (channel_id IS NOT NULL))),
    CONSTRAINT workflow_bindings_configuration_revision_check CHECK ((configuration_revision > 0)),
    CONSTRAINT workflow_bindings_state_revision_check CHECK ((state_revision > 0))
);


--
-- Name: workflow_budget_receipts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_budget_receipts (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    resource text NOT NULL,
    reservation_key text NOT NULL,
    quantity integer NOT NULL,
    disposition text NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_budget_receipts_check CHECK (((quantity > 0) AND (quantity <=
CASE
    WHEN (resource = 'activation'::text) THEN 100000
    ELSE 1000
END))),
    CONSTRAINT workflow_budget_receipts_check1 CHECK (((resource <> 'activation'::text) OR ((quantity = 1) AND (reservation_key = 'activation'::text)))),
    CONSTRAINT workflow_budget_receipts_disposition_check CHECK ((disposition = ANY (ARRAY['granted'::text, 'exhausted'::text]))),
    CONSTRAINT workflow_budget_receipts_reservation_key_check CHECK ((((octet_length(reservation_key) >= 1) AND (octet_length(reservation_key) <= 128)) AND (reservation_key ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]*$'::text))),
    CONSTRAINT workflow_budget_receipts_resource_check CHECK ((resource = ANY (ARRAY['activation'::text, 'model_call'::text, 'repetition'::text])))
);


--
-- Name: TABLE workflow_budget_receipts; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.workflow_budget_receipts IS 'Append-only accounting facts; never provider dispatch authority, execution ownership, or results.';


--
-- Name: workflow_capacity_policy; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_capacity_policy (
    singleton boolean DEFAULT true NOT NULL,
    global_limit integer NOT NULL,
    company_limit integer NOT NULL,
    CONSTRAINT workflow_capacity_policy_check CHECK (((company_limit >= 1) AND (company_limit < global_limit))),
    CONSTRAINT workflow_capacity_policy_global_limit_check CHECK (((global_limit >= 2) AND (global_limit <= 1024))),
    CONSTRAINT workflow_capacity_policy_singleton_check CHECK (singleton)
);


--
-- Name: workflow_control_commands; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_control_commands (
    company_id uuid NOT NULL,
    command_key text NOT NULL,
    run_id uuid NOT NULL,
    actor_id uuid NOT NULL,
    operation text NOT NULL,
    expected_revision bigint NOT NULL,
    result text NOT NULL,
    result_revision bigint NOT NULL,
    audit_sequence bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_control_commands_check CHECK (((result = 'applied'::text) = (audit_sequence IS NOT NULL))),
    CONSTRAINT workflow_control_commands_check1 CHECK ((((result <> 'terminal'::text) OR (operation = 'cancel'::text)) AND ((result <> 'unsafe'::text) OR (operation = 'retry'::text)))),
    CONSTRAINT workflow_control_commands_command_key_check CHECK (((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128))),
    CONSTRAINT workflow_control_commands_expected_revision_check CHECK ((expected_revision > 0)),
    CONSTRAINT workflow_control_commands_operation_check CHECK ((operation = ANY (ARRAY['cancel'::text, 'retry'::text]))),
    CONSTRAINT workflow_control_commands_result_check CHECK ((result = ANY (ARRAY['applied'::text, 'terminal'::text, 'unsafe'::text, 'conflict'::text]))),
    CONSTRAINT workflow_control_commands_result_revision_check CHECK ((result_revision > 0))
);


--
-- Name: workflow_definition_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_definition_events (
    company_id uuid NOT NULL,
    workflow_id uuid NOT NULL,
    id uuid NOT NULL,
    revision bigint NOT NULL,
    event_kind text NOT NULL,
    actor_id uuid NOT NULL,
    version_id uuid,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT workflow_definition_events_check CHECK (((event_kind = 'published'::text) = (version_id IS NOT NULL))),
    CONSTRAINT workflow_definition_events_event_kind_check CHECK ((event_kind = ANY (ARRAY['copied'::text, 'saved'::text, 'archived'::text, 'published'::text]))),
    CONSTRAINT workflow_definition_events_revision_check CHECK ((revision > 0))
);


--
-- Name: workflow_definitions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_definitions (
    company_id uuid NOT NULL,
    id uuid NOT NULL,
    revision bigint NOT NULL,
    title text NOT NULL,
    description text NOT NULL,
    source text NOT NULL,
    template_origin jsonb,
    archived boolean DEFAULT false NOT NULL,
    CONSTRAINT workflow_definitions_description_check CHECK ((octet_length(description) <= 4096)),
    CONSTRAINT workflow_definitions_revision_check CHECK ((revision > 0)),
    CONSTRAINT workflow_definitions_source_check CHECK ((octet_length(source) <= 262144)),
    CONSTRAINT workflow_definitions_template_origin_check CHECK (((template_origin IS NULL) OR ((jsonb_typeof(template_origin) = 'object'::text) AND (octet_length((template_origin)::text) <= 1024)))),
    CONSTRAINT workflow_definitions_title_check CHECK ((((octet_length(title) >= 1) AND (octet_length(title) <= 256)) AND (btrim(title) <> ''::text)))
);


--
-- Name: workflow_dispatch_demand; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_dispatch_demand (
    company_id uuid NOT NULL,
    ticket bigint NOT NULL,
    worker_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    job_id uuid NOT NULL,
    opportunity_until timestamp with time zone,
    registered_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL
);


--
-- Name: workflow_dispatch_demand_ticket_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.workflow_dispatch_demand ALTER COLUMN ticket ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.workflow_dispatch_demand_ticket_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: workflow_executions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_executions (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    id uuid NOT NULL,
    step_id text NOT NULL,
    activation bigint NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    activated_at timestamp with time zone,
    frozen_inputs jsonb,
    completed_at timestamp with time zone,
    committed_output jsonb,
    frozen_choice text,
    committed_route text,
    route_target text,
    successor_execution_id uuid,
    CONSTRAINT workflow_choice_shape CHECK (((frozen_choice IS NULL) OR ((activated_at IS NOT NULL) AND ((octet_length(frozen_choice) >= 1) AND (octet_length(frozen_choice) <= 128))))),
    CONSTRAINT workflow_execution_activation_shape CHECK ((((activated_at IS NULL) AND (frozen_inputs IS NULL)) OR ((activated_at IS NOT NULL) AND (frozen_inputs IS NOT NULL) AND (jsonb_typeof(frozen_inputs) = 'object'::text) AND (octet_length((frozen_inputs)::text) <= 1048576)))),
    CONSTRAINT workflow_execution_output_shape CHECK ((((completed_at IS NULL) AND (committed_output IS NULL)) OR ((completed_at IS NOT NULL) AND (committed_output IS NOT NULL) AND (activated_at IS NOT NULL) AND (completed_at >= activated_at) AND (octet_length((committed_output)::text) <= 1048576)))),
    CONSTRAINT workflow_executions_activation_check CHECK ((activation > 0)),
    CONSTRAINT workflow_executions_step_id_check CHECK (((octet_length(step_id) >= 1) AND (octet_length(step_id) <= 128))),
    CONSTRAINT workflow_route_shape CHECK ((((committed_route IS NULL) AND (route_target IS NULL) AND (successor_execution_id IS NULL)) OR ((completed_at IS NOT NULL) AND (committed_route IS NOT NULL) AND ((octet_length(committed_route) >= 1) AND (octet_length(committed_route) <= 140)) AND (route_target IS NOT NULL) AND (((route_target = '$end'::text) AND (successor_execution_id IS NULL)) OR ((route_target <> '$end'::text) AND ((octet_length(route_target) >= 1) AND (octet_length(route_target) <= 128)) AND (successor_execution_id IS NOT NULL))))))
);


--
-- Name: COLUMN workflow_executions.activation; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.workflow_executions.activation IS 'Positive run-wide logical ordinal. Allocate successors under the run lock; retries reuse it.';


--
-- Name: COLUMN workflow_executions.committed_output; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.workflow_executions.committed_output IS 'Execution-owned immutable committed result; written with progression by the runtime result owner.';


--
-- Name: workflow_history_members; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_history_members (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    message_id uuid NOT NULL,
    ordinal integer NOT NULL,
    CONSTRAINT workflow_history_members_ordinal_check CHECK (((ordinal >= 1) AND (ordinal <= 10000)))
);


--
-- Name: workflow_http_connections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_http_connections (
    company_id uuid NOT NULL,
    id uuid NOT NULL,
    origin text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    bearer boolean NOT NULL,
    revision bigint DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_http_connections_origin_check CHECK (((octet_length(origin) >= 8) AND (octet_length(origin) <= 2048))),
    CONSTRAINT workflow_http_connections_revision_check CHECK ((revision > 0))
);


--
-- Name: workflow_http_credentials; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_http_credentials (
    company_id uuid NOT NULL,
    connection_id uuid NOT NULL,
    envelope text NOT NULL,
    CONSTRAINT workflow_http_credentials_envelope_check CHECK ((((octet_length(envelope) >= 1) AND (octet_length(envelope) <= 16384)) AND (envelope ~~ 'enc:v2:%'::text)))
);


--
-- Name: workflow_http_operations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_http_operations (
    company_id uuid NOT NULL,
    connection_id uuid NOT NULL,
    name text NOT NULL,
    snapshot jsonb NOT NULL,
    approval_required boolean NOT NULL,
    CONSTRAINT workflow_http_operations_check CHECK (((snapshot ->> 'company_id'::text) = (company_id)::text)),
    CONSTRAINT workflow_http_operations_check1 CHECK ((((snapshot -> 'contract'::text) ->> 'name'::text) = name)),
    CONSTRAINT workflow_http_operations_name_check CHECK (((octet_length(name) >= 1) AND (octet_length(name) <= 128))),
    CONSTRAINT workflow_http_operations_snapshot_check CHECK (((octet_length((snapshot)::text) <= 65536) AND (jsonb_typeof(snapshot) = 'object'::text))),
    CONSTRAINT workflow_http_operations_snapshot_check1 CHECK ((((snapshot -> 'contract'::text) -> 'connection'::text) = 'null'::jsonb)),
    CONSTRAINT workflow_http_operations_snapshot_check2 CHECK ((((snapshot -> 'policy'::text) ->> 'recovery'::text) = 'reconcile'::text))
);


--
-- Name: workflow_mcp_operations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_mcp_operations (
    company_id uuid NOT NULL,
    connection_id uuid NOT NULL,
    name text NOT NULL,
    snapshot jsonb NOT NULL,
    approval_required boolean NOT NULL,
    version uuid NOT NULL,
    CONSTRAINT workflow_mcp_operations_check CHECK (((snapshot ->> 'company_id'::text) = (company_id)::text)),
    CONSTRAINT workflow_mcp_operations_check1 CHECK ((((snapshot -> 'contract'::text) ->> 'name'::text) = name)),
    CONSTRAINT workflow_mcp_operations_name_check CHECK (((octet_length(name) >= 1) AND (octet_length(name) <= 128))),
    CONSTRAINT workflow_mcp_operations_snapshot_check CHECK (((octet_length((snapshot)::text) <= 65536) AND (jsonb_typeof(snapshot) = 'object'::text))),
    CONSTRAINT workflow_mcp_operations_snapshot_check1 CHECK ((((snapshot -> 'contract'::text) -> 'connection'::text) = 'null'::jsonb)),
    CONSTRAINT workflow_mcp_operations_snapshot_check2 CHECK ((((snapshot -> 'policy'::text) ->> 'recovery'::text) = 'reconcile'::text))
);


--
-- Name: workflow_message_delivery_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_message_delivery_events (
    seq bigint NOT NULL,
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    ordinal smallint NOT NULL,
    delivery_id uuid NOT NULL,
    old_status text NOT NULL,
    status text NOT NULL,
    failure_class text,
    observed_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL
);


--
-- Name: workflow_message_delivery_events_seq_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.workflow_message_delivery_events ALTER COLUMN seq ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.workflow_message_delivery_events_seq_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: workflow_message_delivery_reopens; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_message_delivery_reopens (
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
    attempt_count integer NOT NULL,
    valid_until timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_message_delivery_reopens_attempt_count_check CHECK ((attempt_count >= 0))
);


--
-- Name: workflow_message_destinations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_message_destinations (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    invocation_id uuid NOT NULL,
    ordinal smallint NOT NULL,
    kind text NOT NULL,
    destination_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    message_id uuid NOT NULL,
    delivery_id uuid,
    principal_id uuid,
    recipient_address text,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_message_destinations_check CHECK ((((kind = 'channel'::text) AND (delivery_id IS NULL) AND (principal_id IS NULL) AND (recipient_address IS NULL)) OR ((kind = 'user'::text) AND (delivery_id IS NOT NULL) AND (principal_id IS NOT NULL) AND (recipient_address IS NOT NULL)) OR ((kind = 'reply'::text) AND (principal_id IS NULL) AND ((delivery_id IS NULL) = (recipient_address IS NULL))))),
    CONSTRAINT workflow_message_destinations_kind_check CHECK ((kind = ANY (ARRAY['channel'::text, 'user'::text, 'reply'::text]))),
    CONSTRAINT workflow_message_destinations_ordinal_check CHECK (((ordinal >= 0) AND (ordinal <= 19))),
    CONSTRAINT workflow_message_destinations_recipient_address_check CHECK (((recipient_address IS NULL) OR ((btrim(recipient_address) <> ''::text) AND (octet_length(recipient_address) <= 998))))
);


--
-- Name: workflow_poll_companies; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_poll_companies (
    worker_id uuid NOT NULL,
    company_id uuid NOT NULL,
    after_job uuid,
    horizon timestamp with time zone DEFAULT clock_timestamp() NOT NULL
);


--
-- Name: workflow_poll_workers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_poll_workers (
    worker_id uuid NOT NULL,
    after_company uuid,
    touched_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL
);


--
-- Name: workflow_root_budget_usage; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_root_budget_usage (
    company_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    activations bigint DEFAULT 0 NOT NULL,
    model_calls bigint DEFAULT 0 NOT NULL,
    repetitions bigint DEFAULT 0 NOT NULL,
    CONSTRAINT workflow_root_budget_usage_activations_check CHECK ((activations >= 0)),
    CONSTRAINT workflow_root_budget_usage_model_calls_check CHECK ((model_calls >= 0)),
    CONSTRAINT workflow_root_budget_usage_repetitions_check CHECK ((repetitions >= 0))
);


--
-- Name: workflow_root_budgets; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_root_budgets (
    company_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    provenance text NOT NULL,
    activations integer,
    model_calls integer,
    repetitions integer,
    CONSTRAINT workflow_root_budgets_check CHECK ((((provenance = 'sealed_legacy'::text) AND (activations IS NULL) AND (model_calls IS NULL) AND (repetitions IS NULL)) OR ((provenance = 'frozen_v2'::text) AND (activations IS NOT NULL) AND ((activations >= 1) AND (activations <= 100000)) AND (model_calls IS NOT NULL) AND ((model_calls >= 1) AND (model_calls <= 1000)) AND (repetitions IS NOT NULL) AND ((repetitions >= 1) AND (repetitions <= 1000))))),
    CONSTRAINT workflow_root_budgets_provenance_check CHECK ((provenance = ANY (ARRAY['frozen_v2'::text, 'sealed_legacy'::text])))
);


--
-- Name: workflow_run_budgets; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_run_budgets (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    parent_run_id uuid,
    CONSTRAINT workflow_run_budgets_check CHECK ((((parent_run_id IS NULL) AND (run_id = root_run_id)) OR ((parent_run_id IS NOT NULL) AND (run_id <> root_run_id) AND (run_id <> parent_run_id))))
);


--
-- Name: workflow_run_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_run_events (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    sequence bigint NOT NULL,
    event_kind text NOT NULL,
    actor_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    execution_id uuid,
    terminal_state text,
    terminal_execution_id uuid,
    CONSTRAINT workflow_action_audit_execution CHECK (((event_kind <> 'action_reconciled'::text) OR (execution_id IS NOT NULL))),
    CONSTRAINT workflow_run_events_event_kind_check CHECK (((octet_length(event_kind) >= 1) AND (octet_length(event_kind) <= 128))),
    CONSTRAINT workflow_run_events_sequence_check CHECK ((sequence > 0)),
    CONSTRAINT workflow_wakeup_shape CHECK ((((event_kind = 'parent_wakeup'::text) AND (terminal_state IS NOT NULL) AND (terminal_state = ANY (ARRAY['succeeded'::text, 'failed'::text, 'cancelled'::text])) AND (execution_id IS NULL)) OR ((event_kind <> 'parent_wakeup'::text) AND (terminal_state IS NULL) AND (terminal_execution_id IS NULL))))
);


--
-- Name: COLUMN workflow_run_events.terminal_state; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.workflow_run_events.terminal_state IS 'Immutable child terminal transition fact. Parent owner consumes in its own run-first transaction.';


--
-- Name: workflow_run_parents; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_run_parents (
    company_id uuid NOT NULL,
    child_run_id uuid NOT NULL,
    parent_run_id uuid NOT NULL,
    parent_execution_id uuid NOT NULL
);


--
-- Name: TABLE workflow_run_parents; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.workflow_run_parents IS 'Immutable child-owned lineage identity; parent FK is checked at admission, never by run progression.';


--
-- Name: workflow_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_runs (
    company_id uuid NOT NULL,
    id uuid NOT NULL,
    binding_id uuid NOT NULL,
    binding_revision bigint NOT NULL,
    workflow_id uuid NOT NULL,
    version_id uuid NOT NULL,
    channel_id uuid,
    thread_id uuid,
    actor_id uuid NOT NULL,
    trigger_id uuid NOT NULL,
    correlation_id uuid NOT NULL,
    bundle bytea NOT NULL,
    input jsonb NOT NULL,
    params jsonb NOT NULL,
    resources jsonb NOT NULL,
    max_steps integer NOT NULL,
    max_context_bytes integer NOT NULL,
    deadline timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    state text DEFAULT 'queued'::text NOT NULL,
    waiting_reason text,
    terminal_execution_id uuid,
    parent_run_id uuid,
    parent_execution_id uuid,
    terminal_wakeup_sequence bigint,
    revision bigint DEFAULT 1 NOT NULL,
    CONSTRAINT workflow_parent_shape CHECK ((((parent_run_id IS NULL) AND (parent_execution_id IS NULL)) OR ((parent_run_id IS NOT NULL) AND (parent_execution_id IS NOT NULL) AND (parent_run_id <> id)))),
    CONSTRAINT workflow_run_state CHECK ((state = ANY (ARRAY['queued'::text, 'running'::text, 'waiting'::text, 'succeeded'::text, 'failed'::text, 'cancelled'::text]))),
    CONSTRAINT workflow_run_waiting_shape CHECK ((((state = 'waiting'::text) AND (waiting_reason IS NOT NULL) AND (waiting_reason = ANY (ARRAY['decision'::text, 'event'::text, 'timer'::text, 'child_run'::text, 'effect'::text, 'reconciliation'::text]))) OR ((state <> 'waiting'::text) AND (waiting_reason IS NULL)))),
    CONSTRAINT workflow_runs_binding_revision_check CHECK ((binding_revision > 0)),
    CONSTRAINT workflow_runs_bundle_check CHECK (((octet_length(bundle) >= 1) AND (octet_length(bundle) <= 4194304))),
    CONSTRAINT workflow_runs_check CHECK (((thread_id IS NULL) OR (channel_id IS NOT NULL))),
    CONSTRAINT workflow_runs_check1 CHECK ((deadline > created_at)),
    CONSTRAINT workflow_runs_input_check CHECK ((octet_length((input)::text) <= 4194304)),
    CONSTRAINT workflow_runs_max_context_bytes_check CHECK (((max_context_bytes >= 1) AND (max_context_bytes <= 1048576))),
    CONSTRAINT workflow_runs_max_steps_check CHECK (((max_steps >= 1) AND (max_steps <= 100000))),
    CONSTRAINT workflow_runs_params_check CHECK ((octet_length((params)::text) <= 4194304)),
    CONSTRAINT workflow_runs_resources_check CHECK (((jsonb_typeof(resources) = 'object'::text) AND (octet_length((resources)::text) <= 262144))),
    CONSTRAINT workflow_runs_revision_check CHECK ((revision > 0)),
    CONSTRAINT workflow_runs_terminal_wakeup_sequence_check CHECK ((terminal_wakeup_sequence > 0))
);


--
-- Name: workflow_versions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_versions (
    company_id uuid NOT NULL,
    workflow_id uuid NOT NULL,
    id uuid NOT NULL,
    draft_revision bigint NOT NULL,
    command_key text NOT NULL,
    actor_id uuid NOT NULL,
    title text NOT NULL,
    description text NOT NULL,
    source text NOT NULL,
    template_origin jsonb,
    bundle bytea NOT NULL,
    content_hash text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT workflow_versions_bundle_check CHECK (((octet_length(bundle) >= 1) AND (octet_length(bundle) <= 4194304))),
    CONSTRAINT workflow_versions_command_key_check CHECK (((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128))),
    CONSTRAINT workflow_versions_content_hash_check CHECK ((content_hash ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_versions_description_check CHECK ((octet_length(description) <= 4096)),
    CONSTRAINT workflow_versions_draft_revision_check CHECK ((draft_revision > 0)),
    CONSTRAINT workflow_versions_source_check CHECK ((octet_length(source) <= 262144)),
    CONSTRAINT workflow_versions_template_origin_check CHECK (((template_origin IS NULL) OR ((jsonb_typeof(template_origin) = 'object'::text) AND (octet_length((template_origin)::text) <= 1024)))),
    CONSTRAINT workflow_versions_title_check CHECK (((octet_length(title) >= 1) AND (octet_length(title) <= 256)))
);


--
-- Name: workflow_wait_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_wait_events (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    id uuid NOT NULL,
    event_name text NOT NULL,
    correlation text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_wait_events_correlation_check CHECK (((octet_length(correlation) >= 1) AND (octet_length(correlation) <= 256))),
    CONSTRAINT workflow_wait_events_event_name_check CHECK (((octet_length(event_name) >= 1) AND (octet_length(event_name) <= 128))),
    CONSTRAINT workflow_wait_events_payload_check CHECK ((octet_length((payload)::text) <= 1048576))
);


--
-- Name: workflow_waits; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_waits (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    id uuid NOT NULL,
    reason text NOT NULL,
    deadline timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    state text DEFAULT 'waiting'::text NOT NULL,
    event_name text,
    correlation text,
    notification_intent jsonb,
    settled_at timestamp with time zone,
    consumed_event_id uuid,
    failure_code text,
    CONSTRAINT workflow_wait_correlation CHECK (((correlation IS NULL) OR ((octet_length(correlation) >= 1) AND (octet_length(correlation) <= 256)))),
    CONSTRAINT workflow_wait_deadline CHECK (((reason = 'timer'::text) OR (deadline > created_at))),
    CONSTRAINT workflow_wait_event_name CHECK (((event_name IS NULL) OR ((octet_length(event_name) >= 1) AND (octet_length(event_name) <= 128)))),
    CONSTRAINT workflow_wait_failure CHECK ((((state = 'failed'::text) = (failure_code IS NOT NULL)) AND ((failure_code IS NULL) OR (failure_code = ANY (ARRAY['activation_limit'::text, 'invalid_output'::text, 'evidence_conflict'::text]))))),
    CONSTRAINT workflow_wait_intent CHECK (((notification_intent IS NULL) OR ((jsonb_typeof(notification_intent) = 'object'::text) AND ((notification_intent ->> 'version'::text) = '1'::text) AND ((notification_intent ->> 'wait_id'::text) = (id)::text) AND (octet_length((notification_intent)::text) <= 2048)))),
    CONSTRAINT workflow_wait_settlement CHECK (((state = 'waiting'::text) = (settled_at IS NULL))),
    CONSTRAINT workflow_wait_state CHECK ((state = ANY (ARRAY['waiting'::text, 'completed'::text, 'expired'::text, 'failed'::text, 'cancelled'::text]))),
    CONSTRAINT workflow_waits_reason_check CHECK ((reason = ANY (ARRAY['decision'::text, 'event'::text, 'timer'::text, 'child_run'::text, 'effect'::text, 'reconciliation'::text])))
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
-- Name: background_tasks background_tasks_company_source_message_channel_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_company_source_message_channel_key UNIQUE (company_id, source_message_uuid, channel_id);


--
-- Name: CONSTRAINT background_tasks_company_source_message_channel_key ON background_tasks; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON CONSTRAINT background_tasks_company_source_message_channel_key ON public.background_tasks IS 'One source message causes at most one task per channel, where the channel is the first of one addressed pipeline.';


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
-- Name: background_tasks background_tasks_schedule_scope_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_schedule_scope_key UNIQUE (company_id, channel_id, thread_id, id);


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
-- Name: channel_schedules channel_schedules_company_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_schedules
    ADD CONSTRAINT channel_schedules_company_id_key UNIQUE (company_id, id);


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
-- Name: message_deliveries message_deliveries_company_message_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_company_message_key UNIQUE (company_id, id, message_id);


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
-- Name: thread_handoff_events thread_handoff_events_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoff_events
    ADD CONSTRAINT thread_handoff_events_command_key UNIQUE (company_id, handoff_id, command_id);


--
-- Name: thread_handoff_events thread_handoff_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoff_events
    ADD CONSTRAINT thread_handoff_events_pkey PRIMARY KEY (id);


--
-- Name: thread_handoff_runs thread_handoff_runs_generation_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoff_runs
    ADD CONSTRAINT thread_handoff_runs_generation_key UNIQUE (company_id, generation);


--
-- Name: thread_handoff_runs thread_handoff_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoff_runs
    ADD CONSTRAINT thread_handoff_runs_pkey PRIMARY KEY (company_id, task_id);


--
-- Name: thread_handoffs thread_handoffs_company_id_generation_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_company_id_generation_key UNIQUE (company_id, generation);


--
-- Name: thread_handoffs thread_handoffs_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_company_id_id_key UNIQUE (company_id, id);


--
-- Name: thread_handoffs thread_handoffs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_pkey PRIMARY KEY (id);


--
-- Name: thread_handoffs thread_handoffs_thread_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_thread_key UNIQUE (company_id, thread_id);


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
-- Name: thread_messages thread_messages_workflow_history_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_messages
    ADD CONSTRAINT thread_messages_workflow_history_key UNIQUE (company_id, channel_id, thread_id, message_id);


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
-- Name: workflow_action_actual_receipt_observations workflow_action_actual_receip_company_id_remote_entry_id_re_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_actual_receipt_observations
    ADD CONSTRAINT workflow_action_actual_receip_company_id_remote_entry_id_re_key UNIQUE (company_id, remote_entry_id, result_digest);


--
-- Name: workflow_action_actual_receipt_observations workflow_action_actual_receip_company_id_run_id_execution_i_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_actual_receipt_observations
    ADD CONSTRAINT workflow_action_actual_receip_company_id_run_id_execution_i_key UNIQUE (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id, id);


--
-- Name: workflow_action_actual_receipt_observations workflow_action_actual_receipt_observations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_actual_receipt_observations
    ADD CONSTRAINT workflow_action_actual_receipt_observations_pkey PRIMARY KEY (company_id, id);


--
-- Name: task_attempts workflow_action_attempt_fence_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_attempts
    ADD CONSTRAINT workflow_action_attempt_fence_key UNIQUE (task_id, attempt_number, execution_generation, worker_id);


--
-- Name: workflow_action_claim_budget_refusals workflow_action_claim_budget_refusals_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_budget_refusals
    ADD CONSTRAINT workflow_action_claim_budget_refusals_pkey PRIMARY KEY (company_id, command_key);


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episode_company_id_job_id_retired_att_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_episodes
    ADD CONSTRAINT workflow_action_claim_episode_company_id_job_id_retired_att_key UNIQUE (company_id, job_id, retired_attempt);


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episode_company_id_run_id_execution_i_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_episodes
    ADD CONSTRAINT workflow_action_claim_episode_company_id_run_id_execution_i_key UNIQUE (company_id, run_id, execution_id, job_id, command_key, retired_attempt);


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episodes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_episodes
    ADD CONSTRAINT workflow_action_claim_episodes_pkey PRIMARY KEY (company_id, command_key);


--
-- Name: workflow_action_claim_retirement_witnesses workflow_action_claim_retirement_witnesses_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_retirement_witnesses
    ADD CONSTRAINT workflow_action_claim_retirement_witnesses_pkey PRIMARY KEY (company_id, command_key, transaction_id);


--
-- Name: workflow_action_evidence_commands workflow_action_command_audit_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_command_audit_unique UNIQUE (company_id, run_id, audit_sequence);


--
-- Name: workflow_action_evidence_commands workflow_action_command_episode_scope; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_command_episode_scope UNIQUE (company_id, run_id, execution_id, scheduled_job_id, command_key);


--
-- Name: workflow_action_dispatches workflow_action_dispatches_company_id_run_id_execution_id_i_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_dispatches
    ADD CONSTRAINT workflow_action_dispatches_company_id_run_id_execution_id_i_key UNIQUE (company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind);


--
-- Name: workflow_action_dispatches workflow_action_dispatches_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_dispatches
    ADD CONSTRAINT workflow_action_dispatches_pkey PRIMARY KEY (company_id, invocation_id);


--
-- Name: workflow_action_evidence_commands workflow_action_evidence_comm_company_id_command_key_id_req_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_evidence_comm_company_id_command_key_id_req_key UNIQUE (company_id, command_key, id, request_digest);


--
-- Name: workflow_action_evidence_commands workflow_action_evidence_commands_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_evidence_commands_pkey PRIMARY KEY (company_id, command_key);


--
-- Name: workflow_action_evidence workflow_action_evidence_company_id_command_key_command_id__key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence
    ADD CONSTRAINT workflow_action_evidence_company_id_command_key_command_id__key UNIQUE (company_id, command_key, command_id, request_digest, id);


--
-- Name: workflow_action_evidence workflow_action_evidence_company_id_run_id_execution_id_inv_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence
    ADD CONSTRAINT workflow_action_evidence_company_id_run_id_execution_id_inv_key UNIQUE (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id);


--
-- Name: workflow_action_evidence_conflicts workflow_action_evidence_conflicts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_conflicts
    ADD CONSTRAINT workflow_action_evidence_conflicts_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_action_evidence_consumptions workflow_action_evidence_consump_company_id_remote_entry_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_consumptions
    ADD CONSTRAINT workflow_action_evidence_consump_company_id_remote_entry_id_key UNIQUE (company_id, remote_entry_id);


--
-- Name: workflow_action_evidence_consumptions workflow_action_evidence_consumptions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_consumptions
    ADD CONSTRAINT workflow_action_evidence_consumptions_pkey PRIMARY KEY (company_id, evidence_id);


--
-- Name: workflow_action_evidence_coverage workflow_action_evidence_coverage_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_coverage
    ADD CONSTRAINT workflow_action_evidence_coverage_pkey PRIMARY KEY (company_id, evidence_id, remote_entry_id);


--
-- Name: workflow_action_evidence workflow_action_evidence_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence
    ADD CONSTRAINT workflow_action_evidence_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_action_intents workflow_action_intents_company_id_idempotency_key_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_intents
    ADD CONSTRAINT workflow_action_intents_company_id_idempotency_key_key UNIQUE (company_id, idempotency_key);


--
-- Name: workflow_action_intents workflow_action_intents_company_id_run_id_execution_id_id_a_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_intents
    ADD CONSTRAINT workflow_action_intents_company_id_run_id_execution_id_id_a_key UNIQUE (company_id, run_id, execution_id, id, argument_digest);


--
-- Name: workflow_action_intents workflow_action_intents_company_id_run_id_execution_id_oper_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_intents
    ADD CONSTRAINT workflow_action_intents_company_id_run_id_execution_id_oper_key UNIQUE (company_id, run_id, execution_id, operation_digest);


--
-- Name: workflow_action_intents workflow_action_intents_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_intents
    ADD CONSTRAINT workflow_action_intents_pkey PRIMARY KEY (company_id, id);


--
-- Name: background_tasks workflow_action_job_scope_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT workflow_action_job_scope_key UNIQUE (company_id, workflow_execution_id, id);


--
-- Name: workflow_action_dispatches workflow_action_marker_evidence_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_dispatches
    ADD CONSTRAINT workflow_action_marker_evidence_key UNIQUE (company_id, run_id, execution_id, invocation_id, argument_digest, id);


--
-- Name: workflow_action_model_calls workflow_action_model_calls_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_model_calls
    ADD CONSTRAINT workflow_action_model_calls_pkey PRIMARY KEY (company_id, run_id, execution_id, model_call_id);


--
-- Name: workflow_action_receipts workflow_action_receipts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_receipts
    ADD CONSTRAINT workflow_action_receipts_pkey PRIMARY KEY (company_id, invocation_id);


--
-- Name: workflow_action_reconciliations workflow_action_reconciliations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_reconciliations
    ADD CONSTRAINT workflow_action_reconciliations_pkey PRIMARY KEY (company_id, invocation_id);


--
-- Name: workflow_action_remote_entries workflow_action_remote_entrie_company_id_invocation_id_job__key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_remote_entries
    ADD CONSTRAINT workflow_action_remote_entrie_company_id_invocation_id_job__key UNIQUE (company_id, invocation_id, job_id, attempt_number);


--
-- Name: workflow_action_remote_entries workflow_action_remote_entrie_company_id_run_id_execution_i_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_remote_entries
    ADD CONSTRAINT workflow_action_remote_entrie_company_id_run_id_execution_i_key UNIQUE (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id);


--
-- Name: workflow_action_remote_entries workflow_action_remote_entries_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_remote_entries
    ADD CONSTRAINT workflow_action_remote_entries_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_action_schedule_witnesses workflow_action_schedule_witnesses_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_schedule_witnesses
    ADD CONSTRAINT workflow_action_schedule_witnesses_pkey PRIMARY KEY (company_id, job_id, retired_attempt, transaction_id);


--
-- Name: workflow_action_state_witnesses workflow_action_state_witnesses_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_state_witnesses
    ADD CONSTRAINT workflow_action_state_witnesses_pkey PRIMARY KEY (company_id, run_id, transaction_id);


--
-- Name: workflow_admission_commands workflow_admission_commands_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_admission_commands
    ADD CONSTRAINT workflow_admission_commands_pkey PRIMARY KEY (company_id, command_key);


--
-- Name: workflow_admissions workflow_admissions_company_id_run_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_admissions
    ADD CONSTRAINT workflow_admissions_company_id_run_id_key UNIQUE (company_id, run_id);


--
-- Name: workflow_admissions workflow_admissions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_admissions
    ADD CONSTRAINT workflow_admissions_pkey PRIMARY KEY (company_id, binding_id, source_key);


--
-- Name: workflow_binding_events workflow_binding_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_binding_events
    ADD CONSTRAINT workflow_binding_events_pkey PRIMARY KEY (company_id, binding_id, state_revision);


--
-- Name: workflow_binding_revisions workflow_binding_revisions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_binding_revisions
    ADD CONSTRAINT workflow_binding_revisions_pkey PRIMARY KEY (company_id, binding_id, revision);


--
-- Name: workflow_bindings workflow_bindings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_bindings
    ADD CONSTRAINT workflow_bindings_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_budget_receipts workflow_budget_receipts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_budget_receipts
    ADD CONSTRAINT workflow_budget_receipts_pkey PRIMARY KEY (company_id, run_id, execution_id, resource, reservation_key);


--
-- Name: workflow_capacity_policy workflow_capacity_policy_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_capacity_policy
    ADD CONSTRAINT workflow_capacity_policy_pkey PRIMARY KEY (singleton);


--
-- Name: workflow_control_commands workflow_control_commands_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_control_commands
    ADD CONSTRAINT workflow_control_commands_pkey PRIMARY KEY (company_id, command_key);


--
-- Name: workflow_definition_events workflow_definition_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_definition_events
    ADD CONSTRAINT workflow_definition_events_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_definitions workflow_definitions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_definitions
    ADD CONSTRAINT workflow_definitions_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_dispatch_demand workflow_dispatch_demand_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_dispatch_demand
    ADD CONSTRAINT workflow_dispatch_demand_pkey PRIMARY KEY (company_id);


--
-- Name: workflow_dispatch_demand workflow_dispatch_demand_ticket_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_dispatch_demand
    ADD CONSTRAINT workflow_dispatch_demand_ticket_key UNIQUE (ticket);


--
-- Name: workflow_dispatch_demand workflow_dispatch_demand_worker_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_dispatch_demand
    ADD CONSTRAINT workflow_dispatch_demand_worker_id_key UNIQUE (worker_id);


--
-- Name: workflow_executions workflow_execution_run_ordinal; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_executions
    ADD CONSTRAINT workflow_execution_run_ordinal UNIQUE (company_id, run_id, activation);


--
-- Name: workflow_executions workflow_executions_company_id_run_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_executions
    ADD CONSTRAINT workflow_executions_company_id_run_id_id_key UNIQUE (company_id, run_id, id);


--
-- Name: workflow_executions workflow_executions_company_id_run_id_step_id_activation_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_executions
    ADD CONSTRAINT workflow_executions_company_id_run_id_step_id_activation_key UNIQUE (company_id, run_id, step_id, activation);


--
-- Name: workflow_executions workflow_executions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_executions
    ADD CONSTRAINT workflow_executions_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_history_members workflow_history_members_company_id_run_id_ordinal_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_history_members
    ADD CONSTRAINT workflow_history_members_company_id_run_id_ordinal_key UNIQUE (company_id, run_id, ordinal);


--
-- Name: workflow_history_members workflow_history_members_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_history_members
    ADD CONSTRAINT workflow_history_members_pkey PRIMARY KEY (company_id, run_id, message_id);


--
-- Name: workflow_http_connections workflow_http_connections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_http_connections
    ADD CONSTRAINT workflow_http_connections_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_http_credentials workflow_http_credentials_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_http_credentials
    ADD CONSTRAINT workflow_http_credentials_pkey PRIMARY KEY (company_id, connection_id);


--
-- Name: workflow_http_operations workflow_http_operations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_http_operations
    ADD CONSTRAINT workflow_http_operations_pkey PRIMARY KEY (company_id, connection_id, name);


--
-- Name: workflow_mcp_operations workflow_mcp_operations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_mcp_operations
    ADD CONSTRAINT workflow_mcp_operations_pkey PRIMARY KEY (company_id, connection_id, name);


--
-- Name: workflow_message_actions workflow_message_actions_company_id_run_id_execution_id_inv_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_actions
    ADD CONSTRAINT workflow_message_actions_company_id_run_id_execution_id_inv_key UNIQUE (company_id, run_id, execution_id, invocation_id);


--
-- Name: workflow_message_actions workflow_message_actions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_actions
    ADD CONSTRAINT workflow_message_actions_pkey PRIMARY KEY (company_id, invocation_id);


--
-- Name: workflow_message_delivery_events workflow_message_delivery_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_delivery_events
    ADD CONSTRAINT workflow_message_delivery_events_pkey PRIMARY KEY (seq);


--
-- Name: workflow_message_delivery_reopens workflow_message_delivery_reopens_company_id_evidence_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_delivery_reopens
    ADD CONSTRAINT workflow_message_delivery_reopens_company_id_evidence_id_key UNIQUE (company_id, evidence_id);


--
-- Name: workflow_message_delivery_reopens workflow_message_delivery_reopens_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_delivery_reopens
    ADD CONSTRAINT workflow_message_delivery_reopens_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_message_destinations workflow_message_destinations_company_id_delivery_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destinations_company_id_delivery_id_key UNIQUE (company_id, delivery_id);


--
-- Name: workflow_message_destinations workflow_message_destinations_company_id_invocation_id_kind_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destinations_company_id_invocation_id_kind_key UNIQUE (company_id, invocation_id, kind, destination_id);


--
-- Name: workflow_message_destinations workflow_message_destinations_company_id_invocation_id_ordi_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destinations_company_id_invocation_id_ordi_key UNIQUE (company_id, invocation_id, ordinal, delivery_id);


--
-- Name: workflow_message_destinations workflow_message_destinations_company_id_message_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destinations_company_id_message_id_key UNIQUE (company_id, message_id);


--
-- Name: workflow_message_destinations workflow_message_destinations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destinations_pkey PRIMARY KEY (company_id, invocation_id, ordinal);


--
-- Name: workflow_executions workflow_one_predecessor; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_executions
    ADD CONSTRAINT workflow_one_predecessor UNIQUE (company_id, run_id, successor_execution_id);


--
-- Name: workflow_poll_companies workflow_poll_companies_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_poll_companies
    ADD CONSTRAINT workflow_poll_companies_pkey PRIMARY KEY (worker_id, company_id);


--
-- Name: workflow_poll_workers workflow_poll_workers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_poll_workers
    ADD CONSTRAINT workflow_poll_workers_pkey PRIMARY KEY (worker_id);


--
-- Name: workflow_root_budget_usage workflow_root_budget_usage_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_root_budget_usage
    ADD CONSTRAINT workflow_root_budget_usage_pkey PRIMARY KEY (company_id, root_run_id);


--
-- Name: workflow_root_budgets workflow_root_budgets_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_root_budgets
    ADD CONSTRAINT workflow_root_budgets_pkey PRIMARY KEY (company_id, root_run_id);


--
-- Name: workflow_runs workflow_run_budget_parent_identity; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_run_budget_parent_identity UNIQUE (company_id, id, parent_run_id);


--
-- Name: workflow_run_budgets workflow_run_budgets_company_id_run_id_root_run_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_budgets
    ADD CONSTRAINT workflow_run_budgets_company_id_run_id_root_run_id_key UNIQUE (company_id, run_id, root_run_id);


--
-- Name: workflow_run_budgets workflow_run_budgets_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_budgets
    ADD CONSTRAINT workflow_run_budgets_pkey PRIMARY KEY (company_id, run_id);


--
-- Name: workflow_run_events workflow_run_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_events
    ADD CONSTRAINT workflow_run_events_pkey PRIMARY KEY (company_id, run_id, sequence);


--
-- Name: workflow_runs workflow_run_parent_identity; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_run_parent_identity UNIQUE (company_id, id, parent_run_id, parent_execution_id);


--
-- Name: workflow_run_parents workflow_run_parents_company_id_child_run_id_parent_run_id__key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_parents
    ADD CONSTRAINT workflow_run_parents_company_id_child_run_id_parent_run_id__key UNIQUE (company_id, child_run_id, parent_run_id, parent_execution_id);


--
-- Name: workflow_run_parents workflow_run_parents_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_parents
    ADD CONSTRAINT workflow_run_parents_pkey PRIMARY KEY (company_id, child_run_id);


--
-- Name: workflow_runs workflow_runs_company_id_id_binding_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_runs_company_id_id_binding_id_key UNIQUE (company_id, id, binding_id);


--
-- Name: workflow_runs workflow_runs_company_id_id_channel_id_thread_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_runs_company_id_id_channel_id_thread_id_key UNIQUE (company_id, id, channel_id, thread_id);


--
-- Name: workflow_runs workflow_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_runs_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_versions workflow_versions_company_id_command_key_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_versions
    ADD CONSTRAINT workflow_versions_company_id_command_key_key UNIQUE (company_id, command_key);


--
-- Name: workflow_versions workflow_versions_company_id_workflow_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_versions
    ADD CONSTRAINT workflow_versions_company_id_workflow_id_id_key UNIQUE (company_id, workflow_id, id);


--
-- Name: workflow_versions workflow_versions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_versions
    ADD CONSTRAINT workflow_versions_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_wait_events workflow_wait_events_company_id_run_id_execution_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_wait_events
    ADD CONSTRAINT workflow_wait_events_company_id_run_id_execution_id_id_key UNIQUE (company_id, run_id, execution_id, id);


--
-- Name: workflow_wait_events workflow_wait_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_wait_events
    ADD CONSTRAINT workflow_wait_events_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_waits workflow_wait_execution; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_waits
    ADD CONSTRAINT workflow_wait_execution UNIQUE (company_id, run_id, execution_id);


--
-- Name: workflow_waits workflow_waits_company_id_run_id_execution_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_waits
    ADD CONSTRAINT workflow_waits_company_id_run_id_execution_id_id_key UNIQUE (company_id, run_id, execution_id, id);


--
-- Name: workflow_waits workflow_waits_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_waits
    ADD CONSTRAINT workflow_waits_pkey PRIMARY KEY (company_id, id);


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

CREATE INDEX background_tasks_pending_ready_idx ON public.background_tasks USING btree (company_id, run_at, created_at, id) WHERE ((status = 'pending'::text) AND (owner_principal_kind = 'agent'::text));


--
-- Name: background_tasks_processing_lease_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_processing_lease_idx ON public.background_tasks USING btree (lock_expires_at, id) WHERE (status = 'processing'::text);


--
-- Name: background_tasks_schedule_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_schedule_idx ON public.background_tasks USING btree (((payload ->> 'schedule_id'::text))) WHERE (task_type = 'scheduled_agent_run'::text);


--
-- Name: background_tasks_thread_correlation_match_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_thread_correlation_match_idx ON public.background_tasks USING btree (company_id, thread_id, correlation_id, queue_kind, ((task_type = ANY (ARRAY['email_agent_dispatch'::text, 'scheduled_agent_run'::text]))) DESC, created_at DESC, id) INCLUDE (task_type) WHERE (thread_id IS NOT NULL);


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
-- Name: workflow_action_one_final_coverage; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX workflow_action_one_final_coverage ON public.workflow_action_evidence USING btree (company_id, invocation_id, coverage_digest) WHERE grant_eligible;


--
-- Name: workflow_dispatch_one_opportunity; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX workflow_dispatch_one_opportunity ON public.workflow_dispatch_demand USING btree ((true)) WHERE (opportunity_until IS NOT NULL);


--
-- Name: workflow_message_actions_open; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_message_actions_open ON public.workflow_message_actions USING btree (company_id, execution_id, consume_after, invocation_id) WHERE (state = ANY (ARRAY['pending'::text, 'uncertain'::text]));


--
-- Name: workflow_message_actions_wait; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_message_actions_wait ON public.workflow_message_actions USING btree (company_id, execution_id, wait_id) WHERE (wait_id IS NOT NULL);


--
-- Name: workflow_message_delivery_events_cursor; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_message_delivery_events_cursor ON public.workflow_message_delivery_events USING btree (company_id, invocation_id, seq);


--
-- Name: workflow_message_destinations_run; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_message_destinations_run ON public.workflow_message_destinations USING btree (company_id, run_id) WHERE (delivery_id IS NOT NULL);


--
-- Name: workflow_one_execution_job; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX workflow_one_execution_job ON public.background_tasks USING btree (company_id, workflow_execution_id) WHERE (queue_kind = 'workflow'::text);


--
-- Name: workflow_run_event_execution_kind; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX workflow_run_event_execution_kind ON public.workflow_run_events USING btree (company_id, run_id, execution_id, event_kind) WHERE (event_kind <> 'action_reconciled'::text);


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

CREATE TRIGGER background_tasks_bump_attention_version BEFORE UPDATE OF owner_principal_id, owner_principal_kind ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.bump_task_attention_version_for_owner_change();


--
-- Name: background_tasks background_tasks_initialize_ownership; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_initialize_ownership BEFORE INSERT ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.initialize_task_ownership();


--
-- Name: background_tasks background_tasks_notify_activity; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_notify_activity AFTER INSERT OR UPDATE OF status ON public.background_tasks FOR EACH ROW WHEN (((new.queue_kind = 'legacy'::text) AND (new.thread_id IS NOT NULL))) EXECUTE FUNCTION public.notify_thread_activity();


--
-- Name: background_tasks background_tasks_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_notify_attention AFTER INSERT OR UPDATE OF status, owner_principal_id, owner_principal_kind, business_priority, business_due_at, attention_version ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.notify_attention_changed('task');


--
-- Name: background_tasks background_tasks_notify_attention_delete; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_notify_attention_delete AFTER DELETE ON public.background_tasks FOR EACH ROW WHEN ((old.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.notify_attention_changed('task');


--
-- Name: background_tasks background_tasks_preserve_queue_identity; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_preserve_queue_identity BEFORE UPDATE OF queue_kind, workflow_execution_id ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.preserve_background_task_queue_identity();


--
-- Name: background_tasks background_tasks_record_initial_ownership; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_record_initial_ownership AFTER INSERT ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.record_initial_task_ownership();


--
-- Name: background_tasks background_tasks_record_status_event; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_record_status_event AFTER INSERT OR UPDATE OF status ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.record_task_status_event();


--
-- Name: background_tasks background_tasks_validate_workflow_association; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_validate_workflow_association BEFORE INSERT OR UPDATE OF company_id, channel_id, thread_id, workflow_execution_id, queue_kind ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'workflow'::text)) EXECUTE FUNCTION public.validate_workflow_job_association();


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
-- Name: company_mcp_connections company_mcp_endpoint_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER company_mcp_endpoint_immutable BEFORE UPDATE ON public.company_mcp_connections FOR EACH ROW EXECUTE FUNCTION public.company_mcp_endpoint_immutable();


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

CREATE TRIGGER lock_task_agent_harnesses BEFORE INSERT OR UPDATE OF status, owner_principal_id, channel_id ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.lock_task_agent_harnesses();


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
-- Name: message_deliveries message_deliveries_preserve_cancellation; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER message_deliveries_preserve_cancellation BEFORE UPDATE OF cancellation_requested_at, cancellation_reason ON public.message_deliveries FOR EACH ROW EXECUTE FUNCTION public.preserve_delivery_cancellation();


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
-- Name: principals principals_release_owned_tasks_on_demotion; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER principals_release_owned_tasks_on_demotion BEFORE UPDATE OF kind ON public.principals FOR EACH ROW WHEN (((old.kind = ANY (ARRAY['person'::text, 'agent'::text])) AND (new.kind = 'external'::text))) EXECUTE FUNCTION public.release_tasks_for_removed_principal();


--
-- Name: delegation_control_commands require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.delegation_control_commands FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: human_task_completions require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.human_task_completions FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: start_agent_task_commands require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.start_agent_task_commands FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: task_agent_instructions require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.task_agent_instructions FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: task_approval_waits require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.task_approval_waits FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: task_channel_targets require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.task_channel_targets FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: task_harness_runs require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.task_harness_runs FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: task_outreaches require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.task_outreaches FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: task_ownership_events require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.task_ownership_events FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: task_status_events require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.task_status_events FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: thread_handoff_runs require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.thread_handoff_runs FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();


--
-- Name: thread_handoff_events require_legacy_handoff_event_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_handoff_event_task BEFORE INSERT ON public.thread_handoff_events FOR EACH ROW EXECUTE FUNCTION public.require_legacy_handoff_event_task();


--
-- Name: agent_channel_provisions require_legacy_provision_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_provision_task BEFORE INSERT OR UPDATE ON public.agent_channel_provisions FOR EACH ROW EXECUTE FUNCTION public.require_legacy_provision_task();


--
-- Name: schedule_runs require_legacy_schedule_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_schedule_task BEFORE INSERT OR UPDATE ON public.schedule_runs FOR EACH ROW EXECUTE FUNCTION public.require_legacy_schedule_task();


--
-- Name: human_approvals require_legacy_source_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_source_task BEFORE INSERT OR UPDATE ON public.human_approvals FOR EACH ROW EXECUTE FUNCTION public.require_legacy_source_task();


--
-- Name: message_deliveries require_legacy_source_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_source_task BEFORE INSERT OR UPDATE ON public.message_deliveries FOR EACH ROW EXECUTE FUNCTION public.require_legacy_source_task();


--
-- Name: response_drafts require_legacy_source_task; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_legacy_source_task BEFORE INSERT OR UPDATE ON public.response_drafts FOR EACH ROW EXECUTE FUNCTION public.require_legacy_source_task();


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
-- Name: channel_schedules schedule_channel_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER schedule_channel_immutable BEFORE UPDATE OF channel_id ON public.channel_schedules FOR EACH ROW EXECUTE FUNCTION public.require_immutable_schedule_channel();


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

CREATE TRIGGER task_status_actionable_notification AFTER UPDATE OF status ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.notification_from_task_status();


--
-- Name: background_tasks task_status_actionable_notification_delete; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_status_actionable_notification_delete AFTER DELETE ON public.background_tasks FOR EACH ROW WHEN ((old.queue_kind = 'legacy'::text)) EXECUTE FUNCTION public.notification_from_task_status();


--
-- Name: task_status_events task_status_events_notify_chain; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER task_status_events_notify_chain AFTER INSERT ON public.task_status_events FOR EACH ROW EXECUTE FUNCTION public.notify_task_chain_changed();


--
-- Name: thread_handoff_events thread_handoff_events_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER thread_handoff_events_immutable BEFORE DELETE OR UPDATE ON public.thread_handoff_events FOR EACH ROW EXECUTE FUNCTION public.thread_handoff_events_are_immutable();


--
-- Name: thread_handoffs thread_handoffs_bump_cleanup_version; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER thread_handoffs_bump_cleanup_version BEFORE UPDATE OF responsible_principal_id ON public.thread_handoffs FOR EACH ROW EXECUTE FUNCTION public.bump_handoff_version_for_responsibility_cleanup();


--
-- Name: thread_handoffs thread_handoffs_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER thread_handoffs_notify_attention AFTER INSERT OR DELETE OR UPDATE OF state, responsible_principal_id, business_priority, business_due_at, version, generation ON public.thread_handoffs FOR EACH ROW EXECUTE FUNCTION public.notify_attention_changed('thread_handoff');


--
-- Name: thread_messages thread_messages_delete_orphan_message; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER thread_messages_delete_orphan_message AFTER DELETE ON public.thread_messages FOR EACH ROW EXECUTE FUNCTION public.delete_orphan_message();


--
-- Name: thread_messages thread_messages_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER thread_messages_notify AFTER INSERT ON public.thread_messages FOR EACH ROW EXECUTE FUNCTION public.notify_thread_message();


--
-- Name: workflow_action_actual_receipt_observations workflow_action_actual_observation_conflicts; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_actual_observation_conflicts AFTER INSERT ON public.workflow_action_actual_receipt_observations FOR EACH ROW EXECUTE FUNCTION public.workflow_action_actual_observation_conflicts();


--
-- Name: workflow_action_actual_receipt_observations workflow_action_actual_observation_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_actual_observation_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_actual_receipt_observations FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_actual_receipt_observations workflow_action_actual_observation_receipt_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_actual_observation_receipt_guard AFTER INSERT ON public.workflow_action_actual_receipt_observations DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_actual_observation_receipt_guard();


--
-- Name: workflow_action_evidence workflow_action_applied_request_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_applied_request_guard BEFORE INSERT ON public.workflow_action_evidence FOR EACH ROW EXECUTE FUNCTION public.workflow_action_applied_request_guard();


--
-- Name: workflow_run_events workflow_action_audit_command_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_audit_command_guard AFTER INSERT ON public.workflow_run_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN ((new.event_kind = 'action_reconciled'::text)) EXECUTE FUNCTION public.workflow_action_audit_command_guard();


--
-- Name: workflow_run_events workflow_action_audit_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_audit_immutable BEFORE DELETE OR UPDATE ON public.workflow_run_events FOR EACH ROW EXECUTE FUNCTION public.workflow_action_audit_immutable();


--
-- Name: workflow_action_evidence_commands workflow_action_bind_schedule; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_bind_schedule AFTER INSERT ON public.workflow_action_evidence_commands FOR EACH ROW EXECUTE FUNCTION public.workflow_action_bind_schedule();


--
-- Name: workflow_runs workflow_action_capture_claim_retirement; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_capture_claim_retirement BEFORE UPDATE ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.workflow_action_capture_claim_retirement();


--
-- Name: background_tasks workflow_action_capture_schedule; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_capture_schedule BEFORE UPDATE ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.workflow_action_capture_schedule();


--
-- Name: workflow_runs workflow_action_capture_state_witness; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_capture_state_witness BEFORE UPDATE ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.workflow_action_capture_state_witness();


--
-- Name: workflow_action_claim_budget_refusals workflow_action_claim_budget_refusal_commit_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_claim_budget_refusal_commit_guard AFTER INSERT ON public.workflow_action_claim_budget_refusals DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_claim_budget_refusal_guard();


--
-- Name: workflow_action_claim_budget_refusals workflow_action_claim_budget_refusal_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_claim_budget_refusal_guard BEFORE INSERT ON public.workflow_action_claim_budget_refusals FOR EACH ROW EXECUTE FUNCTION public.workflow_action_claim_budget_refusal_guard();


--
-- Name: workflow_action_claim_budget_refusals workflow_action_claim_budget_refusal_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_claim_budget_refusal_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_claim_budget_refusals FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episode_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_claim_episode_guard AFTER INSERT ON public.workflow_action_claim_episodes DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_claim_episode_guard();


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episode_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_claim_episode_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_claim_episodes FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_run_events workflow_action_claim_refusal_audit_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_claim_refusal_audit_immutable BEFORE DELETE OR UPDATE ON public.workflow_run_events FOR EACH ROW EXECUTE FUNCTION public.workflow_action_claim_refusal_audit_immutable();


--
-- Name: workflow_action_claim_retirement_witnesses workflow_action_claim_retirement_atomic_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_claim_retirement_atomic_guard AFTER INSERT OR UPDATE ON public.workflow_action_claim_retirement_witnesses DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_claim_retirement_atomic_guard();


--
-- Name: workflow_action_claim_retirement_witnesses workflow_action_claim_retirement_witness_owner; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_claim_retirement_witness_owner BEFORE INSERT OR DELETE OR UPDATE ON public.workflow_action_claim_retirement_witnesses FOR EACH ROW EXECUTE FUNCTION public.workflow_action_claim_retirement_witness_owner();


--
-- Name: workflow_action_evidence_commands workflow_action_command_scope_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_command_scope_guard AFTER INSERT ON public.workflow_action_evidence_commands DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_command_scope_guard();


--
-- Name: background_tasks workflow_action_confirm_claim_retirement; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_confirm_claim_retirement BEFORE UPDATE ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.workflow_action_confirm_claim_retirement();


--
-- Name: workflow_action_evidence_conflicts workflow_action_conflict_park_pending; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_conflict_park_pending AFTER INSERT ON public.workflow_action_evidence_conflicts FOR EACH ROW EXECUTE FUNCTION public.workflow_action_conflict_park_pending();


--
-- Name: workflow_action_evidence_consumptions workflow_action_consumption_commit_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_consumption_commit_guard AFTER INSERT ON public.workflow_action_evidence_consumptions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_consumption_commit_guard();


--
-- Name: workflow_action_evidence_consumptions workflow_action_consumption_reservation_identity; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_consumption_reservation_identity BEFORE INSERT ON public.workflow_action_evidence_consumptions FOR EACH ROW EXECUTE FUNCTION public.workflow_action_reservation_identity();


--
-- Name: workflow_action_dispatches workflow_action_dispatch_commit_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_dispatch_commit_guard AFTER INSERT ON public.workflow_action_dispatches DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_dispatch_commit_guard();


--
-- Name: workflow_action_dispatches workflow_action_dispatch_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_dispatch_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_dispatches FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_remote_entries workflow_action_entry_reservation_identity; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_entry_reservation_identity BEFORE INSERT ON public.workflow_action_remote_entries FOR EACH ROW EXECUTE FUNCTION public.workflow_action_reservation_identity();


--
-- Name: workflow_action_evidence_commands workflow_action_evidence_command_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_evidence_command_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_evidence_commands FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_evidence workflow_action_evidence_commit_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_evidence_commit_guard AFTER INSERT ON public.workflow_action_evidence DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_evidence_commit_guard();


--
-- Name: workflow_action_evidence_conflicts workflow_action_evidence_conflict_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_evidence_conflict_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_evidence_conflicts FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_evidence_conflicts workflow_action_evidence_conflict_revision; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_evidence_conflict_revision AFTER INSERT ON public.workflow_action_evidence_conflicts FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_owned_revision();


--
-- Name: workflow_action_evidence_consumptions workflow_action_evidence_consumption_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_evidence_consumption_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_evidence_consumptions FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_evidence_coverage workflow_action_evidence_coverage_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_evidence_coverage_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_evidence_coverage FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_evidence workflow_action_evidence_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_evidence_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_evidence FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_evidence workflow_action_evidence_revision; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_evidence_revision AFTER INSERT ON public.workflow_action_evidence FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_owned_revision();


--
-- Name: workflow_action_intents workflow_action_intent_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_intent_immutable BEFORE UPDATE ON public.workflow_action_intents FOR EACH ROW EXECUTE FUNCTION public.workflow_action_intent_immutable();


--
-- Name: workflow_run_events workflow_action_link_claim_retirement_audit; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_link_claim_retirement_audit AFTER INSERT ON public.workflow_run_events FOR EACH ROW EXECUTE FUNCTION public.workflow_action_link_claim_retirement_audit();


--
-- Name: workflow_action_model_calls workflow_action_model_call_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_model_call_immutable BEFORE UPDATE ON public.workflow_action_model_calls FOR EACH ROW EXECUTE FUNCTION public.workflow_action_intent_immutable();


--
-- Name: workflow_action_receipts workflow_action_receipt_evidence_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_receipt_evidence_guard BEFORE INSERT ON public.workflow_action_receipts FOR EACH ROW EXECUTE FUNCTION public.workflow_action_receipt_evidence_guard();


--
-- Name: workflow_action_receipts workflow_action_receipt_finality_conflict; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_receipt_finality_conflict AFTER INSERT ON public.workflow_action_receipts FOR EACH ROW EXECUTE FUNCTION public.workflow_action_receipt_finality_conflict();


--
-- Name: workflow_action_receipts workflow_action_receipt_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_receipt_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_receipts FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_reconciliations workflow_action_reconciliation_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_reconciliation_guard BEFORE INSERT ON public.workflow_action_reconciliations FOR EACH ROW EXECUTE FUNCTION public.workflow_action_reconciliation_guard();


--
-- Name: workflow_action_reconciliations workflow_action_reconciliation_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_reconciliation_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_reconciliations FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_remote_entries workflow_action_remote_entry_commit_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_action_remote_entry_commit_guard AFTER INSERT ON public.workflow_action_remote_entries DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_action_remote_entry_commit_guard();


--
-- Name: workflow_action_remote_entries workflow_action_remote_entry_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_remote_entry_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_remote_entries FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_schedule_witnesses workflow_action_schedule_witness_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_schedule_witness_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_schedule_witnesses FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_schedule_witnesses workflow_action_schedule_witness_owner; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_schedule_witness_owner BEFORE INSERT ON public.workflow_action_schedule_witnesses FOR EACH ROW EXECUTE FUNCTION public.workflow_action_state_witness_owner();


--
-- Name: workflow_action_state_witnesses workflow_action_state_witness_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_state_witness_immutable BEFORE DELETE OR UPDATE ON public.workflow_action_state_witnesses FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_action_state_witnesses workflow_action_state_witness_owner; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_action_state_witness_owner BEFORE INSERT ON public.workflow_action_state_witnesses FOR EACH ROW EXECUTE FUNCTION public.workflow_action_state_witness_owner();


--
-- Name: workflow_admissions workflow_admission_parent; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_admission_parent BEFORE INSERT OR UPDATE ON public.workflow_admissions FOR EACH ROW EXECUTE FUNCTION public.check_workflow_parent_source();


--
-- Name: workflow_runs workflow_allocate_wakeup; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_allocate_wakeup BEFORE INSERT OR UPDATE ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.allocate_workflow_wakeup_sequence();


--
-- Name: workflow_budget_receipts workflow_budget_receipt_debit; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_budget_receipt_debit BEFORE INSERT ON public.workflow_budget_receipts FOR EACH ROW EXECUTE FUNCTION public.debit_workflow_budget_receipt();


--
-- Name: workflow_budget_receipts workflow_budget_receipt_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_budget_receipt_immutable BEFORE DELETE OR UPDATE ON public.workflow_budget_receipts FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_budget_receipt();


--
-- Name: workflow_root_budget_usage workflow_budget_usage_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_budget_usage_guard BEFORE INSERT OR DELETE OR UPDATE ON public.workflow_root_budget_usage FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_budget_usage();


--
-- Name: workflow_capacity_policy workflow_capacity_policy_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_capacity_policy_immutable BEFORE DELETE OR UPDATE ON public.workflow_capacity_policy FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_capacity_policy();


--
-- Name: workflow_control_commands workflow_control_receipt_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_control_receipt_immutable BEFORE DELETE OR UPDATE ON public.workflow_control_commands FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_control_receipt();


--
-- Name: background_tasks workflow_control_retry_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_control_retry_guard AFTER UPDATE ON public.background_tasks DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.check_workflow_control_retry();


--
-- Name: workflow_executions workflow_execution_revision; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_execution_revision AFTER INSERT OR UPDATE ON public.workflow_executions FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_owned_revision();


--
-- Name: workflow_executions workflow_executions_preserve_facts; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_executions_preserve_facts BEFORE UPDATE OF activated_at, frozen_inputs, completed_at, committed_output ON public.workflow_executions FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_execution_facts();


--
-- Name: workflow_executions workflow_executions_preserve_identity; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_executions_preserve_identity BEFORE UPDATE OF company_id, id, run_id, step_id, activation ON public.workflow_executions FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_execution_identity();


--
-- Name: background_tasks workflow_fenced_completion_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_fenced_completion_guard AFTER UPDATE ON public.background_tasks DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.check_workflow_fenced_completion();


--
-- Name: background_tasks workflow_fenced_retirement_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_fenced_retirement_guard AFTER UPDATE ON public.background_tasks DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.check_workflow_fenced_retirement();


--
-- Name: workflow_http_connections workflow_http_identity_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_http_identity_immutable BEFORE UPDATE ON public.workflow_http_connections FOR EACH ROW EXECUTE FUNCTION public.workflow_http_identity_immutable();


--
-- Name: background_tasks workflow_job_revision; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_job_revision AFTER INSERT OR UPDATE ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_owned_revision();


--
-- Name: workflow_message_actions workflow_message_action_capture; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_action_capture BEFORE INSERT ON public.workflow_message_actions FOR EACH ROW EXECUTE FUNCTION public.workflow_message_action_capture();


--
-- Name: workflow_message_actions workflow_message_action_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_action_guard BEFORE DELETE OR UPDATE ON public.workflow_message_actions FOR EACH ROW EXECUTE FUNCTION public.workflow_message_action_guard();


--
-- Name: workflow_action_dispatches workflow_message_admission_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_admission_guard BEFORE INSERT ON public.workflow_action_dispatches FOR EACH ROW WHEN ((new.effect_kind = 'message'::text)) EXECUTE FUNCTION public.workflow_message_admission_guard();


--
-- Name: message_deliveries workflow_message_delivery_event; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_delivery_event AFTER UPDATE OF status ON public.message_deliveries FOR EACH ROW WHEN (((old.status IS DISTINCT FROM new.status) AND ((new.status = ANY (ARRAY['delivered'::text, 'dead_letter'::text, 'outcome_unknown'::text])) OR (old.status = 'outcome_unknown'::text)))) EXECUTE FUNCTION public.workflow_message_delivery_event();


--
-- Name: message_deliveries workflow_message_delivery_transition_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_delivery_transition_guard BEFORE UPDATE OF status ON public.message_deliveries FOR EACH ROW WHEN ((old.status IS DISTINCT FROM new.status)) EXECUTE FUNCTION public.workflow_message_delivery_transition_guard();


--
-- Name: workflow_message_destinations workflow_message_destination_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_destination_immutable BEFORE DELETE OR UPDATE ON public.workflow_message_destinations FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_waits workflow_message_effect_wait_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_effect_wait_guard BEFORE INSERT ON public.workflow_waits FOR EACH ROW WHEN ((new.reason = 'effect'::text)) EXECUTE FUNCTION public.workflow_message_effect_wait_guard();


--
-- Name: workflow_message_delivery_events workflow_message_event_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_event_immutable BEFORE DELETE OR UPDATE ON public.workflow_message_delivery_events FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: message_delivery_parts workflow_message_part_transition_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_part_transition_guard BEFORE UPDATE OF status ON public.message_delivery_parts FOR EACH ROW WHEN ((old.status IS DISTINCT FROM new.status)) EXECUTE FUNCTION public.workflow_message_part_transition_guard();


--
-- Name: workflow_action_receipts workflow_message_receipt_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_receipt_guard BEFORE INSERT ON public.workflow_action_receipts FOR EACH ROW WHEN ((new.effect_kind = 'message'::text)) EXECUTE FUNCTION public.workflow_message_receipt_guard();


--
-- Name: workflow_message_delivery_reopens workflow_message_reopen_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_reopen_guard BEFORE INSERT ON public.workflow_message_delivery_reopens FOR EACH ROW EXECUTE FUNCTION public.workflow_message_reopen_guard();


--
-- Name: workflow_message_delivery_reopens workflow_message_reopen_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_reopen_immutable BEFORE DELETE OR UPDATE ON public.workflow_message_delivery_reopens FOR EACH ROW EXECUTE FUNCTION public.workflow_action_fact_immutable();


--
-- Name: workflow_runs workflow_parent_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_parent_immutable BEFORE UPDATE OF parent_run_id, parent_execution_id ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_parent();


--
-- Name: workflow_runs workflow_parent_link; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_parent_link BEFORE INSERT ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.insert_workflow_parent_link();


--
-- Name: workflow_run_parents workflow_parent_link_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_parent_link_immutable BEFORE UPDATE ON public.workflow_run_parents FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_parent_link();


--
-- Name: workflow_executions workflow_preserve_progression; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_preserve_progression BEFORE INSERT OR UPDATE ON public.workflow_executions FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_progression();


--
-- Name: workflow_executions workflow_progression_commit_deadline; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_progression_commit_deadline AFTER INSERT OR UPDATE ON public.workflow_executions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.check_workflow_progression_deadline();


--
-- Name: workflow_root_budgets workflow_root_budget_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_root_budget_immutable BEFORE DELETE OR UPDATE ON public.workflow_root_budgets FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_budget_identity();


--
-- Name: workflow_runs workflow_root_budget_link; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_root_budget_link BEFORE INSERT ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.insert_workflow_budget_link();


--
-- Name: workflow_run_budgets workflow_run_budget_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_run_budget_immutable BEFORE DELETE OR UPDATE ON public.workflow_run_budgets FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_budget_identity();


--
-- Name: workflow_runs workflow_run_revision; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_run_revision BEFORE UPDATE ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_run_revision();


--
-- Name: workflow_runs workflow_runs_preserve_association; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_runs_preserve_association BEFORE UPDATE OF company_id, id, channel_id, thread_id ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_run_association();


--
-- Name: workflow_runs workflow_terminal_parent_wakeup; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_terminal_parent_wakeup AFTER UPDATE OF state ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.schedule_workflow_parent_wakeup();


--
-- Name: workflow_run_events workflow_validate_wakeup; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_validate_wakeup BEFORE INSERT ON public.workflow_run_events FOR EACH ROW EXECUTE FUNCTION public.validate_workflow_wakeup_insert();


--
-- Name: workflow_waits workflow_wait_commit_deadline; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_wait_commit_deadline AFTER INSERT OR UPDATE ON public.workflow_waits DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.check_workflow_wait_commit();


--
-- Name: workflow_wait_events workflow_wait_event_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_wait_event_immutable BEFORE UPDATE ON public.workflow_wait_events FOR EACH ROW EXECUTE FUNCTION public.immutable_workflow_wait_event();


--
-- Name: workflow_waits workflow_wait_revision; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_wait_revision AFTER INSERT OR UPDATE ON public.workflow_waits FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_owned_revision();


--
-- Name: workflow_run_events workflow_wakeup_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_wakeup_immutable BEFORE UPDATE ON public.workflow_run_events FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_wakeup();


--
-- Name: agent_channel_provisions agent_channel_provisions_agent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_agent_id_fkey FOREIGN KEY (agent_id) REFERENCES public.agents(id) ON DELETE CASCADE;


--
-- Name: agent_channel_provisions agent_channel_provisions_agent_scope_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_agent_scope_fk FOREIGN KEY (company_id, agent_id) REFERENCES public.agents(company_id, id) ON DELETE CASCADE;


--
-- Name: agent_channel_provisions agent_channel_provisions_channel_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_channel_id_fkey FOREIGN KEY (channel_id) REFERENCES public.channels(id) ON DELETE CASCADE;


--
-- Name: agent_channel_provisions agent_channel_provisions_channel_scope_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_channel_scope_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: agent_channel_provisions agent_channel_provisions_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.background_tasks(id) ON DELETE CASCADE;


--
-- Name: agent_channel_provisions agent_channel_provisions_task_scope_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agent_channel_provisions
    ADD CONSTRAINT agent_channel_provisions_task_scope_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


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
-- Name: background_tasks background_tasks_workflow_execution_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_workflow_execution_fk FOREIGN KEY (company_id, workflow_execution_id) REFERENCES public.workflow_executions(company_id, id);


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
    ADD CONSTRAINT delegation_control_commands_actor_fk FOREIGN KEY (company_id, actor_principal_id, actor_kind) REFERENCES public.principals(company_id, id, kind) ON UPDATE CASCADE ON DELETE RESTRICT;


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
-- Name: message_deliveries message_deliveries_reopen; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_reopen FOREIGN KEY (company_id, reopen_id) REFERENCES public.workflow_message_delivery_reopens(company_id, id) ON DELETE SET NULL (reopen_id);


--
-- Name: message_deliveries message_deliveries_settled_evidence; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.message_deliveries
    ADD CONSTRAINT message_deliveries_settled_evidence FOREIGN KEY (company_id, settled_evidence_id) REFERENCES public.workflow_action_evidence(company_id, id) ON DELETE SET NULL (settled_evidence_id);


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
-- Name: schedule_runs schedule_runs_channel_scope_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_channel_scope_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: schedule_runs schedule_runs_schedule_company_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_schedule_company_fk FOREIGN KEY (company_id, schedule_id) REFERENCES public.channel_schedules(company_id, id) ON DELETE CASCADE;


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
-- Name: schedule_runs schedule_runs_task_scope_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_task_scope_fk FOREIGN KEY (company_id, channel_id, thread_id, task_id) REFERENCES public.background_tasks(company_id, channel_id, thread_id, id) ON DELETE SET NULL (task_id);


--
-- Name: schedule_runs schedule_runs_thread_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_thread_id_fkey FOREIGN KEY (thread_id) REFERENCES public.threads(id) ON DELETE SET NULL;


--
-- Name: schedule_runs schedule_runs_thread_scope_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_thread_scope_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE SET NULL (thread_id);


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
-- Name: thread_handoff_events thread_handoff_events_company_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoff_events
    ADD CONSTRAINT thread_handoff_events_company_fk FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: thread_handoff_runs thread_handoff_runs_handoff_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoff_runs
    ADD CONSTRAINT thread_handoff_runs_handoff_fk FOREIGN KEY (company_id, handoff_id) REFERENCES public.thread_handoffs(company_id, id) ON DELETE CASCADE;


--
-- Name: thread_handoff_runs thread_handoff_runs_task_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoff_runs
    ADD CONSTRAINT thread_handoff_runs_task_fk FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


--
-- Name: thread_handoffs thread_handoffs_channel_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_channel_fk FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id) ON DELETE CASCADE;


--
-- Name: thread_handoffs thread_handoffs_responsible_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_responsible_fk FOREIGN KEY (company_id, responsible_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (responsible_principal_id);


--
-- Name: thread_handoffs thread_handoffs_source_message_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_source_message_fk FOREIGN KEY (company_id, source_message_id) REFERENCES public.messages(company_id, id) ON DELETE CASCADE;


--
-- Name: thread_handoffs thread_handoffs_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.thread_handoffs
    ADD CONSTRAINT thread_handoffs_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE CASCADE;


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
-- Name: workflow_action_actual_receipt_observations workflow_action_actual_receip_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_actual_receipt_observations
    ADD CONSTRAINT workflow_action_actual_receip_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id) REFERENCES public.workflow_action_remote_entries(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence workflow_action_applied_request_scope; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence
    ADD CONSTRAINT workflow_action_applied_request_scope FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, applied_remote_entry_id) REFERENCES public.workflow_action_remote_entries(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_budget_refusals workflow_action_claim_budget__company_id_run_id_audit_sequ_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_budget_refusals
    ADD CONSTRAINT workflow_action_claim_budget__company_id_run_id_audit_sequ_fkey FOREIGN KEY (company_id, run_id, audit_sequence) REFERENCES public.workflow_run_events(company_id, run_id, sequence) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_budget_refusals workflow_action_claim_budget__company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_budget_refusals
    ADD CONSTRAINT workflow_action_claim_budget__company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, job_id, command_key, retired_attempt) REFERENCES public.workflow_action_claim_episodes(company_id, run_id, execution_id, job_id, command_key, retired_attempt) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episod_company_id_run_id_execution__fkey1; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_episodes
    ADD CONSTRAINT workflow_action_claim_episod_company_id_run_id_execution__fkey1 FOREIGN KEY (company_id, run_id, execution_id, job_id, command_key) REFERENCES public.workflow_action_evidence_commands(company_id, run_id, execution_id, scheduled_job_id, command_key) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episode_company_id_execution_id_job__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_episodes
    ADD CONSTRAINT workflow_action_claim_episode_company_id_execution_id_job__fkey FOREIGN KEY (company_id, execution_id, job_id) REFERENCES public.background_tasks(company_id, workflow_execution_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episode_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_episodes
    ADD CONSTRAINT workflow_action_claim_episode_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id) REFERENCES public.workflow_executions(company_id, run_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_episodes workflow_action_claim_episodes_job_id_retired_attempt_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_episodes
    ADD CONSTRAINT workflow_action_claim_episodes_job_id_retired_attempt_fkey FOREIGN KEY (job_id, retired_attempt) REFERENCES public.task_attempts(task_id, attempt_number) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_retirement_witnesses workflow_action_claim_retirem_company_id_run_id_audit_sequ_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_retirement_witnesses
    ADD CONSTRAINT workflow_action_claim_retirem_company_id_run_id_audit_sequ_fkey FOREIGN KEY (company_id, run_id, audit_sequence) REFERENCES public.workflow_run_events(company_id, run_id, sequence) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_retirement_witnesses workflow_action_claim_retirem_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_retirement_witnesses
    ADD CONSTRAINT workflow_action_claim_retirem_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, job_id, command_key, retired_attempt) REFERENCES public.workflow_action_claim_episodes(company_id, run_id, execution_id, job_id, command_key, retired_attempt) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_conflicts workflow_action_conflict_actual_observation; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_conflicts
    ADD CONSTRAINT workflow_action_conflict_actual_observation FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id, actual_observation_id) REFERENCES public.workflow_action_actual_receipt_observations(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_dispatches workflow_action_dispatches_company_id_execution_id_job_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_dispatches
    ADD CONSTRAINT workflow_action_dispatches_company_id_execution_id_job_id_fkey FOREIGN KEY (company_id, execution_id, job_id) REFERENCES public.background_tasks(company_id, workflow_execution_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_dispatches workflow_action_dispatches_company_id_run_id_execution_id__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_dispatches
    ADD CONSTRAINT workflow_action_dispatches_company_id_run_id_execution_id__fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest) REFERENCES public.workflow_action_intents(company_id, run_id, execution_id, id, argument_digest) ON DELETE CASCADE;


--
-- Name: workflow_action_dispatches workflow_action_dispatches_job_id_attempt_number_execution_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_dispatches
    ADD CONSTRAINT workflow_action_dispatches_job_id_attempt_number_execution_fkey FOREIGN KEY (job_id, attempt_number, execution_generation, worker_id) REFERENCES public.task_attempts(task_id, attempt_number, execution_generation, worker_id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_commands workflow_action_evidence_comm_company_id_command_key_id_re_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_evidence_comm_company_id_command_key_id_re_fkey FOREIGN KEY (company_id, command_key, id, request_digest, evidence_id) REFERENCES public.workflow_action_evidence(company_id, command_key, command_id, request_digest, id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_action_evidence_commands workflow_action_evidence_comm_company_id_execution_id_sche_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_evidence_comm_company_id_execution_id_sche_fkey FOREIGN KEY (company_id, execution_id, scheduled_job_id) REFERENCES public.background_tasks(company_id, workflow_execution_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_commands workflow_action_evidence_comm_company_id_run_id_audit_sequ_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_evidence_comm_company_id_run_id_audit_sequ_fkey FOREIGN KEY (company_id, run_id, audit_sequence) REFERENCES public.workflow_run_events(company_id, run_id, sequence) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_commands workflow_action_evidence_comm_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_evidence_comm_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id) REFERENCES public.workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence workflow_action_evidence_command_link; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence
    ADD CONSTRAINT workflow_action_evidence_command_link FOREIGN KEY (company_id, command_key, command_id, request_digest) REFERENCES public.workflow_action_evidence_commands(company_id, command_key, id, request_digest) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_action_evidence workflow_action_evidence_company_id_run_id_execution_id_in_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence
    ADD CONSTRAINT workflow_action_evidence_company_id_run_id_execution_id_in_fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id) REFERENCES public.workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_consumptions workflow_action_evidence_con_company_id_run_id_execution__fkey1; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_consumptions
    ADD CONSTRAINT workflow_action_evidence_con_company_id_run_id_execution__fkey1 FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id) REFERENCES public.workflow_action_remote_entries(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_conflicts workflow_action_evidence_con_company_id_run_id_execution__fkey2; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_conflicts
    ADD CONSTRAINT workflow_action_evidence_con_company_id_run_id_execution__fkey2 FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, contradictory_evidence_id) REFERENCES public.workflow_action_evidence(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_conflicts workflow_action_evidence_con_company_id_run_id_execution__fkey3; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_conflicts
    ADD CONSTRAINT workflow_action_evidence_con_company_id_run_id_execution__fkey3 FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id) REFERENCES public.workflow_action_remote_entries(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_conflicts workflow_action_evidence_conf_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_conflicts
    ADD CONSTRAINT workflow_action_evidence_conf_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, evidence_id) REFERENCES public.workflow_action_evidence(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_consumptions workflow_action_evidence_cons_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_consumptions
    ADD CONSTRAINT workflow_action_evidence_cons_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, evidence_id) REFERENCES public.workflow_action_evidence(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_coverage workflow_action_evidence_cov_company_id_run_id_execution__fkey1; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_coverage
    ADD CONSTRAINT workflow_action_evidence_cov_company_id_run_id_execution__fkey1 FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id) REFERENCES public.workflow_action_remote_entries(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_coverage workflow_action_evidence_cove_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_coverage
    ADD CONSTRAINT workflow_action_evidence_cove_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, evidence_id) REFERENCES public.workflow_action_evidence(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence workflow_action_evidence_delivery_subject; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence
    ADD CONSTRAINT workflow_action_evidence_delivery_subject FOREIGN KEY (company_id, invocation_id, delivery_ordinal, delivery_id) REFERENCES public.workflow_message_destinations(company_id, invocation_id, ordinal, delivery_id) ON DELETE CASCADE;


--
-- Name: workflow_action_intents workflow_action_intents_company_id_run_id_execution_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_intents
    ADD CONSTRAINT workflow_action_intents_company_id_run_id_execution_id_fkey FOREIGN KEY (company_id, run_id, execution_id) REFERENCES public.workflow_executions(company_id, run_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_model_calls workflow_action_model_calls_company_id_run_id_execution_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_model_calls
    ADD CONSTRAINT workflow_action_model_calls_company_id_run_id_execution_id_fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest) REFERENCES public.workflow_action_intents(company_id, run_id, execution_id, id, argument_digest) ON DELETE CASCADE;


--
-- Name: workflow_action_actual_receipt_observations workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_actual_receipt_observations
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_budget_refusals workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_budget_refusals
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_episodes workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_episodes
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_claim_retirement_witnesses workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_claim_retirement_witnesses
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_dispatches workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_dispatches
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_commands workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_commands
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_conflicts workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_conflicts
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_consumptions workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_consumptions
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_evidence_coverage workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_evidence_coverage
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_receipts workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_receipts
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_reconciliations workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_reconciliations
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_remote_entries workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_remote_entries
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_schedule_witnesses workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_schedule_witnesses
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_state_witnesses workflow_action_owner_lifecycle; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_state_witnesses
    ADD CONSTRAINT workflow_action_owner_lifecycle FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_receipts workflow_action_receipt_evidence_source; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_receipts
    ADD CONSTRAINT workflow_action_receipt_evidence_source FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, reconciliation_evidence_id) REFERENCES public.workflow_action_evidence(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_receipts workflow_action_receipt_remote_entry; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_receipts
    ADD CONSTRAINT workflow_action_receipt_remote_entry FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, remote_entry_id) REFERENCES public.workflow_action_remote_entries(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_receipts workflow_action_receipts_company_id_run_id_execution_id_in_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_receipts
    ADD CONSTRAINT workflow_action_receipts_company_id_run_id_execution_id_in_fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind) REFERENCES public.workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind) ON DELETE CASCADE;


--
-- Name: workflow_action_reconciliations workflow_action_reconciliatio_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_reconciliations
    ADD CONSTRAINT workflow_action_reconciliatio_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind) REFERENCES public.workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind) ON DELETE CASCADE;


--
-- Name: workflow_action_remote_entries workflow_action_remote_entrie_company_id_execution_id_job__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_remote_entries
    ADD CONSTRAINT workflow_action_remote_entrie_company_id_execution_id_job__fkey FOREIGN KEY (company_id, execution_id, job_id) REFERENCES public.background_tasks(company_id, workflow_execution_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_remote_entries workflow_action_remote_entrie_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_remote_entries
    ADD CONSTRAINT workflow_action_remote_entrie_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind) REFERENCES public.workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind) ON DELETE CASCADE;


--
-- Name: workflow_action_remote_entries workflow_action_remote_entrie_job_id_attempt_number_execut_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_remote_entries
    ADD CONSTRAINT workflow_action_remote_entrie_job_id_attempt_number_execut_fkey FOREIGN KEY (job_id, attempt_number, execution_generation, worker_id) REFERENCES public.task_attempts(task_id, attempt_number, execution_generation, worker_id) ON DELETE CASCADE;


--
-- Name: workflow_action_schedule_witnesses workflow_action_schedule_witn_company_id_execution_id_job__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_schedule_witnesses
    ADD CONSTRAINT workflow_action_schedule_witn_company_id_execution_id_job__fkey FOREIGN KEY (company_id, execution_id, job_id) REFERENCES public.background_tasks(company_id, workflow_execution_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_schedule_witnesses workflow_action_schedule_witn_company_id_run_id_execution__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_schedule_witnesses
    ADD CONSTRAINT workflow_action_schedule_witn_company_id_run_id_execution__fkey FOREIGN KEY (company_id, run_id, execution_id) REFERENCES public.workflow_executions(company_id, run_id, id) ON DELETE CASCADE;


--
-- Name: workflow_action_schedule_witnesses workflow_action_schedule_witnesses_job_id_retired_attempt_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_action_schedule_witnesses
    ADD CONSTRAINT workflow_action_schedule_witnesses_job_id_retired_attempt_fkey FOREIGN KEY (job_id, retired_attempt) REFERENCES public.task_attempts(task_id, attempt_number) ON DELETE CASCADE;


--
-- Name: workflow_admission_commands workflow_admission_commands_company_id_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_admission_commands
    ADD CONSTRAINT workflow_admission_commands_company_id_run_id_fkey FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_admissions(company_id, run_id) ON DELETE CASCADE;


--
-- Name: workflow_admissions workflow_admissions_company_id_run_id_binding_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_admissions
    ADD CONSTRAINT workflow_admissions_company_id_run_id_binding_id_fkey FOREIGN KEY (company_id, run_id, binding_id) REFERENCES public.workflow_runs(company_id, id, binding_id) ON DELETE CASCADE;


--
-- Name: workflow_binding_events workflow_binding_events_company_id_binding_id_configuratio_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_binding_events
    ADD CONSTRAINT workflow_binding_events_company_id_binding_id_configuratio_fkey FOREIGN KEY (company_id, binding_id, configuration_revision) REFERENCES public.workflow_binding_revisions(company_id, binding_id, revision);


--
-- Name: workflow_binding_events workflow_binding_events_company_id_binding_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_binding_events
    ADD CONSTRAINT workflow_binding_events_company_id_binding_id_fkey FOREIGN KEY (company_id, binding_id) REFERENCES public.workflow_bindings(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_binding_revisions workflow_binding_revisions_company_id_binding_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_binding_revisions
    ADD CONSTRAINT workflow_binding_revisions_company_id_binding_id_fkey FOREIGN KEY (company_id, binding_id) REFERENCES public.workflow_bindings(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_binding_revisions workflow_binding_revisions_company_id_version_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_binding_revisions
    ADD CONSTRAINT workflow_binding_revisions_company_id_version_id_fkey FOREIGN KEY (company_id, version_id) REFERENCES public.workflow_versions(company_id, id);


--
-- Name: workflow_bindings workflow_bindings_company_id_channel_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_bindings
    ADD CONSTRAINT workflow_bindings_company_id_channel_id_fkey FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id);


--
-- Name: workflow_bindings workflow_bindings_company_id_channel_id_thread_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_bindings
    ADD CONSTRAINT workflow_bindings_company_id_channel_id_thread_id_fkey FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id);


--
-- Name: workflow_bindings workflow_bindings_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_bindings
    ADD CONSTRAINT workflow_bindings_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: workflow_bindings workflow_bindings_selected_revision_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_bindings
    ADD CONSTRAINT workflow_bindings_selected_revision_fk FOREIGN KEY (company_id, id, configuration_revision) REFERENCES public.workflow_binding_revisions(company_id, binding_id, revision) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_budget_receipts workflow_budget_receipts_company_id_run_id_execution_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_budget_receipts
    ADD CONSTRAINT workflow_budget_receipts_company_id_run_id_execution_id_fkey FOREIGN KEY (company_id, run_id, execution_id) REFERENCES public.workflow_executions(company_id, run_id, id) ON DELETE CASCADE;


--
-- Name: workflow_budget_receipts workflow_budget_receipts_company_id_run_id_root_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_budget_receipts
    ADD CONSTRAINT workflow_budget_receipts_company_id_run_id_root_run_id_fkey FOREIGN KEY (company_id, run_id, root_run_id) REFERENCES public.workflow_run_budgets(company_id, run_id, root_run_id);


--
-- Name: workflow_control_commands workflow_control_commands_company_id_run_id_audit_sequence_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_control_commands
    ADD CONSTRAINT workflow_control_commands_company_id_run_id_audit_sequence_fkey FOREIGN KEY (company_id, run_id, audit_sequence) REFERENCES public.workflow_run_events(company_id, run_id, sequence);


--
-- Name: workflow_control_commands workflow_control_commands_company_id_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_control_commands
    ADD CONSTRAINT workflow_control_commands_company_id_run_id_fkey FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_definition_events workflow_definition_events_company_id_workflow_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_definition_events
    ADD CONSTRAINT workflow_definition_events_company_id_workflow_id_fkey FOREIGN KEY (company_id, workflow_id) REFERENCES public.workflow_definitions(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_definition_events workflow_definition_events_company_id_workflow_id_version__fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_definition_events
    ADD CONSTRAINT workflow_definition_events_company_id_workflow_id_version__fkey FOREIGN KEY (company_id, workflow_id, version_id) REFERENCES public.workflow_versions(company_id, workflow_id, id);


--
-- Name: workflow_definitions workflow_definitions_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_definitions
    ADD CONSTRAINT workflow_definitions_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: workflow_executions workflow_executions_company_id_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_executions
    ADD CONSTRAINT workflow_executions_company_id_run_id_fkey FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_history_members workflow_history_members_company_id_channel_id_thread_id_m_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_history_members
    ADD CONSTRAINT workflow_history_members_company_id_channel_id_thread_id_m_fkey FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES public.thread_messages(company_id, channel_id, thread_id, message_id);


--
-- Name: workflow_history_members workflow_history_members_company_id_run_id_channel_id_thre_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_history_members
    ADD CONSTRAINT workflow_history_members_company_id_run_id_channel_id_thre_fkey FOREIGN KEY (company_id, run_id, channel_id, thread_id) REFERENCES public.workflow_runs(company_id, id, channel_id, thread_id) ON DELETE CASCADE;


--
-- Name: workflow_http_connections workflow_http_connections_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_http_connections
    ADD CONSTRAINT workflow_http_connections_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: workflow_http_credentials workflow_http_credentials_company_id_connection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_http_credentials
    ADD CONSTRAINT workflow_http_credentials_company_id_connection_id_fkey FOREIGN KEY (company_id, connection_id) REFERENCES public.workflow_http_connections(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_http_operations workflow_http_operations_company_id_connection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_http_operations
    ADD CONSTRAINT workflow_http_operations_company_id_connection_id_fkey FOREIGN KEY (company_id, connection_id) REFERENCES public.workflow_http_connections(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_mcp_operations workflow_mcp_operations_company_id_connection_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_mcp_operations
    ADD CONSTRAINT workflow_mcp_operations_company_id_connection_id_fkey FOREIGN KEY (company_id, connection_id) REFERENCES public.company_mcp_connections(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_mcp_operations workflow_mcp_operations_company_id_connection_id_name_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_mcp_operations
    ADD CONSTRAINT workflow_mcp_operations_company_id_connection_id_name_fkey FOREIGN KEY (company_id, connection_id, name) REFERENCES public.company_mcp_tool_grants(company_id, connection_id, tool_name) ON DELETE CASCADE;


--
-- Name: workflow_message_actions workflow_message_action_marker; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_actions
    ADD CONSTRAINT workflow_message_action_marker FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind) REFERENCES public.workflow_action_dispatches(company_id, run_id, execution_id, invocation_id, argument_digest, id, effect_kind) ON DELETE CASCADE;


--
-- Name: workflow_message_actions workflow_message_action_owner; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_actions
    ADD CONSTRAINT workflow_message_action_owner FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_message_actions workflow_message_action_wait; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_actions
    ADD CONSTRAINT workflow_message_action_wait FOREIGN KEY (company_id, wait_id) REFERENCES public.workflow_waits(company_id, id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_message_destinations workflow_message_destination_action; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destination_action FOREIGN KEY (company_id, invocation_id) REFERENCES public.workflow_message_actions(company_id, invocation_id) ON DELETE CASCADE;


--
-- Name: workflow_message_destinations workflow_message_destination_delivery; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destination_delivery FOREIGN KEY (company_id, delivery_id, message_id) REFERENCES public.message_deliveries(company_id, id, message_id);


--
-- Name: workflow_message_destinations workflow_message_destination_message; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destination_message FOREIGN KEY (company_id, message_id) REFERENCES public.messages(company_id, id);


--
-- Name: workflow_message_destinations workflow_message_destination_owner; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destination_owner FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_message_destinations workflow_message_destination_principal; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destination_principal FOREIGN KEY (company_id, principal_id) REFERENCES public.principals(company_id, id);


--
-- Name: workflow_message_destinations workflow_message_destination_thread; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_destinations
    ADD CONSTRAINT workflow_message_destination_thread FOREIGN KEY (company_id, thread_id) REFERENCES public.threads(company_id, id);


--
-- Name: workflow_message_delivery_events workflow_message_event_destination; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_delivery_events
    ADD CONSTRAINT workflow_message_event_destination FOREIGN KEY (company_id, invocation_id, ordinal, delivery_id) REFERENCES public.workflow_message_destinations(company_id, invocation_id, ordinal, delivery_id) ON DELETE CASCADE;


--
-- Name: workflow_message_delivery_events workflow_message_event_owner; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_delivery_events
    ADD CONSTRAINT workflow_message_event_owner FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_message_delivery_reopens workflow_message_reopen_destination; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_delivery_reopens
    ADD CONSTRAINT workflow_message_reopen_destination FOREIGN KEY (company_id, invocation_id, ordinal, delivery_id) REFERENCES public.workflow_message_destinations(company_id, invocation_id, ordinal, delivery_id) ON DELETE CASCADE;


--
-- Name: workflow_message_delivery_reopens workflow_message_reopen_evidence; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_delivery_reopens
    ADD CONSTRAINT workflow_message_reopen_evidence FOREIGN KEY (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, evidence_id) REFERENCES public.workflow_action_evidence(company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, id) ON DELETE CASCADE;


--
-- Name: workflow_message_delivery_reopens workflow_message_reopen_owner; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_delivery_reopens
    ADD CONSTRAINT workflow_message_reopen_owner FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_run_parents workflow_parent_execution_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_parents
    ADD CONSTRAINT workflow_parent_execution_fk FOREIGN KEY (company_id, parent_run_id, parent_execution_id) REFERENCES public.workflow_executions(company_id, run_id, id);


--
-- Name: workflow_run_parents workflow_parent_owner_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_parents
    ADD CONSTRAINT workflow_parent_owner_fk FOREIGN KEY (company_id, child_run_id, parent_run_id, parent_execution_id) REFERENCES public.workflow_runs(company_id, id, parent_run_id, parent_execution_id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_poll_companies workflow_poll_companies_worker_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_poll_companies
    ADD CONSTRAINT workflow_poll_companies_worker_id_fkey FOREIGN KEY (worker_id) REFERENCES public.workflow_poll_workers(worker_id) ON DELETE CASCADE;


--
-- Name: workflow_root_budget_usage workflow_root_budget_usage_company_id_root_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_root_budget_usage
    ADD CONSTRAINT workflow_root_budget_usage_company_id_root_run_id_fkey FOREIGN KEY (company_id, root_run_id) REFERENCES public.workflow_root_budgets(company_id, root_run_id) ON DELETE CASCADE;


--
-- Name: workflow_root_budgets workflow_root_budgets_company_id_root_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_root_budgets
    ADD CONSTRAINT workflow_root_budgets_company_id_root_run_id_fkey FOREIGN KEY (company_id, root_run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_run_budgets workflow_run_budgets_company_id_parent_run_id_root_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_budgets
    ADD CONSTRAINT workflow_run_budgets_company_id_parent_run_id_root_run_id_fkey FOREIGN KEY (company_id, parent_run_id, root_run_id) REFERENCES public.workflow_run_budgets(company_id, run_id, root_run_id);


--
-- Name: workflow_run_budgets workflow_run_budgets_company_id_root_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_budgets
    ADD CONSTRAINT workflow_run_budgets_company_id_root_run_id_fkey FOREIGN KEY (company_id, root_run_id) REFERENCES public.workflow_root_budgets(company_id, root_run_id);


--
-- Name: workflow_run_budgets workflow_run_budgets_company_id_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_budgets
    ADD CONSTRAINT workflow_run_budgets_company_id_run_id_fkey FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_run_budgets workflow_run_budgets_company_id_run_id_parent_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_budgets
    ADD CONSTRAINT workflow_run_budgets_company_id_run_id_parent_run_id_fkey FOREIGN KEY (company_id, run_id, parent_run_id) REFERENCES public.workflow_runs(company_id, id, parent_run_id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_run_events workflow_run_event_execution_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_events
    ADD CONSTRAINT workflow_run_event_execution_fk FOREIGN KEY (company_id, run_id, execution_id) REFERENCES public.workflow_executions(company_id, run_id, id);


--
-- Name: workflow_run_events workflow_run_events_company_id_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_events
    ADD CONSTRAINT workflow_run_events_company_id_run_id_fkey FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_runs workflow_run_parent_link_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_run_parent_link_fk FOREIGN KEY (company_id, id, parent_run_id, parent_execution_id) REFERENCES public.workflow_run_parents(company_id, child_run_id, parent_run_id, parent_execution_id);


--
-- Name: workflow_runs workflow_run_terminal_execution_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_run_terminal_execution_fk FOREIGN KEY (company_id, id, terminal_execution_id) REFERENCES public.workflow_executions(company_id, run_id, id);


--
-- Name: workflow_runs workflow_runs_company_id_channel_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_runs_company_id_channel_id_fkey FOREIGN KEY (company_id, channel_id) REFERENCES public.channels(company_id, id);


--
-- Name: workflow_runs workflow_runs_company_id_channel_id_thread_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_runs_company_id_channel_id_thread_id_fkey FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id);


--
-- Name: workflow_runs workflow_runs_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_runs_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: workflow_runs workflow_runs_company_id_workflow_id_version_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_runs_company_id_workflow_id_version_id_fkey FOREIGN KEY (company_id, workflow_id, version_id) REFERENCES public.workflow_versions(company_id, workflow_id, id);


--
-- Name: workflow_executions workflow_successor_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_executions
    ADD CONSTRAINT workflow_successor_fk FOREIGN KEY (company_id, run_id, successor_execution_id) REFERENCES public.workflow_executions(company_id, run_id, id);


--
-- Name: workflow_versions workflow_versions_company_id_workflow_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_versions
    ADD CONSTRAINT workflow_versions_company_id_workflow_id_fkey FOREIGN KEY (company_id, workflow_id) REFERENCES public.workflow_definitions(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_waits workflow_wait_consumed_event; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_waits
    ADD CONSTRAINT workflow_wait_consumed_event FOREIGN KEY (company_id, run_id, execution_id, consumed_event_id) REFERENCES public.workflow_wait_events(company_id, run_id, execution_id, id);


--
-- Name: workflow_wait_events workflow_wait_events_company_id_run_id_execution_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_wait_events
    ADD CONSTRAINT workflow_wait_events_company_id_run_id_execution_id_fkey FOREIGN KEY (company_id, run_id, execution_id) REFERENCES public.workflow_executions(company_id, run_id, id) ON DELETE CASCADE;


--
-- Name: workflow_waits workflow_waits_company_id_run_id_execution_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_waits
    ADD CONSTRAINT workflow_waits_company_id_run_id_execution_id_fkey FOREIGN KEY (company_id, run_id, execution_id) REFERENCES public.workflow_executions(company_id, run_id, id) ON DELETE CASCADE;


--
-- Name: workflow_run_events workflow_wakeup_execution_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_run_events
    ADD CONSTRAINT workflow_wakeup_execution_fk FOREIGN KEY (company_id, run_id, terminal_execution_id) REFERENCES public.workflow_executions(company_id, run_id, id);


--
-- PostgreSQL database dump complete
--

\unrestrict qFwGhfq2cOwzNkKiMUdK0K54svFLzmVHJd6sT7yjkUx8MzDwPwJCpMshLQof2hx

