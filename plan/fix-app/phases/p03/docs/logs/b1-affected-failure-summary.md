# B1 affected union failure checkpoint (2026-11-04)
Full immutable-at-pause output: `b1-affected-union.log`. Final:1071 passed,31 failed,1 ignored;1103 selected;1500 filtered;1064.12s;command exit101. No corrections after this run.
All31 failures remain unresolved at the user pause. Source-inspection candidates are not verified fixes; retain original behavior/native-guard/rollback assertions when resuming.

## adapters::persistence::workflow::tests::admission_tests::batch_tests::wakeup_tests::guard_tests::workflow_wakeup_actual_wait_expiry_produces_failed_fact

```text
thread 'adapters::persistence::workflow::tests::admission_tests::batch_tests::wakeup_tests::guard_tests::workflow_wakeup_actual_wait_expiry_produces_failed_fact' (12779738) panicked at src/adapters/persistence/workflow/wakeup_guard_tests.rs:85:5:
assertion `left == right` failed
  left: Some(ExecutionId(8bf9eccf-c1cd-49aa-8621-fd3306c01c65))
 right: None
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_receipt_tests::history_tests::workflow_action_history_direct_sql_reopen_rechecks_current_retention

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_receipt_tests::history_tests::workflow_action_history_direct_sql_reopen_rechecks_current_retention' (12782536) panicked at src/adapters/persistence/workflow/action_history_tests.rs:298:45:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23514", message: "workflow run cannot move from failed to running", detail: None, hint: None, position: None, where: Some("PL/pgSQL function guard_workflow_run_transition() line 20 at RAISE"), schema: None, table: None, column: None, data_type: None, constraint: Some("workflow_guard:guard_workflow_run_transition"), file: Some("pl_exec.c"), line: Some(3923), routine: Some("exec_stmt_raise") })

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::append_only_tests::workflow_action_reconciliation_append_only_mutation_delete_upsert_and_owner_lifecycle

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::append_only_tests::workflow_action_reconciliation_append_only_mutation_delete_upsert_and_owner_lifecycle' (12783716) panicked at src/adapters/persistence/workflow/action_reconciliation_append_only_tests.rs:74:5:
assertion `left == right` failed: UPDATE workflow_action_evidence SET diagnostic=diagnostic || '{"attempted_change":true}'::jsonb WHERE company_id=$1
  left: Some("workflow_guard:workflow_action_fact_immutable")
 right: None

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::late_truth_tests::conflict_immutable_tests::workflow_action_reconciliation_conflict_row_rejects_update_and_delete

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::late_truth_tests::conflict_immutable_tests::workflow_action_reconciliation_conflict_row_rejects_update_and_delete' (12790417) panicked at src/adapters/persistence/workflow/action_reconciliation_conflict_immutable_tests.rs:16:5:
assertion `left == right` failed: UPDATE workflow_action_evidence_conflicts SET created_at=created_at+interval '1 second' WHERE company_id=$1
  left: Some("workflow_guard:workflow_action_fact_immutable")
 right: None

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::sibling_tests::boundary_tests::workflow_action_reconciliation_siblings_existing_job_reopen_gates_preserve_history

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::sibling_tests::boundary_tests::workflow_action_reconciliation_siblings_existing_job_reopen_gates_preserve_history' (12794643) panicked at src/adapters/persistence/workflow/action_reconciliation_sibling_boundary_tests.rs:30:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23514", message: "new row for relation \"workflow_runs\" violates check constraint \"workflow_run_terminal_execution_shape\"", detail: Some("Failing row contains (925ef97d-905d-49aa-b119-01fc72034f8b, 883493bf-a8ba-4f68-8e25-4d9cc1df48ff, 5df66ad0-2ac9-45b0-a87f-889528dcfafe, 1, 00000000-0000-0000-0000-000000000001, 20162d22-acb2-4549-861d-04e403328bc0, null, null, 1407c50c-e795-4c65-969c-20081e79d041, ed2007b4-f711-4a55-872b-33e821dea19e, f5bff9df-9b51-4c02-83c9-c13776134ffb, \\x7b22666f726d6174223a22776f726b666c6f772d62756e646c652d73746f72..., {\"value\": 1}, {}, {}, 100, 65536, 2026-11-05 12:31:23.929902+00, 2026-11-04 12:31:23.929902+00, failed, null, null, null, null, null, 11, null, deliver, 2026-11-04 12:31:24.983895+00)."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_runs"), column: None, data_type: None, constraint: Some("workflow_run_terminal_execution_shape"), file: Some("execMain.c"), line: Some(2081), routine: Some("ExecConstraints") })

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::sibling_tests::eligibility_tests::claim_sql_tests::candidate_tests::workflow_action_reconciliation_claim_sql_multiple_applicable_candidates

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::sibling_tests::eligibility_tests::claim_sql_tests::candidate_tests::workflow_action_reconciliation_claim_sql_multiple_applicable_candidates' (12802279) panicked at src/adapters/persistence/workflow/action_reconciliation_claim_sql_candidate_support.rs:107:5:
assertion `left == right` failed
  left: ["guard_workflow_run_transition", "workflow_action_capture_claim_retirement", "workflow_action_capture_state_witness", "workflow_allocate_wakeup", "workflow_parent_immutable", "workflow_run_finished", "workflow_run_revision", "workflow_run_snapshot_immutable", "workflow_runs_preserve_association"]
 right: ["workflow_action_capture_claim_retirement", "workflow_action_capture_state_witness", "workflow_allocate_wakeup", "workflow_parent_immutable", "workflow_run_finished", "workflow_run_revision", "workflow_run_snapshot_immutable", "workflow_runs_preserve_association"]

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::sibling_tests::eligibility_tests::claim_sql_tests::episode_tests::workflow_action_reconciliation_claim_sql_episode_wrong_retired_ordinal

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::sibling_tests::eligibility_tests::claim_sql_tests::episode_tests::workflow_action_reconciliation_claim_sql_episode_wrong_retired_ordinal' (12804990) panicked at src/adapters/persistence/workflow/action_reconciliation_claim_sql_episode_tests.rs:238:5:
assertion `left == right` failed
  left: "error returned from database: episode ordinal probe SQLSTATE=23514 SQLERRM=invalid current workflow reconciliation claim episode CONSTRAINT=workflow_guard:workflow_action_claim_episode_guard"
 right: "error returned from database: episode ordinal probe SQLSTATE=23514 SQLERRM=invalid current workflow reconciliation claim episode CONSTRAINT="

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_attribution_tests::native_coverage_tests::workflow_action_reconciliation_native_coverage_marker_nonempty

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_attribution_tests::native_coverage_tests::workflow_action_reconciliation_native_coverage_marker_nonempty' (12812631) panicked at src/adapters/persistence/workflow/action_reconciliation_native_coverage_assertions.rs:72:5:
assertion `left == right` failed
  left: ("", "", "workflow_guard:workflow_action_evidence_commit_guard")
 right: ("", "", "")

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_attribution_tests::native_coverage_tests::workflow_action_reconciliation_native_coverage_unknown_missing

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_attribution_tests::native_coverage_tests::workflow_action_reconciliation_native_coverage_unknown_missing' (12812635) panicked at src/adapters/persistence/workflow/action_reconciliation_native_coverage_assertions.rs:72:5:
assertion `left == right` failed
  left: ("", "", "workflow_guard:workflow_action_evidence_commit_guard")
 right: ("", "", "")

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_attribution_tests::native_coverage_tests::workflow_action_reconciliation_native_coverage_applied_membership

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_attribution_tests::native_coverage_tests::workflow_action_reconciliation_native_coverage_applied_membership' (12812584) panicked at src/adapters/persistence/workflow/action_reconciliation_native_coverage_assertions.rs:72:5:
assertion `left == right` failed
  left: ("", "", "workflow_guard:workflow_action_evidence_commit_guard")
 right: ("", "", "")

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_attribution_tests::workflow_action_reconciliation_native_attribution_scope_and_before

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_attribution_tests::workflow_action_reconciliation_native_attribution_scope_and_before' (12812904) panicked at src/adapters/persistence/workflow/action_reconciliation_native_attribution_assertions.rs:194:9:
assertion `left == right` failed
  left: ("", "", "workflow_guard:workflow_action_applied_request_guard")
 right: ("", "", "")

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_object_tests::native_chronology_tests::workflow_action_reconciliation_native_chronology_marker_rejected

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::validation_tests::native_bounds_tests::native_evidence_tests::native_diagnostic_tests::native_object_tests::native_chronology_tests::workflow_action_reconciliation_native_chronology_marker_rejected' (12814454) panicked at src/adapters/persistence/workflow/action_reconciliation_native_chronology_tests.rs:142:9:
assertion `left == right` failed
  left: "workflow_guard:workflow_action_evidence_commit_guard"
 right: ""

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::provisioning_dispatch_tests::provisioning_lists_an_oversized_directory_within_its_bound

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::provisioning_dispatch_tests::provisioning_lists_an_oversized_directory_within_its_bound' (12827182) panicked at src/adapters/persistence/workflow/provisioning_dispatch_tests.rs:160:49:
called `Result::unwrap()` on an `Err` value: Conflict("Workflow lifecycle revision or command conflict")

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::provisioning_dispatch_tests::provisioning_refusals_commit_a_receipt_and_no_effect

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::provisioning_dispatch_tests::provisioning_refusals_commit_a_receipt_and_no_effect' (12827217) panicked at src/adapters/persistence/workflow/provisioning_dispatch_tests.rs:160:49:
called `Result::unwrap()` on an `Err` value: Conflict("Workflow lifecycle revision or command conflict")

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::provisioning_dispatch_tests::provisioning_transient_fault_leaves_no_marker_receipt_or_effect

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::provisioning_dispatch_tests::provisioning_transient_fault_leaves_no_marker_receipt_or_effect' (12827232) panicked at src/adapters/persistence/workflow/provisioning_dispatch_tests.rs:160:49:
called `Result::unwrap()` on an `Err` value: DatabaseTimeout("Workflow action.local not applied: persistence bound elapsed before commit")

```

## adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::sibling_tests::bounds_tests::prior_bounds_tests::workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses

```text
thread 'adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::provider_tests::authority_tests::proof_tests::sibling_tests::bounds_tests::prior_bounds_tests::workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses' (12795482) panicked at src/adapters/persistence/workflow/action_reconciliation_prior_bounds_support.rs:128:10:
called `Result::unwrap()` on an `Err` value: Conflict("Workflow lease retirement refused: run or lease window closed")

```

## adapters::persistence::workflow::tests::http_tests::approval_tests::continuation_tests::workflow_action_approval_continuation_ceiling_refuses_without_resetting_failures

```text
thread 'adapters::persistence::workflow::tests::http_tests::approval_tests::continuation_tests::workflow_action_approval_continuation_ceiling_refuses_without_resetting_failures' (12835135) panicked at src/adapters/persistence/workflow/action_approval_continuation_tests.rs:128:10:
called `Result::unwrap()` on an `Err` value: DatabaseTimeout("error returned from database: canceling statement due to statement timeout")

```

## adapters::persistence::workflow::tests::message_tests::race_tests::workflow_message_race_delayed_commit_past_fence_expiry_or_deadline_rolls_back

```text
thread 'adapters::persistence::workflow::tests::message_tests::race_tests::workflow_message_race_delayed_commit_past_fence_expiry_or_deadline_rolls_back' (12844129) panicked at src/adapters/persistence/workflow/message_race_tests.rs:169:9:
assertion `left == right` failed: fence expiry
  left: "message.guard_violation"
 right: "message.unresolved"

```

## adapters::persistence::workflow::tests::message_tests::workflow_tool_race_tests::workflow_tool_cancel_and_continuation_settle_the_call_once

```text
thread 'adapters::persistence::workflow::tests::message_tests::workflow_tool_race_tests::workflow_tool_cancel_and_continuation_settle_the_call_once' (12847253) panicked at src/adapters/persistence/workflow/workflow_tool_race_tests.rs:47:45:
called `Result::unwrap()` on an `Err` value: DatabaseTimeout("error returned from database: canceling statement due to statement timeout")

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_association_tests::workflow_job_association_insert_serializes_parent_update

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_association_tests::workflow_job_association_insert_serializes_parent_update' (12847673) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(3e229f64-b47f-447c-a35f-4a6d90d95aa0, 52b2c52d-103f-44b8-9343-56aa795c4c44) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_association_tests::workflow_job_association_nullable_scope_matrix_and_shared_attempts

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_association_tests::workflow_job_association_nullable_scope_matrix_and_shared_attempts' (12847698) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(4f15e740-fc3e-4b25-a012-712627f7df68, 8cedd2f5-7168-4806-b884-e1d86b9ababc) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_association_tests::workflow_job_association_parents_and_scope_reject_mutation

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_association_tests::workflow_job_association_parents_and_scope_reject_mutation' (12847718) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(1c53c137-8f74-47ae-a82c-0ab8dc7abfc6, 7d53a032-f3f3-4c5e-aae4-b1487cf97785) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_association_tests::workflow_job_association_prior_parent_update_serializes_insert

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_association_tests::workflow_job_association_prior_parent_update_serializes_insert' (12847743) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(45fb2156-58f4-4b8b-9a70-8f2e068d052e, d483f904-2d2e-4834-99fa-ad1ae251c387) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_core_query_tests::job_projection_tests::workflow_job_projection_dashboard_counts_every_workflow_job

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_core_query_tests::job_projection_tests::workflow_job_projection_dashboard_counts_every_workflow_job' (12847884) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(cd22e1a6-63b3-4353-be72-a169a1dc7cf8, d75a5efa-6514-438d-85ce-01941e28a79c) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::workflow_job_schema_enforces_exact_ids_and_scoped_execution

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::workflow_job_schema_enforces_exact_ids_and_scoped_execution' (12847936) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(c37cd86c-7748-40c4-ac86-c5fb52352e56, 5ab5c3ec-f5bd-4573-8124-589c89f168dd) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::workflow_job_schema_keeps_the_channel_nullable_for_company_associations

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::workflow_job_schema_keeps_the_channel_nullable_for_company_associations' (12847953) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(8ba7c14f-6321-44d9-a41d-3916866c2d7f, 99fb5618-146c-4b63-871e-5c7722172a19) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_core_query_tests::job_projection_tests::workflow_job_projection_dashboard_outstanding_jobs_name_their_run

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::job_core_query_tests::job_projection_tests::workflow_job_projection_dashboard_outstanding_jobs_name_their_run' (12847929) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(563601b6-d81c-4caf-8d8f-c4057577c1b2, d2c800fd-1dd1-4021-8937-f427d75ec2da) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::workflow_job_schema_rejects_foreign_execution_and_a_non_workflow_job

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::job_schema_tests::workflow_job_schema_rejects_foreign_execution_and_a_non_workflow_job' (12847987) panicked at src/adapters/persistence/workflow/job_schema_tests.rs:48:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(e8176c73-8a9e-43de-ac6e-fea1b136c8c4, e8d5f30a-0965-4dea-92b2-c132650b7118) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::workflow_run_schema_competing_source_claims_rollback_losing_run_and_scope_aliases

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::workflow_run_schema_competing_source_claims_rollback_losing_run_and_scope_aliases' (12848051) panicked at src/adapters/persistence/workflow/run_schema_tests.rs:180:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(4f3d69d2-d507-4cdd-9228-e49fc60c3534, d454c4ae-ec81-45e7-83c0-21d1dc1747ff) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::workflow_run_schema_preserves_snapshot_refuses_binding_removal_and_scopes_children

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::workflow_run_schema_preserves_snapshot_refuses_binding_removal_and_scopes_children' (12848212) panicked at src/adapters/persistence/workflow/run_schema_tests.rs:68:10:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(679aa03e-74af-4030-bbf4-ef84fdb493d3, 78517eaf-7193-43c8-8d14-480916149b1d) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```

## adapters::persistence::workflow::tests::run_schema_tests::workflow_run_schema_history_membership_roundtrip_scope_and_bounds

```text
thread 'adapters::persistence::workflow::tests::run_schema_tests::workflow_run_schema_history_membership_roundtrip_scope_and_bounds' (12848210) panicked at src/adapters/persistence/workflow/run_schema_tests.rs:305:6:
called `Result::unwrap()` on an `Err` value: Database(PgDatabaseError { severity: Error, code: "23503", message: "insert or update on table \"workflow_root_budgets\" violates foreign key constraint \"workflow_root_budgets_company_id_root_run_id_fkey\"", detail: Some("Key (company_id, root_run_id)=(33214a65-ed6b-40ef-9674-f2fccf18dfa9, afffc658-d628-4307-bae3-32c573affce6) is not present in table \"workflow_runs\"."), hint: None, position: None, where: None, schema: Some("public"), table: Some("workflow_root_budgets"), column: None, data_type: None, constraint: Some("workflow_root_budgets_company_id_root_run_id_fkey"), file: Some("ri_triggers.c"), line: Some(2783), routine: Some("ri_ReportViolation") })

```
