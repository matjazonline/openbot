# P08-D4 rev 2: retargeted surfaces, live updates and the post-removal schema (B4)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Rev 2 answers WF-P08-R019 (F074–F083):
- F074: no in-app decision notifications; decisions surface in attention, and by email through
  DecisionNotices;
- F075: delivery-failure recipients defined, with every dependent SQL function listed and a test
  after the drop;
- F076: a NULL notice is tied to a recorded opt-out;
- F077: a reconnect publishes `Resync`;
- F078: one attention row per decision; refusal freshness; the missing indexes;
- F079: a message maps to a list of runs;
- F080: onboarding pins a model, and skips the channel without one;
- F081: the active-agent triggers drop in B4a;
- F082: the verification extended to every consumer;
- F083: triggers split by operation, and the existing trigger function reused.

The gate is held because it adds cross-subsystem read models and a LISTEN/NOTIFY protocol over the
workflow tables, changes a decision-reviewer schema invariant, and fixes the end state of tables
shared with the workflow queue. All of that is costly to unwind once B4a–B4c depend on it.

## Sources

Unchanged from rev 1:
- the plan's "Removal work" and "Acceptance";
- D1 rev 3 §D1.0 and §D1.4.6;
- user decisions P08-Q1, Q3, Q4, Q6, Q7, Q8;
- `logs/b4-inventory.md`, `logs/b4-projections.md`;
- `src/adapters/http/AGENTS.md` (race-safe partial updates) and persistence `AGENTS.md`;
- coordination r10 (WF-C002, WF-C005).

## D4.1 Live updates from workflow tables

**Triggers.** One new trigger function, `notify_workflow_run_changed()`, sends
`pg_notify('workflow_run_changed', json {company_id, channel_id, thread_id, run_id, state,
waiting_reason})`. Four triggers on `workflow_runs` (F083: Postgres forbids `OLD` in an INSERT
trigger's `WHEN`, so INSERT and UPDATE are separate):

| Trigger | When | Function |
| --- | --- | --- |
| `workflow_runs_notify_inserted` | AFTER INSERT, FOR EACH ROW | `notify_workflow_run_changed()` |
| `workflow_runs_notify_changed` | AFTER UPDATE, FOR EACH ROW, `WHEN (OLD.state IS DISTINCT FROM NEW.state OR OLD.waiting_reason IS DISTINCT FROM NEW.waiting_reason)` | `notify_workflow_run_changed()` |
| `workflow_runs_notify_activity_inserted` | AFTER INSERT, `WHEN (NEW.thread_id IS NOT NULL)` | the existing `notify_thread_activity()` |
| `workflow_runs_notify_activity_changed` | AFTER UPDATE, `WHEN (NEW.thread_id IS NOT NULL AND (OLD.state IS DISTINCT FROM NEW.state OR OLD.waiting_reason IS DISTINCT FROM NEW.waiting_reason))` | the existing `notify_thread_activity()` |

`notify_thread_activity()` is a trigger function that reads `NEW.thread_id`, `channel_id` and
`company_id`. Runs have all three, so the mailbox activity channel and its consumers are reused
unchanged.

These are the run-level transitions every retargeted view needs:
- a decision parks: →waiting/decision. All three park paths insert the reviewers in the same
  transaction;
- a submission moves the run;
- recovery: →waiting/reconciliation;
- terminal states;
- an admission inserts the run.

Per-step revision bumps do not notify.

**Listener.** `infra/events.rs` adds the `workflow_run_changed` LISTEN and
`MailboxEvent::WorkflowRunChanged(RunScope {company, channel, thread, run})`. The payload is
parsed fail-closed, like the others: a bad payload is logged and dropped.

**Reconnects (F077).** The loop switches from `PgListener::recv()`, which reconnects silently and
loses the notifications sent meanwhile, to `try_recv()`. On `Ok(None)` (a reconnect) it publishes
`MailboxEvent::Resync`. Every SSE consumer treats `Resync` like a broadcast lag: a full reconcile.
A helper `MailboxEvent::is_resync()` keeps that one decision in one place.

**Consumers** keep the AGENTS protocol: subscribe before the first query; reconcile on connect, on
a matching event, on lag and on `Resync`; and the periodic re-auth tick where one exists.

The legacy channels (`task_ready`, `task_chain_changed`, `task_ownership_changed`) and their
event kinds are deleted in B4b. `attention_changed` stays for delivery failures (§D4.3).

## D4.2 Attention / work list

`ATTENTION_SQL` keeps its paging, ranking, working-set bound (1,000), truncation and the three
views.

| Source kind | Rows | Who sees it | Due | Link |
| --- | --- | --- | --- | --- |
| `decision` | **One row per open decision** (`submitted_at IS NULL`, wait `state='waiting'`) | **My work:** decisions where the viewer is a current reviewer, responsibility Principal(viewer). **Team work** (owner/admin): every open decision in a readable channel or company-associated, with responsibility label "N reviewers". Never Unassigned | wait deadline | `/ui/workflow-decisions/{id}` |
| `run_reconciliation` | runs `waiting/reconciliation` | Unassigned and Team work, ChannelTeam | run deadline | none in p08 (p09 run page) |
| `run_failure` | runs `failed` with `created_at` in the last 7 days and no later run with `rerun_of_run_id` = it | My work for the run actor; Unassigned and Team work otherwise | — | as above |
| `binding_refusal` | refused/failed `workflow_message_intent_bindings`, plus failed `schedule_runs` materializations, in the last 7 days, **one row per binding** (count, latest reason, latest at) | Team work (owner/admin), ChannelTeam | — | none |
| `delivery_failure` | outcome_unknown/dead_letter deliveries, not superseded, **without the `background_tasks` join** (F075): workflow deliveries resolve their run through `workflow_message_destinations.delivery_id` | My work for the run actor when there is a run; otherwise Unassigned and Team work, ChannelTeam | as today | as today |

- The UI copy for the 7-day windows says "in the last 7 days" (F083).
- **Deleted branches:** task, manual handoff (P08-Q6), thread handoff, approval, response review,
  delegation. The port loses `create_handoff`, `change_source_attributes` and `resolve_handoff`,
  and their routes go.
- `AttentionItem.task_id` becomes `run_id`.
- `OperationalSummary` covers the same window: open decisions, the oldest open decision's age,
  runs awaiting reconciliation, failed runs, binding refusals, and permanent delivery failures.
- **Visibility:** the existing `read_context` and readable-channel rule. A company-associated
  decision appears in My work for a reviewer only while that reviewer is still owner or admin,
  the same rule as the decision page (`read_decision`). Owners and admins see it in Team work.
- **Freshness:**
  - decisions, runs and failures reconcile on `WorkflowRunChanged` for a visible channel (or
    company-wide when it is null);
  - delivery failures on `AttentionChanged`, as today;
  - **binding refusals create no run and no event: they refresh on the existing 30 s attention
    tick.** That bound is stated in the page copy as "updated within 30 s".
- **Indexes** (EXPLAIN evidence in B4a):
  - `workflow_decision_reviewers (company_id, user_id)`;
  - `workflow_runs (company_id, state, created_at DESC) WHERE state IN ('waiting','failed')`;
  - `workflow_runs (company_id, rerun_of_run_id) WHERE rerun_of_run_id IS NOT NULL`, for the
    "not superseded" probe;
  - a partial index on `schedule_runs` for failed materializations by company and time, with
    columns as the schema names them.

  The intent-binding refusal index exists.

## D4.3 Notifications, delivery recipients, and the decision-notice preference

**Sources after B4: `delivery` only (F074).**
- Decisions get **no** in-app notification row. The notification model allows one active
  recipient per source, and a decision has up to 16 reviewers. The in-app surface for a reviewer
  is attention My work plus the decision page; email goes through DecisionNotices, honouring the
  preference.
- The task, handoff, response_review and delegation sources, actions and enum values are deleted.
- `notifications.channel_id` stays NOT NULL: every delivery has a channel.

**Delivery-failure recipient (F075).** One recipient:
- **A workflow delivery** (a `workflow_message_destinations.delivery_id` exists): the run's
  `actor_id`, while it is a current team member who can read the channel.
- **Any other company delivery** (a person's mailbox send, which has no task or run): no
  notification, as today. The attention `delivery_failure` row in Team work is its surface.
- **`company_id IS NULL`** deliveries (notices, including notification emails and decision
  notices) stay non-notifying, so a failing notice never notifies about itself.

**Dependent SQL rewritten or deleted in B4c (F075).** PL/pgSQL is not checked against columns
when it is created, so each item is listed:
- `enqueue_actionable_notification_event`: drop the legacy-runtime preservation block (it reads
  `background_tasks`, outreach and drafts); keep the delivery path.
- `notification_from_delivery`: unchanged trigger. Its event resolves through the rewritten
  projection.
- `notify_attention_changed`: drop the task, workflow-queue and draft probes; keep it for
  deliveries.
- `notify_task_chain_changed` and its trigger on `message_deliveries`: delete, together with the
  task-chain channel (B4b).
- The `require_legacy_source_task` trigger function on `message_deliveries` and Rust
  `require_legacy_task_on` (`delivery/enqueue.rs:30`): delete, together with
  `NewDelivery.task_id` and `message_deliveries.task_id`.
- Rust SQL `source_snapshot` (delivery branch) and `recipient_for_source` (the inner join to
  `channels` stays, since a delivery always has a channel): rewritten to the rule above.
- The attention `delivery_failure` branch: rewritten without the `background_tasks` join.
- B4c also runs a catalog check: no remaining function body (`pg_proc.prosrc`) mentions a dropped
  table or column. The test fails on a hit.

**Preference (P08-Q8, with F076).**
- `user_notification_preferences` keeps `delivery_failure_email_enabled`, gains
  `decision_email_enabled boolean NOT NULL DEFAULT true`, and drops the assignment,
  response_review, delegation_timeout and task_failure flags. The profile page shows two toggles.
- `workflow_decision_reviewers` gains `notice_opted_out boolean NOT NULL DEFAULT false`, with
  `CHECK ((notice_delivery_id IS NULL) = notice_opted_out)`.
- The reviewer guard checks the delivery as before when `notice_opted_out` is false. When it is
  true, it requires the reviewer's preference row to exist with `decision_email_enabled = false`.
  "Every reviewer was provably notified, or provably opted out" still holds.
- `eligible_reviewer` and the token approver lookup carry the flag. `compose_notices` skips an
  opted-out reviewer, and `insert_reviewers` writes the flag.
- The reviewer row, not the notice, is the authority to review.

## D4.4 Open-work counts

Unchanged from rev 1:
- "Open runs" per company and per channel: queued→pending, running→active, waiting (all
  reasons)→waiting;
- the per-agent slots are removed;
- `RunCountSnapshot {company, channels}`; `/ui/run-counts/events` replaces
  `/ui/task-counts/events`;
- live through `WorkflowRunChanged` and `Resync` via the `RefreshScheduler` (5 s, 60 s recheck);
- the index `workflow_runs (company_id, channel_id, state) WHERE state IN
  ('queued','running','waiting')`;
- `scripts/tests/run-counts.mjs` replaces `task-counts.mjs`, with `package.json` and CI changed
  in the same commit; `task-counts-query-plan.py` and its doc are deleted.

## D4.5 Dashboard

Unchanged from rev 1, accepted in R019:
- the legacy task metrics use the workflow queue (`queue_kind='workflow'`) and `task_attempts`
  joined to workflow jobs;
- tokens come from `model_usage_calls`;
- the `ai_*` gauges and the hydradb interval are deleted;
- the runtime panel shows active workflow jobs and `WORKFLOW_WORKER_SLOTS`;
- delivery metrics are unchanged.

## D4.6 Mailbox activity, message lookup, simulation

- **`ThreadActivity`**, from the thread's newest run (an unfinished one first): queued → Queued,
  running → Working, waiting/decision → AwaitingReview, waiting/reconciliation →
  NeedsAttention, other waiting → Waiting, failed → Failed, succeeded/cancelled → none.
  - `ThreadWorkSummary` becomes {run_id, activity}.
  - Query: `DISTINCT ON (thread_id)` with the index `workflow_runs (company_id, thread_id,
    created_at DESC) WHERE thread_id IS NOT NULL`.
  - Live through `thread_activity` (§D4.1).
- **Message lookup (F079):** `ThreadMessageView.task_id` becomes `run_ids: Vec<Uuid>`, in binding
  id order:
  - an inbound message: every admitted run through `workflow_message_intents` ⋈ intent bindings
    (outcome admitted). At most one per active message binding of the channel, so at most 8;
  - an agent reply: its run through `workflow_message_destinations.message_id`.
  - The mailbox "· tasks" link is removed. The ids stay in the view for p09 (WF-C005 r2).
- **Simulation:** for each inbound message, **every** intent binding outcome:
  - admitted: its run's state, executions (step, route, timestamps) and model usage (provider,
    model, tokens);
  - refused or failed: its reason.

  It replaces the "Task Execution Parameters" block and `find_task_for_message`. Live through
  `thread_wake_ups` plus `WorkflowRunChanged` for the thread and `Resync`. Owner-only.

## D4.7 Agent and channel creation without personal channels

- **Agents:** `create_agent` and `create_agent_from_library` create the agent only. The agent
  page's "Create assistant channel" card gives it an address.
- **Channels:** creation makes a plain channel with no agent fields. Its automation is a workflow
  binding: the assistant-channel action, or the binding routes (the p09 UI).
- **Sequencing (F081):** B4a's schema edit drops the active-agent constraint triggers and their
  functions:
  - `channel_assignment_active_agent_check`;
  - `enabled_channel_active_agent_check` / `enforce_enabled_channel_has_active_agent`.

  An enabled plain channel is then valid from B4a on. The rest of `channel_agents` and the
  owned-channel triggers go in B4c; nothing creates owned channels after B4a.
- **Onboarding (F080):**
  - Step 1 (company, with an optional model connection) is unchanged.
  - Step 2 creates the chosen agents agent-only. **When the company has a default model
    connection, each created agent is pinned to it** (explicit provider and model), the way
    library pinning already works. Library agents keep their existing pinning.
  - Then, for each created agent that has an explicit model, the step calls
    `AssistantChannelService` with:
    - `AssistantAgent::Existing`;
    - channel slug = the agent's handle and name = the agent name;
    - reply mode Autonomous; team access (P08-Q4);
    - command key `onboarding:{agent_id}`, stable per agent and replay-safe.
  - **No model connection:** the agents are created without channels, and the completion page
    says "Connect a model in company settings, then create an assistant channel from each
    agent's page."
  - **A refusal** (for example a taken slug) shows on the completion page next to that agent,
    with the card as the way on.
  - The completion route becomes `/ui/onboarding/companies/{c}/complete`. It lists the created
    agents and their channels, or why each has none.
- **The Agent Builder** stays a library agent, installed agent-only with its library grants. No
  channel is created automatically.

## D4.8 Schema end state of shared tables

- **B4a** drops only the active-agent constraint triggers and functions (§D4.7), and adds the
  §D4.1 triggers and the §D4.2/§D4.4/§D4.6 indexes.
- **B4c** edits the init migration once:
  - `background_tasks`: keeps the workflow columns, constraints and triggers; drops every
    legacy-only column, status, check, index and trigger. `queue_kind` keeps only `'workflow'`.
    The `background_tasks_workflow_shape_check` is rewritten.
  - `task_attempts`: kept.
  - `task_harness_runs`: keeps the workflow owner arm; `identity.harness = 'rig'` stays a fixed
    literal.
  - `message_deliveries.task_id` and the dependent functions of §D4.3: dropped or rewritten.
  - `notifications` / `notification_events`: delivery source and action only.
  - `workflow_decision_reviewers`: `notice_opted_out` plus the conditional guard (§D4.3).
  - `user_notification_preferences`: §D4.3.
  - `runtime_metric_samples`: the task, hydradb and ai columns dropped; workflow active jobs and
    slots added.
  - Kept: `internal_notes` and its tombstones.
  - **`channel_gate`** is kept: the inbound commit's gate is unchanged, and channel-setting edits
    (enable, access) keep the exclusive lock. B4c adds a competing test: a disable against a
    concurrent ingress serializes, and the ingress sees either the enabled or the disabled
    channel, never a mix (F083).
  - A catalog test: no function body mentions a dropped table or column (§D4.3).

## D4.9 Invariants

1. No view depends on a legacy table. Every retargeted read is company-scoped and
   visibility-filtered, with the same non-disclosing 404s.
2. Every live view subscribes before querying, and reconciles on connect, on events, on lag and
   on `Resync`. No view treats an event as state.
3. A decision reaches each current reviewer through the reviewer row: attention My work and the
   decision page. Its email reaches every reviewer who has not opted out, and the database proves
   either the notice or the opt-out.
4. A failed workflow delivery notifies its run actor. A notice delivery never notifies.
5. Creating an agent never creates a channel. A channel's automation is always a workflow
   binding.
6. The workflow queue's shape, claims, fences and revisions are unchanged by the legacy drops.
   No function body references a dropped object.

## D4.10 Failure modes

- **NOTIFY volume:** bounded to state and waiting_reason transitions. Consumers throttle or
  reconcile, and a lag or `Resync` forces a full reconcile.
- **A LISTEN reconnect:** `Resync`, so the views reconcile and lost notifications cost nothing.
- **An opted-out reviewer:** no email; attention and the page still show the decision. The guard
  proves the opt-out.
- **A run with no channel or thread:** it appears in Team work, or My work for its actor, and in
  company counts. No activity mark.
- **Refusals:** visible within 30 s (the tick), not instantly.
- **Schema drop ordering:** the compile-time sqlx macros on legacy tables (`company_invite.rs`)
  are removed in B4b, before the B4c edit. The function-body catalog check catches any reference
  left behind.

## D4.11 Verification

**B4a**, every new or changed live consumer, as `src/adapters/http/AGENTS.md` requires (F082):
- **Triggers:**
  - an insert notifies;
  - a state or waiting_reason update notifies;
  - an update of only the revision or other columns does not;
  - an activity notify happens only with a thread;
  - asserted over a real LISTEN connection.
- **Listener:**
  - a bad `workflow_run_changed` payload is dropped and logged (fail-closed);
  - a forced reconnect (`pg_terminate_backend` of the listener's backend) publishes `Resync`.
- **Attention:**
  - a decision in a reviewer's My work and not another member's;
  - one row per decision in Team work with its reviewer count;
  - a demoted reviewer no longer sees a company decision;
  - reconciliation and failed runs; a failure superseded by a rerun disappears;
  - refusals grouped per binding;
  - delivery failures without a task;
  - truncation; tenancy 404s.
  - Live: a run state change reconciles; an event between render and subscribe is not lost; two
    reads completing out of order keep the latest (`hx-sync`).
- **Run counts:** the snapshot; the throttle; an event between render and subscribe; out-of-order
  reads; `Resync`; the JS `run-counts.mjs` test.
- **Activity:** the mapping table; a live wake from a run state change; an event between render
  and subscribe; `Resync`.
- **Simulation:** two bindings on one message show two runs; a refused binding shows its reason;
  a live wake; an event between render and subscribe.
- **Message lookup:** the inbound `run_ids` for two bindings; a reply's run.
- **Notifications:** the SSE path is unchanged (its existing tests stay); a workflow delivery
  failure notifies its run actor.
- **Preference:** an opted-out reviewer gets no notice delivery, a reviewer row with
  `notice_opted_out`, and can submit; a NULL notice for an opted-in reviewer is refused by the
  guard; the others still get notices.
- **Creation:**
  - agent-only creates;
  - onboarding with a model connection gives each created agent a team assistant channel;
  - onboarding without one gives agents only, with the completion copy;
  - a taken slug shows the refusal;
  - a replayed onboarding post does not duplicate channels (the key);
  - the Builder library install gives the agent its grants and no channel;
  - an enabled plain channel is creatable after the B4a trigger drop.
- **Dashboard:** workflow queue metrics on fixtures.

**B4b:** compile, and the ingest and workflow suites.

**B4c:**
- a workflow delivery driven to `dead_letter` and to `outcome_unknown` after the drop: a
  notification for the run actor, an attention row, no SQL error;
- a person's mailbox delivery failure: an attention row, no notification;
- the function-body catalog check;
- the channel_gate competing test;
- the duplicate-index and catalog tests;
- `.sqlx` regeneration; recreate `workflow08_test`; EXPLAIN for the new indexes.

**B4d:** exhaustive caller searches as acceptance evidence; the full suite only on the user's
go-ahead.
