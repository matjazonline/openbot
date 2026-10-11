-- Disposable OwnDatabase fixture; all native INSERT owners remain installed.
CREATE TABLE fixture_native_command_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL,
    target text NOT NULL, replacement jsonb, PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_native_command_images (
    company_id uuid NOT NULL, command_key text NOT NULL, original jsonb NOT NULL,
    candidate jsonb NOT NULL, PRIMARY KEY(company_id,command_key)
);
CREATE FUNCTION fixture_native_command_probe(candidate workflow_action_evidence_commands)
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE native_state text; native_schema text; native_table text; native_constraint text;
    native_message text; native_context text;
BEGIN
    BEGIN
        INSERT INTO public.workflow_action_evidence_commands(
            company_id,command_key,id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,
            actor_id,request_digest,expected_revision,result_revision,outcome,evidence_id,audit_sequence,
            scheduled_job_id,previous_state,previous_waiting_reason,created_at)
        VALUES(candidate.company_id,candidate.command_key,candidate.id,candidate.run_id,candidate.execution_id,
            candidate.invocation_id,candidate.argument_digest,candidate.dispatch_id,candidate.actor_id,
            candidate.request_digest,candidate.expected_revision,candidate.result_revision,candidate.outcome,
            candidate.evidence_id,candidate.audit_sequence,candidate.scheduled_job_id,candidate.previous_state,
            candidate.previous_waiting_reason,candidate.created_at);
        RAISE EXCEPTION USING ERRCODE='PZ001',MESSAGE='native diagnostic probe accepted';
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_schema=SCHEMA_NAME,
            native_table=TABLE_NAME,native_constraint=CONSTRAINT_NAME,native_message=MESSAGE_TEXT,
            native_context=PG_EXCEPTION_CONTEXT;
    END;
    RETURN jsonb_build_object('state',native_state,'schema',native_schema,'table',native_table,
        'constraint',native_constraint,'message',native_message,'context',native_context);
END $$;
CREATE FUNCTION fixture_native_command_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_native_command_config%ROWTYPE; captured fixture_native_command_images%ROWTYPE;
    original jsonb := to_jsonb(NEW); candidate jsonb; native jsonb; envelope jsonb;
    evidence jsonb; audit jsonb; job jsonb;
BEGIN
    SELECT settings.company_id,settings.command_key,settings.mode,settings.target,settings.replacement
    INTO config FROM fixture_native_command_config AS settings
    WHERE settings.company_id=NEW.company_id AND settings.command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF pg_trigger_depth()=2 THEN
        SELECT images.company_id,images.command_key,images.original,images.candidate INTO STRICT captured
        FROM fixture_native_command_images AS images
        WHERE images.company_id=NEW.company_id AND images.command_key=NEW.command_key;
        IF captured.candidate IS DISTINCT FROM original OR config.mode='positive' THEN
            RAISE EXCEPTION 'native command nested identity mismatch';
        END IF;
        RETURN NEW;
    END IF;
    IF pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native command unexpected depth'; END IF;
    IF config.mode NOT IN ('orphan','foreign','accepted','positive','wrong','null','missing')
        OR config.target NOT IN ('dispatch_id','audit_sequence','scheduled_job_id',
            'previous_state','previous_waiting_reason','kind') THEN
        RAISE EXCEPTION 'native command unexpected config';
    END IF;
    IF config.mode='positive' THEN RETURN NEW; END IF;
    SELECT to_jsonb(stored) INTO STRICT evidence FROM workflow_action_evidence AS stored
    WHERE stored.company_id=NEW.company_id AND stored.id=NEW.evidence_id;
    SELECT to_jsonb(stored) INTO STRICT audit FROM workflow_run_events AS stored
    WHERE stored.company_id=NEW.company_id AND stored.run_id=NEW.run_id AND stored.sequence=NEW.audit_sequence;
    SELECT to_jsonb(stored) INTO STRICT job FROM background_tasks AS stored
    WHERE stored.company_id=NEW.company_id AND stored.id=NEW.scheduled_job_id;
    candidate := original;
    IF config.mode NOT IN ('accepted','positive') THEN
        IF config.replacement IS NULL AND config.mode<>'missing' THEN RAISE EXCEPTION 'missing replacement'; END IF;
        IF config.target='kind' THEN
            IF config.mode='missing' THEN candidate := original #- '{outcome,kind}';
            ELSE candidate := jsonb_set(original,'{outcome,kind}',config.replacement); END IF;
        ELSE candidate := jsonb_set(original,ARRAY[config.target],config.replacement); END IF;
    END IF;
    INSERT INTO fixture_native_command_images(company_id,command_key,original,candidate)
        VALUES(NEW.company_id,NEW.command_key,original,candidate);
    native := fixture_native_command_probe(jsonb_populate_record(NULL::workflow_action_evidence_commands,candidate));
    envelope := jsonb_build_object('version',1,'mode',config.mode,'original',original,'candidate',candidate,
        'native',native,'owner_pid',pg_backend_pid(),'database',current_database(),'depth',pg_trigger_depth(),
        'evidence',evidence,'audit',audit,'job',job);
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native command envelope overflow'; END IF;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE='FIXTURE_NATIVE_COMMAND_V1:'||envelope::text;
END $$;
CREATE TRIGGER fixture_native_command_before BEFORE INSERT ON workflow_action_evidence_commands
    FOR EACH ROW EXECUTE FUNCTION fixture_native_command_before();
