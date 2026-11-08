CREATE FUNCTION public.agent_history_selected_pair_matches(
    owner public.agent_history_exchanges, metadata jsonb, pair jsonb, ceiling public.access_level)
RETURNS boolean LANGUAGE sql VOLATILE AS $$
    SELECT (metadata-ARRAY['id','position','access_level']::text[])='{}'::jsonb
        AND (pair-ARRAY['input','output']::text[])='{}'::jsonb
        AND EXISTS (
            SELECT 1 FROM public.agent_history_exchanges AS prior
            JOIN public.workflow_executions AS execution ON execution.company_id=prior.company_id
                AND execution.run_id=prior.run_id AND execution.id=prior.execution_id
            WHERE prior.company_id=owner.company_id AND prior.conversation_id=owner.conversation_id
                AND prior.id=(metadata->>'id')::uuid AND prior.position=(metadata->>'position')::bigint
                AND prior.position<owner.position AND prior.state='completed' AND prior.access_level<=ceiling
                AND prior.access_level=(metadata->>'access_level')::smallint
                AND pair->'input'=execution.frozen_inputs->'context' AND pair->'output'=prior.output)
$$;

CREATE FUNCTION public.agent_history_preparation_valid(owner public.agent_history_exchanges)
RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE document jsonb:=owner.frozen_preparation;
        history jsonb:=document->'history';
        source record;
        selected jsonb;
        pair jsonb;
        highest smallint:=0;
        previous_position bigint:=0;
        ordinal integer;
        eligible bigint;
BEGIN
    IF jsonb_typeof(document) IS DISTINCT FROM 'object'
        OR jsonb_typeof(history) IS DISTINCT FROM 'object'
        OR jsonb_typeof(history->'selected') IS DISTINCT FROM 'array'
        OR jsonb_typeof(history->'seed') IS DISTINCT FROM 'array'
        OR octet_length(document::text)>2097152 OR octet_length((history->'seed')::text)>65536
        OR jsonb_array_length(history->'selected')>16
        OR jsonb_array_length(history->'selected')<>jsonb_array_length(history->'seed')
        OR (history-ARRAY['selected','seed','omitted_eligible','selected_level','content_level','budget']::text[])<>'{}'::jsonb
        OR owner.state<>'queued' OR owner.checkpoint_id IS NOT NULL
    THEN RETURN false; END IF;
    -- UPDATE already owns this exchange: never wait backwards against a source owner.
    SELECT run.knowledge_ceiling,run.content_level,execution.frozen_inputs INTO STRICT source
        FROM public.workflow_runs AS run JOIN public.workflow_executions AS execution
          ON execution.company_id=run.company_id AND execution.run_id=run.id
        JOIN public.background_tasks AS job ON job.company_id=run.company_id AND job.id=owner.job_id
          AND job.workflow_execution_id=execution.id
        WHERE run.company_id=owner.company_id AND run.id=owner.run_id AND execution.id=owner.execution_id
          AND run.state IN ('queued','running') AND run.deadline>clock_timestamp()
          AND job.status='processing' AND job.lock_expires_at>clock_timestamp()
          AND execution.completed_at IS NULL FOR SHARE OF run NOWAIT;
    PERFORM 1 FROM public.agent_history_conversations WHERE company_id=owner.company_id
        AND id=owner.conversation_id AND active_exchange_id=owner.id FOR UPDATE;
    IF NOT FOUND OR document->>'version' IS DISTINCT FROM '5'
        OR document->>'history_exchange' IS DISTINCT FROM owner.id::text
        OR document->'context' IS DISTINCT FROM source.frozen_inputs->'context'
        OR document->'output_schema' IS DISTINCT FROM owner.provenance->'output_contract'->'schema'
        OR document->'selection'->>'mode' IS DISTINCT FROM 'tools'
        OR document->'selection'->>'agent' IS DISTINCT FROM owner.agent_id::text
        OR document->'selection'->>'revision' IS DISTINCT FROM owner.revision_id::text
        OR document->'selection'->>'manifest' IS DISTINCT FROM owner.manifest_id::text
        OR (history->>'content_level')::smallint IS DISTINCT FROM source.content_level
    THEN RETURN false; END IF;
    FOR ordinal IN 0..jsonb_array_length(history->'selected')-1 LOOP
        selected:=history->'selected'->ordinal;
        pair:=history->'seed'->ordinal;
        IF jsonb_typeof(selected) IS DISTINCT FROM 'object' OR jsonb_typeof(pair) IS DISTINCT FROM 'object'
            OR (selected->>'position')::bigint<=previous_position
            OR public.agent_history_selected_pair_matches(owner,selected,pair,source.knowledge_ceiling) IS DISTINCT FROM true
        THEN RETURN false; END IF;
        previous_position:=(selected->>'position')::bigint;
        highest:=GREATEST(highest,(selected->>'access_level')::smallint);
    END LOOP;
    SELECT count(*) INTO eligible FROM public.agent_history_exchanges
        WHERE company_id=owner.company_id AND conversation_id=owner.conversation_id AND state='completed'
            AND position<owner.position AND access_level<=source.knowledge_ceiling;
    RETURN (history->>'selected_level')::smallint=highest AND source.content_level>=highest
        AND (history->>'omitted_eligible')::bigint=eligible-jsonb_array_length(history->'selected')
        AND jsonb_typeof(history->'budget')='object'
        AND ((history->'budget')-ARRAY['mandatory_tokens','history_tokens','reserved_output_tokens','request_limit']::text[])='{}'::jsonb
        AND (history->'budget'->>'request_limit')::bigint=131072
        AND (history->'budget'->>'mandatory_tokens')::bigint>0
        AND (history->'budget'->>'history_tokens')::bigint>=0
        AND (history->'budget'->>'reserved_output_tokens')::bigint BETWEEN 1 AND 16384
        AND (history->'budget'->>'mandatory_tokens')::numeric
            +(history->'budget'->>'history_tokens')::numeric
            +(history->'budget'->>'reserved_output_tokens')::numeric<=131072;
END $$;

