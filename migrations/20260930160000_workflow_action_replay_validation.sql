-- Correct historical scheduling validation without changing immutable action facts.
CREATE OR REPLACE FUNCTION workflow_action_replay_supported(company uuid, invocation uuid)
RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE marker workflow_action_dispatches%ROWTYPE;
    operation jsonb; subject jsonb; proof jsonb; guarantee jsonb;
    seconds numeric; nanos numeric; revision numeric; horizon timestamptz;
BEGIN
    SELECT * INTO marker FROM workflow_action_dispatches
        WHERE company_id=company AND invocation_id=invocation;
    IF NOT FOUND OR marker.effect_kind<>'remote' OR marker.replay_subject IS NULL THEN RETURN false; END IF;
    SELECT intent.operation, owner.deadline INTO operation,horizon
        FROM workflow_action_intents AS intent
        JOIN workflow_runs AS owner ON owner.company_id=intent.company_id AND owner.id=intent.run_id
        WHERE intent.company_id=company AND intent.id=invocation;
    subject := convert_from(marker.replay_subject,'UTF8')::jsonb;
    proof := marker.current_policy->'provider_replay';
    guarantee := proof->'guarantee';
    IF subject IS DISTINCT FROM jsonb_build_object('tool',operation->'contract','target',operation->'target')
        OR operation->'contract'->>'company_id' IS DISTINCT FROM company::text
        OR marker.current_policy->'tool' IS DISTINCT FROM operation->'contract'
        OR marker.current_policy->'approval_required' IS DISTINCT FROM 'false'::jsonb
        OR jsonb_typeof(proof) IS DISTINCT FROM 'object'
        OR proof->'version' IS DISTINCT FROM '1'::jsonb
        OR jsonb_typeof(proof->'registration') IS DISTINCT FROM 'string'
        OR octet_length(proof->>'registration') > 128
        OR (proof->>'registration') !~ '^[A-Za-z_][A-Za-z0-9_-]*(\.[A-Za-z_][A-Za-z0-9_-]*)*$'
        OR jsonb_typeof(proof->'operation') IS DISTINCT FROM 'string'
        OR proof->>'operation' IS DISTINCT FROM encode(sha256(marker.replay_subject),'hex')
        OR proof - ARRAY['version','registration','operation','guarantee'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee) IS DISTINCT FROM 'object'
        OR jsonb_typeof(operation->'contract'->'policy'->'policy_revision') IS DISTINCT FROM 'number'
    THEN RETURN false; END IF;
    revision := (operation->'contract'->'policy'->>'policy_revision')::numeric;
    IF revision <> trunc(revision) OR revision <= 0 OR revision > 18446744073709551615
    THEN RETURN false; END IF;
    IF operation->'contract'->'policy'->>'recovery'='safe_repeat' THEN
        RETURN guarantee = '{"mode":"safe_repeat"}'::jsonb;
    END IF;
    IF operation->'contract'->'policy'->>'recovery' IS DISTINCT FROM 'provider_idempotency'
        OR guarantee->>'mode' IS DISTINCT FROM 'provider_idempotency'
        OR guarantee - ARRAY['mode','retention'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee->'retention') IS DISTINCT FROM 'object'
        OR (guarantee->'retention') - ARRAY['secs','nanos'] <> '{}'::jsonb
        OR jsonb_typeof(guarantee->'retention'->'secs') IS DISTINCT FROM 'number'
        OR jsonb_typeof(guarantee->'retention'->'nanos') IS DISTINCT FROM 'number'
    THEN RETURN false; END IF;
    seconds := (guarantee->'retention'->>'secs')::numeric;
    nanos := (guarantee->'retention'->>'nanos')::numeric;
    IF seconds <> trunc(seconds) OR nanos <> trunc(nanos) OR seconds < 0
        OR nanos < 0 OR nanos >= 1000000000 OR seconds+nanos/1000000000 <= 0
        OR seconds+nanos/1000000000 > 31536000 THEN RETURN false; END IF;
    -- Round down sub-microsecond guarantees, conservatively. Both first creation
    -- and the run horizon are authoritative DB timestamps, immune to lock waits.
    RETURN horizon>clock_timestamp() AND marker.created_at
        + make_interval(secs => (seconds+floor(nanos/1000)/1000000)::double precision) >= horizon;
EXCEPTION WHEN OTHERS THEN RETURN false;
END $$;

