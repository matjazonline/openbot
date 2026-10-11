-- Disposable-database substitution of one configured scalar into the real service's NEW, for the
-- command_key, digest, disposition and operation_signature CHECKs. Installed after the accepted
-- scalar fixture, whose probe function and image/witness tables it reuses; that fixture stays inert
-- because its config table has no rows. Everything is keyed on the ORIGINAL command_key, because the
-- substitution may replace it: the nested probe row is matched by its evidence id instead. The
-- config table carries no CHECK so the public CHECK catalog stays production-only.
CREATE TABLE fixture_native_constraint_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL,
    target text NOT NULL, value text NOT NULL,
    PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_native_constraint_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_constraint_config%ROWTYPE; original jsonb; envelope jsonb;
    captured fixture_native_diagnostic_images%ROWTYPE; diagnostic jsonb; prefix text;
BEGIN
    IF pg_trigger_depth()=2 THEN
        SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO captured
            FROM fixture_native_diagnostic_images AS images
            WHERE images.company_id=NEW.company_id AND images.original->>'id'=NEW.id::text;
        IF NOT FOUND THEN RETURN NEW; END IF;
        SELECT config.company_id,config.command_key,config.mode,config.target,config.value INTO STRICT configured
            FROM fixture_native_constraint_config AS config
            WHERE config.company_id=captured.company_id AND config.command_key=captured.command_key;
        IF captured.depth<>1 OR captured.candidate IS DISTINCT FROM to_jsonb(NEW)
            OR configured.mode<>'rejected' THEN
            RAISE EXCEPTION 'native constraint nested identity mismatch';
        END IF;
        RETURN NEW;
    END IF;
    SELECT config.company_id,config.command_key,config.mode,config.target,config.value INTO configured
        FROM fixture_native_constraint_config AS config
        WHERE config.company_id=NEW.company_id AND config.command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF pg_trigger_depth()<>1 OR configured.mode NOT IN ('rejected','positive')
        OR configured.target NOT IN ('command_key','request_digest','coverage_digest','disposition',
            'operation_signature') THEN
        RAISE EXCEPTION 'native constraint unexpected depth, mode or target';
    END IF;
    IF NEW.registration IS NULL OR NEW.verifier_version IS NULL OR NEW.provider IS NULL
        OR NEW.authoritative_reference IS NULL OR NEW.operation_signature IS NULL THEN
        RAISE EXCEPTION 'native constraint unregistered source';
    END IF;
    original := to_jsonb(NEW);
    IF configured.mode='positive' THEN
        -- The genuine service value is committed unchanged; nothing is substituted.
        IF configured.value<>'' THEN RAISE EXCEPTION 'native constraint positive substitutes'; END IF;
    ELSE
        CASE configured.target
            WHEN 'command_key' THEN NEW.command_key := configured.value;
            WHEN 'request_digest' THEN NEW.request_digest := configured.value;
            WHEN 'coverage_digest' THEN NEW.coverage_digest := configured.value;
            WHEN 'disposition' THEN NEW.disposition := configured.value;
            WHEN 'operation_signature' THEN NEW.operation_signature := configured.value;
        END CASE;
        IF original->>configured.target IS NOT DISTINCT FROM configured.value THEN
            RAISE EXCEPTION 'native constraint substitution is not a change';
        END IF;
    END IF;
    INSERT INTO fixture_native_diagnostic_images(company_id,command_key,original,candidate,depth)
        VALUES(configured.company_id,configured.command_key,original,to_jsonb(NEW),pg_trigger_depth());
    IF configured.mode='positive' THEN RETURN NEW; END IF;
    diagnostic := fixture_native_diagnostic_probe(NEW);
    envelope := jsonb_build_object('version',1,'mode',configured.mode,'original',original,
        'candidate',to_jsonb(NEW),'native',diagnostic,'owner_pid',pg_backend_pid(),
        'database',current_database(),'depth',pg_trigger_depth());
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native constraint envelope overflow'; END IF;
    prefix := CASE WHEN diagnostic->>'state'='PZ001' THEN 'FIXTURE_NATIVE_CONSTRAINT_ACCEPTED_V1:'
        ELSE 'FIXTURE_NATIVE_CONSTRAINT_CAPTURED_V1:' END;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_native_constraint_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_native_constraint_before();

CREATE FUNCTION fixture_native_constraint_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE; matched integer;
    configured fixture_native_constraint_config%ROWTYPE; captured fixture_native_diagnostic_images%ROWTYPE;
BEGIN
    SELECT count(*) INTO matched FROM inserted_commands AS inserted
        JOIN fixture_native_constraint_config AS config
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key;
    IF matched=0 THEN RETURN NULL; END IF;
    IF matched<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native constraint ambiguous pair'; END IF;
    SELECT config.company_id,config.command_key,config.mode,config.target,config.value INTO STRICT configured
        FROM fixture_native_constraint_config AS config JOIN inserted_commands AS inserted
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key
        WHERE config.mode='positive' AND config.value='';
    candidate := (SELECT inserted FROM inserted_commands AS inserted
        WHERE inserted.company_id=configured.company_id AND inserted.command_key=configured.command_key);
    SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO STRICT captured
        FROM fixture_native_diagnostic_images AS images
        WHERE images.company_id=candidate.company_id AND images.command_key=candidate.command_key;
    IF captured.depth<>1
        OR captured.original IS DISTINCT FROM captured.candidate
        OR captured.original->>configured.target IS NULL
        OR captured.original->>'id' IS DISTINCT FROM candidate.evidence_id::text
        OR captured.original->>'command_id' IS DISTINCT FROM candidate.id::text
        OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
            WHERE evidence.company_id=candidate.company_id AND evidence.id=candidate.evidence_id
            AND to_jsonb(evidence)=captured.original) THEN
        RAISE EXCEPTION 'native constraint positive pair identity mismatch';
    END IF;
    SET CONSTRAINTS public.workflow_action_evidence_commit_guard IMMEDIATE;
    SET CONSTRAINTS ALL IMMEDIATE;
    INSERT INTO fixture_native_diagnostic_witness(company_id,command_key,evidence_id,command_id,
        selected_flush,all_flush) VALUES(candidate.company_id,candidate.command_key,
        candidate.evidence_id,candidate.id,true,true);
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_native_constraint_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_commands FOR EACH STATEMENT
    EXECUTE FUNCTION fixture_native_constraint_after();
