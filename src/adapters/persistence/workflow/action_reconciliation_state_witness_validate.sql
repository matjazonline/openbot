CREATE FUNCTION fixture_state_pending(company uuid,key text) RETURNS jsonb LANGUAGE sql AS $$
    SELECT fixture_state_pending_base(company,key)||jsonb_build_object(
        'job',(SELECT to_jsonb(job) FROM background_tasks AS job WHERE job.company_id=config.company_id AND job.id=config.job_id),
        'execution',(SELECT to_jsonb(execution) FROM workflow_executions AS execution WHERE execution.company_id=config.company_id AND execution.id=config.execution_id),
        'attempt',(SELECT to_jsonb(attempt) FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id AND job.retry_count=attempt.attempt_number WHERE job.company_id=config.company_id AND job.id=config.job_id),
        'states',COALESCE((SELECT jsonb_agg(to_jsonb(state)) FROM workflow_action_state_witnesses AS state WHERE state.company_id=config.company_id AND state.run_id=config.run_id AND state.transaction_id=COALESCE((SELECT saved.pending->>'xid' FROM fixture_state_witness AS saved WHERE saved.company_id=company AND saved.command_key=key),pg_current_xact_id()::text)::xid8),'[]'::jsonb),
        'schedules',COALESCE((SELECT jsonb_agg(to_jsonb(schedule)) FROM workflow_action_schedule_witnesses AS schedule WHERE schedule.company_id=config.company_id AND schedule.job_id=config.job_id AND schedule.transaction_id=COALESCE((SELECT saved.pending->>'xid' FROM fixture_state_witness AS saved WHERE saved.company_id=company AND saved.command_key=key),pg_current_xact_id()::text)::xid8),'[]'::jsonb),
        'available_proof',(SELECT to_jsonb(proof) FROM workflow_action_evidence AS proof WHERE proof.company_id=config.company_id AND proof.id=workflow_action_not_applied_available(config.company_id,(config.expected->'command'->>'invocation_id')::uuid,NULL)),
        'steps',COALESCE((SELECT jsonb_object_agg(step,image) FROM fixture_state_steps WHERE company_id=config.company_id AND command_key=config.command_key),'{}'::jsonb),
        'xid',COALESCE((SELECT saved.pending->>'xid' FROM fixture_state_witness AS saved WHERE saved.company_id=company AND saved.command_key=key),pg_current_xact_id()::text))
    FROM fixture_state_config AS config WHERE config.company_id=company AND config.command_key=key
$$;
CREATE FUNCTION fixture_state_images(config fixture_state_config,pending jsonb) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE capture fixture_state_capture%ROWTYPE; original jsonb; returned jsonb; changes jsonb; relation_key text; hits integer:=0; all_images jsonb:='{}'::jsonb;
BEGIN
    FOR capture IN SELECT observed.* FROM fixture_state_capture AS observed WHERE observed.company_id=config.company_id AND observed.command_key=config.command_key LOOP
        relation_key:=CASE capture.relation_name WHEN 'workflow_run_events' THEN 'audit' WHEN 'workflow_action_evidence' THEN 'evidence' WHEN 'workflow_action_evidence_commands' THEN 'command' ELSE NULL END;
        original:=capture.original_new;returned:=capture.returned_new;
        IF relation_key IS NULL OR capture.depth<>1 OR returned IS DISTINCT FROM pending->relation_key THEN RAISE EXCEPTION 'state actual inserted image mismatch'; END IF;
        SELECT COALESCE(jsonb_agg(field ORDER BY field),'[]'::jsonb) INTO changes FROM jsonb_object_keys(original) AS field WHERE original->field IS DISTINCT FROM returned->field;
        IF relation_key='command' AND config.mode IN ('native','mismatch-control') THEN
            IF changes<>to_jsonb(ARRAY['outcome','previous_state','previous_waiting_reason','result_revision','scheduled_job_id'])
                AND NOT (config.source_case='FIRST_OLD_HUMAN_REASON' AND changes=to_jsonb(ARRAY['outcome','previous_waiting_reason','result_revision','scheduled_job_id']))
                THEN RAISE EXCEPTION 'state wrong exact NEW changed-field set: %',changes; END IF;
            IF (original-ARRAY['outcome','previous_state','previous_waiting_reason','result_revision','scheduled_job_id'])
                IS DISTINCT FROM (returned-ARRAY['outcome','previous_state','previous_waiting_reason','result_revision','scheduled_job_id'])
                THEN RAISE EXCEPTION 'state protected actual NEW identity changed'; END IF;
        ELSIF changes<>'[]'::jsonb THEN RAISE EXCEPTION 'state observational capture changed NEW'; END IF;
        all_images:=all_images||jsonb_build_object(relation_key,jsonb_build_object('original',original,'returned',returned,'changed_fields',changes));hits:=hits+1;
    END LOOP;
    IF hits<>3 THEN RAISE EXCEPTION 'state three authentic depth-one captures required'; END IF;
    RETURN all_images;
END $$;
CREATE FUNCTION fixture_state_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_state_config%ROWTYPE; candidate workflow_action_evidence_commands%ROWTYPE; hits integer; pending jsonb; images jsonb; predicates jsonb; envelope jsonb;
    native_state text; native_constraint text; native_message text; native_context text; native_schema text; native_table text; expected_message text; prefix text;
BEGIN
    SELECT count(*) INTO hits FROM inserted_state_commands AS inserted JOIN fixture_state_config AS configured ON configured.company_id=inserted.company_id AND configured.command_key=inserted.command_key;
    IF hits=0 THEN RETURN NULL; END IF;
    IF hits<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'state exact owner statement cardinality/depth'; END IF;
    candidate:=(SELECT inserted FROM inserted_state_commands AS inserted JOIN fixture_state_config AS configured ON configured.company_id=inserted.company_id AND configured.command_key=inserted.command_key);
    config:=(SELECT configured FROM fixture_state_config AS configured WHERE company_id=candidate.company_id AND command_key=candidate.command_key);
    IF NOT EXISTS(SELECT 1 FROM pg_constraint AS native JOIN pg_trigger AS attached ON attached.tgconstraint=native.oid JOIN pg_proc AS function ON function.oid=attached.tgfoid
        WHERE native.oid=config.target_oid AND native.conname=config.target_name AND native.connamespace='public'::regnamespace AND native.conrelid='background_tasks'::regclass
            AND native.condeferrable AND native.condeferred AND attached.tgenabled='O' AND attached.tgtype=17 AND function.proname='check_workflow_control_retry')
        THEN RAISE EXCEPTION 'state lost exact selected native guard'; END IF;
    pending:=fixture_state_pending(candidate.company_id,candidate.command_key);images:=fixture_state_images(config,pending);
    PERFORM fixture_state_preserved(config,pending);
    IF NOT (pending->'command' @> (config.expected->'command')) OR NOT (pending->'evidence' @> (config.expected->'evidence'))
        OR candidate.evidence_id<>(pending->'evidence'->>'id')::uuid OR candidate.id<>(pending->'evidence'->>'command_id')::uuid
        OR candidate.result_revision<>(pending->'run'->>'revision')::bigint
        OR NOT (pending->'audit' @> jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,'sequence',candidate.audit_sequence,'execution_id',candidate.execution_id,'actor_id',candidate.actor_id,'event_kind','action_reconciled'))
        THEN RAISE EXCEPTION 'state genuine evidence command/audit exact binding invalid'; END IF;
    IF config.mode='no-attack' THEN
        IF candidate.outcome<>jsonb_build_object('kind','not_applied_recorded') OR candidate.scheduled_job_id IS NOT NULL
            OR pending->'states'<>'[]'::jsonb OR pending->'schedules'<>'[]'::jsonb OR pending->'episode'<>'null'::jsonb
            OR pending->'job'->>'status'<>'failed' THEN RAISE EXCEPTION 'state no-attack audit-only control invalid'; END IF;
        predicates:='{}'::jsonb;
    ELSE
        predicates:=fixture_state_predicates(config,pending);
        IF predicates->>'ordinary_receipt'<>'false' OR (predicates->>'ordinary_helper')::boolean AND (predicates->>'ordinary_receipt')::boolean
            OR (predicates->>'witness_eligible')::boolean<>(config.source_case='WAITING_RECONCILIATION')
            OR candidate.outcome<>jsonb_build_object('kind','scheduled','receipt_only',false) OR candidate.scheduled_job_id<>config.job_id
            OR pending->'available_proof'='null'::jsonb OR (pending->'available_proof'->>'valid_until')::timestamptz<=clock_timestamp()
            OR pending->'available_proof'->>'disposition'<>'final_not_applied' OR pending->'available_proof'->>'grant_eligible'<>'true'
            THEN RAISE EXCEPTION 'state isolation ordinary/witness/scheduled/proof predicates invalid'; END IF;
    END IF;
    BEGIN
        EXECUTE format('SET CONSTRAINTS public.%I IMMEDIATE',config.target_name);
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_constraint=CONSTRAINT_NAME,native_message=MESSAGE_TEXT,
            native_context=PG_EXCEPTION_CONTEXT,native_schema=SCHEMA_NAME,native_table=TABLE_NAME;
    END;
    IF config.mode IN ('positive','no-attack') THEN
        IF native_state IS NOT NULL THEN RAISE EXCEPTION 'state control selected failure: % %',native_state,native_message; END IF;
        SET CONSTRAINTS ALL IMMEDIATE;
        INSERT INTO fixture_state_witness(company_id,command_key,selected_flush,all_immediate,owner_pid,pending) VALUES(candidate.company_id,candidate.command_key,true,true,pg_backend_pid(),pending);
        RETURN NULL;
    END IF;
    envelope:=jsonb_build_object('version',1,'source_case',config.source_case,'mode',config.mode,'company_id',candidate.company_id,'run_id',candidate.run_id,'execution_id',candidate.execution_id,
        'job_id',config.job_id,'command_key',candidate.command_key,'target_oid',config.target_oid,'target_name',config.target_name,'before_hits',3,'after_hits',1,'depth',pg_trigger_depth(),
        'owner_pid',pg_backend_pid(),'database',current_database(),'xid',pg_current_xact_id()::text,'images',images,'pending',pending,'predicates',predicates,
        'native_state',native_state,'native_constraint',native_constraint,'native_message',native_message,'native_context',native_context,'native_schema',native_schema,'native_table',native_table);
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'state bounded diagnostic overflow'; END IF;
    expected_message:=CASE WHEN config.mode='mismatch-control' THEN 'deliberately wrong state witness diagnostic' ELSE 'invalid explicit workflow retry' END;
    prefix:=CASE WHEN native_state IS NULL THEN 'FIXTURE_STATE_ACCEPTED_V1:'
        WHEN COALESCE(native_state='23514' AND native_constraint='' AND native_schema='' AND native_table='' AND native_message=expected_message
            AND position('check_workflow_control_retry()' IN native_context)>0 AND position('SET CONSTRAINTS public.workflow_control_retry_guard IMMEDIATE' IN native_context)>0,false)
        THEN 'FIXTURE_STATE_NATIVE_V1:' ELSE 'FIXTURE_STATE_MISMATCH_V1:' END;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_state_command_after AFTER INSERT ON workflow_action_evidence_commands REFERENCING NEW TABLE AS inserted_state_commands FOR EACH STATEMENT EXECUTE FUNCTION fixture_state_after();
CREATE FUNCTION fixture_state_preserved(config fixture_state_config,pending jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE relation_name text; saved jsonb; current_rows jsonb; old_job jsonb;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY['task_attempts','workflow_executions','workflow_root_budget_usage','workflow_budget_receipts','workflow_run_budgets','workflow_root_budgets','workflow_action_evidence_consumptions','workflow_action_remote_entries','workflow_action_dispatches','workflow_control_commands','fixture_evidence_ledger','fixture_provider_operations','fixture_provider_effects'] LOOP
        EXECUTE format('SELECT COALESCE(jsonb_agg(to_jsonb(row_image)),''[]''::jsonb) FROM public.%I AS row_image',relation_name) INTO current_rows;
        IF NOT (config.baseline->relation_name @> current_rows AND current_rows @> (config.baseline->relation_name))
            OR jsonb_array_length(current_rows)<>jsonb_array_length(config.baseline->relation_name) THEN RAISE EXCEPTION 'state preserved source history changed: %',relation_name; END IF;
    END LOOP;
    FOREACH relation_name IN ARRAY ARRAY['workflow_action_evidence','workflow_action_evidence_coverage','workflow_action_evidence_commands','workflow_run_events','workflow_action_claim_episodes','workflow_action_state_witnesses','workflow_action_schedule_witnesses'] LOOP
        FOR saved IN SELECT value FROM jsonb_array_elements(config.baseline->relation_name) LOOP
            EXECUTE format('SELECT COALESCE(jsonb_agg(to_jsonb(row_image)),''[]''::jsonb) FROM public.%I AS row_image',relation_name) INTO current_rows;
            IF NOT (current_rows @> jsonb_build_array(saved)) THEN RAISE EXCEPTION 'state immutable retained row changed: %',relation_name; END IF;
        END LOOP;
    END LOOP;
    old_job:=(SELECT value FROM jsonb_array_elements(config.baseline->'background_tasks') WHERE value->>'id'=config.job_id::text);
    IF old_job->>'status'<>'failed' OR (old_job-ARRAY['status','run_at','updated_at']) IS DISTINCT FROM ((pending->'job')-ARRAY['status','run_at','updated_at'])
        THEN RAISE EXCEPTION 'state same existing job provenance changed'; END IF;
    IF pending->'effects'<>'[]'::jsonb OR pending->'receipts'<>'[]'::jsonb OR pending->'conflicts'<>'[]'::jsonb
        OR pending->'operation'->>'marker_closed'<>'true' OR jsonb_array_length(pending->'entries')<>1 OR jsonb_array_length(pending->'coverage')<>1
        OR EXISTS(SELECT 1 FROM jsonb_array_elements(pending->'ledger') AS entry WHERE entry->>'final_closed'<>'true' OR entry->'result'<>'null'::jsonb)
        THEN RAISE EXCEPTION 'state genuine finality/provider facts invalid'; END IF;
    IF EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry WHERE entry.company_id=config.company_id AND entry.invocation_id=(config.expected->'command'->>'invocation_id')::uuid
        AND (entry.run_id<>config.run_id OR entry.execution_id<>config.execution_id OR entry.argument_digest<>config.expected->'command'->>'argument_digest'
            OR entry.dispatch_id<>(config.expected->'command'->>'dispatch_id')::uuid OR entry.created_at>(pending->'evidence'->>'observed_at')::timestamptz
            OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=entry.company_id AND coverage.evidence_id=(pending->'evidence'->>'id')::uuid
                AND coverage.remote_entry_id=entry.id AND coverage.run_id=entry.run_id AND coverage.execution_id=entry.execution_id AND coverage.invocation_id=entry.invocation_id AND coverage.argument_digest=entry.argument_digest AND coverage.dispatch_id=entry.dispatch_id)))
        THEN RAISE EXCEPTION 'state genuine exact evidence coverage invalid'; END IF;
END $$;
CREATE FUNCTION fixture_state_predicates(config fixture_state_config,pending jsonb) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE predicates jsonb; candidates jsonb; state jsonb; original_run jsonb;
BEGIN
    SELECT jsonb_build_object('owner_scope',owner.company_id=config.company_id AND owner.id=config.run_id,
        'execution_scope',execution.company_id=owner.company_id AND execution.run_id=owner.id AND execution.id=config.execution_id,
        'job_scope',task.company_id=execution.company_id AND task.workflow_execution_id=execution.id AND task.id=config.job_id,
        'admission_scope',admission.company_id=owner.company_id AND admission.run_id=owner.id,
        'workflow_queue',task.queue_kind='workflow','future_deadline',owner.deadline>clock_timestamp(),'activation_bound',execution.activation<=owner.max_steps,
        'activated',execution.activated_at IS NOT NULL,'uncompleted',execution.completed_at IS NULL,'no_output',execution.committed_output IS NULL,
        'no_route',execution.committed_route IS NULL,'no_successor',execution.successor_execution_id IS NULL,'positive_attempt',task.retry_count>0,
        'remaining_attempts',task.retry_count<task.max_retries,'no_worker',task.worker_id IS NULL,'no_generation',task.execution_generation IS NULL,'no_lease',task.lock_expires_at IS NULL,
        'attempt_scope',attempt.task_id=task.id AND attempt.attempt_number=task.retry_count,'attempt_failed',attempt.status='failed','attempt_finished',attempt.finished_at IS NOT NULL,
        'suitable_retirement',attempt.workflow_retirement IN ('live','expired'),'not_activation_poison',attempt.workflow_failure_code<>'workflow.activation_limit',
        'not_result_poison',attempt.workflow_failure_code<>'workflow.invalid_result','not_deadline_poison',attempt.workflow_failure_code<>'workflow.run_deadline',
        'not_budget_poison',attempt.workflow_failure_code<>'workflow.root_budget_exhausted','action_safe',workflow_action_retry_safe(config.company_id,execution.id) IS TRUE,
        'budget_link',link.company_id=owner.company_id AND link.run_id=owner.id AND link.root_run_id=budget.root_run_id,
        'budget_usage_scope',usage.company_id=budget.company_id AND usage.root_run_id=budget.root_run_id,'frozen_v2',budget.provenance='frozen_v2',
        'budget_activations',usage.activations<=budget.activations,'budget_model',usage.model_calls<budget.model_calls,'budget_repetitions',usage.repetitions<=budget.repetitions,
        'no_exhausted_receipt',NOT EXISTS(SELECT 1 FROM workflow_budget_receipts WHERE company_id=config.company_id AND run_id=config.run_id AND disposition='exhausted'),
        'no_refusal',NOT EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals WHERE company_id=config.company_id AND run_id=config.run_id AND execution_id=config.execution_id AND job_id=config.job_id),
        'non_child',(substring(admission.source_key FROM 4)::jsonb->>0)<>'child',
        'no_runnable_sibling',NOT EXISTS(SELECT 1 FROM workflow_executions AS sibling JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id WHERE sibling.company_id=config.company_id AND sibling.run_id=config.run_id AND other.queue_kind='workflow' AND other.id<>config.job_id AND other.status IN ('pending','processing')),
        'latest_attempt',NOT EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=task.id AND later.attempt_number>task.retry_count),
        'running',owner.state='running','no_terminal_pointer',owner.terminal_execution_id IS NULL,
        'pending',task.status='pending','reopen_safe',workflow_action_reconciliation_reopen_safe(config.company_id,config.run_id,config.job_id)) INTO predicates
        FROM workflow_runs AS owner JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
        JOIN workflow_run_budgets AS link ON link.company_id=owner.company_id AND link.run_id=owner.id
        JOIN workflow_root_budgets AS budget ON budget.company_id=link.company_id AND budget.root_run_id=link.root_run_id
        JOIN workflow_root_budget_usage AS usage ON usage.company_id=budget.company_id AND usage.root_run_id=budget.root_run_id
        WHERE owner.company_id=config.company_id AND owner.id=config.run_id AND task.id=config.job_id;
    IF predicates IS NULL OR EXISTS(SELECT 1 FROM jsonb_each(predicates) WHERE value IS DISTINCT FROM 'true'::jsonb) THEN RAISE EXCEPTION 'state nonattacked reopen predicates invalid: %',predicates; END IF;
    IF jsonb_array_length(pending->'states')<>1 OR jsonb_array_length(pending->'schedules')<>1 OR pending->'episode'='null'::jsonb THEN RAISE EXCEPTION 'state native owner witnesses missing'; END IF;
    state:=pending->'states'->0;
    original_run:=(SELECT value FROM jsonb_array_elements(config.baseline->'workflow_runs') WHERE value->>'id'=config.run_id::text);
    IF state->'initial_state'<>original_run->'state' OR state->'initial_waiting_reason'<>original_run->'waiting_reason'
        OR state->>'transaction_id'<>pending->>'xid' OR state->>'company_id'<>config.company_id::text OR state->>'run_id'<>config.run_id::text
        OR pending->'schedules'->0->>'transaction_id'<>pending->>'xid' OR pending->'schedules'->0->>'company_id'<>config.company_id::text
        OR pending->'schedules'->0->>'run_id'<>config.run_id::text OR pending->'schedules'->0->>'execution_id'<>config.execution_id::text
        OR pending->'schedules'->0->>'job_id'<>config.job_id::text OR pending->'schedules'->0->'retired_attempt'<>pending->'job'->'retry_count'
        OR NOT (pending->'episode' @> jsonb_build_object('company_id',config.company_id,'run_id',config.run_id,'execution_id',config.execution_id,'job_id',config.job_id,'command_key',config.command_key,'retired_attempt',pending->'job'->'retry_count'))
        THEN RAISE EXCEPTION 'state authentic first OLD/schedule/episode scope invalid'; END IF;
    SELECT COALESCE(jsonb_agg(jsonb_build_object('command_key',command.command_key,'revision_equal',command.result_revision=owner.revision,
        'scope',evidence.run_id=command.run_id AND evidence.execution_id=command.execution_id AND evidence.invocation_id=command.invocation_id AND evidence.argument_digest=command.argument_digest AND evidence.dispatch_id=command.dispatch_id,
        'identity',evidence.command_id=command.id AND evidence.command_key=command.command_key AND evidence.request_digest=command.request_digest AND evidence.actor_id=command.actor_id,
        'audit',audit.execution_id=command.execution_id AND audit.actor_id=command.actor_id AND audit.event_kind='action_reconciled',
        'labels',command.previous_state='waiting' AND command.previous_waiting_reason='reconciliation') ORDER BY command.command_key),'[]'::jsonb) INTO candidates
        FROM workflow_runs AS owner JOIN workflow_action_evidence_commands AS command ON command.company_id=owner.company_id AND command.run_id=owner.id
        JOIN workflow_action_evidence AS evidence ON evidence.company_id=command.company_id AND evidence.id=command.evidence_id
        JOIN workflow_run_events AS audit ON audit.company_id=command.company_id AND audit.run_id=command.run_id AND audit.sequence=command.audit_sequence
        WHERE owner.company_id=config.company_id AND owner.id=config.run_id AND command.execution_id=config.execution_id AND command.scheduled_job_id=config.job_id AND command.outcome->>'kind'='scheduled';
    IF jsonb_array_length(candidates)<>(CASE WHEN config.source_case='FIRST_OLD_FAILED' THEN 2 ELSE 1 END)
        OR EXISTS(SELECT 1 FROM jsonb_array_elements(candidates) AS candidate WHERE candidate->>'scope'<>'true' OR candidate->>'identity'<>'true' OR candidate->>'audit'<>'true' OR candidate->>'labels'<>'true'
            OR (candidate->>'revision_equal')::boolean<>(candidate->>'command_key'=config.command_key)) THEN RAISE EXCEPTION 'state candidate isolation invalid: %',candidates; END IF;
    RETURN jsonb_build_object('reopen',predicates,'candidates',candidates,'witness_eligible',state->>'initial_state'='waiting' AND state->>'initial_waiting_reason'='reconciliation',
        'ordinary_helper',workflow_control_retry_safe(config.company_id,config.run_id,config.job_id),
        'ordinary_receipt',EXISTS(SELECT 1 FROM workflow_control_commands WHERE company_id=config.company_id AND run_id=config.run_id AND operation='retry' AND result='applied' AND result_revision=(pending->'run'->>'revision')::bigint));
END $$;
