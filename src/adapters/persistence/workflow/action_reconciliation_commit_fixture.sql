-- OwnDatabase-only observation fixture. No native guard is called or replaced.
CREATE TABLE fixture_commit_config (
    company_id uuid NOT NULL, command_key text NOT NULL, mode text NOT NULL
        CHECK(mode IN ('positive','chronology','wrong_identity','final_positive','final_revision','final_wrong_identity','conflict_positive','conflict_chronology','conflict_wrong_identity')),
    gate_class integer NOT NULL, gate_key integer NOT NULL,
    expected jsonb NOT NULL, baseline jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_commit_initial (
    company_id uuid NOT NULL, command_key text NOT NULL,
    original_new jsonb NOT NULL, returned_new jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_commit_observed (
    company_id uuid NOT NULL, command_key text NOT NULL,
    owner_pid integer NOT NULL, database_name text NOT NULL,
    proposed jsonb NOT NULL, PRIMARY KEY(company_id,command_key)
);

CREATE FUNCTION fixture_commit_require(valid boolean, diagnostic text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    IF valid IS DISTINCT FROM TRUE THEN
        RAISE EXCEPTION 'row3e observer: %', diagnostic;
    END IF;
END $$;

CREATE FUNCTION fixture_commit_public() RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE relation record; rows jsonb; result jsonb := '{}'::jsonb;
BEGIN
    FOR relation IN SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY tablename LOOP
        EXECUTE format('SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text),''[]''::jsonb) FROM public.%I AS r',relation.tablename) INTO rows;
        result := result || jsonb_build_object(relation.tablename,rows);
    END LOOP;
    RETURN result;
END $$;

CREATE FUNCTION fixture_commit_sorted(rows jsonb) RETURNS jsonb LANGUAGE sql AS $$
    SELECT COALESCE(jsonb_agg(value ORDER BY value::text),'[]'::jsonb) FROM jsonb_array_elements(rows)
$$;

CREATE FUNCTION fixture_commit_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_commit_config%ROWTYPE; original jsonb; entry_time timestamptz; marker_time timestamptz;
BEGIN
    SELECT * INTO config FROM fixture_commit_config WHERE company_id=NEW.company_id AND command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    PERFORM fixture_commit_require(pg_trigger_depth()=1,'initial depth');
    original := to_jsonb(NEW);
    PERFORM fixture_commit_require(original @> (config.expected->'evidence'),'authentic initial evidence');
    PERFORM fixture_commit_require(NEW.diagnostic-'attestation'='{"result":null}'::jsonb
        AND (NEW.diagnostic->'attestation')-ARRAY['observed_at','verified_at','valid_until']=config.expected#>'{evidence,diagnostic,attestation}'
        AND (NEW.diagnostic#>>'{attestation,observed_at}')::timestamptz=NEW.observed_at
        -- SQLx encodes chrono timestamps with num_microseconds() (truncation),
        -- whereas a PostgreSQL text timestamp cast rounds excess fractional digits.
        AND regexp_replace(NEW.diagnostic#>>'{attestation,verified_at}',
            '([.][0-9]{6})[0-9]+', '\1')::timestamptz=NEW.verified_at
        AND (NEW.diagnostic#>>'{attestation,valid_until}')::timestamptz=NEW.valid_until,'exact issued envelope');
    SELECT created_at INTO STRICT entry_time FROM workflow_action_remote_entries
        WHERE company_id=NEW.company_id AND id=(config.expected->>'entry')::uuid
          AND run_id=NEW.run_id AND execution_id=NEW.execution_id AND invocation_id=NEW.invocation_id
          AND argument_digest=NEW.argument_digest AND dispatch_id=NEW.dispatch_id;
    SELECT created_at INTO STRICT marker_time FROM workflow_action_dispatches
        WHERE company_id=NEW.company_id AND id=NEW.dispatch_id;
    PERFORM fixture_commit_require(NEW.observed_at>=entry_time AND NEW.verified_at>=NEW.observed_at
        AND NEW.created_at>=NEW.verified_at AND NEW.valid_until>clock_timestamp(),'issued chronology');
    IF config.mode IN ('chronology','conflict_chronology') THEN
        NEW.observed_at := entry_time-interval '1 microsecond';
        PERFORM fixture_commit_require(marker_time<=NEW.observed_at AND NEW.observed_at<entry_time
            AND NEW.observed_at<=NEW.verified_at,'strict marker entry gap');
    END IF;
    INSERT INTO fixture_commit_initial VALUES(NEW.company_id,NEW.command_key,original,to_jsonb(NEW));
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_commit_before BEFORE INSERT ON workflow_action_evidence
    FOR EACH ROW EXECUTE FUNCTION fixture_commit_before();

-- This reconstructs all allowed table deltas. Every other public table must be exact.
-- Fixture metadata has its own exact assertions; rollback equality excludes NOTHING.
CREATE FUNCTION fixture_commit_delta(config fixture_commit_config, proposed jsonb, additions jsonb,
    old_run jsonb, new_run jsonb, old_job jsonb, new_job jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE relation record; expected_rows jsonb; unchanged jsonb;
BEGIN
    FOR relation IN SELECT key,value FROM jsonb_each(config.baseline) LOOP
        IF relation.key IN ('fixture_commit_config','fixture_commit_initial','fixture_commit_observed') THEN CONTINUE; END IF;
        expected_rows := relation.value;
        IF relation.key='workflow_runs' THEN
            SELECT COALESCE(jsonb_agg(value),'[]'::jsonb) INTO unchanged
                FROM jsonb_array_elements(expected_rows) WHERE value IS DISTINCT FROM old_run;
            expected_rows := unchanged || jsonb_build_array(new_run);
        ELSIF relation.key='background_tasks' THEN
            SELECT COALESCE(jsonb_agg(value),'[]'::jsonb) INTO unchanged
                FROM jsonb_array_elements(expected_rows) WHERE value IS DISTINCT FROM old_job;
            expected_rows := unchanged || jsonb_build_array(new_job);
        ELSE
            expected_rows := expected_rows || COALESCE(additions->relation.key,'[]'::jsonb);
        END IF;
        PERFORM fixture_commit_require(proposed->relation.key=fixture_commit_sorted(expected_rows),
            'full proposed delta ' || relation.key);
    END LOOP;
    PERFORM fixture_commit_require((SELECT count(*) FROM jsonb_object_keys(config.baseline))=
        (SELECT count(*) FROM jsonb_object_keys(proposed)),'public table inventory');
END $$;

CREATE FUNCTION fixture_commit_applied(config fixture_commit_config, candidate workflow_action_evidence_commands)
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE evidence workflow_action_evidence%ROWTYPE; initial fixture_commit_initial%ROWTYPE;
    receipt workflow_action_receipts%ROWTYPE; audit workflow_run_events%ROWTYPE;
    owner workflow_runs%ROWTYPE; job background_tasks%ROWTYPE;
    old_run jsonb; old_job jsonb; proposed jsonb; additions jsonb; common jsonb;
    coverage jsonb; state_witness jsonb; schedule_witness jsonb; episode jsonb;
BEGIN
    PERFORM fixture_commit_require(to_jsonb(candidate) @> (config.expected->'command'),'candidate identity');
    SELECT * INTO STRICT evidence FROM workflow_action_evidence WHERE company_id=candidate.company_id AND id=candidate.evidence_id;
    SELECT * INTO STRICT initial FROM fixture_commit_initial WHERE company_id=candidate.company_id AND command_key=candidate.command_key;
    PERFORM fixture_commit_require(to_jsonb(evidence)=initial.returned_new,'stored initial evidence');
    IF config.mode='chronology' THEN
        PERFORM fixture_commit_require(initial.original_new-'observed_at'=initial.returned_new-'observed_at'
            AND initial.original_new->'observed_at'<>initial.returned_new->'observed_at','only chronology changed');
    ELSE
        PERFORM fixture_commit_require(initial.original_new=initial.returned_new,'unchanged initial evidence');
    END IF;
    PERFORM fixture_commit_require(evidence.command_id=candidate.id AND evidence.command_key=candidate.command_key
        AND evidence.request_digest=candidate.request_digest AND evidence.actor_id=candidate.actor_id
        AND evidence.coverage_digest=config.expected->>'coverage_digest','reciprocal evidence command');
    common := to_jsonb(candidate)-ARRAY['command_key','id','actor_id','request_digest','expected_revision',
        'result_revision','outcome','evidence_id','audit_sequence','scheduled_job_id','previous_state',
        'previous_waiting_reason','created_at'];
    coverage := jsonb_build_array(common || jsonb_build_object('evidence_id',evidence.id,
        'remote_entry_id',config.expected->'entry'));
    SELECT * INTO STRICT receipt FROM workflow_action_receipts WHERE company_id=candidate.company_id AND invocation_id=candidate.invocation_id;
    PERFORM fixture_commit_require(to_jsonb(receipt)-'created_at'=common || jsonb_build_object(
        'effect_kind','remote','remote_entry_id',NULL,'reconciliation_evidence_id',evidence.id,
        'result',config.expected->'result'),'exact recovered receipt');
    PERFORM fixture_commit_require(receipt.created_at>=evidence.created_at AND receipt.created_at<=clock_timestamp(),'receipt timestamp');
    SELECT * INTO STRICT audit FROM workflow_run_events WHERE company_id=candidate.company_id
        AND run_id=candidate.run_id AND sequence=candidate.audit_sequence;
    PERFORM fixture_commit_require(to_jsonb(audit)=jsonb_build_object('company_id',candidate.company_id,
        'run_id',candidate.run_id,'sequence',candidate.audit_sequence,'actor_id',candidate.actor_id,
        'execution_id',candidate.execution_id,'event_kind','action_reconciled','created_at',transaction_timestamp(),
        'terminal_state',NULL,'terminal_execution_id',NULL,'decision_id',NULL),'exact actor audit');
    PERFORM fixture_commit_require(candidate.audit_sequence=1+COALESCE((SELECT max((value->>'sequence')::bigint)
        FROM jsonb_array_elements(config.baseline->'workflow_run_events') WHERE value->>'run_id'=candidate.run_id::text),0),'next audit sequence');
    SELECT * INTO STRICT owner FROM workflow_runs WHERE company_id=candidate.company_id AND id=candidate.run_id;
    SELECT * INTO STRICT job FROM background_tasks WHERE company_id=candidate.company_id AND id=candidate.scheduled_job_id;
    SELECT value INTO STRICT old_run FROM jsonb_array_elements(config.baseline->'workflow_runs') WHERE value->>'id'=candidate.run_id::text;
    SELECT value INTO STRICT old_job FROM jsonb_array_elements(config.baseline->'background_tasks') WHERE value->>'id'=candidate.scheduled_job_id::text;
    PERFORM fixture_commit_require(old_run->>'state'='waiting' AND old_run->>'waiting_reason'='reconciliation'
        AND old_job->>'status'='failed' AND (old_job->>'retry_count')::integer>0,'parked existing owner');
    PERFORM fixture_commit_require(to_jsonb(owner)-ARRAY['state','waiting_reason','revision']=old_run-ARRAY['state','waiting_reason','revision']
        AND owner.state='running' AND owner.waiting_reason IS NULL AND owner.revision>(old_run->>'revision')::bigint,'only run reopen and generated revision');
    PERFORM fixture_commit_require(to_jsonb(job)-ARRAY['status','run_at','updated_at']=old_job-ARRAY['status','run_at','updated_at']
        AND job.status='pending' AND job.run_at>=evidence.created_at AND job.updated_at>=job.run_at
        AND job.updated_at<=clock_timestamp(),'same job reopened without debit');
    PERFORM fixture_commit_require(candidate.result_revision=owner.revision AND candidate.expected_revision=(old_run->>'revision')::bigint
        AND candidate.outcome='{"kind":"scheduled","receipt_only":true}'::jsonb
        AND candidate.previous_state='waiting' AND candidate.previous_waiting_reason='reconciliation'
        AND candidate.created_at>=evidence.created_at AND candidate.created_at<=clock_timestamp(),'scheduled complete result');
    state_witness := jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,
        'transaction_id',pg_current_xact_id()::text,'initial_state','waiting','initial_waiting_reason','reconciliation');
    schedule_witness := jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,
        'execution_id',candidate.execution_id,'job_id',job.id,'retired_attempt',job.retry_count,'transaction_id',pg_current_xact_id()::text);
    episode := schedule_witness-'transaction_id' || jsonb_build_object('command_key',candidate.command_key);
    additions := jsonb_build_object('workflow_action_evidence',jsonb_build_array(to_jsonb(evidence)),
        'workflow_action_evidence_coverage',coverage,'workflow_action_receipts',jsonb_build_array(to_jsonb(receipt)),
        'workflow_run_events',jsonb_build_array(to_jsonb(audit)),'workflow_action_evidence_commands',jsonb_build_array(to_jsonb(candidate)),
        'workflow_action_state_witnesses',jsonb_build_array(state_witness),'workflow_action_schedule_witnesses',jsonb_build_array(schedule_witness),
        'workflow_action_claim_episodes',jsonb_build_array(episode));
    proposed := fixture_commit_public();
    PERFORM fixture_commit_delta(config,proposed,additions,old_run,to_jsonb(owner),old_job,to_jsonb(job));
    PERFORM fixture_commit_require(proposed->'fixture_commit_initial'=jsonb_build_array(to_jsonb(initial))
        AND proposed->'fixture_commit_config'=jsonb_build_array(to_jsonb(config))
        AND proposed->'fixture_commit_observed'='[]'::jsonb,'exact fixture state');
    RETURN proposed;
END $$;

CREATE FUNCTION fixture_commit_statement() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_commit_config%ROWTYPE; candidate workflow_action_evidence_commands%ROWTYPE; proposed jsonb;
BEGIN
    IF NOT EXISTS(SELECT 1 FROM inserted_command AS command JOIN fixture_commit_config AS configured
        USING(company_id,command_key)) THEN RETURN NULL; END IF;
    PERFORM fixture_commit_require(pg_trigger_depth()=1 AND (SELECT count(*) FROM inserted_command)=1,'single statement owner');
    SELECT command.* INTO STRICT candidate FROM inserted_command AS command;
    SELECT * INTO STRICT config FROM fixture_commit_config WHERE company_id=candidate.company_id AND command_key=candidate.command_key;
    IF config.mode IN ('final_positive','final_revision','final_wrong_identity') THEN
        proposed := fixture_commit_final(config,candidate);
    ELSIF config.mode IN ('conflict_positive','conflict_chronology','conflict_wrong_identity') THEN
        proposed := fixture_commit_conflict(config,candidate);
    ELSE
        proposed := fixture_commit_applied(config,candidate);
    END IF;
    INSERT INTO fixture_commit_observed VALUES(candidate.company_id,candidate.command_key,pg_backend_pid(),current_database(),proposed);
    -- Last operation: seeing this exact waiter proves every assertion above passed.
    PERFORM pg_advisory_xact_lock(config.gate_class,config.gate_key);
    RETURN NULL;
END $$;
CREATE TRIGGER fixture_commit_statement AFTER INSERT ON workflow_action_evidence_commands
    REFERENCING NEW TABLE AS inserted_command FOR EACH STATEMENT EXECUTE FUNCTION fixture_commit_statement();
