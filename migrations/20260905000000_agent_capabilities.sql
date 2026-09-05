-- Stored skill library and the complete capability selection for each agent.

CREATE TABLE skills (
    id UUID PRIMARY KEY,
    company_id UUID REFERENCES companies(id) ON DELETE CASCADE,
    slug CITEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    trigger TEXT NOT NULL,
    instructions JSONB NOT NULL,
    created_by JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT skills_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT skills_company_slug_key UNIQUE (company_id, slug),
    CONSTRAINT skills_name_bounded CHECK (
        btrim(name) <> '' AND char_length(name) <= 120
    ),
    CONSTRAINT skills_description_bounded CHECK (
        btrim(description) <> '' AND char_length(description) <= 500
    ),
    CONSTRAINT skills_trigger_bounded CHECK (
        btrim(trigger) <> '' AND char_length(trigger) <= 500
    ),
    CONSTRAINT skills_slug_format CHECK (
        char_length(slug::text) <= 120
        AND slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'
    ),
    CONSTRAINT skills_instructions_shape CHECK (
        jsonb_typeof(instructions) = 'array'
        AND jsonb_array_length(instructions) BETWEEN 1 AND 32
        AND octet_length(instructions::text) <= 524288
    ),
    CONSTRAINT skills_created_by_shape_check CHECK (valid_creation_provenance(created_by))
);

CREATE UNIQUE INDEX skills_library_slug_key ON skills (slug) WHERE company_id IS NULL;

CREATE TABLE agent_skills (
    company_id UUID REFERENCES companies(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (agent_id, skill_id),
    CONSTRAINT agent_skills_agent_position_key UNIQUE (agent_id, position),
    CONSTRAINT agent_skills_position_check CHECK (position BETWEEN 0 AND 15)
);

CREATE INDEX agent_skills_skill_idx ON agent_skills (skill_id, agent_id);

CREATE FUNCTION enforce_agent_skill_scope() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM agents AS agent
        WHERE agent.id = NEW.agent_id
          AND agent.company_id IS NOT DISTINCT FROM NEW.company_id
    ) THEN
        RAISE EXCEPTION 'agent must match the relationship company scope'
            USING ERRCODE = '23514', CONSTRAINT = 'agent_skills_agent_scope_check';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM skills AS skill
        WHERE skill.id = NEW.skill_id
          AND skill.company_id IS NOT DISTINCT FROM NEW.company_id
    ) THEN
        RAISE EXCEPTION 'skill must match the relationship company scope'
            USING ERRCODE = '23514', CONSTRAINT = 'agent_skills_skill_scope_check';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_skills_scope_check
BEFORE INSERT OR UPDATE ON agent_skills
FOR EACH ROW EXECUTE FUNCTION enforce_agent_skill_scope();

CREATE FUNCTION prevent_assigned_library_skill_delete() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.company_id IS NULL
       AND EXISTS (SELECT 1 FROM agent_skills WHERE skill_id = OLD.id) THEN
        RAISE EXCEPTION 'library skill is assigned to one or more agents'
            USING ERRCODE = '23503', CONSTRAINT = 'library_skill_delete_guard';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER library_skill_delete_guard
BEFORE DELETE ON skills
FOR EACH ROW EXECUTE FUNCTION prevent_assigned_library_skill_delete();

CREATE TABLE agent_sub_agents (
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL,
    sub_agent_id UUID NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (agent_id, sub_agent_id),
    CONSTRAINT agent_sub_agents_agent_position_key UNIQUE (agent_id, position),
    CONSTRAINT agent_sub_agents_position_check CHECK (position BETWEEN 0 AND 63),
    CONSTRAINT agent_sub_agents_not_self CHECK (agent_id <> sub_agent_id),
    CONSTRAINT agent_sub_agents_agent_fk FOREIGN KEY (company_id, agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE,
    CONSTRAINT agent_sub_agents_sub_fk FOREIGN KEY (company_id, sub_agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE
);

CREATE INDEX agent_sub_agents_sub_idx ON agent_sub_agents (sub_agent_id, agent_id);

CREATE FUNCTION valid_tool_id_array(ids TEXT[]) RETURNS BOOLEAN
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT cardinality(ids) <= 32
       AND array_position(ids, NULL) IS NULL
       AND COALESCE(bool_and(btrim(id) <> '' AND char_length(id) <= 120), TRUE)
    FROM unnest(ids) AS id;
$$;

ALTER TABLE agents
    ADD COLUMN harness_kind TEXT NOT NULL DEFAULT 'ai_agents',
    ADD COLUMN granted_tool_ids TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN native_tool_policy JSONB NOT NULL DEFAULT '{"version": 1}',
    ADD CONSTRAINT agents_harness_kind_check CHECK (harness_kind IN ('ai_agents')),
    ADD CONSTRAINT agents_granted_tool_ids_bounded CHECK (valid_tool_id_array(granted_tool_ids)),
    ADD CONSTRAINT agents_native_tool_policy_shape CHECK (
        jsonb_typeof(native_tool_policy) = 'object'
        AND native_tool_policy ->> 'version' = '1'
        AND octet_length(native_tool_policy::text) <= 16384
    );

ALTER TABLE agents DROP CONSTRAINT agents_config_object_check;
ALTER TABLE agents
    ADD CONSTRAINT agents_config_v1_shape_check CHECK (
        config_json IS NULL
        OR (
            jsonb_typeof(config_json) = 'object'
            AND config_json ->> 'version' = '1'
            AND octet_length(config_json::text) <= 65536
        )
    );
