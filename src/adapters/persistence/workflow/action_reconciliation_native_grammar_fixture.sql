-- Disposable-database substitution of one configured provenance value into the real service's NEW.
-- Installed after the accepted scalar fixture, whose probe function and image/witness tables
-- (fixture_native_diagnostic_images, fixture_native_diagnostic_witness) it reuses; that fixture
-- stays inert because its config table has no rows. The config table carries no CHECK so the
-- public CHECK catalog stays production-only; the triggers enforce mode and column instead.
CREATE TABLE fixture_native_grammar_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL,
    target text NOT NULL, value text NOT NULL,
    PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_native_grammar_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_grammar_config%ROWTYPE; original jsonb; envelope jsonb;
    captured fixture_native_diagnostic_images%ROWTYPE; diagnostic jsonb; prefix text;
BEGIN
    SELECT config.company_id,config.command_key,config.mode,config.target,config.value INTO configured
        FROM fixture_native_grammar_config AS config
        WHERE config.company_id=NEW.company_id AND config.command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF pg_trigger_depth()=2 THEN
        SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO STRICT captured FROM fixture_native_diagnostic_images AS images
            WHERE images.company_id=NEW.company_id AND images.command_key=NEW.command_key;
        IF captured.depth<>1 OR captured.candidate IS DISTINCT FROM to_jsonb(NEW)
            OR configured.mode<>'rejected' THEN
            RAISE EXCEPTION 'native grammar nested identity mismatch';
        END IF;
        RETURN NEW;
    END IF;
    IF pg_trigger_depth()<>1 OR configured.mode NOT IN ('rejected','positive')
        OR configured.target NOT IN ('registration','verifier_version','provider','authoritative_reference') THEN
        RAISE EXCEPTION 'native grammar unexpected depth, mode or target';
    END IF;
    IF NEW.registration IS NULL OR NEW.verifier_version IS NULL OR NEW.provider IS NULL
        OR NEW.authoritative_reference IS NULL OR NEW.operation_signature IS NULL THEN
        RAISE EXCEPTION 'native grammar unregistered source';
    END IF;
    original := to_jsonb(NEW);
    CASE configured.target
        WHEN 'registration' THEN NEW.registration := configured.value;
        WHEN 'verifier_version' THEN NEW.verifier_version := configured.value;
        WHEN 'provider' THEN NEW.provider := configured.value;
        ELSE NEW.authoritative_reference := configured.value;
    END CASE;
    IF original->>configured.target IS NOT DISTINCT FROM configured.value THEN
        RAISE EXCEPTION 'native grammar substitution is not a change';
    END IF;
    INSERT INTO fixture_native_diagnostic_images(company_id,command_key,original,candidate,depth)
        VALUES(NEW.company_id,NEW.command_key,original,to_jsonb(NEW),pg_trigger_depth());
    IF configured.mode='positive' THEN RETURN NEW; END IF;
    diagnostic := fixture_native_diagnostic_probe(NEW);
    envelope := jsonb_build_object('version',1,'mode',configured.mode,'original',original,
        'candidate',to_jsonb(NEW),'native',diagnostic,'owner_pid',pg_backend_pid(),
        'database',current_database(),'depth',pg_trigger_depth());
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native grammar envelope overflow'; END IF;
    prefix := CASE WHEN diagnostic->>'state'='PZ001' THEN 'FIXTURE_NATIVE_GRAMMAR_ACCEPTED_V1:'
        ELSE 'FIXTURE_NATIVE_GRAMMAR_CAPTURED_V1:' END;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_native_grammar_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_native_grammar_before();

CREATE FUNCTION fixture_native_grammar_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE; matched integer;
    configured fixture_native_grammar_config%ROWTYPE; captured fixture_native_diagnostic_images%ROWTYPE;
BEGIN
    SELECT count(*) INTO matched FROM inserted_commands AS inserted
        JOIN fixture_native_grammar_config AS config
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key;
    IF matched=0 THEN RETURN NULL; END IF;
    IF matched<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native grammar ambiguous pair'; END IF;
    SELECT config.company_id,config.command_key,config.mode,config.target,config.value INTO STRICT configured
        FROM fixture_native_grammar_config AS config JOIN inserted_commands AS inserted
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key
        WHERE config.mode='positive';
    candidate := (SELECT inserted FROM inserted_commands AS inserted
        WHERE inserted.company_id=configured.company_id AND inserted.command_key=configured.command_key);
    SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO STRICT captured FROM fixture_native_diagnostic_images AS images
        WHERE images.company_id=candidate.company_id AND images.command_key=candidate.command_key;
    IF captured.depth<>1
        OR captured.original - configured.target IS DISTINCT FROM captured.candidate - configured.target
        OR captured.candidate->>configured.target IS DISTINCT FROM configured.value
        OR captured.original->>configured.target IS NOT DISTINCT FROM configured.value
        OR captured.original->>'id' IS DISTINCT FROM candidate.evidence_id::text
        OR captured.original->>'command_id' IS DISTINCT FROM candidate.id::text
        OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
            WHERE evidence.company_id=candidate.company_id AND evidence.id=candidate.evidence_id
            AND to_jsonb(evidence)=captured.candidate) THEN
        RAISE EXCEPTION 'native grammar positive pair identity mismatch';
    END IF;
    SET CONSTRAINTS public.workflow_action_evidence_commit_guard IMMEDIATE;
    SET CONSTRAINTS ALL IMMEDIATE;
    INSERT INTO fixture_native_diagnostic_witness(company_id,command_key,evidence_id,command_id,
        selected_flush,all_flush) VALUES(candidate.company_id,candidate.command_key,
        candidate.evidence_id,candidate.id,true,true);
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_native_grammar_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_commands FOR EACH STATEMENT
    EXECUTE FUNCTION fixture_native_grammar_after();
