CREATE FUNCTION valid_creation_provenance(provenance JSONB) RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
RETURN
    jsonb_typeof(provenance) = 'object'
    AND (provenance - ARRAY[
        'actor_type', 'actor_id', 'actor_name', 'source_channel_id', 'source_task_id'
    ]::TEXT[]) = '{}'::JSONB
    AND jsonb_typeof(provenance->'actor_type') = 'string'
    AND provenance->>'actor_type' IN ('user', 'agent', 'system')
    AND jsonb_typeof(provenance->'actor_name') = 'string'
    AND btrim(provenance->>'actor_name') <> ''
    AND CASE provenance->>'actor_type'
        WHEN 'system' THEN
            provenance ? 'actor_id'
            AND provenance->'actor_id' = 'null'::JSONB
            AND COALESCE(provenance->'source_channel_id' = 'null'::JSONB, true)
            AND COALESCE(provenance->'source_task_id' = 'null'::JSONB, true)
        WHEN 'user' THEN
            jsonb_typeof(provenance->'actor_id') = 'string'
            AND provenance->>'actor_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            AND COALESCE(provenance->'source_channel_id' = 'null'::JSONB, true)
            AND COALESCE(provenance->'source_task_id' = 'null'::JSONB, true)
        WHEN 'agent' THEN
            jsonb_typeof(provenance->'actor_id') = 'string'
            AND provenance->>'actor_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            AND jsonb_typeof(provenance->'source_channel_id') = 'string'
            AND provenance->>'source_channel_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            AND jsonb_typeof(provenance->'source_task_id') = 'string'
            AND provenance->>'source_task_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        ELSE false
    END;

ALTER TABLE agents
    ADD CONSTRAINT agents_created_by_shape_check
    CHECK (valid_creation_provenance(created_by))
    NOT VALID;

ALTER TABLE channels
    ADD CONSTRAINT channels_created_by_shape_check
    CHECK (valid_creation_provenance(created_by))
    NOT VALID;

ALTER TABLE agents VALIDATE CONSTRAINT agents_created_by_shape_check;
ALTER TABLE channels VALIDATE CONSTRAINT channels_created_by_shape_check;
