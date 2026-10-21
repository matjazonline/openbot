-- Final-only child: capture the generated command before its sole declared mutation.
CREATE TABLE fixture_commit_command_initial (
    company_id uuid NOT NULL, command_key text NOT NULL,
    original_new jsonb NOT NULL, returned_new jsonb NOT NULL,
    PRIMARY KEY(company_id,command_key)
);
CREATE FUNCTION fixture_commit_final_before() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE config fixture_commit_config%ROWTYPE; original jsonb; generated bigint;
    evidence workflow_action_evidence%ROWTYPE;
BEGIN
    SELECT * INTO config FROM fixture_commit_config WHERE company_id=NEW.company_id AND command_key=NEW.command_key;
    IF NOT FOUND THEN RETURN NEW; END IF;
    PERFORM fixture_commit_require(pg_trigger_depth()=1 AND config.mode IN
        ('final_positive','final_revision','final_wrong_identity'),'Final initial command depth/mode');
    SELECT * INTO STRICT evidence FROM workflow_action_evidence WHERE company_id=NEW.company_id AND id=NEW.evidence_id;
    PERFORM fixture_commit_require(evidence.command_id=NEW.id AND evidence.command_key=NEW.command_key
        AND evidence.request_digest=NEW.request_digest AND evidence.actor_id=NEW.actor_id
        AND evidence.run_id=NEW.run_id AND evidence.execution_id=NEW.execution_id
        AND evidence.invocation_id=NEW.invocation_id AND evidence.argument_digest=NEW.argument_digest
        AND evidence.dispatch_id=NEW.dispatch_id,'Final original command evidence identity');
    SELECT revision INTO STRICT generated FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id;
    PERFORM fixture_commit_require(NEW.result_revision=generated AND generated>NEW.expected_revision
        AND NEW.expected_revision>0,'Final original generated revision gap');
    original := to_jsonb(NEW);
    IF config.mode='final_revision' THEN NEW.result_revision := NEW.expected_revision; END IF;
    PERFORM fixture_commit_require(original-'result_revision'=to_jsonb(NEW)-'result_revision','Final mutation fields');
    INSERT INTO fixture_commit_command_initial VALUES(NEW.company_id,NEW.command_key,original,to_jsonb(NEW));
    RETURN NEW;
END $$;
CREATE TRIGGER fixture_commit_final_before BEFORE INSERT ON workflow_action_evidence_commands
    FOR EACH ROW EXECUTE FUNCTION fixture_commit_final_before();

CREATE FUNCTION fixture_commit_final(config fixture_commit_config, candidate workflow_action_evidence_commands)
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE evidence workflow_action_evidence%ROWTYPE; initial fixture_commit_initial%ROWTYPE;
    command_initial fixture_commit_command_initial%ROWTYPE; audit workflow_run_events%ROWTYPE;
    owner workflow_runs%ROWTYPE; job background_tasks%ROWTYPE;
    old_run jsonb; old_job jsonb; proposed jsonb; additions jsonb; common jsonb;
    coverage jsonb; state_witness jsonb; schedule_witness jsonb; episode jsonb;
BEGIN
    PERFORM fixture_commit_require(to_jsonb(candidate) @> (config.expected->'command'),'Final candidate identity');
    SELECT * INTO STRICT evidence FROM workflow_action_evidence WHERE company_id=candidate.company_id AND id=candidate.evidence_id;
    SELECT * INTO STRICT initial FROM fixture_commit_initial WHERE company_id=candidate.company_id AND command_key=candidate.command_key;
    PERFORM fixture_commit_require(to_jsonb(evidence)=initial.returned_new,'stored initial evidence');
    PERFORM fixture_commit_require(initial.original_new=initial.returned_new,'Final unchanged initial evidence');
    PERFORM fixture_commit_require(evidence.command_id=candidate.id AND evidence.command_key=candidate.command_key
        AND evidence.request_digest=candidate.request_digest AND evidence.actor_id=candidate.actor_id
        AND evidence.coverage_digest=config.expected->>'coverage_digest','reciprocal evidence command');
    common := to_jsonb(candidate)-ARRAY['command_key','id','actor_id','request_digest','expected_revision',
        'result_revision','outcome','evidence_id','audit_sequence','scheduled_job_id','previous_state',
        'previous_waiting_reason','created_at'];
    coverage := jsonb_build_array(common || jsonb_build_object('evidence_id',evidence.id,
        'remote_entry_id',config.expected->'entry'));
    SELECT * INTO STRICT command_initial FROM fixture_commit_command_initial
        WHERE company_id=candidate.company_id AND command_key=candidate.command_key;
    PERFORM fixture_commit_require(to_jsonb(candidate)=command_initial.returned_new,'Final stored initial command');
    SELECT * INTO STRICT audit FROM workflow_run_events WHERE company_id=candidate.company_id
        AND run_id=candidate.run_id AND sequence=candidate.audit_sequence;
    PERFORM fixture_commit_require(to_jsonb(audit)=jsonb_build_object('company_id',candidate.company_id,
        'run_id',candidate.run_id,'sequence',candidate.audit_sequence,'actor_id',candidate.actor_id,
        'execution_id',candidate.execution_id,'event_kind','action_reconciled','created_at',transaction_timestamp(),
        'terminal_state',NULL,'terminal_execution_id',NULL,'decision_id',NULL,'wait_id',NULL),'exact actor audit');
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
    PERFORM fixture_commit_require((command_initial.original_new->>'result_revision')::bigint=owner.revision AND candidate.expected_revision=(old_run->>'revision')::bigint
        AND candidate.outcome='{"kind":"scheduled","receipt_only":false}'::jsonb
        AND candidate.previous_state='waiting' AND candidate.previous_waiting_reason='reconciliation'
        AND candidate.created_at>=evidence.created_at AND candidate.created_at<=clock_timestamp(),'scheduled complete result');
    IF config.mode='final_revision' THEN
        PERFORM fixture_commit_require(command_initial.original_new-'result_revision'=command_initial.returned_new-'result_revision'
            AND candidate.result_revision=candidate.expected_revision AND candidate.result_revision>0
            AND candidate.result_revision<owner.revision,'Final only stale revision changed');
    ELSE
        PERFORM fixture_commit_require(command_initial.original_new=command_initial.returned_new
            AND candidate.result_revision=owner.revision,'Final unchanged generated revision');
    END IF;
    state_witness := jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,
        'transaction_id',pg_current_xact_id()::text,'initial_state','waiting','initial_waiting_reason','reconciliation');
    schedule_witness := jsonb_build_object('company_id',candidate.company_id,'run_id',candidate.run_id,
        'execution_id',candidate.execution_id,'job_id',job.id,'retired_attempt',job.retry_count,'transaction_id',pg_current_xact_id()::text);
    episode := schedule_witness-'transaction_id' || jsonb_build_object('command_key',candidate.command_key);
    additions := jsonb_build_object('workflow_action_evidence',jsonb_build_array(to_jsonb(evidence)),
        'workflow_action_evidence_coverage',coverage,'fixture_commit_command_initial',jsonb_build_array(to_jsonb(command_initial)),
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

