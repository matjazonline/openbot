-- CHECK accepts SQL NULL. Optional composite references must explicitly require every
-- non-tenant component when assigned; otherwise MATCH SIMPLE skips the foreign key.
-- Existing invalid rows fail validation instead of guessing an owner or discarding data.
ALTER TABLE public.background_tasks
    DROP CONSTRAINT background_tasks_owner_shape_check,
    ADD CONSTRAINT background_tasks_owner_shape_check CHECK (
        (owner_principal_id IS NULL AND owner_principal_kind IS NULL)
        OR (owner_principal_id IS NOT NULL AND owner_principal_kind IS NOT NULL
            AND owner_principal_kind IN ('person', 'agent'))
    );

ALTER TABLE public.task_outreaches
    DROP CONSTRAINT task_outreaches_creator_shape_check,
    ADD CONSTRAINT task_outreaches_creator_shape_check CHECK (
        (created_by_principal_id IS NULL AND created_by_principal_kind IS NULL)
        OR (created_by_principal_id IS NOT NULL AND created_by_principal_kind IS NOT NULL
            AND created_by_principal_kind = 'agent')
    );

-- A missing or JSON-null version must fail, not evaluate to SQL NULL and pass.
-- The version is a JSON integer, as required by Rust's u8: "1" and 1.0 are not 1.
-- Preserve optional SQL NULL configuration and the existing object/size bounds.
ALTER TABLE public.agents
    DROP CONSTRAINT agents_config_v1_shape_check,
    ADD CONSTRAINT agents_config_v1_shape_check CHECK (
        config_json IS NULL OR COALESCE(
            jsonb_typeof(config_json) = 'object'
            AND jsonb_typeof(config_json -> 'version') = 'number'
            AND config_json ->> 'version' = '1'
            AND octet_length(config_json::text) <= 65536,
            false
        )
    ),
    DROP CONSTRAINT agents_native_tool_policy_shape,
    ADD CONSTRAINT agents_native_tool_policy_shape CHECK (COALESCE(
        jsonb_typeof(native_tool_policy) = 'object'
        AND jsonb_typeof(native_tool_policy -> 'version') = 'number'
        AND native_tool_policy ->> 'version' = '1'
        AND octet_length(native_tool_policy::text) <= 16384,
        false
    ));
