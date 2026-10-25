# P08-D1 — Trigger admission, automatic authority and production composition (rev 1)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Architecture gate for p08 batch B1 (and the schedule contract B2 implements). Schema changes edit
`migrations/20260817000000_init_schema.sql` in place; databases are recreated, never upgraded.
User decisions P08-Q1..Q3 (TASK.md) are inputs, not open questions. Facts cite `da86871`.

## D1.0 Batch plan

| Batch | Scope | Gate |
| --- | --- | --- |
| **B1** Engine + message ingress | D1.1–D1.8: production composition and worker wiring in `serve`; binding run-as and message trigger; atomic inbound acceptance + admission intents (replacing legacy task/hold/outreach creation at ingress); intent worker; reply→wait correlation; relay-trace ownership; simulation delivery policy. Retarget ingest/inbound tests. | this document |
| **B2** Schedules, manual starts, interfaces | D1.9 schedules on bindings (schema, use cases, materializer, routes/pages); manual start/rerun command; company-scoped JSON routes for definitions, bindings, runs (start/read/cancel/retry), comments, decisions, reconciliation, workflow tools + agent grants (WF-C003); minimal `/ui/workflow-decisions/{id}` notification-link page; "Create assistant channel" setup. | D1.9 here; route surface reviewed in the batch review unless a new cross-subsystem decision appears |
| **B3** Removal + retargeting | Delete legacy dispatch/TaskWorker legacy queue, AgentRunner, RigHarness/LegacyBinding/HarnessKind/Registry/harness runs, outreach/quorum/delegation, legacy approvals, response reviews, handoffs/human completion, internal-note agent tasks, memory infrastructure (P08-Q1), channel agent lists/owner/memory/trigger/review/reply settings, agent personal-channel coupling; retarget attention/notifications/task counts/dashboard/mailbox (P08-Q3), onboarding, Agent Builder, bootstrap/seeds/docs/config; exhaustive caller searches; phase gate. | only if a removal needs a new decision |

B1 ends dual dispatch for ingress: after B1 no accepted message creates a legacy task. Legacy code
that becomes unreachable stays until B3 deletes it.

## D1.1 Automatic run authority (P08-Q2)

- `workflow_binding_revisions` gains `run_as_user_id uuid NOT NULL` (FK users). `ConfigureBinding`
  gains `run_as: Option<WorkflowActor>`; `None` = the configuring actor. `BindingConfiguration`
  carries it; it is part of the configuration revision (changing it is a new revision).
- Configure and activate require the run-as user to be current owner/admin **and** to pass the
  same related-association visibility the configuring actor passes (allowlisted channels), and
  resource readiness is checked against the run-as actor (`check_readiness(run_as, …)`), because
  admission and every action dispatch check the run's `actor_id`.
- Message and schedule admissions use `WorkflowActor::authenticated(run_as)`; manual starts use
  the caller. Admission's existing company lock + owner/admin check and association authorization
  are unchanged and are the recheck. A run-as who lost admin/visibility produces a refused
  admission outcome (D1.4) — never a silent drop and never a fallback actor.
- Existing runs keep their `actor_id`; action dispatch already requires its current owner/admin
  authority (`action_authority.rs`), so revocation reaches running work.

## D1.2 Binding message trigger

- `workflow_binding_revisions` gains `message_trigger text NULL CHECK (message_trigger IN
  ('direct','any','mentioned','mentioned_or_reply'))`. NULL = the binding is never admitted by
  messages (manual/schedule only). Domain enum `MessageTrigger` (pure `admits(facts)`), replacing
  `ChannelResponseTrigger` (deleted with the channel column in B3):
  - `direct`: recipient role To, or mentioned (legacy `always`);
  - `any`: To or Cc, or mentioned;
  - `mentioned`: mentioned;
  - `mentioned_or_reply`: mentioned, or the message directly replies to a message a workflow run
    published in this thread (`workflow_message_destinations.message_id`).
  `MessageFacts{role, mentioned, replies_to_workflow}`. `mentioned` keeps the channel name/alias/
  address/handle mention rules; agent-slug mentions are dropped (channels no longer own agents).
- A message trigger requires association `Channel` or `Thread` (configure refuses `Company`).
- **Standard message input** (`MessageTriggerInput`, one constructor, one schema):
  `{source_message: <canonical id>, message: <body text, quoted history stripped, truncated at a
  char boundary to 16 KiB>, subject: <≤998 chars>, sender: <address or handle label, ≤320>}`.
  Configure/activate refuse a message-triggered binding whose pinned version's input schema rejects
  a representative instance (`subject`/`sender`/`message` non-empty sample), so a bad pairing
  fails at configuration, not per message. Shipped fixtures/templates declare `subject` and
  `sender` as optional properties (required stays `[message, source_message]`).

## D1.3 Atomic acceptance + admission intent

- New table `workflow_message_intents` (one row per **accepted message × channel association**,
  only when the folded disposition answers):
  `company_id, id, message_id, channel_id, thread_id` (FK to the thread association),
  `role text CHECK IN ('to','cc')`, `mentioned bool`, `reply_to_message_id uuid NULL` (resolved
  direct parent: trusted `reply_to_message_id` or the provider-mapped In-Reply-To, the same
  resolution `replies_to_agent` uses today), `hop_count int`, `trace_channels uuid[]` (bounded),
  `delivery_policy text CHECK IN ('deliver','in_app_only')`, `correlation_id`, queue columns
  `state ('pending','claimed','done','failed')`, `attempts (0..5)`, `available_at`, `worker_id`,
  `lease_expires_at`, outcome `('matched','admitted','none')` + `matched_run_id`, timestamps.
  UNIQUE `(company_id, message_id, channel_id)`. State-shape CHECKs as for `schedule_runs`.
- Outcomes per binding: `workflow_message_intent_bindings(company_id, intent_id, binding_id,
  outcome ('admitted','refused','failed'), run_id NULL, reason text NULL (bounded code))`,
  PK `(company_id, intent_id, binding_id)`.
- `CommitPlan` loses `tasks`, `holds`, `outreach_transitions`; it gains one `AdmissionIntent` per
  answering association with its facts. `commit_on` inserts intents after `associate_threads`
  (same transaction; statement order fixed; the existing channel gates remain the first locks).
  Redelivery (`recognise_redelivery`) writes nothing — the original intents are the only ones.
  No binding row is read or locked in the commit: eligibility is resolved by the worker.
- Ingress protections (DMARC/source checks, hop limit, loop trace, auto-reply, spam threshold,
  sender authorization, thread injection, turn limit, disabled-channel bounce, redelivery
  recognition, thread resolution under lock) run before the commit exactly as today; no company
  workflow can disable them because admission is downstream of the commit.
- A channel without an active message-triggered binding still stores messages; its intents end
  `done/none`.

## D1.4 Intent worker

- Application service `MessageAdmissions` (port `MessageIntentQueue`: claim/finish/defer with
  lease fencing; port reads on bindings; `WorkflowService::admit`; `WorkflowWaits` reply delivery).
  Claim: `FOR UPDATE SKIP LOCKED`, oldest `available_at`, at most 4 per company per page (window
  function), page ≤ 32, lease 60 s; claim increments `attempts`; lease expiry consumes the attempt;
  `attempts = 5` → `failed` (observable, not reclaimed). Poison-batch regression test required.
- Processing one intent, in order:
  1. **Reply correlation (D1.5).** If matched → outcome `matched(run)`, done; no admission.
  2. **Eligible bindings:** active bindings with a non-NULL `message_trigger`, association
     `Channel(channel_id)` or `Thread(channel_id, thread_id)`, whose rule admits the facts
     (`replies_to_workflow` derived from `reply_to_message_id` here). Evaluated at processing time
     against current activation: deactivation before processing stops admission ("disabling a
     binding stops future admissions"); this is documented, not hidden.
  3. For each eligible binding without an outcome row: `admit` with actor = run-as,
     association `Thread{channel, thread}`, trigger `Message{message_id}` with
     `trigger_id = intent.id`, idempotency key `m:{intent}:{binding}`, correlation = the message's,
     input = `MessageTriggerInput` built from the stored canonical message (identifiers only in
     the intent; content read at admission), delivery policy from the intent (D1.7). Result:
     `Created|Replayed` → `admitted(run)`; deterministic refusal (`NotFound`, `BadRequest`,
     `Conflict`, authorization) → `refused(code)`; transient error → stop, defer the intent
     (backoff), later attempts skip bindings that already have outcome rows.
  4. Done when every eligible binding has an outcome; intent outcome `admitted` or `none`.
- Each binding is admitted independently: one refusal never blocks a sibling binding. Admission's
  `(company, binding, source_key)` uniqueness keeps "one run per binding per message" even across
  intent retries, duplicate workers, or a crash between admission commit and outcome write.
- Wake: in-process `Notify` after an inbound commit that created intents, plus a 1 s poll.

## D1.5 Reply correlation to event waits

- Reserved event name `message.reply`. Correlation = the **outbound canonical message id** the
  reply answers (string UUID). Authors write
  `wait.event: {event: {literal: message.reply}, correlation:
  {ref: /steps/<send>/output/messages/0/message_id}}` — the message step output already lists
  `messages[i].message_id` per destination (`registry/schemas.rs` `message_result`). Payload =
  `MessageTriggerInput` of the reply; the wait step's output schema must accept it (the registry's
  per-name schema rule applies).
- Candidate: `reply_to_message_id` matches a `workflow_message_destinations.message_id` whose
  thread is the intent's thread. References-only ancestry is not used (ambiguous).
- New port method `WorkflowWaits::deliver_reply(run, correlation, event_id, payload)` → under the
  run's producer advisory lock: if any `message.reply` event already exists for
  `(run, correlation)` → `Unmatched` (one matching response per wait); else `record_event_on`.
  `event_id = UUIDv5(intent.id)` so a reprocessed intent is `Duplicate`.
- `Recorded|Duplicate` → matched: the message resumes that wait and admits no run. `Unmatched`
  (run ended, no `message.reply` wait declared, already answered) → ordinary admission (D1.4.2).
- Sender authorization already happened at ingress; the payload carries `sender` so a workflow can
  further check who answered. The legacy quorum loop gets no replacement (NOT_PLANNED).

## D1.6 Relay-trace ownership

`message_runtime`'s `source_relay_on` reads the legacy task payload today. It moves to the intent
row: `(company, message_id, channel_id)` → `(hop_count, trace_channels)`. A message filed without
an intent (FileOnly) has no recorded trace — same as today (no task, no payload). Workflow replies
continue the trace; sends start one (unchanged). The hop limit and loop check stay at ingress.

## D1.7 Simulation delivery policy

`ReplyDelivery::{Send, InAppOnly}` (channel simulation `Run` vs `RunTest`) is carried on the intent,
into `AdmitWorkflowRequest.delivery_policy` (default `Deliver` for manual/schedule), stored on
`workflow_runs.delivery_policy`, inherited by every runtime child (workflow.call, flow.repeat,
workflow tools). `in_app_only`: message steps publish canonical messages with no email delivery
rows (reply/user email targets become in-app publication; receipts are local publication). Other
effects are unchanged, as in the legacy RunTest. Admission replay compares the policy (a different
policy under the same key is `Conflict`). Phase 09 sample execution remains a separate mode.

## D1.8 Production composition and worker wiring

- `infra::workflow` (new) builds, from `AppConfig` and the shared pool: `WorkflowService`,
  `BindingService`, `DefinitionService` whose publication directories are Message, Http, Mcp and
  Agent with `ChildPublicationDirectory` nested **outside** `AgentPublicationDirectory`
  (WF-C001 r2), the shipped `TemplateCatalogue` (fixtures as seed templates), and one composite
  `WorkflowHandler` routing by kind to: `WorkflowAgentRuntime` (agent.run/ai.classify/
  decision.agent) whose `AgentActionTools` include `with_workflow_tools` (WF-C001 r2),
  `HttpRuntime` (http.request/tool.call), `McpRuntime` (mcp.call), `MessageRuntime`
  (message.send/reply), `WorkflowHistoryRuntime` (context.history). Every action-owned kind goes
  through ActionService's marker-first dispatch (§04.10); the composite adds no effect path.
- **Unhandled kinds are refused at publication:** the production composition passes the
  handled-kind set to validation/publication; a version containing `memory.load`/`memory.save`
  (registered, unhandled — NOT_PLANNED) is refused with a diagnostic, so no run can stall on an
  unsupported step. A composition test asserts every registered IO kind is handled or refused.
- `serve` spawns, supervised with the existing shutdown token/JoinSet: `WORKFLOW_WORKER_SLOTS`
  (default 4, bounded 1..=32) `WorkflowWorker` loops with `DecisionNotices(public_base_url,
  app_domain)`; the intent worker; (B2) the schedule materializer. Each loop owns cancellation and
  is joined on shutdown; lease expiry consumes attempts (existing ledgers).
- `TriggerSource::Child` stays refused by `admit` (WF-C001 r2); delegation uses child workflows
  (runtime writer) and explicit message steps; a run's own message publications never admit runs.

## D1.9 Schedules on bindings (implemented in B2)

- `channel_schedules`: keep id/company/channel/name/type/interval/timezone/enabled/next/last
  columns; add `binding_id uuid NOT NULL` (FK, binding association must be the schedule's channel
  or Company); drop `subject_template`, `prompt_template`, `delivery_mode`, `recipient_emails`,
  `run_as_user_id` (authority = binding run-as, D1.1).
- `schedule_runs`: `task_id` → `run_id` (workflow run); `materialized` ⇔ `run_id NOT NULL`;
  add `refusal text NULL`; a deterministic admission refusal ends `failed` immediately (shape
  check updated); transient errors retry under the existing lease/attempt bounds; add
  `command_key text NULL` UNIQUE `(company_id, schedule_id, command_key)` for run-now.
- Materializer (moved out of TaskWorker) admits with trigger `Schedule{schedule_id,
  occurrence_id}`, association `Channel(schedule.channel_id)`, actor = run-as, idempotency key
  `s:{occurrence}`, input `ScheduleTriggerInput{scheduled_for}`; slot dedup is the existing
  `(binding, source_key)` admission key plus the occurrence row. No thread or prompt message is
  created. Run-now inserts an occurrence (`scheduled_for = now`, client `command_key`) and
  materializes it through the same path. Schedule save validates the input against the bound
  version (as D1.2).
- `SCHEDULED_AGENT_RUN_TASK`, `ScheduledRunPayload` and the scheduled dispatch branch are deleted
  in B2 (or B3 with the TaskWorker).

## Invariants

1. No accepted message creates a legacy task; no old and new dispatcher can act for one message.
2. Inbound acceptance and its intents commit together; a redelivery creates no intent; one run per
   (binding, message) regardless of retries/workers; one per (binding, schedule occurrence).
3. Every automatic run's actor is its binding's current-at-admission run-as owner/admin.
4. A matched reply resumes exactly one wait and admits nothing; an unmatched one is ordinary ingress.
5. Each binding is admitted independently; replies name their source message and run
   (`workflow_message_destinations`), and runs never share answers.
6. Unsupported step kinds cannot be published in production.

## Failure modes

| Failure | Behavior |
| --- | --- |
| Crash after inbound commit, before processing | Intent pending; worker processes on restart. |
| Crash after admission, before outcome write | Retry replays the same key → `Replayed(run)`. |
| Two workers / lease expiry | `SKIP LOCKED` + lease fencing; admission key dedups; attempt consumed. |
| Run-as lost admin/visibility; binding inactive; input rejected | `refused(code)` row, sibling bindings unaffected, intent `done`. |
| DB outage during processing | Deferred with backoff; 5 attempts → `failed`, observable. |
| Reply after run ended / second reply | `Unmatched` → ordinary admission. |
| Redelivery | No new message, no new intent. |
| Simulation RunTest | Run carries `in_app_only`; no email delivery rows from it or its children. |

## Affected contracts

`InboundCommitRequest`/`CommitPlan`/`PreparedChannel` (ingest), `commit_on`, `InboundCommitOutcome`
(task/handoff ids → intent ids), `ConfigureBinding`/`BindingConfiguration`/binding persistence
rows, `AdmitWorkflowRequest` (+`delivery_policy`), admission replay equivalence,
`workflow_runs`, runtime child writers (policy inheritance), `WorkflowWaits` (+`deliver_reply`),
message runtime relay lookup and delivery planning, `serve`/setup composition, `AppConfig`
(`WORKFLOW_WORKER_SLOTS`), schedule schema/use cases/routes (B2).

## Verification plan (B1)

DB-backed, on the phase cluster, focused: ingest/inbound commit tests retargeted to intents
(atomic rollback, redelivery, multi-association, FileOnly, disabled channel); intent worker:
eligibility matrix (4 rules × To/Cc/mention/reply), two message-triggered bindings on one channel
→ two independent runs, refused run-as with a sibling admitted, deactivate-before-processing,
**competing claimants** (two workers, one intent), crash-replay (admission committed, outcome
missing), poison batch; reply correlation: matched resume end-to-end through the worker, second
reply → new run, ended run → new run, event-before-park; relay trace continued by a workflow reply
after legacy tasks are gone; simulation `in_app_only` (no delivery rows, child inherits);
composition test (all IO kinds handled or refused; memory step refused at publish; WF-C001
publication of a `workflow.call` parent through the production composition; child admission
refused through `admit`); end-to-end: inbound email → intent → run → agent (scripted provider) →
`message.reply` delivery naming source message and run; two parallel messages in one thread keep
separate runs/replies. Plus clippy, fmt, fresh migration, stack budget for affected modules.
