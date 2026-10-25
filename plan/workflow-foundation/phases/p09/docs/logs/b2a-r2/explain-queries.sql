-- p09 F029: the run page reads for one run (i = 25000) and one parent with five children.
\set company '''11111111-1111-1111-1111-111111111111'''
\set run '''' `psql -Atc "select md5('run25000')::uuid" postgres://workflow09@127.0.0.1:55490/postgres` ''''
\set execution '''' `psql -Atc "select md5('exec25000')::uuid" postgres://workflow09@127.0.0.1:55490/postgres` ''''
\set parent '''' `psql -Atc "select md5('parent5000')::uuid" postgres://workflow09@127.0.0.1:55490/postgres` ''''

\echo '== controls (by run)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, SUMMARY ON) SELECT operation, actor_id, result, expected_revision, result_revision, created_at
   FROM workflow_control_commands WHERE company_id = :company AND run_id = :run
  ORDER BY created_at DESC, command_key DESC LIMIT 201;

\echo '== trigger message (by run)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, SUMMARY ON) SELECT outcome.intent_id FROM workflow_message_intent_bindings AS outcome
  WHERE outcome.company_id = :company AND outcome.run_id = :run LIMIT 1;

\echo '== children (by parent)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, SUMMARY ON) SELECT link.child_run_id, link.parent_execution_id FROM workflow_run_parents AS link
  WHERE link.company_id = :company AND link.parent_run_id = :parent LIMIT 201;

\echo '== reconciliations, run column (B2a SQL)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, SUMMARY ON) SELECT execution_id, invocation_id, dispatch_id, effect_kind, failure_code, observed_at
   FROM workflow_action_reconciliations WHERE company_id = :company AND run_id = :run
  ORDER BY observed_at DESC, dispatch_id DESC LIMIT 201;

\echo '== reconciliations, through the run-led dispatch key'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, SUMMARY ON) SELECT reconciliation.execution_id, reconciliation.invocation_id, reconciliation.dispatch_id,
        reconciliation.effect_kind, reconciliation.failure_code, reconciliation.observed_at
   FROM workflow_action_dispatches AS dispatch
   JOIN workflow_action_reconciliations AS reconciliation
     ON reconciliation.company_id = dispatch.company_id AND reconciliation.invocation_id = dispatch.invocation_id
  WHERE dispatch.company_id = :company AND dispatch.run_id = :run
  ORDER BY reconciliation.observed_at DESC, reconciliation.dispatch_id DESC LIMIT 201;

\echo '== receipts, run and execution columns (B2a SQL)'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, SUMMARY ON) SELECT dispatch_id, invocation_id, effect_kind, result, created_at
   FROM workflow_action_receipts WHERE company_id = :company AND run_id = :run AND execution_id = :execution
  ORDER BY created_at, dispatch_id LIMIT 51;

\echo '== receipts, through the run-led dispatch key'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF, SUMMARY ON) SELECT receipt.dispatch_id, receipt.invocation_id, receipt.effect_kind, receipt.result, receipt.created_at
   FROM workflow_action_dispatches AS dispatch
   JOIN workflow_action_receipts AS receipt
     ON receipt.company_id = dispatch.company_id AND receipt.invocation_id = dispatch.invocation_id
  WHERE dispatch.company_id = :company AND dispatch.run_id = :run AND dispatch.execution_id = :execution
  ORDER BY receipt.created_at, receipt.dispatch_id LIMIT 51;
