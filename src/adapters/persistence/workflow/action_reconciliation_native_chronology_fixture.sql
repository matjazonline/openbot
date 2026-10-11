-- Disposable-database substitution of the covered entry's chronology into the real service's NEW.
-- Installed after the accepted scalar fixture, whose image/witness tables it reuses (that fixture
-- stays inert: its config table has no rows). The deferred production provenance guard is the
-- owner, so the AFTER STATEMENT trigger flushes it by name once the complete pair exists. The
-- predicate copy below is supporting evidence only; it never attributes the native rejection.
-- No CHECK on the config table: the triggers enforce mode and anchor instead.
CREATE TABLE fixture_native_chronology_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL, anchor text NOT NULL,
    PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_native_chronology_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_chronology_config%ROWTYPE; original jsonb;
    marker workflow_action_dispatches%ROWTYPE; entry workflow_action_remote_entries%ROWTYPE;
BEGIN
    SELECT config.company_id,config.command_key,config.mode,config.anchor INTO configured
        FROM fixture_native_chronology_config AS config
        WHERE config.company_id=NEW.company_id AND config.command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF pg_trigger_depth()<>1 OR (configured.mode,configured.anchor) NOT IN (('rejected','marker'),
        ('accepted','entry'),('positive','entry')) THEN
        RAISE EXCEPTION 'native chronology unexpected depth, mode or anchor';
    END IF;
    SELECT * INTO STRICT marker FROM workflow_action_dispatches AS dispatch
        WHERE dispatch.company_id=NEW.company_id AND dispatch.invocation_id=NEW.invocation_id;
    SELECT * INTO STRICT entry FROM workflow_action_remote_entries AS remote
        WHERE remote.company_id=NEW.company_id AND remote.invocation_id=NEW.invocation_id;
    IF NOT (marker.created_at<entry.created_at AND entry.created_at<=NEW.verified_at
        AND NEW.observed_at>entry.created_at) THEN
        RAISE EXCEPTION 'native chronology unexpected source order';
    END IF;
    original := to_jsonb(NEW);
    NEW.observed_at := CASE configured.anchor WHEN 'marker' THEN marker.created_at ELSE entry.created_at END;
    INSERT INTO fixture_native_diagnostic_images(company_id,command_key,original,candidate,depth)
        VALUES(NEW.company_id,NEW.command_key,original,to_jsonb(NEW),pg_trigger_depth());
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_native_chronology_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_native_chronology_before();

CREATE FUNCTION fixture_native_chronology_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE; matched integer; predicates jsonb;
    configured fixture_native_chronology_config%ROWTYPE; captured fixture_native_diagnostic_images%ROWTYPE;
    evidence workflow_action_evidence%ROWTYPE; marker workflow_action_dispatches%ROWTYPE;
    single_entry workflow_action_remote_entries%ROWTYPE; envelope jsonb; prefix text;
    native_state text; native_constraint text; native_message text; native_context text;
    native_schema text; native_table text; anchored timestamptz;
BEGIN
    SELECT count(*) INTO matched FROM inserted_commands AS inserted
        JOIN fixture_native_chronology_config AS config
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key;
    IF matched=0 THEN RETURN NULL; END IF;
    IF matched<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native chronology ambiguous pair'; END IF;
    SELECT config.company_id,config.command_key,config.mode,config.anchor INTO STRICT configured
        FROM fixture_native_chronology_config AS config JOIN inserted_commands AS inserted
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key;
    candidate := (SELECT inserted FROM inserted_commands AS inserted
        WHERE inserted.company_id=configured.company_id AND inserted.command_key=configured.command_key);
    SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO STRICT captured FROM fixture_native_diagnostic_images AS images
        WHERE images.company_id=candidate.company_id AND images.command_key=candidate.command_key;
    SELECT * INTO STRICT evidence FROM workflow_action_evidence AS stored
        WHERE stored.company_id=candidate.company_id AND stored.id=candidate.evidence_id;
    SELECT * INTO STRICT marker FROM workflow_action_dispatches AS dispatch
        WHERE dispatch.company_id=evidence.company_id AND dispatch.invocation_id=evidence.invocation_id;
    SELECT * INTO STRICT single_entry FROM workflow_action_remote_entries AS remote
        WHERE remote.company_id=evidence.company_id AND remote.invocation_id=evidence.invocation_id;
    anchored := CASE configured.anchor WHEN 'marker' THEN marker.created_at ELSE single_entry.created_at END;
    IF captured.depth<>1
        OR captured.original - 'observed_at' IS DISTINCT FROM captured.candidate - 'observed_at'
        OR captured.original->>'id' IS DISTINCT FROM candidate.evidence_id::text
        OR captured.original->>'command_id' IS DISTINCT FROM candidate.id::text
        OR to_jsonb(evidence) IS DISTINCT FROM captured.candidate
        OR evidence.observed_at IS DISTINCT FROM anchored
        OR (captured.original->>'observed_at')::timestamptz<=single_entry.created_at THEN
        RAISE EXCEPTION 'native chronology pair identity mismatch';
    END IF;
    -- Literal transcription of workflow_action_evidence_commit_guard (migration
    -- 20260930181000_workflow_action_evidence_binding.sql:132-150) with NEW written as the stored
    -- `evidence` row; each key is true when that disjunct does not fire, except
    -- entry_after_observed, which is true when its half fires. verified_le_created copies the CHECK
    -- (20260930180000_workflow_action_evidence.sql:27). The guard's final disjunct is split into
    -- entry_after_observed (target) and all_entries_covered: that disjunct fires exactly when
    -- `entry_after_observed OR NOT all_entries_covered`.
    -- Pair completeness the guard does not read: audit_pair copies the command scope guard
    -- (20260930184000_workflow_action_audit_commands.sql:71-74), plus run head and single coverage.
    SELECT jsonb_build_object(
        'remote_marker',(marker.effect_kind='remote') IS TRUE,
        'observed_ge_marker',(NOT evidence.observed_at<marker.created_at) IS TRUE,
        'verified_not_future',(NOT evidence.verified_at>clock_timestamp()) IS TRUE,
        'verified_le_created',(evidence.verified_at<=evidence.created_at) IS TRUE,
        'registered_live',(NOT (evidence.registration IS NOT NULL AND evidence.valid_until<=clock_timestamp())) IS TRUE,
        'command_link',EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
            WHERE command.company_id=evidence.company_id AND command.command_key=evidence.command_key
            AND command.evidence_id=evidence.id AND command.actor_id=evidence.actor_id
            AND command.id=evidence.command_id AND command.request_digest=evidence.request_digest
            AND command.run_id=evidence.run_id AND command.execution_id=evidence.execution_id
            AND command.invocation_id=evidence.invocation_id AND command.argument_digest=evidence.argument_digest
            AND command.dispatch_id=evidence.dispatch_id),
        'applied_request',(NOT ((evidence.applied_request='remote_entry' AND NOT EXISTS(
            SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=evidence.company_id
                AND coverage.evidence_id=evidence.id AND coverage.remote_entry_id=evidence.applied_remote_entry_id))
            OR (evidence.applied_request='marker_reservation' AND EXISTS(
            SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=evidence.company_id
                AND coverage.evidence_id=evidence.id)))) IS NOT FALSE,
        'coverage_bounded',(NOT (SELECT count(*) FROM workflow_action_evidence_coverage WHERE company_id=evidence.company_id AND evidence_id=evidence.id)>128) IS TRUE,
        'entry_after_observed',EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=evidence.company_id AND entry.invocation_id=evidence.invocation_id
                AND entry.created_at<=evidence.created_at AND (entry.created_at>evidence.observed_at)),
        'all_entries_covered',NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=evidence.company_id AND entry.invocation_id=evidence.invocation_id
                AND entry.created_at<=evidence.created_at AND (NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                    WHERE coverage.company_id=evidence.company_id AND coverage.evidence_id=evidence.id AND coverage.remote_entry_id=entry.id))),
        'audit_pair',EXISTS(SELECT 1 FROM workflow_run_events AS audit
            WHERE audit.company_id=candidate.company_id AND audit.run_id=candidate.run_id
            AND audit.sequence=candidate.audit_sequence AND audit.execution_id=candidate.execution_id
            AND audit.actor_id=candidate.actor_id AND audit.event_kind='action_reconciled'),
        'run_head',EXISTS(SELECT 1 FROM workflow_runs AS run WHERE run.company_id=candidate.company_id
            AND run.id=candidate.run_id AND run.revision=candidate.result_revision),
        'single_coverage',(SELECT count(*) FROM workflow_action_evidence_coverage AS coverage
            WHERE coverage.company_id=evidence.company_id AND coverage.evidence_id=evidence.id)=1
            AND EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
            WHERE coverage.company_id=evidence.company_id AND coverage.evidence_id=evidence.id
            AND coverage.remote_entry_id=single_entry.id)) INTO predicates;
    IF EXISTS(SELECT 1 FROM jsonb_each(predicates) AS predicate WHERE predicate.key<>'entry_after_observed'
            AND predicate.value IS DISTINCT FROM 'true'::jsonb)
        OR predicates->'entry_after_observed' IS DISTINCT FROM to_jsonb(configured.anchor='marker') THEN
        RAISE EXCEPTION 'native chronology non-target predicate %',predicates;
    END IF;
    BEGIN
        SET CONSTRAINTS public.workflow_action_evidence_commit_guard IMMEDIATE;
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_constraint=CONSTRAINT_NAME,
            native_message=MESSAGE_TEXT,native_context=PG_EXCEPTION_CONTEXT,native_schema=SCHEMA_NAME,native_table=TABLE_NAME;
    END;
    IF configured.mode<>'positive' THEN
        -- Chosen from the raw state before any COALESCE: no native error means the ACCEPTED frame.
        prefix := CASE WHEN native_state IS NULL THEN 'FIXTURE_NATIVE_CHRONOLOGY_ACCEPTED_V1:'
            ELSE 'FIXTURE_NATIVE_CHRONOLOGY_CAPTURED_V1:' END;
        envelope := jsonb_build_object('version',1,'mode',configured.mode,'original',captured.original,
            'candidate',captured.candidate,'native',jsonb_build_object('state',COALESCE(native_state,''),
            'schema',COALESCE(native_schema,''),'table',COALESCE(native_table,''),
            'constraint',COALESCE(native_constraint,''),'message',COALESCE(native_message,''),
            'context',COALESCE(native_context,'')),'owner_pid',pg_backend_pid(),
            'database',current_database(),'depth',pg_trigger_depth(),'predicates',predicates);
        IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native chronology envelope overflow'; END IF;
        RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
    END IF;
    IF native_state IS NOT NULL THEN RAISE EXCEPTION 'chronology positive native flush failed: % % % %',
        native_state,native_constraint,native_message,native_context; END IF;
    SET CONSTRAINTS ALL IMMEDIATE;
    INSERT INTO fixture_native_diagnostic_witness(company_id,command_key,evidence_id,command_id,
        selected_flush,all_flush) VALUES(candidate.company_id,candidate.command_key,
        candidate.evidence_id,candidate.id,true,true);
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_native_chronology_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_commands FOR EACH STATEMENT
    EXECUTE FUNCTION fixture_native_chronology_after();
