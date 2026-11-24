WITH bounds AS (
    SELECT transaction_timestamp() AS now, transaction_timestamp() - INTERVAL '24 hours' AS since
), runs AS (
    SELECT count(*) FILTER (WHERE state IN ('queued','running')) AS active,
           count(*) FILTER (WHERE state = 'waiting') AS waiting,
           count(*) FILTER (WHERE deadline <= bounds.now) AS expired,
           count(*) FILTER (WHERE waiting_reason = 'reconciliation') AS reconciliation
    FROM workflow_runs CROSS JOIN bounds
    WHERE state IN ('queued','running','waiting')
), queue AS (
    SELECT count(*) FILTER (WHERE status = 'pending' AND run_at <= bounds.now) AS ready,
           COALESCE(max(EXTRACT(EPOCH FROM bounds.now - run_at))
               FILTER (WHERE status = 'pending' AND run_at <= bounds.now), 0) AS age,
           count(*) FILTER (WHERE status = 'processing' AND lock_expires_at <= bounds.now) AS expired
    FROM background_tasks CROSS JOIN bounds
    WHERE status IN ('pending','processing')
), steps AS (
    SELECT count(*) AS completed,
           COALESCE(percentile_cont(0.95) WITHIN GROUP
               (ORDER BY EXTRACT(EPOCH FROM completed_at - activated_at)), 0) AS latency
    FROM workflow_executions CROSS JOIN bounds WHERE completed_at >= bounds.since
), retries AS (
    SELECT count(*) AS count FROM task_attempts AS attempt
    JOIN background_tasks AS job ON job.id = attempt.task_id
    CROSS JOIN bounds
    WHERE attempt.attempt_number > 1 AND attempt.started_at >= bounds.since
), decisions AS (
    SELECT count(*) AS open,
           COALESCE(max(EXTRACT(EPOCH FROM bounds.now - decision.created_at)), 0) AS age
    FROM workflow_decisions AS decision
    JOIN workflow_waits AS wait ON wait.company_id = decision.company_id AND wait.id = decision.wait_id
    CROSS JOIN bounds WHERE wait.state = 'waiting'
), waits AS (
    SELECT count(*) AS expired FROM workflow_waits CROSS JOIN bounds
    WHERE state = 'waiting' AND deadline <= bounds.now
), effects AS (
    SELECT count(*) AS unknown FROM workflow_action_effect_states WHERE effect_state IN ('needs_reconciliation', 'applied_without_result')
), deliveries AS (
    SELECT count(*) FILTER (WHERE status = 'outcome_unknown') AS unknown,
           count(*) FILTER (WHERE status = 'delivered' AND delivered_at >= bounds.since) AS delivered,
           count(*) FILTER (WHERE status = 'dead_letter' AND updated_at >= bounds.since) AS dead
    FROM message_deliveries CROSS JOIN bounds
    WHERE status = 'outcome_unknown' OR delivered_at >= bounds.since OR updated_at >= bounds.since
), usage AS (
    SELECT COALESCE(sum(call.reserved_input + call.reserved_output)
               FILTER (WHERE call.settled_at IS NULL), 0) AS reserved,
           COALESCE(sum(COALESCE(call.input_tokens, call.reserved_input)
                      + COALESCE(call.output_tokens, call.reserved_output))
               FILTER (WHERE call.requested_at >= bounds.since), 0) AS charged,
           COALESCE(sum((call.reserved_input::numeric * price.input_micro_usd_per_million
                        + call.reserved_output::numeric * price.output_micro_usd_per_million) / 1e12)
               FILTER (WHERE call.settled_at IS NULL), 0) AS reserved_usd,
           COALESCE(sum((COALESCE(call.input_tokens, call.reserved_input)::numeric * price.input_micro_usd_per_million
                        + COALESCE(call.output_tokens, call.reserved_output)::numeric * price.output_micro_usd_per_million) / 1e12)
               FILTER (WHERE call.requested_at >= bounds.since), 0) AS charged_usd,
           count(*) FILTER (WHERE price.company_id IS NULL) AS unpriced
    FROM (
        SELECT company_id,provider,model,requested_at,reserved_input,reserved_output,input_tokens,output_tokens,settled_at FROM model_usage_calls
        UNION ALL
        SELECT company_id,provider,model,requested_at,reserved_input,reserved_output,input_tokens,output_tokens,settled_at FROM model_usage_history
    ) AS call
    LEFT JOIN model_prices AS price ON price.company_id = call.company_id
        AND price.provider = call.provider AND price.model = call.model
    CROSS JOIN bounds WHERE call.settled_at IS NULL OR call.requested_at >= bounds.since
), call_facts AS (
    -- Calls of unfinished caller executions only (an open call wait implies one): bounded by
    -- open work. Phases as in gate AWC-P05-D2 r2, D1 (`channel_call_views::call_phase`).
    SELECT call.created_at, wait.state AS wait_state, wait.deadline, child.state AS child_state,
           caller.state AS caller_state, result.call_id IS NOT NULL AS stored,
           EXISTS(SELECT 1 FROM workflow_waits AS review
                    JOIN workflow_decisions AS decision ON decision.company_id = review.company_id AND decision.wait_id = review.id
                   WHERE review.company_id = child.company_id AND review.run_id = child.id AND review.state = 'waiting'
                     AND decision.reply_proposal_id IS NOT NULL) AS final_review,
           EXISTS(SELECT 1 FROM workflow_executions AS step JOIN background_tasks AS job
                    ON job.company_id = step.company_id AND job.workflow_execution_id = step.id
                   WHERE step.company_id = child.company_id AND step.run_id = child.id AND step.completed_at IS NULL
                     AND ((job.status = 'processing' AND job.lock_expires_at <= bounds.now)
                       OR (job.status = 'pending' AND job.run_at <= bounds.now - INTERVAL '300 seconds'))) AS child_stalled,
           EXISTS(SELECT 1 FROM background_tasks AS job
                   WHERE job.company_id = call.company_id AND job.workflow_execution_id = call.execution_id
                     AND ((job.status = 'processing' AND job.lock_expires_at <= bounds.now)
                       OR (job.status = 'pending' AND job.run_at <= bounds.now - INTERVAL '300 seconds'))) AS caller_stalled,
           EXISTS(SELECT 1 FROM task_harness_runs AS checkpoint
                    CROSS JOIN LATERAL jsonb_array_elements(COALESCE(checkpoint.checkpoint->'invocations', '[]'::jsonb)) AS saved(invocation)
                   WHERE checkpoint.company_id = call.company_id AND checkpoint.workflow_run_id = call.run_id
                     AND checkpoint.workflow_execution_id = call.execution_id AND saved.invocation->>'state' = 'completed'
                     AND saved.invocation->'call'->>'invocation_id' = call.call_id::text) AS consumed
    FROM workflow_executions AS execution
    JOIN workflow_channel_calls AS call ON call.company_id = execution.company_id
         AND call.run_id = execution.run_id AND call.execution_id = execution.id
    JOIN workflow_runs AS child ON child.company_id = call.company_id AND child.id = call.child_run_id
    JOIN workflow_runs AS caller ON caller.company_id = call.company_id AND caller.id = call.run_id
    CROSS JOIN LATERAL (SELECT parked.state, parked.deadline FROM workflow_waits AS parked
         WHERE parked.company_id = call.company_id AND parked.run_id = call.run_id
           AND parked.execution_id = call.execution_id AND parked.child_tool_call_id = call.call_id
         ORDER BY parked.created_at DESC, parked.id DESC LIMIT 1) AS wait
    LEFT JOIN workflow_channel_call_results AS result ON result.company_id = call.company_id
         AND result.run_id = call.run_id AND result.execution_id = call.execution_id AND result.call_id = call.call_id
    CROSS JOIN bounds
    WHERE execution.completed_at IS NULL
), call_phases AS (
    SELECT facts.*,
           NOT stored AND wait_state = 'waiting' AS child_side,
           NOT stored AND wait_state = 'completed' AND NOT consumed
               AND caller_state NOT IN ('succeeded', 'failed', 'cancelled') AS caller_side
    FROM call_facts AS facts
), calls AS (
    SELECT count(*) FILTER (WHERE wait_state = 'waiting') AS open,
           count(*) FILTER (WHERE child_side AND child_state = 'waiting' AND final_review) AS review,
           count(*) FILTER (WHERE (child_side OR caller_side) AND child_state = 'succeeded') AS ready,
           count(*) FILTER (WHERE child_side AND child_state IN ('queued', 'running') AND child_stalled) AS child_stalled,
           count(*) FILTER (WHERE caller_side AND caller_stalled) AS caller_stalled,
           count(*) FILTER (WHERE wait_state = 'waiting' AND deadline <= bounds.now) AS past_deadline,
           COALESCE(max(EXTRACT(EPOCH FROM bounds.now - created_at)) FILTER (WHERE child_side OR caller_side), 0) AS age
    FROM call_phases CROSS JOIN bounds
)
SELECT jsonb_build_object(
    'sampled_at', EXTRACT(EPOCH FROM bounds.now),
    'ready_jobs', queue.ready, 'oldest_ready_seconds', queue.age,
    'active_runs', runs.active, 'waiting_runs', runs.waiting,
    'expired_leases', queue.expired, 'expired_run_deadlines', runs.expired,
    'expired_wait_deadlines', waits.expired, 'retries_last24h', retries.count,
    'completed_steps_last24h', steps.completed, 'step_duration_p95_seconds_last24h', steps.latency,
    'open_decisions', decisions.open, 'oldest_decision_seconds', decisions.age,
    'reconciliation_runs', runs.reconciliation, 'unknown_actions', effects.unknown,
    'unknown_deliveries', deliveries.unknown, 'delivered_last24h', deliveries.delivered,
    'dead_deliveries_last24h', deliveries.dead, 'reserved_tokens', usage.reserved,
    'charged_tokens_last24h', usage.charged, 'reserved_estimated_usd', usage.reserved_usd,
    'charged_estimated_usd_last24h', usage.charged_usd, 'unpriced_model_calls', usage.unpriced,
    'channel_calls_open', calls.open, 'channel_calls_review_waiting', calls.review,
    'channel_calls_response_ready', calls.ready, 'channel_calls_child_stalled', calls.child_stalled,
    'channel_calls_caller_stalled', calls.caller_stalled, 'channel_calls_past_deadline', calls.past_deadline,
    'channel_call_oldest_seconds', calls.age
)
FROM bounds CROSS JOIN runs CROSS JOIN queue CROSS JOIN steps CROSS JOIN retries
CROSS JOIN decisions CROSS JOIN waits CROSS JOIN effects CROSS JOIN deliveries CROSS JOIN usage
CROSS JOIN calls
