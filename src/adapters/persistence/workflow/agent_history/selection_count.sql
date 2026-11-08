SELECT count(*)
FROM agent_history_exchanges
WHERE company_id=$1 AND conversation_id=$2 AND state='completed' AND payload_retained
    AND position<$3 AND access_level<=$4
