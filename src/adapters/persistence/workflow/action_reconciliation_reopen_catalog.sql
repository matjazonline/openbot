SELECT jsonb_build_object('oid',native.oid::bigint,'name',native.conname,'relation',native.conrelid::regclass::text,
    'name_count',(SELECT count(*) FROM pg_constraint AS same_name WHERE same_name.connamespace=native.connamespace AND same_name.conname=native.conname),
    'function',function_row.proname,'enabled',trigger_row.tgenabled::text,'timing',trigger_row.tgtype,
    'deferrable',native.condeferrable,'initially_deferred',native.condeferred,
    'definition',pg_get_triggerdef(trigger_row.oid),'function_definition',pg_get_functiondef(function_row.oid))
FROM pg_constraint AS native JOIN pg_trigger AS trigger_row ON trigger_row.tgconstraint=native.oid
    JOIN pg_proc AS function_row ON function_row.oid=trigger_row.tgfoid
WHERE native.connamespace='public'::regnamespace AND native.conname='workflow_control_retry_guard'
