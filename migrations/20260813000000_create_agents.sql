-- up
CREATE TABLE IF NOT EXISTS agents (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    slug VARCHAR(100) NOT NULL,
    provider VARCHAR(100),
    model VARCHAR(100),
    api_key TEXT,
    config_json JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_agents_company_id ON agents(company_id);
CREATE INDEX IF NOT EXISTS idx_agents_slug ON agents(slug);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agents_company_id_slug ON agents(company_id, slug);
