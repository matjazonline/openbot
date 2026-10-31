# Mutation check: trigger name is load-bearing

Renamed `CREATE TRIGGER workflow_run_finished` to `workflow_run_zz_finished` (sorts after `workflow_run_revision`) in the init schema, ran `run_finished_at_keeps_the_revision_when_the_stamp_undoes_a_write`:

```
FAILED: UPDATE workflow_runs SET state = 'succeeded', finished_at = NULL WHERE id = $1
  left: (Some(2026-10-31T00:53:01.938962Z), 5)
 right: (Some(2026-10-31T00:53:01.938962Z), 4)
```

Schema restored from the scratch backup afterwards; `CREATE TRIGGER workflow_run_finished BEFORE` present once.
