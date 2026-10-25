# B4 projection contracts (input to P08-D4)

A read-only Explore agent produced this on 2026-10-23. It is condensed by root; line numbers refer
to the working tree at B3b-2 acceptance.

## Shared facts

- **No workflow table emits NOTIFY.** Every `pg_notify` in the init schema belongs to legacy,
  notification or message functions. The workflow tables only bump `workflow_runs.revision`
  (`advance_workflow_owned_revision` :47 on executions, waits, evidence and jobs;
  `advance_workflow_run_revision` :72). The engine polls; it does not LISTEN.
- **One LISTEN connection serves everything** (`infra/events.rs:299-369`).
  - Channels: thread_message, thread_activity, task_chain_changed, inbound_event_ready,
    task_ready, task_ownership_changed, attention_changed, actionable_notification_changed.
  - They become `MailboxEvent` on one broadcast (capacity 256).
  - Every SSE route subscribes before querying, and reconciles on lag.
- **Workflow deliveries** have `task_id: None` (`message_runtime.rs:234`). Delivery triggers wake
  attention_changed and task_chain_changed for them, and attention's delivery branch lists them.
- **Missing indexes:** nothing serves "decisions by reviewer user", "runs by channel/state" or
  "runs by thread". The indexes that exist: workflow_runs_newest, _binding_newest, the reviewers
  PK `(company_id, decision_id, principal_id)`, and `workflow_message_intent_bindings_binding`
  WHERE outcome<>'admitted'.
- **Existing readers:** `WorkflowReads` (list_runs, run_summary, run_detail, decision_company) and
  `read_decision`. None lists the open decisions for a user, runs per channel or thread, or
  counts. Refusals are written (`message_intents.rs:250-270`) but never read.

## (a) Attention / work list

- **Entity:** `AttentionItem` (source_kind, source_id, company, channel, thread, task, correlation,
  state, responsibility Principal|ChannelTeam|External, label, title, next_action, priority,
  due_at, expires_at, version, href).
  - Views: MyWork, Unassigned, TeamWork.
  - Page: items, next_cursor, as_of, working_set_size, truncated; limits 50/100/1,000.
  - `OperationalSummary` has 8 legacy metrics.
- **Port:** `application/attention.rs:14` (list, create_handoff, change_source_attributes,
  resolve_handoff, operational_summary).
- **SQL:** `ATTENTION_SQL` (`persistence/attention.rs:88-388`) unions seven branches: task,
  manual handoff, thread_handoff, approval, response_review, delegation, delivery_failure.
  - All are legacy except delivery_failure (`task_id IS NULL OR legacy`).
  - Ranking :353-387; `item_href` :423; `operational_summary` :1016-1160.
- **Routes** (`routes/attention.rs:34-59`): `/companies/{c}/attention`, `/summary`, `/handoffs`
  (POST), `/{kind}/{id}` (POST), `/handoffs/{id}/resolve`, `/attention/events`, `/ui/work`,
  `/ui/work/list`.
- **Page:** tabs, cards (badges, state, title, next action, responsibility, due/age, "Open
  source"), a truncation warning, the next page. Nav at `mailbox.rs:1577`.
- **Who may see it:** team members with a person principal, limited to readable channels. Team
  work and the summary need owner/admin.
- **Live:** SSE `/companies/{c}/attention/events`.
  - It subscribes first, reconciles on connect, on `AttentionChanged` for a visible channel, on
    lag, and every 30 s with re-auth.
  - Triggers call `notify_attention_changed` (:1519-1574) from background_tasks (legacy),
    manual_handoffs, human_approvals, response_reviews, task_outreaches, message_deliveries and
    thread_handoffs.
  - `AttentionWakeSource` has no approval variant.
- **Workflow sources:**
  - Open decisions for the viewer: `workflow_decision_reviewers` (company_id, user_id,
    decision_id, principal_id) ⋈ `workflow_decisions` (submitted_at IS NULL, subject_kind) ⋈
    `workflow_waits` (state='waiting', deadline) ⋈ `workflow_runs` (channel, thread). The link
    is `/ui/workflow-decisions/{id}`.
  - Runs `waiting/reconciliation`.
  - Failed runs: terminal_execution_id, `workflow_run_events.terminal_state`,
    `workflow_waits.failure_code`.
  - Refusals: `workflow_message_intent_bindings` (outcome refused/failed, reason ≤64 B such as
    unavailable, input_rejected, key_conflict or transient_exhausted, created_at), and
    `schedule_runs.refusal`/`materialization_status='failed'`/`last_error`.
  - Delivery failures: keep the existing branch.
- **Tests:** attention_tests 19 (2,198 lines), entities 3, pages 2, events 6, live_updates 3. No
  route tests.

## (b) Notifications

- **Entities:**
  - source kinds task, handoff, response_review, delegation, delivery;
  - actions assignment, response_review, delegation_timeout, task_failure, delivery_failure;
  - `NotificationPreferences` (5 email flags); `ActionableNotification`; `NotificationPage`;
    `should_email` (enabled and not self-triggered).
- **Port:** `application/notification.rs:49-119` (claim, resolve, commit, fail, reap, census,
  list, company_for_user, href, open; batch 32, lease 30 s, list limit 100).
- **Tables:**
  - `notification_events` (a queue), with CHECKs at :5418 and :5424;
  - `notifications` (recipient_user_id and principal_id, **channel_id NOT NULL**, state,
    read_at, email_delivery_id; CHECKs at :5450 and :5452);
  - `user_notification_preferences` (5 flags, default true).
- **Producers:** `enqueue_actionable_notification_event` (:837), from attention sources, delivery
  (:1076), outreach, review, task ownership and task status (legacy).
- **Projection:** `source_snapshot` (:639) has legacy filters.
  - Its **delivery branch inner-joins legacy `background_tasks`**, so workflow delivery failures
    resolve to no source and are withdrawn: nobody is notified (an existing gap).
  - `recipient_for_source` :836; `source_href` :1021.
- **Worker:** NotificationWorker polls every 500 ms with a 30 s census. Its email is "Action
  required: {label}", linking to `/ui/notifications/{id}/open`.
- **Routes:** `/companies/{c}/notifications`, `/{id}/read`, `/notifications/events`,
  `/ui/notifications`, `/ui/notifications/list`, `/ui/notifications/{id}/open`.
- **Preferences UI:** `pages/profile.rs:149-193` (5 toggles); PUT `/ui/profile/notifications`;
  persistence `user.rs:154/185`.
- **Live:** the trigger `actionable_notifications_notify` → `actionable_notification_changed`.
  The SSE reconciles on connect, on a user/company match, on lag, and every 30 s.
- **DecisionNotices:**
  - `application/workflow/decisions.rs:294-374` composes a standalone `Notification` delivery,
    "Review requested", with key `notification:workflow-decision:{d}:principal:{p}:`.
  - Three park paths share `compose_notices` and `insert_reviewers`: business, action approval
    (`eligible_reviewer`) and token allowance.
  - **The schema requires `workflow_decision_reviewers.notice_delivery_id NOT NULL`** (UNIQUE,
    FK; the guard `workflow_decision_reviewer_guard` :4078-4097 demands a provable notice
    delivery). Only the insert reads it.
- **The preference path:**
  1. add `decision_email_enabled` (default true);
  2. carry it on `Reviewer` through `eligible_reviewer`;
  3. skip `compose` for opted-out reviewers;
  4. make `notice_delivery_id` nullable, with the guard conditional;
  5. add a profile toggle.

  A cancelled delivery would not fit: the cancellation-reason enum has no preference reason.
- **Tests:** notification_tests 5, entities 2, pages 1, the user preference round trip, the
  profile panes in `pages/tests.rs`, and the decision-notice tests in the workflow decision
  suites.

## (c) Task counts

- **Shapes:** `TaskCounts` {pending, active, waiting}; `TaskCountSnapshot` {company, channels,
  agents}.
- **SQL:** `COUNTS_QUERY` (`persistence/task/counts.rs:12-20`, legacy) groups by channel, agent
  owner and status.
- **Route:** `/ui/task-counts/events` (owner/admin; readable channels).
- **Rendering:** slots `data-task-counts="company|channel:{id}|agent:{id}"`, the "Open tasks" bar
  and badges, `<template>` fragments copied by `TASK_COUNTS_SCRIPT`. Placed on the agent pages,
  the channel pages and in the mailbox script.
- **Live:** wakes on attention_changed with source_kind=task (legacy trigger). A
  `RefreshScheduler` throttles to 5 s with a 60 s recheck, and unchanged fragments are
  suppressed.
- **Workflow:**
  - `workflow_runs` by (channel, state): queued→pending, running→active, waiting→waiting.
  - Runs record no agent, so a per-agent count would need `model_usage_calls.agent_id` or the
    binding bundle, or the agent badges go.
  - No NOTIFY, no (company_id, channel_id, state) index.
- **Tests:**
  - JS `scripts/tests/task-counts.mjs`: 4 Playwright tests. It extracts `TASK_COUNTS_SCRIPT`
    from `pages/task_counts.rs` and `EVENT_DELEGATION_SCRIPT` from `mailbox.rs`. Run by
    `package.json` test:task-counts and CI `ci.yml:44`.
  - `task-counts-query-plan.py` (not in CI).
  - Rust: ui_task_counts_tests 7, counts 1, entities 1, pages 1, live_updates 1.

## (d) Dashboard

- **Routes:** `/ui/dashboard`, `/panels`, `/events`.
- **Scope:** owner/admin get their company; an operator gets the global view; the runtime panel
  and query health are operator-only.
- **Snapshot:** `DashboardSnapshotService`, cached on (company, window), 5 s tick (a ticker, not
  LISTEN). `DashboardSnapshot`: tasks, deliveries, throughput, latency, retry_rate, queue_depth,
  attempts, outstanding (max 20).
- **Legacy-based metrics:** TASK_QUEUE, TASK_PRESSURE, THROUGHPUT, QUEUE_DEPTH, OUTSTANDING
  (`background_tasks` legacy); LATENCY, ATTEMPT_STATS (tokens), RETRY_RATE (`task_attempts` ⋈
  legacy). The process gauges' `ai_*` come only from the legacy runner. The runtime panel's
  active_task_executions and task_worker_concurrency_limit come only from TaskWorker; the
  hydradb interval is memory.
- **Delivery-based** (it already includes workflow deliveries): DELIVERY_QUEUE,
  DELIVERY_PRESSURE.
- **Workflow equivalents:**
  - jobs: `background_tasks WHERE queue_kind='workflow'`;
  - attempts: `task_attempts`, with the workflow failure class, code, retry_safety and
    retirement;
  - runs: state, created_at, deadline; executions: activated_at/completed_at;
  - tokens: `model_usage_calls` (already at `/ui/companies/{c}/usage`).
- **Tests:** dashboard_tests 10, pages 21, runtime 3, route 5, snapshot 7.

## (e) Mailbox thread activity and message lookup

- **ThreadActivity:** Working, Queued, WaitingApproval, WaitingReply, Failed, derived from
  legacy task status and lease. `ThreadWorkSummary` (task_id, ownership, owner_label, activity).
- **SQL:** `list_thread_work_summary` (`task/operations.rs:449-503`, legacy, DISTINCT ON
  thread_id).
- **Callers:** `ui.rs` page_activity (:334), the open-thread strip (:388, :591), the column
  stream (:1229, :1272), thread_handoffs.
- **Rendering:** `pages/thread_activity.rs` mark and strip, slot `activity-{thread_id}`.
- **Message→task:** `THREAD_TASK_LOOKUP_SQL` (`thread/views.rs:177-197`) fills
  `ThreadMessageView.task_id`. Only `mailbox.rs:2832` reads it, for the "· tasks" link.
- **Live:** the trigger `background_tasks_notify_activity` (legacy, thread_id NOT NULL) →
  `thread_activity` → `MailboxEvent::ActivityChanged`. The column stream `/ui/threads/events`
  re-queries the summaries; the thread stream `/ui/events` re-sends the strip.
- **Workflow:**
  - Runs carry thread_id when message-admitted. Map queued→Queued, running→Working,
    waiting/decision→awaiting review, waiting/reconciliation→needs attention, failed→Failed.
  - Message→run: `workflow_message_intents` (UNIQUE company, message, channel; matched_run_id) →
    `workflow_message_intent_bindings.run_id` (admitted); agent replies via
    `workflow_message_destinations.message_id → run_id`.
- **Tests:** task/tests 1, ui_stream_tests 6, pages/tests (5), thread read_tests 8 (LISTEN),
  task_lookup_tests 4, job_projection_index_tests 1.

## (f) Simulation

- **Who may see it:** the owner only, on a channel they own.
- **Thread fragment:** `routes/channel.rs:966-1029` loads the thread, its history,
  `list_company_tasks` (legacy) and the latest reply context. Per message,
  `message_task_payload` → `find_task_for_message` (`pages/tasks.rs:304`) by payload match, or a
  synthesized `email_agent_dispatch` stand-in. `render_message_task_parameters_html` shows the
  "Task Execution Parameters" block (model, config, tokens, summary, payload JSON).
- **Live:** `simulation_stream` (`thread_wake_ups(thread_id)`), re-rendered whole.
- **Workflow:** intents (state, outcome, attempts, delivery_policy), intent_bindings (run or
  refusal), `RunDetail`, `model_usage_calls` per execution, destinations' reply message.
- **Tests:** `pages/tests.rs` 3, `live_updates` 1.

## (g) Agent creation and AssistantChannels

- **Every create makes a personal channel:**
  - create_agent → create_addressable_agent(_with) → create_owned_agent_channel;
  - create_agent_from_library;
  - callers: ui_agents :502, :606, :727; agent.rs :323, :420, :640; onboarding :268, :302;
    create_channel_with_agent from channel.rs :554, :1638 and ui_channels :308.
  - `install_builtin_agent_library` installs only the global library entry. A company copy
    comes from create_agent_from_library.
- **AssistantChannels:**
  - Request: agent_slug (existing agent with an explicit model), channel slug and name,
    reply_mode Autonomous|Reviewed, command_key. Owner/admin only.
  - It creates a team channel (never @public), publishes the template retargeted to the agent,
    and makes a Channel binding (Direct trigger) run as the requester; the reviewer is the
    requester.
  - The `create_agent_channel` tool form also creates the agent.
- **Gaps against the personal-channel creates:**
  - it needs an existing agent with an explicit model;
  - the caller chooses slug and name;
  - it ignores channel_defaults except add_3rd_party; there is no public option;
  - the reviewer is the requester only;
  - it returns JSON, not `ProvisionedAgent`;
  - it is not transactional with agent creation except through the tool's `New` path.
