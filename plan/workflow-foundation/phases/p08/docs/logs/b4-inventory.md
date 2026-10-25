# B4 removal inventory

A read-only Explore agent produced this on 2026-10-23, over the working tree at `da86871` plus
the uncommitted B1–B3b-1 changes. Root saved it verbatim in substance. It builds on
`logs/inventory.md`.

**Since `inventory.md`:**
- B1: the inbound commit writes intents only, with no tasks, holds or outreach.
- B2: scheduled dispatch is gone. Only the string `scheduled_agent_run` remains, in the sort key
  at `thread/views.rs:193` and in the index `background_tasks_thread_correlation_match_idx`.
- B3: added the `infra/workflow.rs` composition, assistant channels and the workflow routes.

**Remaining production producers of legacy tasks:** only `start_agent_task`:
- `ui_internal_notes.rs:358,528`
- `thread_handoffs.rs:297`
- `use_cases/thread/mod.rs:1351`

All of them lead to `persistence/task/instructions.rs:443 insert_task`. `enqueue_task` has no
production caller.

Legend: **D** = delete with the area. **R→X** = retarget to the new owner X.

## A1. TaskWorker legacy queue and `email_agent_dispatch`

- **Files:**
  - `application/services/task_worker.rs` (2571 lines; production code ≈1–1218);
  - `application/task_queue.rs` 717;
  - `application/transport/task_payload.rs`;
  - `use_cases/thread/dispatch.rs` 1567 and `reload.rs` 261;
  - the task parts of `use_cases/thread/mod.rs`: :2067 durable_task_payload, :1444-1488 list
    tasks, :1489, :1497 thread_work_summary, :1516 change_task_ownership, :479 harness
    diagnostics;
  - `adapters/persistence/task/` (8 093 lines);
  - `persistence/legacy_task.rs`;
  - `entities/task.rs` 1891, `stuck_work.rs`, `member_removal.rs`;
  - `infra/events.rs` TaskWakeups :112-150.
- **Schema:** `background_tasks` is shared with the workflow queue.
  - Legacy-only columns: source_message_uuid, owner_principal_id/kind, ownership_version,
    business_priority/due_at, attention_version, awaited_outreach_id, wait_expires_at,
    transition_*.
  - The legacy statuses and `queue_kind 'legacy'`.
  - The checks `_legacy_channel_required`, `_owner_shape`, `_ownership_version`,
    `_attention_version`, `_business_priority`, `_transition_*`.
  - The ON CONFLICT target `(company_id, source_message_uuid, channel_id)`.
  - The index `background_tasks_thread_correlation_match_idx`.
  - `background_tasks_workflow_shape_check` lists the legacy columns, so it must be edited.
  - Legacy tables: task_channel_targets, task_status_events, task_ownership_events. **Keep**
    task_attempts.
  - Legacy triggers and functions: the ownership, status-event, attention and notify-chain
    triggers, require_legacy_auxiliary_task, channels_delete_target_tasks,
    principals_release_owned_tasks.
  - **Keep** preserve_background_task_queue_identity,
    background_tasks_validate_workflow_association and every workflow_* trigger.
- **Callers:**
  - `main.rs` TaskWorker spawn and join: **D**.
  - TaskWakeups and the `task_ready` LISTEN (`setup.rs:295,361`, `events.rs`): **D**.
  - `routes/task.rs`, `ui_tasks.rs` stop/resume: **D**.
  - change_task_ownership at `ui.rs:1847`, `ui_tasks.rs:818`, `ui_team.rs:611`: **D**.
  - The `company_invite.rs` member_work_at_stake plus its **compile-time `sqlx::query!`** on
    background_tasks and task_outreaches (~:462, ~:508): **R**, and they must go before the
    schema drop.
  - `ui_deliveries.rs:306` and `pages/deliveries.rs:429,616` task links: **R→** run link, or drop.
  - `delivery/enqueue.rs:30` require_legacy_task_on, plus `NewDelivery.task_id`,
    `message_deliveries.task_id` and the require_legacy_source_task trigger: **R**, drop them.
  - `thread/views.rs:173-195` THREAD_TASK_LOOKUP_SQL (`ThreadMessageView.task_id`): **R→**
    message→run lookup. This is the plan's "message-lookup" retarget.
  - The mailbox ThreadActivity "agent is working" mark (`pages/thread_activity.rs`, `ui.rs:334`,
    the trigger background_tasks_notify_activity): **R→** workflow run state per thread.
  - Simulation (`routes/channel.rs:1010`, `pages/simulation.rs:610-662`,
    `pages/tasks.rs:304`): **R→** runs admitted for the simulated message.
  - Runtime metrics: ActiveTaskExecutions and task_worker_concurrency (services, infra,
    persistence, entities, `pages/dashboard_runtime.rs`) and their runtime_metric_samples
    columns: **R→** workflow slots, or drop.
  - The stuck-work census also emits the delivery gauges: **R**, move them to delivery
    maintenance.
- **Tests:**
  - `task_worker.rs` inline;
  - `persistence/task/*tests*`;
  - `thread/{inbound_multi_task_tests, task_lookup_tests}`;
  - `use_cases/thread/tests.rs` 5204;
  - the TaskPersistence mock in `smtp/server.rs` tests.
- **Config:**
  - `TASK_WORKER_CONCURRENCY` (`config.rs`, `.env.example:11`, `fly.toml:22`,
    `deploy.md:194,340`, `README.md:106`);
  - `AGENT_RUN_TIMEOUT_SECS` (legacy-only consumer);
  - the comments at `config.rs:29` and `infra/workflow.rs:54-60` (RESERVED_DATABASE_CONNECTIONS);
  - README §3.4–3.5, and `docs/query-evidence/2026-09-12-message-task-lookup.md`.

## A2. AgentRunner, RigHarness, LegacyBinding, HarnessKind, Registry, harness runs

- **Files:**
  - `services/agent_runner/`, `agent_trace_hooks.rs`, `prompt_fence.rs`.
  - `services/harness/`: ports.rs, registry.rs, context and redaction are legacy. **Keep**
    `classifier.rs` and `runs/` (shared).
  - `adapters/harness/mod.rs` (deployment_registry).
  - `adapters/harness/rig`:
    - legacy-only: execution.rs (RigHarness, LegacyBinding, `Binding::Legacy`), direct, compile,
      skills, tools, tool_hook, config, budget, trace, README;
    - shared: workflow.rs, providers, http, usage, protocol, final_response, diagnostics,
      schema, classifier.
  - `entities/harness.rs` (HarnessKind, HarnessConfig, RigAdvancedConfigV1,
    AgentCapabilitySpec, SubAgentScope) and `harness_run.rs`.
  - `persistence/task/harness_runs.rs`.
  - `persistence/harness_checkpoint.rs` is **shared**.
  - `use_cases/skill.rs:153-224` AgentCapabilityReader and `persistence/agent.rs:683`
    load_for_execution: legacy-only consumers.
- **Schema:**
  - `task_harness_runs` is shared: drop the legacy owner arm. `required_checkpoint_fields` pins
    `identity.harness='rig'`.
  - `task_harness_invocations` is legacy-only.
  - Re-check the guard_harness_* triggers for legacy arms.
  - `agents.config_json`, `agents.run_timeout_secs` and `agent_sub_agents` are legacy-only.
- **Callers:**
  - `setup.rs:182-184,269-277`; `thread/mod.rs`; `dispatch.rs`: **D**.
  - Harness diagnostics in `ui_tasks.rs`/`task_monitor.rs`: **D**.
  - HarnessConfig in the agent routes, use cases, persistence and the `agent_runtime_settings`
    page: **D** ("alternate runtime configuration").
  - The approval transitions: **D**.
  - SubAgentScope in channel internal targets and the agent forms: **D**, or a decision.
- **Docs:** `docs/rig.md`, `rig-verification.md`, the `scripts/stack-budget.sh` comments, the rig
  README.

## A3. Outreach, quorum and delegation

- **Files:** `persistence/task/{outreach, collaboration, controls}`; `entities/{outreach,
  delegation, collaboration}`; `task_worker.rs:702-880`; `thread/mod.rs:1530-1757`;
  `pages/delegation.rs`; OutreachWait in harness `runs/state.rs`.
- **Schema:**
  - Tables: task_outreaches, task_outreach_targets, task_outreach_replies,
    delegation_control_commands.
  - background_tasks outreach columns.
  - The `'delegation'` and `'delegation_timeout'` notification enum values, and
    `delegation_timeout_email_enabled`.
  - The outreach triggers and functions.
  - **Keep** `DeliveryPurpose::Outreach` and `ThreadEntryKind::Delegation`: these are name
    collisions only.
- **Callers:**
  - `ui_tasks.rs:554-680`, `ui.rs:2004-2020`, `ui_team.rs:646-661`: **D**.
  - The attention, notification, channel_assignment, response_review and company_invite parts:
    **D**.
  - `persistence/thread/mod.rs:596-599`, the agent-history NOT EXISTS clause: **R**, drop it.
  - `main.rs:165,167` notes: trim.
- **Docs:** `docs/inter_channel_agent_communication.md`, `custom_tools.md:7-8`,
  `transport_architecture.md:202`, `deploy.md:215`, README 99-103 and 289-297.

## A4. Legacy approvals and channel response reviews

- **Approvals:**
  - Files: `use_cases/approval.rs`, `persistence/approval.rs` plus `transitions.rs`,
    `entities/approval.rs`, `routes/approval.rs`, `pages/approvals.rs`, and ApprovalWait in
    `runs/state.rs`.
  - Schema: human_approvals, task_approval_waits, and their triggers.
- **Reviews:**
  - Files: `use_cases/response_review.rs`, `persistence/response_review*`,
    `entities/response_draft.rs`, `routes/ui_response_reviews.rs`, `pages/response_reviews.rs`,
    `thread/human_completion.rs:165-287`.
  - Schema:
    - tables response_drafts, _evidence, _publications, response_reviews,
      response_review_commands;
    - columns companies.external_response_review, channels.external_response_review_override,
      channels.preferred_reviewer_principal_id, response_review_email_enabled;
    - the `'response_review'` enum values, and the triggers and functions.
- **Callers:**
  - setup and app_state; task_worker; thread and dispatch; agent_runner: **D**.
  - `persistence/task/*`, channel_assignment, attention, notification: **D**.
  - The review-policy UI and API across channel, company, ui_companies and ui_channels, their
    persistence, and the use cases: **R**, delete the fields. Review is now `decision.human`.
  - Notification preferences (ui_profile, `pages/profile.rs`, `persistence/user.rs`): **R→** a
    workflow decision-notice preference, or drop.
  - Not coupled: the workflow `ApprovalSubject` only shares the name.
- **Routes:** `/approvals/{token}`, `/ui/approvals/{id}`,
  `/companies/{c}/channels/{ch}/approvals`, `/reviews*`, `/reviews/policy*`.

## A5. Thread handoffs and human completion

- **Files:**
  - `persistence/thread_handoff.rs` (its openers are `#[cfg(test)]`);
  - `entities/thread_handoff.rs`;
  - `application/thread_handoff.rs`, `use_cases/thread_handoff.rs`;
  - `thread/human_completion.rs`;
  - `routes/thread_handoffs.rs`, `ui_thread_handoffs.rs`;
  - `pages/thread_handoffs.rs`, `thread_handoff_marks.rs`;
  - the manual handoffs in attention.
- **Schema:**
  - Tables: thread_handoffs, thread_handoff_runs, thread_handoff_events, manual_handoffs,
    human_task_completions, attention_source_events.
  - Columns: companies.external_reply_handling, channels.external_reply_handling_override.
  - Their triggers and functions.
- **Callers:**
  - setup and app_state: **D**.
  - The `routes/ui.rs` held-reply banner, marks and `/ui/task-complete`; `pages/mailbox.rs`:
    **R**, strip them.
  - `live_updates.rs:56-83`, `events.rs:98,200-205`: **R**.
  - `routes/channel.rs` reply-handling, the company reply-handling fields: **D**.
  - `/attention/handoffs`: **D**, or a decision.
  - The task persistence and notification parts: **D**.
- **Routes:** `/api/companies/{c}/thread-handoffs/{id}[...]`, `/ui/reply-handling*`,
  `/ui/task-complete`.
- **Tests:** thread_handoffs_tests, ui_thread_handoffs_tests, thread_handoff_tests,
  ui_stream_tests (held-reply), the workflow `job_handoff_*` tests.

## A6. Internal-note agent tasks

- **Files:** `persistence/task/instructions.rs`; the AskOwnerToAct, StartAgentTask,
  AgentInstructionNote and AskOwnerOutcome entities (**keep** AddInternalNote, Tombstone and
  InternalNoteView); `thread/mod.rs:1288-1355`; `task_queue.rs`; agent_runner; dispatch.
- **Schema:** task_agent_instructions, task_agent_instruction_notes, start_agent_task_commands,
  and their triggers. **Keep** internal_notes and its tombstones: workflow `history_reader.rs:79`
  reads them.
- **Callers:**
  - `ui_internal_notes.rs` ask, start, ask-owner and agent-tasks routes: **D**.
  - `thread_handoffs.rs:280-298`: **D**.
  - The mailbox forms: **R**, drop the ask/start actions.

## A7. Memory infrastructure (P08-Q1: delete)

- **Files:**
  - `adapters/memory/`;
  - `services/memory_{coordinator, worker, provider, job_lease, job_schedule}`;
  - `use_cases/memory.rs`;
  - `persistence/memory.rs` and `memory_rows.rs`;
  - `entities/memory.rs`;
  - the memory fields in the company, agent and channel entities.
- **Schema:**
  - Tables: memory_provider_connections, memory_provisioning_jobs, memory_cleanup_jobs,
    memory_remote_resource_lifecycles.
  - Columns: companies.memory_provider and default_*_memory (6); channels *_memory (6);
    agents.memory_* (4); runtime_metric_samples.hydradb_*.
  - The memory lifecycle triggers and functions.
- **Callers (all D):**
  - main, setup, app_state;
  - thread, dispatch, ingest commit;
  - the channel, company and agent use cases;
  - company, channel and agent persistence;
  - routes: company memory, ui_companies memory retry, agent, ui_agents, channel, ui_channels,
    onboarding, agent_library;
  - pages: company_settings, agent_settings, channel_settings, channels, companies;
  - runtime metrics: drop the hydradb panel;
  - workflow provisioning's AgentWrite/ChannelWrite memory fields (Hazards).
  - **Keep** the `memory.load`/`memory.save` registry kinds and UNHANDLED_STEP_KINDS.
- **Config:** HYDRA_DB_* and HINDSIGHT_* (4 each), `config.rs:13,17,143-200,694-695,768-769`,
  `.env.example:97-109`, `deploy.md:139-140,378-415`, README, and
  `src/adapters/persistence/AGENTS.md:45-66`.

## A8. Channel agent lists, owner, memory, trigger, review and reply settings

- **Files:** `persistence/channel_assignment.rs`; `channel_gate.rs` (shared); the Channel fields
  owner_agent_id, agent_ids, response_trigger and memory; the ChannelWrite legacy fields,
  ActiveAgent, and check/resolve_internal_target; `persistence/channel.rs` create_with_agent and
  the owned-channel functions.
- **Schema:** channel_agents; channels.response_trigger and owner_agent_id; the channel_agents
  scope, active-agent, position-zero and library-delete-guard triggers.
- **Callers:**
  - The agent_ids users across routes, pages, persistence and use cases: **D/R**.
  - `thread/mod.rs:637` internal-relay "no configured agent" rule: **R**, drop it.
  - `ingest/routing.rs:708-725` bounce description: **R→** channel description.
  - response_trigger: **D**, replaced by the binding's `message_trigger`.
  - Workflow provisioning `provisioning.rs:597,608,618-626,805` and
    `tests/authorization_cases/mod.rs:3-29`: **R** (Hazards).
- **Tests:** channel_response_trigger_tests, channel_assignment_tests, constraint_tests (partly),
  agent_config_tests, the owner cases in `pages/tests.rs`.

## A9. Agent personal-channel coupling

- **Code:**
  - `use_cases/agent.rs` create_agent → create_addressable_agent(_with), create_agent_from_library;
  - `use_cases/channel.rs` create_channel_with_agent :348 and the owned-channel delete guard :589;
  - `persistence/channel.rs` insert_channel_row_on (owner param), create_with_agent,
    create_owned_agent_channel*;
  - `persistence/agent.rs:433-482`; `channel_assignment.rs:148-202`.
- **Schema:** channels.owner_agent_id, the owned_channel_delete_guard, and the position-zero
  triggers.
- **Callers:**
  - `ui_agents.rs:502,606,727`, `agent.rs:323,420,640`: **R→** agent-only creation plus "Create
    assistant channel".
  - `channel.rs:554,1638`, `ui_channels.rs:308`: **R→** AssistantChannels, or a plain channel
    plus a binding.
  - `onboarding.rs:302`: **R**.
  - Agent Builder install (`setup.rs:81` install_builtin_agent_library →
    create_agent_from_library): **R**.
  - Pages: remove the owner badges and links.

## A10. Legacy task pages and views; projections to retarget (P08-Q3)

- **Files:**
  - Routes: ui_tasks, task, ui_task_counts, attention, notifications, ui_dashboard,
    live_updates.
  - Pages: task_board, task_monitor, tasks, task_counts, attention, notifications, dashboard,
    dashboard_runtime, simulation, thread_activity.
  - Application: attention, notification, task_counts, notification_worker,
    dashboard_snapshot.
  - Persistence: attention, notification, dashboard, task/counts.
  - Entities: attention, notification, dashboard, task_counts.
- **Projections:** every projection filters `queue_kind='legacy'`:
  - `attention.rs:121,226,253,277`
  - `notification.rs:649,712,740,770,804`
  - `task/counts.rs:18`
  - `dashboard.rs:103-309`
- **Retargets:**
  - Attention → open decisions for the reviewer, waiting and failed runs, and bindings with
    refusals (D1.4.6).
  - Notifications → keep delivery_failure, and add workflow decision/run-failure sources.
  - Task counts → run and decision counts.
  - Dashboard → workflow runs, executions and attempts.
  - Mailbox → strip the nav, links, handlers, activity and banner.
  - The other `/ui/tasks` links (channels, companies, company_settings, team_settings,
    deliveries) → drop, or point at the decision page.
  - `infra/events.rs` task, ownership and attention channels.
- **Tests:**
  - ui_tasks, task, ui_task_counts_tests, ui_dashboard;
  - about 20 legacy tests in `pages/tests.rs`;
  - `pages/dashboard.rs` inline;
  - attention_tests, notification_tests, dashboard_tests;
  - JS `scripts/tests/task-counts.mjs` (run by CI `npm run test:task-counts`, ci.yml:44) and
    `task-counts-query-plan.py`.

## Coupling hazards (new code using legacy pieces)

1. **HarnessKind** in `application/workflow/agent.rs:17,202` (`RunIdentity.harness`), plus the
   schema check pinning `'rig'`. Replace it with a literal first.
2. **`services::harness::runs` is shared**:
   - the workflow uses RunCheckpoint, RunIdentity and related types in agent.rs, checkpoints,
     checkpoint_models, decision_tokens, agent_publication and `rig/workflow.rs`;
   - trim RunCheckpoint's `wait`/`outreach_wait`.
3. **AgentExecutionOutput/Disposition** live in `ports.rs`: split them out before deleting it.
4. **The Rig `Execution`** is shared between the legacy and workflow bindings. It imports
   compile's CompileContext/CompiledRun, tools' ToolCorrelationId and MAX_TOOL_INVOCATIONS, and
   trace. Move those and remove the legacy arms first.
5. **`persistence/harness_checkpoint.rs`** is shared: keep it.
6. **`task_harness_runs`** is shared: drop only its legacy arm.
7. **`background_tasks` and `task_attempts`** are shared:
   - legacy triggers are gated `WHEN queue_kind='legacy'`;
   - the workflow shape check references the legacy columns;
   - the workflow_decisions FK points at background_tasks.
8. **Provisioning (B3a)**:
   - it inserts channel_agents position 0, sets ChannelWrite.agent_ids, and uses
     `normalize_with(ActiveAgent::SuppliedByCaller)` and AgentWrite's memory fields;
   - edit it in the same step as the trigger and field removal.
9. **The inbound commit takes channel gates** (`channel_gate`): keep it, or drop the gate once
   no assignment writer remains.
10. **`history_reader.rs:79`** reads `internal_notes`: keep it.
11. **Name collisions only:** ApprovalSubject, ThreadEntryKind::Delegation and
    DeliveryPurpose::Outreach.
12. **Workflow tests encode legacy schema:** `job_*` (≈5.6k lines), `harness_owner_tests`,
    `harness_token_refusal_tests`, `decision_authority_tests`, `message_ingress_tests`, and the
    authorization_cases fixture.
13. **Legacy-free:** the workflow routes, assistant_channels, the decision page, and the
    workflow_* adapters use no legacy type.
14. **Compile-time sqlx macros** in `company_invite.rs` touch legacy tables. Remove them before
    the schema edit, then regenerate `.sqlx/`.

## Serve startup today

`main.rs` serve() at :187.

- **Boot:**
  - builtin agent library;
  - memory (legacy);
  - harness registry (legacy);
  - RigTextClassifier (keep);
  - WorkflowEngine (new);
  - NotificationWorker;
  - the legacy approval, review and handoff use cases;
  - ThreadUseCases, carrying legacy builders;
  - InboundEventWorker;
  - ScheduleUseCases on the workflow.
- **Spawned:**
  - New: workflow shutdown bridge and workflow.spawn (N workers, admission, schedule
    materializer).
  - Mixed: the runtime metrics sampler.
  - Legacy: TaskWorker (with maintenance) and MemoryWorker.
  - Retained: InboundEventWorker, NotificationWorker, DeliveryWorker, SMTP, the mailbox LISTEN
    (which also feeds the task channels), and HTTP.

## Suggested removal order (the crate compiles after each step)

0. **Extract shared types:** TokenUsage, AgentExecutionOutput/Disposition, the RunIdentity
   harness literal, ToolCorrelationId/MAX_TOOL_INVOCATIONS, and the delivery stuck-work gauges.
1. **Cut the HTTP and UI entry points:**
   - delete the A10, A4, A5, A6 and A3 routes and pages, harness diagnostics and the runtime
     settings;
   - strip the mailbox;
   - fix the cross-links;
   - retarget the projections or render them empty;
   - update `task-counts.mjs` and CI.
2. **Retarget creation:**
   - agent-only create_agent;
   - channel creation, onboarding, library install and Agent Builder install through
     AssistantChannels or a plain channel;
   - remove the personal-channel functions, the internal-relay agent rule and the bounce agent
     description.
3. **Stop the legacy runtime:** TaskWorker, the MemoryWorker spawn, TaskWakeups, task_ready, and
   the ThreadUseCases legacy builders.
4. **Delete the dispatch stack:** dispatch, reload, human_completion, the delegation and
   internal-note methods, AgentRunner, trace hooks, prompt_fence, task_worker,
   InboundTaskPayload.
5. **Delete the harness legacy.**
6. **Delete the side areas:** approvals, reviews, handoffs, outreach/quorum/delegation,
   internal-note tasks, and member-removal work-at-stake (its macros).
7. **Delete legacy task persistence:** task/*, task_queue, legacy_task, the enqueue check,
   NewDelivery.task_id, ThreadMessageView.task_id, entities/task, stuck_work, member_removal.
8. **Memory (P08-Q1).**
9. **Channel settings**, with the same-step provisioning edit.
10. **Schema in one init-migration edit:**
    - drop the tables, columns, triggers and functions;
    - trim the background_tasks and task_harness_runs checks, the enums and the runtime metrics;
    - recreate the test DB;
    - regenerate `.sqlx/`;
    - update the `job_*`/`harness_*` tests.
11. **Docs, config and bootstrap:**
    - `.env.example`, `fly.toml:21-24`, README;
    - `docs/deploy.md`, `rig.md`, `rig-verification.md`, `inter_channel_agent_communication.md`,
      `custom_tools.md`, `transport_architecture.md`, `docs/performance`, `docs/query-evidence`;
    - the stack-budget comments and the `main.rs:155-172` notes;
    - persistence AGENTS.md;
    - the ToolId comment (`value_objects.rs:383-390`) and the RESUME wording.

## Open decisions surfaced

- `agents.config_json` (HarnessConfig), `agents.run_timeout_secs` and `AGENT_RUN_TIMEOUT_SECS`
  are legacy-only: delete them as "alternate runtime configuration"?
- `agent_sub_agents` and SubAgentScope are legacy-delegation-only.
- Manual handoffs (`/attention/handoffs`, attention_source_events): part of "handoffs", or a
  retained attention feature?
- `channel_gate`: keep it, or drop it?
- The notification email preferences (review, delegation timeout, task failure) and their
  workflow equivalents.
