-- OwnDatabase-only selected deferred coverage owner. Native functions remain unchanged.
-- Configurations bind one genuine service proposal; the scalar fixture stays inert.
CREATE TABLE fixture_native_coverage_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL, target text NOT NULL,
    PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_native_coverage_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_coverage_config%ROWTYPE; original jsonb;
    marker workflow_action_dispatches%ROWTYPE; entry workflow_action_remote_entries%ROWTYPE;
BEGIN
    SELECT config.company_id,config.command_key,config.mode,config.target INTO configured
        FROM fixture_native_coverage_config AS config
        WHERE config.company_id=NEW.company_id AND config.command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF pg_trigger_depth()<>1 OR (configured.mode,configured.target) NOT IN (('rejected','missing'),('accepted','missing'),('positive','missing'),
        ('rejected','membership'),('accepted','membership'),('positive','membership'),
        ('rejected','marker'),('accepted','marker')) THEN
        RAISE EXCEPTION 'native coverage unexpected depth, mode or target';
    END IF;
    SELECT * INTO STRICT marker FROM workflow_action_dispatches AS dispatch
        WHERE dispatch.company_id=NEW.company_id AND dispatch.invocation_id=NEW.invocation_id;
    SELECT * INTO STRICT entry FROM workflow_action_remote_entries AS remote
        WHERE remote.company_id=NEW.company_id AND remote.invocation_id=NEW.invocation_id;
    IF NOT (marker.created_at<entry.created_at AND entry.created_at<=NEW.verified_at
        AND NEW.observed_at>entry.created_at) THEN
        RAISE EXCEPTION 'native coverage unexpected source order';
    END IF;
    original := to_jsonb(NEW);
    IF configured.target='membership' AND configured.mode<>'positive' THEN
        IF NOT (marker.created_at<NEW.valid_until AND NEW.valid_until<=marker.created_at+interval '24 hours') THEN
            RAISE EXCEPTION 'coverage anchor validity precondition';
        END IF;
        NEW.observed_at := marker.created_at;
        NEW.verified_at := marker.created_at;
        NEW.created_at := marker.created_at;
    ELSIF configured.target='marker' THEN
        NEW.applied_request := CASE configured.mode WHEN 'rejected' THEN 'marker_reservation' ELSE 'unattributed' END;
        NEW.applied_remote_entry_id := NULL;
    END IF;
    INSERT INTO fixture_native_diagnostic_images(company_id,command_key,original,candidate,depth)
        VALUES(NEW.company_id,NEW.command_key,original,to_jsonb(NEW),pg_trigger_depth());
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_native_coverage_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_native_coverage_before();

CREATE TABLE fixture_native_coverage_attempts (
    company_id uuid NOT NULL, command_key text NOT NULL, proposed jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_native_coverage_staged (
    company_id uuid NOT NULL, command_key text NOT NULL, proposal jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE FUNCTION fixture_native_coverage_omit() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_coverage_config%ROWTYPE; captured fixture_native_diagnostic_images%ROWTYPE;
BEGIN
    SELECT config.* INTO configured FROM fixture_native_coverage_config AS config
        JOIN fixture_native_diagnostic_images AS images USING(company_id,command_key)
        WHERE images.company_id=NEW.company_id AND images.candidate->>'id'=NEW.evidence_id::text;
    IF NOT FOUND THEN RETURN NEW; END IF;
    SELECT images.* INTO STRICT captured FROM fixture_native_diagnostic_images AS images
        WHERE images.company_id=configured.company_id AND images.command_key=configured.command_key;
    IF pg_trigger_depth()<>1 OR NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
        WHERE entry.company_id=NEW.company_id AND entry.id=NEW.remote_entry_id
        AND entry.run_id=NEW.run_id AND entry.execution_id=NEW.execution_id
        AND entry.invocation_id=NEW.invocation_id AND entry.argument_digest=NEW.argument_digest
        AND entry.dispatch_id=NEW.dispatch_id)
        OR to_jsonb(NEW)-ARRAY['evidence_id','remote_entry_id'] IS DISTINCT FROM
            captured.candidate-ARRAY['id','actor_id','command_id','command_key','request_digest','coverage_digest',
                'disposition','registration','verifier_version','provider','authoritative_reference','operation_signature',
                'observed_at','verified_at','valid_until','created_at','grant_eligible','diagnostic',
                'applied_request','applied_remote_entry_id'] THEN
        RAISE EXCEPTION 'coverage omission exact identity';
    END IF;
    INSERT INTO fixture_native_coverage_attempts VALUES(NEW.company_id,configured.command_key,to_jsonb(NEW));
    IF configured.mode='rejected' AND configured.target IN ('missing','membership') THEN RETURN NULL; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_native_coverage_omit BEFORE INSERT ON workflow_action_evidence_coverage
    FOR EACH ROW EXECUTE FUNCTION fixture_native_coverage_omit();

CREATE FUNCTION fixture_native_coverage_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE; matched integer; predicates jsonb;
    configured fixture_native_coverage_config%ROWTYPE; captured fixture_native_diagnostic_images%ROWTYPE;
    evidence workflow_action_evidence%ROWTYPE; marker workflow_action_dispatches%ROWTYPE;
    single_entry workflow_action_remote_entries%ROWTYPE; envelope jsonb; prefix text;
    native_state text; native_constraint text; native_message text; native_context text;
    native_schema text; native_table text; staged jsonb;
BEGIN
    SELECT count(*) INTO matched FROM inserted_commands AS inserted
        JOIN fixture_native_coverage_config AS config
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key;
    IF matched=0 THEN RETURN NULL; END IF;
    IF matched<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native coverage ambiguous pair'; END IF;
    SELECT config.company_id,config.command_key,config.mode,config.target INTO STRICT configured
        FROM fixture_native_coverage_config AS config JOIN inserted_commands AS inserted
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
    IF captured.depth<>1 OR captured.original->>'id' IS DISTINCT FROM candidate.evidence_id::text
        OR captured.original->>'command_id' IS DISTINCT FROM candidate.id::text
        OR to_jsonb(evidence) IS DISTINCT FROM captured.candidate THEN
        RAISE EXCEPTION 'native coverage pair identity mismatch';
    END IF;
    -- Supporting truth vector only. Native selected SET CONSTRAINTS below owns the result.
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
        'membership_rejects',(evidence.applied_request='remote_entry' AND NOT EXISTS(
            SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=evidence.company_id
                AND coverage.evidence_id=evidence.id AND coverage.remote_entry_id=evidence.applied_remote_entry_id)) IS TRUE,
        'marker_rejects',(evidence.applied_request='marker_reservation' AND EXISTS(
            SELECT 1 FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=evidence.company_id
                AND coverage.evidence_id=evidence.id)) IS TRUE,
        'coverage_bounded',(NOT (SELECT count(*) FROM workflow_action_evidence_coverage WHERE company_id=evidence.company_id AND evidence_id=evidence.id)>128) IS TRUE,
        'entry_after_observed',EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=evidence.company_id AND entry.invocation_id=evidence.invocation_id
                AND entry.created_at<=evidence.created_at AND (entry.created_at>evidence.observed_at)),
        'missing_rejects',EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=evidence.company_id AND entry.invocation_id=evidence.invocation_id
                AND entry.created_at<=evidence.created_at AND (NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage AS coverage
                    WHERE coverage.company_id=evidence.company_id AND coverage.evidence_id=evidence.id AND coverage.remote_entry_id=entry.id))),
        'audit_pair',EXISTS(SELECT 1 FROM workflow_run_events AS audit
            WHERE audit.company_id=candidate.company_id AND audit.run_id=candidate.run_id
            AND audit.sequence=candidate.audit_sequence AND audit.execution_id=candidate.execution_id
            AND audit.actor_id=candidate.actor_id AND audit.event_kind='action_reconciled'),
        'run_head',EXISTS(SELECT 1 FROM workflow_runs AS run WHERE run.company_id=candidate.company_id
            AND run.id=candidate.run_id AND run.revision=candidate.result_revision),
        'coverage_count',(SELECT count(*) FROM workflow_action_evidence_coverage AS coverage
            WHERE coverage.company_id=evidence.company_id AND coverage.evidence_id=evidence.id)) INTO predicates;
    SELECT jsonb_build_object('evidence',to_jsonb(evidence),'command',to_jsonb(candidate),
        'marker',to_jsonb(marker),'entry',to_jsonb(single_entry),'predicates',predicates,
        'attempts',(SELECT jsonb_agg(attempt.proposed) FROM fixture_native_coverage_attempts AS attempt
            WHERE attempt.company_id=candidate.company_id AND attempt.command_key=candidate.command_key),
        'coverage',(SELECT COALESCE(jsonb_agg(to_jsonb(coverage)),'[]'::jsonb)
            FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=evidence.company_id AND coverage.evidence_id=evidence.id),
        'audit',(SELECT to_jsonb(audit) FROM workflow_run_events AS audit WHERE audit.company_id=candidate.company_id
            AND audit.run_id=candidate.run_id AND audit.sequence=candidate.audit_sequence),
        'run',(SELECT to_jsonb(run) FROM workflow_runs AS run WHERE run.company_id=candidate.company_id AND run.id=candidate.run_id),
        'receipt',(SELECT to_jsonb(receipt) FROM workflow_action_receipts AS receipt
            WHERE receipt.company_id=evidence.company_id AND receipt.invocation_id=evidence.invocation_id)) INTO staged;
    INSERT INTO fixture_native_coverage_staged VALUES(candidate.company_id,candidate.command_key,staged);
    BEGIN
        SET CONSTRAINTS public.workflow_action_evidence_commit_guard IMMEDIATE;
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_constraint=CONSTRAINT_NAME,
            native_message=MESSAGE_TEXT,native_context=PG_EXCEPTION_CONTEXT,native_schema=SCHEMA_NAME,native_table=TABLE_NAME;
    END;
    IF configured.mode<>'positive' THEN
        -- Chosen from the raw state before any COALESCE: no native error means the ACCEPTED frame.
        prefix := CASE WHEN native_state IS NULL THEN 'FIXTURE_NATIVE_COVERAGE_ACCEPTED_V1:'
            ELSE 'FIXTURE_NATIVE_COVERAGE_CAPTURED_V1:' END;
        envelope := jsonb_build_object('version',1,'mode',configured.mode,'original',captured.original,
            'candidate',captured.candidate,'native',jsonb_build_object('state',COALESCE(native_state,''),
            'schema',COALESCE(native_schema,''),'table',COALESCE(native_table,''),
            'constraint',COALESCE(native_constraint,''),'message',COALESCE(native_message,''),
            'context',COALESCE(native_context,'')),'owner_pid',pg_backend_pid(),
            'database',current_database(),'depth',pg_trigger_depth(),'staged',staged);
        IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native coverage envelope overflow'; END IF;
        RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
    END IF;
    IF native_state IS NOT NULL THEN RAISE EXCEPTION 'coverage positive native flush failed: % % % %',
        native_state,native_constraint,native_message,native_context; END IF;
    SET CONSTRAINTS ALL IMMEDIATE;
    INSERT INTO fixture_native_diagnostic_witness(company_id,command_key,evidence_id,command_id,
        selected_flush,all_flush) VALUES(candidate.company_id,candidate.command_key,
        candidate.evidence_id,candidate.id,true,true);
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_native_coverage_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_commands FOR EACH STATEMENT
    EXECUTE FUNCTION fixture_native_coverage_after();
