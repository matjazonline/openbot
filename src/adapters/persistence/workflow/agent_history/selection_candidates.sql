SELECT exchange.id, exchange.position, exchange.access_level::smallint AS access_level, exchange.content_exact,
       octet_length((execution.frozen_inputs->'context')::text) AS input_bytes,
       octet_length(exchange.output::text) AS output_bytes
FROM agent_history_exchanges AS exchange
JOIN workflow_executions AS execution ON execution.company_id=exchange.company_id
    AND execution.run_id=exchange.run_id AND execution.id=exchange.execution_id
WHERE exchange.company_id=$1 AND exchange.conversation_id=$2 AND exchange.state='completed' AND exchange.payload_retained
    AND exchange.position<$3 AND exchange.access_level<=$4
ORDER BY exchange.position DESC
LIMIT $5
