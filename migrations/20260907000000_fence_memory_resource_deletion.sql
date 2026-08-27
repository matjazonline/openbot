-- Keep provider resource intent and the active execution fence after a company is deleted.
CREATE TABLE memory_remote_resource_lifecycles (
    provider TEXT NOT NULL CHECK (provider = 'hydradb'),
    remote_database_id TEXT NOT NULL,
    company_id UUID NULL REFERENCES companies(id) ON DELETE SET NULL,
    desired_state TEXT NOT NULL CHECK (desired_state IN ('present', 'absent')),
    operation_generation BIGINT NOT NULL DEFAULT 0 CHECK (operation_generation >= 0),
    operation_lease_token UUID NULL,
    operation_lease_expires_at TIMESTAMPTZ NULL,
    quiesce_until TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (provider, remote_database_id),
    UNIQUE (company_id, provider),
    CHECK (
        (operation_lease_token IS NULL AND operation_lease_expires_at IS NULL)
        OR
        (operation_lease_token IS NOT NULL AND operation_lease_expires_at IS NOT NULL)
    ),
    CHECK (desired_state = 'absent' OR company_id IS NOT NULL)
);

INSERT INTO memory_remote_resource_lifecycles
    (provider, remote_database_id, company_id, desired_state)
SELECT provider, remote_database_id, company_id, 'present'
FROM memory_provider_connections;

INSERT INTO memory_remote_resource_lifecycles
    (provider, remote_database_id, company_id, desired_state, quiesce_until, last_error)
SELECT provider, remote_database_id, NULL, 'absent',
       CURRENT_TIMESTAMP + INTERVAL '180 seconds', last_error
FROM memory_cleanup_jobs
ON CONFLICT (provider, remote_database_id) DO UPDATE
SET company_id = NULL,
    desired_state = 'absent',
    quiesce_until = GREATEST(
        memory_remote_resource_lifecycles.quiesce_until,
        EXCLUDED.quiesce_until
    ),
    last_error = EXCLUDED.last_error,
    updated_at = CURRENT_TIMESTAMP;

ALTER TABLE memory_provisioning_jobs
    ADD COLUMN operation_generation BIGINT NULL;

ALTER TABLE memory_cleanup_jobs
    ADD COLUMN operation_generation BIGINT NULL;

-- Preserve the deadline of operations leased by the application version being replaced.
UPDATE memory_remote_resource_lifecycles AS lifecycle
SET operation_generation = 1,
    operation_lease_token = job.lease_token,
    operation_lease_expires_at = job.lease_expires_at,
    updated_at = CURRENT_TIMESTAMP
FROM memory_provisioning_jobs AS job
WHERE job.provider = lifecycle.provider
  AND job.remote_database_id = lifecycle.remote_database_id
  AND job.status = 'leased';

UPDATE memory_provisioning_jobs
SET operation_generation = 1
WHERE status = 'leased';

UPDATE memory_remote_resource_lifecycles AS lifecycle
SET operation_generation = lifecycle.operation_generation + 1,
    operation_lease_token = job.lease_token,
    operation_lease_expires_at = job.lease_expires_at,
    updated_at = CURRENT_TIMESTAMP
FROM memory_cleanup_jobs AS job
WHERE job.provider = lifecycle.provider
  AND job.remote_database_id = lifecycle.remote_database_id
  AND job.status = 'leased';

UPDATE memory_cleanup_jobs AS job
SET operation_generation = lifecycle.operation_generation
FROM memory_remote_resource_lifecycles AS lifecycle
WHERE job.provider = lifecycle.provider
  AND job.remote_database_id = lifecycle.remote_database_id
  AND job.status = 'leased';

ALTER TABLE memory_provisioning_jobs
    ADD CONSTRAINT memory_provisioning_jobs_generation_state_check
    CHECK (
        (status = 'leased' AND operation_generation IS NOT NULL)
        OR
        (status <> 'leased' AND operation_generation IS NULL)
    ),
    ADD CONSTRAINT memory_provisioning_jobs_lifecycle_fkey
    FOREIGN KEY (provider, remote_database_id)
    REFERENCES memory_remote_resource_lifecycles(provider, remote_database_id);

ALTER TABLE memory_cleanup_jobs
    ADD CONSTRAINT memory_cleanup_jobs_generation_state_check
    CHECK (
        (status = 'leased' AND operation_generation IS NOT NULL)
        OR
        (status <> 'leased' AND operation_generation IS NULL)
    ),
    ADD CONSTRAINT memory_cleanup_jobs_lifecycle_fkey
    FOREIGN KEY (provider, remote_database_id)
    REFERENCES memory_remote_resource_lifecycles(provider, remote_database_id);
