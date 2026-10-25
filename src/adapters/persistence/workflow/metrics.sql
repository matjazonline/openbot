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
    WHERE queue_kind = 'workflow' AND status IN ('pending','processing')
), steps AS (
    SELECT count(*) AS completed,
           COALESCE(percentile_cont(0.95) WITHIN GROUP
               (ORDER BY EXTRACT(EPOCH FROM completed_at - activated_at)), 0) AS latency
    FROM workflow_executions CROSS JOIN bounds WHERE completed_at >= bounds.since
), retries AS (
    SELECT count(*) AS count FROM task_attempts AS attempt
    JOIN background_tasks AS job ON job.id = attempt.task_id
    CROSS JOIN bounds
    WHERE job.queue_kind = 'workflow' AND attempt.attempt_number > 1 AND attempt.started_at >= bounds.since
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
    FROM model_usage_calls AS call
    LEFT JOIN model_prices AS price ON price.company_id = call.company_id
        AND price.provider = call.provider AND price.model = call.model
    CROSS JOIN bounds WHERE call.settled_at IS NULL OR call.requested_at >= bounds.since
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
    'charged_estimated_usd_last24h', usage.charged_usd, 'unpriced_model_calls', usage.unpriced
)
FROM bounds CROSS JOIN runs CROSS JOIN queue CROSS JOIN steps CROSS JOIN retries
CROSS JOIN decisions CROSS JOIN waits CROSS JOIN effects CROSS JOIN deliveries CROSS JOIN usage
