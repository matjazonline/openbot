-- Provisioning creation and readiness polling are separate durable phases. The legacy
-- attempts column remains for compatibility with the previous application version, but new
-- workers account only classified provider failures in failure_attempts.
ALTER TABLE memory_provisioning_jobs
    ADD COLUMN phase TEXT NOT NULL DEFAULT 'create_pending',
    ADD COLUMN failure_attempts INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN readiness_deadline TIMESTAMPTZ NULL,
    ADD COLUMN next_poll_at TIMESTAMPTZ NULL;

UPDATE memory_provisioning_jobs
SET phase = CASE status
        WHEN 'completed' THEN 'ready'
        WHEN 'failed' THEN 'failed'
        ELSE 'create_pending'
    END;

ALTER TABLE memory_provisioning_jobs
    ADD CONSTRAINT memory_provisioning_jobs_phase_check
        CHECK (phase IN ('create_pending', 'waiting_ready', 'ready', 'failed')),
    ADD CONSTRAINT memory_provisioning_jobs_failure_attempts_check
        CHECK (failure_attempts >= 0),
    ADD CONSTRAINT memory_provisioning_jobs_phase_state_check
        CHECK (
            (status IN ('pending', 'leased') AND phase IN ('create_pending', 'waiting_ready'))
            OR (status = 'completed' AND phase = 'ready')
            OR (status = 'failed' AND phase = 'failed')
        ),
    ADD CONSTRAINT memory_provisioning_jobs_readiness_window_check
        CHECK (
            (phase = 'create_pending' AND readiness_deadline IS NULL AND next_poll_at IS NULL)
            OR
            (phase = 'waiting_ready' AND readiness_deadline IS NOT NULL AND next_poll_at IS NOT NULL)
            OR phase IN ('ready', 'failed')
        );

DROP INDEX memory_provisioning_jobs_due_idx;
CREATE INDEX memory_provisioning_jobs_due_idx
    ON memory_provisioning_jobs (
        (CASE phase WHEN 'waiting_ready' THEN next_poll_at ELSE available_at END),
        created_at,
        id
    )
    WHERE status = 'pending';
