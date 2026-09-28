-- Durable workflow authoring and immutable publication. No existing owner is replaced.
CREATE TABLE workflow_definitions (
    company_id uuid NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 256 AND btrim(title) <> ''),
    description text NOT NULL CHECK (octet_length(description) <= 4096),
    source text NOT NULL CHECK (octet_length(source) <= 262144),
    template_origin jsonb,
    archived boolean NOT NULL DEFAULT false,
    PRIMARY KEY (company_id, id),
    CHECK (template_origin IS NULL OR (jsonb_typeof(template_origin) = 'object'
        AND octet_length(template_origin::text) <= 1024))
);

CREATE TABLE workflow_versions (
    company_id uuid NOT NULL,
    workflow_id uuid NOT NULL,
    id uuid NOT NULL,
    draft_revision bigint NOT NULL CHECK (draft_revision > 0),
    command_key text NOT NULL CHECK (octet_length(command_key) BETWEEN 1 AND 128),
    actor_id uuid NOT NULL,
    title text NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 256),
    description text NOT NULL CHECK (octet_length(description) <= 4096),
    source text NOT NULL CHECK (octet_length(source) <= 262144),
    template_origin jsonb,
    bundle bytea NOT NULL CHECK (octet_length(bundle) BETWEEN 1 AND 4194304),
    content_hash text NOT NULL CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, command_key),
    UNIQUE (company_id, workflow_id, id),
    FOREIGN KEY (company_id, workflow_id) REFERENCES workflow_definitions(company_id, id)
        ON DELETE CASCADE,
    CHECK (template_origin IS NULL OR (jsonb_typeof(template_origin) = 'object'
        AND octet_length(template_origin::text) <= 1024))
);

CREATE TABLE workflow_definition_events (
    company_id uuid NOT NULL,
    workflow_id uuid NOT NULL,
    id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    event_kind text NOT NULL CHECK (event_kind IN ('copied', 'saved', 'archived', 'published')),
    actor_id uuid NOT NULL,
    version_id uuid,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, id),
    FOREIGN KEY (company_id, workflow_id) REFERENCES workflow_definitions(company_id, id)
        ON DELETE CASCADE,
    FOREIGN KEY (company_id, workflow_id, version_id)
        REFERENCES workflow_versions(company_id, workflow_id, id),
    CHECK ((event_kind = 'published') = (version_id IS NOT NULL))
);
