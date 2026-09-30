-- Disposable OwnDatabase only. Native diagnostics are captured before service transport.
CREATE TABLE fixture_command_probe_config (
    company_id uuid NOT NULL, command_key text NOT NULL, target text NOT NULL,
    target_oid bigint NOT NULL, target_name text NOT NULL, expected jsonb NOT NULL,
    probe_case text NOT NULL DEFAULT 'positive', replacement uuid,
    foreign_company uuid, foreign_command uuid, foreign_evidence uuid, foreign_actor uuid,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_command_probe_before (
    company_id uuid NOT NULL, command_key text NOT NULL, relation_name text NOT NULL,
    original_new jsonb NOT NULL, returned_new jsonb NOT NULL, depth integer NOT NULL,
    PRIMARY KEY(company_id,command_key,relation_name)
);
CREATE TABLE fixture_command_probe_witness (
    company_id uuid NOT NULL, command_key text NOT NULL, target text NOT NULL,
    target_oid bigint NOT NULL, target_name text NOT NULL, before_hits integer NOT NULL,
    after_hits integer NOT NULL, depth integer NOT NULL, selected_flush boolean NOT NULL,
    all_immediate boolean NOT NULL, pending jsonb NOT NULL, PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_command_probe_pending(company uuid, key text) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE; evidence_row workflow_action_evidence%ROWTYPE;
    result jsonb; authentic_evidence uuid; original_command jsonb; original_evidence jsonb;
BEGIN
    candidate := (SELECT command FROM workflow_action_evidence_commands AS command
        WHERE command.company_id=company AND command.command_key=key);
    original_command := (SELECT before_row.original_new FROM fixture_command_probe_before AS before_row
        WHERE before_row.company_id=company AND before_row.command_key=key AND before_row.relation_name='workflow_action_evidence_commands');
    original_evidence := (SELECT before_row.original_new FROM fixture_command_probe_before AS before_row
        WHERE before_row.company_id=company AND before_row.command_key=key AND before_row.relation_name='workflow_action_evidence');
    IF original_evidence IS NULL THEN
        -- Ordinary committed B/no-probe path follows its explicit unique company/key fact.
        authentic_evidence := (SELECT evidence.id FROM workflow_action_evidence AS evidence
            WHERE evidence.company_id=company AND evidence.command_key=key);
    ELSE authentic_evidence := (original_evidence->>'id')::uuid; END IF;
    evidence_row := (SELECT evidence FROM workflow_action_evidence AS evidence
        WHERE evidence.company_id=company AND evidence.id=authentic_evidence);
    IF candidate.id IS NULL OR evidence_row.id IS NULL THEN
        RAISE EXCEPTION 'positive probe missing exact candidate'; END IF;
    original_command := COALESCE(original_command,to_jsonb(candidate));
    original_evidence := COALESCE(original_evidence,to_jsonb(evidence_row));
    IF NOT (original_evidence @> (original_command-ARRAY['id','evidence_id','audit_sequence','expected_revision','result_revision',
        'outcome','scheduled_job_id','previous_state','previous_waiting_reason','created_at']))
        OR original_command->'id'<>original_evidence->'command_id' OR original_command->'evidence_id'<>original_evidence->'id'
    THEN RAISE EXCEPTION 'positive probe exact owner tuple mismatch'; END IF;
    SELECT jsonb_build_object('command',to_jsonb(candidate),'evidence',to_jsonb(evidence_row),
        'coverage',COALESCE((SELECT jsonb_agg(to_jsonb(coverage) ORDER BY coverage.remote_entry_id)
            FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=company AND coverage.evidence_id=evidence_row.id),'[]'::jsonb),
        'audit',(SELECT to_jsonb(audit) FROM workflow_run_events AS audit
            WHERE audit.company_id=company AND audit.run_id=candidate.run_id AND audit.sequence=candidate.audit_sequence),
        'marker',(SELECT to_jsonb(marker) FROM workflow_action_dispatches AS marker
            WHERE marker.company_id=company AND marker.id=candidate.dispatch_id),
        'entries',COALESCE((SELECT jsonb_agg(to_jsonb(entry) ORDER BY entry.id) FROM workflow_action_remote_entries AS entry
            WHERE entry.company_id=company AND entry.invocation_id=candidate.invocation_id),'[]'::jsonb),
        'ledger',(SELECT to_jsonb(ledger) FROM fixture_evidence_ledger AS ledger
            WHERE ledger.company_id=company AND ledger.invocation_id=candidate.invocation_id AND ledger.entry_id=evidence_row.applied_remote_entry_id),
        'effect',(SELECT to_jsonb(effect) FROM fixture_provider_effects AS effect WHERE effect.entry_id=evidence_row.applied_remote_entry_id),
        'head_revision',(SELECT run.revision FROM workflow_runs AS run WHERE run.company_id=company AND run.id=candidate.run_id),
        'receipts',COALESCE((SELECT jsonb_agg(to_jsonb(receipt)) FROM workflow_action_receipts AS receipt
            WHERE receipt.company_id=company AND receipt.invocation_id=candidate.invocation_id),'[]'::jsonb),
        'conflicts',COALESCE((SELECT jsonb_agg(to_jsonb(conflict)) FROM workflow_action_evidence_conflicts AS conflict
            WHERE conflict.company_id=company AND conflict.invocation_id=candidate.invocation_id),'[]'::jsonb)) INTO result;
    RETURN result;
END $$;

CREATE FUNCTION fixture_command_probe_before_insert() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_command_probe_config%ROWTYPE; original jsonb; returned jsonb;
BEGIN
    IF EXISTS(SELECT 1 FROM fixture_command_probe_config AS configured
        WHERE configured.company_id=NEW.company_id AND configured.command_key=NEW.command_key) THEN
        IF pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'positive probe unexpected BEFORE depth'; END IF;
        config := (SELECT configured FROM fixture_command_probe_config AS configured
            WHERE configured.company_id=NEW.company_id AND configured.command_key=NEW.command_key);
        original := to_jsonb(NEW);
        IF NOT (original @> (config.expected->CASE TG_TABLE_NAME WHEN 'workflow_action_evidence' THEN 'evidence' ELSE 'command' END))
            THEN RAISE EXCEPTION 'probe original owner identity mismatch'; END IF;
        IF TG_TABLE_NAME='workflow_action_evidence' AND config.probe_case IN ('orphan-command','foreign-command') THEN
            returned := jsonb_set(original,'{command_id}',to_jsonb(config.replacement));
        ELSIF TG_TABLE_NAME='workflow_action_evidence_commands' AND config.probe_case IN ('orphan-evidence','foreign-evidence') THEN
            returned := jsonb_set(original,'{evidence_id}',to_jsonb(config.replacement));
        ELSIF TG_TABLE_NAME='workflow_action_evidence_commands' AND config.probe_case IN ('evidence-actor','command-actor','mismatch-control') THEN
            returned := jsonb_set(original,'{actor_id}',to_jsonb(config.replacement));
        ELSE returned := original; END IF;
        NEW := jsonb_populate_record(NEW,returned);
        INSERT INTO fixture_command_probe_before(company_id,command_key,relation_name,original_new,returned_new,depth)
            VALUES(NEW.company_id,NEW.command_key,TG_TABLE_NAME,original,returned,pg_trigger_depth());
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_command_probe_evidence_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_command_probe_before_insert();
CREATE TRIGGER fixture_command_probe_command_before BEFORE INSERT ON workflow_action_evidence_commands
    FOR EACH ROW EXECUTE FUNCTION fixture_command_probe_before_insert();

CREATE FUNCTION fixture_command_probe_images(config fixture_command_probe_config, raw_pending jsonb) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE before_row fixture_command_probe_before%ROWTYPE; canonical jsonb := raw_pending;
    relation_key text; changed_relation text; changed_field text; normalized jsonb; hits integer := 0;
BEGIN
    CASE config.probe_case
        WHEN 'orphan-command','foreign-command' THEN changed_relation := 'evidence'; changed_field := 'command_id';
        WHEN 'orphan-evidence','foreign-evidence' THEN changed_relation := 'command'; changed_field := 'evidence_id';
        WHEN 'evidence-actor','command-actor','mismatch-control' THEN changed_relation := 'command'; changed_field := 'actor_id';
        WHEN 'positive','acceptance-control' THEN changed_relation := NULL;
        ELSE RAISE EXCEPTION 'probe unknown case';
    END CASE;
    FOR before_row IN SELECT observed.* FROM fixture_command_probe_before AS observed
        WHERE observed.company_id=config.company_id AND observed.command_key=config.command_key LOOP
        relation_key := CASE before_row.relation_name WHEN 'workflow_action_evidence' THEN 'evidence'
            WHEN 'workflow_action_evidence_commands' THEN 'command' ELSE NULL END;
        IF (relation_key IS NULL OR before_row.depth<>1 OR before_row.returned_new<>raw_pending->relation_key)
            IS DISTINCT FROM false THEN RAISE EXCEPTION 'probe raw pending/capture mismatch'; END IF;
        normalized := before_row.returned_new;
        IF relation_key=changed_relation THEN
            IF (before_row.original_new->changed_field=to_jsonb(config.replacement)
                OR before_row.returned_new->changed_field<>to_jsonb(config.replacement)) IS DISTINCT FROM false
                THEN RAISE EXCEPTION 'probe missing declared replacement'; END IF;
            normalized := jsonb_set(normalized,ARRAY[changed_field],before_row.original_new->changed_field);
        END IF;
        IF normalized<>before_row.original_new THEN RAISE EXCEPTION 'probe changed undeclared field'; END IF;
        canonical := jsonb_set(canonical,ARRAY[relation_key],before_row.original_new);
        hits := hits+1;
    END LOOP;
    IF hits<>2 THEN RAISE EXCEPTION 'probe capture count mismatch'; END IF;
    IF config.probe_case IN ('orphan-command','orphan-evidence') THEN
        IF EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command WHERE command.id=config.replacement)
            OR EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence WHERE evidence.id=config.replacement)
            THEN RAISE EXCEPTION 'probe replacement is not globally absent'; END IF;
    END IF;
    IF config.probe_case NOT IN ('positive','acceptance-control') THEN
        IF NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence
            JOIN workflow_action_evidence_commands AS command ON command.company_id=evidence.company_id AND command.id=evidence.command_id
            WHERE evidence.company_id=config.foreign_company AND evidence.id=config.foreign_evidence AND command.id=config.foreign_command
                AND command.actor_id=config.foreign_actor AND evidence.actor_id=config.foreign_actor)
            OR config.foreign_company=config.company_id
            OR canonical->'command'->>'id'=config.foreign_command::text
            OR canonical->'evidence'->>'id'=config.foreign_evidence::text
            OR canonical->'command'->>'actor_id'=config.foreign_actor::text
            THEN RAISE EXCEPTION 'probe foreign authentic identity mismatch'; END IF;
        IF (config.probe_case='foreign-command' AND config.replacement<>config.foreign_command)
            OR (config.probe_case='foreign-evidence' AND config.replacement<>config.foreign_evidence)
            OR (config.probe_case IN ('evidence-actor','command-actor','mismatch-control') AND config.replacement<>config.foreign_actor)
            THEN RAISE EXCEPTION 'probe wrong foreign replacement'; END IF;
    END IF;
    RETURN canonical;
END $$;

CREATE FUNCTION fixture_command_probe_validate(config fixture_command_probe_config, candidate workflow_action_evidence_commands, pending jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE scope jsonb;
BEGIN
    scope := (config.expected->'command') - ARRAY['actor_id','command_key','request_digest'];
    IF (NOT (pending->'command' @> (config.expected->'command'))
        OR NOT (pending->'evidence' @> (config.expected->'evidence'))
        OR jsonb_array_length(pending->'coverage')<>1 OR jsonb_array_length(pending->'entries')<>1
        OR NOT (pending->'coverage'->0 @> (scope || jsonb_build_object('evidence_id',(pending->'evidence'->>'id')::uuid,'remote_entry_id',config.expected->'entry')))
        OR NOT (pending->'entries'->0 @> (scope || jsonb_build_object('id',config.expected->'entry')))
        OR NOT (pending->'marker' @> ((scope-'dispatch_id') || jsonb_build_object('id',candidate.dispatch_id)))
        OR NOT (pending->'audit' @> jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,
            'execution_id',candidate.execution_id,'actor_id',(pending->'command'->>'actor_id')::uuid,'sequence',candidate.audit_sequence,'event_kind','action_reconciled'))
        OR NOT (pending->'ledger' @> jsonb_build_object('company_id',candidate.company_id,'invocation_id',candidate.invocation_id,
            'entry_id',config.expected->'entry','recover_result',false,'final_closed',false))
        OR pending->'receipts'<>'[]'::jsonb OR pending->'conflicts'<>'[]'::jsonb
        OR pending->'coverage'->0->'remote_entry_id'<>config.expected->'entry'
        OR pending->'entries'->0->'id'<>config.expected->'entry'
        OR pending->'marker'->'id'<>pending->'evidence'->'dispatch_id'
        OR pending->'audit'->'sequence'<>pending->'command'->'audit_sequence'
        OR pending->'audit'->>'event_kind'<>'action_reconciled'
        OR pending->'ledger'->'entry_id'<>config.expected->'entry'
        OR pending->'effect'->'entry_id'<>config.expected->'entry'
        OR pending->'effect'->'result'<>pending->'ledger'->'result'
        OR pending->'head_revision'<>pending->'command'->'result_revision'
        OR pending->'command'->'outcome'<>jsonb_build_object('kind','applied_recorded','receipt',false)) IS DISTINCT FROM false
    THEN RAISE EXCEPTION 'positive probe missing unchanged complete owner facts'; END IF;
END $$;

CREATE FUNCTION fixture_command_probe_diagnostic(config fixture_command_probe_config, state text, constraint_name text, message text, context text) RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE source_relation text; guard_message text;
BEGIN
    IF config.probe_case IN ('orphan-command','foreign-command','orphan-evidence','foreign-evidence') THEN
        source_relation := CASE WHEN config.probe_case IN ('orphan-command','foreign-command') THEN 'workflow_action_evidence'
            ELSE 'workflow_action_evidence_commands' END;
        RETURN COALESCE(state='23503' AND constraint_name=config.target_name
            AND message=format('insert or update on table "%s" violates foreign key constraint "%s"',source_relation,config.target_name)
            AND position(format('SET CONSTRAINTS public.%I IMMEDIATE',config.target_name) IN context)>0,false);
    END IF;
    guard_message := CASE WHEN config.probe_case IN ('evidence-actor','mismatch-control') THEN 'invalid workflow action evidence provenance'
        WHEN config.probe_case='command-actor' THEN 'workflow action command evidence scope mismatch' ELSE NULL END;
    RETURN COALESCE(state='23514' AND constraint_name='' AND message=guard_message
        AND position(config.target_name||'()' IN context)>0
        AND position(format('SET CONSTRAINTS public.%I IMMEDIATE',config.target_name) IN context)>0,false);
END $$;

CREATE FUNCTION fixture_command_probe_after_statement() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_command_probe_config%ROWTYPE; candidate workflow_action_evidence_commands%ROWTYPE;
    pending jsonb; raw_pending jsonb; before_hits integer; target_count integer; row_count integer;
    native_state text; native_constraint text; native_message text; native_context text;
    native_schema text; native_table text; envelope jsonb; accepted boolean; prefix text;
    changed_relation text; changed_field text;
BEGIN
    SELECT count(*) INTO row_count FROM inserted_commands AS inserted
        JOIN fixture_command_probe_config AS configured ON configured.company_id=inserted.company_id AND configured.command_key=inserted.command_key;
    IF row_count=0 THEN RETURN NULL; END IF;
    IF row_count<>1 OR pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'positive probe ambiguous candidate or unexpected depth'; END IF;
    candidate := (SELECT inserted FROM inserted_commands AS inserted
        JOIN fixture_command_probe_config AS configured ON configured.company_id=inserted.company_id AND configured.command_key=inserted.command_key);
    config := (SELECT configured FROM fixture_command_probe_config AS configured
        WHERE configured.company_id=candidate.company_id AND configured.command_key=candidate.command_key);
    SELECT count(*) INTO target_count FROM pg_constraint AS actual
        WHERE actual.oid=config.target_oid AND actual.conname=config.target_name AND actual.connamespace='public'::regnamespace
            AND actual.condeferrable AND actual.condeferred;
    IF target_count<>1 THEN RAISE EXCEPTION 'positive probe lost native target'; END IF;
    raw_pending := fixture_command_probe_pending(candidate.company_id,candidate.command_key);
    pending := fixture_command_probe_images(config,raw_pending);
    PERFORM fixture_command_probe_validate(config,candidate,pending);
    before_hits := 2;
    BEGIN
        EXECUTE format('SET CONSTRAINTS public.%I IMMEDIATE',config.target_name);
    EXCEPTION WHEN OTHERS THEN
        GET STACKED DIAGNOSTICS native_state=RETURNED_SQLSTATE,native_constraint=CONSTRAINT_NAME,
            native_message=MESSAGE_TEXT,native_context=PG_EXCEPTION_CONTEXT,native_schema=SCHEMA_NAME,native_table=TABLE_NAME;
    END;
    IF config.probe_case<>'positive' THEN
        changed_relation := CASE WHEN config.probe_case IN ('orphan-command','foreign-command') THEN 'evidence' ELSE 'command' END;
        changed_field := CASE WHEN config.probe_case IN ('orphan-command','foreign-command') THEN 'command_id'
            WHEN config.probe_case IN ('orphan-evidence','foreign-evidence') THEN 'evidence_id'
            WHEN config.probe_case='acceptance-control' THEN NULL ELSE 'actor_id' END;
        envelope := jsonb_build_object('version',1,'probe_case',config.probe_case,'company_id',candidate.company_id,
            'command_key',candidate.command_key,'target_oid',config.target_oid,'target_name',config.target_name,
            'before_hits',before_hits,'after_hits',1,'depth',pg_trigger_depth(),'owner_pid',pg_backend_pid(),
            'database',current_database(),'replacement',config.replacement,'original',pending,'returned',raw_pending,
            'changed_relation',changed_relation,'changed_field',changed_field,
            'old_value',pending->changed_relation->changed_field,'new_value',raw_pending->changed_relation->changed_field,
            'authentic_command',pending->'command'->'id','authentic_evidence',pending->'evidence'->'id',
            'returned_command',raw_pending->'command'->'id','returned_evidence_reference',raw_pending->'command'->'evidence_id',
            'images_checked',true,'source_checked',true,'selected_executed',true,'native_state',native_state,
            'native_constraint',native_constraint,'native_message',native_message,'native_context',native_context,
            'native_schema',native_schema,'native_table',native_table);
        IF octet_length(envelope::text)>65536 THEN RAISE EXCEPTION 'probe diagnostic envelope overflow'; END IF;
        accepted := fixture_command_probe_diagnostic(config,native_state,native_constraint,native_message,native_context);
        prefix := CASE WHEN native_state IS NULL THEN 'FIXTURE_COMMAND_PROBE_ACCEPTED_V1:'
            WHEN accepted THEN 'FIXTURE_COMMAND_PROBE_NATIVE_V1:' ELSE 'FIXTURE_COMMAND_PROBE_MISMATCH_V1:' END;
        RAISE EXCEPTION USING ERRCODE='P0001',MESSAGE=prefix||envelope::text;
    END IF;
    IF native_state IS NOT NULL THEN RAISE EXCEPTION 'positive native flush failed: % % % %',
        native_state,native_constraint,native_message,native_context; END IF;
    SET CONSTRAINTS ALL IMMEDIATE;
    INSERT INTO fixture_command_probe_witness(company_id,command_key,target,target_oid,target_name,
        before_hits,after_hits,depth,selected_flush,all_immediate,pending)
        VALUES(candidate.company_id,candidate.command_key,config.target,config.target_oid,config.target_name,
            before_hits,1,pg_trigger_depth(),true,true,pending);
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_command_probe_command_after AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_commands FOR EACH STATEMENT
    EXECUTE FUNCTION fixture_command_probe_after_statement();
