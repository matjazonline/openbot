# P07 — Child workflows and bounded human-feedback revisions

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Identity

- Phase ID: `p07` (IDs `WF-P07-…`). Workflow: `implement-opus-reviewed-2_2`
  (skill dir `/Users/mac02/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Record root: `/Users/mac02/dev/openbot/plan/workflow-foundation/phases/p07/docs/`.
- Session 2 (2026-10-22, resumed from HANDOFF): the work moved from `/Users/mac03/dev/openbot`
  (session 1, 2026-10-21) to `/Users/mac02/dev/openbot`. User commit `2d608f2` ("phase 07
  partial") carries the session-1 records, the user's plan-record cleanup and marketing edits.
- Root = Author (A): this Claude Code session, Opus 5.5 / xhigh (user confirmed 2026-10-22 via
  question; user settings `modelSettings.claude-opus-5-5.effortLevel = xhigh`).
- Worktree: `/Users/mac02/dev/openbot`, branch `workflows`. Phase starting commit
  `921079d125ec0f768eee53f603a1db648ff356bc` (session 1); current HEAD/review base
  `2d608f269761717de7706912bd99ddd325d9d352`. `git diff 921079d 2d608f2` touches no `src/`,
  `migrations/`, `.sqlx/`, `Cargo.*` or `scripts/` (verified 2026-10-22); source equals phase-06
  commit `4dd01b3`.
- Preserved pre-existing work: none in the working tree (session-1 deletions are committed in
  `2d608f2`). `marketing/` and `website/` are the user's unrelated areas (excluded).
- Snapshot exclusions (exact, unchanged): `plan/workflow-foundation/phases/p07/docs
  plan/workflow-foundation/COORDINATION.md marketing website`. Digest at `2d608f2` with no
  source edits: `31c52a4a554512444396a39b77d8b2b5142231232cf0ee3027a5d4855fc91b21`.
- Plan coordination: `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md`,
  writer = this root (handoff recorded in r2), revision 2 (immutable copy
  `logs/coordination-r2.md`). Notices WF-C001 r1 (→ p08) and WF-C002 r1 (→ p09) proposed by
  this phase; none target p07.
- Compaction: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` added to `.claude/settings.local.json` env on
  2026-10-22 (user-approved). It may not apply until a session starts after the edit, so the
  75% target is not claimed for this session.
- Tooling: the `graft` CLI/MCP is not installed on this machine (MCP failed: executable not
  found), so investigation used grep/read instead of the graph.

## Original requirements

- Plan: `plan/workflow-foundation/07-child-workflows-and-review-revisions.md` (authoritative),
  expansion rows 07.1–07.4 in `plan/workflow-foundation/EXPANSION.md`, README, REPLACEMENT-MAP
  rows naming phase 07, NOT_PLANNED (acyclic children).
- Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`, `src/infra/AGENTS.md`.
- Prerequisite: phase 06 accepted 2026-10-21 (06-R025 PASS, digest `0d8bba5d…9b00`), committed
  as `4dd01b3`. Phase-08 obligations from 06 stay with phase 08 (worker wiring etc.).

### Acceptance criteria (from plan §Acceptance + body)

1. `workflow.call` invokes a pinned version with schema-validated explicit inputs; child creation
   and parent wait persist together; one child identity per logical call across retries; child
   gets only explicit inputs and allowed resource bindings; returns its declared output.
2. Children inherit root authorization ceilings, deadlines, budgets and cancellation; cannot gain
   access via a more privileged workflow; dependency graph frozen at publication; recursion
   rejected; child results/failures become durable parent continuation events; parent occupies
   no worker while waiting.
3. Published workflows exposed as agent tools (input/output schemas); saved model call mapped to
   child identity before start; parent agent resumes the exact pending call; rejected/failed
   children yield typed results/errors, never fabricated success.
4. Retry vs revision vs rerun semantics as tabled; never reset a completed agent step.
5. `flow.repeat`: child ref, initial mapping, next-iteration mapping, exit predicate, max
   iterations, exhaustion route; exit evaluated after each child completion before another
   round; last-round acceptance succeeds; exhaustion follows declared route, never
   auto-approves; final output = final child result + round count; prior rounds linked in trace.
6. Shipped support template: three rounds default, exhaustion → human escalation; authors may
   choose a lower bound within platform limits; deadlines/root budgets span all rounds.
7. Successors can explicitly map human results (`/steps/review/output/feedback|data`).
8. Tests: revision feedback reaches next round with prior artifact/decision intact; older draft
   acceptance cannot authorize newer; late links cannot settle current round; only final
   accepted artifact sent once; no replay of context loads/earlier effects; child-creation
   retries, duplicate completion, parent cancellation, child timeout, restart; last-round
   acceptance, exhausted revisions, inherited budget enforcement; agent workflow tools resume
   the original checkpoint after a child human decision.

## Batches

| Batch | Scope | Gate | Status |
| --- | --- | --- | --- |
| B1 | Child calls (`workflow.call`), bounded repeat (`flow.repeat`), cancellation/deadline/budget inheritance, review-round fixtures/template, successor feedback mapping, tests 1,2,4,5,6,7,8 (minus agent tools) | P07-D1 (architecture) | gate passed (rev 4, R004); batch R005 changes → R006 **PASS**; **accepted in phase** at `d82fa028…5c11` |
| B2 | Agent workflow tools (registration, publication freeze, Rig tool target, child from saved model call, agent continuation, typed result/error), criterion 3 + its tests | P07-D2 (architecture, after B1 accepted; product questions may go to the user) | P07-Q1/Q2 answered; gate **passed** (rev 4, R010); implemented `201af80c…1f98`; R011 changes (F027, F028 opt.) corrected; R012 **PASS**; **accepted in phase** at `16e65cb8…aee9` |

Final phase gate after B2: fmt, offline all-target check, strict clippy, fresh migration + SQLx
check, affected DB-backed suites (workflow), stack budget for affected tests; full suite only if
the user asks (AGENTS.md requires full verification before declaring completion — see Final gate).

## Investigation summary (2026-10-21, A)

Existing foundation (phases 02–03): registry/compiler accept `workflow.call`/`flow.repeat` with
pinned `child_workflow_id`/`child_version_id` and `max_iterations` (1..10000); publication pins
child `PublishedBundle`s transitively (`PublishedBundle::children`), rejects missing/duplicate/
foreign/recursive/unused children, depth ≤ 32. Runtime: `workflow_runs.parent_run_id/
parent_execution_id`, immutable `workflow_run_parents` lineage, root budgets shared by
descendants (`workflow_run_budgets`, `workflow_root_budgets` incl. `repetitions`), and a child
terminal-state trigger appending an immutable `parent_wakeup` fact (`schedule_workflow_parent_
wakeup`); `WorkflowParentWakeups::parent_wakeups` reads it. Waits support reason `child_run`
in schema only. Gaps: no handler/park/resume for child kinds (worker returns Unsupported), no
child params/resources rule, no repeat next/exit/exhaustion language, no cancellation cascade,
child admission exists only as binding-based external admission (`TriggerSource::Child`), the
wait index allows one non-decision wait per execution, activation/inspection read
`workflow_admissions.source_key`.

## Agents

- A/root: this session (Opus 5.5/xhigh).
- B (session 1): agent `a46b56e8565db0c84` (general-purpose, opus, xhigh, fresh context),
  spawned 2026-10-21 for P07-D1; round R001. Unreachable from session 2 (other machine/session).
- B (session 2): fresh replacement, same role/settings (general-purpose, opus, xhigh, fresh
  context, no fork), agent `a5e5548971a73e411`, spawned 2026-10-22 for the R001 correction recheck (round WF-P07-R002).

## Decisions

- **P07-D1 (B1 architecture gate)** — `logs/decision-D1.md` revision 1 (sha256
  `7c3245e7…8f3c92`), code digest `e2714983…e3fc`. B round **WF-P07-R001: changes required**
  (2026-10-21). Findings (B-owned, see `agent-review-findings.md`):
  F001 High — child history snapshot leaks later thread messages / unbounded per-round insert
  (fix: children capture no history or share root snapshot; test late message invisible);
  F002 Medium — non-input park/round failures roll back and re-poll forever (define outcomes,
  poison tests); F003 Medium — rerun link deferred without agreed disposition (specify via
  manual admit link or record agreed disposition); F004 Medium — phase-03 binding-based child
  admission bypasses frozen graph/recursion/deadline (refuse/remove in B1, move fixtures);
  F005 Low — `control_retry.rs:8-9` child retry fact undefined (decide, comment, refusal test);
  F006 Low — cascade lookup by unindexed `parent_run_id` under 1 s timeout (use the parent's
  waiting `child_run` wait) and no cancel-vs-park/round race test; F007 optional suggestions.
- **A self-finding for revision 2 (not yet reviewed):** no production `PublicationDirectory`
  captures pinned child bundles (all return `children: vec![]`; `verify_children` only checks
  supplied ones), so a parent with `workflow.call` cannot be published. Rev 2 must add a
  child-capturing directory (company-scoped `workflow_versions` bundle by declared child
  workflow/version, unarchived) and place the by-name child resource check where both parent
  definition and child bundles are visible (publication validation). Reuse the registry
  `static_roots` check (`registry/mod.rs:177`) for `with.params` with a field-specific code.

- **P07-D1 revision 2** (2026-10-22) — `logs/decision-D1-r2.md` (sha256
  `e5ceea98409dc8a4e6dc5a48931305c57d99658877d718e87404a6e8d5738c93`), complete on its own.
  Resolves F001 (no child history; side-effect table), F002 (failure classification table +
  poison tests), F003 (rerun link `rerun_of` built in B1), F004 (`TriggerSource::Child` refused;
  runtime writer is the only child path; fixtures move), F005 (retry refused for children and
  child-failed parents; comment + tests), F006 (cascade via waiting `child_run` wait; competing
  tests), F007.2/.3/.5/.6 adopted, F007.1 declined with rationale, F007.4 → notice WF-C002, and
  the publication self-finding S1 (`ChildPublicationDirectory`). B round **WF-P07-R002: changes
  required** (2026-10-22): all R001 findings resolved at decision level; new F008 Low (fixture
  move list misses `budget_activation`, `budget_schema`, `wakeup_guard`, `wakeup_parent_link`;
  nested fixture; focused filters) and F009 Low (no test for the inaccessible-original rerun
  refusal); F010 optional (5 items).
- **P07-D1 revision 3** (2026-10-22) — `logs/decision-D1-r3.md` (sha256
  `96d201af8768e57a05b5796c1e4bd07d1370ddd914d554900eaa127b42cfe0b6`), complete on its own;
  delta `logs/decision-D1-r2-to-r3.diff`. Fixes F008 (inventory, nested fixture, filters below)
  and F009 (identical `NotFound`, nothing persisted). Adopts all F010 items (wakeup by
  `terminal_wakeup_sequence`, cascade over every waiting child wait, deferred
  `workflow_child_parent_wait` constraint, 256 cap + batched reads, configuration before
  `round:1`). Author refinement: rerun-original checks only in `admission_source::authorize`.
  B round **WF-P07-R003: changes required** (2026-10-22): F009 and F010 closed; F008 still
  open (a: 8 more files reach the removed path via `eligibility::child` and
  `harness_token_refusal::{child,child_checkpoint}`; b: filters used file names, not module
  paths; c: no per-case rule for unreachable states); F011 optional (top-level rule of the
  original not backed by the schema).
- **P07-D1 revision 4** (2026-10-22) — `logs/decision-D1-r4.md` (sha256
  `5c076606580e887de3d12203fa31e3a1d3a73b1112b169840b56938698a4adc4`), complete on its own; delta
  `logs/decision-D1-r3-to-r4.diff`. F008: complete inventory
  method and list (18 persistence + 2 application files); per-case rule A runtime chain /
  B lineage-only child (`lineage_child`: production `children::create_on` + settled
  `child_run` wait) / C retired and replaced, recorded per test in `logs/b1-fixture-moves.md`;
  filters rewritten as module paths with baseline counts. F011 adopted (trigger
  `workflow_rerun_original`). Sent to B for the R003 correction recheck.
  B round **WF-P07-R004: PASS** for revision 4 (sha256 `5c076606…adc4`, digest `31c52a4a…1b21`),
  2026-10-22. **Gate P07-D1 passed; revision 4 is the implementation contract for B1.** Optional
  F012 (lineage_child contract caveats 1–5) adopted in the helper's doc comment and assertions
  (refuse wait/child-kind parent steps; distinct ordinals; outside cascade; revision bump;
  replay-after-settle reads `Refused`). Erratum (from F012.6): D1.2a says "18 in persistence"
  but its list, which is authoritative, names 24 persistence files.
  Coordination r3 (`logs/coordination-r3.md`, sha256 `0c58cfd3…521a`): WF-C001 and WF-C002 active.

- **P07-Q1 (user product decision, 2026-10-22, B2):** agents call workflows through a **named
  tool registration**. An admin registers a workflow tool once (tool name → workflow and exact
  published version, pinned) and grants it to agents by name, like HTTP tools. Workflows using
  that agent freeze the pinned version at publication; upgrading is an explicit admin edit picked
  up on republish. User note (requirement): **add a test that the workflows listed in an agent's
  tools are from the agent's company.** Alternatives declined: pin in the `agent.run` step;
  agent names a workflow directly (latest version at publish).

- **P07-Q2 (user product decision, 2026-10-22, from gate finding F018):** **revoke stops,
  re-pin doesn't.** Re-pinning a registration to another version, or archiving the pinned
  workflow, leaves already-published workflows running on the version they froze; they move to
  the new version only when republished. Revoking (deleting) the registration is a kill switch:
  new runs are refused and running agents can no longer call the tool (like revoking an HTTP
  tool). Alternatives declined: fully frozen (no kill switch); any change stops them.
- **P07-D2 (B2 architecture gate)** — `logs/decision-D2.md` revision 1 (sha256
  `1e11108ac016322ab1be1d293e1d32ca7bc97c29c953936d42918142cd50ff27`), 2026-10-22, from the
  P07-Q1 product decision and a read-only code map of the agent tool path. Key choices:
  `workflow_tool_registrations` (composite FK pins a same-company version); `ToolSnapshot.workflow`
  pin; `ChildPublicationDirectory` outside `AgentPublicationDirectory`, collecting step and tool
  pins; tool children counted in contracts, resources-by-slot, recursion and depth;
  `ChildCause::Action` replaced by `ChildCause::ToolCall` keyed by the saved harness invocation;
  no `ActionService` intent/receipt; agent park via `child_parked` attempts and a tool-call
  `child_run` wait; continuation via `reopen_job`, the replayed call recomputing the result from
  immutable child facts; typed error envelopes for failed/cancelled children, invalid arguments,
  continuation limit and oversized results; new fenced-completion arms; WF-C003 → p09 and
  WF-C001 r2 → p08 proposed. Sent to B for the gate review (WF-P07-R007).
  B round **WF-P07-R007: changes required** (2026-10-22): F018 Medium (re-pin/archive semantics
  vs P07-Q1 → user decision P07-Q2), F019 Medium (competing/recovery tests for `child_parked`),
  F020 Low (registration recheck at dispatch), F021 Low (`connection: None` read as HTTP in more
  sites, incl. `resources::check_readiness`), F022 Low (same-name MCP step tool shadows a
  registration); F023 optional (4 items). Core runtime design judged sound.
- **P07-D2 revision 2** (2026-10-22) — `logs/decision-D2-r2.md` (sha256
  `46301f5ec1a6f67a8c6a9cd11f1eff98b0225d9a595b330561fecfdba2143451`), complete on its own; delta
  `logs/decision-D2-r1-to-r2.diff`. Resolves F018 (P07-Q2: `registered_on` modes Publish = exact
  pin, Run = registration exists; re-pin/archive leave frozen pins; revoke refuses admissions and
  new calls with `workflow.tool_revoked`, a parked call still delivers), F019 (tests: cancel vs
  dispatch both orders and racing, lease expiry incl. inside the transaction, cancel vs
  continuation, reopened attempt failing before saving), F020 (registration `FOR SHARE` before
  run locks at dispatch), F021 (`ToolSnapshot::kind` classifier used exhaustively at every site),
  F022 (ambiguity refusal), all of F023 (safety rationale, dropped unreachable branch, no approval
  flag, WF-C003 → p08 routes + p09 UI). Sent to B for the R007 correction recheck (R008).
  B round **WF-P07-R008: changes required** (F018–F023 fixed/closed; new F024 Low: Publish-mode
  scope over stored child bundles; F025 optional: Run mode should match name + `workflow_id`).
- **P07-D2 revision 3** (2026-10-22) — `logs/decision-D2-r3.md` (sha256
  `b13f51e92d98a69d4f83dd30c7d321eb91b991386940a04e66350afedd0ab335`); delta
  `logs/decision-D2-r2-to-r3.diff`. F024: Publish mode only for the root's freshly captured
  workflow-tool snapshots, Run mode for stored child bundles (as `verify_on` does), tests for a
  child whose tool was re-pinned (publishes, runs the frozen pin) and revoked (refused). F025
  adopted: Run mode matches name and `workflow_id`, so re-pointing or reusing a name for another
  workflow keeps old pins refused. Sent to B for the R008 recheck (R009).
  B round **WF-P07-R009: changes required** (F024 fixed, F025 closed; new F026 Low: dispatch
  step 3 refused only a missing registration, not a re-pointed one).
- **P07-D2 revision 4** (2026-10-22) — `logs/decision-D2-r4.md` (sha256
  `ebf212db284e9a6e128d48d5ac8d97f57d45821db7e3e78c3dc096fc5e2dfab7`); delta
  `logs/decision-D2-r3-to-r4.diff`. F026: dispatch step 3 applies the Run-mode rule (name and
  `workflow_id`), a re-point refuses the next call with `workflow.tool_revoked` and creates no
  child while a parked call still delivers; D2.7 test; invariant wording aligned. Sent to B for
  the R009 recheck (R010).
  B round **WF-P07-R010: PASS** for revision 4 (sha256 `ebf212db…dfab7`), 2026-10-22.
  **Gate P07-D2 passed; revision 4 is the B2 implementation contract.** Implementer note from
  B: D2.6 "the registration's static params" means the frozen pin's params (`pin.params`).
  Coordination r4 (`logs/coordination-r4.md`, sha256 `89563c7e…0f5b`): WF-C001 r2 (→ p08) and
  WF-C003 r1 (→ p08, p09) active.

## B1 implementation notes (2026-10-22, A)

Implemented per D1 rev 4. Deviations and choices for the batch review (each is reviewable
against the contract; none changes a gated invariant):
1. **Round budget exhaustion** (D1.6 says "round: wait completed, run failed"): the parent's `child_run` wait is
   settled `failed` with code `root_budget_exhausted` (new `failure_code` value) through
   `wait_commit::fail`, with `terminal_execution_id` set, the parked job closed without a failed
   attempt and an audit row. Reason: a replay then reads the same unrouted failure instead of a
   completed wait with no successor. `child_waits::budget_exhausted`.
2. **`workflow_wait_child_link`** FK is `DEFERRABLE INITIALLY DEFERRED`: the company cascade
   deletes `workflow_run_parents` before `workflow_waits`, which an immediate FK refused.
3. **`ChildPublicationDirectory`** returns `AppError::BadRequest` (not a `publication.limit`
   diagnostic) for > 256 distinct pins and for the summed byte bound: `capture` returns
   `AppResult`, as `AgentPublicationDirectory`'s refusals do. An undecodable/unparseable draft
   captures nothing so compilation reports it with spans. The byte bound is
   `child_publication::within_bundle_bound` (unit-tested; real 4 MiB bundles are impractical).
4. Test `Fixture` composes `ChildPublicationDirectory<Preflight>` (WF-C001); new support
   `Fixture::publish_source`, `BindingFixture::from_version`.
5. `budget_activation::deferred_commit_failure…` is category B, not A as D1.2a expected: its
   `consumed == false` case needs a child activating before its root (see
   `logs/b1-fixture-moves.md`).
6. The application memory store mirrors the SQL owner (refuses `Child`; parent maps removed).
7. Template end-to-end publish uses the production composition
   (`ChildPublicationDirectory<AgentPublicationDirectory<Preflight>>`) with a real `assistant`
   agent row; the round's synthetic fixture tool facts are not used there (the SQL owner
   rightly refuses an unregistered HTTP tool).

New test modules (filters): `batch_tests::child_runtime_tests::` (10),
`batch_tests::child_repeat_tests::` (4), `batch_tests::child_schema_tests::` (2),
`tests::child_publication_tests::` (3), `admission_authority_tests::rerun_tests::` (4).
Fixture moves: `logs/b1-fixture-moves.md`.

## B1 corrections for WF-P07-R005 (2026-10-22, A)

B round R005 (digest `1e1b0a03…c9e4`): changes required — F013–F016 actionable, F017 optional;
deviations 1–7 accepted; F012 closed. All corrected in one round:
- **F013 (template escalation):** `repeated-human-revision.yaml` routes `escalated` to a new
  `specialist` `agent.run` whose context maps `untrusted_request` (`/input/message`), `artifact`
  and `feedback` explicitly; `timed_out` and `exhausted` still go to the human escalation;
  `accepted` → reply, `cancelled` → end. The fixture's synthetic facts now include the agent
  snapshots for that parent. Test
  `fixtures::tests::workflow_fixtures_revision_escalation_reaches_a_specialist_with_artifact_and_feedback`;
  the end-to-end publication test publishes the changed parent with the real `assistant` row.
- **F014 (params/resources):** registry test
  `registry::runtime_tests::workflow_registry_child_params_are_static_and_resources_match_by_slot`
  (`registry.static_params` for run input and run context, literal conformance, default `{}`,
  `registry.child_resources` for a missing slot and another kind). Runtime tests in
  `batch_tests::child_configuration_tests::`: the child gets exactly the frozen `with.params` and
  the parent's resource for its one slot (parent binds two MCP slots), binding id as provenance;
  a parameter the child refuses — see deviation 8.
- **F015 (decision rounds):** `batch_tests::child_decision_round_tests::workflow_revision_rounds_with_human_decisions_keep_old_rounds_and_load_context_once`
  runs through the real worker, fair poll, history handler and decision notices: a
  `context.history` load, then `flow.repeat` over `decision.human` rounds whose reviewer comes
  from the child's `/params` (parent's static `with.params`). It asserts the revision reaches
  round two as explicit input. Round one's decision row stays byte-identical, and late
  submissions to it (stale and fresh view) are refused while round two stays waiting. Context
  and repeat each run once, history members are unchanged, and only the accepted artifact is
  delivered, once. No effect precedes the repeat in this test; the shipped template's only
  pre-repeat step is the context load.
- **F016:** `batch_tests::child_output_tests::` — a child result the parent's `$end` contract
  refuses, and a stored child result corrupted outside the pinned output schema (raw update with
  triggers bypassed) — both fail the wait `invalid_output`, run failed, no successor, replay
  identical. `child_schema_tests` now uses real rows: a rerun naming a top-level run of another
  workflow of the company or of another company → 23503; a `child_run` wait naming another
  company's real child → 23503 at commit.
- **F017.1 adopted:** `registry.child_params` — a step that omits `with.params` for a child whose
  parameter schema refuses `{}` is reported at publication (`Registration::default_params`).
- **F017.2 declined:** a message body is at most 16 KiB, the same bound as the artifact body, so
  the human notice cannot also carry feedback without risking an oversized message that would
  fail the escalation itself; the specialist path (F013) carries both.
- **Deviation 8 (D1.9 failure table row "park: invalid `with` input; params/resources rejected
  by the child bundle → settle(InvalidInput) → declared error route"):** the step's `input` and
  `params` contracts embed the child's input and parameter schemas, so a value the child refuses
  is refused at step activation. Like every step's invalid frozen input, it fails the run
  unrouted (`pending_recovery::route` requires an activated execution; no frozen inputs can be
  fabricated). With F017.1 a post-activation params refusal is unreachable; the
  post-activation refusal that does occur (the child's own context limit) still takes the error
  route (`workflow_call_child_limits_reject_input_at_park_and_route_it`). Test
  `workflow_child_parameters_the_child_refuses_fail_activation_without_a_child` asserts the actual
  behavior (run failed, `workflow.invalid_input` audited, no child, replay unchanged).
- Test support: `BindingFixture.params`, `BindingFixture::{mcp_slot, from_version}`,
  `AdmissionFixture::{with_bound_child, foreign, sibling}` (moved from `rerun_tests`).

## Batch B1 verification plan

Focused (during B1; DB-backed on the phase cluster, `CARGO_INCREMENTAL=0`,
`DATABASE_URL=postgres://workflow07@127.0.0.1:55470/workflow07`). Libtest takes several
substring filters in one run (`cargo test --lib -- <f1> <f2> …`). These are real module paths;
the number is how many tests each selected in `logs/baseline-workflow-tests.log`:

| Filter | Baseline | Covers |
| --- | --- | --- |
| `sibling_tests::eligibility_tests::` | 53 | `action_reconciliation_{eligibility,claim_fairness,claim_race,claim_sql,claim_sql_wrong_execution,claim_boundary,claim_recovery,owner_lifecycle}` |
| `batch_tests::wakeup_tests::` | 30 | `wakeup`, `wakeup_guard`, `wakeup_parent_link`, `budget_activation`, `budget_schema` |
| `lease_tests::control_tests::` | 37 | `control`, `control_acceptance`, `control_guard`, `budget_race` (`budget_runtime_tests::race_tests`) |
| `model_owner_tests::store::tokens::` | 25 | `harness_token_refusal` (`refusal`), `harness_token_scheduling` (`scheduling`), `harness_checkpoint_update` (`updates`), `atomic` |
| `admission_tests::admission_token_tests::` | 4 | `admission_token` |
| `admission_tests::admission_source_tests::` | 5 | `admission_source` |
| `admission_tests::activation_tests::` | 17 | `activation` |
| `workflow::tests::admission_binding_tests::` | 5 | `admission_binding` |
| `application::workflow::tests::` | 29 | `causality_cases`, `source_validation` |
| `application::workflow::{compiler,registry,publication,templates,fixtures}::` (5 filters) | 33/32/31/10/10 | language, catalogue, publication, template |

New B1 tests are added under named modules (recorded with their filters when created). Every
run records the command, counts and result; a filter that selects fewer tests than expected is
treated as a failed check.
Final gate (after B2): `cargo fmt --check`, `SQLX_OFFLINE=true cargo check --all-targets`,
strict clippy, fresh migration + `cargo sqlx prepare --check`, affected DB-backed workflow
suites, `scripts/stack-budget.sh` for affected tests; the full suite only if the user asks
(AGENTS.md requires it before declaring completion, so it is reported as outstanding until run).

## B2 implementation notes (2026-10-22, A)

Implemented per P07-D2 rev 4 (`logs/decision-D2-r4.md`, gate PASS R010). Choices and deviations
for the batch review (none changes a gated invariant):
1. **Registration** (`application/workflow/tools.rs`, `adapters/persistence/workflow/workflow_tools.rs`):
   refusals map to `BadRequest` (reserved `message.*` name, non-object or > 64 KiB params, params
   the pinned bundle refuses, the 256 cap); unknown, foreign-company and archived versions and a
   non-admin actor are the same non-disclosing `NotFound`; a revision mismatch or a second create
   is `Conflict`. The cap is counted under `authorize_company`'s company row lock.
2. **Snapshot** (`workflow_tool_snapshot`): contract from the pinned child's own schemas,
   `connection: None`, policy `workflow.tool` / revision = registration revision / `Write` /
   `SafeRepeat`, `workflow: Some(pin)`. A pin to an archived version cannot be captured
   (`BadRequest` "archived or missing").
3. **Classifier** (F021): `ToolSnapshot::kind()` → `ToolKind::{Message, Slot, Http, Workflow}`,
   matched exhaustively in `authority::supported_publication` (with `ToolCheck::Publish` for the
   root's fresh snapshots and `ToolCheck::Run` for stored children, admission replay and
   bindings), `resources::check_readiness`, `agent_preparation::targets`,
   `agent::prepared_tool` and `message::is_trusted_message_snapshot`.
4. **Agent grants** (`agent_publication::granted_tool`): the company's workflow registration is
   read first; a captured same-name snapshot is reused only if it is the same workflow pin,
   otherwise refused as ambiguous (F022); a same-name ready HTTP operation is refused
   ("registered both as a workflow and as an HTTP operation").
5. **Compiler**: dependency facts gained `tool_workflows` (`CatalogueFacts.tool_workflows`,
   `compile_resolved`/`check_resolved_budget` parameter) so the compiler's dependency graph
   matches the publication's, whose root edges include tool pins; recursion through a tool is
   `publication.recursion`; `publication.unused`/`missing` count tool pins.
6. **Causality**: `ChildCause::ToolCall(ChildToolCallRef { execution, call: ToolCallId })`
   replaces `ChildCause::Action(ActionRef)`, which no runtime writer created (`ActionRef` stays for
   message causes); the admission source key's slot 4 is the call UUID.
7. **Dispatch** (`workflow_tool_calls.rs`) follows D2.3 steps 0–8 in one transaction; the park
   refusal label is "tool park". `call_on` is `pub(super)` only so the SQL-guard test can hold
   the transaction open.
8. **Wait selection** (`waits::row`): a *completed* tool-call wait never answers execution
   resume/replay (it continued its job); a failed or cancelled one does, so an
   `invalid_output` replay answers `Failed { Replayed }` like a B1 step child.
9. **Child configuration**: the call's child reuses B1's `children::child_binding` (frozen params,
   the parent's bound resources for the child's slots) — runtime slot passing is covered by
   `child_configuration_tests`; B2 tests the publication rule for slots.
10. **F022 test vehicle**: the same-name captured step tool is a slot-bound HTTP step selection
    (the same `([tool], Some(registration))` arm an MCP slot takes); an MCP selection would need a
    live MCP discovery fixture.
11. **Revoked dependencies**: a new admission or binding activation of a workflow whose tool was
    revoked (or re-pointed) is the existing `NotFound` of an unavailable dependency.
12. **Codes**: the child outcome codes are one set in `tools.rs` (`CHILD_FAILED`,
    `CHILD_CANCELLED`, `CHILD_INVALID_INPUT`), used by a step child's failure (`child_waits`, whose
    private copies were removed) and a tool call's typed error; tool-only codes are `TOOL_REVOKED`,
    `TOOL_LIMIT`, `TOOL_RESULT_TOO_LARGE`.

New test modules (filters): `message_tests::workflow_tool_tests::` (6),
`message_tests::workflow_tool_registration_tests::` (3),
`message_tests::workflow_tool_revocation_tests::` (3), `message_tests::workflow_tool_race_tests::`
(5), `publication::tests::workflow_publication_freezes_workflow_tool_pins_like_children` (1);
support `workflow_tool_support.rs`. The user's company requirement is
`workflow_tool_pins_and_grants_stay_within_the_agents_company`.

## B2 corrections for WF-P07-R011 (2026-10-22, A)

R011 (B): changes required — F027 (Low), F028 (optional, three items). All handled in one round:
- **F027** — `workflow_tool_stored_children_keep_their_frozen_pin_until_the_tool_is_revoked` now
  goes past publication: after the re-pin it binds and admits the new outer parent (Run mode over
  the stored child at admission), parks it on the stored agent child (`park_child`), runs the
  child's agent with a scripted model so the tool call is dispatched **from inside a child run**
  (dispatch reads the child run's own saved binding), asserts the grandchild runs the frozen
  `setup.child` (not v2), finishes it, continues the child's agent job (`resume_wait` →
  `Continued`), and replays the same call to the child output (model called exactly twice, one
  grandchild). The revoked case is unchanged.
- **F028.1 adopted** — `HttpOperation::validate` refuses a snapshot carrying `tool.workflow`
  (`application/workflow/http.rs`); tested in the publication-rules test (registration of a
  pinned HTTP operation → `BadRequest`). Its callers are only HTTP operation paths (register,
  stored-snapshot reads in `http.rs`, `http_publication` capture); stored HTTP snapshots never
  carry the field.
- **F028.2 adopted** — the acceptance test reads the tool-call child's `head()` back and asserts
  `TriggerSource::Child { parent: ChildCause::ToolCall(..) }` with the wait's call id and the
  parent execution (exercises the `inspection.rs` decode arm).
- **F028.3 not adopted** — B accepted note 9 (shared `child_binding`, covered by B1).
- Test helpers `kind()`/`saved_result()` moved into `workflow_tool_support.rs` (shared).

## Checks

Baseline on the unchanged tree (2026-10-22, phase cluster `127.0.0.1:55470`,
`CARGO_INCREMENTAL=0`), digest at `2d608f2` `31c52a4a…1b21`:
- `cargo test --lib --no-run`: ok (2m 21s), `logs/baseline-build.log`.
- `DATABASE_URL=postgres://workflow07@127.0.0.1:55470/workflow07 cargo test --lib workflow`:
  1109 passed, 1 failed, 1604 filtered (370.8 s), `logs/baseline-workflow-tests.log`. Failure:
  `lease_resolution_tests::workflow_lease_resolution_stall_beyond_deadline_unknown_then_one_expiry`
  (`elapsed >= persistence_deadline` timing assertion at `lease_resolution_tests.rs:399`). It
  passed 3/3 when rerun alone. Pre-existing and load-sensitive on unchanged code, so unrelated
  to p07; recorded as a baseline flake and not a regression signal.

B1 (2026-10-22, same cluster; `SQLX_OFFLINE=true CARGO_INCREMENTAL=0`,
`DATABASE_URL=postgres://workflow07@127.0.0.1:55470/workflow07`). Intermediate focused runs:
`logs/b1-run1..3.log` (superseded). Final B1 evidence on digest `1e1b0a03…c9e4`:
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --locked --all-targets -j 6 -- -D warnings`: exit 0, `logs/b1-clippy.log`
  (first run found 3 lints — `children.rs` clone-on-copy, `causality.rs` unnecessary closure,
  test-support needless borrow — fixed).
- `cargo test --lib -j 6 workflow` (same selection as the baseline): **1137 passed, 0 failed**,
  1604 filtered (525 s), `logs/b1-workflow-tests.log`. Per-filter counts (plan table): every
  baseline filter selects its baseline count, all ok (53/30/37/25/4/5/17/5/29/33/32/31/10;
  `fixtures` 12 vs 10). New modules: `child_runtime` 11, `child_repeat` 4, `child_schema` 2,
  `child_publication` 3, `rerun` 4; `domain::workflow::causality::tests::child_ordinals…` ok.
  The baseline flake passed in this run.
- `scripts/stack-budget.sh -- child_ control_tests:: wakeup_tests::` (2048 KiB): 102 passed,
  exit 0, `logs/b1-stack-budget.log` — covers `child_waits::resume_on`, cancel recursion
  (`cancel_tree_on`) and the worker arm (`workflow_call_runs_through_the_worker_and_fair_poll`).
- Fresh migration: `psql -v ON_ERROR_STOP=1 -f migrations/20260817000000_init_schema.sql` into a
  throwaway `workflow07_migcheck` (dropped after): exit 0; new constraints/triggers present
  (`workflow_wait_child_link` and `workflow_child_parent_wait` deferrable/deferred),
  `logs/b1-migration-fresh.log`. Every DB-backed test also migrates its own fresh database.
- **Unavailable:** `cargo sqlx prepare --check -- --all-targets` (`sqlx-cli` not installed on this
  machine; CI installs 0.8.6). Evidence it cannot be stale from B1: no `.sqlx` entry and no
  compile-time macro query (`agent.rs`, `company.rs`, `company_invite.rs`, `user.rs`) names a
  workflow table, and the migration diff touches only `workflow_runs`, `workflow_waits`,
  `workflow_run_parents` references and workflow functions/triggers.
- Snapshot: `logs/b1-snapshot.txt`, base `2d608f2`, digest
  `1e1b0a032b4d93b16d2a09774ab5a13cb925741860d16358f334f2698f5ec9e4` (exact exclusions above;
  83 changed paths). Coordination r3 rechecked (sha256 `0c58cfd3…521a`, unchanged).

R005 correction round (2026-10-22), digest
`d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11` (`logs/r006-snapshot.txt`,
87 changed paths, same exclusions; coordination r3 unchanged):
- `cargo fmt --all -- --check`: exit 0 (`logs/r006-fmt.log`).
- `cargo clippy --locked --all-targets -j 6 -- -D warnings`: exit 0 (`logs/r006-clippy.log`).
- `cargo test --lib -j 6 workflow`: 1143 passed, **1 failed**, 1604 filtered (`logs/r006-workflow-tests.log`).
  Failure: `…control_serialization_tests::workflow_action_reconciliation_control_serialization_command_before_expire`,
  `action_reconciliation_control_serialization_lock_support.rs:284`: a `pg_stat_activity` check
  that the owner/observer backends have no active transaction yet. Evidence it is load-sensitive
  timing outside the changed code: it passed in the B1 full run (`logs/b1-workflow-tests.log`)
  and at baseline; this round changed no action-reconciliation code; `control_serialization_tests::`
  alone 4/4 runs (6/6 each) and the whole `action_reconciliation` group under parallel load
  241/241 (`logs/r006-serialization-rerun.log`). Recorded as an unrelated load-sensitive
  failure; the full selection is **not** claimed green for this round.
- Counts: new modules `child_configuration` 2, `child_decision_round` 1, `child_output` 2,
  `child_schema` 2, `rerun` 4, `child_runtime` 11, `child_repeat` 4, `child_publication` 3;
  `fixtures` 13, `registry` 33; baseline filters unchanged (53/30/37/25/…).
- `scripts/stack-budget.sh -- child_ control_tests:: wakeup_tests::` (2048 KiB): 108 passed, exit
  0 (`logs/r006-stack-budget.log`), including the worker-driven decision-round test.
- `cargo sqlx prepare --check`: still unavailable (no `sqlx-cli`); no SQL in macro-checked files
  changed.

B2 (2026-10-22, same cluster and environment), digest
`201af80cd15074697362d56fe58ac51e6b070ca25bff09ea91b1737846241f98` (`logs/b2-snapshot.txt`, base
`2d608f2`, 114 changed paths, same exclusions; coordination r4 sha256 `89563c7e…0f5b`):
- First `cargo test --lib -j 6 workflow` (before the last fixes): 1159 passed, 3 failed. (a)
  `message_tests::agent_tests::workflow_agent_message_tool_parks_on_delivery_and_continues_with_its_receipt`:
  **a B2 regression** — `granted_tool`'s classifier arm no longer reused a captured built-in
  `message.*` snapshot ("conflicts with another tool"); fixed (`ToolKind::Message | ToolKind::Http`
  reuse, as before B2). (b) two `credential_rotation::tests::http_tests::` tests:
  `VersionMismatch(20260817000000)` — the shared `workflow07_test` database still held the
  pre-B2 init-migration checksum; reset (dropped/recreated on the phase cluster, per the
  no-compatibility rule) and both pass.
- `cargo clippy --locked --all-targets -j 6 -- -D warnings`: exit 0 (`logs/b2-clippy.log`; one
  `nonminimal_bool` in dispatch step 3 fixed first).
- `cargo fmt --all -- --check`: exit 0 (`logs/b2-fmt.log`).
- `cargo test --lib -j 6 workflow` (final, frozen tree): **1162 passed, 0 failed**, 1604 filtered
  (383 s), `logs/b2-workflow.log`. Includes the 18 new tests (`workflow_tool_tests` 6,
  `workflow_tool_registration_tests` 3, `workflow_tool_revocation_tests` 3,
  `workflow_tool_race_tests` 5, the pure publication test 1).
- `scripts/stack-budget.sh -- child_ control_tests:: wakeup_tests:: workflow_tool` (2048 KiB): 126
  passed, exit 0 (`logs/b2-stack-budget.log`) — covers dispatch (`call_on`/`park_on`), the
  `child_waits::resume_on` tool branch and `continue_call`, the agent runtime replay.
- Fresh migration into a throwaway `workflow07_migcheck` (dropped after): exit 0;
  `workflow_tool_registration_pin`, `workflow_wait_child`, `workflow_wait_child_tool_call` and
  deferred `workflow_tool_child_wait_continuation`/`workflow_fenced_completion_guard`/
  `workflow_child_parent_wait` present (`logs/b2-migration-fresh.log`).
- **Unavailable:** `cargo sqlx prepare --check` (no `sqlx-cli`); B2 adds only runtime
  `sqlx::query` strings, none in macro-checked files. `graft build` unavailable (`graft` not on
  PATH).

R011 correction round (2026-10-22), digest
`16e65cb8170fea84ca86a10ffb8341f1b903d519c11305e73fa78a1d8f4faee9` (`logs/r012-snapshot.txt`, 115
changed paths — `application/workflow/http.rs` added; same exclusions; coordination r4 unchanged):
- `cargo fmt --all -- --check` exit 0 (`logs/r012-fmt.log`); `cargo clippy --locked --all-targets
  -j 6 -- -D warnings` exit 0 (`logs/r012-clippy.log`).
- Focused affected set `cargo test --lib -j 6 -- http agent_ workflow_tool`: 610 passed, 0 failed,
  1 ignored (pre-existing `#[ignore]` developer aid `http::pages::tests::dump_application_javascript`)
  (`logs/r012-focused.log`). Selection traced from the delta: `HttpOperation::validate` callers
  (HTTP registration/runtime/publication, credential rotation, agent HTTP tools) and the B2 tests.
- `scripts/stack-budget.sh -- child_ control_tests:: wakeup_tests:: workflow_tool` (2048 KiB): 126
  passed, exit 0 (`logs/r012-stack-budget.log`), including the nested child-run call (F027).
- The full `workflow` selection was not rerun for this delta (B2 run `logs/b2-workflow.log`
  1162/0 on `201af80c…`); unchanged code outside the traced callers.

Final phase gate (2026-10-22) on the accepted tree, digest
`16e65cb8170fea84ca86a10ffb8341f1b903d519c11305e73fa78a1d8f4faee9` (recomputed before the runs):
- `cargo fmt --all -- --check`: exit 0 (`logs/r012-fmt.log`, same tree).
- `SQLX_OFFLINE=true cargo check --locked --all-targets`: exit 0 (`logs/final-check.log`).
- `cargo clippy --locked --all-targets -- -D warnings`: exit 0 (`logs/r012-clippy.log`, same tree).
- Fresh migration: exit 0 (`logs/b2-migration-fresh.log`; the init schema is unchanged since).
- `cargo test --lib -j 6 workflow`: **1162 passed, 0 failed**, 1604 filtered (381 s),
  `logs/final-workflow.log`. Baseline filters all select at least their baseline counts.
- `scripts/stack-budget.sh -- child_ control_tests:: wakeup_tests:: workflow_tool` (2048 KiB):
  126/0 (`logs/r012-stack-budget.log`, same tree).
- **Unavailable:** `cargo sqlx prepare --check` (no `sqlx-cli`; p07 adds no compile-time
  macro query, B confirmed in R011) and `graft build` (`graft` not on PATH).
- **Outstanding:** the repository's full test suite (`cargo test` over everything, AGENTS.md's
  completion requirement) has not been run; it runs only on the user's request.
- Combined integration: B's R011/R012 reviewed B2 over the combined B1+B2 tree (B1 files B2
  changed — `children.rs`, `child_waits.rs`, `waits.rs`, `lease.rs`, `inspection.rs`,
  `admission_binding.rs` — were in scope, and B ran the `child_` suites: 1094/0 in
  `logs/r011-review-checks.log`), so no combined effect is left unreviewed.

Full suite (2026-10-22, user request "run test suite" then "fix any issues"), CI's step:
`scripts/test-network-isolation.sh cargo test --locked --offline --all-targets` (macOS sandbox,
loopback only; phase cluster, `workflow07_test` reset earlier this session):
- Run 1 (tree `16e65cb8…aee9`): lib 2743 passed, **1 failed**, 22 ignored (`logs/full-suite.log`);
  cargo stopped before the bin target. Failure:
  `control_serialization_tests::workflow_action_reconciliation_control_serialization_command_before_cancel`
  at `action_reconciliation_control_serialization_lock_support.rs:284` — the same instantaneous
  `pg_stat_activity` "owners drained" check that failed (as `…command_before_expire`) in the R006
  full run. Cause (code reading, not reproduced): an owner's pooled connection is released
  asynchronously after the owner's future returns — sqlx's release task pings the backend, which
  is when a dropped transaction's queued `ROLLBACK`, or a statement abandoned by a timed-out
  bounded attempt, is drained — so under full-suite load the check can run before the release.
  **Fix** (test-only, pre-existing support file outside B1/B2): `inspect_finished_owners` now
  polls within the existing `LOCK_WAIT_HANG_GUARD` (10 s) until the owners and observer are
  quiet, reporting the busy rows on failure and the first busy observation on success; a real
  leaked transaction or lock still fails it. Evidence: `action_reconciliation` 241/241 ×4 under
  load with no busy observation (`logs/full-serialization-fix.log`); a throwaway probe (dropped
  transaction, immediate `pg_stat_activity` read) saw 0/200 open on an idle machine — the race
  needs load; the probe was removed.
- Run 2 (with the fix, `--no-fail-fast`): lib **2744 passed, 0 failed**, 22 ignored (= the 22
  `#[ignore]` in `src`); bin 5 passed (`logs/full-suite-2.log`). fmt check exit 0.
- CI's remaining test gates on the same tree: `scripts/stack-budget.sh --offline` (whole suite at
  2048 KiB) 2744 passed, 0 failed, 22 ignored (`logs/full-stack-budget.log`);
  `scripts/lease-budget.sh --offline` 130 passed, every operation's max code-owned work within
  its bound (largest: `budget.reserve` 352/1000 ms, `lease.renew` 306/2000 ms), exit 0
  (`logs/full-lease-budget.log`).
- Tree with the fix: digest `af49b63b765cc6e1d54d0005c805023a3ead87c31ea092aa69ce6809d403f2cb`
  (`logs/r013-snapshot.txt`, 116 paths; the only new path is the lock-support test file).
  **WF-P07-R013: PASS** at that digest (B confirmed the cause from sqlx-core 0.8.6 and judged the
  bounded drain not to weaken the test; residual risk noted, not a finding: the instantaneous
  `RunProbe::Free` probe would share the release delay only if an owner dropped a transaction
  holding the run lock, which nothing suggests today). Root recomputed the digest: matching.

## Status / next action

B2 implemented and frozen at `201af80c…1f98`; R011: changes required (F027 Low, F028 optional),
corrected in one round at `16e65cb8…aee9`. **WF-P07-R012: PASS** for B2 at
`16e65cb8170fea84ca86a10ffb8341f1b903d519c11305e73fa78a1d8f4faee9`. **B2 accepted in phase** by
root (2026-10-22): digest recomputed and matching, coordination r4 unchanged (sha256
`89563c7e…0f5b`; WF-C001 r2/WF-C002/WF-C003 target p08/p09, none p07), R011/R012 notes,
findings and REVIEW rows saved, F001–F028 all fixed/closed/adopted. Not committed. Next: final
phase gate: **passed** (Checks, "Final phase gate"). Phase p07 implemented and accepted.
The user then asked for the full suite and to fix any issues: full suite green after a test-only
fix (R013 PASS), including CI's stack- and lease-budget gates (Checks, "Full suite"). Accepted
tree: `af49b63b765cc6e1d54d0005c805023a3ead87c31ea092aa69ce6809d403f2cb`. **Committed** at the
user's request as `bd2ef36` on `workflows` and pushed to `origin/workflows` (2026-10-22); its
source is exactly the accepted tree (no edits between R013 and the commit). Not integrated into
`main`. `cargo sqlx prepare --check` was unavailable here (no sqlx-cli; CI runs it).


Session 2: gate P07-D1 passed (R004). B1 batch review R005: changes required (F013–F016,
F017 optional); corrected in one round. B round **WF-P07-R006: PASS** for B1 at digest
`d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11` (deviations 1–8 accepted,
F013–F017 closed, the one full-run failure judged unrelated/load-sensitive).
**B1 accepted in phase** by root (2026-10-22): digest recomputed and matching, coordination r3
unchanged (sha256 `0c58cfd3…521a`, WF-C001/C002 active, none target p07), review notes,
findings and REVIEW row saved, no open actionable findings. Not committed and not integrated;
outstanding for the phase: B2, the final phase gate (incl. `cargo sqlx prepare --check`, still
unavailable here) and the full suite (on user request). Next: the B2 product question to the
user, then P07-D2.
