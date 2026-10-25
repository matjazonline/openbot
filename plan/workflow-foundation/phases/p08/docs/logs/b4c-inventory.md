# B4c legacy-object inventory (read-only Explore agent, 2026-10-24)

> Model output from a read-only Explore agent, not yet verified line by line. B4c verifies each
> item before editing.
>
> - `L<n>` is a line in `migrations/20260817000000_init_schema.sql` (15108 lines) at the B4b freeze
>   (`ac0a8958…63c7`).
> - For constraint, FK and index blocks, the header line is cited.
> - The file has a pg_dump part (L1–14663) and a hand-appended part (L14667–15108): model usage,
>   prices, `task_harness_runs` workflow FKs and the token guards.
> - It defines no enums or domains. Its only sequence is `model_price_revision` (L14730, kept) and
>   its only view is `workflow_action_effect_states` (L6517, workflow-only).

## 1. Legacy-only tables (drop the table, its explicit functions and the FKs and triggers that live on other tables)

### Task bookkeeping
- **task_status_events** (L6064)
  - Indexes: L10669, L10676.
  - FKs: L13432, L13440, L13448.
  - Triggers: L11210, L11378.
  - Its writer is the `background_tasks_record_status_event` trigger (L10878).
- **task_ownership_events** (L6026)
  - FK: L13424.
  - Triggers: L11203, L11343, L11350, L11357.
  - Functions: `task_ownership_events_are_immutable` (L2752), `notification_from_task_ownership` (L1138), `notify_task_ownership` (L1679), `record_initial_task_ownership` (L2152), `release_tasks_for_removed_principal` (L2287).

### Outreach and delegation
- **task_outreaches** (L5991)
  - Indexes: L10648, L10655, L10662.
  - FKs on it: L13400, L13408, L13416.
  - FKs from other tables pointing at it:
    - `background_tasks_awaited_outreach_fk` (L12200)
    - `background_tasks_transition_outreach_fk` (L12256)
    - `delegation_control_commands_outreach_fk` (L12464)
    - L13368, L13328, L13440
  - Triggers: L11099, L11196, L11322, L11329, L11336.
  - Functions: `enforce_outreach_status_transition` (L655), `notification_from_outreach` (L1083).
- **task_outreach_targets** (L5962)
  - Indexes: L10620–10641.
  - FKs: L13352–13392.
  - Triggers: L11308, L11315.
  - Function: `enforce_outreach_target_status_transition` (L687).
- **task_outreach_replies** (L5946)
  - Index: L10613.
  - FKs: L13328–13344.
- **delegation_control_commands** (L4858)
  - Index: L10202.
  - FKs: L12456–12480.
  - Trigger: L11141.

### Approvals, drafts and reviews
- **human_approvals** (L4943)
  - Indexes: L10251–10265.
  - FKs on it: L12536–12560.
  - FKs from other tables pointing at it:
    - `background_tasks_transition_approval_fk` (L12248)
    - `task_approval_waits` FKs (L13232, L13240)
    - L13432
  - Triggers: L10945 (`notify_attention_changed('approval')`), L10952, L11238.
- **task_approval_waits** (L5783)
  - FKs: L13232–13264.
  - Trigger: L11169.
- **response_drafts** (L5544)
  - Indexes: L10521–10535.
  - FKs on it: L13032–13080.
  - FKs from other tables pointing at it: L12992, L13008, L13096, L13112.
  - Triggers: L11249, L11263.
  - Function: `enforce_response_draft_update` (L774).
- **response_draft_evidence** (L5502)
  - Trigger: L11256.
  - Function: `enforce_response_draft_evidence_immutability` (L745).
- **response_draft_publications** (L5527)
  - FKs: L13000–13024.
- **response_review_commands** (L5583)
  - FKs: L13088, L13096.
- **response_reviews** (L5606)
  - Index: L10542.
  - FKs: L13104–13128.
  - Triggers: L11270, L11277, L11284.
  - Function: `notification_from_review` (L1113).

### Task-attached auxiliary tables
All of these are guarded by `require_legacy_auxiliary_task`.
- **start_agent_task_commands** (L5733)
- **task_agent_instructions** (L5759)
  - Function: `notify_agent_instruction` (L1462), fired by trigger L11298.
- **task_agent_instruction_notes** (L5746)
- **task_channel_targets** (L5836)
  - Function: `delete_channel_target_tasks` (L566), fired by trigger L10908 on `channels`.
- **human_task_completions** (L4972)
- **thread_handoff_runs** (L6133)
- **task_harness_invocations** (L5852)
  - FK to `task_harness_runs`: L13296.
  - Only legacy objects reference it.
  - UNSURE.
- **thread_handoffs** (L6154)
  - Triggers: L11392, L11399 (`notify_attention_changed('thread_handoff')`).
- **thread_handoff_events** (L6095)
  - Functions: L2772, L2543.
- **manual_handoffs** (L5117)
  - Triggers: L10938, L11008, L11015.
- **attention_source_events** (L4507)
  - Its `source_kind` is `task` or `handoff`.
  - Functions: L121, `notification_from_attention_source` (L1029).
  - Nothing in `src/` references it.
  - UNSURE.

### Channel agents and sub-agents
- **channel_agents** (L4633)
  - Triggers: L10899, L11106.
  - Functions: `enforce_channel_agent_scope` (L632), `enforce_owned_channel_position_zero` (L708, also used by trigger L11120), `prevent_assigned_library_agent_delete` (L2080, fired by trigger L10994 on `agents`).
- **agent_sub_agents** (L4455)

### Memory
- **memory_provider_connections** (L5172)
  - Triggers: L11022, L11029.
- **memory_provisioning_jobs** (L5189)
  - Trigger: L11036.
- **memory_remote_resource_lifecycles** (L5223)
- **memory_cleanup_jobs** (L5148)
- Functions: L505, L2659, L2728.

### Drop order
The `background_tasks` FKs L12200, L12248 and L12256 go before `task_outreaches` and `human_approvals`.

## 2. Shared tables to trim

### background_tasks (L4536)

**Columns to drop**
- `source_message_uuid`
- `wait_expires_at`
- `transition_*`
- `owner_principal_id`, `owner_principal_kind`
- `ownership_version`
- `business_priority`, `business_due_at`
- `attention_version`
- `awaited_outreach_id`

The `queue_kind` default becomes `'workflow'`.

**Checks**
- Drop: L4573, L4574, L4576, L4578, L4579, L4585, L4586, L4587–4597.
- Rewrite:
  - L4581 `queue_kind`: becomes `'workflow'` only.
  - L4583 `continuation_count`.
  - L4598 `workflow_shape`.
- L4584 `status`: the legacy values are `pending_approval`, `waiting_for_third_party_reply` and `stopped`. `dead_letter` is UNSURE, because the dashboard reads it.
- Keep the lease check (L4575).

**Unique constraint and FKs**
- Drop the unique constraint L7714 `company_source_message_channel_key` and its comment (L7725).
- Drop FKs L12200, L12224, L12232, L12248, L12256.
- Keep FKs L12208, L12240, L12264 and `company_id`.

**Indexes**
- Drop: L10006, L10034, L10041, L10048.
- UNSURE: L10020 `thread_correlation_match_idx`, keyed on legacy task types.
- Keep: L9971–9999, L10013, L10027, L10778.

**Legacy triggers** (`WHEN queue_kind='legacy'`)
- L10829, L10836, L10843, L10850, L10857, L10871, L10878, L11364, L11371.
- On `principals`: L11127, L11134 (`release_tasks_for_removed_principal`).

**Keep**
- L10864 `preserve_queue_identity`, L10885, L14788.
- The workflow guards: L11473, L11550, L11802, L11830, L11837, L11851.

### task_attempts (L5804)
- `stop_reason` check (L5826).
  - Legacy values: `ownership_transferred`, `agent_instruction`, `delegation_cancelled`, `operator_stopped`, `approval_rejected`, `channel_agent_removed`.
  - Values the workflow writes: `completed`, `lease_lost`, `timed_out`, `effect_parked`, `approval_parked`, `child_parked`.
  - UNSURE: `retryable_failure`, `terminal_failure`, `shutdown`.
- UNSURE: `prompt_tokens`, `completion_tokens`, `result`, `error` and the token check (L5827). No non-test writer was found.

### task_harness_runs (L5875)
- Legacy columns: `owner_principal_id`, `ownership_version`.
- `owner_kind` defaults to `'legacy'`. Either drop it or reduce it to `'workflow'`; if it stays, the partial predicates at L14837, L15066 and L15092 stay.
- Constraints:
  - L5907 `owner_shape`: loses its legacy arm.
  - L5901, L5902: simplify.
  - L5925: drop.
  - UNSURE: L5924 and L5927 (`response_contract`).
- Indexes: drop L10599 (`task_id, agent_id, ownership_version`); keep L10606.
- FK: drop L13312.
- Functions:
  - Rewrite `require_harness_owner` (L2397): remove the legacy branch at L2409–2416 and the owner fields of the immutable-row comparison.
  - Adjust `guard_harness_model_owner` (L2456) and `guard_harness_token_pause` (L14969).

### message_deliveries (L5247)
- Drop `task_id` (L5255).
- Constraints and objects tied to `task_id`:
  - L5280 attribution check: remove `task_id IS NULL`. `decision_provenance_tests.rs:168` redefines this constraint.
  - FK L12808, index L10395.
  - Triggers: L11245 `require_legacy_source_task`, L11050 `notify_chain` (`notify_task_chain_changed`).
- L11043 `notify_attention` stays, with its function rewritten (section 3).
- Cancellation check (L5281): drop `channel_agent_removed` and `task_stopped`.
- UNSURE: the purposes `reply`, `mirror` and `outreach`, and `depends_on_delivery_id`. Both are still used by Rust.

### notification_events (L5377) and notifications (L5413)
- Action kinds (L5398, L5430): keep only `delivery_failure`.
- Source kinds (L5404, L5432): keep only `delivery`.

### runtime_metric_samples (L5631)
- Drop:
  - columns: `active_task_executions`, `task_worker_concurrency_limit`, `hydradb_calls`, `hydradb_failures`, `hydradb_duration_ms`;
  - constraints: L5651, L5652, L5656–5659, L5666.
- There are no `ai_*` columns.

### user_notification_preferences (L6240)
- Drop `assignment_email_enabled`, `response_review_email_enabled`, `delegation_timeout_email_enabled`, `task_failure_email_enabled`.
- Keep the delivery-failure and decision columns.

### workflow_decision_reviewers (L6893)
Nothing to trim.

### model_usage_calls (L14678)
Rewrite `guard_model_usage_task` (L14741): drop the legacy arm at L14745.

### internal_notes and tombstones
Kept.

## 3. Functions

### Legacy-only: drop with their triggers
| Line | Function |
| --- | --- |
| L121 | `attention_source_events_are_immutable` |
| L138 | `bump_handoff_version_for_responsibility_cleanup` |
| L156 | `bump_task_attention_version_for_owner_change` |
| L505, L2659, L2728 | the three memory functions |
| L566 | `delete_channel_target_tasks` |
| L632 | `enforce_channel_agent_scope` |
| L655, L687 | the two outreach status-transition guards |
| L708 | `enforce_owned_channel_position_zero` |
| L745, L774 | the two response-draft guards |
| L904 | `initialize_task_ownership` |
| L1029, L1083, L1113, L1138, L1202 | the `notification_from_*` functions for legacy sources |
| L1418 | `notification_withdraw_deleted_source` |
| L1462 | `notify_agent_instruction` |
| L1679 | `notify_task_ownership` |
| L2080 | `prevent_assigned_library_agent_delete` |
| L2134 | `prevent_owned_channel_delete` |
| L2152 | `record_initial_task_ownership` |
| L2181 | `record_task_status_event` |
| L2287 | `release_tasks_for_removed_principal` |
| L2516 | `require_legacy_auxiliary_task` |
| L2543 | `require_legacy_handoff_event_task` |
| L2637 | `require_legacy_source_task` |
| L2752 | `task_ownership_events_are_immutable` |
| L2772 | `thread_handoff_events_are_immutable` |
| L1600 | `notify_task_chain_changed`, once `message_deliveries.task_id` goes; no Rust listener remains |

### Shared: rewrite
- `enqueue_actionable_notification_event` (L809): drop the legacy-source guard at L815–834.
- `notify_attention_changed` (L1491): keep only the `delivery` path. Remove L1500–1508, L1514–1524 and L1525–1530.
- `require_harness_owner` / `guard_harness_model_owner`: see `task_harness_runs` above.
- `guard_model_usage_task`: see `model_usage_calls` above.

### Shared: no change
- `notification_from_delivery` (L1048)
- `notify_thread_activity` (L1727)
- `notify_workflow_run_changed` (L12037)
- the notification recheck and withdraw helpers
- `preserve_background_task_queue_identity` (L1769)
- the `workflow_*` family
- `workflow_continuation_counter_guard` (L14773)

### Source kinds in notify payloads
- `notify_attention_changed` emits `task`, `approval`, `handoff`, `thread_handoff`, `response_review`, `delegation` and `delivery`. Only `delivery` survives.
- Rust's `AttentionWakeSource` has the legacy variants and cannot parse `approval`.

## 4. Views, types and checks
- The only view is workflow-only.
- UNSURE:
  - `thread_messages.entry_kind` allows `'delegation'` (L6189), and it is still produced in Rust.
  - `messages.audience` allows `'legacy_unclassified'` (L5361/5364).

## 5. Legacy columns on surviving tables

### companies (L4721)
- Columns: `default_{retrieve,persist}_*_memory` (L4728–4733), `memory_provider` (L4736), `external_response_review` (L4737), `external_reply_handling` (L4738).
- Checks: L4741–4743.
- UNSURE: `default_add_3rd_party` and `default_participant_emails`.

### channels (L1263)
- Columns: `response_trigger` (L1271), the memory flags (L1273–1278), `owner_agent_id` (L1280), `external_response_review_override` (L1281), `preferred_reviewer_principal_id` (L1282), `external_reply_handling_override` (L1283).
- Checks: L1286, L1287, L1289. Comment: L1297.
- Unique: L7833 `channels_owner_agent_key`.
- FKs: L12368, L12376.
- Triggers: L10908, L11113, L11120.
- UNSURE: `add_3rd_party`.

### agents (L4469)
- Columns: `config_json`, `run_timeout_secs`, the four `memory_*` columns, `response_contract`.
- Checks: L4491, L4493–4495, L4497, L4498.
- Trigger: L10994.
- Keep `workflow_tools`.

### Other UNSURE
- `messages.structured_response` and its body check.
- `valid_creation_provenance` allowing `source_task_id`.
- `workflow_root_budgets` provenance `sealed_legacy`.

## 6. Rust that still touches these objects

### Production code
- **Memory:**
  - `persistence/memory.rs` (whole file);
  - `services/memory_worker.rs`;
  - wiring in `main.rs`, `infra/config.rs`, `infra/setup.rs` and `app_state.rs`;
  - `adapters/memory/{hindsight,hydradb}.rs`.
- **`persistence/company.rs`:** memory defaults, provider, review and reply columns, and the memory lifecycle on delete.
- **`persistence/channel.rs`:**
  - SELECT/INSERT/UPDATE of `owner_agent_id`, `response_trigger`, the memory flags, `preferred_reviewer_principal_id` and the overrides;
  - `channel_agents` reads and writes at ~L145, L385, L737 and L1301.
- **`persistence/channel_assignment.rs`:** the whole module.
- **`persistence/agent.rs`:** `config_json`, `run_timeout_secs`, `memory_*` and `response_contract`; `agent_sub_agents` reads and writes; the `owner_agent_id` lookup.
- **`persistence/workflow/provisioning.rs:620`:** INSERT into `channel_agents`.
- **`persistence/thread/mod.rs:592`:** `task_outreach_targets` exclusion in `find_outbound_reply_after`.
- **`persistence/runtime_metrics.rs`:** the task and HydraDB columns.
- **`persistence/dashboard.rs`:** the legacy statuses in its filters.
- **`persistence/notification.rs` and `entities/notification.rs`:** the legacy source and action variants.
- **`entities/transport/delivery.rs`:** `ChannelAgentRemoved`, `TaskStopped`.
- **`entities/task.rs`:** the legacy `TaskStatus` variants.
- **Pages and routes using these struct fields:**
  - `channel_settings`, `channels`, `agent_settings`, `companies`, `company_settings`;
  - routes `company`, `ui_companies`, `agent`;
  - `use_cases/agent.rs`.

### Schema assertion that breaks
- `persistence/mod.rs:226–290` `clean_schema_contains_only_the_canonical_message_spine` requires `source_message_uuid` and its key and FK.

### Tests and fixtures
- `constraint_tests.rs`
- `skill_tests.rs:465,564`
- `thread/tests.rs:1104–1163`
- `thread/inbound_tests.rs`
- `agent_config_tests.rs`
- `delivery/cancellation_tests.rs`
- `delivery/tests.rs`
- `channel_response_trigger_tests.rs`
- the workflow `job_*` tree, including `job_auxiliary_fixtures.rs` (12 legacy tables) and `job_notification_fixtures.rs`
- `schedule_runtime_tests.rs:133`
- `message_ingress_tests.rs:412` (counts `queue_kind='legacy'`)
- `harness_owner_tests.rs`
- `decision_provenance_tests.rs:168`
- `provisioning_dispatch_tests.rs:277`
- the memory and company tests
