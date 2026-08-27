-- HydraDB calls counted per ten-second sample rather than probed, so the figures are the latency
-- and failures memory recall and ingestion actually paid, and an idle machine polls nobody.
ALTER TABLE runtime_metric_samples
    ADD COLUMN hydradb_calls INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN hydradb_failures INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN hydradb_duration_ms DOUBLE PRECISION NOT NULL DEFAULT 0,
    ADD CONSTRAINT runtime_metric_samples_hydradb_calls_nonnegative
        CHECK (hydradb_calls >= 0),
    ADD CONSTRAINT runtime_metric_samples_hydradb_failures_within_calls
        CHECK (hydradb_failures >= 0 AND hydradb_failures <= hydradb_calls),
    ADD CONSTRAINT runtime_metric_samples_hydradb_duration_nonnegative
        CHECK (hydradb_duration_ms >= 0),
    ADD CONSTRAINT runtime_metric_samples_hydradb_duration_needs_calls
        CHECK (hydradb_calls > 0 OR hydradb_duration_ms = 0);
