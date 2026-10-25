-- p09 F029: representative volume for the run page reads, in a scratch database on the phase
-- cluster only (workflow09_explain on 127.0.0.1:55490). Replica mode skips the FK and guard
-- triggers, so rows can be seeded table by table; CHECK constraints still hold.
-- One company with 50,000 runs: per run 4 control commands, 1 admitted intent binding,
-- 1 parent link, 2 dispatches with receipts; a reconciliation on every 5th run.
SET session_replication_role = replica;
\set company '''11111111-1111-1111-1111-111111111111'''

CREATE TEMP TABLE seed_runs AS
SELECT i, md5('run' || i)::uuid AS run, md5('exec' || i)::uuid AS execution,
       md5('parent' || (i / 5))::uuid AS parent
  FROM generate_series(1, 50000) AS i;

INSERT INTO workflow_control_commands
    (company_id, command_key, run_id, actor_id, operation, expected_revision, result,
     result_revision, audit_sequence, created_at)
SELECT :company, 'key-' || i || '-' || n, run, md5('actor')::uuid,
       CASE WHEN n = 4 THEN 'cancel' ELSE 'retry' END, n, 'conflict', n, NULL,
       now() - (i || ' seconds')::interval
  FROM seed_runs, generate_series(1, 4) AS n;

INSERT INTO workflow_message_intent_bindings
    (company_id, intent_id, binding_id, binding_revision, outcome, run_id, reason)
SELECT :company, md5('intent' || i)::uuid, md5('binding')::uuid, 1, 'admitted', run, NULL
  FROM seed_runs;

INSERT INTO workflow_run_parents (company_id, child_run_id, parent_run_id, parent_execution_id)
SELECT :company, run, parent, md5('pexec' || (i / 5))::uuid FROM seed_runs;

INSERT INTO workflow_action_dispatches
    (company_id, run_id, execution_id, invocation_id, argument_digest, id, job_id,
     attempt_number, execution_generation, worker_id, effect_kind, current_policy)
SELECT :company, run, execution, md5('inv' || i || '-' || n)::uuid, repeat('a', 64),
       md5('disp' || i || '-' || n)::uuid, md5('job' || i)::uuid, 1, md5('gen')::uuid,
       md5('worker')::uuid, 'local', '{}'::jsonb
  FROM seed_runs, generate_series(1, 2) AS n;

INSERT INTO workflow_action_receipts
    (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind,
     result)
SELECT :company, run, execution, md5('inv' || i || '-' || n)::uuid, repeat('a', 64),
       md5('disp' || i || '-' || n)::uuid, 'local', '{"ok": true}'::jsonb
  FROM seed_runs, generate_series(1, 2) AS n;

INSERT INTO workflow_action_reconciliations
    (company_id, run_id, execution_id, invocation_id, argument_digest, dispatch_id, effect_kind,
     failure_code)
SELECT :company, run, execution, md5('inv' || i || '-1')::uuid, repeat('a', 64),
       md5('disp' || i || '-1')::uuid, 'remote', 'workflow.remote_unknown'
  FROM seed_runs WHERE i % 5 = 0;

SET session_replication_role = origin;
ANALYZE workflow_control_commands, workflow_message_intent_bindings, workflow_run_parents,
        workflow_action_dispatches, workflow_action_receipts, workflow_action_reconciliations;
