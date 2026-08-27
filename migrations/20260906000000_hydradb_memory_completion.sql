-- Complete the durable state machines introduced by 20260826000000_memory_settings.sql.
UPDATE companies SET memory_provider = NULL WHERE memory_provider = 'none';

ALTER TABLE companies
    DROP CONSTRAINT IF EXISTS companies_memory_provider_check;
ALTER TABLE companies
    ADD CONSTRAINT companies_memory_provider_check
        CHECK (memory_provider IS NULL OR memory_provider = 'hydradb');

CREATE TABLE memory_provisioning_jobs (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider = 'hydradb'),
    remote_database_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'leased', 'completed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_token UUID NULL,
    lease_expires_at TIMESTAMPTZ NULL,
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (company_id, provider),
    UNIQUE (provider, remote_database_id),
    CHECK (
        (status = 'leased' AND lease_token IS NOT NULL AND lease_expires_at IS NOT NULL)
        OR
        (status <> 'leased' AND lease_token IS NULL AND lease_expires_at IS NULL)
    )
);

CREATE INDEX memory_provisioning_jobs_due_idx
    ON memory_provisioning_jobs (available_at, created_at, id)
    WHERE status = 'pending';

ALTER TABLE memory_cleanup_jobs
    ADD COLUMN lease_token UUID NULL;

UPDATE memory_cleanup_jobs
SET status = 'pending', lease_expires_at = NULL
WHERE status = 'leased';

ALTER TABLE memory_cleanup_jobs
    ADD CONSTRAINT memory_cleanup_jobs_lease_state_check
    CHECK (
        (status = 'leased' AND lease_token IS NOT NULL AND lease_expires_at IS NOT NULL)
        OR
        (status <> 'leased' AND lease_token IS NULL AND lease_expires_at IS NULL)
    );

CREATE INDEX memory_cleanup_jobs_due_idx
    ON memory_cleanup_jobs (available_at, created_at, id)
    WHERE status = 'pending';
