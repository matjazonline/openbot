# P08 — Triggers and replacement of application orchestration

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Identity

- Phase ID: `p08` (IDs `WF-P08-…`). Workflow: `implement-opus-reviewed-2_2`
  (skill dir `/Users/mac02/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Record root: `/Users/mac02/dev/openbot/plan/workflow-foundation/phases/p08/docs/`.
- Root = Author (A): this Claude Code session (started 2026-10-22), Opus 5.5 / xhigh (user stated
  "already Opus/xhigh" in the invoking request). B: agent `af5ae9ab0d4e0a22f`
  (general-purpose, model opus, effort xhigh, fresh context, spawned 2026-10-22 for WF-P08-R001;
  continue it with `SendMessage`).
- Worktree: `/Users/mac02/dev/openbot`, branch `workflows`. Phase starting commit
  `da868710d3b235d49856e3e481f8f3735f6b3e56` (p07 committed as `bd2ef36`, records `da86871`).
- Preserved pre-existing work: none (clean tree at start). `marketing/` and `website/` are the
  user's unrelated areas (excluded).
- Snapshot exclusions (exact): `plan/workflow-foundation/phases/p08/docs
  plan/workflow-foundation/COORDINATION.md marketing website`. Starting digest (no edits):
  `e07e4b1d4a5f9491a6fd1115d251a416d56955383a564f249531e9ba565b47e9`.
- Plan coordination: `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md`,
  revision 4 (sha256 `89563c7e…0f5b`, copy `logs/coordination-r4.md`). Writer was the p07 root
  session; p08 root takes over as writer when it next publishes (recorded in the next revision).
- Compaction: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` is set in `.claude/settings.local.json` env and
  observed in this session's environment.
- Tooling: `graft` CLI/MCP is not installed (MCP: executable not found); discovery uses rg/read.

## Resources

Phase PostgreSQL 18.6 cluster `/private/tmp/workflow08-20261022`, `127.0.0.1:55480`, superuser
role `workflow08` (trust, loopback only), databases `workflow08` (dev, migrated from the init
schema: `logs/baseline-migrate.log`) and `workflow08_test`. Use
`PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH`,
`DATABASE_URL=postgres://workflow08@127.0.0.1:55480/workflow08`, `CARGO_INCREMENTAL=0`.
Restart: `pg_ctl -D /private/tmp/workflow08-20261022 -l /private/tmp/workflow08-20261022/server.log start`.
Never touch the user's 5432 server or p07's 55470 cluster.

## Original requirements

- Plan: `plan/workflow-foundation/08-triggers-and-application-replacement.md` (authoritative),
  EXPANSION rows 08.1–08.4, README, REPLACEMENT-MAP (every row naming 08), NOT_PLANNED
  (email-only approvers removed with no replacement; multi-response aggregation excluded; memory
  out of release), RESUME "Phase-08 obligations (from 06C)".
- Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`, `src/adapters/http/AGENTS.md` (+ `pages/AGENTS.md`),
  `src/infra/AGENTS.md`, `src/adapters/smtp/AGENTS.md` (ingress).
- Coordination notices targeting p08: WF-C001 r2, WF-C003 r1 (see "Coordination").
- Prerequisite: p07 accepted (R006/R012/R013 PASS), committed `bd2ef36`.

### User decisions (2026-10-22, AskUserQuestion)

- **P08-Q1:** delete the memory-provider infrastructure (HydraDB/Hindsight adapters,
  MemoryCoordinator/MemoryWorker, memory job tables, company/agent/channel memory settings,
  routes, env config). A future release restores from git.
- **P08-Q2:** automatic (message/schedule) runs act as a **per-binding run-as owner/admin**,
  recorded on the binding revision (defaults to the configuring caller), rechecked at every
  admission and action; loss of admin refuses admissions visibly. Schedules use their binding's
  run-as.
- **P08-Q3:** delete legacy task pages/routes; retarget attention, notifications, task counts
  and dashboard counts to workflow runs/decisions with minimal projections. p08 ships JSON routes
  for runs; p09 builds run and queue pages.

User decisions (2026-10-23, in conversation, after a clarification request on the
AskUserQuestion draft):

- **P08-Q4:** the Agent Builder's `create_agent_channel` tool creates the agent **and a channel
  bound to the reply mode the Builder picks in the conversation** (the autonomous or the reviewed
  template: the reply is the template's own `message.reply` step, after `decision.human` in the
  reviewed one). **The channel is never public**: it is created with access mode `team`
  (company members only). User's words: "the agent plus a channel on a reply mode the Builder
  picks in the conversation - but the channel should not be public."
- **P08-Q5:** no built-in approval on the tool call. Approval belongs to the tool's own policy or
  to a workflow step (`decision.human`); the provisioning tool does not hardcode one now. User's
  words: "no, depending on a tool or workflow (tool could have it hardcoded, but leave now if
  it's not planned or we add step to workflow)".

### Acceptance criteria (plan §Acceptance + body)

1. Inbound email, scheduled and manual starts, and child calls reach the same run engine.
2. Parallel message runs in one conversation retain separate decisions, artifacts, and replies.
3. Duplicate schedule slots and provider redeliveries do not start extra work.
4. Existing ingress/access/threading protections remain exercised against the new routes.
5. No runtime or UI path depends on removed channel execution settings or alternate harness
   selection.
6. Exhaustive caller searches identify no remaining legacy execution entry points.
7. (body) Inbound acceptance and admission intent persist atomically; each eligible binding is
   resolved independently; redelivery creates no extra admissions; each reply names its
   triggering message and producing run; no combined answers across runs.
8. (body) A channel retains messages without an active binding; disabling stops future
   admissions; cancelling runs is separate; existing runs keep their pinned snapshot subject to
   current access/revocation.
9. (body) Manual start: published workflow, current caller access, client idempotency key,
   immediate run identity. Schedules: bound to binding/version/params, slot dedup, reuse schedule
   infrastructure, scheduled-agent dispatch removed. Delegation: child workflows / explicit
   message steps with causal links and target authorization. Outreach: explicit send +
   correlated event wait, matched replies resume their wait, unrelated messages create
   independent runs, one response per wait, quorum loop removed. Agent creation creates an agent
   only; explicit "Create assistant channel" binds a selected agent to an autonomous/reviewed
   template.
10. (body) Company-scoped HTTP routes over the owning commands: draft save/validate/publish/
    archive, binding revision/activation, run start/read/cancel/retry, comments, decision
    submission, action reconciliation; same semantics across browser/API/agent-tool/notification
    link; idempotency keys for work creation, expected revisions for edits; nested resource
    authorization incl. channel/thread; CSRF and webhook auth.
11. (body) Removal: obsolete dispatch and scheduled-dispatch branches, task-payload context
    snapshots, response-review tables/owners, executable skill instructions, alternate runtime
    configuration, automatic personal-channel coupling; retarget approval, outreach, task-monitor,
    message-lookup and simulation callers; update bootstrap fixtures, seed templates, config docs
    and examples together; memory infrastructure deleted (P08-Q1).
12. (§04.10) Every handler advertising `mcp.call`, `http.request`, `tool.call` routes effects
    only through ActionService's marker-first protocol; a new tool bridge preserves it.
13. (06C obligations, verbatim from RESUME:55-58 and :17-19) "create_agent_channel +
    list_company_agents as workflow tools and Agent Builder creation (revise its prompt: workflow
    tools, explicit provider/model); production workflow worker wiring + refusal of unhandled step
    kinds; replace scheduled legacy runs with workflows (no test drives a scheduled run through
    real Rig today); remove single-value HarnessKind/HarnessRegistry, legacy AgentRunner,
    outreach/quorum/ApprovalUseCases machinery and mcp_connections_by_ids (test-support only) with
    the legacy binding"; "decide delete-or-dormant for memory-provider infrastructure" (→ P08-Q1
    delete); "ToolId comment wording (value_objects.rs) vs workflow tool diagnostics".

## Coordination

- Checked r4 at startup (2026-10-22). Applicable: **WF-C001 r2** (child admission refused through
  `admit`; delegation uses runtime child writer; manual start/rerun expose `rerun_of`; every
  production `DefinitionService` includes `ChildPublicationDirectory` nested outside
  `AgentPublicationDirectory`; every production `AgentActionTools` includes the workflow tool
  runtime) and **WF-C003 r1** (routes over `WorkflowTools` register/revoke/list and agent
  `workflow_tools` grants with P07-Q2 semantics). Disposition: both accepted; WF-C001 in B1
  (composition) and B2 (manual start/rerun), WF-C003 in B3 (routes). Responses recorded in
  coordination r5/r6; verification pending.
- WF-C002 targets p09 only.
- 2026-10-23: published **WF-C004 r1** (coordination r7, sha256 `934993e5…4d64`, copy
  `logs/coordination-r7.md`): every Postgres pool goes through
  `infra::db::discard_leaked_transactions` (F029 root cause). p08 is the origin; F030 (remaining
  test-only pools) is done in B2. WF-C001 verification so far: child admission refused through
  `admit` and parent publication through the production composition (B1 tests,
  `infra::workflow`); rerun link exposure pending (B2).

## Discovery (2026-10-22)

Three read-only Explore inventories (admission surfaces, legacy ingress, other legacy owners);
condensed in `logs/inventory.md`. Headline facts:

- No production composition of the workflow engine exists: `WorkflowService`, the workflow
  worker, `DefinitionService`, publication directories, runtimes, `AgentActionTools` and
  reconciliation are built only in test support; `serve` spawns no workflow worker.
- No HTTP route touches any workflow command or table.
- `admit` requires an owner/admin `WorkflowActor`; no system actor; admission opens its own
  transaction; dedup PK `(company, binding, source_key)`; message source requires a Thread
  association; no binding trigger/eligibility configuration; no channel→active-bindings query.
- No ingress→wait correlation; `record_event` has no production caller.
- Legacy ingress: `commit_inbound` (thread/inbound.rs) atomically commits the message and
  creates legacy `email_agent_dispatch` tasks per pipeline; TaskWorker runs dispatch →
  AgentRunner → RigHarness(LegacyBinding).

## Batches

See `logs/decision-D1-r2.md` §D1.0 (rev 2 split B2 per WF-P08-F012.5 and added F007).
**B1** engine composition + message ingress (gate P08-D1); **B2** schedules on bindings + manual
start/rerun command; **B3** company-scoped routes (WF-C003, WF-C002 list rule), decision link
page, assistant-channel setup, `create_agent_channel`/`list_company_agents` workflow tools (gate
P08-D2 first); **B4** removal + projection retargeting + bootstrap/docs + ToolId wording + phase
gate. One review per batch.

## Architecture gate P08-D1

- rev 1: `logs/decision-D1.md` (sha256 `453bf4a0…7bb`), sent to B 2026-10-22 as WF-P08-R001 →
  **changes required**: F001–F003 High, F004–F007 Medium, F008–F011 Low, F012 optional.
- rev 2: `logs/decision-D1-r2.md` (sha256 `5271630f…0c63a`; diff `logs/decision-D1-r1-to-r2.diff`).
  Dispositions: F001 fold = stated directives only, audience by directive, every association gets
  an intent (`admissible` flag); F002 matched only for an open awaited wait (+ bounded `NotYet`
  defer for event-before-park); F003 responder identity per destination kind + user-send thread
  participation; F004 id-dedup first, classified outcomes, registry-fixed `message.reply` payload;
  F005 `Stale` result with expected binding revision, per-binding transient handling; F006 legacy
  owner/admin-self run-as rule kept, `None` keeps stored run-as, refusal surface (log, metric, B3
  route, B4 attention); F007 tools scheduled in B3 behind gate P08-D2; F008 generation fence,
  CTE fairness, ≤8 message bindings per channel, composite FKs; F009 `in_app` destination column
  and CHECK; F010 pool check, token bridge, lifecycle tests; F011 schedule authority,
  compatibility revalidation, B2 tests. F012: 1 adopted (`activated_at`), 2 adopted, 3 → D2,
  4 adopted (B3), 5 adopted (four batches). Sent to B for recheck as WF-P08-R002 → rev 1
  findings resolved; new F013/F014 Medium, F015–F017 Low, F018 optional.
- rev 3: `logs/decision-D1-r3.md` (sha256 `7c4b9457…aaf7`; diff `logs/decision-D1-r2-to-r3.diff`). F013 run-as without FK
  + removal test; F014 attempt-neutral `NotYet` bounded by grace; F015 default pool 20 + pinned
  test; F016.1 wait row `FOR SHARE` + deadline check, F016.2 recorded-before-deadline event wins in
  `resume_on`/sweeper, F016.3 answered with the existing thread-party fallback
  (`routing.rs:325-343`) plus an allowlist test; F017 cap checked on every change yielding an
  active message-triggered binding under a channel advisory lock; F018 adopted (correlation must
  reference a same-graph message step) and coordination r6 corrects WF-C001/C003 batch placement.
  Sent to B as WF-P08-R003 → **PASS for rev 3** (2026-10-22). **The B1 implementation contract is
  `logs/decision-D1-r3.md`.** Carried into B1 (mandatory, B1 batch review checks them):
  - **F019 (Low):** `deliver_reply` locks the run row `FOR KEY SHARE` first, then the wait row
    `FOR SHARE` (same order as `resume_on`/`expire_on`); the wait-vs-expiry race test asserts no
    deadlock/lock-timeout error.
  - **F020 (optional, adopted):** `NotYet` delay = `least(now() + greatest(2 s, now() -
    created_at), created_at + REPLY_PARK_GRACE)` (age-doubling, no counter); the event's
    `created_at` is the same `clock_timestamp()` value the (b) deadline check used; a wait whose
    deadline equals the run deadline is still ruled by run expiry (`expire_on`).

## Status

- 2026-10-23: **B3a accepted in phase** (not integrated, not committed). B PASS WF-P08-R012 at
  digest `22334ec4414b79d0a68f8d052a3c787fca241611b8afa44e5bcd24ca7a4e77e1` (base `da86871`, same
  exclusions; manifest `logs/b3a-snapshot-r2.txt`). Root recomputed the same digest; coordination
  r8 unchanged. Rounds R011 (changes required: F051 Medium, F050 Low; F035, F046–F049 closed;
  DV-1..DV-6 accepted) and R012 (PASS). No open B3a finding. B3b starts.
- 2026-10-23: R011 changes required (F051 Medium, F050 Low); both corrected (TASK "B3a
  corrections for WF-P08-R011"). Snapshot digest `22334ec4…77e1`. Sent to B as recheck
  WF-P08-R012.
- 2026-10-23: **B3a implemented** (TASK "B3a implementation log"), checks green (table there;
  broad run 2773 passed, its one failure fixed and rerun). Snapshot `logs/b3a-snapshot.txt`:
  digest `9bdad3dae00d010689c5660c3c9a0c8895145079ea6c1989cc3dea27287d1f3c`, base `da86871`, same
  exclusions; coordination r8. Sent to B as batch review WF-P08-R011 (includes F035 and F046–F048).
- 2026-10-23: **gate P08-D2 rev 2 PASS** (WF-P08-R010; `logs/decision-D2-r2.md`, sha256
  `b88fe8add6ae5738708b4e07d1ed93154dc030f452291ee983946cd6a1b08afd`). F036–F044 resolved, F045
  adopted. Carried into the B3a batch review (Low): F046 (refusals only at named deterministic
  checks, never inferred from error variants; injected-timeout test), F047 (`granted_tool`
  resolves reserved provisioning names to the built-in first and refuses an ambiguous capture),
  F048 (requester from the stored `workflow_admissions.source_key`, exhaustive, fail closed).
  F049 (Info) adopted: `Invalid` → 400; "first model" = the default connection's models in stored
  order; the UI digest includes the caller. B3a implementation started.
- 2026-10-23: WF-P08-R009 (gate P08-D2 rev 1): **changes required**. Medium: F036 (run-as of
  provisioned binding), F037 (email-established requester), F038 (bypassed service checks
  unnamed). Low: F039–F044. Info: F045. Rev 2 (`logs/decision-D2-r2.md`, sha256 `b88fe8add6ae5738708b4e07d1ed93154dc030f452291ee983946cd6a1b08afd`)
  answers all of them, adopting F045. The requester is the configuring actor and run-as;
  message roots count only with intent origin `application`; extracted shared checks
  (`save_binding_on`, `publish_on`, `compile_bundle`, `capture_agents_on`, `insert_channel_on`,
  `model_allowed`, `provisioned_binding`); committed typed-error refusals under a savepoint;
  `workflow_provisioning_commands`; one reserved-name rule; competing-claimant tests; library
  copy carries `workflow_tools` and an explicit model. Sent to B as recheck WF-P08-R010. B
  noted the tree moved by the F035 correction during R009; the file list is unchanged from
  `logs/b2-snapshot-r2.txt` (digest now `d98a8fd7…8f28`), and the staged deletions were already
  part of B2.
- 2026-10-23: **B2 accepted in phase** (not integrated, not committed). B PASS WF-P08-R008 at
  digest `e43d8c37b661f17963acebfcf685f9859aeba5d46b9702a3620c0b9aa974b94f` (base `da86871`,
  same exclusions; manifest `logs/b2-snapshot-r2.txt`). Root recomputed the same digest;
  coordination r8 (`0950b907…5e22`) unchanged during the review. Rounds R007 (changes required:
  F031–F033 Low, F034 optional) and R008 (PASS). F031–F034 closed. Open: F035 (Info, optional):
  `fail_expired_occurrences` commits the failed occurrences before their schedules' `last_error`,
  and the materializer logs and counts only after the call returns, so a failed schedule update
  (or a crash between the two statements) loses the log, the metric and `last_error`.
  Disposition: fixed at the start of B3a and reviewed with B3a. The requester owner/admin
  invariant of P08-D2 was raised with the user; no objection received, so the gate goes to B
  with it (the user can still widen it to any member, which reopens the gate).
- 2026-10-23: user decisions P08-Q4/Q5 recorded (Agent Builder tool creates agent + non-public
  channel on the reply mode the Builder picks; no built-in approval on the tool). R007 changes
  required (F031–F033 Low, F034 optional); all corrected (TASK "B2 corrections for
  WF-P08-R007"). Snapshot `logs/b2-snapshot-r2.txt`: digest
  `e43d8c37b661f17963acebfcf685f9859aeba5d46b9702a3620c0b9aa974b94f`, same base and exclusions.
  Sent to B as recheck WF-P08-R008.
- 2026-10-23: gate P08-D2 rev 1 drafted (`logs/decision-D2-r1.md`): B3 is split into B3a
  (provisioning tools, one-transaction assistant provisioning, `/run/deadline`, shipped
  catalogue, Agent Builder) and B3b (JSON routes, decision page, WF-C003). It adds a requester
  owner/admin invariant for `create_agent_channel`. To be sent to B after R008 returns.
- 2026-10-23: B2 implemented (TASK "B2 implementation log"); focused checks green. Snapshot
  `logs/b2-snapshot.txt`: base `da86871`, digest
  `7b932911f8a76a9fb4100af447f33df9388e052f7601576c22f6aa90b8125c51`, same exclusions. Sent to
  B as batch review WF-P08-R007. Coordination r8 (`0950b907…5e22`).
- 2026-10-23: **B1 accepted in phase** (not integrated, not committed). B PASS WF-P08-R006 at
  digest `b90a7f9c850b737062fc92bb971899f18756b8f086f2641649a39a657b49a468` (base `da86871`,
  exclusions `plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md
  marketing website`). Root recomputed the same digest; coordination r6 was unchanged during the
  review. Review rounds R004 (changes required), R005 (F028, F029), R006 (PASS, optional F030).
  F030 (Info): ten test-only pools (B counted ten; `thread/read_tests.rs` is an eleventh) bypass
  the guard. Disposition: fixed at the start of B2 through one guarded test helper. Candidate
  instruction improvement (not applied during the run): a `src/adapters/persistence/AGENTS.md`
  rule that every pool uses `discard_leaked_transactions`. B2 starts.
- 2026-10-23: R005 corrections (F028 fix; F029 root cause = sqlx `begin` cancellation leak, fixed
  by the pool guard `infra::db::discard_leaked_transactions`) verified (table at the end of "B1
  corrections for WF-P08-R005"). Snapshot `logs/b1-snapshot-r3.txt`: base `da86871`, digest
  `b90a7f9c850b737062fc92bb971899f18756b8f086f2641649a39a657b49a468`, same exclusions. Sent to
  B as recheck WF-P08-R006. Coordination r6 unchanged (`23fdc190…c4db`).
- 2026-10-22: B1 implemented with its tests; focused checks green (see B1 log). Snapshot frozen
  and sent to B as batch review WF-P08-R004.
- 2026-10-22: gate P08-D1 rev 3 PASS (R003). B1 implementation started.
- 2026-10-22: setup, discovery and user decisions done; coordination r5 published (p08 writer,
  WF-C001/C003 acknowledged; copy `logs/coordination-r5.md`, sha256 `38fd9afe…5c74`). Gate
  P08-D1 rev 1 under review (WF-P08-R001). No source edits yet.

## B1 implementation log

Implemented to the contract `logs/decision-D1-r3.md` (PASS R003). Lib and tests compile; clippy
`-D warnings` and `cargo fmt --check` clean.

Production code:
- Schema (init migration, in place): `workflow_binding_revisions.run_as_user_id` (no FK, F013) +
  `message_trigger`; `workflow_bindings.activated_at` + shape CHECK; `workflow_runs.delivery_policy`;
  `workflow_message_destinations.in_app` (**D-1**); `workflow_message_intents` (+ comment, F012.2)
  and `workflow_message_intent_bindings` with composite FKs (association FK to
  `thread_messages(company, channel, thread, message)`); `check_workflow_wait_commit` amended
  (**D-3**).
- Domain `MessageTrigger`/`MessageFacts`/cap (`domain/workflow/trigger.rs`); triggers module
  (`MessageTriggerInput`, `ScheduleTriggerInput`, `MESSAGE_REPLY_EVENT`).
- Binding: run-as + trigger in `BindingConfiguration`/`ConfigureBinding`; `check_message_trigger`;
  persistence `check_run_as` (owner/admin-self rule, F006), `check_message_binding_cap` (F017),
  `activated_at`; saved run binding read keeps the run's actor (replay after head removal).
- Admission: `DeliveryPolicy`, `expected_binding_revision`, `AdmissionResult::Stale`,
  `AdmissionBinding{saved}`; `verify_current` → Stale; replay equivalence + child inheritance of
  the policy.
- Message runtime: routes carry the run policy; `plan_in_app`; user-send thread participant
  (D1.5.4); relay trace from intents (D1.6).
- Registry: `message.reply` wait rule (payload schema + correlation shape, F018) (**D-2**).
- Waits: `deliver_reply` (`reply_waits.rs`, F019 lock order, F020 single clock); `resume_on`
  consumes an event recorded by the deadline (D1.5.6) and the commit guard allows it (D-3).
- Ingest: commit plan with intents (fold = stated directive), outreach/hold/answers removed from
  routing (`prepare_channel` extracted), `commit_on` writes intents, redelivery returns them,
  intent wakeup after a committing ingest; handoff opener `#[cfg(test)]` until B4.
- Intent worker: `application/workflow/message_admission.rs` + `persistence/workflow/message_intents.rs`.
- Composition: `infra/workflow.rs` (`WorkflowEngine`, `ProductionHandler`, `worker_slots`,
  `UNHANDLED_STEP_KINDS` refused at publication via `DefinitionService::with_refused_kinds`,
  WF-C001 r2 nesting, workflow tools in `AgentActionTools`); `setup.rs`/`AppState.workflow`;
  `serve` token bridge + `join_background` of every workflow loop; default pool 20 (`infra/db.rs`).
- Fixtures: the five message-started fixtures declare optional `subject`/`sender` (triage-routing
  also `source_message`) so the standard message input binds (D1.2).
- Fixed in passing: a malformed diagnostic string literal in `binding/mod.rs` (lost line
  continuation); stale doc on `thread_access_for` (outreach).

Deviations (all within the gated decisions; B reviews them):
- **D-1** `in_app` is a generated column `delivery_id IS NULL` plus BEFORE INSERT trigger
  `workflow_message_destination_in_app_guard` (a delivery-less `user` destination only for an
  `in_app_only` run): a CHECK cannot read the run, and a stored literal column conflicted with
  existing channel-destination inserts.
- **D-2** a `message.reply` wait must declare the canonical payload schema; "omit it and receive
  it" is not offered because `wait.event` requires `payload_schema` for every event.
- **D-3** D1.5.6 needed one schema change the gate did not name: the deferred constraint trigger
  `check_workflow_wait_commit` refused any event-wait completion after the wait deadline. It now
  allows completion when the consumed event was recorded at or before the deadline; the run-deadline
  check and the park-time check are unchanged. The phase-05 test
  `workflow_wait_event_deadline_guard_is_independent_of_run_deadline` asserted the old rule and was
  rewritten to the D1.5.6 contract (slow commit past the deadline completes with an in-time event;
  an event recorded after the deadline never completes the wait, which expires).
- **D-4** the legacy constraint "enabled channel must have an active agent at position 0" still
  exists until B4 retires channel-owned agents; the end-to-end fixture names an agent on the channel.
- **D-5** (WF-P08-F026) the shipped `TemplateCatalogue` named in D1.8 is not composed in B1:
  nothing in B1 consumes it. It moves to B3, composed with template copy and the "Create assistant
  channel" setup behind gate P08-D2.

Deleted or rewritten legacy tests → replacement coverage:
- `use_cases/thread/{external_reply_tests.rs, external_reply_tests/*}` (customer round trip via
  TaskWorker/AgentRunner) and `rig_execution_tests.rs` → `message_ingress_end_to_end_each_message_gets_its_own_run_and_reply`
  (production engine, scripted Rig agent, `message.reply`).
- `inter_channel_tests.rs` (legacy delegation tasks) → WF-C001 composition test
  `production_publication_refuses_memory_steps_and_captures_children`, relay-trace intent test,
  existing hop/loop ingress tests.
- `simulation_tests.rs` (RunTest → task with `InAppOnly`) → `an_intent_records_disposition_policy_origin_and_relay_trace`,
  `workflow_message_thread_user_send_opens_a_thread_the_member_takes_part_in`,
  `workflow_message_thread_in_app_reply_stays_in_the_source_thread`,
  `workflow_call_child_inherits_the_parents_delivery_policy`.
- `structured_response_tests.rs` (legacy structured agent reply) → workflow `agent.run` output
  schema tests (p05/p06 `agent_runtime_tests`), unchanged.
- `response_trigger_tests.rs` (channel trigger fold, removed by D1.3) → binding rule matrix
  `message_intent_rules_admit_each_eligible_binding_independently`, domain `MessageTrigger` tests,
  rewritten mention tests in `thread/tests.rs` (`a_cc_d_channels_*`).
- `thread/tests.rs` hold-gate section, `internal_channel_callback_resumes_original_task_without_new_task`,
  `a_cc_d_channel_runs_for_its_assigned_agent_slug` (agent-slug mentions dropped by D1.2) →
  intent tests in the same file and the `message_reply_*` correlation tests.
  `test_sender_verification_and_delegation_target_check` → `test_sender_verification_refuses_strangers_replying_into_a_thread`
  (outreach widening removed, D1.3).
- `persistence/thread/inbound_tests.rs` hold and outreach tests; `inbound_multi_task_tests.rs`
  `two_pipeline_tasks_*`, `overlapping_task_channels_*` → `each_association_gets_one_intent_and_redelivery_adds_none`
  (one intent per association; overlap now a schema UNIQUE), `an_intent_records_*`,
  `intent_rows_cannot_reach_across_tenants`. The lock-order test keeps a legacy task as the
  sibling source for the legacy publisher until B4 deletes both.

New B1 tests (verification plan of `decision-D1-r3`):
- `persistence/workflow/message_intent_tests.rs`: rule matrix + independent runs; refused run-as
  visible (reason, metric) with sibling admitted and member removal unblocked; activated-later and
  deactivated bindings; `Stale` re-read (call counts prove the retry); transient sibling + retry +
  `transient_exhausted`; competing claimants + stale-generation writes; crash-replay; poison batch;
  tenant burst (100 vs 1); worker shutdown idle and mid-intent (lease spent, no orphan); run-as
  owner/admin-self rule; trigger needs channel + compatible input; 8-cap on activation and
  reconfiguration.
- `persistence/workflow/message_reply_tests.rs`: matched through the worker + second reply
  ordinary; unawaited send (`mentioned_or_reply` admits a Cc reply to a workflow message); wrong
  responder and after expiry; event-before-park (6 rechecks, no attempts spent) and grace
  fall-through; crash-replay; recorded-before-deadline; reply vs expiry race (no deadlock, agree).
- `persistence/workflow/message_ingress_tests.rs`: end to end (two messages, one thread, separate
  runs and replies, no legacy task); member email reply on an allowlist channel resumes the wait.
- `persistence/workflow/message_thread_tests.rs`: user send thread participation under both
  policies + guard/CHECK rejections; in-app reply; relay trace from the intent.
- `persistence/thread/inbound_multi_task_tests.rs`: intents per association, facts, audience
  (Cc-only answer = external conversation, FileOnly = internal only), tenant FKs.
- `registry/message_tests.rs` reply-wait refusals; `fixtures/tests.rs` message-started fixtures
  accept the standard input; `infra/workflow.rs` memory refusal + WF-C001 parent publication,
  child admission refused, workers stop on the token, pool defaults.

Checks (phase cluster, `DATABASE_URL=postgres://workflow08@127.0.0.1:55480/workflow08`,
`SQLX_OFFLINE=true`, test DB `workflow08_test` recreated after each init-schema edit):
- `cargo test --lib -- use_cases::thread:: use_cases::approval:: persistence::thread::` → 151 ok
  (`logs/b1-run3.log`).
- `cargo test --lib -- persistence::workflow:: application::workflow:: infra::workflow domain::workflow adapters::workflow`
  → 1164 ok, 1 failed (the relay-trace test, retargeted afterwards) (`logs/b1-run4.log`).
- New-test runs: `logs/b1-run5.log` … `logs/b1-run15.log` (all green at their last run).
- clippy `--all-targets -D warnings`, `cargo fmt --check`: clean.
- Broad focused run of every touched module, `cargo test --lib -- adapters::persistence
  application::workflow application::use_cases::thread application::use_cases::approval infra
  domain::workflow adapters::http adapters::workflow` → **2144 passed, 0 failed, 2 ignored**
  (pre-existing opt-in: `dump_application_javascript`, HydraDB adapter) (`logs/b1-focused-final.log`).
- Stack budget `scripts/stack-budget.sh -- message_tests::ingress_tests message_tests::intent_tests
  infra::workflow message_tests::thread_tests` at 2048 KiB → 35 passed (`logs/b1-stack-budget.log`).
- Fresh migration: init schema applied with `ON_ERROR_STOP` to scratch DB `scratch08` (dropped
  after), both new tables present (`logs/b1-fresh-migrate.log`, empty = no errors).
- sqlx offline cache: no changed file uses `query!`/`query_as!`/`query_scalar!`, so `.sqlx` is
  unaffected; `cargo sqlx prepare` not run (would need the user's 5432 database).
- Not run (by design until the whole task completes): the full suite.

Batch artifact for review WF-P08-R004: base `da868710d3b235d49856e3e481f8f3735f6b3e56` (no
commits since; the whole batch is the working tree), digest
`90d4e0abbb34a6426258697b3459e8ba3dbdc6889dfeb8fba163f764370e7390`, exclusions exactly
`plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md marketing
website`; manifest `logs/b1-snapshot.txt`. Coordination r6 (sha256 `23fdc190…c4db`) unchanged.

## B1 corrections for WF-P08-R004

B's R004 (digest `90d4e0ab…7390`): changes required, F021 Medium, F022–F026 Low, F027 optional.
All fixed in one round:
- **F021:** `message_reply_to_a_workflow_reply_matches_only_the_customer_it_answered` (`reply`
  kind: the source's author matches, another thread party is admitted as ordinary ingress) and
  `message_reply_to_a_channel_post_matches_only_an_in_app_answer` (`channel` kind: `external` and
  `internal` origins are unmatched, `application` matches). The reply fixture now asks through any
  of the three destination kinds (`Ask::{Member, Customer, Channel}`).
- **F022:** `.env.example` (pool default 20, `WORKFLOW_WORKER_SLOTS` with range and `8 + 2 × slots`)
  and `docs/deploy.md` settings table.
- **F023:** `MessageAdmissions::claim` fails lease-expired last attempts through the new port
  method `fail_expired_intents` (the sweep left `claim_intents`) and a transient deferral on the
  last attempt goes through `defer`; both log an error and count
  `workflow_message_intents_failed_total{cause=lease_expired|transient}`. The worker loop claims
  through `MessageAdmissions::claim`. Tests:
  `message_intent_lease_lapsing_on_the_last_attempt_fails_it_observably`,
  `message_reply_delivery_failing_on_the_last_attempt_fails_the_intent_observably` (run row held
  so the reply's delivery hits its lock timeout).
- **F024:** new column `workflow_message_intents.reply_settled` (CHECK: only with a
  `reply_to_message_id`, never with outcome `matched`), set by the fenced port method
  `settle_reply` when correlation returns `Unmatched`; `process` offers a reply only while it is
  unsettled (`correlate` helper). Test: `message_reply_that_fell_through_is_never_matched_on_a_retry`
  (the reviewer's sequence).
- **F025:** `message_intent_rows_cannot_name_another_companys_records` (foreign binding, run,
  matched run, answered message and responder each raise 23503; the weak binding case left the
  thread test), `message_intent_admission_replay_with_another_delivery_policy_conflicts`,
  `message_intent_in_app_policy_reaches_the_admitted_run`,
  `workflow_wait_commit_guard_refuses_an_event_recorded_after_the_deadline` (direct SQL completion
  with a late event fails at commit with the guard's message).
- **F026:** deviation D-5 above.
- **F027 (optional, taken):** 1 `authorize_thread_injection` doc; 2 the inbox worker wakes the
  intent worker after a committing Resend/inbox event (`with_intent_wakeups`, wired in setup);
  3 `docs/deploy.md` states workflow `http.request` reaches public HTTPS origins only;
  4 failure-table addendum: a reply recorded just before the *run's* deadline stays unconsumed —
  run expiry (`expire_on`) fails the run regardless, as D1.5.6 states.

Checks: `cargo test --lib -- message_tests::intent_tests wait_guard_tests
persistence::thread::inbound_tests inbound_event_worker message_admission` → 56 passed
(`logs/r004-fix-run1.log`); `cargo test --lib -- wait_tests` → 20 passed (`logs/r004-fix-run2.log`);
clippy `-D warnings` and fmt clean; test DB recreated after the schema edit; fresh init-schema
apply clean (`logs/r004-fresh-migrate.log`). Broad focused rerun of the same module set plus
`application::services::inbound_event_worker` → **2158 passed, 0 failed, 2 pre-existing ignored**
(`logs/r004-fix-focused.log`).

Corrected artifact for recheck WF-P08-R005: base `da86871`, digest `500ae85b9eb6ecb3e11aa1739adf3c30fc56893db95fd232d3f001c6fc691b11`, same
exclusions; manifest `logs/b1-snapshot-r2.txt`.

## B1 corrections for WF-P08-R005

R005 (digest `500ae85b…1b11`): F021–F027 closed; new F028 Medium, F029 Low.

- **F028 (timeout read as undeliverable):** the adapter now classifies at the source. Its permanent
  causes become `ReplyIngress::Undeliverable("event_id_reused" | "event_inbox_full" |
  "payload_rejected")`, ordinary ingress, logged. Every `Err` from `deliver_reply` is transient,
  and exhausting the 2 s wall-clock budget is `AppError::DatabaseTimeout` (it was `lease::timed_out()`
  = `Conflict`); `deliver_within(.., budget)` exposes the budget for the test. Tests:
  `message_reply_delivery_out_of_time_is_retried_not_settled` (a 200 ms budget exceeded while
  the producer lock is held, no statement timing out → `DatabaseTimeout`; through the worker a
  lock timeout defers, leaves `reply_settled` false, and the reply matches on retry) and
  `message_reply_the_run_can_never_take_is_ordinary_ingress` (reused event id → settled,
  admitted).
- **F029 (flaky member-reply e2e) — root cause found:** a cancellation bug in sqlx-postgres
  0.8.6 `begin`. It increments `transaction_depth` only after `BEGIN`'s reply, and its cancellation
  guard (`Rollback` → `start_rollback`) does nothing at depth 0; so a `begin()` future dropped after
  `BEGIN` reached the server returns the connection to the pool **inside an open transaction**
  while sqlx believes it idle. The next user's "autocommit" statements then run uncommitted in
  it until some later `BEGIN`/`COMMIT` on that connection commits them.
  Evidence chain (logs `r005-diag*.log`):
  - reproduced about 1 in 10–50 runs of the isolated focused set;
  - pre-shutdown diagnostics: pool size 5 idle 5, all sessions idle, the reply event recorded and
    consumed, the run succeeded, the intent still `claimed` with a valid lease;
  - temporary step logs: `offer → Recorded`, `finish_intent → Ok(false)` 1 ms later, i.e. the
    fence missed the worker's own claim;
  - stress tests of finish under row locks never missed, so the claim itself was not yet
    committed (it ran inside a leaked transaction), and was committed later;
  - the sqlx source shows the mechanism, and `infra::db::tests::a_cancelled_begin_never_leaks_its_transaction`
    reproduces it deterministically (`begin_with("BEGIN; SELECT pg_sleep(0.5)")` cancelled at
    100 ms → the next connection is inside a transaction).

  The e2e test triggers it because every `run_until` phase cancels the workers mid-query.
  Production triggers it through any timeout or shutdown that drops a `begin`: 141 production
  `begin()` sites, many wrapped in `tokio::time::timeout`. This is a pre-existing hazard (p05–p07
  code), exposed by B1's tests.
  **Fix (shared infrastructure, one place):** `infra::db::discard_leaked_transactions` adds an
  `after_release` hook to the production pool and both test pools. A simple-protocol check
  (`transaction_timestamp() <> statement_timestamp()`) closes a connection released inside a
  transaction (the server rolls it back) and logs a warning. Cost: one round trip per release
  beside sqlx's own release ping. The regression test pins both halves: sqlx still leaks (so the
  guard is needed; the assertion fails, with a message saying so, once sqlx fixes it), and the
  guard prevents it with no false positive (an ordinary release keeps its backend pid).
  Also: the e2e engine runs one worker slot (the 5-connection test pool) and dumps queue, run,
  wait, pool and `pg_stat_activity` state on a timeout, taken before shutdown on a connection of
  its own. All temporary diagnostics were removed.

Checks after the R005 corrections (cluster 55480, `workflow08_test` recreated from the edited
init schema, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`):

| Check | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- adapters::persistence application::workflow application::use_cases::thread application::use_cases::approval application::services::inbound_event_worker infra domain::workflow adapters::http adapters::workflow` | 2161 passed, 0 failed, 2 ignored (pre-existing) | `logs/r005-fix-focused.log` |
| reply / intent / ingress modules | 32 passed | `logs/r005-fix-run1.log` |
| the built test binary run 60 times on `message_tests::intent_tests message_tests::ingress_tests message_tests::thread_tests wait_tests infra::workflow infra::db` (66 tests; before the guard this set failed about 1 in 10–50 runs) | 60/60 runs, 66 passed / 0 failed each | `logs/r005-verify-{1..60}.log` |
| `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` | clean | `logs/r005-clippy.log` |
| `scripts/stack-budget.sh -- message_tests::ingress_tests message_tests::intent_tests infra::workflow infra::db message_tests::thread_tests` (2048 KiB) | 46 passed | `logs/r005-stack-budget.log` |

The earlier multi-process stress runs (`logs/r005-stress-*`) are not evidence: concurrent
processes on one database deadlocked on the shared-database purge and exhausted shared memory in
parallel migrations. Diagnostic logs `r005-diag*`, `r005-repeat-*`, `r005-focused-rerun*` are
investigation evidence for F029.

## B2 plan (drafted 2026-10-23 during R006; starts only after B1 is accepted)

Scope per `logs/decision-D1-r3.md` §D1.0 and §D1.9, plus the manual start/rerun command (WF-C001
r2). Inventory: `logs/b2-inventory.md`. No new architecture gate: D1.9 is gated (rev 3 PASS). The
choices below are not covered by D1.9. They are cheap to unwind (no backward compatibility), so
they are reviewed in the B2 batch review rather than a separate gate.

**B2 choices not covered by D1.9**
- **B2-a (run-now):** `trigger_schedule_now(actor, company, schedule, command_key)` inserts or
  returns the occurrence `(company, schedule, command_key)` with `scheduled_for = now()`. It then
  materializes that occurrence inline through the same fenced claim → admit → record path as the
  materializer, and returns the occurrence and its run, if one exists yet. If inline
  materialization fails transiently, the occurrence stays pending for the materializer. A repeated
  key returns the same occurrence and run.
- **B2-b (slot key):** the slot key `(schedule_id, scheduled_for)` becomes a partial unique index
  `WHERE command_key IS NULL`, so a run-now occurrence never collides with a cadence slot.
- **B2-c (manual trigger identity):** a manual start's `TriggerId` is derived as the first 128 bits
  of SHA-256 over a domain tag, the company and the idempotency key (`sha2` is already a
  dependency). A replay of the same key therefore names the same trigger, and a different key
  never aliases it. The run's actor is the caller (D1.1). Delivery is `Deliver`.
- **B2-d (serialization):** schedule create, update, enable, delete and run-now run in one
  transaction. That transaction first takes `authority::authorize_company` (the company row
  `FOR NO KEY UPDATE`, the same first lock as binding saves and admissions), then the channel
  association check, then the binding compatibility checks, so schedule writes and binding
  reconfiguration are serialized per company. Binding `save_binding` revalidates referencing
  schedules for a `Configuration` change, in the same transaction.
- **B2-e (refusal):** a refused occurrence is marked `failed` with a `refusal` code; the schedule's
  `last_error` names it; the schedule stays enabled. A later successful materialization clears
  `last_error`, as it does today.
- **B2-f (UI):** schedule pages keep CRUD and run-now. The form picks a compatible binding instead
  of the template, delivery and run-as fields. The run list shows each occurrence's status,
  refusal or error, and its run id and state. The run-thread pane, reply-in-thread and the
  run-as picker are deleted. The JSON routes are retargeted the same way.

**Steps**
1. Schema (in-place init edit, then recreate `workflow08_test`):
   - `channel_schedules`: drop `subject_template`, `prompt_template`, `delivery_mode`,
     `recipient_emails` and `run_as_user_id`, together with their CHECK and FK; add `binding_id`
     NOT NULL with a composite FK to `workflow_bindings`.
   - `schedule_runs`: replace `thread_id`/`task_id` with `run_id`, which gets a composite FK to
     `workflow_runs`; add `refusal` and `command_key` with UNIQUE `(company_id, schedule_id,
     command_key)`; make the slot key partial (B2-b); rewrite the state CHECK so that materialized
     ⇔ `run_id`, and failed requires either 5 attempts or a refusal.
   - Rewrite `require_legacy_schedule_task` as an identity and scope guard with no task branch.
   - `background_tasks`: drop `source_schedule_run_id`, its CHECK, unique key and the
     `scheduled_agent_run` index and type references.
2. Domain `entities/schedule.rs`:
   - `ChannelSchedule` gains `binding_id` and loses the removed fields.
   - Delete `ScheduleDeliveryMode`, `ScheduleRunAs*`, `RunAsSelection`, `ScheduledRunPayload` and
     the template rendering.
   - `ScheduleRun` becomes an occurrence view (status, scheduled_for, run, refusal, error).
3. Persistence (`adapters/persistence/schedule.rs`):
   - Actor-scoped, company-locked writes (B2-d), with compatibility checks: the association is
     `Company` or `Channel(schedule.channel)`; the pinned input accepts
     `ScheduleTriggerInput::representative()`; the run-as is visible to the channel.
   - Claim and advance unchanged, apart from the snapshot.
   - `record_run(run_id)`, `record_refusal` and `record_run_error` fenced by worker, generation
     and unexpired lease (inventory gap 6).
   - Run-now insert-or-get.
   - Run listing over `workflow_runs`.
   - Remove `release_failed_claim`.
   - Admission source `schedule()` also requires `channel_schedules.binding_id = command binding`
     and a `Channel` association.
   - `bindings.rs` revalidation (B2-d), refusing with the schedule names.
4. Application:
   - Materializer `application/workflow/schedule_admission.rs`, which reuses the B1
     classification and Stale loop.
   - Admission request: actor = binding run-as; trigger `Schedule{schedule, occurrence}`; key
     `s:{occurrence}`; expected revision; association `Channel(schedule.channel)`; input
     `ScheduleTriggerInput{scheduled_for}`.
   - Spawned from `WorkflowEngine::spawn` as `workflow schedule materializer`, already counted in
     `RESERVED_DATABASE_CONNECTIONS`.
   - `ScheduleUseCases` keeps CRUD and authority (owner/admin plus channel visibility) and gains
     run-now (B2-a).
   - Manual start: `application/workflow/manual.rs` `ManualStarts::start(StartRun{company, actor,
     binding, association, input, idempotency_key, rerun_of})` → `StartedRun{run, replayed}`.
     `Conflict` maps to a 409 `AppError::Conflict`.
5. Delete scheduled dispatch:
   - `SCHEDULED_AGENT_RUN_TASK`;
   - TaskWorker `with_schedules` and its schedule poll;
   - the `dispatch.rs` scheduled branch;
   - `TaskSource::ScheduleRun`, its queue mapping, `ensure_schedule_run_thread` and the views
     joins;
   - `main.rs` wiring.
6. HTTP (`routes/schedule.rs`, `routes/ui_schedules.rs`, `pages/schedules.rs`, channel and agent
   settings, `ui_channels`, `ui_agents`, `live_updates`) retargeted per B2-f.

**Tests (D1.9 F011.4 + WF-C001):**
- duplicate slot under two competing materializers → one run;
- crash after admission and before `record_run` → the retry replays the same run, with no second
  agent call (scripted provider counts calls);
- run-now `command_key` replay;
- refusal → `failed` shape and `last_error`;
- a scheduled run driven end to end through the composed engine and agent runtime (scripted
  provider);
- authority and visibility refusals;
- a binding reconfigure refused by an incompatible schedule;
- a stale-generation record refused;
- schema CHECK, FK and tenancy cases;
- manual start created, replayed, and conflicting with a different input;
- `rerun_of` linked and exposed by inspection; a rerun by a non-admin refused.

**Focused checks:**
- `cargo test --lib -- schedule adapters::persistence::workflow application::workflow
  infra::workflow adapters::http::routes::schedule adapters::http::routes::ui_schedules
  adapters::http::pages services::task_worker use_cases::thread`, plus the callers found by
  exhaustive `grep -rn` of the removed symbols;
- clippy, fmt, fresh migration;
- stack budget for the new materializer tests.

## B2 implementation log (2026-10-23)

Base: B1 accepted at digest `b90a7f9c…a468` (uncommitted tree on `da86871`). Coordination r8
(WF-C004 r2). Plan: "B2 plan" above (choices B2-a..f).

**F030 (B1 Info finding):** `test_support::pool_options()` (guarded) now backs `test_pool`,
`own_database` and nine test-only pools (thread lock support, claim ×2, activation, completion,
batch recovery, budget activation, child runtime, action reconciliation other-audit).
`lease_resolution_tests` keeps one documented bare pool, because the guard's release statement
would replace the `ROLLBACK` that test reads from `pg_stat_activity`. This exemption was
published as WF-C004 r2 (coordination r8, sha256 `0950b907…5e22`, copy
`logs/coordination-r8.md`). `thread/read_tests.rs` is a lazy pool that never connects.

**Implemented**
- **Schema** (init migration in place):
  - `channel_schedules`: + `binding_id` (composite FK to `workflow_bindings`, index); − subject,
    prompt, delivery mode and recipients, − `run_as_user_id` and its FK.
  - `schedule_runs`: − `thread_id` and `task_id` with their FKs and CHECK; + `binding_id`
    (captured), `run_id` (FK `(company_id, run_id, binding_id)` → `workflow_runs`), `refusal`
    (CHECK of codes), `command_key` (CHECK 1..200, UNIQUE `(company_id, schedule_id,
    command_key)`); slot key → partial unique index `WHERE command_key IS NULL`; state CHECK
    rewritten (materialized ⇔ run; failed ⇔ 5 attempts or a refusal).
  - Trigger `require_legacy_schedule_task` → `require_schedule_occurrence_scope`. It captures
    company, channel and binding from the schedule under `FOR SHARE`; identity (incl. binding,
    `scheduled_for`, `command_key`) is immutable; the snapshot scope includes `binding_id`; a run,
    once set, is immutable and must be this occurrence's (`trigger_id = occurrence`, same channel,
    no thread).
  - `background_tasks`: − `source_schedule_run_id`, its CHECK, unique key and schedule index,
    and the workflow-shape term.
- **Domain** `entities/schedule.rs`:
  - `ChannelSchedule` and `ScheduleWrite` carry `binding_id` instead of the template, delivery
    and run-as fields.
  - `ClaimedScheduleRun` carries the captured binding.
  - `ScheduleRun` is an occurrence view (`OccurrenceStatus`, manual flag, run and state,
    refusal, error).
  - Deleted: delivery mode, run-as types, `ScheduledRunPayload`, template rendering.
  - `TaskSource::ScheduleRun` deleted.
- **Application**:
  - `workflow/schedule_admission.rs`: `ScheduleOccurrences` port, `ScheduleMaterializer`
    (classification and Stale loop shared with B1 through `message_admission::classify`, refusal
    metric `workflow_schedule_admission_refusals_total`), `ScheduleRunNow`, and
    `ScheduleMaterializerWorker`.
  - `workflow/manual.rs`: `start(StartRun)` → `StartedRun{run, replayed}`. Client key `u:{key}`
    (B2-c namespace), trigger SHA-256-derived (UUID v8), `Conflict` → `AppError::Conflict`, ≤3
    Stale retries.
  - `use_cases/schedule.rs` rewritten:
    - preflight through `WorkflowAuthorization` (owner/admin plus channel visibility);
    - company lists filtered to visible channels;
    - `binding_choices`;
    - `trigger_schedule_now(.., command_key)` (B2-a).
- **Persistence** `workflow/schedules.rs`:
  - CRUD and run-now under `authorize_company` then channel association (B2-d);
  - binding compatibility (association, input schema, run-as visibility);
  - `revalidate_schedules`, called from `bindings::save_binding` for a reconfiguration;
  - the cadence claim (moved);
  - `claim_occurrence`; `schedule_binding`;
  - fenced outcome writes, which lock the schedule row before the occurrence (the order
    schedule writes, deletes and admissions take) and keep `last_error` (B2-e).
  - `admission_source::schedule` requires a `Channel` association and the occurrence's captured
    binding.
- **Removed scheduled dispatch**:
  - `SCHEDULED_AGENT_RUN_TASK`;
  - the TaskWorker schedule loop and `with_schedules` (`Polled::MoreWaiting` is test-only now);
  - the `dispatch.rs` scheduled path and its evidence/anchor helpers;
  - `ensure_schedule_run_thread`;
  - the queue's schedule source;
  - `main.rs` wiring;
  - the legacy persistence adapter `adapters/persistence/schedule.rs`.
- **Engine**: `WorkflowEngine::spawn` adds `workflow schedule materializer` (already counted in
  `RESERVED_DATABASE_CONNECTIONS`). `authorization()` and `schedule_run_now()` feed
  `ScheduleUseCases` in setup.
- **HTTP** (B2-f):
  - JSON `ScheduleRequest` takes `binding_id` and refuses removed fields (`deny_unknown_fields`);
  - `run-now` takes `{command_key}` and returns the occurrence;
  - new `GET …/schedules/{id}/runs` and `GET …/channels/{channel_id}/schedule-bindings`;
  - the workspace, channel card and agent card pick a binding;
  - the runs column lists occurrences (status, run id and state, refusal or error, run-now
    marker);
  - run-thread pane, reply-in-thread and run-as picker deleted;
  - run-now forms carry a fresh `command_key` per render.
- **README**: task-worker section and diagram no longer describe scheduled dispatch; a
  "Schedules on Workflow Bindings" bullet was added.

**B2 refinements of D1.9, for review**
- **B2-g:** the occurrence captures its binding relationally (`schedule_runs.binding_id`, set by
  the trigger from the schedule at capture). Admission checks the captured binding, so
  retargeting a schedule never changes or breaks what an already-captured occurrence admits,
  including its replay.
- **B2-h:** `schedule_runs.run_id`'s FK includes `binding_id`, so a recorded run is always a run
  of the captured binding. The trigger also checks that its trigger is the occurrence.
- **B2-i:** schedules are offered and allowed with an inactive binding (labelled "inactive"); its
  occurrences are refused `unavailable` until the binding is active again.

**Known limits** (not B2 scope):
- The runs column's live stream still wakes on channel message activity, so an occurrence's
  status change appears on reconnect or refresh. p09 builds the operations UI.
- The run id is shown, but the run detail page is p09.
- The legacy thread views, their index expression and task-monitor tests still mention the
  string `scheduled_agent_run` as a sort/match key that no row can have now. They belong to the
  legacy task views B4 deletes.
- `graft build` is unavailable (the `graft` executable is not installed; the MCP server failed to
  start), so the context graph is stale.

**Tests added or changed**
- `workflow/schedule_tests.rs` (14), all DB-backed:
  - competing materializers → one run;
  - crash after admission → replay, no second run;
  - refusal shape and `last_error`, cleared by a later run;
  - run-now key replay and the slot not colliding;
  - outcome fences (worker, generation, company, reclaimed lease);
  - authority (member; admin without allowlist grant; persistence recheck);
  - compatibility (other channel's binding, input schema, run-as visibility, unknown binding);
  - reconfigure refused naming the schedule (input schema, run-as visibility);
  - slot anchoring (ported);
  - schema scope, identity, snapshot, run scope/FK, refusal shape;
  - capture serializing with a rejected channel move (ported);
  - manual start create/replay/conflict/rerun link (WF-C001), key namespace, non-admin refused.
- `schedule_runtime_tests.rs`: through the production `WorkflowEngine` and scripted provider, a
  due slot whose first materialization crashed after admission → one run, succeeded, recorded on
  attempt 2, the agent called once, no legacy task or thread.
- Unit: `manual` (trigger stability, key namespace), routes (`ScheduleRequest` refuses removed
  fields, form binding parse, run-now key), workspace render tests (occurrences, binding picker,
  removed fields absent, routed paths), `validate_write`.
- Changed:
  - `admission_source_tests`: occurrences carry a binding, plus a new "other binding" refusal;
  - route schedule tests use a real binding;
  - pages tests;
  - the e2e engine comment (5 connections: slot 2, intent 1, materializer 1, test 1);
  - `persistence::tests` schema pin (`background_tasks_single_source_check` gone).
- Deleted: TaskWorker scheduled-run tests (3) and anchor tests (2), and the legacy schedule→task
  tenant tests (`job_tenant_*`), whose scope and identity cases are ported into `schedule_tests`.

**Checks** (cluster 55480, `workflow08_test` recreated after the schema edit, `SQLX_OFFLINE=true`,
`CARGO_INCREMENTAL=0`):

| Check | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- adapters::persistence application::workflow application::use_cases application::services infra domain adapters::http adapters::workflow` | 2529 passed, 2 failed, 2 ignored (pre-existing). Both failures were B2-caused (schema pin; F030 pool in `lease_resolution`), fixed and rerun green | `logs/b2-focused-1.log` |
| `schedule_tests`, `application::workflow::manual` | 14 + 2 passed | `logs/b2-schedule-tests-1.log` |
| `message_tests::ingress_tests` (incl. `schedule_runtime_tests`) | 3 passed | `logs/b2-e2e-1.log` |
| built binary ×15 on `schedule_tests schedule_runtime_tests message_tests::ingress_tests infra::workflow` | 15/15 green, 20 tests each | `logs/b2-repeat-{1..15}.log` |
| `scripts/stack-budget.sh -- schedule_tests schedule_runtime_tests application::workflow::manual routes::schedule routes::ui_schedules infra::workflow` (2048 KiB) | 33 passed | `logs/b2-stack-budget-1.log` |
| `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` | clean | `logs/b2-clippy-1.log` |
| schema applied to a scratch DB (`psql -v ON_ERROR_STOP=1`), then the test DB recreated | OK | — |

## B2 corrections for WF-P08-R007

R007 (digest `7b932911…5c51`): changes required, all Low; F030 closed. All actionable findings
were fixed in one round, and the optional F034 items were adopted too.

- **F031 (nested routes ignored the path channel):**
  - `use_cases::schedule::ScheduleScope { company_id, channel_id: Option }`; a bare company id
    is the workspace scope.
  - `managed_schedule` returns not-found for a schedule of another channel. A schedule's channel
    is immutable, so the preflight holds.
  - `get`, `delete`, `toggle`, `list_runs` and `trigger_now` take the scope. Every nested JSON
    route and the `/ui/channels/{channel_id}/schedules/{id}/…` routes pass
    `ScheduleScope::channel(company, path channel)`; `_channel_id` is gone.
  - Test `routes::schedule::schedule_channel_tests::nested_schedule_routes_refuse_another_channels_or_companys_schedule`
    covers:
    - another channel, another company, and another company with the right channel id, on
      `GET`, `/runs`, `run-now`, `toggle` and `DELETE` (JSON) and on the UI `run-now`, `toggle`
      and `delete`, all 404;
    - the schedule unchanged afterwards (still paused, no occurrence);
    - the right path working;
    - `schedule-bindings` refusing a foreign channel.
- **F032 (lost legacy coverage, B2-g untested, fence change unrecorded):** new DB tests in
  `schedule_tests`:
  - `schedule_poison_batch_is_not_reclaimed_by_the_next_iteration`: a full page of 10
    transient failures; the second iteration claims nothing.
  - `schedule_edit_keeps_its_next_run_and_pause_unless_the_cadence_changes`.
  - `a_captured_one_off_schedule_is_disabled`.
  - `retargeting_a_schedule_after_capture_keeps_the_captured_binding` (B2-g): the old
    occurrence runs under binding A and the next slot under B.
  - `a_lapsed_but_unreclaimed_claim_still_records_its_outcome`.
  - **Recorded decision (fence):** outcome writes are fenced by status, worker and generation,
    not by lease expiry. A reclaim mints a new generation, so a lapsed but unreclaimed claim is
    still the latest one and may record what it did; for example a slow 5th attempt records its
    run before the terminal sweep. This departs from plan step 3's "unexpired lease" wording and
    is documented on `PostgresPersistence::record`.
- **F033 (terminal failure and lease loss invisible):**
  - The port gains `fail_expired_occurrences(page) -> Vec<FailedOccurrence>`. It uses two
    autocommit statements: the occurrences first, then their schedules' `last_error`. Neither
    holds the other's lock, keeping the schedule-then-occurrence order of every other writer.
  - The terminal CTE left `claim_due`; `ScheduleMaterializer::claim` sweeps first, as B1's intent
    queue does.
  - `record_error` returns `ErrorRecorded { Retrying, Failed, Lost }`.
  - New `Materialized::Failed`.
  - Terminal failures log an error and count `workflow_schedule_occurrences_failed_total{cause=transient|lease_expired}`;
    a lost claim logs a warning and counts `workflow_schedule_occurrence_leases_lost_total`.
  - The lease-lapse path names the failure on the schedule.
  - Test `schedule_terminal_failures_and_lost_claims_are_observable` covers all three paths,
    with the in-memory monitor.
- **F034 (optional), adopted:**
  1. The runs column polls `/ui/schedules/runs` every 15 s (the fragment route now answers with
     the column alone). The SSE route `/ui/schedules/{id}/events`, `schedule_runs_stream` and
     `live_updates::channel_wake_ups` (which only it used) are deleted, and the live-updates
     guard test is narrowed to the remaining streams.
  2. `docs/rig.md` and `docs/transport_architecture.md` no longer describe scheduled dispatch;
     the `ConversationAnchor` comment says nothing produces one since p08.
  3. The leftover `pid` was removed.

Checks:

| Check | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- schedule_tests schedule_runtime_tests routes::schedule routes::ui_schedules routes::live_updates routes::ui_channels routes::ui_agents adapters::http::pages application::workflow::manual application::use_cases::schedule infra::workflow message_tests::ingress_tests` | 279 passed, 1 ignored (pre-existing) | `logs/r007-fix-1.log` |
| rerun after the clippy fix (`routes::live_updates schedule_tests schedule_runtime_tests routes::schedule`) | 29 passed | `logs/r007-fix-2.log` |
| built binary ×10 on `schedule_tests schedule_runtime_tests routes::schedule message_tests::ingress_tests admission_source` | 10/10, 34 passed each | `logs/r007-fix-repeat-{1..10}.log` |
| `scripts/stack-budget.sh -- schedule_tests schedule_runtime_tests routes::schedule routes::ui_schedules` | 33 passed at 2048 KiB | `logs/r007-fix-stack-budget.log` |
| `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` | clean (after replacing a single-element loop in the live-updates test) | `logs/r007-fix-clippy.log` |

## B3a implementation log

### F035 (optional, from WF-P08-R008), fixed first

- `PostgresPersistence::fail_expired_occurrences` (`src/adapters/persistence/workflow/schedules.rs`):
  the per-schedule `last_error` update is best effort. A failure logs a warning naming the
  company and schedule, and the already-failed occurrences are still returned. The materializer
  therefore still logs them as errors and counts
  `workflow_schedule_occurrences_failed_total{cause=lease_expired}`. The occurrence row carries
  its own `last_error` either way.
- A crash between the two statements still loses the in-process log and metric; the occurrence
  row is correct. Not fixed: one transaction would need schedule-first locking over a batch (a
  new lock protocol for an Info finding).
- Test `schedule_tests::an_expired_occurrence_is_reported_when_its_schedule_cannot_name_it`: a
  `BEFORE UPDATE` trigger limited to the test's schedule id rejects the update. The sweep still
  fails the occurrence, `claim` returns `Ok`, the counter is 1, and the schedule's `last_error`
  stays empty.

| Check | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- schedule_tests schedule_runtime_tests` | 20 passed | `logs/f035-fix.log` |
| same plus `routes::schedule`, ×5 | 5/5, 27 passed each | `logs/f035-repeat-{1..5}.log` |

### B3a implementation (gate P08-D2 rev 2 plus F046–F049)

**Application:**
- `src/application/workflow/provisioning.rs` (new):
  - `ProvisioningKind`, `is_builtin_provisioning_tool`, and `is_reserved_tool_name` (the message
    and provisioning names);
  - fixed schemas (`provisioning_tool_schemas`), each output `oneOf` the success shape or the
    `tools::tool_error` shape;
  - `builtin_provisioning_snapshot` / `is_trusted_provisioning_snapshot`;
  - `ReplyMode`, `CreateAgentChannel`, `ProvisioningRefusal` (Forbidden / Conflict / Invalid →
    the `workflow.*` codes);
  - `ProvisioningActions::call_tool`: input-schema refusal before any marker, then
    `prepare_tool` and `dispatch_local`;
  - the port `AssistantChannels` with `AssistantChannelRequest`.
- `ToolKind::Provisioning` is the classification for a provisioning name with no connection
  slot and no workflow pin. Every `ToolKind` match handles it:
  - `authority::supported_publication` and `resources::check_readiness`, through
    `builtin_provisioning_tool`;
  - `agent_preparation::targets`, which builds `AgentToolTarget::Provisioning`;
  - `agent::prepared_tool`;
  - `granted_tool`.
- F047: `granted_tool` resolves a provisioning name to the built-in before anything captured. A
  captured snapshot of that name is reused only if it is the trusted built-in; otherwise the grant
  is ambiguous.
- Reserved names are refused at:
  - HTTP operation validation (`HttpOperation::validate`, which also closes `message.*` there);
  - workflow-tool registration;
  - the `tool.call` literal (the registry). `mcp.call` names stay the server's own.
- `/run/deadline`:
  - `ReferenceRoot::RunDeadline` and `RunMetadata.deadline`, with
    `render_deadline` = UTC RFC3339 with microseconds and `Z`;
  - activation and round contexts read it from the run row; admission has none, so the reference
    resolves as missing there;
  - the compiler accepts it like `/run/id`;
  - documented in `docs/workflow-language-v1.md`.
- `reviewed-support.yaml`: `deadline: {ref: /run/deadline}`; its parameters are `{reviewer}`.
- `TemplateCatalogue::shipped()` holds `autonomous-response` and `reviewed-support` (each
  dependency set derived from its own source). It adds `template(id)`, `REPLY_AGENT_STEP`, and
  `CompanyWorkflowDraft::from_template`. `compiler::retarget_agent` replaces only the located
  `agent` literal scalar, double-quoted, then re-decodes and checks it.
- Lifecycle:
  - `compile_bundle` is the pure core of `DefinitionService::compile` (freeze, workflow
    identity, refused kinds);
  - `provisioned_publication`;
  - `provisioned_binding` builds configure and activate through `ConfiguredBinding::new` and
    `check_message_trigger`, and refuses resource slots.
- `use_cases::agent::model_allowed` is the pure core of `validate_model_selection`.
- Agent Builder (`builtin_agent_library`): grants `list_company_agents, create_agent_channel`;
  the prompt is revised per D2.6.
- Library copy (F044):
  - `create_owned_agent_channel` copies the library row's `workflow_tools` in its transaction;
  - a copy that grants tools and would inherit gets the company default connection's first model,
    when that connection has a key;
  - the use case warns `library_model_pinned`.

**Persistence:**
- `src/adapters/persistence/workflow/provisioning.rs` (new):
  - `ProvisioningActionAuthority`, which mirrors `MessageActionAuthority`;
  - `ProvisioningPreparation`;
  - `ProvisioningTemplates` and `ProvisioningEffects`, the first production `SqlLocalAction`;
  - `requester_on` (F048): the root through `parent_run_id` (at most 64 levels), then the root's
    single stored `workflow_admissions.source_key` through the pure `requester_source`:
    - manual or schedule → the root actor;
    - message → the person author, only if the intent's origin is `application`;
    - anything else → none;
  - `authorize_requester`: the not-found answer of the requester's own `authorize_company` is
    the Forbidden refusal;
  - `list_on`;
  - `provision_guarded` and `provision_on`;
  - `AssistantChannelService`, which reads the command record first under the company lock,
    keyed by a digest that includes the caller (F049.3), and writes it with the result.
- F046 refusals are built only at the named checks:
  - the slug pre-checks (`taken_slug`);
  - the slug format check (`malformed`, the DB `*_slug_format` rule);
  - `normalize`;
  - model, tool and skill resolution;
  - the capture's and compiler's `BadRequest` and diagnostics;
  - the requester checks.
  An error inside the savepoint rolls it back. A fresh `taken_slug` then decides between the
  conflict refusal (another creator took the slug) and propagating the error.
- Extractions shared with the existing callers:
  - `save_binding_on`;
  - `publish_on` and `insert_draft_on`;
  - `capture_agents_on` and `agent_selection`;
  - `insert_channel_row_on` (all three channel-creation paths);
  - `message::message_builtins`.
- `SqlLocalAction::apply` also receives the `ApprovalSubject`, so ids derive from the invocation.
  `NoLocal` and the two test effects are updated.
- Schema: `workflow_provisioning_commands` (primary key `(company_id, command_key)`, a 64-hex
  digest, a result object ≤ 4 KiB, cascade with the company).

**HTTP:**
- `routes/assistant_channels.rs`: `POST /api/companies/{company}/assistant-channels` (201) and
  `POST /ui/agents/{agent}/assistant-channel` (re-renders the card).
- Refusal mapping: Forbidden → 404, Conflict → 409, Invalid → 400 (F049.1).
- `FromRef<AppState>` for `Arc<dyn AssistantChannels>`.
- `pages::assistant_channel_card` sits on the agent's Settings tab.

**Engine:** `WorkflowEngine` builds the shipped catalogue at startup, so a template that does
not compile stops `serve`. It composes `ProvisioningRuntime` into
`AgentActionTools::with_provisioning` and exposes `assistant_channels()`.

**Deviations from D2 rev 2** (for the B3a review):
- **DV-1:** D2.3 said "the inner directories capture nothing". That is wrong: the production
  message directory always captures both `message.*` built-ins, and a template without them
  cannot reply (found by the end-to-end test: the reply failed with
  `message.rejected`/NotFound). Provisioning starts its capture from the same
  `message_builtins`, under the company lock it holds.
- **DV-2:** the agent principal id is the helper's random id, not derived. Idempotency rests on
  the action receipt (tool) and the command record (UI), not on ids.
- **DV-3:** the compiler adds no schema typing for `/run/deadline`, as for `/run/id`. Reference
  analysis has no consumer typing, and the step input schema validates the resolved value at
  activation.
- **DV-4:** an extra pure check, `malformed`: `ChannelWrite::normalize_with` does not enforce the
  DB slug character set, which surfaced as a CHECK violation.
- **DV-5:** the conflict after a race is decided by a fresh slug check after the savepoint
  rollback, not by matching the constraint name. This is equivalent (it refuses only when the
  slug is committed by another creator) and does not depend on which helper wraps the
  unique-violation error.
- **DV-6:** `TemplateCatalogue::shipped()` holds only the two reply-mode templates; the other
  fixtures join with B3b's copy route, which also supplies their tool facts.

**B3a checks** (cluster 55480, `workflow08_test` recreated after the schema edit, schema applied
cleanly to a scratch DB with `psql -v ON_ERROR_STOP=1`, `SQLX_OFFLINE=true`):

| Check | Result | Log |
| --- | --- | --- |
| unit: provisioning, templates, compiler, fixtures, registry, publication, http, lifecycle, builtin library, domain context | 149 passed | `logs/b3a-unit-1.log` |
| `provisioning_dispatch_tests` (competing claimants, stale fences, refusals, injected fault, address race, child requester, listing, command replay/conflict/races, F047 capture) | 10 passed | `logs/b3a-dispatch-3.log`, `logs/b3a-focused-1.log` |
| `provisioning_runtime_tests` (production engine: signed-in admin end to end with the created channel answering; member and emailed admin forbidden; reviewed deadline = run deadline, reviewer = requester) | 3 passed | `logs/b3a-runtime-4.log` |
| focused: library Builder copy (F044), reserved names (registry, HTTP, workflow tools), shipped catalogue | 26 passed | `logs/b3a-focused-1.log` |
| broad `cargo test --lib` | 2773 passed, 1 failed (`pages::tests::no_page_reaches_for_a_class_daisyui_5_removed`: the new card used daisyUI 4 classes; fixed), 22 ignored | `logs/b3a-broad-1.log` |
| after the fix: `adapters::http::pages routes::assistant_channels routes::ui_agents agent_api_tests` (includes the new route test) | 228 passed | `logs/b3a-pages-routes.log` |
| built binary ×5: `provisioning_dispatch_tests provisioning_runtime_tests assistant_channel` | 5/5, 14 passed each | `logs/b3a-repeat-{1..5}.log` |
| `scripts/stack-budget.sh -- provisioning_dispatch_tests provisioning_runtime_tests assistant_channel schedule_runtime_tests` | 15 passed at 2048 KiB | `logs/b3a-stack-budget.log` |
| `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` | clean | `logs/b3a-clippy.log` |

Snapshot `logs/b3a-snapshot.txt`:
- base `da86871`, digest `9bdad3dae00d010689c5660c3c9a0c8895145079ea6c1989cc3dea27287d1f3c`;
- same exclusions;
- coordination r8 (`0950b907…5e22`).

The files changed since the B2 snapshot are listed in `logs/b3a-files.txt`. `message_publish.rs`
and `message_runtime.rs` appear there only because a temporary diagnostic line was added and then
removed; their content is unchanged. Sent to B as batch review WF-P08-R011.

## B3a corrections for WF-P08-R011

R011 (digest `9bdad3da…1f3c`) required changes: F051 (Medium) and F050 (Low). F035 and F046–F049
are closed, and DV-1..DV-6 are accepted.

- **F051 (address without the app domain):**
  - `ProvisioningTemplates` is renamed `ProvisioningDeployment` and carries `app_domain`
    (`APP_DOMAIN_NAME`, from `WorkflowEngine::new`).
  - `provision_on` builds the address with `Channel::address_for(channel, company, app_domain)`,
    giving `{channel}@{company}.{app_domain}`.
  - Exact-address assertions were added to:
    - the dispatch competing-claimants test;
    - the production-engine end-to-end test (`d.address("helper")`);
    - the route test;
    - the library Builder copy test.
- **F050 (listing size):**
  - Bounds: names are cut to `MAX_LISTED_NAME_CHARS` (120) and descriptions to
    `MAX_LISTED_DESCRIPTION_CHARS` (300), both also as `maxLength` in the output schema. Entries
    stop at `MAX_LISTING_BYTES` (48 KiB, under the 64 KiB receipt bound), models first and then
    agents by slug. More than 200 agents is detected with `LIMIT 201`.
  - The output gains a required `truncated` field.
  - Test `provisioning_lists_an_oversized_directory_within_its_bound`: 205 agents with 210-char
    names and 600-char descriptions. The call commits, `truncated` is true, the listing is within
    the bound, and the fields are cut. Agent names cannot exceed 255 bytes, which is the
    principal label's CHECK.
  - The unit schema test checks the name bound.

| Check | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- provisioning assistant_channel library_builder_copy infra::workflow` | 30 passed, 1 ignored (HydraDB live smoke, pre-existing) | `logs/r011-fix-1.log` |
| built binary ×3 on `provisioning assistant_channel library_builder_copy` | 3/3, 25 passed each | `logs/r011-fix-repeat-{1..3}.log` |
| clippy `-D warnings`, fmt | clean | `logs/r011-fix-clippy.log` |

Snapshot `logs/b3a-snapshot-r2.txt`: digest
`22334ec4414b79d0a68f8d052a3c787fca241611b8afa44e5bcd24ca7a4e77e1`, same base and exclusions,
coordination r8. Sent to B as recheck WF-P08-R012.

## B3b plan (2026-10-23)

**Batch change: split B3b.**
- **B3b-1** (established patterns, no gate) covers every interface below except action
  reconciliation.
- **B3b-2** covers the action-reconciliation route. It needs a production reconciliation
  composition that exists today only in test support (`action_reconciliation_authority_support.rs`):
  - `SqlReconciliationResources` for the HTTP and MCP targets (messaging has one);
  - an evidence-verifier registration;
  - the `ActionReconciliationService` wiring.

  That is a new cross-subsystem authorization composition, costly to unwind, so per the skill it
  gets its own gate, **P08-D3**, before B3b-2 code.

**B3b-1 interfaces.** Company-scoped JSON routes; every one passes the path company and the
signed-in actor to the owning command, whose authorization decides:
- **Definitions** under `/api/companies/{c}/workflow-definitions`:
  - list;
  - `GET`/`PUT /{w}`: read, and save with an expected revision;
  - `POST /{w}/validate`;
  - `POST /{w}/publish`: expected revision, idempotency key, HTTP/MCP selections;
  - `POST /{w}/archive`: expected revision.
- **Templates:** `GET /api/companies/{c}/workflow-templates` (the shipped catalogue) and
  `POST /{id}/copy`. The workflow id is derived from `(company, key)`, so a retry finds the same
  draft.
- **Bindings** under `/api/companies/{c}/workflow-bindings`: list, read, `PUT /{b}` configure
  (expected revision), and `POST /{b}/activate` and `/deactivate` (expected revision).
- **Runs:**
  - start: `POST /api/companies/{c}/workflow-bindings/{b}/runs`, through B2's `manual::start`
    (`u:` key namespace, rerun);
  - `GET /api/companies/{c}/workflow-runs` (by binding and state) and `GET /{r}`;
  - `POST /{r}/cancel` and `/retry` (expected revision; the WF-C002 child rule: a child is
    controlled through its root).
- **Decisions** under `/api/companies/{c}/workflow-decisions/{d}`: read, `POST /submit` and
  `POST /comments` (command keys), plus the minimal page `/ui/workflow-decisions/{d}` that the
  notices link to. The page resolves the company from the decision, then `read_decision`
  authorizes.
- **Workflow tools (WF-C003)** under `/api/companies/{c}/workflow-tools`: list, `PUT /{name}`
  register (expected revision), `DELETE /{name}` revoke (expected revision).
- **Agent grants:** `PUT /api/companies/{c}/agents/{a}/workflow-tools`, replacing
  `agents.workflow_tools`. The P07-Q2 semantics hold: revoke stops, re-pin and archive do not.

**B3b-1 verification:** route tests per area covering:
- authority: another company's resource → 404, and a member → 404 or 403 as the command answers;
- a revision conflict → 409;
- idempotent replay with the same key, and the same key with a different request → 409;
- revoke stops tool calls (WF-C003);
- the decision page renders, and a submit settles.

## B3b-1 implementation log (2026-10-23)

Artifact: snapshot digest `22923f7f5c2c0e626ae535f8452c946f1e1c1028a3d859fd41b7f11484792c20`
(base `da86871`, exclusions `plan/workflow-foundation/phases/p08/docs
plan/workflow-foundation/COORDINATION.md marketing website`; manifest `logs/b3b1-snapshot.txt`).
Files touched since the B3a snapshot: `logs/b3b1-files.txt`. Coordination r8 (`0950b907…5e22`)
unchanged; WF-C002 (child labels) and WF-C003 r1 (tool routes, grants) applied here.

**Application.**
- `application/workflow/reads.rs` (new): the `WorkflowReads` port: definitions, bindings, runs
  (filter by binding/state), run detail (executions, waits with decision/child ids), and a
  decision's company. Each listing is bounded at `MAX_LISTED` = 200.
- `authorization.rs`: `WorkflowOperation::Inspect`, for route reads; `RelatedAssociation` derives
  `Hash`, for the per-request visibility memo.
- `decisions.rs`: `DecisionView.choices`. A business decision has its declared `Routes::Choices`
  names in order; an approval has `approve`/`reject`.
- `tools.rs`: `WorkflowTools::grant_agent_tools(company, actor, agent, expected, tools)` and
  `MAX_AGENT_TOOL_GRANTS` = 32.

**Persistence.**
- `workflow/reads.rs`: plain company-scoped reads, with no locks.
- `decisions.rs` `read_on`: computes `choices`. A business decision reads its binding's saved
  revision bundle (`admission_binding::read_saved_binding`), then the step's choice routes.
- `workflow_tools.rs` `grant_agent_tools`:
  - it rejects more than 32 names, or duplicates, before the write;
  - `authorize_company` takes the company lock (FOR NO KEY UPDATE) and requires owner/admin;
  - the agent row is read `FOR UPDATE` scoped by `company_id`; a missing row is 404 and a
    current list different from `expected` is 409; then the list is replaced.
  - Lock order is company then agent, as in publication and provisioning. The general agent
    update locks only the agent row and leaves `workflow_tools` alone (`COALESCE($16, …)`).
- `provisioning.rs`: a `ProvisioningDeployment::catalogue()` accessor.

**Infra.** `WorkflowEngine` accessors: `reads()`, `decisions()`, `workflow_tools()`,
`template_catalogue()`, `template_copies()`, `wake_workers()`. Also a `#[cfg(test)] for_tests`
constructor, which `engine_on` uses. `AppState` → `Arc<WorkflowEngine>` via FromRef.

**HTTP** (`routes/workflows/`, merged into the protected router):
- `mod.rs` holds the router and shared helpers:
  - `WorkflowError` (App | Diagnostic → 422 JSON with code, message, field and schema paths,
    and line/column), with From impls for LifecycleError and TemplateError;
  - `authorize` (Inspect), `AssociationBody`, `keyed_id`;
  - `revision` (a positive revision, else 400).
- `definitions.rs`:
  - list, read (with versions), save (`expected_revision` absent creates the draft), validate,
    publish, archive (204);
  - publish takes a key; the version id is `keyed_id("workflow.publish:{w}", company, key)`, so a
    retry replays it;
  - templates list, and copy by key: the workflow id is keyed; a retry under the key for the same
    template returns 200 with the draft; anything else under the key is 409.
- `bindings.rs`:
  - list and read are filtered by association visibility through `Visibility` (each association
    authorized once per request);
  - configure, activate and deactivate (expected state revision);
  - start through `manual::start`: 201 new, or 200 replayed; it wakes the workers;
  - runs list (WF-C002: `parent_run_id` labels children), run detail, and cancel/retry mapped from
    the CancelResult/RetryResult variants (200/409/404).
- `decisions.rs`:
  - JSON read; submit (200, with `replayed`); comment (201 new, 200 replayed);
  - the page `GET /ui/workflow-decisions/{d}` resolves the company from the decision row, and
    `read_decision` authorizes;
  - the page's htmx forms `POST /ui/workflow-decisions/{d}/submit` and `/comments` re-render the
    section. A 400/409/404 refusal shows as a banner over the current view; any other error
    propagates to the shared UI error feedback;
  - a submission that commits a successor wakes the workers.
- `tools.rs`:
  - list, `PUT /{name}` register (201 new, 200 with `expected_revision`), `DELETE /{name}` revoke
    (204);
  - `PUT /api/companies/{c}/agents/{a}/workflow-tools` with `{expected, tools}` → 200
    `{agent_id, tools}`.
- `pages/workflow_decision.rs`: proposal, deadline, one submit button per declared choice, data
  and feedback fields, discussion and a comment form.
  - Feedback: the submit buttons are disabled while a request runs (`hx-disabled-elt`), with a
    spinner and a "Posting…" label;
  - escaped text; daisyUI 5 classes; no banned v4 classes.

**Deviations.**
- DV-7: the page and its forms carry no company parameter. The company comes only from the
  decision row, then `read_decision` authorizes. Notices link with the decision id alone.
- DV-8: agent grants compare-and-set against the list the caller saw (`expected`), because
  `agents` has no revision column and a schema change was not warranted. The 32-name and
  uniqueness checks mirror `valid_workflow_type_names`.
- DV-9: "revoke stops tool calls" is the existing `revoke_workflow_tool` command's behaviour,
  covered by p07's `workflow_tool_revocation_tests` (run green below). The route test covers the
  route → command mapping (409 stale, 204, then 404) and the emptied listing.
- DV-10: route tests call the handlers directly (the convention of the other route tests), plus
  `workflow_routes_mount_in_the_app_router`, which builds the whole app router to catch path
  conflicts.
- Tooling: the `graft` CLI/MCP server is not installed here (ENOENT); discovery used grep and
  reads.

**Tests** (`routes/workflows/tests.rs` fixture plus four child modules):
- `tenancy_tests`:
  - every route (20) answers a stranger and a plain member 404. The fixture's member and admin
    both have principals, so the member is refused for the role, not for a missing principal;
  - an owner naming another tenant's ids under their own company gets 404/409, and the foreign
    records stay unchanged;
  - an admin not allowed on an allowlist channel gets neither that channel's binding nor its run
    in listings, and reading or starting them is 404; the owner sees both.
- `lifecycle_tests`:
  - draft create/conflict/stale save;
  - a 422 diagnostic, a stale validate, and valid;
  - publish replay by key, and a stale publish;
  - binding create conflict, stale and ok activate;
  - run start replay, the same key with different input → 409, list (child label), detail;
  - stale and ok cancel; stale and ok archive;
  - template copy replay; another template under the same key, or a stale revision → 409; an
    unknown template → 404; member and stranger → 404; `template_origin`.
- `tool_tests`:
  - register (201), a duplicate → 409, update 200 (rev 2), stale → 409;
  - a reserved name or a bad name → 400, a foreign version → 404;
  - grants: apply, stale → 409, duplicates or 33 names → 400; a foreign agent, a member or a
    stranger → 404; the stored list;
  - revoke: stale, ok, then gone;
  - `competing_agent_grants_apply_once`: two concurrent grants from the same `expected`, one 200
    and one 409, one name stored.
- `decision_tests`: through the production engine's workers:
  - the decision parks; the page shows the declared choices and the disabled-while-busy forms;
  - the JSON read lists `choices`; a comment applies then replays; the UI comment re-renders
    escaped text; a UI submit with invalid JSON shows a banner;
  - a stale revision → 409;
  - a stranger (own and foreign company paths) and a non-reviewer member → 404, and a stranger
    on the page → 404;
  - submit applies, then replays; the same key with another choice → 409;
  - the run succeeds; the page shows the decision; an unknown decision → 404.

**Checks** (cluster 55480, `workflow08_test`, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`):

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib routes::workflows` (first run, 6 tests) | 6 passed | `logs/b3b1-routes-1.log` |
| focused: `workflow::provisioning`, `pages::tests`, `routes::assistant_channels`, `decision_tests`, `child_decision_round_tests`, `provisioning_dispatch_tests`, `provisioning_runtime_tests`, `workflow_tool` | 6, 158 (+1 ignored), 1, 39, 1, 11, 3, 19 passed | `logs/b3b1-focused-1.log` |
| `scripts/stack-budget.sh -- routes::workflows decision_tests` | 45 passed at 2048 KiB | `logs/b3b1-stack-budget.log` |
| `cargo test --lib routes::workflows` after the visibility memo, the error propagation, principals in the fixture, the admin-visibility test and the module split | 7 passed | `logs/b3b1-routes-2.log`, `logs/b3b1-routes-3.log` |
| repeats ×5 (7 tests), then ×3 with `competing_agent_grants_apply_once` (8 tests) | all passed | `logs/b3b1-routes-repeat.log` |
| `cargo clippy --lib --tests`, `cargo fmt --check` | clean | — |

The full suite was not run (it is the final gate). There are no `query!` macro changes, so
`.sqlx` is unchanged.

## B3b-1 corrections for WF-P08-R013 (2026-10-23)

B's R013 (CHANGES REQUIRED) raised F054 (Medium), F052, F053 and F055–F058 (Low), and F059–F061
(Info). All are fixed in one round, the Info ones included. Artifact: digest
`b075aa52c21ccd966c2372c3a6685b4ee7eeb91032942eda364683fb3ff711ef` (same base and exclusions;
`logs/b3b1-snapshot-r2.txt`). Files changed in this round: `logs/r013-fix-files.txt`.

- **F054** (validate without selections): `ValidateBody` carries the same `http`/`mcp`
  selections as publish, and a shared `selections()` parses both. Test
  `validate_and_publish_carry_http_selections` uses a registered HTTP operation and a `tool.call`
  draft: without selections → 422, with them → 200, publish → 200.
- **F055** (publish retry with other selections): the version id is
  `keyed_id("workflow.publish:{w}", company, key ‖ canonical selections)`. The key with other
  selections therefore names another version, and the command refuses it (409, tested in the same
  test).
- **F052** (silent truncation):
  - listings are keyset pages, `?after=<last id>&limit=1..=200`, answered as `{items, next}`;
  - `next` follows the scanned rows, so a page an admin sees partly or not at all still says
    whether more exist;
  - a cursor that names no row is 400, and so is an out-of-range limit;
  - the read port has `Page`/`PageRequest`;
  - definition versions, run executions and run waits are the newest 200, with
    `versions_truncated`, `executions_truncated` and `waits_truncated`;
  - tests: `run_listing_pages_newest_first`, the admin page-of-one check in
    `workflow_listings_hide_channels_an_admin_cannot_see`, and the truncation flags asserted
    false.
- **F053** (per-thread checks, unindexed reads):
  - `Visibility` reduces a stored thread association to its channel. The
    `(company_id, channel_id, thread_id)` foreign keys on `workflow_runs` and `workflow_bindings`
    already hold the thread in that channel, so a page checks each distinct channel once.
  - Past 32 checks a page logs `workflow listing visibility checks` with the count: the working
    set evidence for a later batch read, per the persistence deferral rule.
  - The init schema gains `workflow_decision_id` (UNIQUE on `workflow_decisions(id)`; this also
    enforces what DV-7 relies on), `workflow_runs_newest` (company, created_at DESC, id DESC) and
    `workflow_runs_binding_newest`.
  - `list_runs` builds its WHERE from the filters present (QueryBuilder), so generic plans keep
    the indexes.
  - Before/after `EXPLAIN ANALYZE` on 200k synthetic rows is in `logs/r013-fix-explain.log`:
    - run pages: about 2,660 buffers through a bitmap scan and top-N sort become 25–104 buffers
      through an index-only scan, including after a cursor;
    - the decision lookup: a sequential scan becomes an index scan.
  - `workflow08_test` was recreated after the edit.
- **F056** (child control):
  - `controllable_run` authorizes the company and the run's association, then refuses a run with
    a parent (409, naming the parent run) before cancel/retry;
  - a stranger still gets 404, so the refusal discloses nothing;
  - the internal child controls go through the service and are unaffected;
  - `RetryResult::Unsafe` now answers "This run cannot be retried: only a failed run whose
    failed step is safe to repeat can be (revision N)".
  - Test `child_runs_are_controlled_through_their_parent` (`workflow.call` parent through the
    engine) covers the child label in the list, the cancel and retry refusals naming the parent,
    retry of a succeeded root, and a stranger getting 404.
- **F057** (provisioning grants): `grant_agent_tools` refuses (400) a grant that *adds*
  `list_company_agents` or `create_agent_channel`. An agent already holding them, the Agent
  Builder, keeps or drops them alongside its other grants. This is D2 invariant 6, with no
  amendment. Tested in `workflow_tool_routes_register_revoke_and_grant`.
- **F058** (decision tenancy):
  - JSON comments by a stranger (own and foreign company paths) and by a non-reviewer member →
    404;
  - the UI submit and UI comment by a stranger and by a member → 404, with the stored comment
    count unchanged;
  - the tenancy probe now covers 22 company routes, adding template copy and agent grants. The
    module docs say the decision routes are covered in `decision_tests`.
- **F059**: the workflow router sets an explicit `DefaultBodyLimit` of 2 MiB (a 256 KiB source as
  a JSON string, escaped).
- **F060**:
  - the page uses daisyUI 5 `fieldset`/`fieldset-legend`;
  - the busy state shows a spinner with "Submitting…" in a `role=status` element;
  - the UI submit success path is tested: it shows "Decision submitted" and "Decided: accept",
    and the JSON API with the same command replays it.
- **F061**: a grant whose current list already equals the requested one answers 200, so a retry
  of an applied grant succeeds. Names stay unchecked at grant time (they resolve at publication,
  DV-8), and the validation order is unchanged.
- **Also**:
  - route tests now use `own_database`. The engine workers the decision and child tests spawn
    claim unscoped, and on the shared test database they deadlocked another test's company
    delete (`logs/r013-fix-routes-2.log`). The company cleanup is gone, since the database drops
    with the test;
  - each fixture publish has its own key, because publish keys are the company's.

**Checks** (cluster 55480, `workflow08_test` recreated, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`):

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib routes::workflows` | 11 passed (two earlier runs failed: the shared fixture publish key, then the cross-test deadlock, both fixed above) | `logs/r013-fix-routes-1.log`, `-2.log`, `-3.log` |
| repeats ×3 | 11 passed each | `logs/r013-fix-repeat.log` |
| `cargo clippy --lib --tests -- -D warnings`, `cargo fmt --check` | clean | `logs/r013-fix-clippy.log` |
| `scripts/stack-budget.sh -- routes::workflows` | 11 passed at 2048 KiB | `logs/r013-fix-stack-budget.log` |
| focused: `adapters::persistence::tests` (duplicate-index check), `decision_tests`, `workflow_tool`, `provisioning_dispatch_tests`, `provisioning_runtime_tests`, `routes::assistant_channels`, `pages::tests`, `child_runtime_tests`, `schedule_tests` | 5, 40, 19, 11, 3, 1, 158 (+1 ignored), 12, 19 passed | `logs/r013-fix-focused.log` |

## B3b-1 corrections for WF-P08-R014 (2026-10-23)

R014: F052–F059 and F061 are fixed; F060 is partly fixed, and its broken part is **F062** (Low). F063 and F064 are Info.
- **F062:** the "Submitting…" status used `[.htmx-request_&]:inline-flex`, which the committed
  `assets/app.css` does not contain. It now uses `[.htmx-request_&]:inline`, which is compiled and
  already used by `channel_settings`, `agents`, `profile` and `company_skills`. Every class on the
  decision page is checked present in `assets/app.css` (a script over the page's `class`
  attributes: 0 missing), so `npm run check:css` has no new class to emit.
  - `npm run check:css` itself was not run: there are no `node_modules`, and installing them
    needs a download. The busy state was not checked visually in the running app either.
  - The B3a assistant-channel card's `form-control` labels follow the existing markup there. That
    class is a no-op in daisyUI 5 and emits nothing, so it does not affect the CSS check.
- **F063 (Info):** the `controllable_run` doc comment is corrected. The engine's child cascade is
  `cancel_tree_on`, a persistence transition, and the rule lives in the route because the routes
  are the only production caller of the service's cancel/retry. Disposition: moving the refusal
  into the service is recorded for whichever phase adds another ingress for those commands. p09's
  UI uses these routes.
- **F064 (Info), accepted as recorded behaviour:**
  - `next` is the last scanned row's id, so it may name a row the caller cannot see. Only the id
    is exposed, to an owner or admin of the company.
  - The definitions cursor follows the current title, so a rename between pages can skip or repeat
    a row.
  - The EXPLAIN probe is narrow; the real query is an index scan with heap fetches on the same
    indexes.

Artifact: digest `57566f926c9396e4d9b3eae72e5d6b46d74e5c8b3455d6fca912aa50d18ddd33` (see `logs/b3b1-snapshot-r3.txt`). Checks: `cargo test --lib --
routes::workflows pages::tests` passed 169 (+1 ignored); clippy `-D warnings` and fmt are clean
(`logs/r014-fix-checks.log`).

## B3b-1 acceptance (2026-10-23)

**B3b-1 is accepted in phase** (not integrated, not committed):
- B gave an explicit PASS in WF-P08-R015 at digest
  `57566f926c9396e4d9b3eae72e5d6b46d74e5c8b3455d6fca912aa50d18ddd33`, and root recomputed it to
  match (same base and four exclusions).
- Rounds: R013 (CHANGES REQUIRED, F052–F061), R014 (CHANGES REQUIRED, F062; F063–F064 Info) and
  R015 (PASS). No acceptance-blocking finding is open.
- Coordination r9 records WF-C003's p08 verification.
- The final gate must also run `npm run check:css` (CI does). There are no `node_modules` here,
  and installing them needs a download, so the user decides.

## Architecture gate P08-D3 (2026-10-23)

Rev 1 is at `logs/decision-D3-r1.md` (sha256 `121779fc3c94826a41c2b1a2d2413fa4b70d15643b468ee513cc7e60508625d4`). It covers the production action-reconciliation composition
and route for B3b-2:
- `CompanyReconciliationResources`, under the command actor; provisioning fails closed;
- no production verifier, so `UnknownNote` only and nothing reopens;
- `WorkflowEngine::reconciliation()`;
- a run-detail `actions` discovery read;
- `POST /api/companies/{c}/workflow-runs/{r}/reconciliations`;
- invariants, failure modes, affected contracts, and the verification plan.

Its working draft was folded into it and removed. Sent to B as WF-P08-R016.

R016 (CHANGES REQUIRED): F065 and F066 Medium; F067, F068 and F069 Low; F070 Info. **Rev 2** is at
`logs/decision-D3-r2.md` (sha256 `0cacf996a4a2d6a995938a8c9c7cc6deab6e1459ccd9d6f9716f26a7748f558c`;
diff `logs/decision-D3-r1-to-r2.diff`). Changes:
- F065: dispatch by `resource_kind` in either target form, so slotless HTTP is `Local { http }`;
- F066: an outcome→status table, the fresh-key rule and the ordering;
- F067: discovery adds `has_receipt`, `reconcilable` and the destination status/parts, drops
  `local` markers, and computes `evidence_conflict` once per execution;
- F068: readiness stays required, documented, with cancel as the path; the trait doc is corrected
  in B3b-2;
- F069: every test goes through the production composition, with the five target forms, exact
  outcomes and rows, the same-key and different-key races, readiness and MCP;
- F070: timeout → 503 with `Retry-After` on the workflow routes, and a pending p09 notice
  (annotation, not resume).

Sent to B as WF-P08-R017.
**R017: P08-D3 rev 2 PASS** (2026-10-23; sha256 `0cacf996…558c`). F065–F069 resolved, F070
adopted. Carried into the B3b-2 batch review: **F071 (Low)**, which B3b-2 must satisfy:
- route docs and the p09 notice say that a replay needs current access and readiness (404
  otherwise, nothing recorded);
- a 503 timeout may follow a committed settlement, so the client retries with the **same key and
  request** after a 503, and with a new key only after a 409;
- the implementation record corrects D3.7's rollback wording;
- a test shows a replay answering 404 after the connection is disabled.

F072 (Info) is adopted as wording:
- `evidence_conflict` follows a recorded note and is labelled as such;
- the competing test advances the revision once per evidence row;
- a destination's `reconcilable` flag is a hint, and the command stays authoritative.

**The B3b-2 implementation contract is `logs/decision-D3-r2.md`.**

## B3b-2 implementation log (2026-10-23)

Contract: `logs/decision-D3-r2.md` (PASS R017), plus F071 and F072 carried from R017. Artifact:
digest `4156165b358161367b1822179edbde0df23aa64d1ac6148e04797dbed68e20b3` (base `da86871`, same
exclusions; `logs/b3b2-snapshot.txt`). Files since the accepted B3b-1: `logs/b3b2-files.txt`.

- **§D3.1** `action_reconciliation/resources.rs`: `CompanyReconciliationResources`.
  - A pure `resource_route(target)` decides by `resource_kind` in either form: messaging `Local`,
    http `Connection`/`Local`, mcp `Connection`. Provisioning and every other kind or form is
    NotFound.
  - Messaging delegates to `MessageActionAuthority`. Connections run `authorize_company`, then
    `resources::inspect_on`.
  - `message::MESSAGING` becomes `pub(super)`.
- **§D3.2a** The trait doc is corrected: it says reconciliation does not need the current
  approval policy, but the service still requires the resource authorized, Ready and supporting
  the contract.
- **§D3.3** The engine composes the production reconciliation:
  - `WorkflowEngine::reconciliation()` with `ProductionReconciliation` and verifier `None`;
  - the route calls it with a fresh `CancellationToken` and a 5 s budget, boxed;
  - `WorkflowError` answers `AppError::Timeout` with 503 and `Retry-After: 1`.
- **§D3.4** Reads: `ActionSummary` and `DestinationSummary` in `application/workflow/reads.rs`.
  - `run_detail` lists `actions` with `actions_truncated`: remote and message dispatches only,
    newest first, at most 200;
  - each carries `has_receipt` (per invocation: receipts are keyed by `(company_id,
    invocation_id)`);
  - `evidence_conflict` is computed once per distinct execution through `unnest`;
  - message destinations are delivery-backed only (in-app destinations have no delivery and
    cannot be targeted), with status and part count;
  - the hints are `ActionSummary::reconcilable(run)` and `DestinationSummary::reconcilable()`;
  - the route's run JSON shows them.
- **§D3.5** Route `POST /api/companies/{c}/workflow-runs/{r}/reconciliations` in
  `routes/workflows/reconciliations.rs`:
  - the body maps `delivery_id`, and `evidence` is the command's own `EvidenceInput`;
  - statuses follow the rev 2 table: 200 for a recorded outcome; 409 for `revision_conflict`,
    `idempotency_conflict`, `blocked` and `evidence_conflict` (F072: documented as recorded but
    conflicting);
  - the module doc states the fresh-key rule after a 409; the same request under the same key
    after a 503 (F071: a timeout may follow a committed settlement); and that every call, a
    replay included, needs current access and a ready resource, or it is 404 with nothing
    recorded (F071).
- **D3.7 rollback wording, corrected (F071):** a 503 does not prove a rollback. The budget is a
  timeout around the whole operation, so it can fire after the settlement committed. That is why
  a client retries a 503 with the same key and request.
- **Tests:**
  - The fixture `persistence/workflow/reconciliation_fixture.rs` is a child of `http_tests`,
    re-exported crate-wide under `cfg(test)` as `persistence::workflow::reconciliation_fixture`.
    The server answers 500 after the marker, the job is retired, and the run waits on
    reconciliation. It comes in two forms, Slot (`Connection { http }`) and Tool
    (`Local { http }`), on its own database.
  - `routes/workflows/reconciliation_tests.rs`:
    - `reconciliation_annotates_both_http_forms`: discovery is reconcilable with no receipt; the
      note gives 200 `unknown_recorded`, one row, revision +1, the run still waiting; the same
      request replays with no new row;
    - `reconciliation_outcomes_follow_the_status_table`, each asserting rows:
      - stale → 409 `revision_conflict`;
      - the same key with the corrected revision → 409 `idempotency_conflict`;
      - a fresh key → 200;
      - the same key with another note → 409 `idempotency_conflict`;
      - a verified reference at the current revision → 400 with no row; at a stale revision →
        409 `revision_conflict`;
    - `reconciliation_needs_a_ready_resource_even_to_replay` (F068, F071): with the connection
      disabled, the earlier request's replay → 404 and a new note → 404, nothing recorded;
      re-enabled → 200;
    - `reconciliation_refuses_strangers_members_and_foreign_markers`: a stranger on both company
      paths and a member → 404; a foreign marker → refused; no rows;
    - `competing_reconciliations_record_once`:
      - different keys from one revision give exactly one `unknown_recorded`, and the other is
        `revision_conflict` or `blocked{stale_snapshot}` (409);
      - one row; the revision advances once per evidence row (F072);
      - the same key concurrently gives one row, both `unknown_recorded`, exactly one replayed;
    - `workflow_timeouts_answer_service_unavailable`.
  - Production composition at the service level (F069, F065):
    - `workflow_mcp_unknown_note_through_the_production_composition` (`Connection { mcp }`);
    - `workflow_message_unknown_note_through_the_production_composition` (`Local { messaging }`):
      it also asserts discovery shows the unknown delivery as `outcome_unknown` with one part,
      the only reconcilable destination, and that a verified reference is refused.
  - `reconciliation_resources_route_by_kind_in_either_form`: all five forms, plus provisioning
    in both forms, `Local mcp`, `Connection messaging` and unknown kinds failing closed.
  - `provisioning_lists_agents_and_models` now asserts that the run's only marker is `local` and
    that `run_detail.actions` is empty.
- **Deviations from the D3 rev 2 plan:**
  - DV-11: the provisioning and unknown-kind refusals are proven on the pure dispatch function,
    not a DB `lock_resource` call. A `FrozenAction` cannot be built outside the actions module,
    and the refusal happens before any DB access. This is rev 2's stated fallback.
  - DV-12: MCP and messaging go through the production composition at the service level, not
    the route. Their fixtures live in the persistence test tree. The route over the same service
    is covered by the HTTP forms.
  - DV-13: the admin kept off an allowlist channel is not repeated at the route. The fixture's
    run is company-associated, and the association check is the existing service's, covered by
    `action_reconciliation_authority_tests`.

**Checks** (cluster 55480, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`):

| Command | Result | Log |
| --- | --- | --- |
| new tests (`reconciliation_tests`, `reconciliation_resources`, `workflow_timeouts`, `production_composition`) | 11 passed | `logs/b3b2-tests-1.log`, `-2.log` |
| focused: `action_reconciliation`, `mcp_tests`, `message_tests::reconcile`, `http_tests`, `provisioning`, `routes::workflows`, `reads` | 242, 26, 20, 73, 23 (+1 ignored), 17, 29 passed | `logs/b3b2-focused.log` |
| repeats ×3 of `routes::workflows` + `production_composition` | 19 passed each | `logs/b3b2-repeat.log` |
| `scripts/stack-budget.sh -- routes::workflows production_composition reconciliation_resources` | 20 passed at 2048 KiB | `logs/b3b2-stack-budget.log` |
| clippy `-D warnings`, fmt | clean | `logs/b3b2-clippy.log` |

There are no `query!` macro changes and no schema change.

## B3b-2 acceptance and B4 inputs (2026-10-23)

**B3b-2 is accepted in phase** (not integrated, not committed):
- B gave an explicit PASS in WF-P08-R018 at digest
  `4156165b358161367b1822179edbde0df23aa64d1ac6148e04797dbed68e20b3`, and root recomputed it to
  match.
- F071 is fixed, F072 adopted, DV-11..DV-13 accepted.
- **F073 (Info, optional)** is deferred into B4, where it is implemented and reviewed with that
  batch:
  - the race-test 409 and recorded-refusal assertions;
  - pin the foreign marker to 404;
  - a `warn!` on the 503 branch;
  - a route-doc line about axum's 422 for malformed bodies;
  - the message test asserting the 400 and the absence of a row.
- Coordination **r10** publishes **WF-C005** for p09: paging, reconciliation as an annotation,
  the client rules, child control, the decision page.

**User decisions for B4** (asked 2026-10-23 during R018):
- **P08-Q6:** delete manual handoffs (`/attention/handoffs`, attention_source_events) with the
  thread-handoff system. A person handling a reply is a `decision.human` step.
- **P08-Q7:** delete sub-agents (`agent_sub_agents`, SubAgentScope) with delegation.
- **P08-Q8:** replace the legacy email preferences (review requests, delegation timeouts, task
  failures) with **one** "email me when a workflow decision needs my review" preference,
  default on, honoured by the decision-notice composer.

Open questions from the inventory, resolved by the plan without asking:
- `agents.config_json` (HarnessConfig), `agents.run_timeout_secs` and `AGENT_RUN_TIMEOUT_SECS`
  are "alternate runtime configuration" (plan "Removal work"), so they are deleted.
- `channel_gate`: an implementation choice settled in the B4 plan.

Inventory: `logs/b4-inventory.md`.

## B4 plan (2026-10-23)

B4 is too large for one thorough review: the inventory touches about 40k lines across ten legacy
areas. It is split at stable contracts. Each sub-batch keeps the crate compiling, runs its focused
checks, and gets one batch review; the later ones build only on an accepted earlier one.

| Sub-batch | Scope | Gate |
| --- | --- | --- |
| **B4a** Surfaces | See below | **P08-D4** first |
| **B4b** Runtime | See below | none (pure removal) |
| **B4c** Data | See below | covered by P08-D4 §schema |
| **B4d** Close-out | See below | none |

**B4a: retarget the user-facing surfaces onto the workflow, and cut the legacy entry points.**
- Retarget:
  - attention / work list → open decisions for the reviewer, waiting and failed runs, and
    bindings with recent refusals;
  - notifications → workflow decision and run-failure sources, plus the P08-Q8 decision-notice
    preference;
  - task counts → run and decision counts;
  - dashboard → runs, executions and attempts;
  - mailbox thread activity and the message→run lookup;
  - simulation → the runs admitted for the simulated message.
- Retarget creation:
  - agent creation is agent-only;
  - channel creation, onboarding, library install and the Agent Builder install go through
    AssistantChannels or a plain channel;
  - no personal-channel coupling.
- Delete these routes and pages:
  - A10 tasks: board, monitor, list, counts SSE;
  - A4 approvals and reviews (review policy UI included);
  - A5 thread handoffs, manual handoffs (P08-Q6), reply-handling and task-complete;
  - A6 the ask/start agent-task actions;
  - A3 the delegation controls;
  - harness diagnostics, the agent runtime settings, sub-agents (P08-Q7).
- Strip the mailbox banner, marks and task links, and fix the cross-links.
- Update `scripts/tests/task-counts.mjs` and its CI line.

**B4b: stop and delete the legacy runtime.**
- Stop: TaskWorker, MemoryWorker, TaskWakeups and `task_ready`; the ThreadUseCases legacy
  builders.
- Delete:
  - dispatch, reload and human_completion; AgentRunner, trace hooks, prompt_fence;
    `task_worker.rs`; InboundTaskPayload;
  - the Rig legacy binding and its modules, after extracting the shared types (inventory step 0);
  - HarnessRegistry, ports AgentHarness/AgentRun, HarnessConfig and HarnessKind (the RunIdentity
    harness becomes a literal);
  - `persistence/task/harness_runs.rs`, AgentCapabilityReader;
  - the side areas' application and persistence code: approvals, reviews, handoffs,
    outreach/quorum/delegation, internal-note tasks;
  - legacy task persistence and entities, member-removal work-at-stake (including its
    compile-time sqlx macros);
  - the RunCheckpoint `wait`/`outreach_wait` fields.

**B4c: delete the data model.**
- Memory (P08-Q1): code, config, routes, pages, metrics.
- Channel agent lists, owner, response_trigger and the review/reply overrides, with the
  same-step provisioning edit (inventory hazard 8).
- The legacy runtime-config fields (`agents.config_json`, `agents.run_timeout_secs`,
  `AGENT_RUN_TIMEOUT_SECS`).
- **One init-migration edit:** drop the legacy tables, columns, triggers, functions and enum
  values; trim the shared `background_tasks`, `task_harness_runs` and `runtime_metric_samples`;
  add the P08-Q8 preference column.
- Recreate `workflow08_test`, regenerate `.sqlx/`, update the workflow `job_*`/`harness_*` tests.

**B4d: close out.**
- Docs, config and bootstrap: `.env.example`, `fly.toml`, README, `docs/*`, the persistence
  AGENTS.md memory section, the stack-budget comments, the `main.rs` notes.
- The ToolId comment wording; F073 (deferred from B3b-2).
- Exhaustive caller searches (acceptance: "no remaining legacy execution entry points").
- The phase gate.

**Gate P08-D4 (before B4a), the decisions costly to unwind:**
1. The retargeted read models and their live-update protocol: which workflow tables notify,
   subscribe-before-query, and reconcile on reconnect (`src/adapters/http/AGENTS.md`).
2. The decision-notice preference contract (P08-Q8).
3. Agent and Agent Builder installation without personal channels.
4. The schema end state of the shared tables: what B4c keeps.
5. The channel_gate disposition.

**Per sub-batch checks:**
- B4a: the retargeted and deleted route/page tests, `pages::tests`, `routes::*`, the
  task-counts JS test.
- B4b: compile plus the ingest and workflow suites (inbound, message_ingress, provisioning,
  schedule).
- B4c: schema/catalog tests, `adapters::persistence::tests`, the workflow `job_*`/`harness_*`,
  `.sqlx` offline compile.
- B4d: the phase gate, which is the full verification suite, run only with the user's go-ahead
  at the end.

## Architecture gate P08-D4 (2026-10-23)

Rev 1 is at `logs/decision-D4-r1.md` (sha256 `2f46c343954fd586bd83bc6e3298069690feef51d93086d5cfa33ece6387647d`). Inputs: `logs/b4-inventory.md` and
`logs/b4-projections.md`. It covers:
- D4.1: a `workflow_runs_notify_changed` trigger on state and waiting_reason transitions,
  feeding `workflow_run_changed` and `thread_activity`;
- D4.2: attention with decision, reconciliation, failure, refusal and delivery branches;
- D4.3: notifications for decision (in-app) and delivery (recipient fixed); the P08-Q8
  preference, with `notice_delivery_id` nullable;
- D4.4: run counts;
- D4.5: the dashboard on workflow jobs;
- D4.6: activity, message lookup and simulation from runs;
- D4.7: agent-only creation, onboarding through `AssistantChannelService`, the Builder as a
  library agent;
- D4.8: the shared-table end state; channel_gate kept;
- D4.9: invariants; D4.10: failure modes; D4.11: verification.

Sent to B as WF-P08-R019.

R019 (CHANGES REQUIRED): F074, F075 and F080 Medium; F076–F079, F081 and F082 Low; F083 Info.
**Rev 2** is at `logs/decision-D4-r2.md` (sha256 `d88aeb869d546d6308e106dddcb66e7274838351c4b2b001629da706027d3bce`; diff `logs/decision-D4-r1-to-r2.diff`):
- F074: no decision notifications; delivery is the only source;
- F075: delivery recipients defined; the dependent functions listed for rewrite or deletion; a
  catalog check; a post-drop test;
- F076: a `notice_opted_out` column, CHECK and guard;
- F077: a `Resync` on reconnect, through try_recv;
- F078: one row per decision; refusal freshness at 30 s; the rerun and schedule indexes;
- F079: `run_ids`, and the simulation shows every binding outcome;
- F080: onboarding pins the default model, with key `onboarding:{agent_id}`, and skips channels
  without a model;
- F081: the active-agent triggers drop in B4a;
- F082: race and live tests per consumer, trigger and parse tests;
- F083: split triggers, the existing `notify_thread_activity()` reused, the UI copy, and a
  channel_gate competing test.

Sent to B as WF-P08-R020.
**R020: P08-D4 rev 2 PASS** (2026-10-23; sha256 `d88aeb86…3bce`). F074–F082 resolved, F083
adopted. **The B4 contract is `logs/decision-D4-r2.md`.**

Carried into the B4a batch review:
- **F084 (Low):** publish `Resync` also after the outer listener is rebuilt (`listen_all`
  finished), and test a reconnect that fails once and then recovers.

F085 (Info), adopted as implementation notes:
1. The catalog check matches qualified names.
2. `eligible_reviewer` locks the preference row `FOR SHARE`.
3. The onboarding replay test replays the channel step for existing agents.
4. `recipient_for_source` keeps its inner join to channels.

**B4a placement clarified (no decision change):** the P08-Q8 preference is added in B4a, because
it is a B4a surface and D4.11 tests it there. That means the `decision_email_enabled` column,
`notice_opted_out` with its CHECK and guard, and DecisionNotices honouring it. B4c only drops the
legacy preference columns.

### B4a work order
1. Schema additions: the D4.1 triggers; the D4.2, D4.4 and D4.6 indexes; the preference columns,
   CHECK and guard; drop the active-agent triggers. Recreate `workflow08_test`.
2. `infra/events.rs`: `WorkflowRunChanged`, `Resync` through `try_recv`, and after an outer
   rebuild (F084).
3. Creation: agent-only creates, plain channels, onboarding, library install.
4. Projections: attention, notifications (delivery only, recipient rule), run counts, dashboard,
   activity, message lookup, simulation.
5. Delete the legacy routes and pages (A10, A4, A5, A6, A3, harness diagnostics, runtime
   settings, sub-agents); strip the mailbox; fix the cross-links.
6. The preference: DecisionNotices and the profile toggles.
7. Tests per D4.11, then the batch review.

### B4a progress (2026-10-23)
- **Step 1 done:**
  - D4.1 run NOTIFY triggers;
  - the D4.2/D4.4/D4.6 indexes;
  - `decision_email_enabled`, `notice_opted_out` with its CHECK and guard arm;
  - the active-agent triggers and function dropped.
  - Loaded with psql into a scratch database; `workflow08_test` recreated empty.
- **Step 2 code done:**
  - `WorkflowRunChanged(RunScope)`, parsed fail-closed;
  - `Resync` after `listen_all` and on `try_recv` `Ok(None)` (F077, F084);
  - `wake_ups` maps `Resync` to `Lagged`;
  - the notifications SSE and `attention::wakes_attention` reconcile on it.
  - Its D4.11 tests come in step 7.
- **Step 3 done (creation, D4.7):**
  - **Agents:**
    - `AgentUseCases::create_agent` is agent-only.
    - `create_agent_from_library(.., LibraryCopyModel)` with `FollowDefault` (Agents page, API) or `PinDefault` (onboarding). `pinned_default_model` is the one rule.
    - Removed: `create_addressable_agent(_with)`, `PersonalChannelPlan` and friends, the `SpamScanning` constructor argument, and `create_channel_with_agent` / `create_with_agent`.
    - Persistence `create_agent_from_library`, with `prepare_library_agent_on` shared with the legacy owned-channel path.
  - **Agents page:** the channel step (routes `/ui/agents/new/{channel,agent,create}`, its forms, views and tests) is deleted; Advanced posts `/ui/agents` and creates.
  - **Channels:**
    - `/ui/channels` keeps one plain-channel create form; the Easy (library agents → channels) and Simple (instructions → agent) tabs and `/ui/channels/easy` are deleted.
    - The legacy `/companies/{c}/channels` page loses its Simple form.
    - The channel JSON `system_prompt` and the form's `system_prompt`/`form_mode` fields are gone.
  - **Onboarding:**
    - Step 2 creates agent-only (custom agents get `ModelOverrides` from `pinned_default_model`; library agents get `PinDefault`).
    - Then, for each agent with a model, `onboarding_channel` calls `AssistantChannels` with the slug = handle, name, Autonomous and key `onboarding:{agent_id}`.
    - The POST renders the completion page with each agent's `OnboardingChannel` (Created / Refused / NoModel / Missing).
    - `GET /ui/onboarding/companies/{c}/complete` re-reads each agent's stored command result through the new `AssistantChannels::assistant_channel_result`. That read is lock-free and returns `None` for non-managers; refusals are not stored, so those agents show as Missing.
- **Transitional (removed in B4c, recorded per the B4 plan):**
  - the dormant owned-channel tab (`/ui/agents/{id}/channel`, `update_owned_channel`);
  - `OwnedAgentChannelPersistence::create_owned_agent_channel` (now tests only) and `update_agent_and_owned_address`;
  - the channel form's agent assignment list (A8);
  - provisioning's position-0 `channel_agents` row.
- **Focused checks for step 3** (`logs/b4a-creation-focused.log`), all against `workflow08_test` on 127.0.0.1:55480:
  - `cargo test --lib -- onboarding use_cases::agent:: agent_api_tests assistant_channels skill_tests routes::channel:: agent_form_tests ui_channels ui_agents pages::tests`: 182 passed, 0 failed, 1 ignored.
  - `routes::agent:: persistence::skill:: persistence::channel:: response_trigger ui_thread_handoffs ui_stream`: 43 passed, 1 failed. The failure was `owned_agent_channel_lifecycle…`, which asserted the dropped active-agent trigger. That section is replaced by `an_enabled_channel_needs_no_agent` (D4.11).
  - Rerun of `persistence::channel::`: 5 passed.
- **New tests:**
  - `creating_an_agent_creates_only_the_agent` (the mock's owned-channel write is `unreachable!`);
  - the `PinDefault` cases in `company_owner_agent_crud_flow_works`;
  - the Builder install creating no channel;
  - `the_channel_step_gives_each_modelled_agent_one_assistant_channel` (team channel, replay adds none, NoModel, taken-slug refusal, stored read-back, outsider reads nothing);
  - `an_enabled_channel_needs_no_agent`;
  - the plain create pane and the onboarding completion page.
- **Step 4 so far (2026-10-24):**
  - **Attention (D4.2):**
    - `ATTENTION_SQL` branches are now: decision (My work, from `workflow_decisions_open` probed on the reviewer key — see the index evidence below; Team work, one row each with "N reviewers"; never unassigned), run_reconciliation, run_failure (7 days, not rerun; the run actor's while a member), binding_refusal (Team work, one row per binding, 7 days, "updated within 30 s"), and delivery_failure (no `background_tasks` join; the run actor through `workflow_message_destinations`).
    - `$14` is now `manager`: company-associated items are visible only to owners and admins.
    - `AttentionItem` has `channel_id: Option`, `run_id`, and an optional `href` (no link for runs or bindings until p09).
    - The handoff commands, port methods and routes are deleted, and so is `all_owned`.
    - `OperationalSummary` holds the six D4.2 figures.
    - `attention_page(query, working_set)` is the truncation seam.
    - **New index `workflow_decisions_open (company_id, run_id) WHERE submitted_at IS NULL`** for team work and the summary. D4.2 did not list it; recorded here for review.
  - **Notifications (D4.3):** `source_snapshot` handles only `delivery/delivery_failure`, through destination → run → actor principal. The legacy sources still queued until B4c resolve to no source (withdrawn); B4c deletes their enum values with their producers. `NotificationRecipient.task_id` is now `run_id`, and legacy actions link to `/ui/work`.
  - **Run counts (D4.4):**
    - `RunCountSnapshot {company, channels}` from `RUN_COUNTS_QUERY` (open runs, visible channels plus company-wide).
    - `/ui/run-counts/events` wakes on `WorkflowRunChanged` for the company and on `Resync`.
    - `pages::run_counts`, `RUN_COUNTS_SCRIPT`, "Open runs"; per-agent slots removed.
    - `scripts/tests/run-counts.mjs`, `package.json` `test:run-counts`, the CI step.
    - `task-counts-query-plan.py` and `docs/performance/task-counts.*` deleted; the legacy `task/counts.rs` and its support deleted.
  - **Dashboard (D4.5):**
    - Every job figure reads `queue_kind = 'workflow'`; tokens come from `model_usage_calls` (`TOKENS`).
    - Outstanding jobs may have no channel ("Company-wide"); there are no `/ui/tasks` links; a threadless job is plain.
    - **Placement:** the runtime panel's gauges (`ai_*`, hydradb, active task executions → active workflow jobs and `WORKFLOW_WORKER_SLOTS`) move with B4c's `runtime_metric_samples` edit, since their columns change there.
  - **Focused checks**, all passing against 127.0.0.1:55480:
    - attention 27 + 4 (`logs/b4a-attention-focused.log`);
    - notifications 17 (`logs/b4a-notification-focused.log`);
    - run counts 14 (`logs/b4a-run-counts-focused.log`);
    - dashboard 50 (`logs/b4a-dashboard-focused.log`).
  - **Unavailable:** `npm run test:run-counts` (no Playwright in `node_modules`; nothing installed unasked).
  - **Legacy tests removed** because their sources or branches are gone:
    - attention tests of tasks, handoffs, approvals, reviews and delegation (replaced);
    - the thread-handoff and task attention assertions;
    - `workflow_job_external_attention_*` and `*_notification_sources_*`;
    - the three task-source notification tests;
    - the handoff queue-wording page test;
    - the legacy-seeded dashboard tests, replaced by the workflow-job dashboard test and delivery-seeded scoping/cache tests.
- **Step 4e and step 5 done (2026-10-24): mailbox activity, lookup, simulation; legacy surfaces cut.**
  - **Thread activity (D4.6):**
    - `entities::thread_activity::{ThreadActivity, ThreadWorkSummary {run_id, activity}}` with the mapping table (`from_run`) and its unit tests;
    - the port `application::thread_activity::ThreadActivityReader` and `THREAD_ACTIVITY_SQL` (DISTINCT ON thread over `workflow_runs_by_thread`, unfinished first); `AppState.thread_activity`.
    - `routes/ui.rs` reads it for the page, the column fragments, the column stream's badges and the open thread's strip. The legacy `ThreadActivity`, `ThreadWorkSummary`, `list_thread_work_summary` and `ThreadUseCases::thread_work_summary` are deleted.
    - Live: `thread_wake_ups` (the open thread) and the new `channel_wake_ups` (the column) wake on the `thread_activity` NOTIFY a run on the thread raises; `Resync` arrives as `Lagged`. `mailbox_thread_wake_ups`, `mailbox_channel_wake_ups` and `task_chain_wake_ups` are deleted.
  - **Message lookup (D4.6, F079):** `ThreadMessageView.task_id` → `run_ids: Vec<Uuid>`: the run that wrote the message (`workflow_message_destinations_company_id_message_id_key`), then every run its admission started (intent ⋈ intent bindings, `admitted`) in binding id order, de-duplicated. `THREAD_RUN_LOOKUP_SQL` replaces `THREAD_TASK_LOOKUP_SQL`; all three thread reads use it. The mailbox "· tasks" link is gone; the ids stay for p09.
  - **Simulation (D4.6):**
    - `entities::message_outcome` (`MessageBindingOutcome`, `BindingOutcome::{Admitted(AdmittedRun), Refused, Failed}`, `RunExecutionSummary`, `RunModelUsage`, `RUN_EXECUTION_LIMIT = 20`);
    - the port `application::message_outcomes::MessageOutcomeReader`, implemented in `persistence/workflow/message_outcomes.rs` with three bounded reads: outcomes (intent association key ⋈ intent bindings ⋈ revision ⋈ version, LEFT run), each run's newest 20 executions (LATERAL over `(company_id, run_id, id)`), and usage grouped by provider and model;
    - `inspection::run_state` is now the one decoder of the stored `state`/`waiting_reason` (the run head uses it too).
    - The page renders "Workflow outcomes" per inbound message (state badge, executions table, usage, or the refusal/failure reason) and "Written by run …" per reply; "Task Execution Parameters", `MessageTaskContext` and the stand-in payload are gone.
    - Live: `simulation_wake_ups` (messages, activity, and `WorkflowRunChanged` for the thread; `Resync` → `Lagged`).
    - **Owner-only throughout:** the page GET and the open-thread routes now require `company.user_id == user.id`, like the POST and the stream already did (D4.6).
  - **New index `model_usage_calls_run (company_id, run_id) WHERE run_id IS NOT NULL`** for the simulation's usage read and the run delete cascade. D4.6 did not list it; recorded here for review, with evidence below.
  - **Deleted (step 5):**
    - routes `task.rs`, `approval.rs`, `thread_handoffs.rs`, `ui_response_reviews.rs`, `ui_tasks.rs`, `ui_thread_handoffs.rs` (with their tests); `/ui/task-complete`, `/ui/task-ownership/*`, `/ui/task-delegation/*`; the internal-note ask/start routes; the channel reply-handling fields and endpoints;
    - pages `approvals`, `delegation`, `response_reviews`, `task_board`, `task_monitor`, `tasks`, `thread_handoff_marks`, `thread_handoffs`, `harness_diagnostics`, `agent_runtime_settings` (+ its tests);
    - the mailbox's owner/delegation/human-completion panels, held-reply banner and badges, collaboration strip, task links and Tasks rail entry;
    - the agent forms' runtime controls (advanced Rig JSON, run timeout, response format/schema) and the sub-agent picker (P08-Q7). The agent-level response contract is legacy-only: the workflow model step uses its own output schema (`application/workflow/agent.rs`), so its control goes with the runtime settings. **For B4c:** `agents.response_contract` and the form/library/API fields behind these controls (`config_json`, `run_timeout_secs`, `response_format`/`response_schema`, `sub_agent_ids`) are still parsed and stored until their columns go; `scripts/tests/agent-runtime.mjs` (the library form script's payload mapping) stays green and goes with them.
    - Cross-links: the deliveries pane's task link and task summary (and its `ThreadUseCases` plumbing); the company settings Tasks tile → Work; the member pane's "Open the task" links.
  - **Transitional (B4b/B4c):** `MailboxEvent::AttentionChanged`'s `ThreadHandoff` source and `is_thread_handoff_in_channel`; the channel/company review and reply fields in the routes (their UI controls are gone); member removal's work-at-stake (legacy task model).
- **Step 6 done: the decision-notice preference (P08-Q8, D4.3).**
  - `NotificationPreferences {delivery_failure_email_enabled, decision_email_enabled}`; read/write and the notification recipient read carry only those two (the legacy columns keep their defaults until B4c drops them). The legacy action kinds never email (their sources are withdrawn).
  - The profile shows two toggles: "Email me when a workflow decision needs my review" and delivery failures.
  - `eligible_reviewer` (decisions and the token approver, `approvers`) reads the reviewer's preference **`FOR SHARE`** after member and principal; no row means on. `compose_notices` returns `None` for an opted-out reviewer; `insert_reviewers` writes `notice_delivery_id NULL, notice_opted_out true` for them, which the guard checks against the locked row.
- **Step 7 done: D4.11 tests.**
  - Triggers over a real LISTEN connection (own databases): `workflow_run_triggers_notify_a_threadless_run_on_insert_and_state_changes_only` (no `thread_activity`, null association ids), `workflow_run_triggers_notify_a_threads_run_on_both_channels` (insert, state, waiting-reason; a `deadline` update is silent, proven with a marker).
  - Listener: `the_listener_resyncs_on_start_and_reconnect_and_drops_a_bad_run_payload` (start Resync = the outer-rebuild path, F084; a malformed payload dropped and the next delivered; `pg_terminate_backend` → Resync, F077).
  - Activity: the mapping unit tests; `ui_stream_tests` rewritten around a scripted reader: connect seeds badges and rows, a run change redraws one badge and a finished run clears it, `Resync` re-sends, the open thread streams its strip and ignores attention.
  - Simulation and lookup: `message_outcomes_lookup_and_activity_read_every_binding_of_a_message` (two admitted + one refused binding: outcomes in binding order with the reason, `run_ids` through both reads, activity Queued, another company reads nothing); `a_message_a_run_wrote_reads_back_with_its_run` (a reply's run through all three reads); the simulation fragment test; `the_simulation_wakes_for_its_threads_runs_and_a_resync`; `the_mailbox_streams_wake_for_messages_and_activity_and_not_for_attention`.
  - Preference: `workflow_decision_opted_out_reviewer_gets_no_notice_and_still_reviews` (no notice, opted-out row, the other reviewer noticed, the guard refuses an unbacked opt-out and a row with neither, the opted-out reviewer submits); **competing claimants:** `workflow_decision_park_waits_for_a_preference_toggle_in_flight` (a toggle holding the row makes the park wait — observed in `pg_stat_activity` — and the park then records the committed opt-in); the two-toggle profile test; the preference round trip per flag.
- **Index evidence (D4.2/D4.4/D4.6 + the two additions):** `logs/b4a-index-explain.sql` → `logs/b4a-index-explain.log`. Temp probe tables in `workflow08` with the production columns, keys and pre-existing indexes; two companies of 200k runs, 100k decisions, 200k reviewers, 300k schedule runs, 400k usage calls. Buffers before → after:
  - run_failure 13,011 → 1,009 (`workflow_runs_attention` + `workflow_runs_reruns`); run_reconciliation 6,754 → 106; run counts 6,754 → 1,177 (`workflow_runs_open_by_channel`); activity for 50 threads 6,754 → 405 (`workflow_runs_by_thread`); team-work decisions 3,233 → 2,524 (`workflow_decisions_open`); schedule refusals 4,832 → 65 (`schedule_runs_failed_recent`); run usage 6,182 → 30 (`model_usage_calls_run`).
  - **`workflow_decision_reviewers (company_id, user_id)` (D4.2) is not added:** driving My work from it costs 12,381 buffers (a reviewer's whole history: 2,480 reviews for 22 open), against 2,023 driving from `workflow_decisions_open` and probing the reviewer primary key, which stays bounded by open decisions. Nothing else reads reviewers by user. A change to the D4.2 index list, for review.
- **Focused checks (2026-10-24)**, all against 127.0.0.1:55480 (`workflow08_test` recreated after each schema edit):
  - mailbox, routes, pages, thread persistence, job projection: 339 passed (`logs/b4a-mailbox-focused.log`);
  - decisions, approvals, intents, deliveries, preference, profile, wake-ups: 180 passed (`logs/b4a-preference-focused.log`);
  - trigger tests 2 passed; `infra::events` 7 passed;
  - **broad B4a set** (`adapters::http domain application infra` and the persistence areas B4a touched: attention, notification, thread, user, dashboard, run_counts, delivery, workflow message/admission/schedule tests, job projection): **2,071 passed, 0 failed, 2 ignored** (`logs/b4a-broad-1.log`);
  - `cargo clippy --lib --tests`: clean (`logs/b4a-clippy.log`);
  - `node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs`: 8 passed (the navigation test's example URLs now name existing pages).
  - **Not run (left for the end, with the user's go-ahead):** `npm run check:css`. The deleted pages removed class candidates, so `assets/app.css` likely needs a rebuild; every class the changed pages use is present in it (checked by script, three pre-existing exceptions). `npm run test:run-counts` remains unavailable (no Playwright).
- **Legacy tests removed in steps 4e–6:** the page tests of the deleted pages (task monitor/board/detail, delegation, approvals, handoff banner and badges, chain timeline, task-parameter rendering, delivery task links); `task_lookup_tests.rs`, `job_projection_index_tests.rs` and the view/job-projection task-link tests (the legacy lookup is gone); the legacy thread-activity entity and persistence tests; the handoff stream tests (replaced); the runtime-settings tests.
- **Frozen** at digest `5b27364a…2c16` (`logs/b4a-snapshot.txt`, manifest `logs/b4a-files.txt`); batch review **WF-P08-R021: CHANGES REQUIRED** (F086 Low; F087 Info; F084 fixed, F085 adopted).

### B4a corrections for WF-P08-R021 (2026-10-24)
- **F086 (Low), fixed:** "Open in Simulator" was shown to every reader of a thread, but every simulation route is owner-only. `MessagePane.viewer_membership: CompanyMembership` (from `principal_access_for_user`, whose owner is `companies.user_id` — the routes' rule) and `simulator_link` render it for the owner only; the mailbox and the internal-note re-render both pass it. Test `only_the_company_owner_is_offered_the_simulator` (owner yes; admin, member, stranger no).
- **F087 (Info):**
  1. index deviations accepted — no change;
  2. "de-duplicated": the de-duplication is in Rust (`fetch_thread_runs` keeps the first of each run id), not in the SQL; the TASK wording stands;
  3. a demoted run actor's delivery-failure link to `/ui/deliveries`: kept as D4.3 states the link ("as today"); a member who can no longer manage the company still sees the in-app item and the thread through attention — noted, not changed;
  4. `check:css` and `test:run-counts` remain for the end, as recorded;
  5. the `/approvals/{token}` link builder goes with the approval code in B4b.
- **Checks:** `cargo test --lib -- adapters::http::pages adapters::http::routes::ui adapters::http::routes::channel`: 233 passed, 1 ignored (`logs/b4a-f086.log`); `cargo clippy --lib --tests -- -D warnings` clean (`logs/b4a-clippy-r2.log`); `cargo fmt --check` clean.
- **Corrected digest** `a54400486b9484c469752a35a48bfd207bdd020486e540f10c5591d4f4e720c8` (`logs/b4a-snapshot-r2.txt`, same exclusions).
- **B4a accepted in phase (2026-10-24):** B PASS **WF-P08-R022** at `a54400486b9484c469752a35a48bfd207bdd020486e540f10c5591d4f4e720c8`; root recomputed the same digest (base `da86871`, same exclusions, coordination r10). Not integrated, not committed. Rounds R021 (CHANGES REQUIRED: F086), R022 (PASS). Open for later batches: F087.5 (approval link builder → B4b); `check:css` and `test:run-counts` at the end.

### B4b work order (2026-10-24)
Scope from the B4 plan; the crate compiles after each step. Placement notes (recorded, for review):
- `HarnessConfig`, `SubAgentScope`/`AgentCapabilitySpec`'s agent-side use, and the agent `config_json`/`run_timeout_secs`/`response_contract`/`sub_agent_ids` fields stay until **B4c**, where their columns and tables go (the B4 plan lists the runtime-config fields there); B4b removes only their legacy *runtime* consumers.
- `ExternalResponseReview` and `ExternalReplyHandling` (company/channel fields until B4c) survive their side areas' deletion until B4c.
- The runtime panel's active-task gauge keeps reporting the (now idle) legacy counter until B4c retargets the panel with `runtime_metric_samples` (B4a placement).
Steps:
0. Extract shared types: `AgentExecutionOutput`/`Disposition` out of `harness/ports.rs`; `RunIdentity.harness` → a one-value literal; `TokenUsage` out of `entities/task.rs`; the delivery gauges out of the task stuck-work census.
1. Stop the runtime: TaskWorker and MemoryWorker spawns, TaskWakeups and the `task_ready`/ownership/task-chain LISTEN channels, the ThreadUseCases legacy builders and the harness registry.
2. Delete the dispatch stack: `task_worker.rs`, `agent_runner/`, trace hooks, `prompt_fence`, `dispatch.rs`, `reload.rs`, `human_completion.rs`, `InboundTaskPayload`, `task_queue.rs`, the thread delegation/internal-note task methods.
3. Delete the harness legacy: registry, ports, context, the rig legacy binding and its modules (`RigHarness`, `direct`, `compile`, `skills`, `tools`, `tool_hook`, `config`, `budget`, `trace`), `AgentCapabilityReader`, `persistence/task/harness_runs.rs`, RunCheckpoint `wait`/`outreach_wait`.
4. Delete the side areas: approvals (+ F087.5 link builder), reviews, handoffs, outreach/quorum/delegation, internal-note tasks, member-removal work-at-stake (its compile-time macros).
5. Delete legacy task persistence and entities: `persistence/task/*`, `legacy_task.rs`, `NewDelivery.task_id` and `require_legacy_task_on`, `entities/task.rs` (keeping only what the workflow job projection needs), `stuck_work`, `member_removal`.
Checks per the B4 plan: compile plus the ingest and workflow suites (inbound, message_ingress, provisioning, schedule), plus the pages/routes the deletions touch.

### B4b progress (2026-10-24)

Implemented as one deletion, then error-driven fixes until the crate compiled; the work order's
steps 0–5 are all done except where a deviation below says otherwise. Staged deletions (`git rm`)
are part of the artifact; nothing is committed.

**Runtime stopped and deleted (steps 1–3).**
- `main.rs` spawns no TaskWorker or memory worker. `infra/events.rs` has no task wakeups and no
  task-ready, ownership or task-chain channels; its listener takes `(pool, events,
  inbound_events, shutdown)`.
- `setup.rs`/`app_state.rs` wire no harness registry, memory worker or coordinator,
  approval/review/handoff use cases or agent run timeout. `ThreadUseCases` is
  `new(...).with_monitoring(...).with_intent_wakeups(...)`. `ThreadStores` lost `tasks`.
- The struct lost the response validator, harness run store, task/agent persistence, capability
  reader, approval use cases, memory, harnesses and run timeout fields, with their builders and
  getters. It lost `harness_run_diagnostics`, `ask_owner_to_act`, `start_agent_task`,
  `list_company_tasks*`, `get_task_persistence`, `change_task_ownership`,
  `execute_delegation_command` and the replacement-destination helpers.
- `InboundIngestResult::durable_task_payload`, `TaskChannelTarget`, `AgentExecutionResult` and
  `agent_response_body` are gone.
- Shared output types moved to `services/harness/output.rs`: `TokenUsage`,
  `AgentExecutionOutput`, `AgentExecutionDisposition` and `EXECUTION_DIAGNOSTICS_KEY`. The
  harness module is now `classifier`, `output`, `redaction` and `runs`.
- The harness runs protocol lost `HarnessRunStore`, `OpenRun`, `RunWrite`, `RunExecution` and
  `RunDiagnostics` (with `runs/diagnostics.rs`).
  - `RunCheckpoint` lost `wait`/`outreach_wait`, `ApprovalWait`/`OutreachWait`, their
    park/resolve methods and `supersede()`, along with `RunState::{Waiting, Superseded}` and
    `InvocationState::Waiting`.
  - The ledger check now reads: budget exceeded ⇔ `BudgetExceeded`, and `Completed` ⇔ a final
    output.
  - `harness_checkpoint::state_name` follows. The DB state CHECK still lists the old values; it
    narrows in B4c.
- The rig adapter keeps `execution.rs` for the workflow binding only, with
  `Binding = Box<WorkflowBinding>`. `RigHarness`, the legacy binding and the
  direct/compile/skills/tools/tool_hook/config/budget/trace/schema modules are deleted.
- Monitoring: `record_task_execution`, `TaskExecutionMetrics` and `TaskStatusMetric` are gone
  from the trait and all three monitors, and the in-memory stats lost their `tasks` block. The
  stuck-work census is delivery-only and published by `DeliveryWorker::reap()`; the gauge keys
  are unchanged.

**Side areas and legacy persistence (steps 4–5).**
- Deleted: approvals (closing **F087.5**, since the `/approvals/{token}` link builder went with
  `use_cases/approval.rs`), response reviews, thread handoffs (entity trimmed to
  `ExternalReplyHandling`), outreach, delegation, collaboration, member-removal entities,
  `persistence/task/*`, `legacy_task.rs`, `task_queue.rs`, `thread/reply_publication.rs` and
  `thread_lock_test_support.rs`.
- `entities/task.rs` is only `TaskStatus` (the job status the dashboard counts).
- `internal_note.rs` keeps `AddInternalNote`, `TombstoneInternalNote` and `InternalNoteView`.
- Deliveries: `task_id` is gone from `DeliveryRequest`, `NewDelivery`, `DeliveryRecord`,
  `DeliveryEntry`, the row struct, `DELIVERY_COLUMNS`, the view select and the insert
  (placeholders renumbered).
  - Removed with it: `require_legacy_task_on`, `DeliveryReader::list_task_deliveries`,
    `deliveries_for_tasks` and `cancel_task_deliveries_on`.
  - The `message_deliveries.task_id` column drops in B4c.
- Dead code the deletions exposed is removed:
  - attention helpers (`command_fingerprint`, `actor_is_manager`, `require_*_principal`,
    `lock_attention_source`);
  - `channel_gate::task_gate_key_on`;
  - `response_schema::validate_publication`;
  - in thread code: `scrub_json_secrets`, `preferred_email_for_principal`, and the support
    helpers (`outbound_reference_ids`, `rfc_message_id`, prompt text, `is_third_party_address`).
- `.sqlx`: 18 stale query files removed (45 → 27). Each was matched by exact query text against
  the current source, and all 18 appear only in HEAD's `persistence/company_invite.rs` (the
  removed work-at-stake code and its tests). `SQLX_OFFLINE` builds of every target succeed.
  sqlx-cli is not installed, so `cargo sqlx prepare --check` was **not run**.

**Deviations from the work order (for review).**
- **DV-B4b-1 Member removal is plain.** `remove_company_team_member` = owner verification, the
  owner-row refusal, then `remove_member` (the principal is demoted to external). Work-at-stake,
  `MemberWorkCommands`, `checked_handover`, the guided removal and the team pane's handover UI are
  deleted. Reason: in the workflow model a removed member holds nothing to hand over. A run-as
  user who is no longer a member makes admission refuse, and decision authority is rechecked at
  submission.
  - Tests: `the_owner_removes_a_member` (use case) and
    `removing_a_member_demotes_their_principal_and_spares_the_owner` (DB).
- **DV-B4b-2 Channel agent removal stops nothing.** With no legacy work there is nothing to
  withdraw. `channel_assignment.rs` keeps the gate, the channel and assignment locks, the settings
  write, the diff and the time budget.
  - `RemovalBudget` is `{lock_timeout, deadline}`. `AssignmentRemoval` is `{removed_agent_ids}`,
    with the summary "Removed N agent(s)."
  - Tests rewritten to six: report removed, reorder/add removes nobody, failed edit rolls back,
    owner kept, omitted list keeps agents, lock timeout. Channel agent lists themselves go in B4c.
- **DV-B4b-3 Help/bounce directory.** It no longer borrows the responding agent's description
  when a channel has none, because that read went through `ThreadUseCases`' agent persistence.
  The test `a_channel_without_its_own_description_borrows_its_agent_s` is removed; channel agent
  lists go in B4c.
- **DV-B4b-4 `RunIdentity.harness` literal deferred to B4c.** `HarnessKind` stays while
  `HarnessConfig` (B4c) uses it; replacing it now would duplicate the enum.
- **DV-B4b-5 `AgentCapabilityReader` kept.** It is the skill editor's read
  (`SkillUseCases::company_agent_capabilities`), not a runtime port. Renamed
  `load_for_execution` → `load_capabilities` (and `execution_company_id` → `company_id`).
- **DV-B4b-6 Memory provider registry.** It is no longer built in `setup.rs`, since nothing reads
  it. The configured-provider set still feeds `MemoryUseCases` until memory is deleted (B4c).
  `MemoryProviderActivity` stays for the runtime panel (B4c).
- **DV-B4b-7 Tests deleted with their subject.** None was weakened.
  - Legacy dispatch and reply Cc: `a_failed_agent_run_commits_no_reply_message_and_no_delivery`,
    the two `outbound_cc_for` tests, the payload-shape test, `MockTaskPersistence`,
    `MockAgentPersistence`.
  - SMTP: `MockTaskPersistence`.
  - Legacy-isolation job tests that call deleted Rust APIs: six in `job_core_query_tests`, all of
    `job_external_query_tests` and `job_handoff_consumer_tests`, and one each in
    `job_association_tests`, `job_source_association_tests` and `job_projection_tests` (×2).
  - `harness_owner_tests`: the legacy-supersede test and the legacy-store assertions.
  - Dashboard attempt-ledger test; multi-task enqueue test; sibling-publication test; the
    member-pane handover test.
  - Kept: the dashboard D4.5 job-count test (still on `mixed_fixture`) and the SQL-level
    isolation tests that still compile. Those go with the legacy schema in B4c.
- **DV-B4b-8 Cancellation tests.** These worker-path tests now cancel through a `#[cfg(test)]`
  seam, `cancellation::cancel_deliveries_on`. It is the same lock-in-id-order-then-settle
  through `cancel_locked_deliveries_on`, keyed by delivery id, and it uses `WorkflowCancelled`.
  All six cancellation tests still run.

**Left for B4c/B4d (recorded, not done here).**
- Cancellation reasons: `DeliveryCancellationReason::{ChannelAgentRemoved, TaskStopped}`, with
  their CHECK.
- Legacy variants of `AttentionWakeSource`, with their triggers.
- `AgentCapabilitySpec` (unused).
- `TASK_WORKER_CONCURRENCY` and the runtime panel's task-worker capacity.
- `DEFAULT_AGENT_RUN_TIMEOUT_SECS`/`MAX_AGENT_RUN_TIMEOUT_SECS`.
- The reply-handling comment in `persistence/channel.rs` (~491).
- `find_outbound_reply_after`'s outreach exclusion and its test's raw legacy rows.
- Team settings copy that mentions "tasks" (B4d).

**B4b checks (2026-10-24, cluster 127.0.0.1:55480, `workflow08` / `workflow08_test`, no schema
edit so no DB recreate).**

| Check | Result |
| --- | --- |
| `SQLX_OFFLINE=true CARGO_INCREMENTAL=0 cargo check --all-targets` | clean, no warnings |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`) | clean |
| `cargo fmt --all -- --check` | clean |
| Batch 1: `cargo test --lib -- use_cases::thread use_cases::company_invite use_cases::channel adapters::persistence::{company_invite,channel_assignment,delivery,dashboard,thread} services::harness services::delivery_worker adapters::monitoring domain::monitoring entities::{stuck_work,task,internal_note,delivery} adapters::smtp adapters::protocols::email infra::events infra::config adapters::http::pages adapters::http::routes::ui_stream application::transport` | **507 passed**, 0 failed, 3 ignored (`logs/b4b-focused-1.log`). The first run failed one assertion of my new removal test: the owner has no member row. The assertion was corrected (the member row is gone, the owner's principal stays `person`) and rerun green. |
| Batch 2: `cargo test --lib -- adapters::persistence::workflow::tests::{run_schema_tests,admission_tests::lease_tests,schedule_tests} message_ingress adapters::persistence::inbound_event application::services::inbound_event_worker application::workflow adapters::harness::rig adapters::http::routes adapters::persistence::{skill,agent}` | **972 passed**, 0 failed (225 s, `logs/b4b-focused-2.log`) |
| `cargo sqlx prepare --check` | **not run** (sqlx-cli not installed; not installing unasked) |
| full suite, `scripts/stack-budget.sh`, `npm run check:css`, `npm run test:run-counts` | not run (end of task, ask the user; Playwright absent) |

### B4b corrections for WF-P08-R023 (2026-10-24)

R023 returned CHANGES REQUIRED. All actionable findings are fixed in one round.

- **F088 (Medium).** `#[path = "harness_store_tests.rs"] mod store_tests;` is restored in
  `harness_owner_tests.rs`; the deletion hunk had also taken those two lines.
  - The 4 workflow checkpoint-store tests compile and pass again, including the two
    competing-claimant tests.
  - A whole-tree orphan scan (every `.rs` file reached by a `mod` or `#[path]`) finds no other
    undeclared file once F089 is fixed.
  - DV-B4b-7's "none was weakened" now holds.
  - Optional guard against undeclared files: not added. It is recorded as a candidate instruction
    improvement.
- **F089 (Low).** `git rm src/application/use_cases/thread/model_usage_tests.rs`.
- **F090 (Low).** Dead code removed:
  - `AgentExecutionDisposition`. The enum and the output's `disposition` field are gone: with
    `Suspended` unreachable a one-value persisted field says nothing, and the checkpoint JSON
    changes shape (no backward compatibility).
  - The diagnostics `validation` loses its unreachable `"pending"` arm.
  - `stamp_duration` and `EXECUTION_DIAGNOSTICS_KEY`.
  - `Progress::Suspend` and `Driven::Suspended` now carry a `WaitId`, not an `Option`, and the
    unreachable no-wait arm is removed.
  - `ThreadUseCases::{save_message_with_deliveries, compose_delivery, get_agent_history}`.
  - `InboundEnvelope::addressed_in` and `MessageWrite::with_structured`.
  - The test helpers `email_write`, `insert_thread`, `queued_deliveries`, `only_mail`,
    `recording_transport` and `test_machine`.
  - `agent_run_timeout_from_env`, its parser, `DEFAULT_AGENT_RUN_TIMEOUT_SECS` and their two
    tests; nothing read the variable since B4b.
  - The cascade, found by re-running a HEAD-relative scan of uncalled functions and of trait
    methods that are implemented but never called until both were empty:
    - `ThreadPersistence::create_message_with_deliveries` (trait, Postgres impl, in-memory impl
      and its unread `deliveries` store field);
    - `TransportRenderer::classify_external_destination` and `ExternalDestinationClassification`;
    - `ParticipantPersistence::identities_for_principals`;
    - `CompanyPersistence::{list_company_team_accounts, list_company_team_emails}` (trait,
      Postgres impl and 6 doubles), `CompanyTeamAccount` and
      `CompanyMembership::from_access_role`.
  - 2 more `.sqlx` files went stale with the two company queries. Both were confirmed by exact
    text in HEAD's `persistence/company.rs` and removed, leaving 25 files; the offline build
    passes.
- **F091 (Low).** Stale comments and intra-doc links reworded:
  - `ingress.rs` and `InboundIngestResult` docs (`InboundTaskPayloadV1`);
  - `classifier.rs` and `dependency_tests.rs` (`AgentHarness`, `harness/tests.rs`);
  - `dashboard.rs` (`super::task`);
  - `app_error.rs` ("task worker");
  - the `HarnessKind` reason, which now names B4c;
  - `in_memory_monitor.rs` (`delivery_dead_lettered`);
  - `message_runtime.rs` (`dispatch.rs`);
  - `persistence/channel.rs` (`ThreadHandoffPolicyPersistence`).

  `cargo doc --lib --document-private-items --no-deps` now reports 12 unresolved links. All 12
  were also in R023's run and none names a B4b-deleted item; the 4 that did are gone.
- **F092 (Low).**
  - New DB test `delivery::tests::the_stuck_delivery_census_counts_each_stall_and_not_a_due_delivery`
    (`own_database`, because the census is global). It covers dead-letter, overdue retryable and
    outcome_unknown, which count once each, plus a pending delivery due inside the threshold and a
    delivered one, which do not.
  - The census comment now states the sequential pass. HEAD's census made the same scan.
- **F093 (Info), dispositions:**
  1. TASK wording on `.sqlx`: two of the first 18 queries also appeared in HEAD's `controls.rs` and
     channel-assignment tests. The removal stands, because none is in the current source. `prepare
     --check` is still owed (no sqlx-cli).
  2. A member removed while they are a decision's only reviewer leaves it unsettleable until its
     deadline or a cancel. HEAD has the same gap; recorded for B4d's close-out list.
  3. Memory jobs are not drained until B4c deletes memory: accepted.
  4. The docs, `.env.example` and `fly.toml` still name `AGENT_RUN_TIMEOUT_SECS`: B4c/B4d (the
     parser is now gone).
  5. B4d's doc list gains the `src/**/AGENTS.md` guides.
  6. The task NOTIFY triggers fire with no listener until B4c drops them: accepted.
  7. Renaming `RemovalBudget`: not done. Channel agent lists, and with them `channel_assignment`,
     go in B4c.

**Checks after the corrections** (cluster 127.0.0.1:55480, `workflow08_test`; no schema edit)

| Check | Result |
| --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`) | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo doc --lib --document-private-items --no-deps` | builds; 12 unresolved links, all also present in R023's run, none naming a B4b item |
| orphan-module scan, HEAD-relative uncalled-function and uncalled-trait-method scans, `.sqlx` exact-text scan | clean (`.sqlx` 25 files, none stale) |
| `cargo test --lib -- harness_owner_tests adapters::persistence::delivery services::delivery_worker services::harness adapters::harness::rig use_cases::{company,company_invite,channel,participant,thread,agent} adapters::persistence::{company,participant,thread} adapters::smtp adapters::protocols::email application::transport infra::config adapters::monitoring entities::company adapters::http::pages adapters::http::routes` | **707 passed**, 0 failed, 3 ignored (`logs/b4b-r023-corrections.log`). This includes the 4 restored `store_tests` and the new census test. |
| `cargo test --lib -- agent_model_step agent_runtime harness_checkpoint checkpoint message_ingress application::workflow adapters::persistence::workflow::tests::admission_tests::lease_tests` | 777 passed, **1 failed** (`logs/b4b-r023-corrections-2.log`): `lease_tests::resolution_tests::workflow_lease_resolution_stall_beyond_deadline_unknown_then_one_expiry`, at `assert!(elapsed >= gated_policy().persistence_deadline())`. Rerun alone it **passed 3/3**. It also failed once in B1 (`logs/b1-run1.log`) and was green later. B4b's delta touches no lease, persistence-bound or workflow-claim code. Retained as an **intermittent timing verification gap** under parallel load, not claimed unrelated. |

### B4b acceptance (2026-10-24)

- **B4b accepted in phase.** B PASS **WF-P08-R024** at
  `c4b7eaa9c3625d3ac8e0d74a7afa802efcdf8a9dfee1f90a02de81107e63c896` (base/HEAD `da86871` plus
  the working tree and index; same exclusions; `logs/b4b-snapshot-r2.txt`). Root recomputed the
  same digest. Coordination r10 (`1df326af…`) was unchanged throughout.
- Rounds: R023 CHANGES REQUIRED (F088 Medium; F089–F092 Low; F093 Info), then R024 PASS (all
  fixed or resolved). Not integrated, not committed.
- Carried forward:
  - **F094** (Low) goes to the phase gate. The intermittent `lease_resolution` timing test
    predates B4b and lies outside it. The fix is planned for B4d, before the full suite: print
    `elapsed`, then either tolerate the server's 1 ms `lock_timeout` granularity or have
    `resolve_retryable` wait until the deadline.
  - B4d also takes F093.2 (a member removed while they are a decision's only reviewer), the
    `src/**/AGENTS.md` docs and `AGENT_RUN_TIMEOUT_SECS` in the docs.
  - `cargo sqlx prepare --check` is still owed; sqlx-cli is not installed.

## B4c plan (2026-10-24)

B4c is split by area rather than into "code then schema". Some code and schema are coupled: for
example, provisioning inserts `channel_agents` rows only because the schema's owned-channel
triggers demand them. Each sub-batch therefore removes an area's code **and** its init-migration
objects together. This refines the B4 plan's "one init-migration edit" into three area-scoped
edits of the same file (still no backward compatibility). `workflow08_test` is recreated after
each.

Each sub-batch gets one batch review. Input: `logs/b4c-inventory.md`, a read-only inventory not yet
verified. Every item is checked against the code before it is edited, and each UNSURE item is
decided with recorded evidence: drop it if nothing in the workflow tree reads or writes it, keep it
otherwise.

| Sub-batch | Scope |
| --- | --- |
| **B4c-1** Memory (P08-Q1) | The memory code (adapters `memory/*`, the memory services, coordinator, worker, job lease and schedule, `persistence/memory.rs`, entities, use cases, the routes and pages and their copy, config `HYDRADB_*`/`HINDSIGHT_*`, `MemoryProviderActivity`). Company, channel and agent memory fields. The memory tables, columns, checks, triggers and functions. `runtime_metric_samples.hydradb_*` and its sampler and panel parts. |
| **B4c-2** Channel agents and agent runtime config | `channel_agents`, `channels.owner_agent_id`, `response_trigger`, the review and reply-handling overrides, `preferred_reviewer_principal_id`, the company review and reply defaults, and the dead `add_3rd_party` if confirmed unread. `channel_assignment`, `ExternalResponseReview`/`ExternalReplyHandling`. `agents.config_json`, `run_timeout_secs`, `response_contract`, `agent_sub_agents`, `HarnessConfig`/`HarnessKind`/`SubAgentScope`/`AgentCapabilitySpec`, and `RunIdentity.harness` becoming the fixed literal `"rig"`. The provisioning edit (inventory hazard 8). The matching schema objects. The agent and channel forms. |
| **B4c-3** Legacy runtime schema | The legacy task and side-area tables. The shared-table trims (`background_tasks`, `task_attempts`, `task_harness_runs`, `message_deliveries.task_id`, notifications/events, `user_notification_preferences`, `runtime_metric_samples` task columns with the runtime-panel retarget to workflow jobs and slots, per D4.8). The rewrites of the shared functions. The legacy enum variants (`TaskStatus`, notification kinds, `DeliveryCancellationReason::{ChannelAgentRemoved,TaskStopped}`, `AttentionWakeSource`). The D4.8 catalog test (no function body names a dropped object) and the F083 `channel_gate` competing test. The workflow `job_*`/`harness_*`/constraint/schema tests. `.sqlx` by exact-text check (sqlx-cli absent). |

Focused checks per sub-batch:
- `cargo check --all-targets`, clippy `-D warnings` and fmt.
- The affected persistence, use-case, page and route suites.
- After the DB recreate, the schema and catalog tests (`adapters::persistence::tests`,
  `constraint_tests`) plus that area's workflow suites.

B4d keeps docs/config/bootstrap, the `src/**/AGENTS.md` guides, F073, F094, F093.2, the
exhaustive caller searches and the phase gate (full suite with the user's go-ahead).

### B4c-1 progress (2026-10-24)

Scope: the B4c plan row **B4c-1 Memory (P08-Q1)**. Inventory items for memory were checked against
the code before editing. Nothing in the workflow tree reads or writes the provider memory (the
workflow `memory.load`/`memory.save` steps stay registered and unhandled; `infra/workflow.rs`
`UNHANDLED_STEP_KINDS` and its production-refusal test are unchanged).

Code removed:
- Whole modules (git rm): `src/adapters/memory/`, `application/services/{memory_coordinator,
  memory_job_lease, memory_job_schedule, memory_provider, memory_worker}.rs`,
  `application/use_cases/memory.rs`, `adapters/persistence/{memory,memory_rows}.rs`,
  `domain/entities/memory.rs`; their `mod` registrations.
- Company/channel/agent memory fields (`retrieve_/persist_{company,agent,user}_memory`, the
  company `default_*` variants, `memory_enabled`, `memory_persistence_mode`, `memory_recall_mode`,
  `memory_max_results`, `companies.memory_provider`) from entities, writes, forms, drafts, JSON
  payloads, pages, routes, persistence SQL/binds/row mappings and test literals.
  `CompanyChannelDefaults` is now `{add_3rd_party, participant_emails}` (its fate is a B4c-2 item).
- Config: `AppConfig.hydradb/hindsight`, `MemoryProviderHttpConfig`, `configured_memory_providers`,
  their `from_env` reads and the six memory config tests. `.env.example` and `docs/deploy.md` no
  longer name `HYDRA_DB_*`/`HINDSIGHT_*` (AGENTS.md: documented keys must be ones the app reads).
- Wiring: `AppState.memory_use_cases`/`memory_provider_activity`, setup's `MemoryUseCases` and
  `with_memory_persistence`, `ChannelUseCases::memory_ready`, `main.rs`'s sampler argument.
- Routes: `/api/companies/{id}/memory` (status/retry), `/ui/companies/{company_id}/memory/retry`,
  `CompanyResponse.memory`, `MemoryStatusResponse`, the validate/apply-provider steps in every
  company create/update handler (HTML, JSON, pane), `SubmittedMemorySettings` and
  `memory_settings()` on the channel forms.
- Pages: the company memory select and status, the channel memory grants fieldsets (settings and
  classic), the agent memory controls, `*_pane_with_memory` variants (the plain
  `company_{create,edit}_pane` / `channel_{create,edit}_pane` are now the only entry points),
  `AgentChannelTab.memory_ready`, the agent-library multi-select's memory payload keys.
- Runtime metrics: `MemoryProviderActivity`, `MemoryProviderInterval`, the bucket and sample
  `hydradb_*` fields, the persistence columns/aggregates and the dashboard memory-provider panel
  and its tests. `memory_limit_bytes` (cgroup) is unrelated and kept.
- `AgentWrite`'s hand-written `Default` became `#[derive(Default)]` (clippy: the memory defaults
  were the only non-derivable values).
- Tests removed as compatibility-only or memory-only: `obsolete_channel_memory_tuning_is_ignored_by_the_form`
  (ui_channels; asserted obsolete fields were ignored), `a_blank_agent_form_stays_inside_the_bounds_its_own_fields_declare`
  (pages; all three asserts were memory controls), the memory-provider dashboard tests; memory
  asserts dropped from surviving tests (one page test now checks the shared
  "Capabilities &amp; advanced options" editor instead of "Memory persistence").

Schema (init migration edited in place; `workflow08_test` recreated):
- Dropped 26 dump segments: tables `memory_provider_connections`, `memory_provisioning_jobs`,
  `memory_remote_resource_lifecycles`, `memory_cleanup_jobs` with their PK/unique/FK constraints,
  two indexes, three triggers and the three trigger functions
  (`create_/retire_memory_lifecycle_for_legacy_connection`, `synchronize_legacy_memory_provisioning_phase`).
- Dropped 28 lines: channels' six memory columns; agents' four memory columns and three checks;
  companies' six default memory columns, `memory_provider` and its check;
  `runtime_metric_samples.hydradb_{calls,failures,duration_ms}` and their four checks. None was a
  table's last line, so no comma repair was needed. No other object names memory (`grep -i memory`
  leaves only `memory_limit_bytes` and its check).
- Applied the full migration set to a scratch database (`workflow08_migcheck`, dropped after): clean.

New tests:
- `adapters::persistence::tests::the_memory_provider_schema_is_gone` (catalog: no `memory%` table,
  no `%memory%`/`hydradb%` column except `runtime_metric_samples.memory_limit_bytes`, no
  `%memory%` function). Mutation-checked: a temporary `memory_probe` table makes it fail
  ("memory tables remain"); restored and green.
- `adapters::persistence::company::tests::a_company_save_writes_every_field_to_its_own_column`
  (DB): both company UPDATEs had their placeholders renumbered and no test ran them; this round-trips
  every field through `update_for_user`, checks omitted policies are preserved by `update`, and
  that a stranger's owner-scoped save fails and leaves the row.
- Renumbered channel UPDATE and agent INSERT/UPDATE are already DB-covered
  (`persistence::channel` CRUD test; `agent_config_tests` response-contract/config round trips;
  `agent_api_tests` workflow_tools).

Checks (`DATABASE_URL=postgres://workflow08@127.0.0.1:55480/workflow08 SQLX_OFFLINE=true CARGO_INCREMENTAL=0`):
- `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`: clean.
- `.sqlx`: no compile-time macro text changed; all 25 remaining files' queries found verbatim in `src`.
- Dead-code scans (HEAD-relative, scratchpad `uncalled.py` + a type/const variant): nothing new
  but two false positives (a trait impl method, a test fn). No Cargo dependency became unused.
- Focused: `logs/b4c1-focused-1.log` 307 passed, 1 failed (a page test still asserting the deleted
  channel-memory copy; asserts removed); `logs/b4c1-focused-2.log` **331 passed** (persistence
  company/channel/agent, runtime_metrics, use cases agent/channel/company, all page tests, routes
  company/ui_companies/ui_channels/agent/ui_agents/channel, infra config/setup/workflow,
  delete_company, constraint_tests, decision_provenance, job_core_query, job_handoff_history,
  job_routine, wait_guard, response_trigger, agent_api, ui_team, company_member);
  `logs/b4c1-focused-3.log` 15 passed (new company test); `logs/b4c1-focused-4.log` 1 passed (new
  schema test); `logs/b4c1-focused-5.log` **32 passed** (`adapters::persistence::tests`,
  `constraint_tests`, company, agent_config) after the recreate.
- Not run: `cargo sqlx prepare --check` (no sqlx-cli), `npm run check:css` (end, with the user's
  go-ahead; memory controls were removed from pages, so classes may now be unused, never missing).

Left for later sub-batches (by plan): docs/AGENTS copy that still describes memory (README,
`docs/rig.md`, `docs/transport_architecture.md`, `src/adapters/persistence/AGENTS.md` memory
section) → B4d. `main.rs` `NO_BACKWARD_COMPATIBILITY_NOTES` keeps the memory plan files (they
still carry the note). `marketing/` and `website/` are excluded user content and untouched.

Manifests: per-file hashes `logs/b4c1-filehashes.txt`; delta against the B4b acceptance freeze
(`logs/b4b-filehashes-r2.txt`) `logs/b4c1-delta.txt` (42 changed, 17 deleted, 14 first touched).

Frozen for review: digest `94139bfd7ef976af49f6ac6814c86e1b905521d9389a7cc140b72cbe5bec1e11`
(base/HEAD `da86871` plus the working tree and index; exclusions
`plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md marketing website`;
`logs/b4c1-snapshot.txt`). Coordination r10 (`1df326af…d9ae`) unchanged. Review: WF-P08-R025.

### B4c-1 acceptance (2026-10-24)

- **B4c-1 accepted in phase.** B PASS **WF-P08-R025** at
  `94139bfd7ef976af49f6ac6814c86e1b905521d9389a7cc140b72cbe5bec1e11` (base/HEAD `da86871` plus
  the working tree and index; same exclusions; `logs/b4c1-snapshot.txt`); root recomputed the same
  digest. Coordination r10 (`1df326af…`) unchanged. One round. Not integrated, not committed.
- B's checks: clippy/fmt clean; focused 616 passed (`logs/r025-focused*.log`); transport,
  services and monitoring 83 passed; CI node tests 8 passed; migration checksum and catalog queries
  on `workflow08_test`; macro-bound `.sqlx` check (`logs/r025-sqlx-macro-check.py`).
- Carried into B4c-2 (step 0, before any other edit):
  - **F095 (Low, from B4b):** `git rm` the two stale `.sqlx` files
    (`query-e83a43e16f43…`, `query-fe922e729f61…`), whose macros lived in the work-at-stake tests
    B4b deleted. From now on the `.sqlx` check is B's macro-bound script, not text presence (it
    computes sqlx's SHA-256 key per `query!`/`query_as!`/`query_scalar!` literal).
  - **F096.1 (Info):** `AGENT_RUN_TIMEOUT_SECS` is no longer read (since B4b r2) but
    `.env.example`, `fly.toml` and `docs/deploy.md` still name it; it goes with the B4c-2 runtime
    config removal.
  - F096.2 (Info, optional): strengthen the advanced-editor page assert (e.g. `open`).
- To B4d: F096.4, `docs/deploy.md` "`ERROR_BACKOFF` in `task_worker.rs`".

### B4c-2 work order (2026-10-24)

Inputs:
- The B4c plan row B4c-2, and D4.7/D4.8 of `logs/decision-D4-r2.md`.
- A read-only Explore verification of every B4c-2 item against the working tree (the B4c-1
  freeze). Its verdicts, re-checked by root at the decision points:
  - DEAD-TO-WORKFLOW:
    - `channel_agents` and its three functions and triggers, and the `Channel.agent_ids` readers;
    - `channels.owner_agent_id` and its unique key, FK, `prevent_owned_channel_delete` and
      `enforce_owned_channel_position_zero`, plus `create_owned_agent_channel`, which has no
      production caller;
    - `channels.response_trigger` and `ChannelResponseTrigger` (ingress computes mention and
      reply on its own);
    - both review/reply overrides, `preferred_reviewer_principal_id`, the company
      `external_response_review` and `external_reply_handling`, and the enums
      `ExternalResponseReview` and `ExternalReplyHandling`. They are written only: `CompanyDb`
      never reads the policies back, and the workflow reviewer query uses access mode and grants;
    - `companies.default_participant_emails`, which is stored but never applied;
    - `agents.config_json`, `run_timeout_secs` and `response_contract` (the model step's contract
      comes from the step `output_schema`; the run deadline comes from the claim window);
    - `AGENT_RUN_TIMEOUT_SECS`, which is not read since B4b;
    - `agent_sub_agents`, `sub_agent_ids`, `SubAgentScope`, `HarnessConfig` with its Rig
      advanced config, and `AgentCapabilitySpec`, which is never constructed;
    - the uncalled `use_cases/channel.rs` internal-target helpers.
  - LIVE, kept:
    - `channels.add_3rd_party` (ingress decides from it whether CC'd outsiders join);
    - `companies.default_add_3rd_party` (provisioning copies it into new assistant channels);
    - `HarnessKind::Rig` and `RunIdentity.harness` (the D4.8 fixed literal; the stale "goes in
      B4c" comment gets reworded);
    - `channel_gate` (D4.8).

Decisions (no architecture gate: none is costly to unwind; B reviews them in the batch):
- **DV-B4c2-1: internal relay to a plain channel.** `resolve_internal_destination` drops its
  "has no configured agent" refusal.
  - Since D4.7 a plain channel is valid, and what answers it is decided by its workflow bindings
    at admission.
  - The company, self-relay, enabled and sender-identity checks stay.
- **DV-B4c2-2: channel edits keep the gate, lose the assignment diff.**
  - `channel_assignment.rs` becomes `channel_edit.rs`, which keeps:
    - the time budget;
    - the exclusive `channel_gate`;
    - the tenant-checked `FOR UPDATE` row lock;
    - the settings write.
  - `ChannelGateAccess::ChangeAssignments` is renamed `ChangeSettings`. The gate's module doc is
    rewritten for the workflow: the inbound commit takes it shared, and a settings edit takes it
    exclusively.
  - The lock-timeout and rollback tests are kept and adapted. The assignment and owner tests go.
- **DV-B4c2-3: the legacy ownership trigger goes with `channel_agents`.**
  - `initialize_task_ownership()` and its `queue_kind='legacy'` trigger are the last schema reader
    of `channel_agents`, and no production code inserts legacy rows since B4b.
  - Both are dropped here, not in B4c-3.
  - The two legacy-trigger fixtures (`job_core_query_tests`, `job_routine_tests`) seed the legacy
    owner explicitly until B4c-3 rewrites those suites.
  - After the recreate, a catalog query confirms that no function body names a dropped
    object. The D4.8 test itself stays in B4c-3.
- **DV-B4c2-4: `CompanyChannelDefaults` stays as `{add_3rd_party}`.** Its doc becomes "copied into
  each new assistant channel". The participant-emails default and its spam-scan precondition are
  deleted with the column.

Steps:
- **0.** F095: the two stale `.sqlx` files `git rm`'d (done). From now on the macro-bound check
  `logs/r025-sqlx-macro-check.py` is the `.sqlx` gate (23/23, none stale or missing).
- **1. Agent side.** Remove:
  - the runtime-config columns, the harness config types, sub-agents, the response contract column
    and `ContractUpdate` if nothing else uses it;
  - their forms, JSON payloads, library copies, pages, JS and tests;
  - the owned-channel branch of the agent update (it becomes a plain update);
  - `AGENT_RUN_TIMEOUT_SECS` from `.env.example`, `fly.toml` and `docs/deploy.md` (F096.1).
- **2. Channel side.** Remove:
  - `channel_agents` from persistence, provisioning, forms, pages and mailbox avatars, and the
    agent page's "used by";
  - the owner, `response_trigger`, the overrides, the preferred reviewer, both company policies,
    the default participants, the dead helpers and the relay agent check;
  - the owned-channel agent tab and its route.
- **3. Schema.** Edit the init migration in place, recreate `workflow08_test`, and check the
  function bodies in the catalog. Then extend `the_memory_provider_schema_is_gone`'s neighbour:
  a retired-objects assertion for this batch's tables and columns.
- **4. Checks.**
  - `cargo check --all-targets`, clippy `-D warnings` and fmt;
  - the macro-bound `.sqlx` check;
  - the uncalled-definition scans;
  - focused tests: persistence channel, agent, company, `channel_edit`, provisioning,
    `agent_config`, `constraint_tests` and `adapters::persistence::tests`; the use cases agent,
    channel, company and thread; pages, routes and assistant_channels; thread ingest and relay;
    and the workflow suites that touch channels or agents (`message_ingress`, `provisioning*`,
    `agent_*`, `harness_owner`, `job_core_query`, `job_routine` and `authorization_cases`);
  - `scripts/tests/agent-runtime.mjs`, if it is affected.

### B4c-2 progress (2026-10-24)

Implements the B4c-2 work order above (DV-B4c2-1..4). Step 0 (F095) done first.

**Agent side**
- `Agent` loses `config_json`, `run_timeout_secs` and `response_contract`. `AgentWrite` loses those
  three plus `sub_agent_ids`.
- Deleted outright:
  - `HarnessConfig`, `RigAdvancedConfigV1`, `SubAgentScope`, `AgentCapabilitySpec` and the
    run-timeout / sub-agent constants;
  - `ContractUpdate`;
  - `AgentUseCases::with_response_validator`, which only checked the agent contract.
- `entities/harness.rs` now holds only `HarnessKind` (the D4.8 literal; doc reworded).
  `ResponseContract` stays: step `output_schema`, run identities and messages use it.
- `OwnedAgentChannelPersistence` → `LibraryAgentCopyPersistence`, which now has only
  `create_agent_from_library`. `create_owned_agent_channel` had no production caller and is
  deleted.
- The agent update is a plain transactional `update_agent`: capabilities, then one UPDATE whose
  `$1..$9` binds were recounted. The owned-channel slug and binding move are gone. A slug collision
  is now `Conflict("An agent with that slug already exists.")`, the same as create.
- Capability loading returns skills only.
- Removed from the HTTP and JS surfaces:
  - the classic agent form, the `/ui` agent form and the JSON payloads (`deny_unknown_fields`
    now rejects the retired keys);
  - the library routes and their response;
  - the library JS payload (it can no longer throw, so the "enter valid JSON" path went too);
  - the classic agent row and edit fields (run timeout, config);
  - the agent draft's response-format fields;
  - `stored_agent_config` and `agent_response_schema`.
- `AGENT_RUN_TIMEOUT_SECS` is removed from `.env.example`, `fly.toml` and `docs/deploy.md`
  (F096.1).
- `docs/rig.md`: the agent example lost `config_json`. Its structured-answer section now says a
  step's `output_schema` is the contract. The parser test still reads all three examples.
  `docs/custom_tools.md` no longer documents `config_json`.

**Channel side**
- `Channel` loses `owner_agent_id`, `agent_ids`, `response_trigger` and `is_owned_by`.
  `ChannelResponseTrigger` and `AnswerFacts` are deleted, with both
  `channel_response_trigger_tests.rs` files.
- `ChannelWrite` loses `agent_ids`, `response_trigger`, the review override and the preferred
  reviewer.
- Deleted outright:
  - `AssignmentRemoval` and `ChannelUpdateOutcome`; `ChannelPersistence::update` now returns
    `Channel`;
  - the owner pin and owner delete refusal in the channel use cases;
  - the uncalled `check_internal_target`/`resolve_internal_target` helpers, plus an orphaned doc
    comment above them;
  - `actor_user_id`;
  - `list_assignable_agents`, `list_selectable_agents` and `SelectableAgents` (pickers only).
- `response_draft.rs` and `thread_handoff.rs` (the two policy enums) are deleted. The company
  review and reply policies, the channel overrides and the default participants are removed from
  persistence, writes, forms, pages and the company spam precondition. `CompanyForm::write` lost
  its spam flag.
- `CompanyChannelDefaults` is now `{add_3rd_party}`, documented as copied into new assistant
  channels (DV-B4c2-4).
- **DV-B4c2-2.** `channel_assignment.rs` became `channel_edit.rs`: the budget, the exclusive gate,
  the tenant-checked `FOR UPDATE` and the settings write. The UPDATE is now `$1..$7`.
  `ChannelGateAccess::ChangeAssignments` is renamed `ChangeSettings`. The gate key prefix is
  renamed `channel-gate:`; the only user is in Rust. The `channel_gate` module doc is rewritten
  for workflow admission.
- **DV-B4c2-1.** `resolve_internal_destination` no longer refuses a target with no agent.
- Provisioning no longer inserts into `channel_agents`, and `channel_write` lost its unused agent
  argument.
- The mailbox no longer attributes agent replies to the channel's single agent. `channel_agent()`
  is gone, along with the `agent` parameter of `render_message_pane`, `MessagePane` and
  `message_bubble_chat`, and the unused `AgentUseCases` extractors in the mailbox and
  internal-note routes. A reply shows the channel address it was stored with. Four
  `#[expect(clippy::too_many_arguments)]` became unfulfilled and were removed.
- **Pages.** Removed:
  - every agent picker: classic `render_agents_selection*` with its JS, and the workspace
    radios, library modal and `syncChannelAgents`/`pickChannelLibraryAgent` JS;
  - the owner and readonly-address handling, the "Agents" field and the response-trigger select;
  - the save notice (`ChannelEditPane.notice`);
  - the Agents workspace Channel tab (`AgentTab`, `AgentPaneBody`, `AgentChannelTab`, its route
    `PUT /ui/agents/{id}/channel`), "Run by" and the owned-channel delete warning (now "This cannot
    be undone.");
  - the sidebar address, now `@slug`;
  - the company default-participants field.
  - The classic channel row also lost its legacy "Configured on active agent" grid and its
    **"Task Executions" link to the task pages deleted in B4a**. The test now asserts the link
    is gone.

**Schema** (init migration edited in place; `workflow08_test` recreated)
- 28 segments dropped:
  - `channel_agents` and `agent_sub_agents` with their PKs, unique keys, indexes and FKs;
  - `channels_owner_agent_key`, `channels_owner_agent_fk`, `channels_preferred_reviewer_fk`;
  - the triggers `channel_agents_scope_check`, `owned_channel_assignment_position_zero_check`,
    `owned_channel_position_zero_check`, `owned_channel_delete_guard`, `library_agent_delete_guard`
    and `background_tasks_initialize_ownership`;
  - their functions `enforce_channel_agent_scope`, `enforce_owned_channel_position_zero`,
    `prevent_assigned_library_agent_delete`, `prevent_owned_channel_delete` and
    `initialize_task_ownership` (DV-B4c2-3);
  - the comment on `channels.external_reply_handling_override`.
- 20 lines dropped:
  - channels: 5 columns and 3 checks;
  - companies: `external_response_review`, `external_reply_handling`,
    `default_participant_emails` and 3 checks;
  - agents: `config_json`, `run_timeout_secs`, `response_contract` and 3 checks.
  - `channels` lost its last line, so the trailing comma before `);` was repaired.
- Applied cleanly to a scratch database. The catalog check (`pg_proc.prosrc` for every dropped
  table, column and function name, plus `memory_`/`hydradb`) found nothing.
- DV-B4c2-3 fixtures: `job_core_query_tests` and `job_routine_tests` no longer insert
  `channel_agents`. Their legacy rows name the company's agent principal as owner explicitly; the
  routine test's owner-release assertion still holds. B4c-3 retires these suites.

**Tests**
- New:
  - `channel_edit::tests::an_edit_waits_for_an_admission_holding_the_gate_then_applies`: a
    competing claimant. Admission holds the gate shared, the edit is still pending after 300 ms,
    the admission commits, and the edit applies.
  - `an_edit_of_another_companys_channel_finds_nothing`.
  - `a_failed_edit_rolls_back_every_setting_with_it` and the lock-timeout test, both adapted.
  - `agent_config_tests::retired_agent_and_channel_runtime_columns_and_tables_are_gone`: 14
    columns, 5 tables and 5 functions, replacing that file's config tests.
  - `thread::tests::a_hop_to_a_plain_channel_is_relayed` (DV-B4c2-1).
  - `api_agents_carry_no_retired_runtime_fields` and `retired_agent_fields_are_unknown`
    (`deny_unknown_fields`).
  - `a_native_urlencoded_submission_builds_the_write`.
- Rewritten: the two bubble-attribution tests now cover the address and the other-channel mark,
  and the company save round trip no longer has policy columns.
- Removed because their subject is gone:
  - assignment, owner and channel-tab tests (pages, persistence, use cases, integration);
  - the response-trigger tests;
  - the config, contract, timeout and sub-agent tests (routes, use cases, persistence,
    `constraint_tests`, `skill_tests`);
  - the default-participant and spam-precondition tests;
  - the library-modal test.

**Checks** (`DATABASE_URL=postgres://workflow08@127.0.0.1:55480/workflow08 SQLX_OFFLINE=true CARGO_INCREMENTAL=0`)
- `cargo check --all-targets`, clippy `-D warnings` and fmt: clean (`logs/b4c2-clippy.log`,
  `logs/b4c2-fmt.log`).
- `.sqlx` macro-bound check: 23 sites, 23 files, none stale or missing (`logs/b4c2-sqlx-check.log`).
- Dead-code scans: no new uncalled function or type. `list_assignable_agents` was found and
  deleted; no new uncalled trait method.
- Focused tests:
  - `logs/b4c2-focused-1.log`: 769 passed, 7 failed. All seven were fixtures or asserts naming
    removed objects, and all were fixed.
  - `logs/b4c2-focused-2.log`: 773 passed.
  - **`logs/b4c2-focused-3.log`: 833 passed**, 0 failed, 3 ignored. Filters:
    - `adapters::http`; persistence channel, agent, company and tests; `constraint_tests`;
    - `provisioning`; use cases agent, channel, company and thread;
    - `message_ingress`, `harness_owner`, `job_`, `authorization_cases`;
    - `response_schema`, `skill`, `persistence::integration`, `domain::entities`, `smtp`,
      `infra::`;
    - `agent_model_step`, `agent_recovery`, `workflow::agent`;
    - `channel_gate`, `inbound`, `transport`, `egress`.
  - The new relay test also ran on its own.
- CI node tests (`sse-navigation.mjs`, `agent-runtime.mjs`): 7 passed (`logs/b4c2-node.log`).
- `scripts/tests/transport-boundary.sh` and `scripts/transport-boundary-check.sh`: passed.
- Not run: `cargo sqlx prepare --check` (no sqlx-cli), `npm run check:css` (end), the full suite.

**Left for B4d** (docs):
- README feature copy (e.g. manual handoff / `external_reply_handling`);
- the other stale `docs/rig.md` passages (task-backed execution, review drafts);
- the agent create pane's address preview, which still shows `handle@company` even though agents
  have no address;
- the `#[serde(default)]` "re-hydrate" notes on `Channel` fields (legacy payload compatibility).

Manifests: `logs/b4c2-filehashes.txt`; the delta vs the B4c-1 freeze is `logs/b4c2-delta.txt`
(53 changed, 11 first touched, 8 deleted).

Frozen for review: digest `97531db8994a61e52cb4fe227c5fc2961b7873690e87ce6304a6ae047340802d`
(base/HEAD `da86871` plus the working tree and index; same exclusions; `logs/b4c2-snapshot.txt`).
Coordination r10 (`1df326af…`) unchanged. Review: WF-P08-R026.

### B4c-2 corrections for WF-P08-R026 (2026-10-24)

R026 at `97531db8…802d`: **CHANGES REQUIRED**.
- WF-P08-F097 (Medium): the channel gate doc claimed the inbound commit re-reads eligibility
  under the gate; it did not, and the competing test held a bare gate rather than an admission.
- WF-P08-F098 (Info, 6 items).
- Accepted by B: DV-B4c2-1, DV-B4c2-3, DV-B4c2-4. DV-B4c2-2 accepted for the edit itself.

**F097: B's option (a), implemented** (DV-B4c2-5). The commit now refuses a message routed
against settings an edit has since replaced, instead of storing it under a mix:
- Schema (init migration, in place):
  - `channels.settings_revision bigint DEFAULT 0 NOT NULL`;
  - `'superseded'` added to `valid_inbound_event_error_class`.
- `write_channel_settings_on` moves the revision on with the settings. It is the only post-create
  writer of the channel's settings, slugs and allowlist grants.
- `Channel.settings_revision` is read in `CHANNEL_SELECT`, the one statement that also reads the
  slugs and grants routing decides on.
- `InboundAdmissionIntent.settings_revision`: `CommitPlan` copies it from the routed candidate's
  channel.
- `persistence/thread/inbound.rs`:
  - New `hold_channel_settings` (`pub(super)`): the shared gates, then `verify_channel_settings`,
    a statement of its own that refuses as `AppError::Superseded` when any intent channel is
    missing or at another revision. `commit_on` calls it first and is now `pub(super)` for the
    tests.
  - Module doc: the gate rule added as the first of three concurrency rules.
- `AppError::Superseded` (new): transient, nothing applied, replan.
  - HTTP maps it to 409, like Conflict.
  - SMTP answers 451, the existing non-BadRequest default, so the sender's retry routes the
    message again.
  - The event worker classifies it as `InboundEventErrorClass::Superseded` (new, retryable) and
    retries; decoding again runs preflight again.
- Docs rewritten: the `channel_gate` module doc (the actual protocol, and why the check is its
  own statement under READ COMMITTED); `channel_edit` step 3.
- Scope boundary, unchanged by this round: deleting a principal cascades its grants without
  moving the revision. That is a membership change, not a settings edit, and the gate never
  claimed it.

**Tests** (`thread/inbound_tests.rs`, real commit, both orders, F083's D4.8 test):
- `a_commit_queued_behind_a_settings_edit_is_refused_with_nothing_stored`: an edit holds the gate,
  `commit_inbound` waits, the edit commits, and the commit returns `Superseded` with 0 messages,
  0 intents and the channel at `(false, 1)`.
- `a_settings_edit_queued_behind_a_commit_applies_after_it`: the commit runs
  `hold_channel_settings` (no row written yet, the window where only the gate protects), and the
  edit waits. The rest of `commit_on` and the commit follow; then the edit applies. 1 message,
  1 intent, `(false, 1)`.
- Removed `channel_edit_tests::an_edit_waits_for_an_admission_holding_the_gate_then_applies` (the
  bare-gate holder B flagged). The edit's timeout test keeps a bare holder; it tests the budget,
  not the protocol.
- Unit tests: the intent carries the revision (`commit.rs`); SMTP 451 for `Superseded`
  (`smtp/server.rs` answer test); `Superseded` retries and `Conflict` stays terminal
  (`inbound_event_worker_tests.rs`); `Superseded` is a retryable class (`transport/inbound.rs`).
- Mutation checks:
  - Without the revision check, the edit-first test fails.
  - Without the gate in `commit_on` (first form), the edit-first test fails, but the commit-first
    test passed: the foreign-key locks from the commit's inserts also block the edit's
    `FOR UPDATE`. So the commit-first test now pauses after `hold_channel_settings`.
  - Without the gate in `hold_channel_settings`, both tests fail.
  - Every mutation was restored.
- Fixture note: the thread fixture's channel is created disabled, so the tests switch it on with
  direct SQL that leaves the revision alone. A disable edit is then a real change.

**F098 dispositions:**
1. Mailbox avatar always `None`: kept as is. Attributing workflow replies to the authoring
   principal is p09.
2. The agent pane's "used by" list: p09, as binding usage.
3. The agent create pane's address preview: **fixed now**, as B suggested. It previews `@handle`,
   the form the list and edit pane use, with no `data-address-suffix`. The JS prefixes `@`. The
   `pages/tests.rs` assertion is inverted.
4. `get_by_company_slug_and_agent_slug` has no caller: B4d exhaustive search.
5. `LIMIT 1` owner fixtures: deleted or rewritten with the legacy fixtures in B4c-3.
6. Slug conflict on agent update is a 409: noted, no change.

**B4c-3 scope change:** the F083 `channel_gate` competing test is done here. B4c-3 keeps the D4.8
catalog test.

**Checks** (cluster 55480; `workflow08_test` recreated after the schema edit):
- clippy `--locked --all-targets -D warnings` clean (`logs/f097-clippy.log`); fmt clean
  (`logs/f097-fmt.log`).
- Focused F097 set: 423 passed (`logs/f097-focused.log`).
- Wide set (the B4c-2 filters plus the inbound, event-worker, Resend and `app_error` filters):
  **881 passed**, 0 failed, 3 ignored (`logs/f097-focused-wide.log`).
- Node: 7 passed (`logs/f097-node.log`). Both transport-boundary scripts passed
  (`logs/f097-boundary.log`).
- `.sqlx` macro check: 23/23, none stale or missing; no macro text changed.
- Not run: `cargo sqlx prepare --check` (no sqlx-cli), `npm run check:css`, the full suite, and
  `stack-budget.sh` (end of phase).

**Frozen:** digest `2d4ea381960e8134768631a06eacc425442310dd0ed06c7a4723ba880e8a7098`
(base/HEAD `da86871` plus the working tree and index; same exclusions;
`logs/b4c2-snapshot-r2.txt`).
- Correction delta: 24 files (`logs/b4c2-r2-delta.txt`), 3 of them first touched in this round.
- Hashes for the next delta: `logs/b4c2-filehashes-r2.txt`.
- Inventory for B4c-3, verified read-only: `logs/b4c3-inventory-verified.md`.

### B4c-3 work order (2026-10-24; starts after B4c-2 acceptance)

Input: `logs/b4c3-inventory-verified.md`, a read-only check of `logs/b4c-inventory.md` against the
post-B4c-2 tree. Every line number is re-grepped before an edit.

**Split.** B4c-3 is too large for one thorough review, and the runtime-metrics retarget is a
feature change rather than a removal, so it splits in two. Each part gets one batch review:

| Part | Scope |
| --- | --- |
| **B4c-3a** Legacy runtime schema | The 24 legacy tables and the 25 legacy functions with their triggers. The `background_tasks` trims (15 columns, checks, constraints, FKs, indexes and triggers, including the principals release triggers). The `task_attempts` trims. The `task_harness_runs` owner trims. `message_deliveries.task_id` and the two cancellation reasons. The notification source and action checks, and the 4 preference flags. Rewrites of `enqueue_actionable_notification_event`, `notify_attention_changed`, `require_harness_owner` and `guard_model_usage_task`. The enums `TaskStatus`, notification source/action kinds, `DeliveryCancellationReason::{ChannelAgentRemoved,TaskStopped}` and `AttentionWakeSource`. The dashboard SQL literals, and the `thread/mod.rs` outreach clause. The legacy fixtures and tests, deleted or rewritten as the inventory's §Tests lists them. The D4.8 catalog test. |
| **B4c-3b** Runtime metrics retarget | `runtime_metric_samples.active_task_executions`, `task_worker_concurrency_limit` and their checks, replaced with active workflow jobs and worker slots (source `WORKFLOW_WORKER_SLOTS`). The sampler, `TaskWorkerConcurrencyRecommendation`, `ActiveTaskExecutions`, `TASK_WORKER_CONCURRENCY`, the dashboard runtime panel, and their tests. The config and docs entries move to B4d's docs pass unless code needs them. |

**Decisions for B4c-3a.** Each is verified with evidence while it is applied:
- **DV-B4c3-1.** `background_tasks.status` keeps `pending`, `processing`, `completed` and
  `failed`.
  - `dead_letter`, `pending_approval`, `waiting_for_third_party_reply` and `stopped` go, with
    `TaskStatus`'s legacy variants and the dashboard `dead_letter` filters.
  - A failed workflow job stays `failed`. The dashboard keeps its existing meaning for `failed`
    and gains no new "needs attention" rule.
  - `queue_kind` keeps only `'workflow'`, which becomes its default.
- **DV-B4c3-2.** `task_harness_runs.owner_kind` is kept, constrained to `'workflow'` with
  `'workflow'` as its default. Dropping the column would touch 6 Rust sites and three partial
  predicates for no behavioural gain.
- **DV-B4c3-3.** `background_tasks_unsettled_channel_idx` is dropped if, after the drops, no query
  matches its predicate.
- **DV-B4c3-4.** `task_attempts`:
  - drops `prompt_tokens`, `completion_tokens`, `result`, `error` and `token_check` (no
    production writer);
  - `stop_reason` becomes `completed`, `effect_parked`, `child_parked`, `approval_parked`,
    `lease_lost`, `timed_out` or NULL.
- **DV-B4c3-5.** Out of scope, recorded for B4d's exhaustive search. D4.8 does not name them, and
  none is task-runtime schema proper:
  - the `message_deliveries` purposes `reply`, `mirror` and `outreach`, which have no production
    constructor;
  - `depends_on_delivery_id`;
  - the `agent`/`source_task_id` arm of `valid_creation_provenance`;
  - the optional dead-branch cleanups (`guard_harness_model_owner`, `guard_harness_token_pause`,
    `advance_workflow_owned_revision`);
  - the `model_usage_calls` `num_nonnulls IN (0,5)` check.
- F083's competing test is already done in B4c-2 (DV-B4c2-5).

**Focused checks for B4c-3a** (after the DB is recreated):
- clippy `-D warnings` and fmt;
- `adapters::persistence::tests`, the catalog test, and `persistence::workflow::` (`job_`,
  `harness_`, `schedule_runtime`, `message_ingress`, `completion_refusal`,
  `decision_provenance`);
- `delivery::`, `notification`, `attention`, `dashboard`, `infra::events`, `live_updates`,
  `ui_stream`, `adapters::http::pages`, `adapters::persistence::thread::`, `model_usage`;
- the `.sqlx` macro check.

**Focused checks for B4c-3b:** `runtime_metrics` (persistence, infra, services), `infra::config`,
`dashboard_runtime`, `adapters::persistence::tests`.

### B4c-2 correction for WF-P08-R027 (2026-10-24)

R027 at `2d4ea381…7098`: F097 fixed, and B accepts the protocol (DV-B4c2-5). F098 is resolved.
Two new findings:
- **WF-P08-F099 (Low): fixed.** The commit-first gate test spawned its edit with
  `EditBudget::DEFAULT`, whose 2 s `lock_timeout` also bounds the advisory-lock wait, so a loaded
  run could time out. The spawned edit now gets a 30 s `lock_timeout`, with a comment saying
  why. The edit's own lock-timeout refusal is already covered in `channel_edit_tests`.
- **WF-P08-F100 (Info): recorded.** Three routing inputs sit outside the settings revision, and
  D4.8 never claimed any of them:
  - **Binding status.** It is re-checked in the commit, but not under the gate, and a non-key
    UPDATE does not conflict with the FK KEY SHARE lock. A revoke can therefore land between that
    check and the commit.
  - **Membership.** Removing a member changes the access context, not the settings.
  - **Principal deletion.** Already serialized by the FKs.

  B's optional suggestion, a database round-trip test of every inbound error class, was not
  taken. Binding status under the gate is a candidate for p09 or B4d; no change in this batch.

Checks:
- fmt and clippy clean (`logs/f099-fmt.log`, `logs/f099-clippy.log`).
- The two gate tests plus `adapters::persistence::channel_edit`: 5 passed. The two gate tests
  were then rerun 5 times, all passing (`logs/f099-focused.log`).

Frozen: digest `95fe39183361bb49316fa8cb1181a5cedf45fa0834bfff5e694e4c094baacbe5`
(`logs/b4c2-snapshot-r3.txt`; same base and exclusions). Delta against R027: only
`src/adapters/persistence/thread/inbound_tests.rs` (`logs/b4c2-filehashes-r3.txt`).

### B4c-2 acceptance (2026-10-24)

**B4c-2 is accepted in phase.** It is not integrated and not committed.
- B's R028 explicitly PASSed digest `95fe39183361bb49316fa8cb1181a5cedf45fa0834bfff5e694e4c094baacbe5`, and root recomputed the same digest (same base `da86871`, same exclusions).
- Coordination r10 (`1df326af…d9ae`) is unchanged.
- Review rounds: R026 (CHANGES REQUIRED: F097 Medium, F098 Info), R027 (F099 Low, F100 Info) and R028 (PASS).
- Findings: F095 and F096.1 fixed in step 0; F097 and F099 fixed; F098 and F100 resolved.
- Carried forward: F094 (Low, phase gate), F096.2–4 (notes; .4 to B4d), and F098.4 plus F100's binding-status note (B4d/p09).
- Accepted decisions: DV-B4c2-1..5.
- Per-file hashes for B4c-3a's delta: `logs/b4c2-filehashes-r3.txt`.
- End-of-phase checks not yet run: `cargo sqlx prepare --check` (no sqlx-cli), `npm run check:css`, the full suite and `stack-budget.sh`.

Next: B4c-3a (TASK "B4c-3 work order").

### B4c-3a progress (2026-10-24)

Implemented per the work order. Inputs: `logs/b4c3-inventory-verified.md`; every item re-checked
against the tree before editing.

**Init migration** (edited in place; no backward compatibility):
- Segment drops by script (`scratchpad/b4c3/{migseg,plan,apply}.py`, which asserts exact counts;
  the pre-edit copy is kept in the scratchpad). 263 pg_dump segments (3107 lines) go:
  - the 24 legacy tables with all their constraints, indexes, triggers, FKs and comments;
  - the 25 legacy functions, and every trigger that executes one;
  - the FKs from `background_tasks` into dropped tables;
  - the named `background_tasks` objects: the source-message unique key and its comment, the
    owner-principal and source-message FKs, the four legacy indexes, `unsettled_channel_idx`
    (DV-B4c3-3), and the `notify_activity`, `notify_attention` and `notify_attention_delete`
    triggers, whose WHEN clauses name `'legacy'`;
  - `task_harness_runs`: the owner-principal FK and the `task_harness_runs_scope` index;
  - `message_deliveries`: `task_fk` and `task_idx`.
- Hand-written SQL can sit inside a pg_dump segment. The script checks that every dropped segment
  holds one object. One segment did not: the `task_harness_runs require_legacy_auxiliary_task`
  header also holds the live `guard_harness_model_owner` and `require_harness_owner` triggers.
  It is kept, its header is renamed `task_harness_runs require_harness_owner`, and only the
  `task_harness_invocations` trigger statement is cut out of it.
- Text edits:
  - `background_tasks` keeps the workflow columns only. `queue_kind` defaults to `'workflow'`
    and its check allows only that. `status` allows `pending`, `processing`, `completed` and
    `failed`. The continuation check is simplified, and `workflow_shape` now requires the
    execution, the task type and the payload.
  - `task_attempts`: `error`, `prompt_tokens`, `completion_tokens`, `result` and `token_check`
    are dropped, and `stop_reason` takes six values (DV-B4c3-4).
  - `task_harness_runs`: `owner_kind` defaults to `'workflow'`, `owner_principal_id`,
    `ownership_version` and its check are dropped, and `owner_shape` keeps only the workflow arm
    (DV-B4c3-2).
  - `require_harness_owner`: the ROW comparison loses the dropped columns, and the legacy branch
    and its comment go.
  - `message_deliveries`: `task_id` goes, along with its term in the attribution check and the
    `channel_agent_removed` and `task_stopped` cancellation reasons.
  - `notification_events` and `notifications`: the source must be `delivery` and the action
    `delivery_failure`.
  - `user_notification_preferences` loses its 4 retired flags.
  - `enqueue_actionable_notification_event` loses the legacy preservation block.
  - `notify_attention_changed` keeps the delivery path only; its sole caller is now
    `message_deliveries_notify_attention`.
  - `guard_model_usage_task` drops the legacy arm, so it requires a run and an exact execution.
- Verified:
  - Applied to a fresh scratch DB `workflow08_scratch`: clean (`logs/b4c3a-scratch-apply.log`).
  - Catalog: no function, constraint, index, trigger or view mentions any dropped table, column
    or function, or the literal `'legacy'`. No dropped table or column remains; 107 tables
    (`logs/b4c3a-catalog.log`).
  - `workflow08_test` recreated.

**Rust:**
- `TaskStatus` keeps `Pending`, `Processing`, `Completed` and `Failed` (DV-B4c3-1).
- Dashboard:
  - The outstanding list shows `processing` jobs and due `pending` jobs, stalled first.
  - Throughput counts `failed` alone, and queue depth counts `pending` and `processing`.
  - The job panel loses its `Dead-lettered` stat, which no workflow job can reach.
  - `needs_attention` is true only for a stalled job.
  - The legacy state labels and icons are gone.
- `NotificationSourceKind` is reduced to `Delivery` and `NotificationActionKind` to
  `DeliveryFailure` (D4.3). With them go the always-`None` legacy arm of `source_snapshot` and
  the `/ui/work` arm of `source_href`.
- `DeliveryCancellationReason` loses `ChannelAgentRemoved` and `TaskStopped`, and their
  `cancellation_detail` arms.
- `AttentionWakeSource` is reduced to `Delivery`.
- `find_outbound_reply_after` loses its `task_outreach_targets` clause.
- Comments that named dropped tables are reworded (`persistence/mod.rs`, `entities/message.rs`),
  and a stale "hold gate" banner at the end of `thread/inbound_tests.rs` is removed.
- Out of scope (DV-B4c3-5): the `model_usage/mod.rs` LEFT JOINs stay, and the guard now refuses
  a run-less row.

**Tests:**
- Deleted, since each existed only to test the legacy schema:
  - `persistence/constraint_tests.rs`;
  - `workflow/job_routine_tests.rs` and its children `job_source_association_tests`,
    `job_auxiliary_association_tests`, `job_auxiliary_fixtures`, `job_handoff_history_tests`;
  - `job_notification_fixtures.rs`;
  - `job_schema_tests::workflow_job_schema_does_not_create_legacy_ownership_or_status_events`.
- Rewritten, keeping their purpose (DV-B4c3-7):
  - `job_schema_tests`:
    - `..._keeps_the_channel_nullable_for_company_associations` drops the legacy-guard
      assertion.
    - `..._rejects_foreign_execution_and_a_non_workflow_job` now uses an exactly scoped row
      whose task type alone is wrong, so the shape check is what fires.
    - The queue-identity immutability check now uses `'other'`.
  - `job_association_tests`: a job with no execution is refused by
    `background_tasks_workflow_execution_fk`. With `queue_kind` defaulting to `workflow`, the
    association trigger fires before the CHECK.
  - `job_core_query_tests::scoped_fixture` has no legacy row. `job_projection_tests` is now
    `..._counts_every_workflow_job`, with one 90 s retry attempt and no token columns.
  - `completion_refusal_tests`: the legacy half becomes "a claimed fence naming a job the queue
    does not hold refuses".
  - `harness_owner_tests`: `..._exact_scope_and_workflow_owner_only`. The dropped-column cases
    are removed, since `jsonb_populate_record` ignores unknown keys; `owner_kind 'legacy'` is
    still refused.
  - `thread/tests.rs`: `find_outbound_reply_after_sees_answers_and_ignores_relayed_mail`, with
    the outreach bookkeeping removed. A delegation (relay) entry is still not the answer.
  - `notification_tests` backoff test: events are `delivery`/`delivery_failure`.
  - `delivery/cancellation_tests`: the rewrite attempt uses `'workflow_expired'`, so only the
    immutability trigger can refuse it.
  - `persistence/mod.rs` spine test: `background_tasks.source_message_uuid` moves to the retired
    columns, leaving the thread-message spine.
  - `action_reconciliation_claim_sql_candidate_{tests,support}`: the hidden-job fixture uses
    `completed` instead of the legacy `stopped` (DV-B4c3-6). That transition is unguarded from
    `pending`, and so is the restore.
  - `schedule_runtime_tests` and `message_ingress_tests`: the `queue_kind = 'legacy'` counts are
    removed, since the CHECK makes them tautological.
  - Enum tests: `task.rs`, `notification.rs` (new `only_the_delivery_source_and_action_parse`),
    the `events.rs` wake-source payloads (delivery parses, the five retired kinds are refused),
    the `live_updates` and `ui_stream` fixtures, and the `pages/notifications` and dashboard
    page tests.
- New `persistence/legacy_runtime_schema_tests.rs`, the D4.8 catalog test:
  - `the_legacy_runtime_tables_columns_and_functions_are_gone`: 26 tables, the retired column
    names, plus `message_deliveries.task_id` and `task_attempts.result`/`error`, and 25
    functions.
  - `no_catalog_object_names_the_legacy_runtime`: functions, constraints, indexes, triggers and
    views are scanned for the retired names and for `'legacy'`. The test checks itself by
    planting a PL/pgSQL function that reads `task_outreaches` in a rolled-back transaction and
    asserting the scan reports exactly that.

**Checks** (cluster 55480):
- clippy `--locked --all-targets -D warnings` clean (`logs/b4c3a-clippy.log`); fmt clean.
- Focused run 1 (persistence, pages, live updates, `ui_stream`, events, entities, services):
  1443 passed, 8 failed (`logs/b4c3a-focused-1.log`). All 8 were the expected fixture and
  assert fallout listed above, and all are fixed.
- Focused run 2 (`adapters::persistence::`, `adapters::http::`, `infra::`, `domain::entities`,
  `application::services`, `application::use_cases`, `application::transport`,
  `model_usage`): **1807 passed**, 0 failed, 1 ignored (`logs/b4c3a-focused-2.log`).
- The fixed tests alone: 45 passed.
- Node: 7 passed (`logs/b4c3a-node.log`). Both transport-boundary scripts passed
  (`logs/b4c3a-boundary.log`).
- `.sqlx` macro check: 23/23, none stale or missing.
- Not run: `cargo sqlx prepare --check` (no sqlx-cli), `npm run check:css`, the full suite and
  `stack-budget.sh` (end of phase).

**Left for later:**
- B4c-3b: runtime metrics, including the `main.rs:193` comment.
- B4d:
  - the `src/adapters/persistence/AGENTS.md` passages that name the dropped tables;
  - the DV-B4c3-5 list;
  - `Icon` variants that may now be unused, to be checked in B4d's exhaustive search (clippy
    reports nothing).

**Frozen for review:** digest
`fad11e2751bc298f501dbcbd1b6b8049c292ecfcf5d12eec25941eb75f1c1dde` (base/HEAD `da86871` plus the
working tree and index; same exclusions; `logs/b4c3a-snapshot.txt`).
- Delta against the accepted B4c-2 hashes: 38 files, of which 23 changed, 7 deleted and 8 first
  touched (`logs/b4c3a-delta.txt`).
- Hashes for the next delta: `logs/b4c3a-filehashes.txt`.
- Coordination r10 is unchanged.
- Review: WF-P08-R029.

### B4c-3a acceptance (2026-10-24)

**B4c-3a accepted in phase** (not integrated, not committed). B's R029 gave an explicit PASS at
digest `fad11e2751bc298f501dbcbd1b6b8049c292ecfcf5d12eec25941eb75f1c1dde`; root recomputed the
same digest (same base and exclusions). Coordination r10 (`1df326af…d9ae`) is unchanged.
- One round. B accepted DV-B4c3-1..7.
- B's evidence:
  - a statement-level diff of the migration (`logs/r029-migration-diff.txt`): 275 statements
    removed, all belonging to dropped objects, and 11 rewritten;
  - a catalog scan that includes the retired values;
  - 2033 focused tests passed, with `application::workflow` included.
- **F101 (Info, optional) dispositions:**
  - .1 Thin evidence logs: from B4c-3b on, the full scan output goes into the logs.
  - .2 The catalog test should read `pg_get_functiondef` (for SQL-standard bodies) and scan
    for the retired values: **taken into B4c-3b** step 0, to be reviewed with it.
  - .3 Unused `Icon::People` and `Icon::Goal`: B4d sweep.
  - .4 Narrowing `model_usage_calls` `num_nonnulls` to `= 5`: with DV-B4c3-5 in B4d.
  - .5 Dashboard meaning: noted. A failed job's run surfaces in attention as `run_failure`.
- Per-file hashes for B4c-3b's delta: `logs/b4c3a-filehashes.txt`.

Next: B4c-3b (runtime metrics retarget), F101.2 first.

### B4c-3b progress (2026-10-24)

**Step 0 (F101.2):** the D4.8 catalog scan in `persistence/legacy_runtime_schema_tests.rs` now:
- reads `pg_get_functiondef` for `prokind IN ('f','p')` instead of `prosrc`, which is empty for
  a SQL-standard body;
- also scans for 14 distinctive retired stored values (`RETIRED_VALUES`, including `'legacy'`).
  Generic values that are live elsewhere (`stopped`, `dead_letter`, `task`) are left to the
  narrowed checks.

The self-check now plants two canaries in a rolled-back transaction: a PL/pgSQL function that
names `task_outreaches`, and a SQL-standard function that returns `'pending_approval'`. It
asserts that the scan reports exactly those two.

**Runtime metrics retarget (D4.8):**
- **Schema** (init migration, edited in place):
  - `runtime_metric_samples.active_task_executions` → `active_workflow_jobs`;
  - `task_worker_concurrency_limit` → `workflow_worker_slots`;
  - the checks are renamed `_active_jobs_nonnegative`, `_active_jobs_within_slots` and
    `_worker_slots_positive`.
  - Scratch apply is clean (`logs/b4c3b-scratch-apply.log`, 980 lines, no errors). The full
    catalog output for the table is in `logs/b4c3b-catalog.log`; no function definition names the
    old columns.
- **Renames:**
  - `ActiveTaskExecutions`/`ActiveTaskExecutionGuard` → `ActiveWorkflowJobs`/
    `ActiveWorkflowJobGuard`;
  - `TaskWorkerConcurrencyRecommendation` → `WorkflowWorkerSlotsRecommendation`;
  - `TaskWorkerRecommendationPolicy`/`TASK_WORKER_RECOMMENDATION_POLICY` →
    `WorkflowSlotsRecommendationPolicy`/`WORKFLOW_SLOTS_RECOMMENDATION_POLICY` (thresholds
    unchanged);
  - `suggested_task_worker_concurrency` → `suggested_workflow_worker_slots`;
  - the observation and sample fields are renamed to match.
- **The gauge:**
  - `WorkflowWorker` gains `active_jobs: Option<&ActiveWorkflowJobs>`. `process` enters it on
    the `PollWork::Job` path only, after the non-job early returns, and holds it across the job
    until it drops.
  - `WorkflowEngine` owns one gauge, passes it to every spawned slot, and exposes `slots()` and
    `active_jobs()`. `main.rs` builds the `LinuxRuntimeMetricSource` from them.
  - The gauge was never incremented before: `ActiveTaskExecutions::default()` was passed to the
    sampler only.
  - The other 11 `WorkflowWorker` literals in tests get `active_jobs: None`; three more already
    used a struct spread.
- **Config:**
  - `TASK_WORKER_CONCURRENCY` is gone: `task_worker_concurrency_from_env`, its parse function,
    constants and 2 tests.
  - Its `.env.example` and `fly.toml` entries, and both `docs/deploy.md` rows, are removed.
  - README §3.4's `TaskWorker` bullet (which named it) becomes an accurate "Workflow Worker
    Slots" bullet. Its claims are checked against the code: 1 s poll, 30 s lease, admission
    wakeups, 24 h recommendation window.
  - The rest of README §3.4 and the stack-size row in `docs/deploy.md` (which names the old
    chain) stay with B4d.
- **Panel:** "Workflow worker capacity", with "Active jobs" and "Configured slots"
  (`WORKFLOW_WORKER_SLOTS`), and the suggested maximum given as "safe observed
  WORKFLOW_WORKER_SLOTS".

**Tests:**
- `polling_tests::workflow_poll_shutdown_drops_actual_future_and_leaves_recoverable_lease`
  passes a gauge. It asserts 0 before the job, 1 while the handler runs, and 0 after shutdown
  drops the future.
- Mutation check: without the `enter`, the test fails with "the slot is executing a job" (file
  restored).
- The renamed runtime-metrics, panel and recommendation tests pass unchanged in meaning.

**Checks** (cluster 55480; `workflow08_test` recreated):
- clippy `-D warnings` and fmt clean (`logs/b4c3b-clippy.log`, `logs/b4c3b-fmt.log`).
- Focused run 1 (runtime metrics, panel, `infra::`, polling, catalog, schema, dashboard pages):
  79 passed (`logs/b4c3b-focused-1.log`).
- Broad focused run (`adapters::persistence::`, `adapters::http::`, `infra::`,
  `domain::entities`, `application::services`, `application::use_cases`,
  `application::transport`, `application::workflow`, `model_usage`): **2031 passed**, 0 failed,
  1 ignored (`logs/b4c3b-focused-2.log`). That is R029's 2033 minus the 2 deleted config tests.
- Node: 7 passed. Both transport-boundary scripts passed.
- `.sqlx` macro check: 23/23.
- Not run: `cargo sqlx prepare --check`, `npm run check:css`, the full suite and
  `stack-budget.sh` (end of phase).

**Frozen for review:** digest
`845d5596b5945223b27143dc847eda42cc5cd620a84edc4d5262b9e39793e1d6` (same base and exclusions;
`logs/b4c3b-snapshot.txt`).
- Delta against the accepted B4c-3a hashes: 22 files, of which 15 changed and 7 first touched
  (`logs/b4c3b-delta.txt`).
- Hashes for the next delta: `logs/b4c3b-filehashes.txt`.
- Coordination r10 is unchanged.
- Review: WF-P08-R030.

### B4d work order (drafted 2026-10-24; starts after B4c-3b acceptance)

B4d closes the phase. It is one batch, with one review, followed by the phase gate.

1. **F094 (Low, required before the full suite).** The lease-resolution stall tests
   (`lease_resolution_tests.rs`, the exact lower bounds near `:322` and `:398`):
   - print `elapsed` in both assertions;
   - take B's option (a): tolerate the server's millisecond granularity (`elapsed + 1 ms >=
     T`), with a comment on the floored `lock_timeout`/`statement_timeout`.

   The runtime is unchanged: "unknown" is still the right classification.
2. **Exhaustive caller and leftover search** (plan acceptance: "Exhaustive caller searches
   identify no remaining legacy execution entry points"):
   - Rerun the HEAD-relative dead-code scans (`uncalled.py`, `uncalled_trait.py`, and the
     type/const scan) over the whole tree, and remove what the removal batches orphaned. Known so
     far: `AgentPersistence::get_by_company_slug_and_agent_slug` (F098.4), `Icon::People` and
     `Icon::Goal` (F101.3).
   - Grep for entry points: the deleted route prefixes, `task_type` values other than
     `workflow_execution`, harness selection, and channel execution settings.
   - Decide each DV-B4c3-5 item on evidence:
     - the delivery purposes `reply`, `mirror` and `outreach`, which have no production
       constructor;
     - `message_deliveries.depends_on_delivery_id`, which only `test_support` sets;
     - the `valid_creation_provenance` agent and `source_task_id` arm;
     - the dead `owner_kind <> 'workflow'` branches in `guard_harness_model_owner`,
       `guard_harness_token_pause` and `advance_workflow_owned_revision`;
     - `model_usage_calls` `num_nonnulls IN (0,5)` → `= 5`, with inner joins in `reserve_on`.
     A schema edit here recreates `workflow08_test` and reruns the catalog test.
   - Remove the `#[serde(default)]` "re-hydrate legacy `background_tasks` payloads" notes on the
     `Channel` fields, which are now stale (B4c-2 leftover).
3. **Docs and guides:**
   - README §3.4 "Durable Task Store", and any other legacy feature copy (manual handoff,
     `external_reply_handling`);
   - the stale `docs/rig.md` passages (task-backed execution, review drafts);
   - `docs/transport_architecture.md` ×2;
   - `docs/deploy.md`: the stack-size row that names the task-worker chain, and `:485`
     `ERROR_BACKOFF` in `task_worker.rs` (F096.4);
   - the subsystem guides `src/AGENTS.md`, `src/adapters/persistence/AGENTS.md` and
     `src/application/AGENTS.md` (F093.5);
   - every config key named in docs or examples must still be read.
4. **Records:**
   - F093.2: add one line to DV-B4b-1 noting the open-decision reviewer-removal limitation, as a
     p09 follow-up.
   - F100: record binding status under the gate as a p09 candidate.
   - Note: plan files 05–09 do not carry the no-backward-compatibility note themselves; the
     plan README does. This is plan-wide, so it goes to the coordinator as a candidate, not an
     edit of other phases' files.
5. **F073 (Info, optional):** pin the foreign-marker test to 404 (.2), and `warn!` on the
   reconciliation 503 branch so timeouts are logged (.3). Items .1, .4 and .5 only if cheap.
6. **Phase gate**, after B's batch PASS:
   - Ask the user whether to run the full suite (`DATABASE_URL=… cargo test --lib` plus
     `scripts/stack-budget.sh`) and `npm run check:css`.
   - Report what cannot run: `cargo sqlx prepare --check` (no sqlx-cli) and
     `npm run test:run-counts` (no Playwright).
   - B then reviews the combined integration effects, and root checks coordination before phase
     acceptance.

Focused checks: per item, the affected suites. If item 2 edits the schema, also the catalog and
schema tests. Plus clippy, fmt, the macro check, the node tests and the boundary scripts.

### B4c-3b acceptance (2026-10-24)

**B4c-3b accepted in phase** (not integrated, not committed). B's R030 explicitly PASSed digest
`845d5596b5945223b27143dc847eda42cc5cd620a84edc4d5262b9e39793e1d6`, and root recomputed the same
digest. Coordination r10 is unchanged. It took one round.

F101 is closed: .1 adopted and .2 fixed. F102 (Info, optional) dispositions:
- .1 and .2 are confirmations, no change.
- The optional panel note on `DATABASE_MAX_CONNECTIONS ≥ 8 + 2 × slots` and the optional
  `transition_reason` values in the catalog scan go to B4d as cheap extras.
- README §3.4's Shutdown and Leased Execution bullets and the TaskWorker diagram are already on
  B4d's docs list.

**B4c (B4c-1, B4c-2, B4c-3a, B4c-3b) is complete in phase.** Next: B4d (TASK "B4d work order").
Hashes for its delta: `logs/b4c3b-filehashes.txt`.

### B4d progress (2026-10-24)

**1. F094 (Low): fixed** (`lease_resolution_tests.rs`). Both exact lower bounds now read
`elapsed + SERVER_TIMEOUT_GRANULARITY >= persistence_deadline()` and print `{elapsed:?}`. The new
1 ms constant carries a comment on the floored server `lock_timeout`/`statement_timeout`. The 13
lease-resolution tests passed 3 runs in a row.

**2. Exhaustive leftover search:**
- HEAD-relative scans (`uncalled.py`, `uncalled_trait.py`) found no non-test definition newly
  orphaned by p08. A full scan for non-test definitions referenced only by themselves
  (`scratchpad/b4d/uncalled-all.txt`) gave 34 names. Most are trait-impl false positives
  (`visit_*`, `deref`, the MCP HTTP methods) or pre-existing helpers that were dead at HEAD too,
  and those are left alone.
- **Removed:**
  - `AgentPersistence::get_by_company_slug_and_agent_slug`: trait method, Postgres impl and mock
    (F098.4).
  - Legacy helpers that were already dead at HEAD:
    - `ThreadUseCases::hydrate_ingest_configuration` (re-hydrated a legacy task's company and
      channel);
    - `test_support::agent_channel` (the delegation lookup);
    - `InMemoryThreads::mark_sent_by_outreach` with its `outreach_sent` filter, which mirrors the
      Postgres clause B4c-3a removed;
    - `stored_email_history`.
  - `Icon::People` and `Icon::Goal` (F101.3).
  - The dead "Tasks" link to `/companies/{id}/tasks` on the companies page (no such route).
- **Entry-point grep, clean:** no `task_type` other than `workflow_execution`, no legacy route
  prefixes, no harness selection, and no channel execution settings outside negative tests.
  `/ui/work` is the live attention list.
- **DV-B4c3-5 decisions (DV-B4d-1..3):**
  - **DV-B4d-1 (kept):** the delivery purposes `reply`/`mirror`/`outreach` and
    `depends_on_delivery_id`. They are transport-layer vocabulary rather than legacy execution
    entry points, about 40 transport/egress/delivery tests use them as generic purposes, and the
    delivery worker still enforces predecessor ordering. D4.8 does not name them. Revisit with the
    transport or cutover plans.
  - **DV-B4d-2 (dropped):** the `agent` creation-provenance arm, which required a legacy
    `source_task_id`. `CreatorType::Agent`, `CreationProvenance::agent` (no caller) and the
    `source_channel_id`/`source_task_id` fields go. `valid_creation_provenance` now allows only
    `actor_type`/`actor_id`/`actor_name` and `user`|`system`; the scratch DB confirms an `agent`
    provenance is refused.
  - **DV-B4d-3:** `model_usage_calls` `num_nonnulls(...) = 5`, since every call belongs to a
    run, and `reserve_on` uses inner joins, so a missing execution is the existing "owner is
    missing" conflict rather than a guard error. The dead `owner_kind <> 'workflow'` early
    returns in three guards are kept: correct, defensive, and not worth churning complex
    functions.
- The `#[serde(default)]` "re-hydrate legacy `background_tasks` payloads" notes and defaults on
  `Channel` are removed, along with `default_true`.
- The catalog scan's `RETIRED_VALUES` gains `source_task_id` and the 10 distinctive legacy
  `transition_reason`s (F102 extra).

**3. Docs and guides** (README §3.x verified by a read-only Explore fact-check against the
code):
- `README.md`:
  - Highlights: workflow-driven channels, human decisions, governance without
    `MemberWorkAtStake`, the real UI pages.
  - The Rig paragraph: model steps, skills appended to the prompt, the `output_schema` contract.
  - §3.3: the actual `X-MailAgents-*` headers, `MAX_INGRESS_HOPS` 5, 60 messages/hour.
  - §3.4: one inbound commit with intents, worker slots, leases (job 30 s with a 10 s
    heartbeat; intents and occurrences 60 s; deliveries 120 s), and the real shutdown joins.
  - The 155-line legacy task flow diagram is replaced with a workflow execution flow that lists
    the 16 handled step kinds.
  - §3.5: the work list and run controls.
  - §3.6: channel wording.
  - §3.7: rewritten. Per-channel admission and triggers, `+` lists without sequencing, bounces
    (webhook vs SMTP 550), replies to the sender only, and the Ana example redone. Manual handoff,
    shared reply context, upstream pipeline context and the prompt-variable claims are removed.
  - The flow diagram's last box, §3.8 history, §3.9 bubbles and the rail.
- `docs/deploy.md`:
  - "workflow workers" in the apps table and in the operational notes;
  - the database-outage note, which cited `ERROR_BACKOFF` in `task_worker.rs` (F096.4);
  - the stack-size row;
  - the Rig rollout runbook rewritten for workflow runs: run detail, decision wait, the
    reconciliation note route (annotation only), cancel, deactivating bindings for rollback, and
    version-pinned contracts. Pilot rows: structured output by `output_schema`, MCP via a
    workflow `mcp.call` receipt.
- `docs/rig.md`: ownership line, Tools/Skills/Context rows (skills are appended to the preamble;
  `read_resource` is gone from the workflow path; context is the step's fenced `context` input),
  the skills paragraph (16 skills, 32 steps, 8,000 characters), the execution paragraph, review
  drafts removed, the reconciliation route, diagnostics wording, preflight wording.
- `docs/custom_tools.md`: the skills bullet, and Agent Builder rewritten (its two provisioning
  tools, owner/admin requester). The stale "Planned" section is removed.
- `docs/inter_channel_agent_communication.md`: replaced. The outreach protocol is gone; it now
  describes the internal relay (sender identity, refusals, receiving ingress, reply correlation,
  trace/cycle/`MAX_INGRESS_HOPS`, headers).
- Guides:
  - `src/adapters/persistence/AGENTS.md` (F093.5): rules kept, examples made historical or
    pointed at live code (`AGENT_COLUMNS`, `message_intents.rs` aliases, the macro file list,
    impl counts);
  - `src/AGENTS.md`: `thread/` layout and test-support wording;
  - `src/adapters/harness/rig/README.md`;
  - the `value_objects.rs` `ToolId` comment and the `mailbox.rs`/`ui_team.rs` comments.
  - `src/application/AGENTS.md` uses "task" as a generic unit of work in rules that still hold,
    so it is unchanged.
- Kept as historical records: `docs/rig-verification.md` (a dated measurement) and
  `docs/transport_architecture.md` (delivery-policy vocabulary, kept per DV-B4d-1).
- Every config key in `.env.example`, `fly.toml` and `docs/deploy.md` is read by the app, except
  the expected ones: placeholders, `RUST_LOG`, `RUST_MIN_STACK`, `SQLX_OFFLINE`,
  `DATABASE_SLOW_QUERY_LOGGING_ENABLED` and script inputs.

**4. Records:**
- **F093.2 (DV-B4b-1 addendum):** removing a member who is the only listed reviewer of an open
  workflow decision leaves it unsettleable until its wait or run deadline, or a cancel. This is
  a p09 follow-up: say so in the removal confirmation, or offer reassignment.
- **F100:** binding status checked under the channel gate is a p09 candidate.
- Plan files 05–09 carry no no-backward-compatibility note of their own (the plan README does):
  a candidate for the coordinator, not an edit of other phases' files.
- `PipelineStep` is now carried but never read after routing (fact-check): recorded, kept as
  transport vocabulary.

**5. F073 (Info):** .2 the foreign-marker test now pins 404; .3 the reconciliation 503 branch now
logs `warn!`. Items .1, .4 and .5 are not taken.

**Schema:** edited in place (`valid_creation_provenance`, the `model_usage_calls` check);
`workflow08_test` recreated. Scratch apply is clean (`logs/b4d-scratch-apply.log`, full output);
catalog output in `logs/b4d-catalog.log`.

**Checks:**
- clippy and fmt clean (`logs/b4d-clippy.log`, `logs/b4d-fmt.log`).
- Broad focused run (persistence, http, infra, entities, services, use cases, transport,
  workflow, `model_usage`, harness, protocols, smtp): **2114 passed**, 0 failed, 3 ignored
  (`logs/b4d-focused-1.log`).
- After the last code edits: 562 passed (`logs/b4d-focused-2.log`).
- Node: 7 passed. Both boundary scripts passed. Macro check: 23/23.
- Not run: `cargo sqlx prepare --check` (no sqlx-cli), `npm run check:css`, the full suite and
  `stack-budget.sh`. These belong to the phase gate, with the user's go-ahead.

**Frozen:** digest `4ec81c1a9bf670f8efb5ef46b9ebac7f8c44261a70fa538b19cecfa598140fc1` (same base
and exclusions; `logs/b4d-snapshot.txt`). Delta against B4c-3b: 27 files (`logs/b4d-delta.txt`).
Hashes: `logs/b4d-filehashes.txt`. Review: WF-P08-R031, the B4d batch plus the phase-level
acceptance criteria.

### B4d corrections for WF-P08-R031 (2026-10-24)

R031 at `4ec81c1a…0fc1`: CHANGES REQUIRED (F103 Low, F104 Low, F105 Info). F094, F101 and F102 are
recorded as resolved.

- **F103: fixed.**
  - `docs/rig.md`: the "Model attempts" row says 8 per model step, shared by continuations and
    repairs, not configurable per agent or step. `PreparedAgent::policy` (`application/workflow/agent.rs`) uses
    `RigExecutionPolicy::default()`, with `model_calls: 8`; `max_turns` went with `config_json`.
  - `docs/deploy.md`: the Skill pilot row now asks for a controlled `agent.run` step whose agent
    has an attached skill to answer by its instructions.
  - Re-grep: the only remaining `read_resource` in `docs/` is `docs/rig-verification.md:35`, a
    dated verification record, kept as history. No `max_turns` remains.
- **F104: fixed.** `McpPersistence::mcp_connections_by_ids` (the trait method and the Postgres
  impl) is removed, as criterion 13 requires.
  - `mcp_read_tests.rs` becomes `catalog_reads_are_tenant_scoped_and_exclude_deleted_rows` on
    `list_mcp_connections`: the owner sees only its live connection, and the foreign company only
    its own.
  - `workflow/mcp_test_support.rs` reads through a new `connection_on` helper (the live catalog
    filtered by id).
  - The other criterion-13 items are verified gone (0 hits each): `HarnessRegistry`,
    `AgentRunner`, `ApprovalUseCases`, `outreach_and_await_quorum`. `HarnessKind` is kept as the
    fixed `identity.harness` literal, per D4.8.
- **F105 (Info).**
  - Item 1, B's acceptance matrix, is adopted as the phase gate's evidence map
    (`agent-review-findings.md` F105.1).
  - Criterion 2's decisions and artifacts: the structural argument is recorded in place of a
    new test.
    - A decision belongs to exactly one run's wait: `workflow_decisions_wait_fkey` →
      `workflow_waits(company_id, run_id, execution_id, id)`, with `UNIQUE(company_id,
      wait_id)`.
    - A submission settles only that wait, and therefore only that run.
    - Step outputs (artifacts) are per execution, and an execution belongs to one run.
    - So two message runs in one thread cannot share a decision or an artifact. Same-thread
      independence of waits, runs and replies is tested directly
      (`workflow_wait_same_thread_messages_remain_independent`, the email end-to-end test).
  - The 60-per-hour turn guard has no test, at HEAD or now: a pre-existing gap, recorded and not
    added.
  - Item 4: DV-B4d-1's "revisit" is a candidate for the coordinator (transport or cutover plans),
    together with the plan-note candidate.
- **Gate note (R031 (e)).**
  - The dev database `workflow08` on :55480 still has a pre-p08 schema. Tests use
    `workflow08_test` and their own databases. Recreate it before any `cargo sqlx prepare` or
    migration run against it; that needs sqlx-cli, which is absent.
  - `workflow08_scratch` is disposable and is dropped at completion.

**Checks:**
- clippy `-D warnings` and fmt clean.
- `cargo test --lib -- mcp`: 53 passed (`logs/f103-f104-focused.log`).
- Macro check: 23/23.

**Frozen:** digest `00ec454fd48fd7bc7600589474672feb5f76553e437e4f0d3333f2ec595b6829`
(`logs/b4d-snapshot-r2.txt`). The delta against R031 is 6 files: `docs/deploy.md`, `docs/rig.md`,
`persistence/mcp.rs`, `mcp_read_tests.rs`, `workflow/mcp_test_support.rs` and
`use_cases/mcp.rs` (`logs/b4d-filehashes-r2.txt`). Recheck: WF-P08-R032.

### B4d acceptance and phase status (2026-10-24)

**B4d accepted in phase** (not integrated, not committed).
- B's R032 explicitly PASSed digest `00ec454fd48fd7bc7600589474672feb5f76553e437e4f0d3333f2ec595b6829`,
  and root recomputed the same digest (same base and exclusions). Coordination r10
  (`1df326af…d9ae`) is unchanged, and no notice targets p08 beyond those already acknowledged.
- Rounds: R031 (CHANGES REQUIRED: F103 and F104 Low, F105 Info) and R032 (PASS).

**Phase status.** Every batch is accepted in phase: B1, B2, B3a, B3b-1, B3b-2, B4a, B4b, B4c-1,
B4c-2, B4c-3a, B4c-3b and B4d.
- B reviewed the plan's acceptance criteria in R031 and R032. The evidence map is F105.1 together
  with the criterion-2 structural argument above. No blocking finding is open.
- **Phase acceptance waits for the gate checks, which need the user's go-ahead:**
  - the full `DATABASE_URL=postgres://workflow08@127.0.0.1:55480/workflow08 SQLX_OFFLINE=true
    CARGO_INCREMENTAL=0 cargo test --lib`;
  - `scripts/stack-budget.sh`;
  - `npm run check:css`.
- Unavailable here: `cargo sqlx prepare --check` (no sqlx-cli; the dev DB `workflow08` would
  need recreating first) and `npm run test:run-counts` (no Playwright).

**Gate checks deferred by the user (2026-10-24).** Asked at the phase gate, the user chose "Not
now".
- The phase stays **implemented and review-accepted per batch**, but is **not phase-accepted**:
  the full `cargo test --lib`, `scripts/stack-budget.sh` and `npm run check:css` are owed.
  `cargo sqlx prepare --check` and `npm run test:run-counts` are unavailable here.
- Nothing is committed or integrated.
- Resources: cluster 55480 with `workflow08` (pre-p08 schema; recreate before any sqlx prepare)
  and `workflow08_test` (current schema). `workflow08_scratch` has been dropped.
- Reviewer B (`af5ae9ab0d4e0a22f`) is idle and can be resumed for any post-suite changes.
