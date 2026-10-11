-- C uses a genuine consumed proof and a new unattributed receipt conflict.
CREATE FUNCTION fixture_commit_conflict(config fixture_commit_config, candidate workflow_action_evidence_commands)
RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE evidence workflow_action_evidence%ROWTYPE; initial fixture_commit_initial%ROWTYPE;
    receipt workflow_action_receipts%ROWTYPE; audit workflow_run_events%ROWTYPE;
    owner workflow_runs%ROWTYPE; job background_tasks%ROWTYPE;
    old_run jsonb; old_job jsonb; proposed jsonb; additions jsonb; common jsonb;
    coverage jsonb; conflict workflow_action_evidence_conflicts%ROWTYPE; older workflow_action_remote_entries%ROWTYPE; newer workflow_action_remote_entries%ROWTYPE;
BEGIN
    PERFORM fixture_commit_require(to_jsonb(candidate) @> (config.expected->'command'),'candidate identity');
    SELECT * INTO STRICT evidence FROM workflow_action_evidence WHERE company_id=candidate.company_id AND id=candidate.evidence_id;
    SELECT * INTO STRICT initial FROM fixture_commit_initial WHERE company_id=candidate.company_id AND command_key=candidate.command_key;
    PERFORM fixture_commit_require(to_jsonb(evidence)=initial.returned_new,'stored initial evidence');
    IF config.mode='conflict_chronology' THEN
        PERFORM fixture_commit_require(initial.original_new-'observed_at'=initial.returned_new-'observed_at'
            AND initial.original_new->'observed_at'<>initial.returned_new->'observed_at','only chronology changed');
    ELSE
        PERFORM fixture_commit_require(initial.original_new=initial.returned_new,'unchanged initial evidence');
    END IF;
    PERFORM fixture_commit_require(evidence.command_id=candidate.id AND evidence.command_key=candidate.command_key
        AND evidence.request_digest=candidate.request_digest AND evidence.actor_id=candidate.actor_id
        AND evidence.coverage_digest=config.expected->>'coverage_digest','reciprocal evidence command');
    SELECT * INTO STRICT older FROM workflow_action_remote_entries WHERE company_id=candidate.company_id AND id=(config.expected->>'old_entry')::uuid;
    SELECT * INTO STRICT newer FROM workflow_action_remote_entries WHERE company_id=candidate.company_id AND id=(config.expected->>'entry')::uuid;
    PERFORM fixture_commit_require(older.created_at<newer.created_at AND older.created_at<=evidence.observed_at,'older covered entry chronology');
    IF config.mode='conflict_chronology' THEN
        PERFORM fixture_commit_require(evidence.observed_at=newer.created_at-interval '1 microsecond','exact new entry chronology defect');
    END IF;
    common := to_jsonb(candidate)-ARRAY['command_key','id','actor_id','request_digest','expected_revision',
        'result_revision','outcome','evidence_id','audit_sequence','scheduled_job_id','previous_state',
        'previous_waiting_reason','created_at'];
    coverage := jsonb_build_array(common || jsonb_build_object('evidence_id',evidence.id,
        'remote_entry_id',config.expected->'entry'), common || jsonb_build_object('evidence_id',evidence.id,
        'remote_entry_id',config.expected->'old_entry'));
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
        'terminal_state',NULL,'terminal_execution_id',NULL),'exact actor audit');
    PERFORM fixture_commit_require(candidate.audit_sequence=1+COALESCE((SELECT max((value->>'sequence')::bigint)
        FROM jsonb_array_elements(config.baseline->'workflow_run_events') WHERE value->>'run_id'=candidate.run_id::text),0),'next audit sequence');
    SELECT * INTO STRICT conflict FROM workflow_action_evidence_conflicts WHERE company_id=candidate.company_id AND contradictory_evidence_id=evidence.id;
    PERFORM fixture_commit_require(to_jsonb(conflict)-ARRAY['id','created_at']=common || jsonb_build_object(
        'evidence_id',config.expected->'proof','contradictory_evidence_id',evidence.id,
        'remote_entry_id',NULL,'actual_observation_id',NULL,'reason','unattributed_applied'), 'exact new conflict');
    PERFORM fixture_commit_require(conflict.id IS NOT NULL AND conflict.created_at>=receipt.created_at
        AND conflict.created_at<=clock_timestamp(),'new conflict identity timestamp');
    PERFORM fixture_commit_require(config.baseline->'workflow_action_evidence_conflicts'='[]'::jsonb
        AND config.baseline->'workflow_action_receipts'='[]'::jsonb,'no prior conflict or receipt');
    SELECT * INTO STRICT owner FROM workflow_runs WHERE company_id=candidate.company_id AND id=candidate.run_id;
    SELECT * INTO STRICT job FROM background_tasks WHERE company_id=candidate.company_id AND id=(config.expected->>'job')::uuid;
    SELECT value INTO STRICT old_run FROM jsonb_array_elements(config.baseline->'workflow_runs') WHERE value->>'id'=candidate.run_id::text;
    SELECT value INTO STRICT old_job FROM jsonb_array_elements(config.baseline->'background_tasks') WHERE value->>'id'=job.id::text;
    PERFORM fixture_commit_require(old_run->>'state'='waiting' AND old_run->>'waiting_reason'='reconciliation'
        AND old_job->>'status'='failed' AND (old_job->>'retry_count')::integer=2,'parked consumed owner');
    PERFORM fixture_commit_require(to_jsonb(owner)-'revision'=old_run-'revision'
        AND owner.revision=(old_run->>'revision')::bigint+2,'only evidence and conflict generated revision');
    PERFORM fixture_commit_require(to_jsonb(job)=old_job,'no job reopen or debit');
    PERFORM fixture_commit_require(candidate.result_revision=owner.revision AND candidate.expected_revision=(old_run->>'revision')::bigint
        AND candidate.outcome='{"kind":"evidence_conflict"}'::jsonb AND candidate.scheduled_job_id IS NULL
        AND candidate.previous_state='waiting' AND candidate.previous_waiting_reason='reconciliation'
        AND candidate.created_at>=evidence.created_at AND candidate.created_at<=clock_timestamp(),'conflict complete result');
    additions := jsonb_build_object('workflow_action_evidence',jsonb_build_array(to_jsonb(evidence)),
        'workflow_action_evidence_coverage',coverage,'workflow_action_receipts',jsonb_build_array(to_jsonb(receipt)),
        'workflow_run_events',jsonb_build_array(to_jsonb(audit)),'workflow_action_evidence_commands',jsonb_build_array(to_jsonb(candidate)),
        'workflow_action_evidence_conflicts',jsonb_build_array(to_jsonb(conflict)));
    proposed := fixture_commit_public();
    PERFORM fixture_commit_delta(config,proposed,additions,old_run,to_jsonb(owner),old_job,to_jsonb(job));
    PERFORM fixture_commit_require(proposed->'fixture_commit_initial'=jsonb_build_array(to_jsonb(initial))
        AND proposed->'fixture_commit_config'=jsonb_build_array(to_jsonb(config))
        AND proposed->'fixture_commit_observed'='[]'::jsonb,'exact fixture state');
    RETURN proposed;
END $$;

