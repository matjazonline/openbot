CREATE TABLE fixture_reopen_config (
    company_id uuid NOT NULL,command_key text NOT NULL,probe_case text NOT NULL,target_oid bigint NOT NULL,target_name text NOT NULL,
    expected jsonb NOT NULL,first_execution uuid NOT NULL,first_job uuid NOT NULL,other_actor uuid NOT NULL,history jsonb NOT NULL,baseline jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE TABLE fixture_reopen_capture (
    company_id uuid NOT NULL,command_key text NOT NULL,relation_name text NOT NULL,
    original_new jsonb NOT NULL,returned_new jsonb NOT NULL,depth integer NOT NULL,PRIMARY KEY(company_id,command_key,relation_name)
);
CREATE TABLE fixture_reopen_witness (
    company_id uuid NOT NULL,command_key text NOT NULL,selected_flush boolean NOT NULL,all_immediate boolean NOT NULL,
    owner_pid integer NOT NULL,pending jsonb NOT NULL,PRIMARY KEY(company_id,command_key)
);
CREATE FUNCTION fixture_reopen_pending(company uuid,key text) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE candidate workflow_action_evidence_commands%ROWTYPE;
BEGIN
    candidate := (SELECT command FROM workflow_action_evidence_commands AS command WHERE command.company_id=company AND command.command_key=key);
    IF candidate.id IS NULL THEN RAISE EXCEPTION 'Final missing actual command'; END IF;
    RETURN jsonb_build_object('command',to_jsonb(candidate),
        'evidence',(SELECT to_jsonb(evidence) FROM workflow_action_evidence AS evidence WHERE evidence.company_id=company AND evidence.id=candidate.evidence_id),
        'audit',(SELECT to_jsonb(audit) FROM workflow_run_events AS audit WHERE audit.company_id=company AND audit.run_id=candidate.run_id AND audit.sequence=candidate.audit_sequence),
        'run',(SELECT to_jsonb(owner) FROM workflow_runs AS owner WHERE owner.company_id=company AND owner.id=candidate.run_id),
        'job',(SELECT to_jsonb(job) FROM background_tasks AS job WHERE job.company_id=company AND job.id=candidate.scheduled_job_id),
        'marker',(SELECT to_jsonb(marker) FROM workflow_action_dispatches AS marker WHERE marker.company_id=company AND marker.id=candidate.dispatch_id),
        'episode',(SELECT to_jsonb(episode) FROM workflow_action_claim_episodes AS episode WHERE episode.company_id=company AND episode.command_key=key),
        'coverage',COALESCE((SELECT jsonb_agg(to_jsonb(coverage) ORDER BY coverage.remote_entry_id) FROM workflow_action_evidence_coverage AS coverage WHERE coverage.company_id=company AND coverage.evidence_id=candidate.evidence_id),'[]'::jsonb),
        'entries',COALESCE((SELECT jsonb_agg(to_jsonb(entry) ORDER BY entry.id) FROM workflow_action_remote_entries AS entry WHERE entry.company_id=company AND entry.invocation_id=candidate.invocation_id),'[]'::jsonb),
        'ledger',COALESCE((SELECT jsonb_agg(to_jsonb(ledger) ORDER BY ledger.entry_id) FROM fixture_evidence_ledger AS ledger WHERE ledger.company_id=company AND ledger.invocation_id=candidate.invocation_id),'[]'::jsonb),
        'operation',(SELECT to_jsonb(operation) FROM fixture_provider_operations AS operation WHERE operation.invocation_id=candidate.invocation_id),
        'effects',COALESCE((SELECT jsonb_agg(to_jsonb(effect) ORDER BY effect.entry_id) FROM fixture_provider_effects AS effect),'[]'::jsonb),
        'receipts',COALESCE((SELECT jsonb_agg(to_jsonb(receipt) ORDER BY receipt.invocation_id) FROM workflow_action_receipts AS receipt WHERE receipt.company_id=company AND receipt.invocation_id=candidate.invocation_id),'[]'::jsonb),
        'conflicts',COALESCE((SELECT jsonb_agg(to_jsonb(conflict) ORDER BY conflict.id) FROM workflow_action_evidence_conflicts AS conflict WHERE conflict.company_id=company AND conflict.invocation_id=candidate.invocation_id),'[]'::jsonb));
END $$;
CREATE FUNCTION fixture_reopen_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_reopen_config%ROWTYPE; original jsonb; matches integer;
BEGIN
    IF TG_TABLE_NAME='workflow_run_events' THEN
        IF NEW.event_kind<>'action_reconciled' THEN RETURN NEW; END IF;
        SELECT count(*) INTO matches FROM fixture_reopen_config AS configured WHERE configured.company_id=NEW.company_id
            AND configured.expected->'command'->>'run_id'=NEW.run_id::text
            AND configured.expected->'command'->>'execution_id'=NEW.execution_id::text AND configured.expected->'command'->>'actor_id'=NEW.actor_id::text;
        IF matches=0 THEN RETURN NEW; END IF;
        IF matches<>1 THEN RAISE EXCEPTION 'Final ambiguous event operation'; END IF;
        config := (SELECT configured FROM fixture_reopen_config AS configured WHERE configured.company_id=NEW.company_id
            AND configured.expected->'command'->>'run_id'=NEW.run_id::text
            AND configured.expected->'command'->>'execution_id'=NEW.execution_id::text AND configured.expected->'command'->>'actor_id'=NEW.actor_id::text);
        original := to_jsonb(NEW);
        IF config.probe_case IN ('actor','mismatch-control') THEN NEW.actor_id:=config.other_actor;
        ELSIF config.probe_case='execution' THEN NEW.execution_id:=config.first_execution; END IF;
    ELSE
        config := (SELECT configured FROM fixture_reopen_config AS configured WHERE configured.company_id=NEW.company_id AND configured.command_key=NEW.command_key);
        IF config.company_id IS NULL THEN RETURN NEW; END IF;
        original := to_jsonb(NEW);
        IF NOT (original @> (config.expected->CASE TG_TABLE_NAME WHEN 'workflow_action_evidence' THEN 'evidence' ELSE 'command' END))
            THEN RAISE EXCEPTION 'Final original owner identity mismatch'; END IF;
        IF TG_TABLE_NAME='workflow_action_evidence_commands' AND config.probe_case='historical-revision' THEN
            NEW.result_revision:=(config.history->'command'->>'result_revision')::bigint;
        END IF;
    END IF;
    IF pg_trigger_depth()<>1 THEN RAISE EXCEPTION 'Final BEFORE depth'; END IF;
    INSERT INTO fixture_reopen_capture(company_id,command_key,relation_name,original_new,returned_new,depth)
        VALUES(config.company_id,config.command_key,TG_TABLE_NAME,original,to_jsonb(NEW),pg_trigger_depth());
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_reopen_event_before BEFORE INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION fixture_reopen_before();
CREATE TRIGGER fixture_reopen_evidence_before BEFORE INSERT ON workflow_action_evidence FOR EACH ROW EXECUTE FUNCTION fixture_reopen_before();
CREATE TRIGGER fixture_reopen_command_before BEFORE INSERT ON workflow_action_evidence_commands FOR EACH ROW EXECUTE FUNCTION fixture_reopen_before();
CREATE FUNCTION fixture_reopen_images(config fixture_reopen_config,raw_pending jsonb) RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE capture fixture_reopen_capture%ROWTYPE; canonical jsonb:=raw_pending; relation_key text; changed_relation text; changed_field text; replacement jsonb; normalized jsonb; hits integer:=0;
BEGIN
    CASE config.probe_case
        WHEN 'actor','mismatch-control' THEN changed_relation:='audit';changed_field:='actor_id';replacement:=to_jsonb(config.other_actor);
        WHEN 'execution' THEN changed_relation:='audit';changed_field:='execution_id';replacement:=to_jsonb(config.first_execution);
        WHEN 'historical-revision' THEN changed_relation:='command';changed_field:='result_revision';replacement:=config.history->'command'->'result_revision';
        WHEN 'positive','acceptance-control' THEN changed_relation:=NULL;
        ELSE RAISE EXCEPTION 'Final unknown case';
    END CASE;
    FOR capture IN SELECT observed.company_id,observed.command_key,observed.relation_name,observed.original_new,observed.returned_new,observed.depth
        FROM fixture_reopen_capture AS observed WHERE observed.company_id=config.company_id AND observed.command_key=config.command_key LOOP
        relation_key:=CASE capture.relation_name WHEN 'workflow_run_events' THEN 'audit' WHEN 'workflow_action_evidence' THEN 'evidence' WHEN 'workflow_action_evidence_commands' THEN 'command' ELSE NULL END;
        IF (relation_key IS NULL OR capture.depth<>1 OR capture.returned_new<>raw_pending->relation_key) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Final raw image mismatch'; END IF;
        normalized:=capture.returned_new;
        IF relation_key=changed_relation THEN
            IF (capture.original_new->changed_field=replacement OR capture.returned_new->changed_field<>replacement) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Final declared change absent'; END IF;
            -- Assertion normalization only; never write protected rows back.
            normalized:=jsonb_set(normalized,ARRAY[changed_field],capture.original_new->changed_field);
        END IF;
        IF normalized<>capture.original_new THEN RAISE EXCEPTION 'Final undeclared NEW change'; END IF;
        canonical:=jsonb_set(canonical,ARRAY[relation_key],capture.original_new);hits:=hits+1;
    END LOOP;
    IF hits<>3 THEN RAISE EXCEPTION 'Final requires exactly three depth-one captures'; END IF;
    RETURN canonical;
END $$;
