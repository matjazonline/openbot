CREATE FUNCTION fixture_reopen_candidates(config fixture_reopen_config,candidate workflow_action_evidence_commands,pending jsonb) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE history_now jsonb; candidate_filters jsonb;
BEGIN
    IF config.history<>'null'::jsonb THEN
        SELECT jsonb_build_object('command',to_jsonb(command),'evidence',to_jsonb(evidence),'audit',to_jsonb(audit),'episode',to_jsonb(episode)) INTO history_now
            FROM workflow_action_evidence_commands AS command JOIN workflow_action_evidence AS evidence ON evidence.company_id=command.company_id AND evidence.id=command.evidence_id
            JOIN workflow_run_events AS audit ON audit.company_id=command.company_id AND audit.run_id=command.run_id AND audit.sequence=command.audit_sequence
            JOIN workflow_action_claim_episodes AS episode ON episode.company_id=command.company_id AND episode.command_key=command.command_key
            WHERE command.company_id=config.company_id AND command.command_key=config.history->'command'->>'command_key';
        IF history_now IS DISTINCT FROM config.history OR (history_now->'command'->>'result_revision')::bigint>=(pending->'run'->>'revision')::bigint
            THEN RAISE EXCEPTION 'Final C1 authentic history/revision changed'; END IF;
    END IF;
    SELECT jsonb_agg(jsonb_build_object('command_key',receipt.command_key,'revision_equal',receipt.result_revision=owner.revision,
        'actor_equal',audit.actor_id=receipt.actor_id,'execution_equal',audit.execution_id=receipt.execution_id,
        'other_filters',receipt.previous_state='waiting' AND receipt.previous_waiting_reason='reconciliation'
            AND evidence.run_id=receipt.run_id AND evidence.execution_id=receipt.execution_id AND evidence.invocation_id=receipt.invocation_id
            AND evidence.argument_digest=receipt.argument_digest AND evidence.dispatch_id=receipt.dispatch_id AND audit.event_kind='action_reconciled'
            AND EXISTS(SELECT 1 FROM workflow_action_state_witnesses AS state WHERE state.company_id=owner.company_id AND state.run_id=owner.id
                AND state.transaction_id=pg_current_xact_id() AND state.initial_state='waiting' AND state.initial_waiting_reason='reconciliation')) ORDER BY receipt.command_key)
        INTO candidate_filters FROM workflow_runs AS owner JOIN workflow_action_evidence_commands AS receipt ON receipt.company_id=owner.company_id AND receipt.run_id=owner.id
        JOIN workflow_action_evidence AS evidence ON evidence.company_id=receipt.company_id AND evidence.id=receipt.evidence_id
        JOIN workflow_run_events AS audit ON audit.company_id=receipt.company_id AND audit.run_id=receipt.run_id AND audit.sequence=receipt.audit_sequence
        WHERE owner.company_id=candidate.company_id AND owner.id=candidate.run_id AND receipt.execution_id=candidate.execution_id
            AND receipt.scheduled_job_id=candidate.scheduled_job_id AND receipt.outcome->>'kind'='scheduled';
    IF jsonb_array_length(candidate_filters)<>(CASE WHEN config.history='null'::jsonb THEN 1 ELSE 2 END)
        OR EXISTS(SELECT 1 FROM jsonb_array_elements(candidate_filters) AS filter WHERE filter->>'other_filters'<>'true'
            OR (config.probe_case='historical-revision' AND (filter->>'actor_equal'<>'true' OR filter->>'execution_equal'<>'true' OR filter->>'revision_equal'<>'false')))
        THEN RAISE EXCEPTION 'Final all-candidate nonattacked predicates invalid'; END IF;
    RETURN candidate_filters;
END $$;
CREATE FUNCTION fixture_reopen_validate(config fixture_reopen_config,candidate workflow_action_evidence_commands,canonical jsonb) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE pending jsonb:=canonical; old_job jsonb; relation_name text; current_rows jsonb;
BEGIN
    IF (NOT (pending->'command' @> (config.expected->'command')) OR NOT (pending->'evidence' @> (config.expected->'evidence'))
        OR pending->'command'->'evidence_id'<>pending->'evidence'->'id' OR pending->'command'->'id'<>pending->'evidence'->'command_id'
        OR pending->'command'->'result_revision'<>pending->'run'->'revision'
        OR pending->'command'->'outcome'<>jsonb_build_object('kind','scheduled','receipt_only',false)
        OR pending->'command'->>'previous_state'<>'waiting' OR pending->'command'->>'previous_waiting_reason'<>'reconciliation'
        OR NOT (pending->'audit' @> jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,'execution_id',candidate.execution_id,
            'actor_id',candidate.actor_id,'sequence',candidate.audit_sequence,'event_kind','action_reconciled'))
        OR pending->'job'->>'status'<>'pending' OR pending->'job'->'worker_id'<>'null'::jsonb
        OR pending->'job'->'execution_generation'<>'null'::jsonb OR pending->'job'->'lock_expires_at'<>'null'::jsonb
        OR pending->'run'->>'state'<>'running' OR pending->'run'->'terminal_execution_id'<>'null'::jsonb
        OR pending->'receipts'<>'[]'::jsonb OR pending->'conflicts'<>'[]'::jsonb OR pending->'effects'<>'[]'::jsonb
        OR pending->'operation'->>'marker_closed'<>'true'
        OR jsonb_array_length(pending->'coverage')<>jsonb_array_length(pending->'entries')
        OR jsonb_array_length(pending->'entries')<>(CASE WHEN config.history='null'::jsonb THEN 1 ELSE 2 END)
        OR (pending->'evidence'->>'valid_until')::timestamptz<=clock_timestamp()) IS DISTINCT FROM false
        THEN RAISE EXCEPTION 'Final canonical owner shape/time invalid'; END IF;
    IF NOT EXISTS(SELECT 1 FROM users AS actor WHERE actor.id=config.other_actor AND actor.id<>candidate.actor_id)
        OR NOT EXISTS(SELECT 1 FROM workflow_executions AS first JOIN background_tasks AS job ON job.company_id=first.company_id AND job.workflow_execution_id=first.id
            WHERE first.company_id=candidate.company_id AND first.run_id=candidate.run_id AND first.id=config.first_execution AND first.completed_at IS NOT NULL
                AND first.successor_execution_id=candidate.execution_id AND job.id=config.first_job AND job.status='completed')
        THEN RAISE EXCEPTION 'Final authentic replacement identities absent'; END IF;
    IF EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry WHERE entry.company_id=candidate.company_id AND entry.invocation_id=candidate.invocation_id
        AND (entry.run_id<>candidate.run_id OR entry.execution_id<>candidate.execution_id OR entry.argument_digest<>candidate.argument_digest OR entry.dispatch_id<>candidate.dispatch_id
            OR entry.created_at>(pending->'evidence'->>'observed_at')::timestamptz
            OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS covered WHERE covered.company_id=entry.company_id AND covered.evidence_id=candidate.evidence_id
                AND covered.remote_entry_id=entry.id AND covered.run_id=entry.run_id AND covered.execution_id=entry.execution_id
                AND covered.invocation_id=entry.invocation_id AND covered.argument_digest=entry.argument_digest AND covered.dispatch_id=entry.dispatch_id)
            OR NOT EXISTS(SELECT 1 FROM fixture_evidence_ledger AS ledger WHERE ledger.company_id=entry.company_id AND ledger.invocation_id=entry.invocation_id
                AND ledger.entry_id=entry.id AND ledger.final_closed AND ledger.result IS NULL)))
        THEN RAISE EXCEPTION 'Final exact genuine entries/quiescence invalid'; END IF;
    old_job:=(SELECT job FROM jsonb_array_elements(config.baseline->'background_tasks') AS job WHERE job->>'id'=candidate.scheduled_job_id::text);
    IF old_job->>'status'<>'failed' OR ((pending->'job')-ARRAY['status','run_at','updated_at'])<>(old_job-ARRAY['status','run_at','updated_at'])
        OR NOT workflow_action_reconciliation_reopen_safe(candidate.company_id,candidate.run_id,candidate.scheduled_job_id)
        OR workflow_control_retry_safe(candidate.company_id,candidate.run_id,candidate.scheduled_job_id)
        OR EXISTS(SELECT 1 FROM workflow_control_commands AS ordinary WHERE ordinary.company_id=candidate.company_id AND ordinary.run_id=candidate.run_id
            AND ordinary.operation='retry' AND ordinary.result='applied' AND ordinary.result_revision=(pending->'run'->>'revision')::bigint)
        THEN RAISE EXCEPTION 'Final same-job safe/nonordinary predicates invalid'; END IF;
    FOREACH relation_name IN ARRAY ARRAY['task_attempts','workflow_executions','workflow_root_budget_usage','workflow_budget_receipts','workflow_action_evidence_consumptions','workflow_action_remote_entries','workflow_action_dispatches','fixture_evidence_ledger','fixture_provider_operations','fixture_provider_effects'] LOOP
        EXECUTE format('SELECT COALESCE(jsonb_agg(to_jsonb(saved)),''[]''::jsonb) FROM public.%I AS saved',relation_name) INTO current_rows;
        IF EXISTS(SELECT 1 FROM jsonb_array_elements(current_rows) AS row_image WHERE NOT (config.baseline->relation_name @> jsonb_build_array(row_image)))
            OR jsonb_array_length(current_rows)<>jsonb_array_length(config.baseline->relation_name)
            THEN RAISE EXCEPTION 'Final preserved history changed: %',relation_name; END IF;
    END LOOP;
    IF NOT EXISTS(SELECT 1 FROM workflow_action_state_witnesses AS state JOIN workflow_action_schedule_witnesses AS schedule
        ON schedule.company_id=state.company_id AND schedule.run_id=state.run_id AND schedule.transaction_id=state.transaction_id
        JOIN workflow_action_claim_episodes AS episode ON episode.company_id=schedule.company_id AND episode.run_id=schedule.run_id
            AND episode.execution_id=schedule.execution_id AND episode.job_id=schedule.job_id AND episode.retired_attempt=schedule.retired_attempt
        WHERE state.company_id=candidate.company_id AND state.run_id=candidate.run_id AND state.transaction_id=pg_current_xact_id()
            AND state.initial_state='waiting' AND state.initial_waiting_reason='reconciliation'
            AND schedule.execution_id=candidate.execution_id AND schedule.job_id=candidate.scheduled_job_id
            AND schedule.retired_attempt=(pending->'job'->>'retry_count')::integer AND episode.command_key=candidate.command_key)
        THEN RAISE EXCEPTION 'Final native current-xid first-OLD/schedule/episode missing'; END IF;
    RETURN fixture_reopen_candidates(config,candidate,pending);
END $$;
CREATE FUNCTION fixture_reopen_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_reopen_config%ROWTYPE; candidate workflow_action_evidence_commands%ROWTYPE; row_count integer;
    raw_pending jsonb; canonical jsonb; predicates jsonb; envelope jsonb; native_state text; native_constraint text; native_message text;
    native_context text; native_schema text; native_table text; prefix text; expected_message text;
BEGIN
    SELECT count(*) INTO row_count FROM inserted_reopen_commands AS inserted JOIN fixture_reopen_config AS configured ON configured.company_id=inserted.company_id AND configured.command_key=inserted.command_key;
    IF row_count=0 THEN RETURN NULL; END IF;
    IF row_count<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'Final ambiguous owner statement'; END IF;
    candidate:=(SELECT inserted FROM inserted_reopen_commands AS inserted JOIN fixture_reopen_config AS configured ON configured.company_id=inserted.company_id AND configured.command_key=inserted.command_key);
    config:=(SELECT configured FROM fixture_reopen_config AS configured WHERE configured.company_id=candidate.company_id AND configured.command_key=candidate.command_key);
    IF NOT EXISTS(SELECT 1 FROM pg_constraint AS native JOIN pg_trigger AS trigger_row ON trigger_row.tgconstraint=native.oid JOIN pg_proc AS function_row ON function_row.oid=trigger_row.tgfoid
        WHERE native.oid=config.target_oid AND native.conname=config.target_name AND native.connamespace='public'::regnamespace AND native.condeferrable AND native.condeferred
            AND native.conrelid='background_tasks'::regclass AND trigger_row.tgenabled='O' AND trigger_row.tgtype=17 AND function_row.proname='check_workflow_control_retry')
        THEN RAISE EXCEPTION 'Final lost exact native target'; END IF;
    raw_pending:=fixture_reopen_pending(candidate.company_id,candidate.command_key);canonical:=fixture_reopen_images(config,raw_pending);
    predicates:=fixture_reopen_validate(config,candidate,canonical);
    BEGIN
        EXECUTE format('SET CONSTRAINTS public.%I IMMEDIATE',config.target_name);
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_constraint=CONSTRAINT_NAME,native_message=MESSAGE_TEXT,
            native_context=PG_EXCEPTION_CONTEXT,native_schema=SCHEMA_NAME,native_table=TABLE_NAME;
    END;
    IF config.probe_case='positive' THEN
        IF native_state IS NOT NULL THEN RAISE EXCEPTION 'Final positive native failure: % %',native_state,native_message; END IF;
        SET CONSTRAINTS ALL IMMEDIATE;
        INSERT INTO fixture_reopen_witness(company_id,command_key,selected_flush,all_immediate,owner_pid,pending) VALUES(candidate.company_id,candidate.command_key,true,true,pg_backend_pid(),raw_pending);
        RETURN NULL;
    END IF;
    envelope:=jsonb_build_object('version',1,'probe_case',config.probe_case,'company_id',candidate.company_id,'command_key',candidate.command_key,
        'target_oid',config.target_oid,'target_name',config.target_name,'before_hits',3,'after_hits',1,'depth',pg_trigger_depth(),'owner_pid',pg_backend_pid(),'database',current_database(),
        'first_execution',config.first_execution,'first_job',config.first_job,'other_actor',config.other_actor,'original',canonical,'returned',raw_pending,'predicates',predicates,
        'images_checked',true,'source_checked',true,'selected_executed',true,'native_state',native_state,'native_constraint',native_constraint,'native_message',native_message,
        'native_context',native_context,'native_schema',native_schema,'native_table',native_table);
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'Final diagnostic overflow'; END IF;
    expected_message:=CASE WHEN config.probe_case='mismatch-control' THEN 'deliberately wrong reopen diagnostic' ELSE 'invalid explicit workflow retry' END;
    prefix:=CASE WHEN native_state IS NULL THEN 'FIXTURE_REOPEN_ACCEPTED_V1:'
        WHEN COALESCE(native_state='23514' AND native_constraint='' AND native_schema='' AND native_table='' AND native_message=expected_message
            AND position('check_workflow_control_retry()' IN native_context)>0 AND position('SET CONSTRAINTS public.workflow_control_retry_guard IMMEDIATE' IN native_context)>0,false)
        THEN 'FIXTURE_REOPEN_NATIVE_V1:' ELSE 'FIXTURE_REOPEN_MISMATCH_V1:' END;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_reopen_command_after AFTER INSERT ON workflow_action_evidence_commands REFERENCING NEW TABLE AS inserted_reopen_commands FOR EACH STATEMENT EXECUTE FUNCTION fixture_reopen_after();
