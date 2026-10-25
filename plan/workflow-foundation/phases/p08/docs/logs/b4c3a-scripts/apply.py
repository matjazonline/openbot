import re, sys
sys.path.insert(0, '.')
import plan as P
P.NAMED |= {
    'background_tasks background_tasks_notify_activity',
    'background_tasks background_tasks_notify_attention',
    'background_tasks background_tasks_notify_attention_delete',
    'task_harness_runs task_harness_runs_company_id_owner_principal_id_fkey',
    'background_tasks_unsettled_channel_idx',
    'CONSTRAINT background_tasks_company_source_message_channel_key ON background_tasks',
}
KEEP = {'task_harness_runs require_legacy_auxiliary_task'}  # also holds the two live harness triggers
lines, segs, tail, drop = P.plan()
drop = [(s, w) for s, w in drop if s['name'] not in KEEP]
missing = [n for n in P.NAMED if not any(s['name'] == n for s, _ in drop)]
assert not missing, missing
gone = set()
for s, _ in drop:
    gone.update(range(s['start'], s['end']))
text = '\n'.join(l for i, l in enumerate(lines) if i not in gone)
print('dropped segments', len(drop), 'lines', len(gone))

def rep(old, new, n=1):
    global text
    c = text.count(old)
    assert c == n, (c, old[:80])
    text = text.replace(old, new)

# --- background_tasks: workflow columns and checks only (D4.8, DV-B4c3-1)
start = text.index('CREATE TABLE public.background_tasks (\n')
end = text.index('\n);', start) + 3
text = text[:start] + '''CREATE TABLE public.background_tasks (
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
);''' + text[end:]

# --- task_attempts (DV-B4c3-4)
rep("""    status text NOT NULL,
    error text,
    stop_reason text,
    prompt_tokens integer,
    completion_tokens integer,
    result jsonb,
    started_at""", """    status text NOT NULL,
    stop_reason text,
    started_at""")
rep("""    CONSTRAINT task_attempts_stop_reason_check CHECK ((stop_reason = ANY (ARRAY['completed'::text, 'retryable_failure'::text, 'terminal_failure'::text, 'timed_out'::text, 'shutdown'::text, 'lease_lost'::text, 'ownership_transferred'::text, 'agent_instruction'::text, 'delegation_cancelled'::text, 'operator_stopped'::text, 'approval_rejected'::text, 'channel_agent_removed'::text, 'effect_parked'::text, 'approval_parked'::text, 'child_parked'::text]))),
    CONSTRAINT task_attempts_token_check CHECK ((((prompt_tokens IS NULL) OR (prompt_tokens >= 0)) AND ((completion_tokens IS NULL) OR (completion_tokens >= 0)))),
""", """    CONSTRAINT task_attempts_stop_reason_check CHECK ((stop_reason = ANY (ARRAY['completed'::text, 'timed_out'::text, 'lease_lost'::text, 'effect_parked'::text, 'approval_parked'::text, 'child_parked'::text]))),
""")

# --- task_harness_runs: the workflow owner arm only (D4.8, DV-B4c3-2)
rep("""    owner_kind text NOT NULL DEFAULT 'legacy',
    owner_principal_id uuid,
    ownership_version bigint,
""", """    owner_kind text NOT NULL DEFAULT 'workflow',
""")
rep("""    CONSTRAINT task_harness_runs_owner_shape CHECK (
        (owner_kind = 'legacy' AND owner_principal_id IS NOT NULL AND ownership_version IS NOT NULL
            AND num_nonnulls(workflow_run_id,workflow_execution_id,workflow_generation,
                workflow_worker_id,workflow_attempt,workflow_prepared) = 0)
        OR (owner_kind = 'workflow' AND owner_principal_id IS NULL AND ownership_version IS NULL
            AND num_nonnulls(workflow_run_id,workflow_execution_id,workflow_generation,
                workflow_worker_id,workflow_attempt,workflow_prepared) = 6
            AND workflow_attempt > 0 AND final_output_consumed_at IS NULL
            AND jsonb_typeof(workflow_prepared) = 'object'
            AND octet_length(workflow_prepared::text) <= 2097152)),
""", """    CONSTRAINT task_harness_runs_owner_shape CHECK (
        owner_kind = 'workflow'
            AND num_nonnulls(workflow_run_id,workflow_execution_id,workflow_generation,
                workflow_worker_id,workflow_attempt,workflow_prepared) = 6
            AND workflow_attempt > 0 AND final_output_consumed_at IS NULL
            AND jsonb_typeof(workflow_prepared) = 'object'
            AND octet_length(workflow_prepared::text) <= 2097152),
""")
rep("""    CONSTRAINT task_harness_runs_ownership_version_check CHECK ((ownership_version >= 0)),
""", "")
rep("""-- Native checkpoints share serialization with legacy runs, never legacy queue authority.
CREATE FUNCTION public.require_harness_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND
        ROW(NEW.id,NEW.company_id,NEW.task_id,NEW.agent_id,NEW.owner_kind,
            NEW.owner_principal_id,NEW.ownership_version,NEW.workflow_run_id,
            NEW.workflow_execution_id,NEW.workflow_prepared)
        IS DISTINCT FROM
        ROW(OLD.id,OLD.company_id,OLD.task_id,OLD.agent_id,OLD.owner_kind,
            OLD.owner_principal_id,OLD.ownership_version,OLD.workflow_run_id,
            OLD.workflow_execution_id,OLD.workflow_prepared) THEN
        RAISE EXCEPTION 'harness owner and prepared inputs are immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.owner_kind = 'legacy' THEN
        IF NOT EXISTS (SELECT 1 FROM public.background_tasks AS task
            WHERE task.company_id=NEW.company_id AND task.id=NEW.task_id AND task.queue_kind='legacy') THEN
            RAISE EXCEPTION 'legacy harness requires legacy task'
                USING ERRCODE='23514', CONSTRAINT='legacy_auxiliary_task_queue_kind';
        END IF;
        RETURN NEW;
    END IF;
""", """-- A native checkpoint keeps its owner and prepared inputs, and writes only under a live workflow fence.
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
""")
rep("""-- Name: task_harness_runs require_legacy_auxiliary_task; Type: TRIGGER; Schema: public; Owner: -""",
    """-- Name: task_harness_runs require_harness_owner; Type: TRIGGER; Schema: public; Owner: -""")
rep("""    FOR EACH ROW EXECUTE FUNCTION public.require_harness_owner();
CREATE TRIGGER require_legacy_auxiliary_task BEFORE INSERT OR UPDATE ON public.task_harness_invocations
    FOR EACH ROW EXECUTE FUNCTION public.require_legacy_auxiliary_task();
""", """    FOR EACH ROW EXECUTE FUNCTION public.require_harness_owner();
""")

# --- message_deliveries: no task link, no legacy cancellation reasons (D4.3)
start = text.index('CREATE TABLE public.message_deliveries (\n')
end = text.index('\n);', start) + 3
block = text[start:end]
for old, new in [
    ("    external_destination text,\n    task_id uuid,\n", "    external_destination text,\n"),
    (" AND (destination_binding_id IS NULL) AND (task_id IS NULL) AND", " AND (destination_binding_id IS NULL) AND"),
    ("ARRAY['channel_agent_removed'::text, 'task_stopped'::text, 'workflow_cancelled'::text", "ARRAY['workflow_cancelled'::text"),
]:
    assert block.count(old) == 1, old
    block = block.replace(old, new)
text = text[:start] + block + text[end:]

# --- notifications: the delivery source and action only (D4.3)
rep("CHECK ((action_kind = ANY (ARRAY['assignment'::text, 'response_review'::text, 'delegation_timeout'::text, 'task_failure'::text, 'delivery_failure'::text])))",
    "CHECK ((action_kind = 'delivery_failure'::text))", n=2)
rep("CHECK ((source_kind = ANY (ARRAY['task'::text, 'handoff'::text, 'response_review'::text, 'delegation'::text, 'delivery'::text])))",
    "CHECK ((source_kind = 'delivery'::text))", n=2)
rep("""    user_id uuid NOT NULL,
    assignment_email_enabled boolean DEFAULT true NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    response_review_email_enabled boolean DEFAULT true CONSTRAINT user_notification_preferenc_response_review_email_enab_not_null NOT NULL,
    delegation_timeout_email_enabled boolean DEFAULT true CONSTRAINT user_notification_preferenc_delegation_timeout_email_e_not_null NOT NULL,
    task_failure_email_enabled boolean DEFAULT true CONSTRAINT user_notification_preferenc_task_failure_email_enabled_not_null NOT NULL,
""", """    user_id uuid NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
""")
rep("""    -- Existing actionable notifications belong to the legacy task runtime. Preserve
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
""", "")
rep("""DECLARE
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

    IF company""", """DECLARE
    row_data JSONB := CASE WHEN TG_OP = 'DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END;
    source_id UUID := NULLIF(row_data->>'id', '')::UUID;
    company UUID := NULLIF(row_data->>'company_id', '')::UUID;
    channel UUID := NULLIF(row_data->>'channel_id', '')::UUID;
BEGIN
    IF company""")
rep("""        WHERE task.company_id=NEW.company_id AND task.id=NEW.task_id
          AND ((NEW.run_id IS NULL AND task.queue_kind='legacy')
            OR (NEW.run_id IS NOT NULL AND task.queue_kind='workflow' AND task.workflow_execution_id=NEW.execution_id))) THEN""",
    """        WHERE task.company_id=NEW.company_id AND task.id=NEW.task_id
          AND NEW.run_id IS NOT NULL AND task.workflow_execution_id=NEW.execution_id) THEN""")
import migseg; open(migseg.M, 'w').write(text)
print('written', text.count('\n') + 1, 'lines')
