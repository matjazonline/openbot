SELECT jsonb_object_agg(objects.identity,objects.definition ORDER BY objects.identity)
FROM (
    SELECT 'constraint:' || relation.relname || '.' || constraint_row.conname AS identity,
        jsonb_build_object('oid',constraint_row.oid::bigint,'definition',pg_get_constraintdef(constraint_row.oid),
            'deferred',constraint_row.condeferred,'deferrable',constraint_row.condeferrable,'validated',constraint_row.convalidated) AS definition
    FROM pg_constraint AS constraint_row JOIN pg_class AS relation ON relation.oid=constraint_row.conrelid
    JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace WHERE namespace.nspname='public'
    UNION ALL
    SELECT 'trigger:' || relation.relname || '.' || trigger_row.tgname,
        jsonb_build_object('oid',trigger_row.oid::bigint,'definition',pg_get_triggerdef(trigger_row.oid),
            'enabled',trigger_row.tgenabled::text,'deferred',trigger_row.tginitdeferred,'deferrable',trigger_row.tgdeferrable)
    FROM pg_trigger AS trigger_row JOIN pg_class AS relation ON relation.oid=trigger_row.tgrelid
    JOIN pg_namespace AS namespace ON namespace.oid=relation.relnamespace WHERE namespace.nspname='public'
    UNION ALL
    SELECT 'function:' || function_row.oid,
        jsonb_build_object('oid',function_row.oid::bigint,'name',function_row.proname,'definition',pg_get_functiondef(function_row.oid))
    FROM pg_proc AS function_row JOIN pg_namespace AS namespace ON namespace.oid=function_row.pronamespace
    WHERE namespace.nspname='public' AND function_row.prokind IN ('f','p')
) AS objects
