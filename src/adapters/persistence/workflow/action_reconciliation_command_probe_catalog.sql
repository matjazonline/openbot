SELECT jsonb_build_object(
    'oid',constraint_row.oid::bigint,'name',constraint_row.conname,'type',constraint_row.contype::text,
    'relation',constraint_row.conrelid::regclass::text,
    'reference',CASE WHEN constraint_row.confrelid=0 THEN NULL ELSE constraint_row.confrelid::regclass::text END,
    'columns',to_jsonb(ARRAY(SELECT attribute.attname::text FROM unnest(constraint_row.conkey) WITH ORDINALITY AS key_column(number,position)
        JOIN pg_attribute AS attribute ON attribute.attrelid=constraint_row.conrelid AND attribute.attnum=key_column.number ORDER BY key_column.position)),
    'reference_columns',to_jsonb(ARRAY(SELECT attribute.attname::text FROM unnest(constraint_row.confkey) WITH ORDINALITY AS key_column(number,position)
        JOIN pg_attribute AS attribute ON attribute.attrelid=constraint_row.confrelid AND attribute.attnum=key_column.number ORDER BY key_column.position)),
    'deferrable',constraint_row.condeferrable,'initially_deferred',constraint_row.condeferred,
    'definition',pg_get_constraintdef(constraint_row.oid),
    'name_count',(SELECT count(*) FROM pg_constraint AS same_name WHERE same_name.connamespace=constraint_row.connamespace AND same_name.conname=constraint_row.conname),
    'triggers',(SELECT jsonb_agg(jsonb_build_object('oid',trigger_row.oid::bigint,'name',trigger_row.tgname,
        'relation',trigger_row.tgrelid::regclass::text,'enabled',trigger_row.tgenabled::text,
        'deferrable',trigger_row.tgdeferrable,'initially_deferred',trigger_row.tginitdeferred,
        'function',function_row.proname,'function_oid',function_row.oid::bigint,
        'definition',pg_get_triggerdef(trigger_row.oid),'function_definition',pg_get_functiondef(function_row.oid)) ORDER BY trigger_row.oid)
        FROM pg_trigger AS trigger_row JOIN pg_proc AS function_row ON function_row.oid=trigger_row.tgfoid
        WHERE trigger_row.tgconstraint=constraint_row.oid))
FROM pg_constraint AS constraint_row
WHERE constraint_row.conrelid IN ('workflow_action_evidence'::regclass,'workflow_action_evidence_commands'::regclass,'workflow_run_events'::regclass)
ORDER BY constraint_row.oid
