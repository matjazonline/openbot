-- Durable refusals cannot be reopened by operator retry, including direct SQL.
CREATE OR REPLACE FUNCTION workflow_control_retry_safe(company uuid, run uuid, job uuid)
RETURNS boolean LANGUAGE sql AS $$
    SELECT EXISTS (
        SELECT 1 FROM workflow_runs AS owner
        JOIN workflow_executions AS execution ON execution.company_id=owner.company_id AND execution.run_id=owner.id
        JOIN background_tasks AS task ON task.company_id=execution.company_id AND task.workflow_execution_id=execution.id
        JOIN task_attempts AS attempt ON attempt.task_id=task.id AND attempt.attempt_number=task.retry_count
        JOIN workflow_admissions AS admission ON admission.company_id=owner.company_id AND admission.run_id=owner.id
        WHERE owner.company_id=company AND owner.id=run AND task.id=job AND task.queue_kind='workflow'
            AND owner.deadline>clock_timestamp() AND execution.activation<=owner.max_steps
            AND execution.activated_at IS NOT NULL AND execution.completed_at IS NULL
            AND execution.committed_output IS NULL AND execution.committed_route IS NULL
            AND execution.successor_execution_id IS NULL
            AND task.retry_count>0 AND task.retry_count<task.max_retries
            AND attempt.status='failed' AND attempt.finished_at IS NOT NULL
            AND attempt.workflow_retry_safety='safe' AND attempt.workflow_retirement IN ('live','expired')
            AND attempt.workflow_failure_code NOT IN ('workflow.activation_limit','workflow.invalid_result','workflow.run_deadline')
            AND NOT EXISTS (SELECT 1 FROM workflow_budget_receipts AS receipt
                WHERE receipt.company_id=company AND receipt.run_id=run AND receipt.disposition='exhausted')
            AND (substring(admission.source_key FROM 4)::jsonb->>0)<>'child'
            AND NOT EXISTS (SELECT 1 FROM workflow_executions AS sibling
                JOIN background_tasks AS other ON other.company_id=sibling.company_id AND other.workflow_execution_id=sibling.id
                WHERE sibling.company_id=company AND sibling.run_id=run AND other.queue_kind='workflow'
                    AND other.id<>job AND other.status IN ('pending','processing'))
    )
$$;
