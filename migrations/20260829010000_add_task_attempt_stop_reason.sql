ALTER TABLE task_attempts
ADD COLUMN stop_reason TEXT NULL
    CONSTRAINT task_attempts_stop_reason_check
    CHECK (stop_reason IN (
        'completed', 'retryable_failure', 'terminal_failure',
        'timed_out', 'shutdown', 'lease_lost'
    ));
