# P08-D4 rev 1: retargeted surfaces, live updates and the post-removal schema (B4)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

A gate before B4a. It is held because it adds cross-subsystem read models and a new
LISTEN/NOTIFY protocol over the workflow tables, changes a schema invariant of decision
reviewers, and fixes the end state of tables shared with the workflow queue. All of these are
costly to unwind once B4a–B4c depend on them.

## Sources

- Plan `08-…`, "Removal work" and "Acceptance":
  - retarget the approval, outreach, task-monitor, message-lookup and simulation callers;
  - no runtime or UI path depends on removed channel execution settings;
  - exhaustive caller searches.
- D1 rev 3 §D1.0 B4 row; §D1.4.6 (B4 projects bindings with recent refusals into the attention
  list).
- User decisions:
  - P08-Q1: delete the memory infrastructure;
  - P08-Q3: delete the legacy task pages, with a minimal retarget;
  - P08-Q4: assistant channels are team-only;
  - P08-Q6: delete manual handoffs;
  - P08-Q7: delete sub-agents;
  - P08-Q8: one decision-notice email preference, default on.
- Inputs: `logs/b4-inventory.md` (areas A1–A10, hazards 1–14, the removal order) and
  `logs/b4-projections.md` (the projection contracts).
- `src/adapters/http/AGENTS.md`, "Make partial-page updates race-safe" (subscribe before query,
  reconcile on connect and lag, keep `sse-connect` mounted, `hx-sync` replace); persistence
  `AGENTS.md` (indexes with EXPLAIN evidence, operational views report truncation).
- Coordination r10: WF-C002 (child labels), WF-C005 (p09 routes contract).

## D4.1 Live updates from workflow tables

**Today no workflow table emits NOTIFY.** One trigger is added:

- `workflow_runs_notify_changed`: AFTER INSERT, or AFTER UPDATE when `state` or
  `waiting_reason` changes (`WHEN OLD.state IS DISTINCT FROM NEW.state OR OLD.waiting_reason IS
  DISTINCT FROM NEW.waiting_reason`), FOR EACH ROW. It sends:
  - `pg_notify('workflow_run_changed', json {company_id, channel_id, thread_id, run_id, state})`.
    Channel and thread may be null.
  - When `thread_id` is not null, the existing `notify_thread_activity(company_id, channel_id,
    thread_id)` call. The mailbox activity mark then rides the existing `thread_activity`
    channel and its consumers unchanged.

These are the run-level transitions every retargeted view needs:
- a decision parks: state→waiting/decision;
- a submission: waiting→running;
- recovery: →waiting/reconciliation;
- terminal states;
- a message admission creates the run.

Per-step revision bumps do not notify, which keeps the volume at a few events per run.
Decisions notify through the run's state change. Reviewer assignment needs no separate channel
because it happens in the same transaction as the run's waiting/decision transition.

`infra/events.rs` adds the `workflow_run_changed` LISTEN and `MailboxEvent::WorkflowRunChanged
{company, channel, thread, run}` (payload parse is fail-closed like the others). Every consumer
keeps the AGENTS protocol:
- subscribe before the first query;
- reconcile on connect, on a matching event and on lag;
- the periodic re-auth tick where it exists today.

The legacy channels (`task_ready`, `task_chain_changed`, `task_ownership_changed`) and their
event kinds are deleted in B4b. `attention_changed` stays for delivery failures (its delivery
trigger is kept).

## D4.2 Attention / work list

`ATTENTION_SQL` keeps its paging, ranking, working-set bound (1,000), truncation and views.
Its branches become:

| Source kind | Rows | Responsibility | Due | Link |
| --- | --- | --- | --- | --- |
| `decision` | open decisions (`submitted_at IS NULL`, the wait `state='waiting'`) in channels the viewer can read, or company-associated | Principal(reviewers): My work for a reviewer; Team work for owner/admin | wait deadline | `/ui/workflow-decisions/{id}` |
| `run_reconciliation` | runs `waiting/reconciliation` | ChannelTeam: Unassigned/Team work | run deadline | none in p08; p09 run page (WF-C005 r2) |
| `run_failure` | runs `failed` in the last 7 days and not superseded by a rerun (`rerun_of_run_id` of a later run) | Principal(run actor), or ChannelTeam when the actor is gone | — | as above |
| `binding_refusal` | `workflow_message_intent_bindings` outcome refused/failed, plus `schedule_runs` failed materializations, in the last 7 days, **grouped per binding** (count, latest reason, latest at) | ChannelTeam (Team work, owner/admin) | — | none |
| `delivery_failure` | unchanged (it already includes workflow deliveries) | unchanged | unchanged | unchanged |

- The task, manual-handoff, thread-handoff, approval, response-review and delegation branches
  are deleted (A1/A3–A5, P08-Q6).
- The port loses `create_handoff`, `change_source_attributes` and `resolve_handoff`, and their
  routes go.
- `AttentionItem.task_id` becomes `run_id`. `correlation_id` comes from the run.
- `OperationalSummary` becomes, over the same window: open decisions, the oldest open decision's
  age, runs awaiting reconciliation, failed runs, binding refusals, and permanent delivery
  failures.
- **Visibility:** the existing `read_context` and readable-channel rule.
  - A company-associated decision is visible to its reviewers, and to owners/admins in Team
    work.
  - Runs on a channel follow channel readability.
- **Live:** reconcile on `WorkflowRunChanged` for a visible channel (or company-wide when the
  channel is null), as for `AttentionChanged` today.
- **Indexes** (EXPLAIN evidence in B4a):
  - `workflow_decision_reviewers (company_id, user_id)`;
  - a partial `workflow_runs (company_id, state, created_at DESC) WHERE state IN ('waiting',
    'failed')`.

  The refusal index exists already.

## D4.3 Notifications and the decision-notice preference (P08-Q8)

**Sources after B4:**
- `delivery` (kept): the recipient rule is corrected for deliveries with no task, closing a gap
  that exists today. The recipient is the run actor, through `workflow_message_destinations
  (delivery_id) → run → actor_id`, or for a schedule- or system-created delivery the channel's
  owner/admins.
- `decision` (new, in-app only): one notification per reviewer, inserted by a trigger on
  `workflow_decision_reviewers` INSERT (`enqueue_actionable_notification_event('decision',
  'decision_review', …)`). It is withdrawn or resolved when the decision is submitted or its
  wait settles; the source snapshot reads `submitted_at` and the wait state. The link is
  `/ui/workflow-decisions/{id}`. The NotificationWorker never emails `decision_review`:
  DecisionNotices already emails, honouring the preference, so there is no duplicate.
- The task, handoff, response_review and delegation sources are deleted with their enum values.
- `notifications.channel_id` becomes nullable, for company-associated decisions. Visibility for
  a decision notification is the recipient being a current reviewer, plus channel readability
  when there is a channel.

**Preference:** `user_notification_preferences` keeps `delivery_failure_email_enabled` and gains
`decision_email_enabled boolean NOT NULL DEFAULT true`. The assignment, response_review,
delegation_timeout and task_failure flags are dropped. The profile page shows two toggles.

**DecisionNotices honours it.**
- `eligible_reviewer` and the token approver lookup carry the reviewer's flag.
- `compose_notices` skips an opted-out reviewer.
- `workflow_decision_reviewers.notice_delivery_id` becomes **nullable**, and its guard checks
  the delivery only when it is set. The UNIQUE constraint tolerates NULLs.

The reviewer row, not the notice, is the authority for reviewing, so the in-app notification and
the decision page still reach an opted-out reviewer.

## D4.4 Open-work counts (task counts retarget)

The "Open tasks" bar becomes "Open runs", per company and per channel:
- `queued`→pending, `running`→active, `waiting`→waiting (all waiting reasons). Decisions show in
  attention, not here.
- The per-agent slots are removed: runs record no agent, and deriving one from bundles is
  guessing.
- `TaskCountSnapshot` becomes `RunCountSnapshot {company, channels}`.
- **Live:** `WorkflowRunChanged`, through the existing `RefreshScheduler` throttle (5 s, 60 s
  recheck). The SSE route is `/ui/run-counts/events`; the old route is deleted.
- **Index:** `workflow_runs (company_id, channel_id, state) WHERE state IN ('queued', 'running',
  'waiting')`.
- `scripts/tests/task-counts.mjs` is renamed `run-counts.mjs` and its extraction updated, with
  `package.json` and the CI step changed in the same commit. `task-counts-query-plan.py` and its
  doc are deleted.

## D4.5 Dashboard

The legacy task metrics are retargeted to the workflow queue by switching the filter to
`queue_kind='workflow'`:
- queue, pressure, throughput, queue depth and outstanding;
- latency, attempt stats and retry rate on `task_attempts` joined to workflow jobs;
- tokens from `model_usage_calls` for the window;
- the outstanding list links nowhere in p08.

Process gauges `ai_*`, fed only by the legacy runner, are deleted. The runtime panel:
- `active_task_executions` becomes the workflow jobs in `processing`, sampled from the database;
- `task_worker_concurrency_limit` becomes `WORKFLOW_WORKER_SLOTS`;
- the hydradb interval is deleted;
- the `runtime_metric_samples` columns follow.

Delivery metrics are unchanged.

## D4.6 Mailbox activity, message lookup, simulation

- **`ThreadActivity`** derives from the thread's newest run, preferring an unfinished one:
  - queued → Queued; running → Working;
  - waiting/decision → AwaitingReview; waiting/reconciliation → NeedsAttention; other waiting →
    Waiting;
  - failed → Failed; succeeded or cancelled → none.
  - `ThreadWorkSummary` becomes {run_id, activity}; ownership is gone.
  - Query: `DISTINCT ON (thread_id)` over `workflow_runs WHERE company_id=$1 AND thread_id =
    ANY($2)`, with the index `workflow_runs (company_id, thread_id, created_at DESC) WHERE
    thread_id IS NOT NULL`.
  - Live through `thread_activity`, from D4.1.
- **`ThreadMessageView.task_id` → `run_id`:**
  - an inbound message: its admitted run through `workflow_message_intents` ⋈ intent bindings
    (outcome admitted);
  - an agent reply: `workflow_message_destinations.message_id → run_id`.
  - The mailbox "· tasks" link is removed. The id stays in the view for p09 (WF-C005 r2).
- **Simulation:**
  - per message: the intent outcome (admitted, or refused with its reason); the run's state;
    its executions (step, route, timestamps); the model usage per run (provider, model, tokens);
  - from `RunDetail`, the intent tables and `model_usage_calls`;
  - this replaces the "Task Execution Parameters" block and `find_task_for_message`;
  - live through `thread_wake_ups` plus `WorkflowRunChanged` for the thread;
  - owner-only access is unchanged.

## D4.7 Agent and channel creation without personal channels

- **Agents:**
  - `create_agent` and `create_agent_from_library` create the agent only, with no channel;
  - the routes (ui_agents, agent, the JSON API) answer with the agent;
  - the agent page's "Create assistant channel" card (B3a) is the way to give it an address.
- **Channels:**
  - creation makes a plain channel, with no agent fields;
  - a channel needs no active agent any more: the constraint triggers go in B4c;
  - its automation is a workflow binding, through the assistant-channel action for an agent or
    the p08 binding routes (the p09 UI).
- **Onboarding** keeps its two steps:
  1. create the first agent (from instructions or the library);
  2. "create its channel" through `AssistantChannelService` with `AssistantAgent::Existing`:
     - slug = the agent's handle; name = "{agent name}";
     - reply mode Autonomous; access team (P08-Q4);
     - a command key derived from the onboarding session.

  The welcome copy says the channel answers team members' mail.
- **The Agent Builder** stays a library entry, installed per company as an agent (agent-only),
  with its provisioning grants written by the library install (B3a). No channel is created
  automatically; the admin uses the card.
- **Gaps accepted:** assistant channels ignore `channel_defaults` other than add_3rd_party, and
  are never public. Company channel defaults stay for plain channels.

## D4.8 Schema end state of shared tables (B4c)

- **`background_tasks`:**
  - keeps the workflow columns, constraints and triggers; drops every legacy-only column,
    status, check, index and trigger (inventory A1);
  - `queue_kind` keeps only `'workflow'` and the column stays (no rename churn);
  - `background_tasks_workflow_shape_check` is rewritten without the legacy columns.
- **`task_attempts`:** kept whole.
- **`task_harness_runs`:** keeps the workflow owner arm only; `required_checkpoint_fields`
  keeps `identity.harness = 'rig'` as a fixed literal (inventory hazard 1).
- **`message_deliveries.task_id`** and its legacy-source trigger are dropped.
- **`notifications`, `notification_events`:** source and action enums reduced to delivery and
  decision; `channel_id` nullable (D4.3).
- **`workflow_decision_reviewers.notice_delivery_id`** nullable, with the guard conditional
  (D4.3).
- **`runtime_metric_samples`:** the task, hydradb and ai columns dropped; the workflow
  active-jobs and slots columns added (D4.5).
- **`user_notification_preferences`:** as D4.3.
- **New:** the trigger `workflow_runs_notify_changed` (D4.1) and the indexes of D4.2, D4.4 and
  D4.6.
- `internal_notes` and its tombstones are kept (workflow history reads them).
- `channel_gate`:
  - kept, with the inbound commit's gate unchanged;
  - B4c removes the assignment writers but keeps the gate as the serializer between channel
    setting edits (enable, access) and ingress;
  - no lock-order change.

## D4.9 Invariants

1. No view depends on a legacy table. Every retargeted read is company-scoped and
   visibility-filtered as before, with the same 404 non-disclosure.
2. Every live view subscribes before querying and reconciles on connect, on events and on lag.
   No view treats an event as state.
3. A decision reaches each reviewer through the reviewer row (in-app and the page) whether or
   not the email preference is on.
4. Delivery failures of workflow deliveries notify the run actor, closing today's gap.
5. Creating an agent never creates a channel. A channel's automation is always a workflow
   binding.
6. The workflow queue's shape, claims, fences and revisions are unchanged by the legacy column
   drops.

## D4.10 Failure modes

- **NOTIFY volume:** bounded to state and waiting_reason transitions. Each consumer throttles or
  reconciles; a lagged broadcast forces a full reconcile.
- **A lost wake** (LISTEN reconnect): the existing reconnect logic emits lag, and consumers
  reconcile.
- **An opted-out reviewer:** no email, but in-app notification and attention. The guard no
  longer demands a delivery.
- **A run with no channel or thread:** it appears in Team work only. There is no activity mark.
  Counts appear at company level.
- **Schema drop ordering:** the compile-time sqlx macros on legacy tables
  (`company_invite.rs`) are removed in B4b, before the B4c schema edit (hazard 14).

## D4.11 Verification (per sub-batch)

**B4a:**
- **Attention:** a decision for its reviewer in My work and not for another member; owner Team
  work; reconciliation and failed runs; refusals grouped per binding; the delivery branch;
  truncation; tenancy 404s; live reconcile on `WorkflowRunChanged`, with an event between render
  and subscribe; two reads out of order (`hx-sync`).
- **Notifications:** a decision notification inserted and withdrawn on submission; a workflow
  delivery failure notifies the run actor; the email worker skips `decision_review`.
- **Preference:** an opted-out reviewer gets no notice delivery but a reviewer row, and can still
  submit; DecisionNotices sends to the others.
- **Counts:** the run-count snapshot and SSE throttle; the JS `run-counts.mjs` test.
- **Dashboard:** the workflow queue metrics on fixtures.
- **Activity:** the mapping table; a live wake from a run state change.
- **Message lookup:** inbound and reply run ids.
- **Simulation:** an admitted message shows its run; a refused one shows the reason.
- **Creation:** agent-only creates; onboarding produces an agent and a team assistant channel;
  the library install of the Builder gives it its grants and no channel.

**B4b/B4c:** compile and the ingest/workflow suites; the schema catalog and duplicate-index
tests; `.sqlx` regeneration; recreate `workflow08_test`; EXPLAIN for the new indexes.

**B4d:** exhaustive caller searches as acceptance evidence; the full suite only on the user's
go-ahead.
