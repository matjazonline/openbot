-- Disposable-database instrumentation of the real service's prospective NEW.
CREATE TABLE fixture_native_diagnostic_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_native_diagnostic_images (
    company_id uuid NOT NULL, command_key text NOT NULL, original jsonb NOT NULL,
    candidate jsonb NOT NULL, depth integer NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_native_diagnostic_witness (
    company_id uuid NOT NULL, command_key text NOT NULL, evidence_id uuid NOT NULL,
    command_id uuid NOT NULL, selected_flush boolean NOT NULL, all_flush boolean NOT NULL,
    PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_native_diagnostic_probe(candidate workflow_action_evidence)
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE native_state text; native_schema text; native_table text; native_constraint text;
    native_message text; native_context text;
BEGIN
    BEGIN
        INSERT INTO public.workflow_action_evidence(
            company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,
            actor_id,command_id,command_key,request_digest,coverage_digest,disposition,
            registration,verifier_version,provider,authoritative_reference,operation_signature,
            observed_at,verified_at,valid_until,created_at,grant_eligible,diagnostic,
            applied_request,applied_remote_entry_id)
        VALUES(candidate.company_id,candidate.run_id,candidate.execution_id,candidate.invocation_id,
            candidate.argument_digest,candidate.dispatch_id,candidate.id,candidate.actor_id,
            candidate.command_id,candidate.command_key,candidate.request_digest,candidate.coverage_digest,
            candidate.disposition,candidate.registration,candidate.verifier_version,candidate.provider,
            candidate.authoritative_reference,candidate.operation_signature,candidate.observed_at,
            candidate.verified_at,candidate.valid_until,candidate.created_at,candidate.grant_eligible,
            candidate.diagnostic,candidate.applied_request,candidate.applied_remote_entry_id);
        -- Force rollback of an unexpectedly accepted initial INSERT, including queued guards.
        RAISE EXCEPTION USING ERRCODE='PZ001', MESSAGE='native diagnostic probe accepted';
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_schema=SCHEMA_NAME,
            native_table=TABLE_NAME,native_constraint=CONSTRAINT_NAME,native_message=MESSAGE_TEXT,
            native_context=PG_EXCEPTION_CONTEXT;
    END;
    RETURN jsonb_build_object('state',native_state,'schema',native_schema,'table',native_table,
        'constraint',native_constraint,'message',native_message,'context',native_context);
END $$;

CREATE FUNCTION fixture_native_diagnostic_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_diagnostic_config%ROWTYPE; original jsonb; envelope jsonb;
    captured fixture_native_diagnostic_images%ROWTYPE; diagnostic jsonb; prefix text;
BEGIN
    SELECT config.company_id,config.command_key,config.mode INTO configured FROM fixture_native_diagnostic_config AS config
        WHERE config.company_id=NEW.company_id AND config.command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF pg_trigger_depth()=2 THEN
        SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO STRICT captured FROM fixture_native_diagnostic_images AS images
            WHERE images.company_id=NEW.company_id AND images.command_key=NEW.command_key;
        IF captured.depth<>1 OR captured.candidate IS DISTINCT FROM to_jsonb(NEW)
            OR configured.mode NOT IN ('scalar','acceptance') THEN
            RAISE EXCEPTION 'native diagnostic nested identity mismatch';
        END IF;
        RETURN NEW;
    END IF;
    IF pg_trigger_depth()<>1 OR configured.mode NOT IN ('scalar','acceptance','positive') THEN
        RAISE EXCEPTION 'native diagnostic unexpected depth or mode';
    END IF;
    original := to_jsonb(NEW);
    IF configured.mode='scalar' THEN NEW.diagnostic := '7'::jsonb; END IF;
    INSERT INTO fixture_native_diagnostic_images(company_id,command_key,original,candidate,depth)
        VALUES(NEW.company_id,NEW.command_key,original,to_jsonb(NEW),pg_trigger_depth());
    IF configured.mode='positive' THEN RETURN NEW; END IF;
    diagnostic := fixture_native_diagnostic_probe(NEW);
    envelope := jsonb_build_object('version',1,'mode',configured.mode,'original',original,
        'candidate',to_jsonb(NEW),'native',diagnostic,'owner_pid',pg_backend_pid(),
        'database',current_database(),'depth',pg_trigger_depth());
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native diagnostic envelope overflow'; END IF;
    prefix := CASE WHEN diagnostic->>'state'='PZ001' THEN 'FIXTURE_NATIVE_DIAGNOSTIC_ACCEPTED_V1:'
        ELSE 'FIXTURE_NATIVE_DIAGNOSTIC_CAPTURED_V1:' END;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_native_diagnostic_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_native_diagnostic_before();

CREATE FUNCTION fixture_native_diagnostic_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE; matched integer;
    captured fixture_native_diagnostic_images%ROWTYPE;
BEGIN
    SELECT count(*) INTO matched FROM inserted_commands AS inserted
        JOIN fixture_native_diagnostic_config AS config
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key;
    IF matched=0 THEN RETURN NULL; END IF;
    IF matched<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native diagnostic ambiguous pair'; END IF;
    candidate := (SELECT inserted FROM inserted_commands AS inserted
        JOIN fixture_native_diagnostic_config AS config
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key
        WHERE config.mode='positive');
    SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO STRICT captured FROM fixture_native_diagnostic_images AS images
        WHERE images.company_id=candidate.company_id AND images.command_key=candidate.command_key;
    IF captured.original IS DISTINCT FROM captured.candidate OR captured.depth<>1
        OR captured.original->>'id' IS DISTINCT FROM candidate.evidence_id::text
        OR captured.original->>'command_id' IS DISTINCT FROM candidate.id::text
        OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
            WHERE evidence.company_id=candidate.company_id AND evidence.id=candidate.evidence_id
            AND to_jsonb(evidence)=captured.original) THEN
        RAISE EXCEPTION 'native diagnostic positive pair identity mismatch';
    END IF;
    SET CONSTRAINTS public.workflow_action_evidence_commit_guard IMMEDIATE;
    SET CONSTRAINTS ALL IMMEDIATE;
    INSERT INTO fixture_native_diagnostic_witness(company_id,command_key,evidence_id,command_id,
        selected_flush,all_flush) VALUES(candidate.company_id,candidate.command_key,
        candidate.evidence_id,candidate.id,true,true);
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_native_diagnostic_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_commands FOR EACH STATEMENT
    EXECUTE FUNCTION fixture_native_diagnostic_after();
