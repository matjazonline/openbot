-- Installed only in a fresh OwnDatabase, with exactly one deferred attachment.
-- Reuse the complete proposal already checked before the externally observed gate.
CREATE FUNCTION fixture_commit_injected() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_commit_config%ROWTYPE;
    candidate workflow_action_evidence_commands%ROWTYPE;
    captured fixture_commit_observed%ROWTYPE;
    current_rows jsonb; expected_rows jsonb; candidate_row jsonb; prior_job jsonb;
BEGIN
    SELECT config.* INTO STRICT configured FROM fixture_commit_config AS config;
    SELECT command.* INTO STRICT candidate FROM workflow_action_evidence_commands AS command
        WHERE command.company_id=configured.company_id AND command.command_key=configured.command_key;
    PERFORM fixture_commit_require(configured.mode IN ('positive','final_positive') AND TG_NARGS=1
        AND TG_WHEN='AFTER' AND TG_LEVEL='ROW' AND pg_trigger_depth()=1,'injection attachment');
    CASE TG_ARGV[0]
        WHEN 'evidence' THEN
            PERFORM fixture_commit_require(TG_TABLE_NAME='workflow_action_evidence' AND TG_OP='INSERT','injection evidence owner');
            IF NEW.company_id<>candidate.company_id OR NEW.id<>candidate.evidence_id THEN RETURN NULL; END IF;
        WHEN 'receipt' THEN
            PERFORM fixture_commit_require(configured.mode='positive' AND TG_TABLE_NAME='workflow_action_receipts' AND TG_OP='INSERT','injection receipt owner');
            IF NEW.company_id<>candidate.company_id OR NEW.reconciliation_evidence_id IS DISTINCT FROM candidate.evidence_id THEN RETURN NULL; END IF;
        WHEN 'reopen' THEN
            PERFORM fixture_commit_require(TG_TABLE_NAME='background_tasks' AND TG_OP='UPDATE','injection reopen owner');
            IF NEW.id<>candidate.scheduled_job_id THEN RETURN NULL; END IF;
            SELECT value INTO STRICT prior_job FROM jsonb_array_elements(configured.baseline->'background_tasks')
                WHERE value->>'id'=candidate.scheduled_job_id::text;
            PERFORM fixture_commit_require(to_jsonb(OLD)=prior_job AND OLD.status='failed' AND NEW.status='pending','injection exact reopen');
        WHEN 'audit' THEN
            PERFORM fixture_commit_require(TG_TABLE_NAME='workflow_run_events' AND TG_OP='INSERT','injection audit owner');
            IF NEW.company_id<>candidate.company_id OR NEW.run_id<>candidate.run_id OR NEW.sequence<>candidate.audit_sequence THEN RETURN NULL; END IF;
            PERFORM fixture_commit_require(NEW.event_kind='action_reconciled','injection audit kind');
        WHEN 'command' THEN
            PERFORM fixture_commit_require(TG_TABLE_NAME='workflow_action_evidence_commands' AND TG_OP='INSERT','injection command owner');
            IF NEW.company_id<>candidate.company_id OR NEW.id<>candidate.id THEN RETURN NULL; END IF;
        ELSE RAISE EXCEPTION 'row3e observer: unknown injection position';
    END CASE;
    SELECT observation.* INTO STRICT captured FROM fixture_commit_observed AS observation
        WHERE observation.company_id=candidate.company_id AND observation.command_key=candidate.command_key;
    PERFORM fixture_commit_require(captured.owner_pid=pg_backend_pid() AND captured.database_name=current_database(),
        'injection exact captured owner');
    SELECT value INTO STRICT candidate_row FROM jsonb_array_elements(captured.proposed->TG_TABLE_NAME)
        WHERE value=to_jsonb(NEW);
    PERFORM fixture_commit_require(candidate_row=to_jsonb(NEW),'injection exact queued row');
    expected_rows := jsonb_set(captured.proposed,'{fixture_commit_observed}',jsonb_build_array(to_jsonb(captured)));
    current_rows := fixture_commit_public();
    PERFORM fixture_commit_require(current_rows=expected_rows,'injection complete unchanged valid proposal');
    IF configured.mode='final_positive' THEN
        RAISE EXCEPTION 'row3e injected Final %', TG_ARGV[0];
    END IF;
    RAISE EXCEPTION 'row3e injected Applied %', TG_ARGV[0];
END $$;
