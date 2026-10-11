-- Immediate probes only. The shared diagnostic fixture remains inert (empty config).
CREATE TABLE fixture_native_attribution_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL,
    replacement uuid NOT NULL, PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_native_attribution_images (
    company_id uuid NOT NULL, command_key text NOT NULL, original jsonb NOT NULL,
    candidate jsonb NOT NULL, PRIMARY KEY(company_id,command_key)
);
CREATE FUNCTION fixture_native_attribution_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE configured fixture_native_attribution_config%ROWTYPE;
    captured fixture_native_attribution_images%ROWTYPE; original jsonb := to_jsonb(NEW);
    diagnostic jsonb; envelope jsonb; prefix text; own_entry uuid; predicates jsonb;
BEGIN
    SELECT config.company_id,config.command_key,config.mode,config.replacement INTO configured
        FROM fixture_native_attribution_config AS config
        WHERE config.company_id=NEW.company_id AND config.command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    IF pg_trigger_depth()=2 THEN
        SELECT images.company_id,images.command_key,images.original,images.candidate INTO STRICT captured
            FROM fixture_native_attribution_images AS images
            WHERE images.company_id=NEW.company_id AND images.command_key=NEW.command_key;
        IF captured.candidate IS DISTINCT FROM to_jsonb(NEW) THEN
            RAISE EXCEPTION 'native attribution nested identity mismatch';
        END IF;
        RETURN NEW;
    END IF;
    IF pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'native attribution unexpected depth'; END IF;
    SELECT entry.id INTO STRICT own_entry FROM workflow_action_remote_entries AS entry
        WHERE entry.company_id=NEW.company_id AND entry.invocation_id=NEW.invocation_id;
    CASE configured.mode
        WHEN 'invalid' THEN NEW.applied_request := 'invalid';
        WHEN 'missing_entry' THEN NEW.applied_remote_entry_id := NULL;
        WHEN 'marker_entry' THEN NEW.applied_request := 'marker_reservation';
        WHEN 'unattributed_entry' THEN NEW.applied_request := 'unattributed';
        WHEN 'unknown_attributed' THEN NEW.applied_request := 'remote_entry'; NEW.applied_remote_entry_id := own_entry;
        WHEN 'final_attributed' THEN NEW.disposition := 'final_not_applied';
        WHEN 'orphan','foreign' THEN NEW.applied_remote_entry_id := configured.replacement;
        WHEN 'missing_attribution' THEN NEW.applied_request := NULL; NEW.applied_remote_entry_id := NULL;
        WHEN 'null_with_entry' THEN NEW.applied_request := NULL;
        WHEN 'remote_accepted','unknown_accepted' THEN NULL;
        WHEN 'marker_accepted' THEN NEW.applied_request := 'marker_reservation'; NEW.applied_remote_entry_id := NULL;
        WHEN 'unattributed_accepted' THEN NEW.applied_request := 'unattributed'; NEW.applied_remote_entry_id := NULL;
        WHEN 'final_accepted' THEN NEW.disposition := 'final_not_applied'; NEW.applied_request := NULL; NEW.applied_remote_entry_id := NULL;
        ELSE RAISE EXCEPTION 'native attribution unexpected mode';
    END CASE;
    -- Supporting truth values only. Native INSERT diagnostics below own rejection.
    predicates := jsonb_build_object(
        'grammar',NEW.applied_request IN ('remote_entry','marker_reservation','unattributed'),
        'shape',(NEW.applied_request IS NULL AND NEW.applied_remote_entry_id IS NULL)
            OR (NEW.disposition='applied' AND ((NEW.applied_request='remote_entry' AND NEW.applied_remote_entry_id IS NOT NULL)
            OR (NEW.applied_request IN ('marker_reservation','unattributed') AND NEW.applied_remote_entry_id IS NULL))),
        'before_rejects',NEW.disposition='applied' AND NEW.applied_request IS NULL);
    INSERT INTO fixture_native_attribution_images(company_id,command_key,original,candidate)
        VALUES(NEW.company_id,NEW.command_key,original,to_jsonb(NEW));
    diagnostic := fixture_native_diagnostic_probe(NEW);
    envelope := jsonb_build_object('version',1,'mode',configured.mode,'original',original,
        'candidate',to_jsonb(NEW),'native',diagnostic,'owner_pid',pg_backend_pid(),
        'database',current_database(),'depth',pg_trigger_depth(),'predicates',predicates);
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'native attribution envelope overflow'; END IF;
    prefix := CASE WHEN diagnostic->>'state'='PZ001' THEN 'FIXTURE_NATIVE_ATTRIBUTION_ACCEPTED_V1:'
        ELSE 'FIXTURE_NATIVE_ATTRIBUTION_CAPTURED_V1:' END;
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_native_attribution_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_native_attribution_before();
