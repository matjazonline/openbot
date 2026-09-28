-- The logical binding owns activity/CAS; immutable configuration revisions own
-- selected version, parameters and credential-free resource references.
CREATE TABLE workflow_bindings (
    company_id uuid NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    id uuid NOT NULL,
    state_revision bigint NOT NULL CHECK (state_revision > 0),
    configuration_revision bigint NOT NULL CHECK (configuration_revision > 0),
    active boolean NOT NULL DEFAULT false,
    channel_id uuid,
    thread_id uuid,
    PRIMARY KEY (company_id, id),
    CHECK (thread_id IS NULL OR channel_id IS NOT NULL),
    FOREIGN KEY (company_id, channel_id) REFERENCES channels(company_id, id),
    FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id)
);

CREATE TABLE workflow_binding_revisions (
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    version_id uuid NOT NULL,
    params jsonb NOT NULL CHECK (octet_length(params::text) <= 4194304),
    resources jsonb NOT NULL CHECK (jsonb_typeof(resources) = 'object'
        AND octet_length(resources::text) <= 262144),
    actor_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, binding_id, revision),
    FOREIGN KEY (company_id, binding_id) REFERENCES workflow_bindings(company_id, id)
        ON DELETE CASCADE,
    FOREIGN KEY (company_id, version_id) REFERENCES workflow_versions(company_id, id)
);

-- Deferred only to permit head + initial immutable revision in one transaction.
-- No committed head may select a missing, foreign or different binding's revision.
ALTER TABLE workflow_bindings ADD CONSTRAINT workflow_bindings_selected_revision_fk
    FOREIGN KEY (company_id, id, configuration_revision)
    REFERENCES workflow_binding_revisions(company_id, binding_id, revision)
    DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE workflow_binding_events (
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    state_revision bigint NOT NULL CHECK (state_revision > 0),
    configuration_revision bigint NOT NULL CHECK (configuration_revision > 0),
    event_kind text NOT NULL CHECK (event_kind IN ('configured', 'activated', 'deactivated')),
    actor_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, binding_id, state_revision),
    FOREIGN KEY (company_id, binding_id) REFERENCES workflow_bindings(company_id, id)
        ON DELETE CASCADE,
    FOREIGN KEY (company_id, binding_id, configuration_revision)
        REFERENCES workflow_binding_revisions(company_id, binding_id, revision)
);
