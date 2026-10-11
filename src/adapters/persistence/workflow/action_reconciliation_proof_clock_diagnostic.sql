WITH observed AS MATERIALIZED (SELECT clock_timestamp() AS observed_at)
SELECT jsonb_build_object(
    'observed_at', observed.observed_at,
    'requested', jsonb_build_object('worker', $5::uuid, 'generation', $6::uuid, 'attempt', $7::integer),
    'job', (
        SELECT jsonb_build_object(
            'status', job.status, 'worker', job.worker_id,
            'generation', job.execution_generation, 'retry_count', job.retry_count,
            'expires_at', job.lock_expires_at,
            'lease_live', job.lock_expires_at > observed.observed_at
        )
        FROM background_tasks AS job WHERE job.company_id = $1 AND job.id = $4
    ),
    'run', (
        SELECT jsonb_build_object(
            'state', run.state, 'deadline', run.deadline, 'revision', run.revision,
            'deadline_live', run.deadline > observed.observed_at
        )
        FROM workflow_runs AS run WHERE run.company_id = $1 AND run.id = $2
    ),
    'execution', (
        SELECT jsonb_build_object('completed_at', execution.completed_at)
        FROM workflow_executions AS execution
        WHERE execution.company_id = $1 AND execution.run_id = $2 AND execution.id = $3
    ),
    'attempt', (
        SELECT jsonb_build_object(
            'id', attempt.id, 'status', attempt.status, 'worker', attempt.worker_id,
            'generation', attempt.execution_generation, 'attempt', attempt.attempt_number
        )
        FROM task_attempts AS attempt WHERE attempt.task_id = $4 AND attempt.attempt_number = $7
    ),
    'proofs', (
        SELECT COALESCE(jsonb_agg(proof.facts ORDER BY proof.id), '[]'::jsonb)
        FROM (
            SELECT evidence.id, jsonb_build_object(
                'id', evidence.id, 'disposition', evidence.disposition,
                'valid_until', evidence.valid_until,
                'expired', evidence.valid_until <= observed.observed_at,
                'grant_eligible', evidence.grant_eligible
            ) AS facts
            FROM workflow_action_evidence AS evidence
            WHERE evidence.company_id = $1 AND evidence.run_id = $2
              AND evidence.execution_id = $3 AND evidence.invocation_id = $8
            ORDER BY evidence.id LIMIT 4
        ) AS proof
    ),
    'consumptions', (
        SELECT count(*) FROM workflow_action_evidence_consumptions AS consumption
        WHERE consumption.company_id = $1 AND consumption.run_id = $2
          AND consumption.execution_id = $3 AND consumption.invocation_id = $8
    ),
    'entries', (
        SELECT count(*) FROM workflow_action_remote_entries AS entry
        WHERE entry.company_id = $1 AND entry.run_id = $2
          AND entry.execution_id = $3 AND entry.invocation_id = $8
    ),
    'conflicts', (
        SELECT count(*) FROM workflow_action_evidence_conflicts AS conflict
        WHERE conflict.company_id = $1 AND conflict.run_id = $2
          AND conflict.execution_id = $3 AND conflict.invocation_id = $8
    ),
    'receipts', (
        SELECT count(*) FROM workflow_action_receipts AS receipt
        WHERE receipt.company_id = $1 AND receipt.run_id = $2
          AND receipt.execution_id = $3 AND receipt.invocation_id = $8
    )
)
FROM observed
