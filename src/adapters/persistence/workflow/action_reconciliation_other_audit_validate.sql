CREATE FUNCTION fixture_other_audit_validate(config fixture_other_audit_config,candidate workflow_action_evidence_commands,pending jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE history_now jsonb; old_job jsonb; relation_name text; current_rows jsonb; evidence workflow_action_evidence%ROWTYPE;
BEGIN
    SELECT jsonb_build_object('command',to_jsonb(command),'evidence',to_jsonb(historical_evidence),'audit',to_jsonb(audit)) INTO history_now
        FROM workflow_action_evidence_commands AS command JOIN workflow_action_evidence AS historical_evidence ON historical_evidence.company_id=command.company_id AND historical_evidence.id=command.evidence_id
        JOIN workflow_run_events AS audit ON audit.company_id=command.company_id AND audit.run_id=command.run_id AND audit.sequence=command.audit_sequence
        WHERE command.company_id=candidate.company_id AND command.command_key=config.history->'command'->>'command_key';
    IF (history_now<>config.history OR history_now->'command'->>'command_key'=candidate.command_key
        OR history_now->'command'->>'id'=candidate.id::text OR history_now->'evidence'->>'disposition'<>'unknown'
        OR NOT (history_now->'audit' @> jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,
            'execution_id',candidate.execution_id,'actor_id',candidate.actor_id,'event_kind','action_reconciled'))
        OR history_now->'audit'->'sequence'=pending->'audit'->'sequence') IS DISTINCT FROM false
        THEN RAISE EXCEPTION 'OtherAudit exact authentic C1/A1/E1 changed'; END IF;
    evidence:=(SELECT saved FROM workflow_action_evidence AS saved WHERE saved.company_id=candidate.company_id AND saved.id=candidate.evidence_id);
    IF (NOT (pending->'command' @> (config.expected->'command')) OR NOT (pending->'evidence' @> (config.expected->'evidence'))
        OR evidence.id IS NULL OR evidence.command_id<>candidate.id OR evidence.command_key<>candidate.command_key OR evidence.request_digest<>candidate.request_digest
        OR pending->'command'->'result_revision'<>pending->'run'->'revision'
        OR pending->'command'->'outcome'<>jsonb_build_object('kind','scheduled','receipt_only',false)
        OR candidate.previous_state<>'waiting' OR candidate.previous_waiting_reason<>'reconciliation'
        OR NOT (pending->'audit' @> jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,'execution_id',candidate.execution_id,
            'actor_id',candidate.actor_id,'sequence',candidate.audit_sequence,'event_kind','action_reconciled'))
        OR EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS owner WHERE owner.company_id=candidate.company_id AND owner.run_id=candidate.run_id
            AND owner.audit_sequence=candidate.audit_sequence AND owner.command_key<>candidate.command_key)
        OR pending->'job'->>'status'<>'pending' OR pending->'job'->'worker_id'<>'null'::jsonb
        OR pending->'job'->'execution_generation'<>'null'::jsonb OR pending->'job'->'lock_expires_at'<>'null'::jsonb
        OR pending->'run'->>'state'<>'running' OR pending->'run'->'terminal_execution_id'<>'null'::jsonb
        OR jsonb_array_length(pending->'entries')<>1 OR jsonb_array_length(pending->'coverage')<>1
        OR evidence.valid_until<=clock_timestamp() OR evidence.observed_at>clock_timestamp()
        OR evidence.verified_at>clock_timestamp() OR evidence.observed_at<(pending->'marker'->>'created_at')::timestamptz
        OR evidence.valid_until<=evidence.verified_at OR evidence.valid_until>evidence.verified_at+interval '24 hours'
        OR workflow_action_not_applied_available(candidate.company_id,candidate.invocation_id) IS DISTINCT FROM candidate.evidence_id)
        IS DISTINCT FROM false THEN RAISE EXCEPTION 'OtherAudit genuine Final owner NEW/shape/time invalid'; END IF;
    IF NOT EXISTS(SELECT 1 FROM fixture_provider_operations AS operation WHERE operation.invocation_id=candidate.invocation_id AND operation.marker_closed)
        OR EXISTS(SELECT 1 FROM fixture_provider_effects)
        OR EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt WHERE receipt.company_id=candidate.company_id AND receipt.invocation_id=candidate.invocation_id)
        OR EXISTS(SELECT 1 FROM workflow_action_evidence_conflicts AS conflict WHERE conflict.company_id=candidate.company_id AND conflict.invocation_id=candidate.invocation_id)
        OR EXISTS(SELECT 1 FROM workflow_action_evidence AS applied WHERE applied.company_id=candidate.company_id AND applied.invocation_id=candidate.invocation_id AND applied.disposition='applied')
        OR EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry WHERE entry.company_id=candidate.company_id AND entry.invocation_id=candidate.invocation_id
            AND (entry.run_id<>candidate.run_id OR entry.execution_id<>candidate.execution_id OR entry.argument_digest<>candidate.argument_digest OR entry.dispatch_id<>candidate.dispatch_id
                OR entry.created_at>evidence.observed_at
                OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS covered WHERE covered.company_id=entry.company_id AND covered.evidence_id=candidate.evidence_id
                    AND covered.remote_entry_id=entry.id AND covered.run_id=entry.run_id AND covered.execution_id=entry.execution_id
                    AND covered.invocation_id=entry.invocation_id AND covered.argument_digest=entry.argument_digest AND covered.dispatch_id=entry.dispatch_id)
                OR NOT EXISTS(SELECT 1 FROM fixture_evidence_ledger AS ledger WHERE ledger.company_id=entry.company_id AND ledger.invocation_id=entry.invocation_id
                    AND ledger.entry_id=entry.id AND ledger.final_closed AND ledger.result IS NULL)))
        THEN RAISE EXCEPTION 'OtherAudit genuine exact coverage/quiescence/veto invalid'; END IF;
    old_job:=(SELECT job FROM jsonb_array_elements(config.baseline->'background_tasks') AS job WHERE job->>'id'=candidate.scheduled_job_id::text);
    IF (old_job->>'status'<>'failed' OR ((pending->'job')-ARRAY['status','run_at','updated_at'])<>(old_job-ARRAY['status','run_at','updated_at'])
        OR NOT workflow_action_reconciliation_reopen_safe(candidate.company_id,candidate.run_id,candidate.scheduled_job_id)
        OR workflow_control_retry_safe(candidate.company_id,candidate.run_id,candidate.scheduled_job_id)) IS DISTINCT FROM false
        OR EXISTS(SELECT 1 FROM workflow_control_commands AS ordinary WHERE ordinary.company_id=candidate.company_id AND ordinary.run_id=candidate.run_id
            AND ordinary.operation='retry' AND ordinary.result='applied' AND ordinary.result_revision=candidate.result_revision)
        THEN RAISE EXCEPTION 'OtherAudit exact pending job/nonordinary predicates invalid'; END IF;
    IF NOT EXISTS(SELECT 1 FROM workflow_action_state_witnesses AS state JOIN workflow_action_schedule_witnesses AS schedule
        ON schedule.company_id=state.company_id AND schedule.run_id=state.run_id AND schedule.transaction_id=state.transaction_id
        WHERE state.company_id=candidate.company_id AND state.run_id=candidate.run_id AND state.transaction_id=pg_current_xact_id()
            AND state.initial_state='waiting' AND state.initial_waiting_reason='reconciliation' AND schedule.execution_id=candidate.execution_id
            AND schedule.job_id=candidate.scheduled_job_id AND schedule.retired_attempt=(pending->'job'->>'retry_count')::integer)
        THEN RAISE EXCEPTION 'OtherAudit actual current-xid first OLD/schedule missing'; END IF;
    FOREACH relation_name IN ARRAY ARRAY['task_attempts','workflow_executions','workflow_root_budget_usage','workflow_budget_receipts',
        'workflow_action_evidence_consumptions','workflow_action_remote_entries','workflow_action_dispatches','fixture_evidence_ledger','fixture_provider_operations','fixture_provider_effects'] LOOP
        EXECUTE format('SELECT COALESCE(jsonb_agg(to_jsonb(saved)),''[]''::jsonb) FROM public.%I AS saved',relation_name) INTO current_rows;
        IF EXISTS(SELECT 1 FROM jsonb_array_elements(current_rows) AS row_image WHERE NOT (config.baseline->relation_name @> jsonb_build_array(row_image)))
            OR jsonb_array_length(current_rows)<>jsonb_array_length(config.baseline->relation_name)
            THEN RAISE EXCEPTION 'OtherAudit preserved history changed: %',relation_name; END IF;
    END LOOP;
END $$;
