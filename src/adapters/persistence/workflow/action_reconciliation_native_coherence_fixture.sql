-- Disposable-database substitution into the real service's NEW for the evidence row's cross-column
-- CHECKs: time order (observed<=verified<=created), provenance presence (all-or-none) and
-- grant_eligible scope. Installed after the accepted scalar fixture, whose probe function and
-- image/witness tables it reuses; that fixture stays inert because its config table has no rows.
-- Keyed on the ORIGINAL command_key; the nested probe row is matched by its evidence id. The config
-- table carries no CHECK so the public CHECK catalog stays production-only; the triggers enforce
-- mode, shape and source instead and fail closed on anything else.
CREATE TABLE fixture_native_coherence_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL,
    shape text NOT NULL, source text NOT NULL,
    PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_native_coherence_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_coherence_config%ROWTYPE; original jsonb; envelope jsonb;
    captured fixture_native_diagnostic_images%ROWTYPE; diagnostic jsonb; prefix text; absent integer;
BEGIN
    IF pg_trigger_depth()=2 THEN
        SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO captured
            FROM fixture_native_diagnostic_images AS images
            WHERE images.company_id=NEW.company_id AND images.original->>'id'=NEW.id::text;
        IF NOT FOUND THEN RETURN NEW; END IF;
        SELECT config.company_id,config.command_key,config.mode,config.shape,config.source INTO STRICT configured
            FROM fixture_native_coherence_config AS config
            WHERE config.company_id=captured.company_id AND config.command_key=captured.command_key;
        IF captured.depth<>1 OR captured.candidate IS DISTINCT FROM to_jsonb(NEW)
            OR configured.mode<>'rejected' THEN
            RAISE EXCEPTION 'native coherence nested identity mismatch';
        END IF;
        RETURN NEW;
    END IF;
    SELECT config.company_id,config.command_key,config.mode,config.shape,config.source INTO configured
        FROM fixture_native_coherence_config AS config
        WHERE config.company_id=NEW.company_id AND config.command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF pg_trigger_depth()<>1
        OR NOT ((configured.mode='rejected' AND configured.shape IN ('observed_after_verified',
            'verified_after_created','null_registration','null_verifier_version','null_provider',
            'null_authoritative_reference','null_operation_signature','grant_eligible','registration_present'))
            OR (configured.mode='positive' AND configured.shape IN ('observed_equals_verified',
            'verified_equals_created','genuine')))
        OR configured.source NOT IN ('registered','unregistered')
        OR (configured.source='unregistered' AND configured.shape NOT IN ('registration_present','genuine'))
        OR (configured.source='registered' AND configured.shape='registration_present') THEN
        RAISE EXCEPTION 'native coherence unexpected depth, mode, shape or source';
    END IF;
    absent := num_nulls(NEW.registration,NEW.verifier_version,NEW.provider,NEW.authoritative_reference,
        NEW.operation_signature);
    IF (configured.source='registered' AND absent<>0)
        OR (configured.source='unregistered' AND (absent<>5 OR NEW.valid_until IS DISTINCT FROM NEW.verified_at)) THEN
        RAISE EXCEPTION 'native coherence source mismatch';
    END IF;
    original := to_jsonb(NEW);
    CASE configured.shape
        WHEN 'observed_after_verified' THEN NEW.observed_at := NEW.verified_at + interval '1 microsecond';
        WHEN 'verified_after_created' THEN NEW.verified_at := NEW.created_at + interval '1 microsecond';
        WHEN 'observed_equals_verified' THEN NEW.observed_at := NEW.verified_at;
        WHEN 'verified_equals_created' THEN NEW.verified_at := NEW.created_at;
        WHEN 'null_registration' THEN NEW.registration := NULL;
        WHEN 'null_verifier_version' THEN NEW.verifier_version := NULL;
        WHEN 'null_provider' THEN NEW.provider := NULL;
        WHEN 'null_authoritative_reference' THEN NEW.authoritative_reference := NULL;
        WHEN 'null_operation_signature' THEN NEW.operation_signature := NULL;
        WHEN 'grant_eligible' THEN NEW.grant_eligible := true;
        WHEN 'registration_present' THEN NEW.registration := 'reg';
        WHEN 'genuine' THEN NULL;
    END CASE;
    -- Every rejected substitution must change the row and the genuine positive must not. The
    -- service issues verified_at as max(elapsed clock, observed_at) (service.rs:173), so a genuine
    -- row may already sit on an equality boundary: those positives require the boundary instead.
    IF configured.shape='observed_equals_verified' THEN
        IF NEW.observed_at IS DISTINCT FROM NEW.verified_at
            OR original - 'observed_at' IS DISTINCT FROM to_jsonb(NEW) - 'observed_at' THEN
            RAISE EXCEPTION 'native coherence boundary mismatch';
        END IF;
    ELSIF configured.shape='verified_equals_created' THEN
        IF NEW.verified_at IS DISTINCT FROM NEW.created_at
            OR original - 'verified_at' IS DISTINCT FROM to_jsonb(NEW) - 'verified_at' THEN
            RAISE EXCEPTION 'native coherence boundary mismatch';
        END IF;
    ELSIF (configured.shape<>'genuine') IS DISTINCT FROM (original IS DISTINCT FROM to_jsonb(NEW)) THEN
        RAISE EXCEPTION 'native coherence substitution mismatch';
    END IF;
    INSERT INTO fixture_native_diagnostic_images(company_id,command_key,original,candidate,depth)
        VALUES(configured.company_id,configured.command_key,original,to_jsonb(NEW),pg_trigger_depth());
    IF configured.mode='positive' THEN RETURN NEW; END IF;
    diagnostic := fixture_native_diagnostic_probe(NEW);
    envelope := jsonb_build_object('version',1,'mode',configured.mode,'original',original,
        'candidate',to_jsonb(NEW),'native',diagnostic,'owner_pid',pg_backend_pid(),
        'database',current_database(),'depth',pg_trigger_depth());
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native coherence envelope overflow'; END IF;
    prefix := CASE WHEN diagnostic->>'state'='PZ001' THEN 'FIXTURE_NATIVE_COHERENCE_ACCEPTED_V1:'
        ELSE 'FIXTURE_NATIVE_COHERENCE_CAPTURED_V1:' END;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_native_coherence_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_native_coherence_before();

CREATE FUNCTION fixture_native_coherence_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE; matched integer;
    configured fixture_native_coherence_config%ROWTYPE; captured fixture_native_diagnostic_images%ROWTYPE;
BEGIN
    SELECT count(*) INTO matched FROM inserted_commands AS inserted
        JOIN fixture_native_coherence_config AS config
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key;
    IF matched=0 THEN RETURN NULL; END IF;
    IF matched<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native coherence ambiguous pair'; END IF;
    SELECT config.company_id,config.command_key,config.mode,config.shape,config.source INTO STRICT configured
        FROM fixture_native_coherence_config AS config JOIN inserted_commands AS inserted
        ON config.company_id=inserted.company_id AND config.command_key=inserted.command_key
        WHERE config.mode='positive';
    candidate := (SELECT inserted FROM inserted_commands AS inserted
        WHERE inserted.company_id=configured.company_id AND inserted.command_key=configured.command_key);
    SELECT images.company_id,images.command_key,images.original,images.candidate,images.depth INTO STRICT captured
        FROM fixture_native_diagnostic_images AS images
        WHERE images.company_id=candidate.company_id AND images.command_key=candidate.command_key;
    IF captured.depth<>1
        OR (configured.shape='genuine' AND captured.original IS DISTINCT FROM captured.candidate)
        OR (configured.shape='observed_equals_verified' AND (
            captured.original - 'observed_at' IS DISTINCT FROM captured.candidate - 'observed_at'
            OR captured.candidate->'observed_at' IS DISTINCT FROM captured.candidate->'verified_at'))
        OR (configured.shape='verified_equals_created' AND (
            captured.original - 'verified_at' IS DISTINCT FROM captured.candidate - 'verified_at'
            OR captured.candidate->'verified_at' IS DISTINCT FROM captured.candidate->'created_at'))
        OR captured.original->>'id' IS DISTINCT FROM candidate.evidence_id::text
        OR captured.original->>'command_id' IS DISTINCT FROM candidate.id::text
        OR NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
            WHERE evidence.company_id=candidate.company_id AND evidence.id=candidate.evidence_id
            AND to_jsonb(evidence)=captured.candidate) THEN
        RAISE EXCEPTION 'native coherence positive pair identity mismatch';
    END IF;
    SET CONSTRAINTS public.workflow_action_evidence_commit_guard IMMEDIATE;
    SET CONSTRAINTS ALL IMMEDIATE;
    INSERT INTO fixture_native_diagnostic_witness(company_id,command_key,evidence_id,command_id,
        selected_flush,all_flush) VALUES(candidate.company_id,candidate.command_key,
        candidate.evidence_id,candidate.id,true,true);
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_native_coherence_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_commands FOR EACH STATEMENT
    EXECUTE FUNCTION fixture_native_coherence_after();
