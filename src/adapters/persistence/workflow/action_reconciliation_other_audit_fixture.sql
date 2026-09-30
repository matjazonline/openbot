-- Test-only actual NEW capture. Native ownership UNIQUE is never caught here.
CREATE TABLE fixture_other_audit_config (
    version integer NOT NULL CHECK(version=1), mode text NOT NULL CHECK(mode IN ('unchanged','other-audit')),
    company_id uuid PRIMARY KEY,command_key text NOT NULL,expected jsonb NOT NULL,
    history jsonb NOT NULL,baseline jsonb NOT NULL,owner_pid integer NOT NULL,database_name text NOT NULL
);
CREATE TABLE fixture_other_audit_capture (
    company_id uuid PRIMARY KEY,command_key text NOT NULL,original_new jsonb NOT NULL,returned_new jsonb NOT NULL,
    pending jsonb NOT NULL,owner_pid integer NOT NULL,after_checked boolean NOT NULL DEFAULT false
);
CREATE FUNCTION fixture_other_audit_pending(candidate workflow_action_evidence_commands) RETURNS jsonb LANGUAGE sql AS $$
    SELECT jsonb_build_object('command',to_jsonb(candidate),
        'evidence',(SELECT to_jsonb(evidence) FROM workflow_action_evidence AS evidence WHERE evidence.company_id=candidate.company_id AND evidence.id=candidate.evidence_id),
        'audit',(SELECT to_jsonb(audit) FROM workflow_run_events AS audit WHERE audit.company_id=candidate.company_id AND audit.run_id=candidate.run_id AND audit.sequence=candidate.audit_sequence),
        'run',(SELECT to_jsonb(owner) FROM workflow_runs AS owner WHERE owner.company_id=candidate.company_id AND owner.id=candidate.run_id),
        'job',(SELECT to_jsonb(job) FROM background_tasks AS job WHERE job.company_id=candidate.company_id AND job.id=candidate.scheduled_job_id),
        'marker',(SELECT to_jsonb(marker) FROM workflow_action_dispatches AS marker WHERE marker.company_id=candidate.company_id AND marker.id=candidate.dispatch_id),
        'coverage',COALESCE((SELECT jsonb_agg(to_jsonb(coverage) ORDER BY coverage.remote_entry_id) FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=candidate.company_id AND coverage.evidence_id=candidate.evidence_id),'[]'::jsonb),
        'entries',COALESCE((SELECT jsonb_agg(to_jsonb(entry) ORDER BY entry.id) FROM workflow_action_remote_entries AS entry WHERE entry.company_id=candidate.company_id AND entry.invocation_id=candidate.invocation_id),'[]'::jsonb));
$$;
CREATE FUNCTION fixture_other_audit_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_other_audit_config%ROWTYPE; original jsonb; returned jsonb; pending jsonb; hits integer;
BEGIN
    SELECT count(*) INTO hits FROM fixture_other_audit_config AS configured WHERE configured.company_id=NEW.company_id AND configured.command_key=NEW.command_key;
    IF hits=0 THEN RETURN NEW; END IF;
    IF hits<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'OtherAudit ambiguous/deep selected command'; END IF;
    config:=(SELECT configured FROM fixture_other_audit_config AS configured WHERE configured.company_id=NEW.company_id AND configured.command_key=NEW.command_key);
    IF config.version<>1 OR config.mode NOT IN ('unchanged','other-audit') OR pg_backend_pid()<>config.owner_pid OR current_database()<>config.database_name
        OR EXISTS(SELECT 1 FROM fixture_other_audit_capture AS capture WHERE capture.company_id=NEW.company_id)
        THEN RAISE EXCEPTION 'OtherAudit version/mode/backend/second command'; END IF;
    original:=to_jsonb(NEW); pending:=fixture_other_audit_pending(NEW);
    PERFORM fixture_other_audit_validate(config,NEW,pending);
    IF EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command WHERE command.company_id=NEW.company_id AND command.command_key=NEW.command_key)
        OR EXISTS(SELECT 1 FROM workflow_action_claim_episodes AS episode WHERE episode.company_id=NEW.company_id AND episode.command_key=NEW.command_key)
        THEN RAISE EXCEPTION 'OtherAudit BEFORE cannot have command/episode'; END IF;
    IF config.mode='other-audit' THEN NEW.audit_sequence:=(config.history->'audit'->>'sequence')::bigint; END IF;
    returned:=to_jsonb(NEW);
    IF config.mode='other-audit' THEN
        IF original->'audit_sequence'=returned->'audit_sequence'
            OR returned->'audit_sequence'<>config.history->'audit'->'sequence'
            OR jsonb_set(returned,'{audit_sequence}',original->'audit_sequence')<>original
            THEN RAISE EXCEPTION 'OtherAudit mutation must change only audit_sequence'; END IF;
    ELSIF returned<>original THEN RAISE EXCEPTION 'OtherAudit unchanged NEW differed'; END IF;
    -- Transactional negative captures vanish; these assertions execute before UNIQUE.
    INSERT INTO fixture_other_audit_capture(company_id,command_key,original_new,returned_new,pending,owner_pid)
        VALUES(NEW.company_id,NEW.command_key,original,returned,pending,pg_backend_pid());
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_other_audit_before BEFORE INSERT ON workflow_action_evidence_commands
    FOR EACH ROW EXECUTE FUNCTION fixture_other_audit_before();
CREATE FUNCTION fixture_other_audit_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_other_audit_config%ROWTYPE; candidate workflow_action_evidence_commands%ROWTYPE; hits integer;
BEGIN
    SELECT count(*) INTO hits FROM inserted_other_audit AS inserted JOIN fixture_other_audit_config AS configured
        ON configured.company_id=inserted.company_id AND configured.command_key=inserted.command_key;
    IF hits=0 THEN RETURN NULL; END IF;
    IF hits<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'OtherAudit ambiguous AFTER'; END IF;
    candidate:=(SELECT inserted FROM inserted_other_audit AS inserted JOIN fixture_other_audit_config AS configured
        ON configured.company_id=inserted.company_id AND configured.command_key=inserted.command_key);
    config:=(SELECT configured FROM fixture_other_audit_config AS configured WHERE configured.company_id=candidate.company_id);
    IF config.mode<>'unchanged' THEN RAISE EXCEPTION 'OtherAudit UNIQUE negative unexpectedly inserted'; END IF;
    PERFORM fixture_other_audit_validate(config,candidate,fixture_other_audit_pending(candidate));
    IF (SELECT count(*) FROM fixture_other_audit_capture AS capture WHERE capture.company_id=candidate.company_id AND capture.original_new=to_jsonb(candidate) AND capture.returned_new=to_jsonb(candidate))<>1
        OR NOT EXISTS(SELECT 1 FROM workflow_action_claim_episodes AS episode JOIN workflow_action_schedule_witnesses AS witness
            ON witness.company_id=episode.company_id AND witness.run_id=episode.run_id AND witness.execution_id=episode.execution_id
                AND witness.job_id=episode.job_id AND witness.retired_attempt=episode.retired_attempt
            WHERE episode.company_id=candidate.company_id AND episode.command_key=candidate.command_key AND episode.job_id=candidate.scheduled_job_id
                AND witness.transaction_id=pg_current_xact_id())
        THEN RAISE EXCEPTION 'OtherAudit positive native command/episode absent'; END IF;
    SET CONSTRAINTS workflow_control_retry_guard IMMEDIATE;
    SET CONSTRAINTS ALL IMMEDIATE;
    UPDATE fixture_other_audit_capture SET after_checked=true WHERE company_id=candidate.company_id;
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_other_audit_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_other_audit FOR EACH STATEMENT EXECUTE FUNCTION fixture_other_audit_after();
