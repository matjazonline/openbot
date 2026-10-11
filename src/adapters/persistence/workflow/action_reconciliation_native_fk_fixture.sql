-- Immediate initial-INSERT probes; the diagnostic fixture's config stays empty.
CREATE TABLE fixture_native_fk_config (
    company_id uuid NOT NULL, command_key text NOT NULL, target text NOT NULL,
    mode text NOT NULL, replacement uuid, PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_native_fk_images (
    company_id uuid NOT NULL, command_key text NOT NULL, target text NOT NULL,
    original jsonb NOT NULL, candidate jsonb NOT NULL, evidence jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE FUNCTION fixture_native_fk_coverage_probe(candidate workflow_action_evidence_coverage)
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE native_state text; native_schema text; native_table text; native_constraint text;
    native_message text; native_context text;
BEGIN
    BEGIN
        INSERT INTO public.workflow_action_evidence_coverage(
            company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,evidence_id,remote_entry_id)
        VALUES(candidate.company_id,candidate.run_id,candidate.execution_id,candidate.invocation_id,
            candidate.argument_digest,candidate.dispatch_id,candidate.evidence_id,candidate.remote_entry_id);
        RAISE EXCEPTION USING ERRCODE='PZ001', MESSAGE='native diagnostic probe accepted';
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_schema=SCHEMA_NAME,
            native_table=TABLE_NAME,native_constraint=CONSTRAINT_NAME,native_message=MESSAGE_TEXT,
            native_context=PG_EXCEPTION_CONTEXT;
    END;
    RETURN jsonb_build_object('state',native_state,'schema',native_schema,'table',native_table,
        'constraint',native_constraint,'message',native_message,'context',native_context);
END $$;

CREATE FUNCTION fixture_native_fk_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_fk_config%ROWTYPE; captured fixture_native_fk_images%ROWTYPE;
    original jsonb := to_jsonb(NEW); candidate jsonb; evidence jsonb; diagnostic jsonb;
    envelope jsonb; prefix text; selected_key text; field_name text;
BEGIN
    IF pg_trigger_depth()=2 THEN
        SELECT images.company_id,images.command_key,images.target,images.original,images.candidate,images.evidence
        INTO STRICT captured FROM fixture_native_fk_images AS images
        WHERE images.company_id=NEW.company_id AND images.candidate=to_jsonb(NEW);
        SELECT config.company_id,config.command_key,config.target,config.mode,config.replacement
        INTO STRICT configured FROM fixture_native_fk_config AS config
        WHERE config.company_id=captured.company_id AND config.command_key=captured.command_key;
        IF configured.mode NOT IN ('orphan','foreign','accepted') OR configured.target<>captured.target
            OR (TG_TABLE_NAME='workflow_action_evidence') IS DISTINCT FROM (configured.target='dispatch')
            OR captured.evidence->>'id' IS DISTINCT FROM
                (CASE WHEN configured.target='dispatch' THEN captured.original->>'id'
                    ELSE captured.original->>'evidence_id' END) THEN
            RAISE EXCEPTION 'native FK nested identity mismatch';
        END IF;
        RETURN NEW;
    END IF;
    IF pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native FK unexpected depth'; END IF;
    IF TG_TABLE_NAME='workflow_action_evidence' THEN
        evidence := original;
    ELSE
        SELECT to_jsonb(stored) INTO STRICT evidence FROM workflow_action_evidence AS stored
        WHERE stored.company_id=NEW.company_id AND stored.id=NEW.evidence_id;
    END IF;
    selected_key := evidence->>'command_key';
    SELECT config.company_id,config.command_key,config.target,config.mode,config.replacement
    INTO configured FROM fixture_native_fk_config AS config
    WHERE config.company_id=NEW.company_id AND config.command_key=selected_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF (TG_TABLE_NAME='workflow_action_evidence')<>(configured.target='dispatch') THEN RETURN NEW; END IF;
    IF configured.mode NOT IN ('orphan','foreign','accepted','positive')
        OR configured.target NOT IN ('dispatch','evidence','entry') THEN
        RAISE EXCEPTION 'native FK unexpected target or mode';
    END IF;
    field_name := CASE configured.target WHEN 'dispatch' THEN 'dispatch_id'
        WHEN 'evidence' THEN 'evidence_id' ELSE 'remote_entry_id' END;
    candidate := original;
    IF configured.mode IN ('orphan','foreign') THEN
        IF configured.replacement IS NULL THEN RAISE EXCEPTION 'native FK missing replacement'; END IF;
        candidate := jsonb_set(original,ARRAY[field_name],to_jsonb(configured.replacement));
    ELSIF configured.replacement IS NOT NULL THEN RAISE EXCEPTION 'native FK unexpected replacement';
    END IF;
    INSERT INTO fixture_native_fk_images(company_id,command_key,target,original,candidate,evidence)
        VALUES(NEW.company_id,selected_key,configured.target,original,candidate,evidence);
    IF configured.mode='positive' THEN RETURN NEW; END IF;
    IF configured.target='dispatch' THEN
        diagnostic := fixture_native_diagnostic_probe(jsonb_populate_record(NULL::workflow_action_evidence,candidate));
    ELSE
        diagnostic := fixture_native_fk_coverage_probe(jsonb_populate_record(NULL::workflow_action_evidence_coverage,candidate));
    END IF;
    envelope := jsonb_build_object('version',1,'mode',configured.mode,'original',original,
        'candidate',candidate,'evidence',evidence,'native',diagnostic,'owner_pid',pg_backend_pid(),
        'database',current_database(),'depth',pg_trigger_depth());
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native FK envelope overflow'; END IF;
    prefix := CASE WHEN diagnostic->>'state'='PZ001' THEN 'FIXTURE_NATIVE_FK_ACCEPTED_V1:'
        ELSE 'FIXTURE_NATIVE_FK_CAPTURED_V1:' END;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_native_fk_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_native_fk_before();
CREATE TRIGGER fixture_native_fk_before BEFORE INSERT ON workflow_action_evidence_coverage
    FOR EACH ROW EXECUTE FUNCTION fixture_native_fk_before();
