ALTER TABLE schedule_runs
    ADD COLUMN materialization_status TEXT NOT NULL DEFAULT 'pending',
    ADD COLUMN materialization_attempts INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN materialization_available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ADD COLUMN materialization_worker_id UUID,
    ADD COLUMN materialization_generation UUID,
    ADD COLUMN materialization_locked_at TIMESTAMPTZ,
    ADD COLUMN materialization_lock_expires_at TIMESTAMPTZ;

UPDATE schedule_runs
SET materialization_status = 'materialized'
WHERE task_id IS NOT NULL;

ALTER TABLE schedule_runs
    ADD CONSTRAINT schedule_runs_materialization_attempts_check
        CHECK (materialization_attempts >= 0 AND materialization_attempts <= 5),
    ADD CONSTRAINT schedule_runs_materialization_state_check CHECK (
        (materialization_status = 'pending'
         AND task_id IS NULL
         AND materialization_worker_id IS NULL
         AND materialization_generation IS NULL
         AND materialization_locked_at IS NULL
         AND materialization_lock_expires_at IS NULL)
        OR
        (materialization_status = 'materializing'
         AND task_id IS NULL
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

DROP INDEX schedule_runs_pending_idx;
CREATE INDEX schedule_runs_materialization_ready_idx
    ON schedule_runs (materialization_available_at, created_at, id)
    WHERE materialization_status = 'pending';
CREATE INDEX schedule_runs_materialization_expired_idx
    ON schedule_runs (materialization_lock_expires_at, created_at, id)
    WHERE materialization_status = 'materializing';
