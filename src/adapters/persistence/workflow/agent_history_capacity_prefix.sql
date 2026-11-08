-- OwnDatabase-only historical load prefix. Keep real FKs/checks and actual production limits.
-- USER lifecycle triggers are restored before any admission/cleanup assertion.
CREATE TEMP TABLE ah_history_prefix (id uuid PRIMARY KEY,job_id uuid NOT NULL,position bigint NOT NULL) ON COMMIT DROP;
INSERT INTO ah_history_prefix SELECT gen_random_uuid(),gen_random_uuid(),position FROM generate_series(2,$6::bigint) AS position;
INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation,created_at,activated_at,frozen_inputs,
    completed_at,committed_output,response_disposition,frozen_choice,committed_route,route_target,successor_execution_id)
SELECT execution.company_id,execution.run_id,prefix.id,execution.step_id,execution.activation+prefix.position-1,
    execution.created_at,execution.activated_at,execution.frozen_inputs,NULL,NULL,'automatic',NULL,NULL,NULL,NULL
FROM workflow_executions AS execution CROSS JOIN ah_history_prefix AS prefix
WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.id=$3;
INSERT INTO background_tasks(id,company_id,channel_id,thread_id,correlation_id,status,retry_count,continuation_count,
    max_retries,worker_id,execution_generation,locked_at,lock_expires_at,run_at,created_at,updated_at,workflow_execution_id)
SELECT prefix.job_id,job.company_id,job.channel_id,job.thread_id,job.correlation_id,'failed',0,0,
    job.max_retries,NULL,NULL,NULL,NULL,job.run_at,job.created_at,job.updated_at,prefix.id
FROM background_tasks AS job CROSS JOIN ah_history_prefix AS prefix WHERE job.company_id=$1 AND job.workflow_execution_id=$3 AND job.id=$4;
INSERT INTO agent_history_exchanges(company_id,id,conversation_id,agent_id,revision_id,manifest_id,run_id,execution_id,job_id,
    position,input_digest,provenance,access_level,state,output,frozen_preparation,payload_retained,payload_expired_at,
    preparation_digest,created_at,activated_at,finished_at)
SELECT exchange.company_id,prefix.id,exchange.conversation_id,exchange.agent_id,exchange.revision_id,exchange.manifest_id,
    exchange.run_id,prefix.id,prefix.job_id,prefix.position,exchange.input_digest,exchange.provenance,exchange.access_level,
    'cancelled',NULL,CASE WHEN $8='retained' THEN jsonb_set($7::jsonb,'{history_exchange}',to_jsonb(prefix.id::text)) ELSE NULL END,
    $8='retained',CASE WHEN $8='expired' THEN clock_timestamp() ELSE NULL END,
    sha256(convert_to(jsonb_set($7::jsonb,'{history_exchange}',to_jsonb(prefix.id::text))::text,'UTF8')),
    exchange.created_at,exchange.activated_at,exchange.finished_at
FROM agent_history_exchanges AS exchange CROSS JOIN ah_history_prefix AS prefix WHERE exchange.company_id=$1 AND exchange.run_id=$2 AND exchange.id=$3;
UPDATE agent_history_conversations AS conversation SET last_exchange_order=$6,
    expired_count=$6-(SELECT count(*) FROM agent_history_exchanges AS exchange
      WHERE exchange.company_id=$1 AND exchange.conversation_id=$5 AND exchange.payload_retained),
    expired_through_order=COALESCE((SELECT min(position)-1 FROM agent_history_exchanges AS exchange
      WHERE exchange.company_id=$1 AND exchange.conversation_id=$5 AND exchange.payload_retained),$6)
WHERE conversation.company_id=$1 AND conversation.id=$5;
