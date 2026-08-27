ALTER TABLE schedule_runs
    DROP CONSTRAINT schedule_runs_materialization_state_check,
    ADD CONSTRAINT schedule_runs_materialization_state_check CHECK (
        (materialization_status = 'pending'
         AND task_id IS NULL
         AND materialization_attempts < 5
         AND materialization_worker_id IS NULL
         AND materialization_generation IS NULL
         AND materialization_locked_at IS NULL
         AND materialization_lock_expires_at IS NULL)
        OR
        (materialization_status = 'materializing'
         AND task_id IS NULL
         AND materialization_attempts BETWEEN 1 AND 5
         AND materialization_worker_id IS NOT NULL
         AND materialization_generation IS NOT NULL
         AND materialization_locked_at IS NOT NULL
         AND materialization_lock_expires_at IS NOT NULL
         AND materialization_lock_expires_at > materialization_locked_at)
        OR
        (materialization_status = 'materialized'
         AND task_id IS NOT NULL
         AND materialization_worker_id IS NULL
         AND materialization_generation IS NULL
         AND materialization_locked_at IS NULL
         AND materialization_lock_expires_at IS NULL)
        OR
        (materialization_status = 'failed'
         AND task_id IS NULL
         AND materialization_attempts = 5
         AND materialization_worker_id IS NULL
         AND materialization_generation IS NULL
         AND materialization_locked_at IS NULL
         AND materialization_lock_expires_at IS NULL)
    );
