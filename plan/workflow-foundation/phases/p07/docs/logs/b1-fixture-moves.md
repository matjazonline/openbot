# B1 fixture moves (07-D1 rev 4 §D1.2a, F008)

Every test that constructed a child (`TriggerSource::Child`, `ChildCause::Execution`, or a helper
that did) at the review base `2d608f2`, with its category now. Method: each listed file at HEAD
and in the working tree was split into functions; a test is affected when it reaches a child
constructor through any same-file helper, and its category is read from what it reaches now
(`chain`/`spawn`/`caller`/`calling`/`chained`/`limited_chain`/`limited_child`/`child_fixture`
→ A; `lineage_child`, directly or through `eligibility::child`, `claim_fairness::unique_child`,
`harness_token_refusal::{child, child_checkpoint}` → B). All 60 pre-change names were checked
present in `baseline-workflow-tests.log`. No assertion was weakened; setups and identities
changed, and the error kind of a child admission changed from `NotFound` to `BadRequest`
(service refusal, D1.2a).

Categories: **A** runtime chain (`AdmissionFixture::chain`, production `park_child`);
**B** lineage-only child (`lineage_child`: production `children::create_on`, settled
`cancelled` parent wait, for states the runtime never produces); **C** retired, with the named
replacement.

## Persistence (`src/adapters/persistence/workflow/`)

| File | Test (at base) | Cat. | Now / reason |
| --- | --- | --- | --- |
| admission_token_tests | workflow_admit_token_child_inherits_root_reviewers_and_rechecks_them | C | → `workflow_admit_token_child_admission_refused_and_runtime_child_shares_root_policy` (A): child admission refused; a chain child's token gate runs under the root policy. |
| admission_source_tests | workflow_admit_sql_parent_source_checks_run_execution_step_and_action_is_unsupported | C | → `workflow_admit_refuses_every_child_trigger_and_schema_checks_runtime_child_keys` (A): every `Child` cause refused; 6-element runtime source keys checked in the schema. |
| admission_binding_tests | workflow_admission_source_key_has_stable_versioned_identity | C | Updated to the runtime 6-element child key (`[child, run, exec, step, action\|null, ordinal\|null]`); no child run needed. |
| activation_tests | workflow_activation_parent_metadata_comes_from_admitted_cause | A | Renamed `workflow_activation_parent_metadata_comes_from_run_lineage`: `/run/parent_id` read from `parent_run_id` of a chain child. |
| control_tests | workflow_control_retry_refuses_unknown_exhaustion_deadline_completed_and_child | A | Kind 4 uses `lease_tests::child_fixture` (same I/O step as a parked root's chain child). |
| control_guard_tests | workflow_control_child_admission_competes_and_existing_child_replays | C | → `workflow_control_cancel_competes_with_child_creation` (A): cancel vs competing `park_child`. |
| control_acceptance_tests | workflow_control_child_admission_both_deterministic_orders | C | → `workflow_control_cancel_and_child_creation_both_deterministic_orders` (A): cancel vs `park_child`, both orders (D1.5). |
| budget_race_tests | workflow_budget_runtime_descendants_share_without_locking_root_or_parent | B | A child holding its own claim while its descendant runs (ancestors are always parked); guards shared usage-row locking. |
| budget_activation_tests | workflow_budget_activation_competing_descendants_replay_and_retirement | B | Three runs of one root activating at once; guards the shared usage-row protocol. |
| budget_activation_tests | workflow_budget_activation_pure_io_wait_refusal_no_hot_poll | A | `limited_chain`: parking the root on its child spends the only activation. |
| budget_activation_tests | workflow_budget_activation_child_ignores_held_ancestor_locks | A | `limited_chain` three levels; root and child parked. |
| budget_activation_tests | workflow_budget_activation_deferred_commit_failure_preserves_freeze_and_refusal | B | **Deviation from D1.2a's expectation (A).** The `consumed == false` case needs a child activating before its root, which the runtime never produces (park activates the root first); guards deferred-commit rollback of the shared usage. |
| budget_schema_tests | workflow_budget_schema_competing_descendants_and_exact_duplicate_receipts | B | Three runs of one root reserving at once; guards the shared usage-row protocol. |
| budget_schema_tests | workflow_budget_schema_rollback_scope_mutation_and_owner_cascade | A | `limited_child`: a root parked on one child. |
| budget_schema_tests | workflow_budget_schema_consumption_never_locks_root_run_or_execution | A | `limited_child`. |
| wakeup_tests | workflow_wakeup_completion_has_no_parent_run_or_execution_lock | A | `chained`: root parked at its `workflow.call`. |
| wakeup_tests | workflow_wakeup_real_child_completion_and_parent_cancel_contenders | A | `chained`. |
| wakeup_tests | workflow_wakeup_fenced_io_and_timer_completion_use_same_fact | A | `chained`. |
| wakeup_tests | workflow_wakeup_write_and_commit_failures_rollback_progression | A | `chained`. |
| wakeup_tests | workflow_wakeup_terminal_transitions_are_immutable_and_noops_do_not_repeat | A | `chained`; a parked parent is resumed (`resume_wait`) instead of advanced. |
| wakeup_tests | workflow_wakeup_scope_bounds_and_parent_identity_guards | A | `chained`. |
| wakeup_guard_tests | workflow_wakeup_sibling_progress_and_cursors_are_independent | B | Children of consecutive executions while their parent keeps progressing; guards fact scoping. |
| wakeup_guard_tests | workflow_wakeup_actual_wait_expiry_produces_failed_fact | A | Chain child. |
| wakeup_guard_tests | workflow_wakeup_rejects_forged_duplicate_and_overridden_transition_facts | A | Chain child. |
| wakeup_guard_tests | workflow_wakeup_parent_composite_fk_rejects_wrong_run_execution_pair | A | Chain child; raw negative insert keeps its immediate error. |
| wakeup_guard_tests | workflow_wakeup_parent_composite_fk_rejects_valid_foreign_parent | A | Chain child; raw negative insert keeps its immediate error. |
| wakeup_parent_link_tests | workflow_wakeup_parent_link_is_immutable_and_owner_cascades | B then A | First part B: the lineage guard of a parent execution that never activated (park activates first), else the activation receipt's guard masks the cross-tree FK under test. Owner-cascade part uses a fresh chain (A), deleting the parent wait first. |
| harness_token_refusal_tests | workflow_harness_token_sibling_usage_reparks_exact_request_without_debit | B | `refusal::child`: a descendant running beside its agent ancestor, which keeps its claim and token state; guards the shared root token budget. |
| harness_token_refusal_tests | workflow_harness_token_approved_request_reparks_when_usage_itself_exceeds_allowance | B | Same. |
| harness_token_refusal_tests | workflow_harness_token_inflight_settlement_retains_truth_after_sibling_denial | B | Same. |
| harness_token_refusal_tests | workflow_harness_token_refusal_stops_admitted_descendants_and_inflight_output | B | Same. |
| harness_token_scheduling_tests | workflow_harness_token_pending_pause_preserves_attempts_and_approval_releases | B | `refusal::child`; same reason. |
| harness_token_scheduling_tests | workflow_harness_token_processing_pause_charges_one_failure_only | B | Same. |
| harness_token_scheduling_tests | workflow_harness_token_pause_last_attempt_terminalizes | B | Same. |
| harness_token_scheduling_tests | workflow_harness_token_deferred_sibling_retires_after_denial | B | Same. |
| harness_token_scheduling_tests | workflow_harness_token_competing_deferred_claims_do_not_lock_ancestor | B | Same. |
| harness_token_scheduling_tests | workflow_harness_token_pure_pending_reports_deferral_and_resumes | B | Same. |
| harness_token_scheduling_tests | workflow_harness_token_deferral_is_capped_by_run_deadline | B | Same. |
| harness_token_scheduling_tests | workflow_harness_token_pause_cannot_replace_hard_model_budget_failure | B | Same. |
| harness_checkpoint_update_tests | workflow_harness_update_retains_result_after_refusal_but_cannot_prepare_next | B | `refusal::child_checkpoint`; same reason. |
| action_reconciliation_eligibility_tests | workflow_action_reconciliation_eligibility_claim_rechecks_shared_root_budget | B | `eligibility::child`: a descendant running beside an ancestor in reconciliation or holding a claim; guards shared root budget checks. |
| action_reconciliation_claim_fairness_tests | workflow_action_reconciliation_claim_fairness_descendant_first_claim_with_ancestors_locked | B | `unique_child`: each ancestor stays in its own I/O state; guards the descendant claim policy. |
| action_reconciliation_claim_race_tests | workflow_action_reconciliation_claim_race_debit_first_refuses_both_claimants | B | `eligibility::child`. |
| action_reconciliation_claim_race_tests | workflow_action_reconciliation_claim_race_claim_first_dispatches_once | B | Same. |
| action_reconciliation_claim_race_tests | workflow_action_reconciliation_claim_race_rolled_back_debit_preserves_allowance | B | Same. |
| action_reconciliation_claim_sql_tests | workflow_action_reconciliation_claim_sql_eligible_and_caller_labelled_refusals | B | Same. |
| action_reconciliation_claim_sql_tests | workflow_action_reconciliation_claim_sql_historical_audit_without_current_retirement | B | Same. |
| action_reconciliation_claim_sql_tests | workflow_action_reconciliation_claim_sql_consumed_episode_refusal | B | Same. |
| action_reconciliation_claim_sql_tests | workflow_action_reconciliation_claim_sql_wrong_kind_with_current_retirement | B | Same. |
| action_reconciliation_claim_sql_tests | workflow_action_reconciliation_claim_sql_same_transaction_audit_protection | B | Same. |
| action_reconciliation_claim_sql_tests | workflow_action_reconciliation_claim_sql_actual_refusal_later_audit_protection | B | Same. |
| action_reconciliation_claim_sql_wrong_execution_tests | workflow_action_reconciliation_claim_sql_wrong_execution_with_current_retirement | B | Same. |
| action_reconciliation_claim_boundary_tests | workflow_action_reconciliation_claim_boundary_receipt_only_model_headroom | B | Same (sibling descendant). |
| action_reconciliation_claim_recovery_tests | workflow_action_reconciliation_claim_recovery_lost_response_automatic_saved_retry | B | Same. |
| action_reconciliation_claim_recovery_tests | workflow_action_reconciliation_claim_recovery_late_receipt_after_exhaustion | B | Same. |
| action_reconciliation_claim_recovery_tests | workflow_action_reconciliation_claim_recovery_returned_result_completes_after_exhaustion | B | Same. |
| action_reconciliation_owner_lifecycle_tests | workflow_action_reconciliation_owner_lifecycle_scheduled_and_refused | B | Same. |

Also changed, not in the D1.2a inventory: `lease_tests` gains `lease_source`/`child_fixture`
(category A support for `control_tests` kind 4).

## Application (`src/application/workflow/tests/`)

| File | Test (at base) | Cat. | Now / reason |
| --- | --- | --- | --- |
| causality_cases | four_sources_share_run_head_and_claim_provenance | C | The child source is now refused (`BadRequest`, no run); claims count 3. |
| causality_cases | missing_foreign_or_inconsistent_sources_fail_before_any_write | C | The missing-parent child case is now the service refusal (`BadRequest`, was `NotFound`). |
| causality_cases | child_action_must_match_its_authoritative_execution_and_run | C | → `child_triggers_are_refused_before_any_lookup`: real, forged and step causes refused with the source reader failing, so the refusal precedes any lookup. |
| source_validation (memory store, not a test) | `TriggerSource::Child` branch | C | Mirrors the SQL owner: refuses `Child` (`BadRequest`); the store's parent-execution/action maps are removed. |

## New B1 tests (not moves)

`child_runtime_tests` (11), `child_repeat_tests` (4), `child_schema_tests` (2),
`child_publication_tests` (3), `rerun_tests` (4); after R005: `child_configuration_tests` (2),
`child_decision_round_tests` (1), `child_output_tests` (2), plus registry/fixture unit tests listed
in TASK.
