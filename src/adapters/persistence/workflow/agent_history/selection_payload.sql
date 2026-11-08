SELECT execution.frozen_inputs->'context' AS input, exchange.output
FROM agent_history_exchanges AS exchange
JOIN workflow_executions AS execution ON execution.company_id=exchange.company_id
    AND execution.run_id=exchange.run_id AND execution.id=exchange.execution_id
WHERE exchange.company_id=$1 AND exchange.id=$2 AND exchange.state='completed' AND exchange.payload_retained
    AND octet_length((execution.frozen_inputs->'context')::text)=$3
    AND octet_length(exchange.output::text)=$4
    AND $3::bigint+$4::bigint<=$5
