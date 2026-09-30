-- A separate audit-only seam; the accepted AppliedMissing probe remains unchanged.
CREATE TABLE fixture_audit_execution_config (
    company_id uuid NOT NULL, command_key text NOT NULL, probe_case text NOT NULL,
    target_oid bigint NOT NULL, target_name text NOT NULL, expected jsonb NOT NULL,
    first_execution uuid NOT NULL, first_job uuid NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_audit_execution_capture (
    company_id uuid NOT NULL, command_key text NOT NULL, relation_name text NOT NULL,
    original_new jsonb NOT NULL, returned_new jsonb NOT NULL, depth integer NOT NULL,
    PRIMARY KEY(company_id,command_key,relation_name)
);
CREATE TABLE fixture_audit_execution_witness (
    company_id uuid NOT NULL, command_key text NOT NULL, selected_flush boolean NOT NULL,
    all_immediate boolean NOT NULL, owner_pid integer NOT NULL, pending jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_audit_execution_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_audit_execution_config%ROWTYPE; original jsonb; returned jsonb; matches integer;
BEGIN
    IF TG_TABLE_NAME='workflow_run_events' THEN
        IF NEW.event_kind<>'action_reconciled' THEN RETURN NEW; END IF;
        SELECT count(*) INTO matches FROM fixture_audit_execution_config AS configured
            WHERE configured.company_id=NEW.company_id AND configured.expected->'command'->>'run_id'=NEW.run_id::text
                AND configured.expected->'command'->>'execution_id'=NEW.execution_id::text
                AND configured.expected->'command'->>'actor_id'=NEW.actor_id::text;
        IF matches=0 THEN RETURN NEW; END IF;
        IF matches<>1 THEN RAISE EXCEPTION 'audit seam ambiguous genuine event'; END IF;
        config := (SELECT configured FROM fixture_audit_execution_config AS configured
            WHERE configured.company_id=NEW.company_id AND configured.expected->'command'->>'run_id'=NEW.run_id::text
                AND configured.expected->'command'->>'execution_id'=NEW.execution_id::text
                AND configured.expected->'command'->>'actor_id'=NEW.actor_id::text);
        original := to_jsonb(NEW);
        IF config.probe_case IN ('audit-execution','mismatch-control') THEN
            NEW.execution_id := config.first_execution;
        END IF;
    ELSE
        config := (SELECT configured FROM fixture_audit_execution_config AS configured
            WHERE configured.company_id=NEW.company_id AND configured.command_key=NEW.command_key);
        IF config.company_id IS NULL THEN RETURN NEW; END IF;
        original := to_jsonb(NEW);
        IF NOT (original @> (config.expected->CASE TG_TABLE_NAME
            WHEN 'workflow_action_evidence' THEN 'evidence' ELSE 'command' END))
            THEN RAISE EXCEPTION 'audit seam original identity mismatch'; END IF;
    END IF;
    IF pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'audit seam unexpected BEFORE depth'; END IF;
    returned := to_jsonb(NEW);
    INSERT INTO fixture_audit_execution_capture(company_id,command_key,relation_name,original_new,returned_new,depth)
        VALUES(config.company_id,config.command_key,TG_TABLE_NAME,original,returned,pg_trigger_depth());
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_audit_execution_event_before BEFORE INSERT ON workflow_run_events
    FOR EACH ROW EXECUTE FUNCTION fixture_audit_execution_before();
CREATE TRIGGER fixture_audit_execution_evidence_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_audit_execution_before();
CREATE TRIGGER fixture_audit_execution_command_before BEFORE INSERT ON workflow_action_evidence_commands
    FOR EACH ROW EXECUTE FUNCTION fixture_audit_execution_before();

CREATE FUNCTION fixture_audit_execution_images(config fixture_audit_execution_config, raw_pending jsonb)
    RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE capture fixture_audit_execution_capture%ROWTYPE; canonical jsonb := raw_pending;
    relation_key text; normalized jsonb; hits integer := 0;
BEGIN
    FOR capture IN SELECT observed.company_id,observed.command_key,observed.relation_name,
        observed.original_new,observed.returned_new,observed.depth FROM fixture_audit_execution_capture AS observed
        WHERE observed.company_id=config.company_id AND observed.command_key=config.command_key LOOP
        relation_key := CASE capture.relation_name WHEN 'workflow_run_events' THEN 'audit'
            WHEN 'workflow_action_evidence' THEN 'evidence' WHEN 'workflow_action_evidence_commands' THEN 'command' ELSE NULL END;
        IF (relation_key IS NULL OR capture.depth<>1 OR capture.returned_new<>raw_pending->relation_key)
            IS DISTINCT FROM false THEN RAISE EXCEPTION 'audit seam raw capture mismatch'; END IF;
        normalized := capture.returned_new;
        IF relation_key='audit' AND config.probe_case IN ('audit-execution','mismatch-control') THEN
            IF (capture.original_new->>'execution_id'=config.first_execution::text
                OR capture.returned_new->>'execution_id'<>config.first_execution::text) IS DISTINCT FROM false
                THEN RAISE EXCEPTION 'audit seam missing declared execution change'; END IF;
            -- Normalize assertion data only. No protected fact is written back.
            normalized := jsonb_set(normalized,'{execution_id}',capture.original_new->'execution_id');
        END IF;
        IF normalized<>capture.original_new THEN RAISE EXCEPTION 'audit seam undeclared difference'; END IF;
        canonical := jsonb_set(canonical,ARRAY[relation_key],capture.original_new);
        hits := hits+1;
    END LOOP;
    IF hits<>3 THEN RAISE EXCEPTION 'audit seam requires exactly three genuine NEW images'; END IF;
    RETURN canonical;
END $$;

CREATE FUNCTION fixture_audit_execution_validate(config fixture_audit_execution_config,
    candidate workflow_action_evidence_commands, canonical jsonb, raw_pending jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE applied_config fixture_command_probe_config%ROWTYPE; exact_evidence boolean; exact_event boolean;
BEGIN
    applied_config.expected := config.expected;
    PERFORM fixture_command_probe_validate(applied_config,candidate,canonical);
    SELECT EXISTS(SELECT 1 FROM workflow_executions AS execution
        JOIN background_tasks AS task ON task.workflow_execution_id=execution.id AND task.company_id=execution.company_id
        WHERE execution.company_id=candidate.company_id AND execution.run_id=candidate.run_id
            AND execution.id=config.first_execution AND execution.completed_at IS NOT NULL
            AND execution.successor_execution_id=candidate.execution_id
            AND task.id=config.first_job AND task.status='completed'
            AND execution.id<>candidate.execution_id) INTO exact_event;
    IF NOT exact_event THEN RAISE EXCEPTION 'audit seam missing authentic completed first execution'; END IF;
    SELECT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
        WHERE evidence.company_id=candidate.company_id AND evidence.id=candidate.evidence_id
            AND evidence.command_key=candidate.command_key AND evidence.command_id=candidate.id
            AND evidence.request_digest=candidate.request_digest AND evidence.actor_id=candidate.actor_id
            AND evidence.run_id=candidate.run_id AND evidence.execution_id=candidate.execution_id
            AND evidence.invocation_id=candidate.invocation_id AND evidence.argument_digest=candidate.argument_digest
            AND evidence.dispatch_id=candidate.dispatch_id) INTO exact_evidence;
    SELECT EXISTS(SELECT 1 FROM workflow_run_events AS audit
        JOIN workflow_executions AS execution ON execution.company_id=audit.company_id
            AND execution.run_id=audit.run_id AND execution.id=audit.execution_id
        WHERE audit.company_id=candidate.company_id AND audit.run_id=candidate.run_id
            AND audit.sequence=candidate.audit_sequence AND audit.actor_id=candidate.actor_id
            AND audit.event_kind='action_reconciled') INTO exact_event;
    IF NOT exact_evidence OR NOT exact_event OR raw_pending->'command'<>canonical->'command'
        OR raw_pending->'evidence'<>canonical->'evidence' THEN
        RAISE EXCEPTION 'audit seam nonattacked native predicates invalid'; END IF;
END $$;

CREATE FUNCTION fixture_audit_execution_after() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_audit_execution_config%ROWTYPE; candidate workflow_action_evidence_commands%ROWTYPE;
    row_count integer; raw_pending jsonb; canonical jsonb; envelope jsonb;
    native_state text; native_constraint text; native_message text; native_context text; native_schema text; native_table text;
    prefix text; expected_message text;
BEGIN
    SELECT count(*) INTO row_count FROM inserted_audit_commands AS inserted
        JOIN fixture_audit_execution_config AS configured ON configured.company_id=inserted.company_id
            AND configured.command_key=inserted.command_key;
    IF row_count=0 THEN RETURN NULL; END IF;
    IF row_count<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'audit seam ambiguous owner statement'; END IF;
    candidate := (SELECT inserted FROM inserted_audit_commands AS inserted
        JOIN fixture_audit_execution_config AS configured ON configured.company_id=inserted.company_id
            AND configured.command_key=inserted.command_key);
    config := (SELECT configured FROM fixture_audit_execution_config AS configured
        WHERE configured.company_id=candidate.company_id AND configured.command_key=candidate.command_key);
    IF NOT EXISTS(SELECT 1 FROM pg_constraint AS native
        JOIN pg_trigger AS trigger_row ON trigger_row.tgconstraint=native.oid
        JOIN pg_proc AS function_row ON function_row.oid=trigger_row.tgfoid
        WHERE native.oid=config.target_oid AND native.conname=config.target_name
            AND native.connamespace='public'::regnamespace AND native.condeferrable AND native.condeferred
            AND native.conrelid='workflow_action_evidence_commands'::regclass
            AND trigger_row.tgenabled='O' AND trigger_row.tgtype=5
            AND function_row.proname='workflow_action_command_scope_guard')
        THEN RAISE EXCEPTION 'audit seam lost exact native target'; END IF;
    raw_pending := fixture_command_probe_pending(candidate.company_id,candidate.command_key);
    canonical := fixture_audit_execution_images(config,raw_pending);
    PERFORM fixture_audit_execution_validate(config,candidate,canonical,raw_pending);
    BEGIN
        EXECUTE format('SET CONSTRAINTS public.%I IMMEDIATE',config.target_name);
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_constraint=CONSTRAINT_NAME,
            native_message=MESSAGE_TEXT,native_context=PG_EXCEPTION_CONTEXT,native_schema=SCHEMA_NAME,native_table=TABLE_NAME;
    END;
    IF config.probe_case='positive' THEN
        IF native_state IS NOT NULL THEN RAISE EXCEPTION 'audit positive native failure: % %',native_state,native_message; END IF;
        SET CONSTRAINTS ALL IMMEDIATE;
        INSERT INTO fixture_audit_execution_witness(company_id,command_key,selected_flush,all_immediate,owner_pid,pending)
            VALUES(candidate.company_id,candidate.command_key,true,true,pg_backend_pid(),raw_pending);
        RETURN NULL;
    END IF;
    envelope := jsonb_build_object('version',1,'probe_case',config.probe_case,'company_id',candidate.company_id,
        'command_key',candidate.command_key,'target_oid',config.target_oid,'target_name',config.target_name,
        'before_hits',3,'after_hits',1,'depth',pg_trigger_depth(),'owner_pid',pg_backend_pid(),'database',current_database(),
        'first_execution',config.first_execution,'first_job',config.first_job,'original',canonical,'returned',raw_pending,
        'images_checked',true,'source_checked',true,'selected_executed',true,'native_state',native_state,
        'native_constraint',native_constraint,'native_message',native_message,'native_context',native_context,
        'native_schema',native_schema,'native_table',native_table);
    IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'audit diagnostic overflow'; END IF;
    expected_message := CASE WHEN config.probe_case='mismatch-control' THEN 'workflow action command evidence scope mismatch'
        ELSE 'workflow action command audit scope mismatch' END;
    prefix := CASE WHEN native_state IS NULL THEN 'FIXTURE_AUDIT_EXECUTION_ACCEPTED_V1:'
        WHEN COALESCE(native_state='23514' AND native_constraint='' AND native_schema='' AND native_table=''
            AND native_message=expected_message AND position('workflow_action_command_scope_guard()' IN native_context)>0
            AND position('SET CONSTRAINTS public.workflow_action_command_scope_guard IMMEDIATE' IN native_context)>0,false)
        THEN 'FIXTURE_AUDIT_EXECUTION_NATIVE_V1:' ELSE 'FIXTURE_AUDIT_EXECUTION_MISMATCH_V1:' END;
    -- Every nonpositive branch aborts the outer ordinary service owner.
    RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
END $$;
CREATE TRIGGER fixture_audit_execution_command_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_audit_commands FOR EACH STATEMENT
    EXECUTE FUNCTION fixture_audit_execution_after();
