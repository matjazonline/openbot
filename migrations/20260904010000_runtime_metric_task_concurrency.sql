ALTER TABLE runtime_metric_samples
    ADD COLUMN active_task_executions INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN task_worker_concurrency_limit INTEGER NOT NULL DEFAULT 1,
    ADD CONSTRAINT runtime_metric_samples_active_tasks_nonnegative
        CHECK (active_task_executions >= 0),
    ADD CONSTRAINT runtime_metric_samples_worker_limit_positive
        CHECK (task_worker_concurrency_limit > 0),
    ADD CONSTRAINT runtime_metric_samples_active_tasks_within_limit
        CHECK (active_task_executions <= task_worker_concurrency_limit);
