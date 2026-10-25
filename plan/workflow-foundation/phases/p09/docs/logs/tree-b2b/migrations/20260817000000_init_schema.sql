-- Squashed baseline for a newly created database (through phase 04, 2026-10-18).
-- Pre-release: schema changes edit this file in place and existing databases are reset;
-- incremental upgrades and data backfills are intentionally unsupported.

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
        IF NEW.retry_count<>OLD.retry_count OR NEW.continuation_count<>OLD.continuation_count OR NEW.max_retries<>OLD.max_retries
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
        IF NEW.retry_count<>OLD.retry_count OR NEW.continuation_count<>OLD.continuation_count
            OR OLD.lock_expires_at IS NULL OR OLD.lock_expires_at <= clock_timestamp()
            OR NOT EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id = execution.company_id AND run.id = execution.run_id
                JOIN task_attempts AS attempt ON attempt.task_id = NEW.id
                    AND attempt.attempt_number = OLD.attempt_ordinal
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
                                    AND wait.reason = 'effect' AND wait.state = 'waiting'))
                        OR (attempt.stop_reason='approval_parked' AND execution.completed_at IS NULL
                            AND execution.committed_route IS NULL AND run.state='waiting' AND run.waiting_reason='decision'
                            AND OLD.continuation_count < 63
                            AND EXISTS (SELECT 1 FROM workflow_decisions AS decision
                                JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id
                                WHERE decision.company_id=execution.company_id AND decision.execution_id=execution.id
                                    AND decision.subject_kind IN ('action_approval','token_allowance') AND decision.submitted_at IS NULL
                                    AND decision.job_id=OLD.id AND decision.parked_attempt_number=OLD.attempt_ordinal
                                    AND wait.reason='decision' AND wait.state='waiting'))
                        -- A workflow tool call parks its agent job on the call's child (07-D2 §D2.5).
                        OR (attempt.stop_reason='child_parked' AND execution.completed_at IS NULL
                            AND execution.committed_route IS NULL AND run.state='waiting' AND run.waiting_reason='child_run'
                            AND OLD.continuation_count < 63
                            AND EXISTS (SELECT 1 FROM workflow_waits AS wait
                                WHERE wait.company_id=execution.company_id AND wait.execution_id=execution.id
                                    AND wait.reason='child_run' AND wait.state='waiting'
                                    AND wait.child_tool_call_id IS NOT NULL
                                    AND wait.parked_attempt_number=OLD.attempt_ordinal)))
            ) THEN
            RAISE EXCEPTION 'workflow completion lost ownership before commit' USING ERRCODE = '23514';
        END IF;
    END IF;
    IF OLD.queue_kind='workflow' AND OLD.status='completed' AND NEW.status='pending' THEN
        IF NEW.retry_count <> OLD.retry_count OR NEW.max_retries <> OLD.max_retries
            OR NEW.continuation_count <> OLD.continuation_count + 1 OR NEW.continuation_count > 63
            OR (NEW.company_id,NEW.workflow_execution_id) IS DISTINCT FROM (OLD.company_id,OLD.workflow_execution_id)
            OR NEW.worker_id IS NOT NULL OR NEW.execution_generation IS NOT NULL OR NEW.lock_expires_at IS NOT NULL
            OR NOT (EXISTS (
                -- A model-tool delivery continues its parked agent job: the exact action whose
                -- marker closed this attempt, its completed effect wait and its receipt.
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                JOIN workflow_message_actions AS action ON action.company_id=execution.company_id
                    AND action.execution_id=execution.id AND action.origin='model_tool'
                JOIN workflow_action_dispatches AS marker ON marker.company_id=action.company_id
                    AND marker.invocation_id=action.invocation_id AND marker.id=action.dispatch_id
                JOIN workflow_waits AS wait ON wait.company_id=action.company_id
                    AND wait.execution_id=action.execution_id AND wait.id=action.wait_id
                JOIN workflow_action_receipts AS receipt ON receipt.company_id=action.company_id
                    AND receipt.invocation_id=action.invocation_id
                JOIN task_attempts AS attempt ON attempt.task_id=OLD.id AND attempt.attempt_number=OLD.attempt_ordinal
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND execution.completed_at IS NULL AND execution.committed_route IS NULL
                    AND run.state='running' AND run.deadline>clock_timestamp()
                    AND attempt.status='completed' AND attempt.stop_reason='effect_parked'
                    AND marker.job_id=OLD.id AND marker.attempt_number=OLD.attempt_ordinal
                    AND wait.reason='effect' AND wait.state='completed'
            ) OR EXISTS (
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                JOIN workflow_decisions AS decision ON decision.company_id=execution.company_id AND decision.execution_id=execution.id
                JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id
                JOIN task_attempts AS attempt ON attempt.task_id=OLD.id AND attempt.attempt_number=OLD.attempt_ordinal
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND execution.completed_at IS NULL AND execution.committed_route IS NULL
                    AND run.state='running' AND run.deadline>clock_timestamp()
                    AND attempt.status='completed' AND attempt.stop_reason='approval_parked'
                    AND decision.subject_kind IN ('action_approval','token_allowance') AND decision.choice='approve'
                    AND (decision.subject_kind<>'token_allowance' OR NOT EXISTS(SELECT 1 FROM workflow_run_budgets AS link JOIN workflow_root_budget_usage AS usage USING(company_id,root_run_id) WHERE link.company_id=decision.company_id AND link.run_id=decision.run_id AND usage.token_refusal IS NOT NULL))
                    AND decision.job_id=OLD.id AND decision.parked_attempt_number=OLD.attempt_ordinal
                    AND decision.submitted_xact=pg_current_xact_id()
                    AND wait.reason='decision' AND wait.state='completed'
            ) OR EXISTS (
                -- A workflow tool call continues its parked agent job once the call's child is
                -- terminal and its wait completed (07-D2 §D2.4).
                SELECT 1 FROM workflow_executions AS execution
                JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
                JOIN workflow_waits AS wait ON wait.company_id=execution.company_id AND wait.execution_id=execution.id
                JOIN workflow_runs AS child ON child.company_id=wait.company_id AND child.id=wait.child_run_id
                JOIN task_attempts AS attempt ON attempt.task_id=OLD.id AND attempt.attempt_number=OLD.attempt_ordinal
                WHERE execution.company_id=NEW.company_id AND execution.id=NEW.workflow_execution_id
                    AND execution.completed_at IS NULL AND execution.committed_route IS NULL
                    AND run.state='running' AND run.deadline>clock_timestamp()
                    AND attempt.status='completed' AND attempt.stop_reason='child_parked'
                    AND wait.reason='child_run' AND wait.state='completed'
                    AND wait.child_tool_call_id IS NOT NULL AND wait.parked_attempt_number=OLD.attempt_ordinal
                    AND child.terminal_wakeup_sequence IS NOT NULL
            )) THEN
            RAISE EXCEPTION 'workflow job reopen requires an approval, delivery or child continuation' USING ERRCODE='23514';
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
            AND attempt_number=OLD.attempt_ordinal AND execution_generation=OLD.execution_generation
            AND worker_id=OLD.worker_id AND status='failed';
        IF NOT FOUND OR retirement.workflow_retirement IS NULL OR NEW.retry_count<>OLD.retry_count+1
            OR NEW.continuation_count<>OLD.continuation_count
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
DECLARE source jsonb; parent_run uuid; parent_execution uuid; ordinal numeric; call uuid;
BEGIN
    IF TG_OP='UPDATE' AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow admission identity is immutable' USING ERRCODE='23514';
    END IF;
    source := substring(NEW.source_key FROM 4)::jsonb;
    SELECT run.parent_run_id,run.parent_execution_id INTO parent_run,parent_execution
      FROM workflow_runs AS run WHERE run.company_id=NEW.company_id AND run.id=NEW.run_id;
    IF source->>0='child' THEN
        -- Runtime children: a step round [child, run, execution, step, null, ordinal] (07-D1
        -- §D1.2) or a workflow tool call [child, run, execution, step, call, null] (07-D2 §D2.3).
        IF jsonb_typeof(source->5)='number' THEN
            ordinal := (source->>5)::numeric;
        END IF;
        IF jsonb_typeof(source->4)='string'
           AND source->>4 ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
            call := (source->>4)::uuid;
        END IF;
        IF jsonb_array_length(source)<>6
           OR NOT ((ordinal IS NOT NULL AND source->4='null'::jsonb)
               OR (call IS NOT NULL AND source->5='null'::jsonb))
           OR (ordinal IS NOT NULL AND (ordinal<1 OR ordinal>1000 OR ordinal<>trunc(ordinal)))
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
    IF (NEW.notification_intent IS NOT NULL OR NEW.reason='decision') AND
       (TG_OP='INSERT' OR (OLD.state='waiting' AND NEW.state='completed')) THEN
        -- An event recorded by the wait's deadline still completes it after the deadline
        -- (p08 D1.5.6); the run's own deadline is never extended.
        IF EXISTS (SELECT 1 FROM workflow_runs AS run WHERE run.company_id=NEW.company_id
            AND run.id=NEW.run_id AND run.deadline<=clock_timestamp()) OR
           (NEW.reason='decision' AND NEW.deadline<=clock_timestamp()) OR
           (NEW.reason='event' AND NEW.deadline<=clock_timestamp() AND NOT EXISTS (
               SELECT 1 FROM workflow_wait_events AS event WHERE event.company_id=NEW.company_id
                 AND event.run_id=NEW.run_id AND event.id=NEW.consumed_event_id
                 AND event.created_at<=NEW.deadline)) THEN
            RAISE EXCEPTION 'workflow wait deadline reached before commit' USING ERRCODE='23514';
        END IF;
    END IF;
    -- A run-scoped event is consumed by exactly the wait that records it (05-D2 §11).
    IF NEW.consumed_event_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM workflow_wait_events AS event
        WHERE event.company_id=NEW.company_id AND event.run_id=NEW.run_id AND event.id=NEW.consumed_event_id
            AND event.consumed_execution_id=NEW.execution_id) THEN
        RAISE EXCEPTION 'workflow wait consumed an event it does not own' USING ERRCODE='23514';
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
    NEW.disposition=CASE WHEN budget.provenance='frozen_v6' AND NEW.quantity>0
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
    IF (NEW.company_id, NEW.run_id, NEW.id, NEW.event_name, NEW.correlation, NEW.payload, NEW.created_at)
            IS DISTINCT FROM (OLD.company_id, OLD.run_id, OLD.id, OLD.event_name, OLD.correlation, OLD.payload, OLD.created_at)
        OR (OLD.consumed_execution_id IS NOT NULL AND NEW.consumed_execution_id IS DISTINCT FROM OLD.consumed_execution_id)
        OR (OLD.consumed_execution_id IS NULL AND NEW.consumed_execution_id IS NULL) THEN
        RAISE EXCEPTION 'workflow event facts are immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


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
            IF stored->>'compiler_revision'='6' THEN
                SELECT version->'compiled'->'root_budget' INTO limits
                    FROM jsonb_array_elements(stored->'versions') AS version
                    WHERE version->>'version'=stored->>'root';
            END IF;
        EXCEPTION WHEN data_exception THEN
            limits=NULL;
        END;
        INSERT INTO workflow_root_budgets(company_id,root_run_id,provenance,activations,model_calls,repetitions,token_policy)
            VALUES(NEW.company_id,root,CASE WHEN limits IS NULL THEN 'sealed_legacy' ELSE 'frozen_v6' END,
                (limits->>'activations')::integer,(limits->>'model_calls')::integer,(limits->>'repetitions')::integer,limits->'tokens');
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
-- Name: valid_creation_provenance(jsonb); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_creation_provenance(provenance jsonb) RETURNS boolean
    LANGUAGE sql IMMUTABLE
    RETURN ((jsonb_typeof(provenance) = 'object'::text) AND ((provenance - ARRAY['actor_type'::text, 'actor_id'::text, 'actor_name'::text]) = '{}'::jsonb) AND (jsonb_typeof((provenance -> 'actor_type'::text)) = 'string'::text) AND ((provenance ->> 'actor_type'::text) = ANY (ARRAY['user'::text, 'system'::text])) AND (jsonb_typeof((provenance -> 'actor_name'::text)) = 'string'::text) AND (btrim((provenance ->> 'actor_name'::text)) <> ''::text) AND CASE (provenance ->> 'actor_type'::text) WHEN 'system'::text THEN ((provenance ? 'actor_id'::text) AND ((provenance -> 'actor_id'::text) = 'null'::jsonb)) WHEN 'user'::text THEN ((jsonb_typeof((provenance -> 'actor_id'::text)) = 'string'::text) AND ((provenance ->> 'actor_id'::text) ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'::text)) ELSE false END);


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
    description text,
    settings_revision bigint DEFAULT 0 NOT NULL,
    CONSTRAINT channels_access_mode_check CHECK ((access_mode = ANY (ARRAY['team'::text, 'allowlist'::text, 'public'::text]))),
    CONSTRAINT channels_created_by_shape_check CHECK (public.valid_creation_provenance(created_by)),
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
-- Name: notify_attention_changed(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_attention_changed() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    row_data JSONB := CASE WHEN TG_OP = 'DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END;
    source_id UUID := NULLIF(row_data->>'id', '')::UUID;
    company UUID := NULLIF(row_data->>'company_id', '')::UUID;
    channel UUID := NULLIF(row_data->>'channel_id', '')::UUID;
BEGIN
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
        IF NEW.activations<>0 OR NEW.model_calls<>0 OR NEW.repetitions<>0 OR NEW.authorized_tokens<>0 OR NEW.token_refusal IS NOT NULL THEN
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
        OR NEW.authorized_tokens<OLD.authorized_tokens
        OR (OLD.token_refusal IS NOT NULL AND NEW.token_refusal IS DISTINCT FROM OLD.token_refusal)
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
-- Name: check_workflow_child_parent_wait(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workflow_child_parent_wait() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- One creation path (07-D1 §D1.2a, §D1.3): a child run exists only with the child_run wait
    -- of its parent execution, written in the same transaction by the runtime child writer.
    -- The wait names the same round or saved call as the child's admission (07-D2 §D2.5).
    IF NOT EXISTS (SELECT 1 FROM workflow_waits AS wait
        JOIN workflow_admissions AS admission ON admission.company_id=wait.company_id
          AND admission.run_id=wait.child_run_id
        WHERE wait.company_id=NEW.company_id AND wait.child_run_id=NEW.id
          AND wait.run_id=NEW.parent_run_id AND wait.execution_id=NEW.parent_execution_id
          AND wait.reason='child_run'
          AND substring(admission.source_key FROM 4)::jsonb->4
              = COALESCE(to_jsonb(wait.child_tool_call_id::text),'null'::jsonb)
          AND substring(admission.source_key FROM 4)::jsonb->5
              = COALESCE(to_jsonb(wait.child_ordinal),'null'::jsonb)) THEN
        RAISE EXCEPTION 'Workflow child run has no parent child_run wait' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END $$;


--
-- Name: check_workflow_rerun_original(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.check_workflow_rerun_original() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- A rerun links a top-level original (07-D1 §D1.7). Lineage is immutable, so insert suffices.
    IF NEW.rerun_of_run_id IS NOT NULL AND EXISTS (SELECT 1 FROM workflow_runs AS original
        WHERE original.company_id=NEW.company_id AND original.id=NEW.rerun_of_run_id
          AND original.parent_run_id IS NOT NULL) THEN
        RAISE EXCEPTION 'Workflow rerun original is not a top-level run' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


--
-- Name: preserve_workflow_parent(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.preserve_workflow_parent() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.parent_run_id,NEW.parent_execution_id,NEW.rerun_of_run_id) IS DISTINCT FROM
       (OLD.parent_run_id,OLD.parent_execution_id,OLD.rerun_of_run_id) THEN
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
-- A native checkpoint keeps its owner and prepared inputs, and writes only under a live workflow fence.
CREATE FUNCTION public.require_harness_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND
        ROW(NEW.id,NEW.company_id,NEW.task_id,NEW.agent_id,NEW.owner_kind,NEW.workflow_run_id,
            NEW.workflow_execution_id,NEW.workflow_prepared)
        IS DISTINCT FROM
        ROW(OLD.id,OLD.company_id,OLD.task_id,OLD.agent_id,OLD.owner_kind,OLD.workflow_run_id,
            OLD.workflow_execution_id,OLD.workflow_prepared) THEN
        RAISE EXCEPTION 'harness owner and prepared inputs are immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.owner_kind <> 'workflow' THEN
        RAISE EXCEPTION 'unknown harness owner' USING ERRCODE='23514';
    END IF;
    IF NEW.checkpoint->>'run_id' IS DISTINCT FROM NEW.id::text
        OR NEW.checkpoint->'identity'->>'company_id' IS DISTINCT FROM NEW.company_id::text
        OR NEW.checkpoint->'identity'->>'task_id' IS DISTINCT FROM NEW.task_id::text
        OR NEW.checkpoint->'identity'->>'agent_id' IS DISTINCT FROM NEW.agent_id::text THEN
        RAISE EXCEPTION 'native checkpoint identity is required' USING ERRCODE='23514';
    END IF;
    IF TG_OP = 'UPDATE' THEN
        IF NEW.revision <> OLD.revision + 1
            OR NEW.checkpoint->'identity' IS DISTINCT FROM OLD.checkpoint->'identity'
            OR NEW.checkpoint->'policy' IS DISTINCT FROM OLD.checkpoint->'policy' THEN
            RAISE EXCEPTION 'native checkpoint revision or identity changed' USING ERRCODE='23514';
        END IF;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM public.background_tasks AS job
        JOIN public.workflow_executions AS execution
            ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id
        JOIN public.workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id
        JOIN public.task_attempts AS attempt ON attempt.task_id=job.id
            AND attempt.attempt_number=NEW.workflow_attempt
            AND attempt.execution_generation=NEW.workflow_generation
            AND attempt.worker_id=NEW.workflow_worker_id AND attempt.status='processing'
        WHERE job.company_id=NEW.company_id AND job.id=NEW.task_id AND job.queue_kind='workflow'
            AND execution.id=NEW.workflow_execution_id AND run.id=NEW.workflow_run_id
            AND job.status='processing' AND job.worker_id=NEW.workflow_worker_id
            AND job.execution_generation=NEW.workflow_generation AND job.attempt_ordinal=NEW.workflow_attempt
            AND job.lock_expires_at>clock_timestamp() AND execution.completed_at IS NULL
            AND run.state IN ('queued','running') AND run.deadline>clock_timestamp()
    ) THEN
        RAISE EXCEPTION 'native checkpoint requires live workflow fence' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;

-- Model ownership is separate from the most recent checkpoint writer.
CREATE FUNCTION public.guard_harness_model_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.owner_kind<>'workflow' THEN RETURN NEW; END IF;
    IF TG_OP='INSERT' AND (jsonb_array_length(NEW.checkpoint->'reservations')<>0
        OR jsonb_array_length(NEW.checkpoint->'turns')<>0 OR NEW.model_request_id IS NOT NULL) THEN
        RAISE EXCEPTION 'native checkpoint must begin without model work' USING ERRCODE='23514';
    END IF;
    IF TG_OP='UPDATE' AND NEW.checkpoint->'reservations' IS DISTINCT FROM OLD.checkpoint->'reservations'
        AND (jsonb_array_length(NEW.checkpoint->'reservations')=jsonb_array_length(OLD.checkpoint->'reservations')+1
            AND (NEW.checkpoint->'reservations') - jsonb_array_length(OLD.checkpoint->'reservations') = OLD.checkpoint->'reservations'
            AND NEW.model_request_id IS NOT NULL AND OLD.model_request_id IS NULL
            AND NEW.checkpoint->'reservations'->-1->>'request_id'=NEW.model_request_id::text) IS NOT TRUE THEN
        RAISE EXCEPTION 'new model reservation requires fresh active ownership' USING ERRCODE='23514';
    END IF;
    IF TG_OP='UPDATE' AND NEW.checkpoint->'turns' IS DISTINCT FROM OLD.checkpoint->'turns'
        AND (jsonb_array_length(NEW.checkpoint->'turns')=jsonb_array_length(OLD.checkpoint->'turns')+1
            AND (NEW.checkpoint->'turns') - jsonb_array_length(OLD.checkpoint->'turns') = OLD.checkpoint->'turns'
            AND OLD.model_request_id IS NOT NULL AND NEW.model_request_id IS NULL
            AND NEW.checkpoint->'turns'->-1->>'request_id'=OLD.model_request_id::text
            AND ROW(OLD.model_generation,OLD.model_worker_id,OLD.model_attempt)
                = ROW(NEW.workflow_generation,NEW.workflow_worker_id,NEW.workflow_attempt)) IS NOT TRUE THEN
        RAISE EXCEPTION 'model settlement requires exact admitting owner' USING ERRCODE='23514';
    END IF;
    IF NEW.model_request_id IS NOT NULL THEN
        IF NOT EXISTS (SELECT 1 FROM jsonb_array_elements(NEW.checkpoint->'reservations') AS reservation
            WHERE reservation->>'request_id'=NEW.model_request_id::text)
            OR EXISTS (SELECT 1 FROM jsonb_array_elements(NEW.checkpoint->'turns') AS turn
                WHERE turn->>'request_id'=NEW.model_request_id::text) THEN
            RAISE EXCEPTION 'active model owner requires unresolved reservation' USING ERRCODE='23514';
        END IF;
        IF TG_OP='INSERT' OR ROW(NEW.model_request_id,NEW.model_generation,NEW.model_worker_id,NEW.model_attempt)
            IS DISTINCT FROM ROW(OLD.model_request_id,OLD.model_generation,OLD.model_worker_id,OLD.model_attempt) THEN
            IF ROW(NEW.model_generation,NEW.model_worker_id,NEW.model_attempt)
                IS DISTINCT FROM ROW(NEW.workflow_generation,NEW.workflow_worker_id,NEW.workflow_attempt)
                OR (TG_OP='UPDATE' AND EXISTS (
                    SELECT 1 FROM jsonb_array_elements(OLD.checkpoint->'reservations') AS reservation
                    WHERE reservation->>'request_id'=NEW.model_request_id::text)) THEN
                RAISE EXCEPTION 'model admission requires fresh request and current fence' USING ERRCODE='23514';
            END IF;
        END IF;
    END IF;
    IF TG_OP='UPDATE' AND OLD.model_request_id IS NOT NULL
        AND ROW(NEW.model_request_id,NEW.model_generation,NEW.model_worker_id,NEW.model_attempt)
            IS DISTINCT FROM ROW(OLD.model_request_id,OLD.model_generation,OLD.model_worker_id,OLD.model_attempt)
        AND NOT (NEW.model_request_id IS NULL AND (
            (ROW(OLD.model_generation,OLD.model_worker_id,OLD.model_attempt)
                = ROW(NEW.workflow_generation,NEW.workflow_worker_id,NEW.workflow_attempt)
                AND EXISTS (SELECT 1 FROM jsonb_array_elements(NEW.checkpoint->'turns') AS turn
                    WHERE turn->>'request_id'=OLD.model_request_id::text))
            OR (NEW.workflow_generation<>OLD.model_generation AND NEW.workflow_attempt>OLD.model_attempt))) THEN
        RAISE EXCEPTION 'active model owner must settle or retire before replacement' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;

--
-- Name: require_schedule_occurrence_scope(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.require_schedule_occurrence_scope() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE owner_company uuid; owner_channel uuid; owner_binding uuid;
BEGIN
    IF TG_OP = 'INSERT' THEN
        -- SHARE also conflicts with a non-key schedule UPDATE: either we capture
        -- before a retarget or observe its committed scope, never a mixed snapshot.
        SELECT schedule.company_id, schedule.channel_id, schedule.binding_id
        INTO owner_company, owner_channel, owner_binding
        FROM public.channel_schedules AS schedule WHERE schedule.id = NEW.schedule_id
        FOR SHARE;
        IF owner_company IS NULL
           OR (NEW.company_id IS NOT NULL AND NEW.company_id <> owner_company)
           OR (NEW.channel_id IS NOT NULL AND NEW.channel_id <> owner_channel)
           OR (NEW.binding_id IS NOT NULL AND NEW.binding_id <> owner_binding) THEN
            RAISE EXCEPTION 'Schedule occurrence requires its actual owner scope at capture'
                USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_owner_scope';
        END IF;
        NEW.company_id := owner_company;
        NEW.channel_id := owner_channel;
        NEW.binding_id := owner_binding;
    ELSE
        IF (NEW.company_id, NEW.channel_id, NEW.binding_id, NEW.schedule_id, NEW.schedule_snapshot,
            NEW.scheduled_for, NEW.command_key)
            IS DISTINCT FROM
           (OLD.company_id, OLD.channel_id, OLD.binding_id, OLD.schedule_id, OLD.schedule_snapshot,
            OLD.scheduled_for, OLD.command_key) THEN
            RAISE EXCEPTION 'Captured schedule occurrence identity is immutable'
                USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_identity_immutable';
        END IF;
        IF OLD.run_id IS NOT NULL AND NEW.run_id IS DISTINCT FROM OLD.run_id THEN
            RAISE EXCEPTION 'A materialized schedule occurrence keeps its run'
                USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_run_immutable';
        END IF;
        IF NEW.run_id IS NOT DISTINCT FROM OLD.run_id THEN
            RETURN NEW;
        END IF;
    END IF;
    -- Incomplete snapshots already fail the application decoder. When an identity
    -- is present it must agree with the relational occurrence, including raw SQL.
    IF (NEW.schedule_snapshot ? 'id' AND NEW.schedule_snapshot->>'id' IS DISTINCT FROM NEW.schedule_id::text)
       OR (NEW.schedule_snapshot ? 'company_id' AND NEW.schedule_snapshot->>'company_id' IS DISTINCT FROM NEW.company_id::text)
       OR (NEW.schedule_snapshot ? 'channel_id' AND NEW.schedule_snapshot->>'channel_id' IS DISTINCT FROM NEW.channel_id::text)
       OR (NEW.schedule_snapshot ? 'binding_id' AND NEW.schedule_snapshot->>'binding_id' IS DISTINCT FROM NEW.binding_id::text) THEN
        RAISE EXCEPTION 'Schedule snapshot identity must match captured occurrence scope'
            USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_snapshot_scope';
    END IF;
    -- The run must be the one admitted for this occurrence: its trigger is the occurrence and
    -- its association the occurrence's channel (the binding is the composite foreign key).
    IF NEW.run_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.workflow_runs AS run
        WHERE run.company_id = NEW.company_id AND run.id = NEW.run_id
          AND run.trigger_id = NEW.id AND run.channel_id = NEW.channel_id AND run.thread_id IS NULL
    ) THEN
        RAISE EXCEPTION 'Schedule result requires the run admitted for this occurrence'
            USING ERRCODE = '23514', CONSTRAINT = 'schedule_occurrence_run_scope';
    END IF;
    RETURN NEW;
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
    RETURN (class = ANY (ARRAY['decode'::text, 'invalid_payload'::text, 'routing'::text, 'dependency'::text, 'rate_limited'::text, 'provider_fault'::text, 'deadline'::text, 'internal'::text, 'unsupported_transport'::text, 'lease_expired'::text, 'superseded'::text]));


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
-- Name: valid_workflow_type_names(text[]); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.valid_workflow_type_names(names text[]) RETURNS boolean
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
    AS $$
    -- Mirrors domain::workflow::TypeName: dotted parts [A-Za-z_][A-Za-z0-9_-]*, <=128 bytes;
    -- ordered, unique, at most 32 (agent workflow tool grants, skill required tools).
    SELECT cardinality(names) <= 32
       AND array_position(names, NULL) IS NULL
       AND (SELECT count(DISTINCT name) FROM unnest(names) AS name) = cardinality(names)
       AND COALESCE(bool_and(octet_length(name) BETWEEN 1 AND 128
           AND name ~ '^[A-Za-z_][A-Za-z0-9_-]*([.][A-Za-z_][A-Za-z0-9_-]*)*$'), TRUE)
    FROM unnest(names) AS name;
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
            VALUES(OLD.company_id,run,OLD.workflow_execution_id,OLD.id,OLD.attempt_ordinal-1,pg_current_xact_id());
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
                AND job.status='failed' AND job.queue_kind='workflow' AND job.workflow_execution_id=NEW.execution_id AND job.attempt_ordinal-1=NEW.retired_attempt
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
            AND job.attempt_ordinal-1=NEW.retired_attempt AND job.status='pending'
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
                AND witness.retired_attempt=OLD.attempt_ordinal-1 AND witness.transaction_id=pg_current_xact_id()
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
            IF NOT has_delivery OR action.fence_expires_at<=clock_timestamp()
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
        IF has_delivery THEN
            RAISE EXCEPTION 'workflow message with deliveries must park';
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
          AND job.attempt_ordinal=NEW.attempt_number AND job.lock_expires_at>clock_timestamp()
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
            AND task.queue_kind='workflow' AND task.status='pending' AND task.attempt_ordinal-1=episode.retired_attempt
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
        WHERE link.company_id=company AND link.run_id=run AND budget.provenance='frozen_v6'
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
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.attempt_ordinal-1
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
            AND job.attempt_ordinal=NEW.attempt_number AND job.lock_expires_at>clock_timestamp()
            AND attempt.attempt_number=NEW.attempt_number AND attempt.worker_id=NEW.worker_id
            AND attempt.execution_generation=NEW.execution_generation AND attempt.status='processing')
    THEN RAISE EXCEPTION 'workflow action remote entry lost live fence'; END IF;
    SELECT * INTO marker FROM workflow_action_dispatches WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id;
    proof:=workflow_action_not_applied_available(NEW.company_id,NEW.invocation_id,NEW.id);
    IF workflow_action_has_evidence_conflict(NEW.company_id,NEW.execution_id)
        OR (marker.current_policy->'approval_required'='true'::jsonb
            AND EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
                WHERE entry.company_id=NEW.company_id AND entry.invocation_id=NEW.invocation_id AND entry.id<>NEW.id))
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
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.attempt_ordinal-1
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
-- Name: workflow_decision_fact_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_decision_fact_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    RAISE EXCEPTION 'workflow decision facts are immutable' USING ERRCODE='23514';
END $$;


--
-- Name: workflow_decision_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_decision_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- The only transition: one submission recorded once, bumping the revision (05-D2 §5.1).
    IF OLD.submitted_at IS NULL AND NEW.submitted_at IS NOT NULL AND NEW.revision = OLD.revision + 1
        AND (to_jsonb(NEW)-ARRAY['submitted_at','submitted_by','submitted_xact','command_key','command_fingerprint','choice','feedback','data','revision'])
            IS NOT DISTINCT FROM (to_jsonb(OLD)-ARRAY['submitted_at','submitted_by','submitted_xact','command_key','command_fingerprint','choice','feedback','data','revision']) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'workflow decision facts are immutable' USING ERRCODE='23514';
END $$;


--
-- Name: workflow_decision_reviewer_guard(); Type: FUNCTION; Schema: public; Owner: -
--

-- Historical identities are checked and locked only when first recorded (05-D5/D6).
CREATE FUNCTION public.workflow_decision_person_snapshot(company uuid, principal_id uuid) RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE snapshot_user uuid;
BEGIN
    SELECT principal.user_id INTO snapshot_user FROM principals AS principal
        WHERE principal.company_id=company AND principal.id=principal_id AND principal.kind='person'
        FOR SHARE;
    IF snapshot_user IS NULL THEN
        RAISE EXCEPTION 'workflow decision identity is not a current tenant person' USING ERRCODE='23514';
    END IF;
    RETURN snapshot_user;
END $$;

CREATE FUNCTION public.workflow_decision_comment_identity() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM workflow_decision_person_snapshot(NEW.company_id, NEW.author_principal_id);
    RETURN NEW;
END $$;

CREATE FUNCTION public.workflow_decision_submit_identity() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.submitted_by IS NOT NULL THEN
        PERFORM workflow_decision_person_snapshot(NEW.company_id, NEW.submitted_by);
        NEW.submitted_xact := pg_current_xact_id();
    END IF;
    RETURN NEW;
END $$;

CREATE FUNCTION public.workflow_decision_reviewer_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE snapshot_user uuid;
BEGIN
    snapshot_user := workflow_decision_person_snapshot(NEW.company_id, NEW.principal_id);
    IF NEW.user_id IS NOT NULL AND NEW.user_id <> snapshot_user THEN
        RAISE EXCEPTION 'workflow decision reviewer user snapshot mismatches principal' USING ERRCODE='23514';
    END IF;
    NEW.user_id := snapshot_user;
    -- Every reviewer was provably notified, or provably opted out of decision email (P08-D4 F076).
    IF NEW.notice_opted_out THEN
        IF NOT EXISTS (SELECT 1 FROM user_notification_preferences AS preference
                WHERE preference.user_id=snapshot_user AND NOT preference.decision_email_enabled) THEN
            RAISE EXCEPTION 'workflow decision reviewer opt-out is not recorded' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM message_deliveries AS delivery WHERE delivery.id=NEW.notice_delivery_id
            AND delivery.purpose='notification' AND delivery.company_id IS NULL AND delivery.channel_id IS NULL
            AND delivery.message_id IS NULL AND delivery.source_binding_id IS NULL
            AND delivery.destination_binding_id IS NULL AND delivery.external_destination IS NOT NULL
            AND starts_with(delivery.idempotency_key, 'notification:workflow-decision:' || NEW.decision_id
                || ':principal:' || NEW.principal_id || ':')) THEN
        RAISE EXCEPTION 'workflow decision reviewer lacks a provable member notice' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;


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
        AND job.attempt_ordinal=marker.attempt_number AND job.lock_expires_at>clock_timestamp()
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
-- Name: workflow_message_destination_in_app_guard(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.workflow_message_destination_in_app_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- A member destination is emailed unless its run publishes in-app only (p08 D1.7).
    IF NEW.kind='user' AND NEW.delivery_id IS NULL AND NOT EXISTS(
        SELECT 1 FROM workflow_runs AS run
        WHERE run.company_id=NEW.company_id AND run.id=NEW.run_id AND run.delivery_policy='in_app_only'
    ) THEN
        RAISE EXCEPTION 'workflow user destination requires a delivery unless its run is in-app only'
            USING ERRCODE='23514';
    END IF;
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

CREATE FUNCTION public.workflow_tool_child_wait_continuation_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- 07-D2 §D2.5: a workflow tool call's child wait only continues its agent job; at commit the
    -- execution is still open and its job is pending again.
    IF NOT EXISTS(SELECT 1 FROM workflow_executions AS execution
            JOIN background_tasks AS job ON job.company_id=execution.company_id
                AND job.workflow_execution_id=execution.id
            WHERE execution.company_id=NEW.company_id AND execution.id=NEW.execution_id
              AND execution.completed_at IS NULL AND execution.committed_route IS NULL
              AND job.status='pending')
    THEN RAISE EXCEPTION 'workflow tool child wait only continues its agent job' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;


CREATE FUNCTION public.workflow_model_tool_wait_continuation_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- 06-D4: a saved model tool call's delivery wait only continues its agent job; at commit
    -- the execution is still open and its job is pending again. Step waits complete the step.
    IF EXISTS(SELECT 1 FROM workflow_message_actions AS action
            WHERE action.company_id=NEW.company_id AND action.execution_id=NEW.execution_id
              AND action.wait_id=NEW.id AND action.origin='model_tool')
       AND NOT EXISTS(SELECT 1 FROM workflow_executions AS execution
            JOIN background_tasks AS job ON job.company_id=execution.company_id
                AND job.workflow_execution_id=execution.id
            WHERE execution.company_id=NEW.company_id AND execution.id=NEW.execution_id
              AND execution.completed_at IS NULL AND execution.committed_route IS NULL
              AND job.status='pending')
    THEN RAISE EXCEPTION 'model-tool delivery only continues its agent job' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END $$;


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
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    avatar_url text,
    created_by jsonb NOT NULL,
    -- Ordered workflow tool names this agent grants (06-D5).
    workflow_tools text[] DEFAULT '{}'::text[] NOT NULL CONSTRAINT agents_workflow_tools_shape CHECK (public.valid_workflow_type_names(workflow_tools)),
    CONSTRAINT agents_avatar_url_scheme_check CHECK (((avatar_url IS NULL) OR (avatar_url ~ '^https?://'::text))),
    CONSTRAINT agents_created_by_shape_check CHECK (public.valid_creation_provenance(created_by)),
    CONSTRAINT agents_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT agents_slug_format CHECK ((((slug)::text = lower((slug)::text)) AND ((slug)::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'::text)))
);


--
-- Name: background_tasks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.background_tasks (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    channel_id uuid,
    thread_id uuid,
    correlation_id uuid NOT NULL,
    task_type text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    retry_count integer DEFAULT 0 NOT NULL,
    continuation_count smallint DEFAULT 0 NOT NULL,
    attempt_ordinal integer GENERATED ALWAYS AS (retry_count + continuation_count + 1) STORED,
    max_retries integer DEFAULT 3 NOT NULL,
    last_error text,
    worker_id uuid,
    execution_generation uuid,
    locked_at timestamp with time zone,
    lock_expires_at timestamp with time zone,
    run_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    queue_kind text DEFAULT 'workflow'::text NOT NULL,
    workflow_execution_id uuid,
    CONSTRAINT background_tasks_lease_check CHECK ((((status = 'processing'::text) AND (worker_id IS NOT NULL) AND (execution_generation IS NOT NULL) AND (locked_at IS NOT NULL) AND (lock_expires_at IS NOT NULL) AND (lock_expires_at > locked_at)) OR ((status <> 'processing'::text) AND (worker_id IS NULL) AND (execution_generation IS NULL) AND (locked_at IS NULL) AND (lock_expires_at IS NULL)))),
    CONSTRAINT background_tasks_max_retries_check CHECK ((max_retries > 0)),
    CONSTRAINT background_tasks_payload_object_check CHECK ((jsonb_typeof(payload) = 'object'::text)),
    CONSTRAINT background_tasks_queue_kind_check CHECK ((queue_kind = 'workflow'::text)),
    CONSTRAINT background_tasks_retry_count_check CHECK ((retry_count >= 0)),
    CONSTRAINT background_tasks_continuation_count_check CHECK (continuation_count BETWEEN 0 AND 63),
    CONSTRAINT background_tasks_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'processing'::text, 'completed'::text, 'failed'::text]))),
    CONSTRAINT background_tasks_workflow_shape_check CHECK (((workflow_execution_id IS NOT NULL) AND (task_type = 'workflow_execution'::text) AND (payload = jsonb_build_object('version', 1, 'execution_id', (workflow_execution_id)::text))))
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
    binding_id uuid NOT NULL,
    name text NOT NULL,
    schedule_type text NOT NULL,
    interval_seconds bigint,
    timezone text DEFAULT 'UTC'::text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    last_run_at timestamp with time zone,
    next_run_at timestamp with time zone,
    last_error text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
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
    default_add_3rd_party boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    avatar_url text,
    CONSTRAINT companies_avatar_url_scheme_check CHECK (((avatar_url IS NULL) OR (avatar_url ~ '^https?://'::text))),
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
    CONSTRAINT message_deliveries_attribution_check CHECK ((((company_id IS NOT NULL) AND (channel_id IS NOT NULL) AND (message_id IS NOT NULL) AND (source_binding_id IS NOT NULL) AND (destination_binding_id IS NOT NULL)) OR ((company_id IS NULL) AND (channel_id IS NULL) AND (message_id IS NULL) AND (source_binding_id IS NULL) AND (destination_binding_id IS NULL) AND (depends_on_delivery_id IS NULL) AND (external_destination IS NOT NULL) AND (purpose = 'notification'::text)))),
    CONSTRAINT message_deliveries_cancellation_check CHECK ((((cancellation_requested_at IS NULL) = (cancellation_reason IS NULL)) AND ((cancellation_reason IS NULL) OR (cancellation_reason = ANY (ARRAY['workflow_cancelled'::text, 'workflow_expired'::text, 'workflow_revoked'::text, 'recipient_revoked'::text, 'evidence_conflict'::text]))) AND ((cancellation_requested_at IS NULL) OR (status <> ALL (ARRAY['pending'::text, 'retryable'::text]))))),
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
    CONSTRAINT notification_events_action_kind_check CHECK ((action_kind = 'delivery_failure'::text)),
    CONSTRAINT notification_events_attempt_check CHECK (((attempt_count >= 0) AND (max_attempts > 0) AND (attempt_count <= max_attempts))),
    CONSTRAINT notification_events_error_check CHECK (((last_error_class IS NULL) OR (last_error_class = ANY (ARRAY['database'::text, 'composition'::text, 'invalid_source'::text, 'lease_expired'::text, 'internal'::text])))),
    CONSTRAINT notification_events_lease_check CHECK ((((status = 'processing'::text) AND (execution_id IS NOT NULL) AND (owner_worker_id IS NOT NULL) AND (locked_at IS NOT NULL) AND (lock_expires_at IS NOT NULL) AND (lock_expires_at > locked_at)) OR ((status <> 'processing'::text) AND (execution_id IS NULL) AND (owner_worker_id IS NULL) AND (locked_at IS NULL) AND (lock_expires_at IS NULL)))),
    CONSTRAINT notification_events_projection_check CHECK (((status = 'projected'::text) = (projected_at IS NOT NULL))),
    CONSTRAINT notification_events_source_generation_check CHECK ((source_generation > 0)),
    CONSTRAINT notification_events_source_kind_check CHECK ((source_kind = 'delivery'::text)),
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
    CONSTRAINT notifications_action_kind_check CHECK ((action_kind = 'delivery_failure'::text)),
    CONSTRAINT notifications_source_generation_check CHECK ((source_generation > 0)),
    CONSTRAINT notifications_source_kind_check CHECK ((source_kind = 'delivery'::text)),
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
    active_workflow_jobs integer DEFAULT 0 NOT NULL,
    workflow_worker_slots integer DEFAULT 1 NOT NULL,
    CONSTRAINT runtime_metric_samples_acquire_duration_nonnegative CHECK ((database_acquire_duration_ms >= (0)::double precision)),
    CONSTRAINT runtime_metric_samples_active_jobs_nonnegative CHECK ((active_workflow_jobs >= 0)),
    CONSTRAINT runtime_metric_samples_active_jobs_within_slots CHECK ((active_workflow_jobs <= workflow_worker_slots)),
    CONSTRAINT runtime_metric_samples_cpu_steal_nonnegative CHECK (((cpu_steal_percent IS NULL) OR (cpu_steal_percent >= (0)::double precision))),
    CONSTRAINT runtime_metric_samples_cpu_throttle_nonnegative CHECK (((cpu_throttle_percent IS NULL) OR (cpu_throttle_percent >= (0)::double precision))),
    CONSTRAINT runtime_metric_samples_cpu_utilization_nonnegative CHECK (((cpu_utilization_percent IS NULL) OR (cpu_utilization_percent >= (0)::double precision))),
    CONSTRAINT runtime_metric_samples_memory_limit_nonnegative CHECK (((memory_limit_bytes IS NULL) OR (memory_limit_bytes >= 0))),
    CONSTRAINT runtime_metric_samples_pool_active_nonnegative CHECK ((pool_active >= 0)),
    CONSTRAINT runtime_metric_samples_pool_idle_nonnegative CHECK ((pool_idle >= 0)),
    CONSTRAINT runtime_metric_samples_pool_parts_fit CHECK (((pool_idle + pool_active) = pool_size)),
    CONSTRAINT runtime_metric_samples_pool_size_nonnegative CHECK ((pool_size >= 0)),
    CONSTRAINT runtime_metric_samples_rss_nonnegative CHECK (((process_rss_bytes IS NULL) OR (process_rss_bytes >= 0))),
    CONSTRAINT runtime_metric_samples_worker_slots_positive CHECK ((workflow_worker_slots > 0))
);


--
-- Name: schedule_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.schedule_runs (
    id uuid NOT NULL,
    schedule_id uuid NOT NULL,
    scheduled_for timestamp with time zone NOT NULL,
    schedule_snapshot jsonb NOT NULL,
    binding_id uuid NOT NULL,
    run_id uuid,
    command_key text,
    refusal text,
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
    CONSTRAINT schedule_runs_command_key_check CHECK (((command_key IS NULL) OR ((length(command_key) >= 1) AND (length(command_key) <= 200)))),
    CONSTRAINT schedule_runs_materialization_state_check CHECK ((((materialization_status = 'pending'::text) AND (run_id IS NULL) AND (refusal IS NULL) AND (materialization_attempts < 5) AND (materialization_worker_id IS NULL) AND (materialization_generation IS NULL) AND (materialization_locked_at IS NULL) AND (materialization_lock_expires_at IS NULL)) OR ((materialization_status = 'materializing'::text) AND (run_id IS NULL) AND (refusal IS NULL) AND ((materialization_attempts >= 1) AND (materialization_attempts <= 5)) AND (materialization_worker_id IS NOT NULL) AND (materialization_generation IS NOT NULL) AND (materialization_locked_at IS NOT NULL) AND (materialization_lock_expires_at IS NOT NULL) AND (materialization_lock_expires_at > materialization_locked_at)) OR ((materialization_status = 'materialized'::text) AND (run_id IS NOT NULL) AND (refusal IS NULL) AND (materialization_worker_id IS NULL) AND (materialization_generation IS NULL) AND (materialization_locked_at IS NULL) AND (materialization_lock_expires_at IS NULL)) OR ((materialization_status = 'failed'::text) AND (run_id IS NULL) AND ((materialization_attempts = 5) OR (refusal IS NOT NULL)) AND (materialization_worker_id IS NULL) AND (materialization_generation IS NULL) AND (materialization_locked_at IS NULL) AND (materialization_lock_expires_at IS NULL)))),
    CONSTRAINT schedule_runs_refusal_check CHECK (((refusal IS NULL) OR (refusal = ANY (ARRAY['unavailable'::text, 'input_rejected'::text, 'key_conflict'::text])))),
    CONSTRAINT schedule_runs_snapshot_object_check CHECK ((jsonb_typeof(schedule_snapshot) = 'object'::text))
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
    -- Workflow tools the agent must grant for this skill (06-D5); never a grant itself.
    required_tools text[] DEFAULT '{}'::text[] NOT NULL CONSTRAINT skills_required_tools_shape CHECK (public.valid_workflow_type_names(required_tools)),
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
-- Name: task_attempts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_attempts (
    id uuid NOT NULL,
    task_id uuid NOT NULL,
    attempt_number integer NOT NULL,
    status text NOT NULL,
    stop_reason text,
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
    CONSTRAINT task_attempts_stop_reason_check CHECK ((stop_reason = ANY (ARRAY['completed'::text, 'timed_out'::text, 'lease_lost'::text, 'effect_parked'::text, 'approval_parked'::text, 'child_parked'::text]))),
    CONSTRAINT workflow_attempt_failure_shape CHECK ((((workflow_failure_class IS NULL) AND (workflow_failure_code IS NULL) AND (workflow_retry_safety IS NULL) AND (workflow_retirement IS NULL)) OR ((status = 'failed'::text) AND (finished_at IS NOT NULL) AND (workflow_failure_class IS NOT NULL) AND (workflow_failure_class = ANY (ARRAY['retryable'::text, 'terminal'::text])) AND (workflow_failure_code IS NOT NULL) AND ((octet_length(workflow_failure_code) >= 1) AND (octet_length(workflow_failure_code) <= 128)) AND (workflow_retry_safety IS NOT NULL) AND (workflow_retry_safety = ANY (ARRAY['safe'::text, 'unknown'::text])) AND (workflow_retirement IS NOT NULL) AND (workflow_retirement = ANY (ARRAY['live'::text, 'expired'::text, 'deadline'::text, 'cancel'::text])))))
);


--
-- Name: task_harness_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.task_harness_runs (
    id uuid NOT NULL,
    company_id uuid NOT NULL,
    task_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    owner_kind text NOT NULL DEFAULT 'workflow',
    workflow_run_id uuid,
    workflow_execution_id uuid,
    workflow_generation uuid,
    workflow_worker_id uuid,
    workflow_attempt integer,
    workflow_prepared jsonb,
    model_blocked_decision uuid,
    model_request_id uuid,
    model_generation uuid,
    model_worker_id uuid,
    model_attempt integer,
    schema_version smallint NOT NULL,
    revision bigint NOT NULL,
    state text NOT NULL,
    checkpoint jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    final_output_consumed_at timestamp with time zone,
    CONSTRAINT task_harness_runs_token_owner CHECK (model_blocked_decision IS NULL OR owner_kind='workflow'),
    CONSTRAINT task_harness_runs_model_owner_shape CHECK (
        num_nonnulls(model_request_id,model_generation,model_worker_id,model_attempt)=0
        OR (owner_kind='workflow'
            AND num_nonnulls(model_request_id,model_generation,model_worker_id,model_attempt)=4
            AND model_attempt>0)),
    CONSTRAINT task_harness_runs_owner_shape CHECK (
        owner_kind = 'workflow'
            AND num_nonnulls(workflow_run_id,workflow_execution_id,workflow_generation,
                workflow_worker_id,workflow_attempt,workflow_prepared) = 6
            AND workflow_attempt > 0 AND final_output_consumed_at IS NULL
            AND jsonb_typeof(workflow_prepared) = 'object'
            AND octet_length(workflow_prepared::text) <= 2097152),
    CONSTRAINT task_harness_runs_check CHECK ((((checkpoint ->> 'schema_version'::text))::integer = schema_version)),
    CONSTRAINT task_harness_runs_check1 CHECK ((((checkpoint ->> 'revision'::text))::bigint = revision)),
    CONSTRAINT task_harness_runs_check2 CHECK (((checkpoint ->> 'state'::text) = state)),
    CONSTRAINT task_harness_runs_check3 CHECK ((((checkpoint ->> 'run_id'::text) = (id)::text) AND (((checkpoint -> 'identity'::text) ->> 'company_id'::text) = (company_id)::text) AND (((checkpoint -> 'identity'::text) ->> 'task_id'::text) = (task_id)::text) AND (((checkpoint -> 'identity'::text) ->> 'agent_id'::text) = (agent_id)::text))),
    CONSTRAINT task_harness_runs_checkpoint_check CHECK (((jsonb_typeof(checkpoint) = 'object'::text) AND (octet_length((checkpoint)::text) <= 2097152))),
    CONSTRAINT task_harness_runs_checkpoint_check1 CHECK ((jsonb_array_length((checkpoint -> 'reservations'::text)) <= 16)),
    CONSTRAINT task_harness_runs_checkpoint_check2 CHECK ((jsonb_array_length((checkpoint -> 'invocations'::text)) <= 64)),
    CONSTRAINT task_harness_runs_invalid_output CHECK (((state <> 'invalid_output'::text) OR COALESCE((((checkpoint -> 'final_output'::text) = 'null'::jsonb) AND (((checkpoint -> 'identity'::text) -> 'response_contract'::text) <> 'null'::jsonb) AND (jsonb_array_length(jsonb_path_query_array(checkpoint, '$."reservations"[*]?(@."repair" != null)."repair"'::jsonpath)) = 2) AND ((((checkpoint -> 'turns'::text) -> '-1'::integer) ->> 'invalid_response'::text) = ANY (ARRAY['malformed_json'::text, 'schema_mismatch'::text, 'output_limit'::text, 'tool_call'::text]))), false))),
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
    CONSTRAINT task_harness_runs_state_check CHECK ((state = ANY (ARRAY['active'::text, 'waiting'::text, 'completed'::text, 'superseded'::text, 'invalid_output'::text, 'budget_exceeded'::text])))
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
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    delivery_failure_email_enabled boolean DEFAULT true CONSTRAINT user_notification_preferenc_delivery_failure_email_ena_not_null NOT NULL,
    -- Email me when a workflow decision needs my review (P08-Q8).
    decision_email_enabled boolean DEFAULT true NOT NULL
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
    CONSTRAINT workflow_action_reconciliations_failure_code_check CHECK (((octet_length(failure_code) BETWEEN 1 AND 128) AND (failure_code ~ '^[A-Za-z_][A-Za-z0-9_-]*(\.[A-Za-z_][A-Za-z0-9_-]*)*$'::text)))
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
    run_as_user_id uuid NOT NULL,
    message_trigger text,
    CONSTRAINT workflow_binding_revisions_message_trigger_check CHECK (((message_trigger IS NULL) OR (message_trigger = ANY (ARRAY['direct'::text, 'any'::text, 'mentioned'::text, 'mentioned_or_reply'::text])))),
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
    activated_at timestamp with time zone,
    CONSTRAINT workflow_bindings_activation_shape CHECK ((active = (activated_at IS NOT NULL))),
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
    CONSTRAINT workflow_budget_receipts_reservation_key_check CHECK (((octet_length(reservation_key) BETWEEN 1 AND 128) AND (reservation_key ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]*$'::text))),
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
-- Name: workflow_decision_comments; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_decision_comments (
    company_id uuid NOT NULL,
    decision_id uuid NOT NULL,
    sequence bigint NOT NULL,
    author_principal_id uuid NOT NULL,
    command_key text NOT NULL,
    body text NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_decision_comments_body_check CHECK (((char_length(body) >= 1) AND (char_length(body) <= 16384))),
    CONSTRAINT workflow_decision_comments_command_key_check CHECK (((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128))),
    CONSTRAINT workflow_decision_comments_sequence_check CHECK (((sequence >= 1) AND (sequence <= 1000)))
);


--
-- Name: workflow_decision_reviewers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_decision_reviewers (
    company_id uuid NOT NULL,
    user_id uuid NOT NULL,
    decision_id uuid NOT NULL,
    ordinal smallint NOT NULL,
    principal_id uuid NOT NULL,
    -- The member notice, or NULL when the reviewer had opted out of decision email (p08 P08-Q8).
    notice_delivery_id uuid,
    notice_opted_out boolean DEFAULT false NOT NULL,
    CONSTRAINT workflow_decision_reviewers_notice_check CHECK (((notice_delivery_id IS NULL) = notice_opted_out)),
    CONSTRAINT workflow_decision_reviewers_ordinal_check CHECK (((ordinal >= 0) AND (ordinal <= 15)))
);


--
-- Name: workflow_decisions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_decisions (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    id uuid NOT NULL,
    wait_id uuid NOT NULL,
    checkpoint_id uuid,
    checkpoint_revision bigint,
    token_request_id uuid,
    token_stage text,
    token_proposal jsonb,
    subject_kind text NOT NULL,
    job_id uuid,
    parked_attempt_number integer,
    action_invocation_id uuid,
    argument_digest text,
    operation_digest text,
    submitted_xact xid8,
    proposal_digest text NOT NULL,
    revision bigint DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    submitted_at timestamp with time zone,
    submitted_by uuid,
    command_key text,
    command_fingerprint text,
    choice text,
    feedback text,
    data jsonb,
    CONSTRAINT workflow_decision_submission CHECK ((((submitted_at IS NULL) AND (submitted_by IS NULL) AND (command_key IS NULL) AND (command_fingerprint IS NULL) AND (choice IS NULL) AND (feedback IS NULL) AND (data IS NULL) AND (revision = 1)) OR ((submitted_at IS NOT NULL) AND (submitted_by IS NOT NULL) AND (command_key IS NOT NULL) AND (command_fingerprint IS NOT NULL) AND (choice IS NOT NULL) AND (feedback IS NOT NULL) AND (data IS NOT NULL) AND (revision = 2)))),
    CONSTRAINT workflow_decisions_choice_check CHECK (((choice IS NULL) OR ((octet_length(choice) >= 1) AND (octet_length(choice) <= 140)))),
    CONSTRAINT workflow_decisions_command_fingerprint_check CHECK (((command_fingerprint IS NULL) OR (command_fingerprint ~ '^[0-9a-f]{64}$'::text))),
    CONSTRAINT workflow_decisions_command_key_check CHECK (((command_key IS NULL) OR ((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128)))),
    CONSTRAINT workflow_decisions_data_check CHECK (((data IS NULL) OR (octet_length((data)::text) <= 1048576))),
    CONSTRAINT workflow_decisions_feedback_check CHECK (((feedback IS NULL) OR (octet_length(feedback) <= 65536))),
    CONSTRAINT workflow_decisions_proposal_digest_check CHECK ((proposal_digest ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_decisions_subject_kind_check CHECK (
        (subject_kind IN ('business','action_approval')
            AND num_nonnulls(checkpoint_id,checkpoint_revision,token_request_id,token_stage,token_proposal)=0
            AND ((subject_kind='business' AND num_nonnulls(job_id,parked_attempt_number,action_invocation_id,argument_digest,operation_digest)=0)
                OR (subject_kind='action_approval' AND num_nonnulls(job_id,parked_attempt_number,action_invocation_id,argument_digest,operation_digest)=5 AND parked_attempt_number>0)))
        OR (subject_kind='token_allowance' AND num_nonnulls(checkpoint_id,checkpoint_revision,token_request_id,token_stage,token_proposal,job_id,parked_attempt_number)=7
            AND num_nonnulls(action_invocation_id,argument_digest,operation_digest)=0
            AND checkpoint_revision>=0 AND parked_attempt_number>0 AND token_stage IN ('reservation','settlement')
            AND jsonb_typeof(token_proposal)='object' AND octet_length(token_proposal::text)<=8192)),
    CONSTRAINT workflow_decisions_submission_xact CHECK ((submitted_at IS NULL) = (submitted_xact IS NULL))
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
    CONSTRAINT workflow_definitions_title_check CHECK (((octet_length(title) BETWEEN 1 AND 256) AND (btrim(title) <> ''::text)))
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
    CONSTRAINT workflow_http_credentials_envelope_check CHECK (((octet_length(envelope) BETWEEN 1 AND 16384) AND (envelope ~~ 'enc:v2:%'::text)))
);


--
-- Name: workflow_tool_registrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_tool_registrations (
    company_id uuid NOT NULL,
    name text NOT NULL,
    workflow_id uuid NOT NULL,
    version_id uuid NOT NULL,
    params jsonb DEFAULT '{}'::jsonb NOT NULL,
    revision bigint DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    -- 07-D2 §D2.1: a granted tool name; `message.*` names are the built-in message tools.
    CONSTRAINT workflow_tool_registration_name CHECK ((public.valid_workflow_type_names(ARRAY[name]) AND (name !~ '^message[.]'::text))),
    CONSTRAINT workflow_tool_registration_params CHECK (((jsonb_typeof(params) = 'object'::text) AND (octet_length((params)::text) <= 65536))),
    CONSTRAINT workflow_tool_registration_revision CHECK ((revision >= 1))
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
    in_app boolean GENERATED ALWAYS AS ((delivery_id IS NULL)) STORED,
    CONSTRAINT workflow_message_destinations_check CHECK ((((kind = 'channel'::text) AND (delivery_id IS NULL) AND (principal_id IS NULL) AND (recipient_address IS NULL)) OR ((kind = 'user'::text) AND (principal_id IS NOT NULL) AND ((delivery_id IS NULL) = (recipient_address IS NULL))) OR ((kind = 'reply'::text) AND (principal_id IS NULL) AND ((delivery_id IS NULL) = (recipient_address IS NULL))))),
    CONSTRAINT workflow_message_destinations_kind_check CHECK ((kind = ANY (ARRAY['channel'::text, 'user'::text, 'reply'::text]))),
    CONSTRAINT workflow_message_destinations_ordinal_check CHECK (((ordinal >= 0) AND (ordinal <= 19))),
    CONSTRAINT workflow_message_destinations_recipient_address_check CHECK (((recipient_address IS NULL) OR ((btrim(recipient_address) <> ''::text) AND (octet_length(recipient_address) <= 998))))
);


--
-- Name: workflow_message_intent_bindings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_message_intent_bindings (
    company_id uuid NOT NULL,
    intent_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    binding_revision bigint NOT NULL,
    outcome text NOT NULL,
    run_id uuid,
    reason text,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_message_intent_binding_outcome CHECK ((((outcome = 'admitted'::text) AND (run_id IS NOT NULL) AND (reason IS NULL)) OR ((outcome = ANY (ARRAY['refused'::text, 'failed'::text])) AND (run_id IS NULL) AND (reason IS NOT NULL)))),
    CONSTRAINT workflow_message_intent_binding_reason CHECK (((reason IS NULL) OR ((octet_length(reason) >= 1) AND (octet_length(reason) <= 64)))),
    CONSTRAINT workflow_message_intent_binding_revision CHECK ((binding_revision > 0))
);


--
-- Name: workflow_message_intents; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_message_intents (
    company_id uuid NOT NULL,
    id uuid NOT NULL,
    message_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    role text NOT NULL,
    mentioned boolean NOT NULL,
    admissible boolean NOT NULL,
    origin text NOT NULL,
    reply_to_message_id uuid,
    reply_settled boolean DEFAULT false NOT NULL,
    responder_principal_id uuid,
    hop_count integer NOT NULL,
    trace_channels uuid[] NOT NULL,
    delivery_policy text NOT NULL,
    correlation_id uuid NOT NULL,
    state text DEFAULT 'pending'::text NOT NULL,
    attempts smallint DEFAULT 0 NOT NULL,
    available_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    worker_id uuid,
    claim_generation uuid,
    lease_expires_at timestamp with time zone,
    outcome text,
    matched_run_id uuid,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT workflow_message_intent_attempts CHECK (((attempts >= 0) AND (attempts <= 5))),
    CONSTRAINT workflow_message_intent_delivery_policy CHECK ((delivery_policy = ANY (ARRAY['deliver'::text, 'in_app_only'::text]))),
    CONSTRAINT workflow_message_intent_hops CHECK (((hop_count >= 0) AND (hop_count <= 64) AND (cardinality(trace_channels) <= 5))),
    CONSTRAINT workflow_message_intent_origin CHECK ((origin = ANY (ARRAY['external'::text, 'internal'::text, 'application'::text]))),
    CONSTRAINT workflow_message_intent_outcome CHECK ((((outcome IS NULL) OR (outcome = ANY (ARRAY['matched'::text, 'admitted'::text, 'none'::text]))) AND ((outcome = 'matched'::text) = (matched_run_id IS NOT NULL)))),
    CONSTRAINT workflow_message_intent_role CHECK ((role = ANY (ARRAY['to'::text, 'cc'::text]))),
    CONSTRAINT workflow_message_intent_reply_settled CHECK (((NOT reply_settled) OR ((reply_to_message_id IS NOT NULL) AND (outcome IS DISTINCT FROM 'matched'::text)))),
    CONSTRAINT workflow_message_intent_state CHECK ((((state = 'pending'::text) AND (worker_id IS NULL) AND (claim_generation IS NULL) AND (lease_expires_at IS NULL) AND (outcome IS NULL) AND (attempts < 5)) OR ((state = 'claimed'::text) AND (worker_id IS NOT NULL) AND (claim_generation IS NOT NULL) AND (lease_expires_at IS NOT NULL) AND (outcome IS NULL) AND (attempts >= 1)) OR ((state = 'done'::text) AND (worker_id IS NULL) AND (claim_generation IS NULL) AND (lease_expires_at IS NULL) AND (outcome IS NOT NULL)) OR ((state = 'failed'::text) AND (worker_id IS NULL) AND (claim_generation IS NULL) AND (lease_expires_at IS NULL) AND (outcome IS NULL) AND (attempts = 5))))
);


--
-- Name: TABLE workflow_message_intents; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.workflow_message_intents IS 'One accepted inbound message x channel association, committed with the message (p08 D1.3). Besides queueing workflow admission it is the only durable owner of the inbound relay trace (hop_count, trace_channels) that workflow replies continue, so rows must not be swept while their message exists.';


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
-- Name: workflow_provisioning_commands; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_provisioning_commands (
    company_id uuid NOT NULL,
    command_key text NOT NULL,
    request_digest text NOT NULL,
    actor_id uuid NOT NULL,
    result jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT workflow_provisioning_commands_command_key_check CHECK (((octet_length(command_key) >= 1) AND (octet_length(command_key) <= 128))),
    CONSTRAINT workflow_provisioning_commands_request_digest_check CHECK ((request_digest ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT workflow_provisioning_commands_result_check CHECK (((jsonb_typeof(result) = 'object'::text) AND (octet_length((result)::text) <= 4096)))
);


--
-- Name: TABLE workflow_provisioning_commands; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON TABLE public.workflow_provisioning_commands IS 'One "Create assistant channel" command per client key (p08 D2.4): the request digest decides replay or conflict, and the stored result is what a replay returns. The tool path needs none; its action receipt is the record.';


--
-- Name: workflow_root_budget_usage; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_root_budget_usage (
    authorized_tokens bigint NOT NULL DEFAULT 0 CHECK (authorized_tokens>=0),
    token_refusal text CHECK (token_refusal IN ('rejected','expired','cancelled')),
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

-- Frozen optional token allowance; current reviewer authority is checked at admission/use.
CREATE FUNCTION public.valid_workflow_token_policy(policy jsonb) RETURNS boolean
    LANGUAGE plpgsql IMMUTABLE
    AS $$
DECLARE reviewer jsonb; seen uuid[] = '{}'; reviewer_id uuid; amount bigint;
BEGIN
    IF jsonb_typeof(policy) IS DISTINCT FROM 'object'
       OR NOT policy ?& ARRAY['max_tokens','reviewers']
       OR policy - ARRAY['max_tokens','reviewers'] <> '{}'::jsonb
       OR jsonb_typeof(policy->'max_tokens') IS DISTINCT FROM 'number'
       OR policy->>'max_tokens' !~ '^[0-9]+$'
       OR jsonb_typeof(policy->'reviewers') IS DISTINCT FROM 'array' THEN RETURN false; END IF;
    amount=(policy->>'max_tokens')::bigint;
    IF amount<1 OR amount>2147483647 OR (policy->>'max_tokens')::numeric<>amount
       OR jsonb_array_length(policy->'reviewers') NOT BETWEEN 1 AND 16 THEN RETURN false; END IF;
    FOR reviewer IN SELECT value FROM jsonb_array_elements(policy->'reviewers') LOOP
        IF jsonb_typeof(reviewer) IS DISTINCT FROM 'object'
           OR reviewer->>'kind' IS DISTINCT FROM 'user'
           OR NOT reviewer ?& ARRAY['kind','id']
           OR reviewer - ARRAY['kind','id'] <> '{}'::jsonb
           OR jsonb_typeof(reviewer->'id') IS DISTINCT FROM 'string' THEN RETURN false; END IF;
        reviewer_id=(reviewer->>'id')::uuid;
        IF reviewer_id=ANY(seen) THEN RETURN false; END IF;
        seen=array_append(seen,reviewer_id);
    END LOOP;
    RETURN true;
EXCEPTION WHEN data_exception THEN RETURN false;
END $$;

CREATE TABLE public.workflow_root_budgets (
    company_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    provenance text NOT NULL,
    activations integer,
    model_calls integer,
    repetitions integer,
    token_policy jsonb,
    CONSTRAINT workflow_root_token_policy_check CHECK (token_policy IS NULL OR (provenance='frozen_v6' AND public.valid_workflow_token_policy(token_policy))),
    CONSTRAINT workflow_root_budgets_check CHECK ((((provenance = 'sealed_legacy'::text) AND (activations IS NULL) AND (model_calls IS NULL) AND (repetitions IS NULL)) OR ((provenance = 'frozen_v6'::text) AND (activations IS NOT NULL) AND ((activations >= 1) AND (activations <= 100000)) AND (model_calls IS NOT NULL) AND ((model_calls >= 1) AND (model_calls <= 1000)) AND (repetitions IS NOT NULL) AND ((repetitions >= 1) AND (repetitions <= 1000))))),
    CONSTRAINT workflow_root_budgets_provenance_check CHECK ((provenance = ANY (ARRAY['frozen_v6'::text, 'sealed_legacy'::text])))
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
    decision_id uuid,
    run_id uuid NOT NULL,
    sequence bigint NOT NULL,
    event_kind text NOT NULL,
    actor_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    execution_id uuid,
    terminal_state text,
    terminal_execution_id uuid,
    -- Effect park/continuation audits of one exact delivery wait; an agent execution may
    -- park on several sequential deliveries, each with its own audit identity.
    wait_id uuid,
    CONSTRAINT workflow_run_event_wait_shape CHECK (((wait_id IS NULL) OR ((execution_id IS NOT NULL) AND (decision_id IS NULL) AND (event_kind = ANY (ARRAY['wait_parked'::text, 'wait_completed'::text]))))),
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
    rerun_of_run_id uuid,
    delivery_policy text DEFAULT 'deliver'::text NOT NULL,
    CONSTRAINT workflow_run_delivery_policy CHECK ((delivery_policy = ANY (ARRAY['deliver'::text, 'in_app_only'::text]))),
    CONSTRAINT workflow_rerun_shape CHECK (((rerun_of_run_id IS NULL) OR ((parent_run_id IS NULL) AND (rerun_of_run_id <> id)))),
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
    agent_ids uuid[] NOT NULL,
    CONSTRAINT workflow_versions_agent_ids_check CHECK ((array_position(agent_ids, NULL::uuid) IS NULL)),
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
-- Name: COLUMN workflow_versions.agent_ids; Type: COMMENT; Schema: public; Owner: -
--

COMMENT ON COLUMN public.workflow_versions.agent_ids IS 'Display index only: the distinct agents frozen in this version''s bundle and its pinned children, written once with the row. No execution, admission or authorization path reads it.';


--
-- Name: workflow_wait_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.workflow_wait_events (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    id uuid NOT NULL,
    event_name text NOT NULL,
    correlation text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    consumed_execution_id uuid,
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
    child_run_id uuid,
    child_ordinal integer,
    child_tool_call_id uuid,
    parked_attempt_number integer,
    CONSTRAINT workflow_wait_child CHECK ((((reason = 'child_run'::text) = (child_run_id IS NOT NULL)) AND ((reason = 'child_run'::text) = ((child_ordinal IS NOT NULL) <> (child_tool_call_id IS NOT NULL))) AND ((reason = 'child_run'::text) OR ((child_ordinal IS NULL) AND (child_tool_call_id IS NULL))) AND ((child_ordinal IS NULL) OR ((child_ordinal >= 1) AND (child_ordinal <= 1000))) AND ((parked_attempt_number IS NOT NULL) = (child_tool_call_id IS NOT NULL)) AND ((parked_attempt_number IS NULL) OR (parked_attempt_number >= 1)))),
    CONSTRAINT workflow_wait_correlation CHECK (((correlation IS NULL) OR ((octet_length(correlation) >= 1) AND (octet_length(correlation) <= 256)))),
    CONSTRAINT workflow_wait_deadline CHECK (((reason = 'timer'::text) OR (deadline > created_at))),
    CONSTRAINT workflow_wait_event_name CHECK (((event_name IS NULL) OR ((octet_length(event_name) >= 1) AND (octet_length(event_name) <= 128)))),
    CONSTRAINT workflow_wait_failure CHECK ((((state = 'failed'::text) = (failure_code IS NOT NULL)) AND ((failure_code IS NULL) OR (failure_code = ANY (ARRAY['activation_limit'::text, 'invalid_output'::text, 'evidence_conflict'::text, 'root_budget_exhausted'::text]))))),
    CONSTRAINT workflow_wait_intent CHECK (((notification_intent IS NULL) OR ((jsonb_typeof(notification_intent) = 'object'::text) AND ((notification_intent ->> 'version'::text) = '1'::text) AND ((notification_intent ->> 'wait_id'::text) = (id)::text) AND (octet_length((notification_intent)::text) <= 2048)))),
    CONSTRAINT workflow_wait_settlement CHECK (((state = 'waiting'::text) = (settled_at IS NULL))),
    CONSTRAINT workflow_wait_state CHECK ((state = ANY (ARRAY['waiting'::text, 'completed'::text, 'expired'::text, 'failed'::text, 'cancelled'::text]))),
    CONSTRAINT workflow_waits_reason_check CHECK ((reason = ANY (ARRAY['decision'::text, 'event'::text, 'timer'::text, 'child_run'::text, 'effect'::text, 'reconciliation'::text])))
);


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
-- Name: background_tasks background_tasks_company_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_company_id_id_key UNIQUE (company_id, id);


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
-- Name: schedule_runs schedule_runs_command_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_command_key UNIQUE (company_id, schedule_id, command_key);


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
-- Name: workflow_decision_comments workflow_decision_comment_command; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decision_comments
    ADD CONSTRAINT workflow_decision_comment_command UNIQUE (company_id, decision_id, author_principal_id, command_key);


--
-- Name: workflow_decision_comments workflow_decision_comments_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decision_comments
    ADD CONSTRAINT workflow_decision_comments_pkey PRIMARY KEY (company_id, decision_id, sequence);


--
-- Name: workflow_decision_reviewers workflow_decision_reviewer_notice; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decision_reviewers
    ADD CONSTRAINT workflow_decision_reviewer_notice UNIQUE (notice_delivery_id);


--
-- Name: workflow_decision_reviewers workflow_decision_reviewer_ordinal; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decision_reviewers
    ADD CONSTRAINT workflow_decision_reviewer_ordinal UNIQUE (company_id, decision_id, ordinal);


--
-- Name: workflow_decision_reviewers workflow_decision_reviewers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decision_reviewers
    ADD CONSTRAINT workflow_decision_reviewers_pkey PRIMARY KEY (company_id, decision_id, principal_id);


--
-- Name: workflow_decisions workflow_decision_execution; Type: CONSTRAINT; Schema: public; Owner: -
--

CREATE UNIQUE INDEX workflow_decision_execution ON public.workflow_decisions(company_id,run_id,execution_id) WHERE subject_kind='business';
CREATE UNIQUE INDEX workflow_decision_action ON public.workflow_decisions(company_id,run_id,execution_id,action_invocation_id) WHERE subject_kind='action_approval';
-- A decision id names one decision across companies: the notice link names only the decision,
-- and the decision page finds its company by it (p08 DV-7).
CREATE UNIQUE INDEX workflow_decision_id ON public.workflow_decisions(id);
-- Every open decision in a company, for the work list (both views: My work probes each one on the
-- reviewer key, which stays bounded by open decisions as a reviewer's history grows) and the
-- operational summary (P08-D4 §D4.2; logs/b4a-index-explain.log).
CREATE INDEX workflow_decisions_open ON public.workflow_decisions(company_id, run_id) WHERE submitted_at IS NULL;
-- Failed schedule materializations in the work list's refusal window (P08-D4 §D4.2).
CREATE INDEX schedule_runs_failed_recent ON public.schedule_runs(company_id, created_at DESC) WHERE materialization_status = 'failed';
ALTER TABLE ONLY public.workflow_decisions ADD UNIQUE(company_id,run_id,execution_id,id);
ALTER TABLE ONLY public.workflow_decisions ADD UNIQUE(company_id,wait_id);


--
-- Name: workflow_decisions workflow_decisions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decisions
    ADD CONSTRAINT workflow_decisions_pkey PRIMARY KEY (company_id, id);


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
-- Name: workflow_tool_registrations workflow_tool_registrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_tool_registrations
    ADD CONSTRAINT workflow_tool_registrations_pkey PRIMARY KEY (company_id, name);


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
-- Name: workflow_message_intent_bindings workflow_message_intent_bindings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intent_bindings
    ADD CONSTRAINT workflow_message_intent_bindings_pkey PRIMARY KEY (company_id, intent_id, binding_id);


--
-- Name: workflow_message_intents workflow_message_intents_association_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intents
    ADD CONSTRAINT workflow_message_intents_association_key UNIQUE (company_id, message_id, channel_id);


--
-- Name: workflow_message_intents workflow_message_intents_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intents
    ADD CONSTRAINT workflow_message_intents_pkey PRIMARY KEY (company_id, id);


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
-- Name: workflow_provisioning_commands workflow_provisioning_commands_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_provisioning_commands
    ADD CONSTRAINT workflow_provisioning_commands_pkey PRIMARY KEY (company_id, command_key);


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

-- The run listings' keyset order, company-wide and by binding (p08 B3b-1).
CREATE INDEX workflow_runs_newest ON public.workflow_runs(company_id, created_at DESC, id DESC);
CREATE INDEX workflow_runs_binding_newest ON public.workflow_runs(company_id, binding_id, created_at DESC, id DESC);
-- The retargeted surfaces (P08-D4): attention, run counts and thread activity.
CREATE INDEX workflow_runs_attention ON public.workflow_runs(company_id, state, created_at DESC) WHERE state IN ('waiting', 'failed');
CREATE INDEX workflow_runs_reruns ON public.workflow_runs(company_id, rerun_of_run_id) WHERE rerun_of_run_id IS NOT NULL;
CREATE INDEX workflow_runs_open_by_channel ON public.workflow_runs(company_id, channel_id, state) WHERE state IN ('queued', 'running', 'waiting');
CREATE INDEX workflow_runs_by_thread ON public.workflow_runs(company_id, thread_id, created_at DESC) WHERE thread_id IS NOT NULL;
-- The run page's reads by run (p09 B2a, F029): its controls, the message that started it and its
-- children. Receipts and reconciliations are reached through the run-led dispatch key instead.
CREATE INDEX workflow_control_commands_by_run ON public.workflow_control_commands(company_id, run_id, created_at DESC, command_key DESC);
CREATE INDEX workflow_message_intent_bindings_by_run ON public.workflow_message_intent_bindings(company_id, run_id) WHERE run_id IS NOT NULL;
CREATE INDEX workflow_run_parents_by_parent ON public.workflow_run_parents(company_id, parent_run_id, parent_execution_id);


--
-- Name: workflow_runs workflow_run_workflow_identity; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_run_workflow_identity UNIQUE (company_id, id, workflow_id);


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
-- Name: workflow_wait_events workflow_wait_events_company_id_run_id_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_wait_events
    ADD CONSTRAINT workflow_wait_events_company_id_run_id_id_key UNIQUE (company_id, run_id, id);


--
-- Name: workflow_wait_events workflow_wait_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_wait_events
    ADD CONSTRAINT workflow_wait_events_pkey PRIMARY KEY (company_id, id);


--
-- Name: workflow_waits workflow_wait_execution; Type: CONSTRAINT; Schema: public; Owner: -
--

-- Effect waits are per message action (unique wait_id per action); an agent execution may
-- park on several sequential deliveries, while workflow_wait_one_active keeps one active wait.
-- Each flow.repeat round has its own child_run wait; one execution holds one wait per ordinal.
CREATE UNIQUE INDEX workflow_wait_execution ON public.workflow_waits(company_id,run_id,execution_id,reason) WHERE reason NOT IN ('decision','effect','child_run');

CREATE UNIQUE INDEX workflow_wait_child_ordinal ON public.workflow_waits(company_id,run_id,execution_id,child_ordinal) WHERE reason='child_run';
-- One child per saved model tool call of an agent execution (07-D2 §D2.3).
CREATE UNIQUE INDEX workflow_wait_child_tool_call ON public.workflow_waits(company_id,run_id,execution_id,child_tool_call_id) WHERE child_tool_call_id IS NOT NULL;

CREATE UNIQUE INDEX workflow_wait_one_active ON public.workflow_waits(company_id,run_id,execution_id) WHERE state='waiting';


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
-- Name: workflow_waits workflow_wait_child_run_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_waits
    ADD CONSTRAINT workflow_wait_child_run_key UNIQUE (company_id, child_run_id);


--
-- Name: agent_skills_skill_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX agent_skills_skill_idx ON public.agent_skills USING btree (skill_id, agent_id);


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
-- Name: background_tasks_processing_lease_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_processing_lease_idx ON public.background_tasks USING btree (lock_expires_at, id) WHERE (status = 'processing'::text);


--
-- Name: background_tasks_thread_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX background_tasks_thread_idx ON public.background_tasks USING btree (thread_id) WHERE (thread_id IS NOT NULL);


--
-- Name: binding_audit_events_binding_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX binding_audit_events_binding_idx ON public.binding_audit_events USING btree (company_id, binding_id, created_at DESC, id DESC);


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
-- Name: channel_schedules_binding_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX channel_schedules_binding_idx ON public.channel_schedules USING btree (company_id, binding_id);


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
-- Name: schedule_runs_schedule_slot_key; Type: INDEX; Schema: public; Owner: -
--

-- A cadence slot is captured once; run-now occurrences carry a command key and never collide.
CREATE UNIQUE INDEX schedule_runs_schedule_slot_key ON public.schedule_runs USING btree (schedule_id, scheduled_for) WHERE (command_key IS NULL);


--
-- Name: skills_library_slug_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX skills_library_slug_key ON public.skills USING btree (slug) WHERE (company_id IS NULL);


--
-- Name: task_harness_runs_task; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX task_harness_runs_task ON public.task_harness_runs USING btree (company_id, task_id);


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

CREATE UNIQUE INDEX workflow_message_actions_wait ON public.workflow_message_actions USING btree (company_id, execution_id, wait_id) WHERE (wait_id IS NOT NULL);

-- A message step parks on at most one effect wait; only saved model tool calls may park
-- sequentially within one (agent) execution (06-D4).
CREATE UNIQUE INDEX workflow_message_step_one_wait ON public.workflow_message_actions USING btree (company_id, execution_id) WHERE ((origin = 'step'::text) AND (wait_id IS NOT NULL));


--
-- Name: workflow_message_delivery_events_cursor; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_message_delivery_events_cursor ON public.workflow_message_delivery_events USING btree (company_id, invocation_id, seq);


--
-- Name: workflow_message_destinations_run; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_message_destinations_run ON public.workflow_message_destinations USING btree (company_id, run_id) WHERE (delivery_id IS NOT NULL);


--
-- Name: workflow_message_intents_claim; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_message_intents_claim ON public.workflow_message_intents USING btree (company_id, available_at, id) WHERE (state = ANY (ARRAY['pending'::text, 'claimed'::text]));


--
-- Name: workflow_message_intent_bindings_binding; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_message_intent_bindings_binding ON public.workflow_message_intent_bindings USING btree (company_id, binding_id, created_at DESC) WHERE (outcome <> 'admitted'::text);


--
-- Name: workflow_one_execution_job; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX workflow_one_execution_job ON public.background_tasks USING btree (company_id, workflow_execution_id) WHERE (queue_kind = 'workflow'::text);


--
-- Name: workflow_wait_event_consumer; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX workflow_wait_event_consumer ON public.workflow_wait_events USING btree (company_id, consumed_execution_id) WHERE (consumed_execution_id IS NOT NULL);


--
-- Name: workflow_wait_event_unconsumed; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX workflow_wait_event_unconsumed ON public.workflow_wait_events USING btree (company_id, run_id, event_name, correlation, created_at, id) WHERE (consumed_execution_id IS NULL);


--
-- Name: workflow_run_event_execution_kind; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX workflow_run_event_execution_kind ON public.workflow_run_events USING btree (company_id, run_id, execution_id, event_kind) WHERE (event_kind <> 'action_reconciled'::text AND decision_id IS NULL AND wait_id IS NULL);

CREATE UNIQUE INDEX workflow_run_event_wait_kind ON public.workflow_run_events USING btree (company_id, run_id, wait_id, event_kind) WHERE (wait_id IS NOT NULL);


--
-- Name: notifications actionable_notifications_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER actionable_notifications_notify AFTER INSERT OR UPDATE OF state, read_at ON public.notifications FOR EACH ROW EXECUTE FUNCTION public.notify_actionable_notification_changed();


--
-- Name: agent_skills agent_skills_scope_check; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER agent_skills_scope_check BEFORE INSERT OR UPDATE ON public.agent_skills FOR EACH ROW EXECUTE FUNCTION public.enforce_agent_skill_scope();


--
-- Name: background_tasks background_tasks_preserve_queue_identity; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_preserve_queue_identity BEFORE UPDATE OF queue_kind, workflow_execution_id ON public.background_tasks FOR EACH ROW EXECUTE FUNCTION public.preserve_background_task_queue_identity();


--
-- Name: background_tasks background_tasks_validate_workflow_association; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER background_tasks_validate_workflow_association BEFORE INSERT OR UPDATE OF company_id, channel_id, thread_id, workflow_execution_id, queue_kind ON public.background_tasks FOR EACH ROW WHEN ((new.queue_kind = 'workflow'::text)) EXECUTE FUNCTION public.validate_workflow_job_association();


--
-- Name: binding_audit_events binding_audit_events_append_only; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER binding_audit_events_append_only BEFORE DELETE OR UPDATE ON public.binding_audit_events FOR EACH ROW EXECUTE FUNCTION public.reject_binding_audit_rewrite();


--
-- Name: company_mcp_connections company_mcp_endpoint_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER company_mcp_endpoint_immutable BEFORE UPDATE ON public.company_mcp_connections FOR EACH ROW EXECUTE FUNCTION public.company_mcp_endpoint_immutable();


--
-- Name: message_deliveries delivery_actionable_notification; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER delivery_actionable_notification AFTER DELETE OR UPDATE OF status ON public.message_deliveries FOR EACH ROW EXECUTE FUNCTION public.notification_from_delivery();




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
-- Name: skills library_skill_delete_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER library_skill_delete_guard BEFORE DELETE ON public.skills FOR EACH ROW EXECUTE FUNCTION public.prevent_assigned_library_skill_delete();


--
-- Name: message_deliveries message_deliveries_notify_attention; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER message_deliveries_notify_attention AFTER INSERT OR DELETE OR UPDATE OF status ON public.message_deliveries FOR EACH ROW EXECUTE FUNCTION public.notify_attention_changed('delivery');


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
-- Name: task_harness_runs require_harness_owner; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER guard_harness_model_owner BEFORE INSERT OR UPDATE ON public.task_harness_runs
    FOR EACH ROW EXECUTE FUNCTION public.guard_harness_model_owner();

CREATE TRIGGER require_harness_owner BEFORE INSERT OR UPDATE ON public.task_harness_runs
    FOR EACH ROW EXECUTE FUNCTION public.require_harness_owner();


--
-- Name: schedule_runs require_schedule_occurrence_scope; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER require_schedule_occurrence_scope BEFORE INSERT OR UPDATE ON public.schedule_runs FOR EACH ROW EXECUTE FUNCTION public.require_schedule_occurrence_scope();


--
-- Name: channel_schedules schedule_channel_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER schedule_channel_immutable BEFORE UPDATE OF channel_id ON public.channel_schedules FOR EACH ROW EXECUTE FUNCTION public.require_immutable_schedule_channel();


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
-- Name: workflow_message_destinations workflow_message_destination_in_app; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_message_destination_in_app BEFORE INSERT ON public.workflow_message_destinations FOR EACH ROW EXECUTE FUNCTION public.workflow_message_destination_in_app_guard();


--
-- Name: workflow_waits workflow_message_effect_wait_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_tool_child_wait_continuation AFTER UPDATE ON public.workflow_waits DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (((old.state = 'waiting'::text) AND (new.state = 'completed'::text) AND (new.reason = 'child_run'::text) AND (new.child_tool_call_id IS NOT NULL))) EXECUTE FUNCTION public.workflow_tool_child_wait_continuation_guard();

CREATE CONSTRAINT TRIGGER workflow_model_tool_wait_continuation AFTER UPDATE ON public.workflow_waits DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (((old.state = 'waiting'::text) AND (new.state = 'completed'::text) AND (new.reason = 'effect'::text))) EXECUTE FUNCTION public.workflow_model_tool_wait_continuation_guard();

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

CREATE TRIGGER workflow_parent_immutable BEFORE UPDATE OF parent_run_id, parent_execution_id, rerun_of_run_id ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_parent();


--
-- Name: workflow_runs workflow_parent_link; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_parent_link BEFORE INSERT ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.insert_workflow_parent_link();


--
-- Name: workflow_runs workflow_child_parent_wait; Type: TRIGGER; Schema: public; Owner: -
--

CREATE CONSTRAINT TRIGGER workflow_child_parent_wait AFTER INSERT ON public.workflow_runs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN ((new.parent_run_id IS NOT NULL)) EXECUTE FUNCTION public.check_workflow_child_parent_wait();


--
-- Name: workflow_runs workflow_rerun_original; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_rerun_original BEFORE INSERT ON public.workflow_runs FOR EACH ROW WHEN ((new.rerun_of_run_id IS NOT NULL)) EXECUTE FUNCTION public.check_workflow_rerun_original();


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

-- Live updates for the workflow views (P08-D4 §D4.1): run-level transitions only, never per-step
-- revision bumps. INSERT and UPDATE are separate because an INSERT trigger's WHEN cannot see OLD.
CREATE FUNCTION public.notify_workflow_run_changed() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM pg_notify(
        'workflow_run_changed',
        json_build_object(
            'company_id', NEW.company_id,
            'channel_id', NEW.channel_id,
            'thread_id', NEW.thread_id,
            'run_id', NEW.id,
            'state', NEW.state,
            'waiting_reason', NEW.waiting_reason
        )::text
    );
    RETURN NULL;
END;
$$;

CREATE TRIGGER workflow_runs_notify_inserted AFTER INSERT ON public.workflow_runs FOR EACH ROW EXECUTE FUNCTION public.notify_workflow_run_changed();
CREATE TRIGGER workflow_runs_notify_changed AFTER UPDATE ON public.workflow_runs FOR EACH ROW WHEN (((old.state IS DISTINCT FROM new.state) OR (old.waiting_reason IS DISTINCT FROM new.waiting_reason))) EXECUTE FUNCTION public.notify_workflow_run_changed();
CREATE TRIGGER workflow_runs_notify_activity_inserted AFTER INSERT ON public.workflow_runs FOR EACH ROW WHEN ((new.thread_id IS NOT NULL)) EXECUTE FUNCTION public.notify_thread_activity();
CREATE TRIGGER workflow_runs_notify_activity_changed AFTER UPDATE ON public.workflow_runs FOR EACH ROW WHEN (((new.thread_id IS NOT NULL) AND ((old.state IS DISTINCT FROM new.state) OR (old.waiting_reason IS DISTINCT FROM new.waiting_reason)))) EXECUTE FUNCTION public.notify_thread_activity();


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
-- Name: workflow_decision_comments workflow_decision_comment_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_decision_comment_identity BEFORE INSERT ON public.workflow_decision_comments FOR EACH ROW EXECUTE FUNCTION public.workflow_decision_comment_identity();
CREATE TRIGGER workflow_decision_submit_identity BEFORE INSERT OR UPDATE ON public.workflow_decisions FOR EACH ROW EXECUTE FUNCTION public.workflow_decision_submit_identity();

CREATE TRIGGER workflow_decision_comment_immutable BEFORE UPDATE ON public.workflow_decision_comments FOR EACH ROW EXECUTE FUNCTION public.workflow_decision_fact_immutable();


--
-- Name: workflow_decision_reviewers workflow_decision_reviewer_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_decision_reviewer_guard BEFORE INSERT ON public.workflow_decision_reviewers FOR EACH ROW EXECUTE FUNCTION public.workflow_decision_reviewer_guard();


--
-- Name: workflow_decision_reviewers workflow_decision_reviewer_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_decision_reviewer_immutable BEFORE UPDATE ON public.workflow_decision_reviewers FOR EACH ROW EXECUTE FUNCTION public.workflow_decision_fact_immutable();


--
-- Name: workflow_decisions workflow_decision_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_decision_immutable BEFORE UPDATE ON public.workflow_decisions FOR EACH ROW EXECUTE FUNCTION public.workflow_decision_immutable();


--
-- Name: workflow_waits workflow_wait_revision; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_wait_revision AFTER INSERT OR UPDATE ON public.workflow_waits FOR EACH ROW EXECUTE FUNCTION public.advance_workflow_owned_revision();


--
-- Name: workflow_run_events workflow_wakeup_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER workflow_wakeup_immutable BEFORE UPDATE ON public.workflow_run_events FOR EACH ROW EXECUTE FUNCTION public.preserve_workflow_wakeup();


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
-- Name: agents agents_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.agents
    ADD CONSTRAINT agents_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


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
-- Name: background_tasks background_tasks_thread_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.background_tasks
    ADD CONSTRAINT background_tasks_thread_fk FOREIGN KEY (company_id, channel_id, thread_id) REFERENCES public.threads(company_id, channel_id, id) ON DELETE SET NULL (thread_id);


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
-- Name: channel_schedules channel_schedules_binding_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.channel_schedules
    ADD CONSTRAINT channel_schedules_binding_fk FOREIGN KEY (company_id, binding_id) REFERENCES public.workflow_bindings(company_id, id) ON DELETE CASCADE;


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
-- Name: schedule_runs schedule_runs_binding_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_binding_fk FOREIGN KEY (company_id, binding_id) REFERENCES public.workflow_bindings(company_id, id) ON DELETE CASCADE;


--
-- Name: schedule_runs schedule_runs_run_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.schedule_runs
    ADD CONSTRAINT schedule_runs_run_fk FOREIGN KEY (company_id, run_id, binding_id) REFERENCES public.workflow_runs(company_id, id, binding_id) ON DELETE CASCADE;


--
-- Name: skills skills_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.skills
    ADD CONSTRAINT skills_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: task_attempts task_attempts_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_attempts
    ADD CONSTRAINT task_attempts_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.background_tasks(id) ON DELETE CASCADE;


--
-- Name: task_harness_runs task_harness_runs_company_id_agent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_runs
    ADD CONSTRAINT task_harness_runs_company_id_agent_id_fkey FOREIGN KEY (company_id, agent_id) REFERENCES public.agents(company_id, id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: task_harness_runs task_harness_runs_company_id_task_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.task_harness_runs
    ADD CONSTRAINT task_harness_runs_company_id_task_id_fkey FOREIGN KEY (company_id, task_id) REFERENCES public.background_tasks(company_id, id) ON DELETE CASCADE;


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

ALTER TABLE ONLY public.workflow_run_events
    ADD CONSTRAINT workflow_run_event_wait FOREIGN KEY (company_id, run_id, execution_id, wait_id) REFERENCES public.workflow_waits(company_id, run_id, execution_id, id);


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
-- Name: workflow_decision_comments workflow_decision_comments_decision_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decision_comments
    ADD CONSTRAINT workflow_decision_comments_decision_fkey FOREIGN KEY (company_id, decision_id) REFERENCES public.workflow_decisions(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_decision_reviewers workflow_decision_reviewers_decision_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decision_reviewers
    ADD CONSTRAINT workflow_decision_reviewers_decision_fkey FOREIGN KEY (company_id, decision_id) REFERENCES public.workflow_decisions(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_decision_reviewers workflow_decision_reviewers_notice_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decision_reviewers
    ADD CONSTRAINT workflow_decision_reviewers_notice_fkey FOREIGN KEY (notice_delivery_id) REFERENCES public.message_deliveries(id);








--
-- Name: workflow_decisions workflow_decisions_wait_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_decisions
    ADD CONSTRAINT workflow_decisions_wait_fkey FOREIGN KEY (company_id, run_id, execution_id, wait_id) REFERENCES public.workflow_waits(company_id, run_id, execution_id, id) ON DELETE CASCADE;


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
-- Name: workflow_tool_registrations workflow_tool_registrations_company_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_tool_registrations
    ADD CONSTRAINT workflow_tool_registrations_company_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: workflow_tool_registrations workflow_tool_registration_pin; Type: FK CONSTRAINT; Schema: public; Owner: -
-- A registration pins a version of its own company only (07-D2 §D2.1).
--

ALTER TABLE ONLY public.workflow_tool_registrations
    ADD CONSTRAINT workflow_tool_registration_pin FOREIGN KEY (company_id, workflow_id, version_id) REFERENCES public.workflow_versions(company_id, workflow_id, id);


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
-- Name: workflow_message_intent_bindings workflow_message_intent_binding_intent; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intent_bindings
    ADD CONSTRAINT workflow_message_intent_binding_intent FOREIGN KEY (company_id, intent_id) REFERENCES public.workflow_message_intents(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_message_intent_bindings workflow_message_intent_binding_binding; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intent_bindings
    ADD CONSTRAINT workflow_message_intent_binding_binding FOREIGN KEY (company_id, binding_id) REFERENCES public.workflow_bindings(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_message_intent_bindings workflow_message_intent_binding_run; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intent_bindings
    ADD CONSTRAINT workflow_message_intent_binding_run FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


--
-- Name: workflow_message_intents workflow_message_intent_company; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intents
    ADD CONSTRAINT workflow_message_intent_company FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


--
-- Name: workflow_message_intents workflow_message_intent_association; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intents
    ADD CONSTRAINT workflow_message_intent_association FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES public.thread_messages(company_id, channel_id, thread_id, message_id) ON DELETE CASCADE;


--
-- Name: workflow_message_intents workflow_message_intent_reply_to; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intents
    ADD CONSTRAINT workflow_message_intent_reply_to FOREIGN KEY (company_id, reply_to_message_id) REFERENCES public.messages(company_id, id) ON DELETE SET NULL (reply_to_message_id);


--
-- Name: workflow_message_intents workflow_message_intent_responder; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intents
    ADD CONSTRAINT workflow_message_intent_responder FOREIGN KEY (company_id, responder_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (responder_principal_id);


--
-- Name: workflow_message_intents workflow_message_intent_matched_run; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_message_intents
    ADD CONSTRAINT workflow_message_intent_matched_run FOREIGN KEY (company_id, matched_run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


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
-- Name: workflow_provisioning_commands workflow_provisioning_commands_company_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_provisioning_commands
    ADD CONSTRAINT workflow_provisioning_commands_company_id_fkey FOREIGN KEY (company_id) REFERENCES public.companies(id) ON DELETE CASCADE;


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
-- Name: workflow_runs workflow_rerun_original_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_runs
    ADD CONSTRAINT workflow_rerun_original_fk FOREIGN KEY (company_id, rerun_of_run_id, workflow_id) REFERENCES public.workflow_runs(company_id, id, workflow_id);


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
-- Name: workflow_waits workflow_wait_child_link; Type: FK CONSTRAINT; Schema: public; Owner: -
--

-- A child_run wait can name only a child of its own run and execution (07-D1 §D1.3). Checked at
-- commit, so a company cascade that removes the lineage before the wait still completes.
ALTER TABLE ONLY public.workflow_waits
    ADD CONSTRAINT workflow_wait_child_link FOREIGN KEY (company_id, child_run_id, run_id, execution_id) REFERENCES public.workflow_run_parents(company_id, child_run_id, parent_run_id, parent_execution_id) DEFERRABLE INITIALLY DEFERRED;


--
-- Name: workflow_waits workflow_wait_consumed_event; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_waits
    ADD CONSTRAINT workflow_wait_consumed_event FOREIGN KEY (company_id, run_id, consumed_event_id) REFERENCES public.workflow_wait_events(company_id, run_id, id);


--
-- Name: workflow_wait_events workflow_wait_event_consumer_execution; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_wait_events
    ADD CONSTRAINT workflow_wait_event_consumer_execution FOREIGN KEY (company_id, run_id, consumed_execution_id) REFERENCES public.workflow_executions(company_id, run_id, id);


--
-- Name: workflow_wait_events workflow_wait_events_company_id_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.workflow_wait_events
    ADD CONSTRAINT workflow_wait_events_company_id_run_id_fkey FOREIGN KEY (company_id, run_id) REFERENCES public.workflow_runs(company_id, id) ON DELETE CASCADE;


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

-- Approval names one immutable operation, including both digests (05-D2 §16.3).
ALTER TABLE public.workflow_action_intents ADD CONSTRAINT workflow_action_approval_identity
    UNIQUE(company_id,run_id,execution_id,id,argument_digest,operation_digest);
ALTER TABLE public.workflow_decisions ADD CONSTRAINT workflow_decisions_action_identity
    FOREIGN KEY(company_id,run_id,execution_id,action_invocation_id,argument_digest,operation_digest)
    REFERENCES public.workflow_action_intents(company_id,run_id,execution_id,id,argument_digest,operation_digest);


-- Payload-free model usage; reservation and settlement commit with the execution checkpoint.
ALTER TABLE public.workflow_runs ADD CONSTRAINT workflow_usage_version_identity
    UNIQUE(company_id, id, workflow_id, version_id);
CREATE TABLE public.model_usage_calls (
    company_id uuid NOT NULL REFERENCES public.companies(id) ON DELETE CASCADE,
    request_id uuid NOT NULL,
    task_id uuid NOT NULL,
    agent_id uuid NOT NULL,
    workflow_id uuid,
    version_id uuid,
    run_id uuid,
    execution_id uuid,
    root_run_id uuid,
    provider text NOT NULL CHECK (octet_length(provider) BETWEEN 1 AND 128 AND provider = lower(btrim(provider))),
    model text NOT NULL CHECK (octet_length(model) BETWEEN 1 AND 256 AND model = btrim(model)),
    requested_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    reserved_input bigint NOT NULL CHECK (reserved_input BETWEEN 0 AND 2147483647),
    reserved_output bigint NOT NULL CHECK (reserved_output BETWEEN 0 AND 2147483647),
    input_tokens bigint CHECK (input_tokens BETWEEN 0 AND 2147483647),
    output_tokens bigint CHECK (output_tokens BETWEEN 0 AND 2147483647),
    usage_source text CHECK (usage_source IN ('reported', 'estimated', 'mixed')),
    settled_at timestamptz,
    PRIMARY KEY(company_id, request_id),
    CHECK (reserved_input + reserved_output > 0),
    CHECK ((input_tokens IS NULL AND output_tokens IS NULL AND usage_source IS NULL AND settled_at IS NULL)
        OR (input_tokens IS NOT NULL AND output_tokens IS NOT NULL AND usage_source IS NOT NULL AND settled_at IS NOT NULL)),
    CHECK (settled_at IS NULL OR settled_at >= requested_at),
    -- Every model call belongs to a workflow run (p08 D4.8).
    CHECK (num_nonnulls(workflow_id, version_id, run_id, execution_id, root_run_id) = 5),
    FOREIGN KEY(company_id, task_id) REFERENCES public.background_tasks(company_id,id) ON DELETE CASCADE,
    FOREIGN KEY(company_id, agent_id) REFERENCES public.agents(company_id,id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(company_id, run_id, workflow_id, version_id)
        REFERENCES public.workflow_runs(company_id,id,workflow_id,version_id) ON DELETE CASCADE,
    FOREIGN KEY(company_id, run_id, execution_id)
        REFERENCES public.workflow_executions(company_id,run_id,id) ON DELETE CASCADE,
    FOREIGN KEY(company_id, execution_id, task_id)
        REFERENCES public.background_tasks(company_id,workflow_execution_id,id) ON DELETE CASCADE,
    FOREIGN KEY(company_id, run_id, root_run_id)
        REFERENCES public.workflow_run_budgets(company_id,run_id,root_run_id) ON DELETE CASCADE
);
CREATE INDEX model_usage_company_time ON public.model_usage_calls(company_id,requested_at,request_id);
-- A run's calls, for the simulation's per-run usage (p08 D4.6) and the run delete cascade.
CREATE INDEX model_usage_calls_run ON public.model_usage_calls(company_id,run_id) WHERE run_id IS NOT NULL;
CREATE FUNCTION public.guard_model_usage_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (to_jsonb(NEW) - ARRAY['input_tokens','output_tokens','usage_source','settled_at'])
        IS DISTINCT FROM (to_jsonb(OLD) - ARRAY['input_tokens','output_tokens','usage_source','settled_at'])
        OR (OLD.settled_at IS NOT NULL AND NEW IS DISTINCT FROM OLD) THEN
        RAISE EXCEPTION 'model usage identity and settled usage are immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER model_usage_immutable BEFORE UPDATE ON public.model_usage_calls
    FOR EACH ROW EXECUTE FUNCTION public.guard_model_usage_update();

CREATE SEQUENCE public.model_price_revision;
CREATE TABLE public.model_prices (
    company_id uuid NOT NULL REFERENCES public.companies(id) ON DELETE CASCADE,
    provider text NOT NULL CHECK (octet_length(provider) BETWEEN 1 AND 128 AND provider = lower(btrim(provider))),
    model text NOT NULL CHECK (octet_length(model) BETWEEN 1 AND 256 AND model = btrim(model)),
    input_micro_usd_per_million bigint NOT NULL CHECK (input_micro_usd_per_million BETWEEN 0 AND 1000000000000),
    output_micro_usd_per_million bigint NOT NULL CHECK (output_micro_usd_per_million BETWEEN 0 AND 1000000000000),
    revision bigint NOT NULL DEFAULT nextval('public.model_price_revision') CHECK (revision > 0),
    PRIMARY KEY(company_id,provider,model)
);

CREATE FUNCTION public.guard_model_usage_task() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM public.background_tasks AS task
        WHERE task.company_id=NEW.company_id AND task.id=NEW.task_id
          AND NEW.run_id IS NOT NULL AND task.workflow_execution_id=NEW.execution_id) THEN
        RAISE EXCEPTION 'model usage requires exact task provenance' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER model_usage_task_scope BEFORE INSERT ON public.model_usage_calls
    FOR EACH ROW EXECUTE FUNCTION public.guard_model_usage_task();


-- Successful approval continuations have their own bounded debit and immutable subject.
ALTER TABLE public.workflow_decisions
    ADD FOREIGN KEY(company_id,execution_id,job_id)
        REFERENCES public.background_tasks(company_id,workflow_execution_id,id) DEFERRABLE INITIALLY DEFERRED,
    ADD FOREIGN KEY(job_id,parked_attempt_number)
        REFERENCES public.task_attempts(task_id,attempt_number) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE public.workflow_run_events
    ADD FOREIGN KEY(company_id,run_id,execution_id,decision_id)
        REFERENCES public.workflow_decisions(company_id,run_id,execution_id,id) DEFERRABLE INITIALLY DEFERRED,
    ADD CHECK (decision_id IS NULL OR execution_id IS NOT NULL),
    ADD CONSTRAINT workflow_approval_event_kind CHECK ((decision_id IS NOT NULL) =
        (event_kind IN ('action_approval_opened','action_approved','token_allowance_opened','token_allowance_approved','token_allowance_rejected','token_allowance_expired',
            'http.approval_rejected','http.approval_expired',
            'mcp.approval_rejected','mcp.approval_expired',
            'message.approval_rejected','message.approval_expired')));
CREATE UNIQUE INDEX workflow_approval_event_identity ON public.workflow_run_events(company_id,run_id,decision_id,event_kind) WHERE decision_id IS NOT NULL;

CREATE FUNCTION public.workflow_continuation_counter_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.continuation_count<>0 THEN
            RAISE EXCEPTION 'workflow continuation must begin at zero' USING ERRCODE='23514';
        END IF;
    ELSIF NEW.continuation_count<>OLD.continuation_count AND NOT (
        OLD.queue_kind='workflow' AND NEW.queue_kind='workflow'
        AND OLD.status='completed' AND NEW.status='pending'
        AND NEW.continuation_count=OLD.continuation_count+1
        AND NEW.retry_count=OLD.retry_count) THEN
        RAISE EXCEPTION 'workflow continuation requires an approval reopen' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_continuation_counter_guard BEFORE INSERT OR UPDATE ON public.background_tasks
    FOR EACH ROW EXECUTE FUNCTION public.workflow_continuation_counter_guard();

CREATE FUNCTION public.workflow_decision_wait_pair_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM workflow_waits WHERE company_id=NEW.company_id AND id=NEW.id AND reason='decision')
        AND NOT EXISTS(SELECT 1 FROM workflow_decisions WHERE company_id=NEW.company_id AND wait_id=NEW.id) THEN
        RAISE EXCEPTION 'workflow decision wait requires its exact decision' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_decision_wait_pair_guard AFTER INSERT OR UPDATE ON public.workflow_waits
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_decision_wait_pair_guard();

CREATE FUNCTION public.workflow_approval_audit_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP<>'INSERT' AND OLD.decision_id IS NOT NULL THEN
        IF TG_OP='DELETE' AND pg_trigger_depth()>1
            AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id)
        THEN RETURN OLD; END IF;
        RAISE EXCEPTION 'workflow approval audit is append-only' USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    IF TG_OP='UPDATE' AND NEW.decision_id IS NOT NULL THEN
        RAISE EXCEPTION 'workflow event cannot become an approval audit' USING ERRCODE='23514';
    END IF;
    IF NEW.decision_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM workflow_decisions
        WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND execution_id=NEW.execution_id
            AND id=NEW.decision_id AND ((subject_kind='action_approval' AND NEW.event_kind NOT LIKE 'token_allowance_%') OR (subject_kind='token_allowance' AND NEW.event_kind LIKE 'token_allowance_%'))) THEN
        RAISE EXCEPTION 'workflow approval audit requires its exact decision' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_approval_audit_guard BEFORE INSERT OR UPDATE OR DELETE ON public.workflow_run_events
    FOR EACH ROW EXECUTE FUNCTION public.workflow_approval_audit_guard();

CREATE FUNCTION public.workflow_decision_wait_delete_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM workflow_waits WHERE company_id=OLD.company_id AND id=OLD.wait_id)
        AND NOT EXISTS(SELECT 1 FROM workflow_decisions WHERE company_id=OLD.company_id AND wait_id=OLD.wait_id) THEN
        RAISE EXCEPTION 'workflow decision wait requires its exact decision' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER workflow_decision_wait_delete_guard AFTER DELETE ON public.workflow_decisions
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.workflow_decision_wait_delete_guard();

-- Native checkpoint owner relationships are exact, tenant-scoped and retained across claims.
CREATE UNIQUE INDEX task_harness_runs_workflow_execution
    ON public.task_harness_runs(company_id,workflow_execution_id) WHERE owner_kind='workflow';
ALTER TABLE public.task_harness_runs
    ADD FOREIGN KEY(company_id,workflow_run_id,workflow_execution_id)
        REFERENCES public.workflow_executions(company_id,run_id,id) ON DELETE CASCADE,
    ADD FOREIGN KEY(company_id,workflow_execution_id,task_id)
        REFERENCES public.background_tasks(company_id,workflow_execution_id,id) ON DELETE CASCADE,
    ADD FOREIGN KEY(task_id,workflow_attempt,workflow_generation,workflow_worker_id)
        REFERENCES public.task_attempts(task_id,attempt_number,execution_generation,worker_id),
    ADD FOREIGN KEY(task_id,model_attempt,model_generation,model_worker_id)
        REFERENCES public.task_attempts(task_id,attempt_number,execution_generation,worker_id);


-- Root token authorization is a decision fact, never an independent per-call ledger.
ALTER TABLE public.workflow_decisions
    ADD FOREIGN KEY(company_id,job_id,checkpoint_id) REFERENCES public.task_harness_runs(company_id,task_id,id);
ALTER TABLE public.task_harness_runs
    ADD FOREIGN KEY(company_id,workflow_run_id,workflow_execution_id,model_blocked_decision)
        REFERENCES public.workflow_decisions(company_id,run_id,execution_id,id) DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION public.apply_workflow_token_decision() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE root uuid;
BEGIN
    IF NEW.subject_kind<>'token_allowance' THEN RETURN NEW; END IF;
    SELECT root_run_id INTO STRICT root FROM workflow_run_budgets WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
    IF NEW.choice='approve' THEN
        UPDATE workflow_root_budget_usage SET authorized_tokens=GREATEST(authorized_tokens,(NEW.token_proposal->>'proposed_allowance')::bigint)
            WHERE company_id=NEW.company_id AND root_run_id=root AND token_refusal IS NULL;
        IF NOT FOUND THEN RAISE EXCEPTION 'root token continuation already refused' USING ERRCODE='23514'; END IF;
    ELSIF NEW.choice='reject' THEN
        UPDATE workflow_root_budget_usage SET token_refusal=COALESCE(token_refusal,'rejected') WHERE company_id=NEW.company_id AND root_run_id=root;
    ELSE RAISE EXCEPTION 'invalid token decision choice' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER apply_workflow_token_decision AFTER UPDATE ON public.workflow_decisions
    FOR EACH ROW EXECUTE FUNCTION public.apply_workflow_token_decision();

CREATE FUNCTION public.refuse_expired_workflow_tokens() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state='waiting' AND NEW.state IN ('expired','cancelled') THEN
        UPDATE workflow_root_budget_usage AS usage SET token_refusal=COALESCE(usage.token_refusal,NEW.state)
        FROM workflow_run_budgets AS link, workflow_decisions AS decision
        WHERE decision.company_id=NEW.company_id AND decision.wait_id=NEW.id AND decision.subject_kind='token_allowance'
            AND link.company_id=decision.company_id AND link.run_id=decision.run_id
            AND usage.company_id=link.company_id AND usage.root_run_id=link.root_run_id;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER refuse_expired_workflow_tokens AFTER UPDATE ON public.workflow_waits
    FOR EACH ROW EXECUTE FUNCTION public.refuse_expired_workflow_tokens();

CREATE FUNCTION public.valid_workflow_token_proposal(proposal jsonb, stage text, request uuid) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE field text; amount bigint; original bigint; previous bigint; proposed bigint;
BEGIN
    IF jsonb_typeof(proposal) IS DISTINCT FROM 'object'
        OR NOT proposal ?& ARRAY['original_allowance','old_allowance','proposed_allowance','usage','request']
        OR proposal-ARRAY['original_allowance','old_allowance','proposed_allowance','usage','request']<>'{}'::jsonb
        OR jsonb_typeof(proposal->'usage') IS DISTINCT FROM 'object'
        OR NOT (proposal->'usage') ?& ARRAY['reported','estimated','mixed','unconfirmed']
        OR (proposal->'usage')-ARRAY['reported','estimated','mixed','unconfirmed']<>'{}'::jsonb THEN RETURN false; END IF;
    FOREACH field IN ARRAY ARRAY['original_allowance','old_allowance','proposed_allowance'] LOOP
        IF jsonb_typeof(proposal->field) IS DISTINCT FROM 'number' OR proposal->>field !~ '^[0-9]+$' THEN RETURN false; END IF;
        amount=(proposal->>field)::bigint;
        IF amount<=0 THEN RETURN false; END IF;
    END LOOP;
    original=(proposal->>'original_allowance')::bigint;
    previous=(proposal->>'old_allowance')::bigint;
    proposed=(proposal->>'proposed_allowance')::bigint;
    IF original>2147483647 OR previous<original OR proposed<previous OR proposed-previous<original THEN RETURN false; END IF;
    FOREACH field IN ARRAY ARRAY['reported','estimated','mixed','unconfirmed'] LOOP
        IF jsonb_typeof(proposal->'usage'->field) IS DISTINCT FROM 'number' OR proposal->'usage'->>field !~ '^[0-9]+$' THEN RETURN false; END IF;
        amount=(proposal->'usage'->>field)::bigint;
        IF amount<0 OR amount>4294967294000 THEN RETURN false; END IF;
    END LOOP;
    IF stage='settlement' THEN RETURN proposal->'request'='null'::jsonb; END IF;
    IF stage<>'reservation' OR jsonb_typeof(proposal->'request') IS DISTINCT FROM 'object'
        OR proposal->'request'->>'fingerprint' IS NULL OR proposal->'request'->>'fingerprint' !~ '^[0-9a-f]{64}$'
        OR proposal->'request'->'reservation'->>'request_id' IS DISTINCT FROM request::text THEN RETURN false; END IF;
    FOREACH field IN ARRAY ARRAY['input_tokens','output_tokens'] LOOP
        IF jsonb_typeof(proposal->'request'->'reservation'->field) IS DISTINCT FROM 'number'
            OR proposal->'request'->'reservation'->>field !~ '^[0-9]+$' THEN RETURN false; END IF;
        amount=(proposal->'request'->'reservation'->>field)::bigint;
        IF amount<1 OR amount>2147483647 THEN RETURN false; END IF;
    END LOOP;
    RETURN true;
EXCEPTION WHEN OTHERS THEN RETURN false;
END $$;
ALTER TABLE public.workflow_decisions ADD CONSTRAINT workflow_token_proposal_shape
    CHECK (subject_kind<>'token_allowance' OR public.valid_workflow_token_proposal(token_proposal,token_stage,token_request_id));

CREATE FUNCTION public.guard_workflow_token_proposal() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE root uuid; policy jsonb; allowance bigint; refused text; observed jsonb; needed bigint;
BEGIN
    IF NEW.subject_kind<>'token_allowance' THEN RETURN NEW; END IF;
    IF NOT public.valid_workflow_token_proposal(NEW.token_proposal,NEW.token_stage,NEW.token_request_id) THEN
        RAISE EXCEPTION 'invalid token proposal' USING ERRCODE='23514'; END IF;
    SELECT usage.root_run_id,budget.token_policy,GREATEST(usage.authorized_tokens,(budget.token_policy->>'max_tokens')::bigint),usage.token_refusal
        INTO STRICT root,policy,allowance,refused FROM workflow_run_budgets AS link
        JOIN workflow_root_budget_usage AS usage USING(company_id,root_run_id)
        JOIN workflow_root_budgets AS budget USING(company_id,root_run_id)
        WHERE link.company_id=NEW.company_id AND link.run_id=NEW.run_id FOR UPDATE OF usage;
    IF policy IS NULL OR refused IS NOT NULL OR (NEW.token_proposal->>'original_allowance')::bigint<>(policy->>'max_tokens')::bigint
        OR (NEW.token_proposal->>'old_allowance')::bigint<>allowance THEN
        RAISE EXCEPTION 'token proposal does not match current root allowance' USING ERRCODE='23514'; END IF;
    SELECT jsonb_build_object(
        'reported',COALESCE(sum(input_tokens+output_tokens) FILTER(WHERE usage_source='reported'),0),
        'estimated',COALESCE(sum(input_tokens+output_tokens) FILTER(WHERE usage_source='estimated'),0),
        'mixed',COALESCE(sum(input_tokens+output_tokens) FILTER(WHERE usage_source='mixed'),0),
        'unconfirmed',COALESCE(sum(reserved_input+reserved_output) FILTER(WHERE settled_at IS NULL),0))
        INTO observed FROM model_usage_calls WHERE company_id=NEW.company_id AND root_run_id=root;
    needed=CASE WHEN NEW.token_stage='reservation' THEN (NEW.token_proposal->'request'->'reservation'->>'input_tokens')::bigint
        +(NEW.token_proposal->'request'->'reservation'->>'output_tokens')::bigint ELSE 0 END;
    IF NEW.token_proposal->'usage' IS DISTINCT FROM observed
        OR (NEW.token_proposal->>'proposed_allowance')::bigint<needed+(observed->>'reported')::bigint
            +(observed->>'estimated')::bigint+(observed->>'mixed')::bigint+(observed->>'unconfirmed')::bigint THEN
        RAISE EXCEPTION 'token proposal does not cover observed usage' USING ERRCODE='23514'; END IF;
    IF NOT EXISTS(SELECT 1 FROM task_harness_runs AS checkpoint JOIN background_tasks AS job ON job.company_id=checkpoint.company_id AND job.id=checkpoint.task_id
        WHERE checkpoint.company_id=NEW.company_id AND checkpoint.id=NEW.checkpoint_id AND checkpoint.workflow_run_id=NEW.run_id
            AND checkpoint.workflow_execution_id=NEW.execution_id AND checkpoint.task_id=NEW.job_id AND checkpoint.revision=NEW.checkpoint_revision
            AND checkpoint.model_blocked_decision=NEW.id AND checkpoint.model_request_id IS NULL AND checkpoint.state='active'
            AND job.status='processing' AND job.attempt_ordinal=NEW.parked_attempt_number AND job.lock_expires_at>clock_timestamp()
            AND job.worker_id=checkpoint.workflow_worker_id AND job.execution_generation=checkpoint.workflow_generation
            AND ((NEW.token_stage='settlement' AND EXISTS(SELECT 1 FROM jsonb_array_elements(checkpoint.checkpoint->'turns') AS turn WHERE turn->>'request_id'=NEW.token_request_id::text))
                OR (NEW.token_stage='reservation' AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(checkpoint.checkpoint->'reservations') AS reservation WHERE reservation->>'request_id'=NEW.token_request_id::text)))) THEN
        RAISE EXCEPTION 'token proposal requires its exact live checkpoint' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER guard_workflow_token_proposal BEFORE INSERT ON public.workflow_decisions
    FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_token_proposal();

-- The deferred association permits checkpoint-first parking, but never an
-- arbitrary pointer, an unapproved consumption, or substitution of a request.
CREATE FUNCTION public.guard_harness_token_pause() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE prior public.workflow_decisions%ROWTYPE; next_decision public.workflow_decisions%ROWTYPE;
BEGIN
    IF NEW.owner_kind<>'workflow' THEN RETURN NEW; END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.model_blocked_decision IS NOT NULL THEN
            RAISE EXCEPTION 'new checkpoint cannot inherit token approval' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    IF OLD.model_blocked_decision IS NOT DISTINCT FROM NEW.model_blocked_decision THEN
        IF OLD.model_blocked_decision IS NOT NULL AND
            (NEW.checkpoint-'revision'-'executions' IS DISTINCT FROM OLD.checkpoint-'revision'-'executions') THEN
            RAISE EXCEPTION 'paused checkpoint may only reserve execution time' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    IF OLD.model_blocked_decision IS NOT NULL THEN
        SELECT * INTO STRICT prior FROM public.workflow_decisions AS decision
            WHERE decision.company_id=OLD.company_id AND decision.id=OLD.model_blocked_decision;
        IF prior.choice IS DISTINCT FROM 'approve' OR NOT EXISTS (
            SELECT 1 FROM public.workflow_waits AS wait WHERE wait.company_id=prior.company_id
                AND wait.id=prior.wait_id AND wait.state='completed') THEN
            RAISE EXCEPTION 'token pause requires completed approval' USING ERRCODE='23514';
        END IF;
        IF NEW.model_blocked_decision IS NULL AND prior.token_stage='reservation'
            AND (NEW.model_request_id IS NULL
                OR NEW.checkpoint->'reservations'->-1 IS DISTINCT FROM prior.token_proposal->'request'->'reservation') THEN
            RAISE EXCEPTION 'token approval consumption requires exact model reservation' USING ERRCODE='23514';
        END IF;
        IF NEW.model_blocked_decision IS NULL AND prior.token_stage='settlement' THEN
            PERFORM public.require_workflow_token_progression(NEW.company_id,NEW.workflow_run_id);
        END IF;
    END IF;
    IF NEW.model_blocked_decision IS NOT NULL THEN
        SELECT * INTO STRICT next_decision FROM public.workflow_decisions AS decision
            WHERE decision.company_id=NEW.company_id AND decision.id=NEW.model_blocked_decision;
        IF next_decision.subject_kind<>'token_allowance'
            OR ROW(next_decision.run_id,next_decision.execution_id,next_decision.job_id,next_decision.checkpoint_id,next_decision.checkpoint_revision)
                IS DISTINCT FROM ROW(NEW.workflow_run_id,NEW.workflow_execution_id,NEW.task_id,NEW.id,NEW.revision)
            OR NEW.model_request_id IS NOT NULL
            OR NEW.checkpoint-'revision' IS DISTINCT FROM OLD.checkpoint-'revision'
            OR (OLD.model_blocked_decision IS NOT NULL AND prior.token_stage='reservation'
                AND next_decision.token_proposal->'request' IS DISTINCT FROM prior.token_proposal->'request') THEN
            RAISE EXCEPTION 'token pause requires exact saved proposal' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER guard_harness_token_pause AFTER INSERT OR UPDATE ON public.task_harness_runs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.guard_harness_token_pause();

CREATE FUNCTION public.require_workflow_token_progression(company uuid, run uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE refused text; allowance bigint; observed numeric;
BEGIN
    SELECT usage.token_refusal,GREATEST(usage.authorized_tokens,(budget.token_policy->>'max_tokens')::bigint)
        INTO refused,allowance FROM workflow_run_budgets AS link
        JOIN workflow_root_budget_usage AS usage USING(company_id,root_run_id)
        JOIN workflow_root_budgets AS budget USING(company_id,root_run_id)
        WHERE link.company_id=company AND link.run_id=run FOR UPDATE OF usage;
    -- Invalid associations remain the composite foreign keys' responsibility.
    -- Every valid run creates its immutable budget link in its insert trigger.
    IF NOT FOUND THEN RETURN; END IF;
    IF refused IS NOT NULL THEN
        RAISE EXCEPTION 'workflow token continuation refused: %',refused USING ERRCODE='23514';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM workflow_run_budgets AS link
        JOIN workflow_root_budgets AS budget USING(company_id,root_run_id)
        WHERE link.company_id=company AND link.run_id=run AND budget.token_policy IS NOT NULL) THEN RETURN; END IF;
    SELECT COALESCE(sum(CASE WHEN call.settled_at IS NULL THEN call.reserved_input+call.reserved_output
        ELSE call.input_tokens+call.output_tokens END),0) INTO observed FROM model_usage_calls AS call
        JOIN workflow_run_budgets AS link ON link.company_id=call.company_id AND link.root_run_id=call.root_run_id
        WHERE link.company_id=company AND link.run_id=run;
    IF observed>allowance THEN
        RAISE EXCEPTION 'workflow token approval required' USING ERRCODE='23514';
    END IF;
END $$;

CREATE FUNCTION public.guard_workflow_token_progression() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM public.require_workflow_token_progression(NEW.company_id,NEW.run_id);
    RETURN NEW;
END $$;
CREATE TRIGGER guard_workflow_token_dispatch BEFORE INSERT ON public.workflow_action_dispatches
    FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_token_progression();
CREATE TRIGGER guard_workflow_token_remote_entry BEFORE INSERT ON public.workflow_action_remote_entries
    FOR EACH ROW EXECUTE FUNCTION public.guard_workflow_token_progression();
CREATE TRIGGER guard_workflow_token_completion BEFORE UPDATE OF completed_at ON public.workflow_executions
    FOR EACH ROW WHEN (OLD.completed_at IS NULL AND NEW.completed_at IS NOT NULL)
    EXECUTE FUNCTION public.guard_workflow_token_progression();

CREATE FUNCTION public.guard_harness_token_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM public.require_workflow_token_progression(NEW.company_id,NEW.workflow_run_id);
    RETURN NEW;
END $$;
CREATE TRIGGER guard_harness_token_admission BEFORE UPDATE OF model_request_id ON public.task_harness_runs
    FOR EACH ROW WHEN (NEW.owner_kind='workflow' AND OLD.model_request_id IS NULL AND NEW.model_request_id IS NOT NULL)
    EXECUTE FUNCTION public.guard_harness_token_admission();

CREATE FUNCTION public.guard_harness_workflow_progression() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE progresses boolean;
BEGIN
    progresses = jsonb_array_length(NEW.checkpoint->'invocations')>jsonb_array_length(OLD.checkpoint->'invocations')
        OR NEW.checkpoint->'final_output' IS DISTINCT FROM OLD.checkpoint->'final_output'
        OR NEW.checkpoint->'tool_catalogue_fingerprint' IS DISTINCT FROM OLD.checkpoint->'tool_catalogue_fingerprint';
    IF NEW.model_request_id IS NOT NULL AND
        (NEW.checkpoint->'invocations' IS DISTINCT FROM OLD.checkpoint->'invocations'
            OR NEW.checkpoint->'final_output' IS DISTINCT FROM OLD.checkpoint->'final_output'
            OR NEW.checkpoint->'tool_catalogue_fingerprint' IS DISTINCT FROM OLD.checkpoint->'tool_catalogue_fingerprint'
            OR NEW.state IS DISTINCT FROM OLD.state) THEN
        RAISE EXCEPTION 'active model request prevents checkpoint progression' USING ERRCODE='23514';
    END IF;
    IF progresses THEN
        IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(NEW.checkpoint->'executions') AS execution
            WHERE execution->>'generation'=NEW.workflow_generation::text) THEN
            RAISE EXCEPTION 'checkpoint progression requires execution reservation' USING ERRCODE='23514';
        END IF;
        PERFORM public.require_workflow_token_progression(NEW.company_id,NEW.workflow_run_id);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER guard_harness_workflow_progression BEFORE UPDATE ON public.task_harness_runs
    FOR EACH ROW WHEN (NEW.owner_kind='workflow')
    EXECUTE FUNCTION public.guard_harness_workflow_progression();

CREATE FUNCTION public.guard_workflow_token_reopen() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- Wait completion temporarily marks the owner running before its rejection
    -- or expiry handler closes it. Only the committed state grants progression.
    IF EXISTS(SELECT 1 FROM workflow_runs AS run WHERE run.company_id=NEW.company_id
        AND run.id=NEW.id AND run.state IN ('queued','running')) THEN
        PERFORM public.require_workflow_token_progression(NEW.company_id,NEW.id);
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER guard_workflow_token_reopen AFTER UPDATE ON public.workflow_runs
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN (OLD.state NOT IN ('queued','running') AND NEW.state IN ('queued','running'))
    EXECUTE FUNCTION public.guard_workflow_token_reopen();
