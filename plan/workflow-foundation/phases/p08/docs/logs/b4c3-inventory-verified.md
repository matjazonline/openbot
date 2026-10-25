# B4c-3 inventory, verified against the working tree (2026-10-24)

Read-only Explore pass over the uncommitted tree after B4c-2 (digest `97531db8…802d`), checking
`logs/b4c-inventory.md` §1–6 and D4.8/D4.3. `M` = `migrations/20260817000000_init_schema.sql`
(14398 lines at the time; line numbers drift with every edit — re-grep before use). Nothing was
compiled or run. Model output: verify each item before acting on it.

## Corrections to the old inventory
- Already gone: memory tables, `channel_agents`, `agent_sub_agents`, the B4c-2 functions,
  `runtime_metric_samples.hydradb_*` (no `ai_*` columns exist); channels/agents/companies have no
  legacy columns left.
- Already done for D4.3: `workflow_decision_reviewers.notice_opted_out` + check;
  `user_notification_preferences.decision_email_enabled`; `NewDelivery.task_id` and
  `require_legacy_task_on` gone from Rust; attention `delivery_failure` no longer joins tasks.
- Only production Rust still touching legacy schema: `persistence/thread/mod.rs:563-566`
  (`find_outbound_reply_after`, a `task_outreach_targets` NOT EXISTS clause).

## background_tasks
- No workflow code reads/writes `queue_kind='legacy'`; inserts set `'workflow'`
  (`admission_write.rs:145`, `batch_commit.rs:84`). `'legacy'` literals only in tests
  (`schedule_runtime_tests.rs:133`, `message_ingress_tests.rs:411`, `job_schema_tests.rs:213`).
- KEEP columns: id, company_id, channel_id, thread_id, correlation_id, task_type
  (`'workflow_execution'`), status, payload, retry_count, continuation_count, attempt_ordinal,
  max_retries, last_error, worker_id, execution_generation, locked_at, lock_expires_at, run_at,
  created_at, updated_at, queue_kind, workflow_execution_id.
- DROP columns: source_message_uuid, wait_expires_at, transition_reason/actor_kind/actor_id/
  approval_id/outreach_id, owner_principal_id/kind, ownership_version, business_priority,
  business_due_at, attention_version, awaited_outreach_id (`attention.rs` `business_priority` is a
  synthetic alias, not the column).
- Statuses written by workflow: pending, processing, completed, failed. `pending_approval`,
  `waiting_for_third_party_reply`, `stopped`, `dead_letter` have no workflow writer; `dead_letter`
  is read by dashboard filters (`persistence/dashboard.rs:197,200,371-376`,
  `entities/dashboard.rs:299`) — legacy leftover. UNSURE (product): whether the dashboard should
  flag `failed` jobs once `dead_letter` goes.
- Checks: DROP attention_version, business_priority, legacy_channel_required, owner_shape,
  ownership_version, transition_actor_kind, transition_reason, transition_shape. REWRITE
  queue_kind (`'workflow'` only; default too), continuation_count (simplify), status (4 values),
  workflow_shape (drop legacy arm + NULL conjuncts; `workflow_execution_id` NOT NULL optional).
  KEEP lease_check, max_retries, payload_object, retry_count.
- Constraints: DROP unique company_source_message_channel_key (+comment), FKs awaited_outreach,
  owner_principal, source_message, transition_approval, transition_outreach. KEEP company_id_id_key,
  pkey, workflow_action_job_scope_key, FKs channel, company_id, thread, workflow_execution.
- Indexes: DROP pending_ready_idx, thread_correlation_match_idx, unsettled_owner_idx,
  waiting_due_idx. UNSURE unsettled_channel_idx (legacy statuses in predicate, no reader found).
- Triggers: DROP bump_attention_version, notify_activity, notify_attention,
  notify_attention_delete, record_initial_ownership, record_status_event,
  task_status_actionable_notification (+ _delete), and principals_release_owned_tasks
  (+ _on_demotion). KEEP preserve_queue_identity, validate_workflow_association,
  workflow_action_capture_schedule, workflow_action_confirm_claim_retirement,
  workflow_control_retry_guard, workflow_fenced_completion_guard, workflow_fenced_retirement_guard,
  workflow_job_revision, workflow_continuation_counter_guard.

## Legacy-only tables (all DROP; no surviving FK into them except background_tasks' above)
task_status_events, task_ownership_events, task_outreaches, task_outreach_targets,
task_outreach_replies, delegation_control_commands, human_approvals, task_approval_waits,
response_drafts, response_draft_evidence, response_draft_publications, response_review_commands,
response_reviews, start_agent_task_commands, task_agent_instructions,
task_agent_instruction_notes, task_channel_targets (+ `channels_delete_target_tasks` trigger),
human_task_completions, thread_handoff_runs, task_harness_invocations, thread_handoffs,
thread_handoff_events, manual_handoffs, attention_source_events.

## Functions
- DROP (legacy-only, with triggers): attention_source_events_are_immutable,
  bump_handoff_version_for_responsibility_cleanup, bump_task_attention_version_for_owner_change,
  delete_channel_target_tasks, enforce_outreach_status_transition,
  enforce_outreach_target_status_transition, enforce_response_draft_evidence_immutability,
  enforce_response_draft_update, notification_from_attention_source, notification_from_outreach,
  notification_from_review, notification_from_task_ownership, notification_from_task_status,
  notification_withdraw_deleted_source, notify_agent_instruction, notify_task_chain_changed (Rust
  never listens on task_chain_changed/task_ready/task_ownership_changed), notify_task_ownership,
  record_initial_task_ownership, record_task_status_event, release_tasks_for_removed_principal,
  require_legacy_auxiliary_task, require_legacy_handoff_event_task, require_legacy_source_task,
  task_ownership_events_are_immutable, thread_handoff_events_are_immutable.
- REWRITE: enqueue_actionable_notification_event (drop the legacy preservation block);
  notify_attention_changed (drop workflow-task skip, draft_id fallback, response_review and
  delegation branches; only caller left `message_deliveries_notify_attention`);
  require_harness_owner (drop owner_principal_id/ownership_version from ROWs, legacy branch);
  guard_model_usage_task (drop the `run_id IS NULL AND queue_kind='legacy'` arm; optional
  `model_usage_calls` num_nonnulls `IN (0,5)` → `=5`; `model_usage/mod.rs:39-50` LEFT JOINs).
- Optional dead-branch cleanup: guard_harness_model_owner, guard_harness_token_pause,
  advance_workflow_owned_revision. KEEP workflow_continuation_counter_guard.
- KEEP: notification_from_delivery, notify_thread_activity (workflow triggers use it; only legacy
  trigger goes), notify_actionable_notification_changed, notify_internal_note_change,
  notification_recheck_*, notification_withdraw_deleted_principal,
  withdraw_unauthorized_notifications, notification_principal_can_view,
  preserve_background_task_queue_identity, preserve_delivery_cancellation,
  validate_workflow_job_association, notify_workflow_run_changed, all workflow_*.
- UNSURE: valid_creation_provenance `agent` arm (needs source_task_id; `CreationProvenance::agent`
  has no caller) — outside the B4c list.

## Shared tables
- task_attempts: KEEP. stop_reason check → completed, effect_parked, child_parked,
  approval_parked, lease_lost, timed_out (+NULL). DROP prompt_tokens, completion_tokens, result,
  error, token_check (only test writer `job_projection_tests.rs:31`; dashboard tokens come from
  model_usage_calls).
- task_harness_runs: DROP owner_principal_id, ownership_version, ownership_version_check, legacy
  owner_shape arm, unique task_harness_runs_scope, the owner principal FK. owner_kind: keep with
  `='workflow'` CHECK (default currently 'legacy'; Rust writes 'workflow' explicitly at
  `checkpoints.rs:383` and filters on it). KEEP invalid_output and response_contract (live:
  `application/workflow/agent.rs:205`, `harness/rig/final_response.rs`).
- message_deliveries: DROP task_id, its attribution term, FK, index, triggers
  require_legacy_source_task and notify_chain, cancellation reasons channel_agent_removed and
  task_stopped. KEEP notify_attention (function rewritten), preserve_cancellation. UNSURE (not in
  D4.8): purposes reply/mirror/outreach and depends_on_delivery_id have no production writer
  (`decision_provenance_tests.rs:173` injects `'reply'` as a fault).
- notification_events / notifications: action → delivery_failure only; source → delivery only.
- user_notification_preferences: DROP assignment, response_review, delegation_timeout,
  task_failure flags (`pages/tests.rs:4107` asserts their absence — keep).
- runtime_metric_samples: REWRITE active_task_executions, task_worker_concurrency_limit and their
  checks into workflow active jobs + slots. Readers/writers: `persistence/runtime_metrics.rs`,
  `entities/runtime_metrics.rs` (`TaskWorkerConcurrencyRecommendation`),
  `infra/runtime_metrics.rs`, `services/runtime_metrics.rs` (`ActiveTaskExecutions`, never
  incremented), `main.rs:193-219`, `infra/config.rs` (`TASK_WORKER_CONCURRENCY`),
  `pages/dashboard_runtime.rs`, `.env.example`, `fly.toml`, `docs/deploy.md`, `README.md`.
  Replacement source: `WORKFLOW_WORKER_SLOTS` (`infra/workflow.rs`, `infra/setup.rs`).
- KEEP internal_notes + tombstones, thread_messages `'delegation'` (live channel relays),
  messages.audience `'legacy_unclassified'`, workflow_root_budgets `'sealed_legacy'`.

## Rust enums / leftovers
- TaskStatus legacy variants (`entities/task.rs`; dashboard pages/persistence/entities) — DROP,
  rewrite dashboard SQL literals.
- NotificationSourceKind::{Task,Handoff,ResponseReview,Delegation},
  NotificationActionKind::{Assignment,ResponseReview,DelegationTimeout,TaskFailure} — DROP.
- DeliveryCancellationReason::{ChannelAgentRemoved,TaskStopped} — DROP with the check
  (`cancellation_tests.rs:432` compares enum and constraint).
- AttentionWakeSource::{Task,Handoff,ThreadHandoff,ResponseReview,Delegation} — DROP.
- `thread/mod.rs:560-566` clause — DROP.
- `persistence/mod.rs` `clean_schema_contains_only_the_canonical_message_spine` — REWRITE
  (source_message_uuid is not workflow-used).

## Tests
- Delete with schema: `persistence/constraint_tests.rs`; `workflow/job_routine_tests.rs` and its
  children `job_source_association_tests.rs`, `job_auxiliary_association_tests.rs`,
  `job_auxiliary_fixtures.rs`, `job_handoff_history_tests.rs`, `job_notification_fixtures.rs`
  (declared at `job_schema_tests.rs:5`); in `job_schema_tests.rs` the tests
  `opens_only_with_legacy_channel_and_association_guards` and
  `does_not_create_legacy_ownership_or_status_events`.
- Rewrite: `job_schema_tests::rejects_foreign_execution_and_legacy_link`;
  `job_association_tests.rs` tail assert; `job_core_query_tests::mixed_fixture` legacy row and
  `job_projection_tests.rs` (legacy loop, prompt_tokens); `completion_refusal_tests.rs` legacy
  half; `schedule_runtime_tests.rs` and `message_ingress_tests.rs` legacy counts;
  `harness_owner_tests.rs` legacy cases; `thread/tests.rs`
  `find_outbound_reply_after_sees_answers_and_ignores_outreach_mail` legacy setup;
  `delivery/cancellation_tests.rs:173` (`'task_stopped'`); `decision_provenance_tests.rs` only if
  `reply` dropped; `notification_tests.rs:102` (task → delivery; unverified); events/live_updates/
  ui_stream/notifications/dashboard tests for trimmed enums; runtime_metrics tests.
- Keep: `agent_config_tests.rs` (extend for B4c-3), `pages/tests.rs:4107`,
  `thread/inbound_tests.rs:365`.
