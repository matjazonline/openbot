ALTER TABLE companies
    ADD COLUMN memory_provider TEXT NULL
        CHECK (memory_provider IS NULL OR memory_provider IN ('none', 'hydradb'));

ALTER TABLE channels
    ADD COLUMN retrieve_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN retrieve_agent_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN retrieve_user_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN persist_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN persist_agent_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN persist_user_memory BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN memory_recall_mode TEXT NOT NULL DEFAULT 'fast'
        CHECK (memory_recall_mode IN ('fast', 'thinking')),
    ADD COLUMN memory_max_results SMALLINT NOT NULL DEFAULT 5
        CHECK (memory_max_results BETWEEN 1 AND 20);

CREATE TABLE memory_provider_connections (
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('hydradb')),
    remote_database_id TEXT NOT NULL,
    readiness TEXT NOT NULL DEFAULT 'pending'
        CHECK (readiness IN ('pending', 'provisioning', 'ready', 'failed')),
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, provider),
    UNIQUE (provider, remote_database_id)
);

CREATE TABLE memory_cleanup_jobs (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL CHECK (provider IN ('hydradb')),
    remote_database_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'leased', 'completed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_expires_at TIMESTAMPTZ NULL,
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (provider, remote_database_id)
);
