ALTER TABLE agents
ADD COLUMN run_timeout_secs INTEGER NULL
    CONSTRAINT agents_run_timeout_secs_check
    CHECK (run_timeout_secs BETWEEN 1 AND 3600);
