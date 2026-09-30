CREATE TABLE fixture_state_config (
    company_id uuid NOT NULL,run_id uuid NOT NULL,execution_id uuid NOT NULL,job_id uuid NOT NULL,command_key text NOT NULL,
    source_case text NOT NULL,mode text NOT NULL,target_oid bigint NOT NULL,target_name text NOT NULL,expected jsonb NOT NULL,history jsonb NOT NULL,baseline jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_state_capture (
    company_id uuid NOT NULL,command_key text NOT NULL,relation_name text NOT NULL,original_new jsonb NOT NULL,returned_new jsonb NOT NULL,depth integer NOT NULL,
    PRIMARY KEY(company_id,command_key,relation_name)
);
CREATE TABLE fixture_state_steps (
    company_id uuid NOT NULL,command_key text NOT NULL,step text NOT NULL,image jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key,step)
);
CREATE TABLE fixture_state_witness (
    company_id uuid NOT NULL,command_key text NOT NULL,selected_flush boolean NOT NULL,all_immediate boolean NOT NULL,owner_pid integer NOT NULL,pending jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE FUNCTION fixture_state_pending_base(company uuid,key text) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE;
BEGIN
    candidate := (SELECT command FROM workflow_action_evidence_commands AS command WHERE command.company_id=company AND command.command_key=key);
    IF candidate.id IS NULL THEN RAISE EXCEPTION 'Final missing actual command'; END IF;
    RETURN jsonb_build_object('command',to_jsonb(candidate),
        'evidence',(SELECT to_jsonb(evidence) FROM workflow_action_evidence AS evidence WHERE evidence.company_id=company AND evidence.id=candidate.evidence_id),
        'audit',(SELECT to_jsonb(audit) FROM workflow_run_events AS audit WHERE audit.company_id=company AND audit.run_id=candidate.run_id AND audit.sequence=candidate.audit_sequence),
        'run',(SELECT to_jsonb(owner) FROM workflow_runs AS owner WHERE owner.company_id=company AND owner.id=candidate.run_id),
        'job',(SELECT to_jsonb(job) FROM background_tasks AS job WHERE job.company_id=company AND job.id=candidate.scheduled_job_id),
        'marker',(SELECT to_jsonb(marker) FROM workflow_action_dispatches AS marker WHERE marker.company_id=company AND marker.id=candidate.dispatch_id),
        'episode',(SELECT to_jsonb(episode) FROM workflow_action_claim_episodes AS episode WHERE episode.company_id=company AND episode.command_key=key),
        'coverage',COALESCE((SELECT jsonb_agg(to_jsonb(coverage) ORDER BY coverage.remote_entry_id) FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=company AND coverage.evidence_id=candidate.evidence_id),'[]'::jsonb),
        'entries',COALESCE((SELECT jsonb_agg(to_jsonb(entry) ORDER BY entry.id) FROM workflow_action_remote_entries AS entry WHERE entry.company_id=company AND entry.invocation_id=candidate.invocation_id),'[]'::jsonb),
        'ledger',COALESCE((SELECT jsonb_agg(to_jsonb(ledger) ORDER BY ledger.entry_id) FROM fixture_evidence_ledger AS ledger WHERE ledger.company_id=company AND ledger.invocation_id=candidate.invocation_id),'[]'::jsonb),
        'operation',(SELECT to_jsonb(operation) FROM fixture_provider_operations AS operation WHERE operation.invocation_id=candidate.invocation_id),
        'effects',COALESCE((SELECT jsonb_agg(to_jsonb(effect) ORDER BY effect.entry_id) FROM fixture_provider_effects AS effect),'[]'::jsonb),
        'receipts',COALESCE((SELECT jsonb_agg(to_jsonb(receipt) ORDER BY receipt.invocation_id) FROM workflow_action_receipts AS receipt WHERE receipt.company_id=company AND receipt.invocation_id=candidate.invocation_id),'[]'::jsonb),
        'conflicts',COALESCE((SELECT jsonb_agg(to_jsonb(conflict) ORDER BY conflict.id) FROM workflow_action_evidence_conflicts AS conflict WHERE conflict.company_id=company AND conflict.invocation_id=candidate.invocation_id),'[]'::jsonb));
END $$;
CREATE FUNCTION fixture_state_step(config fixture_state_config,label text) RETURNS void LANGUAGE plpgsql AS $$
DECLARE owner_image jsonb; original_bundle jsonb;
BEGIN
    SELECT to_jsonb(owner) INTO STRICT owner_image FROM workflow_runs AS owner
        WHERE owner.company_id=config.company_id AND owner.id=config.run_id;
    SELECT saved->'bundle' INTO STRICT original_bundle FROM jsonb_array_elements(config.baseline->'workflow_runs') AS saved
        WHERE saved->>'company_id'=config.company_id::text AND saved->>'id'=config.run_id::text;
    IF owner_image->'bundle' IS DISTINCT FROM original_bundle THEN RAISE EXCEPTION 'state immutable bundle changed'; END IF;
    INSERT INTO fixture_state_steps(company_id,command_key,step,image)
        SELECT config.company_id,config.command_key,label,jsonb_build_object('xid',pg_current_xact_id()::text,
            -- Assert the full frozen bundle above; retain every mutable head field without repeating it four times in the bounded transport.
            'run',owner_image-'bundle',
            'job',(SELECT to_jsonb(job) FROM background_tasks AS job WHERE job.company_id=config.company_id AND job.id=config.job_id),
            'states',COALESCE((SELECT jsonb_agg(to_jsonb(state)) FROM workflow_action_state_witnesses AS state WHERE state.company_id=config.company_id AND state.run_id=config.run_id AND state.transaction_id=pg_current_xact_id()),'[]'::jsonb),
            'schedules',COALESCE((SELECT jsonb_agg(to_jsonb(schedule)) FROM workflow_action_schedule_witnesses AS schedule WHERE schedule.company_id=config.company_id AND schedule.job_id=config.job_id AND schedule.transaction_id=pg_current_xact_id()),'[]'::jsonb));
END $$;
CREATE FUNCTION fixture_state_attack(config fixture_state_config,candidate workflow_action_evidence_commands) RETURNS workflow_action_evidence_commands LANGUAGE plpgsql AS $$
DECLARE source jsonb; first_image jsonb; modified integer; final_revision bigint;
BEGIN
    PERFORM fixture_state_step(config,'before');
    source:=(SELECT image FROM fixture_state_steps WHERE company_id=config.company_id AND command_key=config.command_key AND step='before');
    IF source->'states'<>'[]'::jsonb OR source->'schedules'<>'[]'::jsonb THEN RAISE EXCEPTION 'state source has current witness before attack'; END IF;
    IF candidate.outcome<>jsonb_build_object('kind','not_applied_recorded') OR candidate.scheduled_job_id IS NOT NULL
        OR candidate.previous_state IS DISTINCT FROM source->'run'->>'state'
        OR candidate.previous_waiting_reason IS DISTINCT FROM source->'run'->>'waiting_reason'
        OR candidate.result_revision<>(source->'run'->>'revision')::bigint
        OR source->'job'->>'status'<>'failed' THEN RAISE EXCEPTION 'state genuine original service command invalid'; END IF;
    IF config.source_case='FIRST_OLD_HUMAN_REASON' THEN
        UPDATE workflow_runs SET waiting_reason='reconciliation' WHERE company_id=config.company_id AND id=config.run_id AND state='waiting' AND waiting_reason='decision';
    ELSE
        UPDATE workflow_runs SET state='waiting',waiting_reason='reconciliation',terminal_execution_id=NULL
            WHERE company_id=config.company_id AND id=config.run_id AND state IN ('failed','cancelled');
    END IF;
    GET DIAGNOSTICS modified=ROW_COUNT;
    IF modified<>1 THEN RAISE EXCEPTION 'state first public UPDATE cardinality'; END IF;
    PERFORM fixture_state_step(config,'first');
    first_image:=(SELECT image FROM fixture_state_steps WHERE company_id=config.company_id AND command_key=config.command_key AND step='first');
    IF jsonb_array_length(first_image->'states')<>1 OR first_image->'states'->0->'initial_state'<>source->'run'->'state'
        OR first_image->'states'->0->'initial_waiting_reason'<>source->'run'->'waiting_reason'
        OR first_image->'run'->>'state'<>'waiting' OR first_image->'run'->>'waiting_reason'<>'reconciliation'
        OR (config.source_case='FIRST_OLD_HUMAN_REASON' AND first_image->'states'->0->>'initial_waiting_reason'<>'decision')
        THEN RAISE EXCEPTION 'state first OLD source was not retained immediately'; END IF;
    UPDATE background_tasks SET status='pending',run_at=clock_timestamp(),updated_at=clock_timestamp()
        WHERE company_id=config.company_id AND id=config.job_id AND workflow_execution_id=config.execution_id AND status='failed';
    GET DIAGNOSTICS modified=ROW_COUNT;
    IF modified<>1 THEN RAISE EXCEPTION 'state failed-pending cardinality'; END IF;
    PERFORM fixture_state_step(config,'scheduled');
    UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=config.company_id AND id=config.run_id AND state='waiting' AND waiting_reason='reconciliation';
    GET DIAGNOSTICS modified=ROW_COUNT;
    IF modified<>1 THEN RAISE EXCEPTION 'state running cardinality'; END IF;
    PERFORM fixture_state_step(config,'final');
    SELECT revision INTO final_revision FROM workflow_runs WHERE company_id=config.company_id AND id=config.run_id;
    candidate.outcome:=jsonb_build_object('kind','scheduled','receipt_only',false);candidate.scheduled_job_id:=config.job_id;
    candidate.previous_state:='waiting';candidate.previous_waiting_reason:='reconciliation';candidate.result_revision:=final_revision;
    RETURN candidate;
END $$;
CREATE FUNCTION fixture_state_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_state_config%ROWTYPE; original jsonb; matches integer;
BEGIN
    IF TG_TABLE_NAME='workflow_run_events' THEN
        IF NEW.event_kind<>'action_reconciled' THEN RETURN NEW; END IF;
        SELECT count(*) INTO matches FROM fixture_state_config WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND execution_id=NEW.execution_id;
        IF matches=0 THEN RETURN NEW; END IF;
        IF matches<>1 THEN RAISE EXCEPTION 'state ambiguous audit owner'; END IF;
        config:=(SELECT configured FROM fixture_state_config AS configured WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND execution_id=NEW.execution_id);
    ELSE
        config:=(SELECT configured FROM fixture_state_config AS configured WHERE company_id=NEW.company_id AND command_key=NEW.command_key);
        IF config.company_id IS NULL THEN RETURN NEW; END IF;
    END IF;
    IF pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'state BEFORE depth'; END IF;
    original:=to_jsonb(NEW);
    IF TG_TABLE_NAME IN ('workflow_action_evidence','workflow_action_evidence_commands')
        AND NOT (original @> (config.expected->CASE TG_TABLE_NAME WHEN 'workflow_action_evidence' THEN 'evidence' ELSE 'command' END))
        THEN RAISE EXCEPTION 'state authentic source identity mismatch'; END IF;
    IF TG_TABLE_NAME='workflow_action_evidence_commands' AND config.mode IN ('native','mismatch-control') THEN NEW:=fixture_state_attack(config,NEW); END IF;
    INSERT INTO fixture_state_capture(company_id,command_key,relation_name,original_new,returned_new,depth)
        VALUES(config.company_id,config.command_key,TG_TABLE_NAME,original,to_jsonb(NEW),pg_trigger_depth());
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_state_audit_before BEFORE INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION fixture_state_before();
CREATE TRIGGER fixture_state_evidence_before BEFORE INSERT ON workflow_action_evidence FOR EACH ROW EXECUTE FUNCTION fixture_state_before();
CREATE TRIGGER fixture_state_command_before BEFORE INSERT ON workflow_action_evidence_commands FOR EACH ROW EXECUTE FUNCTION fixture_state_before();
