# Remaining legacy query audit inventory

Unchecked source groups from the complete persistence inventory; test/fixture groups may be discarded only after inspecting their scope. Core queue.rs/operations.rs and channel_gate::task_gate_key_on are separately reviewed. Other symbols in channel_gate remain due. Workflow modules are excluded because they own the new workflow path. Application/HTTP/nullable inventories remain as listed in JOB-SLICE-03.1.md.

- [x] `src/adapters/persistence/agent_channel.rs` `provisioning_is_atomic_attributed_and_idempotent` L288-L448 — lines 347, 351
- [x] `src/adapters/persistence/approval/transitions.rs` `transition_approval` L119-L169 — lines 149
- [x] `src/adapters/persistence/approval/transitions.rs` `decision_note` L256-L282 — lines 263
- [x] `src/adapters/persistence/approval/transitions.rs` `lock_subject` L14-L31 — lines 20
- [x] `src/adapters/persistence/approval/transitions.rs` `link_wait` L33-L116 — lines 46
- [x] `src/adapters/persistence/approval/transitions.rs` `settle_task` L191-L236 — lines 207
- [x] `src/adapters/persistence/approval.rs` `create_approval` L105-L219 — lines 165, 175
- [x] `src/adapters/persistence/approval.rs` `consume_quorum_timeout_action` L295-L537 — lines 345, 351, 412, 438, 490
- [x] `src/adapters/persistence/attention.rs` `ATTENTION_SQL` L87-L380 — lines 118, 189, 241, 268, 299, 327
- [x] `src/adapters/persistence/attention.rs` `change_source_attributes` L711-L903 — lines 764, 829
- [x] `src/adapters/persistence/attention.rs` `operational_summary` L999-L1203 — lines 1024, 1087, 1097, 1118, 1126, 1135, 1145, 1152, 1176
- [x] `src/adapters/persistence/channel_assignment.rs` `withdraw_removed_agents_work_on` L293-L383 — lines 349
- [x] `src/adapters/persistence/channel_assignment.rs` `affected_tasks_on` L399-L462 — lines 408, 436, 446
- [x] `src/adapters/persistence/channel_assignment.rs` `module level`  — lines 389
- [x] `src/adapters/persistence/company_invite.rs` `ask` L991-L1052 — lines 994
- [x] `src/adapters/persistence/company_invite.rs` `owned_task_on` L879-L896 — lines 882
- [x] `src/adapters/persistence/company_invite.rs` `owned_tasks_at_stake` L450-L488 — lines 459
- [x] `src/adapters/persistence/company_invite.rs` `delegated_asks_at_stake` L496-L551 — lines 510
- [x] `src/adapters/persistence/company_invite.rs` `tests` L603-L1539 — lines 1067
- [x] `src/adapters/persistence/company_invite.rs` `removing_a_member_releases_the_live_work_they_still_own` L1071-L1191 — lines 1085, 1106, 1136, 1153
- [x] `src/adapters/persistence/dashboard.rs` `module level`  — lines 98, 246, 279, 281
- [x] `src/adapters/persistence/dashboard.rs` `TASK_QUEUE_SQL` L99-L105 — lines 102
- [x] `src/adapters/persistence/dashboard.rs` `TASK_PRESSURE_SQL` L112-L121 — lines 120
- [x] `src/adapters/persistence/dashboard.rs` `THROUGHPUT_BODY` L190-L206 — lines 195
- [x] `src/adapters/persistence/dashboard.rs` `LATENCY_BODY` L219-L241 — lines 230, 231
- [x] `src/adapters/persistence/dashboard.rs` `QUEUE_DEPTH_BODY` L259-L277 — lines 262
- [x] `src/adapters/persistence/dashboard.rs` `ATTEMPT_STATS_SQL` L290-L307 — lines 304, 305
- [x] `src/adapters/persistence/dashboard.rs` `RETRY_RATE_BODY` L311-L327 — lines 316, 317
- [x] `src/adapters/persistence/dashboard.rs` `OUTSTANDING_SQL` L339-L369 — lines 354
- [x] `src/adapters/persistence/mod.rs` `clean_schema_contains_only_the_canonical_message_spine` L215-L311 — lines 243, 244, 261, 271, 293, 294, 295, 304, 305, 306
- [x] `src/adapters/persistence/notification.rs` `source_snapshot` L639-L832 — lines 648, 709, 738, 768, 800
- [x] `src/adapters/persistence/response_review/commands.rs` `execute_command` L14-L219 — lines 39
- [x] `src/adapters/persistence/response_review/commands.rs` `approve_on` L267-L359 — lines 322
- [x] `src/adapters/persistence/response_review/commands.rs` `reject_on` L385-L437 — lines 417
- [x] `src/adapters/persistence/response_review/commands.rs` `expire_locked_review` L475-L506 — lines 487
- [x] `src/adapters/persistence/response_review.rs` `expire_due` L751-L791 — lines 765
- [x] `src/adapters/persistence/response_review.rs` `resolve_reviewer_on` L320-L372 — lines 327
- [x] `src/adapters/persistence/response_review.rs` `validate_evidence_on` L414-L520 — lines 478, 479, 492
- [x] `src/adapters/persistence/response_review.rs` `evidence_openable` L920-L1004 — lines 992
- [x] `src/adapters/persistence/schedule.rs` `list_schedule_runs` L577-L614 — lines 599
- [x] `src/adapters/persistence/schedule.rs` `schedule_run_contains_thread` L616-L641 — lines 625
- [x] `src/adapters/persistence/task/harness_runs.rs` `lock_task_execution_on` L11-L35 — lines 20
- [x] `src/adapters/persistence/task/harness_runs.rs` `resolve_harness_outreach_on` L593-L659 — lines 600, 654
- [x] `src/adapters/persistence/task/board.rs` `board_query_sql` L119-L286 — lines 164, 181
- [x] `src/adapters/persistence/task/controls.rs` `revoke_internal_child` L234-L304 — lines 250, 277, 290
- [x] `src/adapters/persistence/task/controls.rs` `wake_task` L306-L341 — lines 315, 321, 322, 323
- [x] `src/adapters/persistence/task/board.rs` `chain_detail_on` L474-L715 — lines 491, 524, 545, 546, 629, 672
- [x] `src/adapters/persistence/task/collaboration.rs` `load_collaboration_tasks` L527-L594 — lines 536, 554, 576
- [x] `src/adapters/persistence/task/collaboration.rs` `load_collaboration_targets` L596-L659 — lines 639
- [x] `src/adapters/persistence/task/collaboration.rs` `collaboration_summary_on` L661-L742 — lines 673
- [x] `src/adapters/persistence/task/controls.rs` `apply_operation` L640-L945 — lines 684, 906, 919
- [x] `src/adapters/persistence/task/controls.rs` `execute_delegation_command_on` L951-L1067 — lines 976, 1015
- [x] `src/adapters/persistence/task/instructions.rs` `lock_instruction_task` L148-L166 — lines 154
- [x] `src/adapters/persistence/task/instructions.rs` `requeue_processing_task` L226-L277 — lines 233, 265
- [x] `src/adapters/persistence/task/instructions.rs` `ensure_thread_has_no_active_task` L389-L413 — lines 394
- [x] `src/adapters/persistence/task/ownership.rs` `change_task_ownership_on` L261-L503 — lines 297, 411, 446
- [x] `src/adapters/persistence/task/board.rs` `BOARD_ELIGIBLE_RECENT` L35-L65 — lines 38, 53, 60
- [x] `src/adapters/persistence/task/board.rs` `BOARD_ELIGIBLE_EVERY_CHAIN` L73-L91 — lines 75, 79, 86
- [x] `src/adapters/persistence/task/counts.rs` `COUNTS_QUERY` L12-L20 — lines 14
- [x] `src/adapters/persistence/task/counts.rs` `all_statuses_visibility_and_current_ownership_agree_with_sql` L77-L183 — lines 167
- [x] `src/adapters/persistence/task/instructions.rs` `claim_agent_instruction_notes` L598-L677 — lines 606
- [x] `src/adapters/persistence/task/rows.rs` `module level`  — lines 1
- [x] `src/adapters/persistence/thread/inbound.rs` `tasks_for_message` L223-L240 — lines 230
- [x] `src/adapters/persistence/thread/reply_publication.rs` `source_threads_on` L63-L89 — lines 69
- [x] `src/adapters/persistence/thread/views.rs` `module level`  — lines 175
- [x] `src/adapters/persistence/thread/views.rs` `THREAD_TASK_LOOKUP_SQL` L177-L197 — lines 182, 189

Task-module additional entry audit: board::chain_status_events_query and instructions::ask_owner_to_act replay corrected and independently reviewed; harness independent checkpoint reads/supersession, ownership event reads, outreach tally and MCP journal checked through guarded execution callers. Remaining channel_gate functions only acquire advisory keys. No auxiliary DB association guard acceptance is implied. Final gate evidence in JOB-SLICE-03.1.md.

Projection-slice fixture audit: agent_channel provisioning and company_invite helper/removal SQL
are scoped legacy fixture inserts/updates/assertions; mod.rs reads only schema catalogs;
counts.rs ownership UPDATE is confined to its fixture company/principals. No production consumer
is hidden in these groups. Production projection groups await independent review.

External projection production groups passed independent actual-code/correction review. This
checks the bounded query audit only; final gate evidence is recorded in JOB-SLICE-03.1.md.

External projection final performance correction: additive90000index restores the original bounded
lookup guard and adds workflow-heavy custom/generic plan coverage. Independent actual-code PASS;
final gates PASS1939full+5projection+4lookup, offline/Clippy/SQLx/fmt/diff/graft. Full evidence and
unchanged remaining scope in JOB-SLICE-03.1.md; checkmarks do not enable workflow jobs or accept03.1.
