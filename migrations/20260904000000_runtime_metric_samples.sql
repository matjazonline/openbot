CREATE TABLE runtime_metric_samples (
    machine_id TEXT NOT NULL,
    machine_region TEXT,
    sampled_at TIMESTAMPTZ NOT NULL,
    process_rss_bytes BIGINT,
    memory_limit_bytes BIGINT,
    cpu_utilization_percent DOUBLE PRECISION,
    cpu_steal_percent DOUBLE PRECISION,
    cpu_throttle_percent DOUBLE PRECISION,
    database_acquire_duration_ms DOUBLE PRECISION NOT NULL,
    database_acquire_succeeded BOOLEAN NOT NULL,
    pool_size INTEGER NOT NULL,
    pool_idle INTEGER NOT NULL,
    pool_active INTEGER NOT NULL,
    PRIMARY KEY (machine_id, sampled_at),
    CONSTRAINT runtime_metric_samples_rss_nonnegative
        CHECK (process_rss_bytes IS NULL OR process_rss_bytes >= 0),
    CONSTRAINT runtime_metric_samples_memory_limit_nonnegative
        CHECK (memory_limit_bytes IS NULL OR memory_limit_bytes >= 0),
    CONSTRAINT runtime_metric_samples_cpu_utilization_nonnegative
        CHECK (cpu_utilization_percent IS NULL OR cpu_utilization_percent >= 0),
    CONSTRAINT runtime_metric_samples_cpu_steal_nonnegative
        CHECK (cpu_steal_percent IS NULL OR cpu_steal_percent >= 0),
    CONSTRAINT runtime_metric_samples_cpu_throttle_nonnegative
        CHECK (cpu_throttle_percent IS NULL OR cpu_throttle_percent >= 0),
    CONSTRAINT runtime_metric_samples_acquire_duration_nonnegative
        CHECK (database_acquire_duration_ms >= 0),
    CONSTRAINT runtime_metric_samples_pool_size_nonnegative CHECK (pool_size >= 0),
    CONSTRAINT runtime_metric_samples_pool_idle_nonnegative CHECK (pool_idle >= 0),
    CONSTRAINT runtime_metric_samples_pool_active_nonnegative CHECK (pool_active >= 0),
    CONSTRAINT runtime_metric_samples_pool_parts_fit
        CHECK (pool_idle + pool_active = pool_size)
);

-- The primary key is also the covering B-tree for reads and pruning by machine and sample time.
COMMENT ON CONSTRAINT runtime_metric_samples_pkey ON runtime_metric_samples IS
    'Supports runtime history reads on (machine_id, sampled_at)';
