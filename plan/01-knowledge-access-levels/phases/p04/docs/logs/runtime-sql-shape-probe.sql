PREPARE knowledge_shape(uuid,text,smallint,text[]) AS WITH exact_terms AS (
    SELECT phraseto_tsquery('simple', term) AS query
      FROM unnest($4::text[]) AS terms(term)
), exact_query AS (
    SELECT string_agg('(' || query::text || ')', ' | ')
        FILTER (WHERE query::text <> '')::tsquery AS query FROM exact_terms
)
SELECT chunk.id, document.name AS document_name, chunk.heading, chunk.body,
       chunk.access_level::smallint AS access_level, chunk.embedding,
       CASE WHEN exact_query.query IS NOT NULL AND chunk.search_text @@ exact_query.query
            THEN ts_rank_cd(chunk.search_text, exact_query.query) END AS keyword_rank
  FROM knowledge_chunks AS chunk
  JOIN knowledge_documents AS document
    ON (document.company_id, document.id, document.access_level)
       = (chunk.company_id, chunk.document_id, chunk.access_level)
 CROSS JOIN exact_query
 WHERE chunk.company_id=$1 AND chunk.embedding_model=$2 AND chunk.access_level<=$3
 ORDER BY chunk.id
 LIMIT 8193;
EXECUTE knowledge_shape('00000000-0000-0000-0000-000000000000','test-model',0,ARRAY['AX-4410']);
DEALLOCATE knowledge_shape;
PREPARE frozen_shape(uuid,uuid,uuid) AS SELECT execution.step_id,execution.frozen_inputs,run.knowledge_ceiling::smallint AS ceiling
FROM workflow_executions AS execution JOIN workflow_runs AS run
ON run.company_id=execution.company_id AND run.id=execution.run_id
WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.id=$3;
EXECUTE frozen_shape('00000000-0000-0000-0000-000000000000','00000000-0000-0000-0000-000000000000','00000000-0000-0000-0000-000000000000');
DEALLOCATE frozen_shape;
