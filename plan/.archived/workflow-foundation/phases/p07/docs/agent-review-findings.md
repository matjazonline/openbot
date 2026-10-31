# P07 agent review findings

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Reviewer-owned log (B). One section per finding; resolution history is appended, never rewritten.
Review rounds are in `agent-review-notes.md`.

Common context for WF-P07-F001…F007: date 2026-10-21; workflow `implement-opus-reviewed-2_2`;
task `TASK.md`, review `REVIEW.md`; batch B1; stage architecture gate; round `WF-P07-R001`;
reviewer Claude Opus 5.5 (`claude-opus-5-5`), independent fresh-context reviewer, effort as
configured by the spawner; phase `p07`; worktree `/Users/mac03/dev/openbot` (branch `workflows`);
examined decision `P07-D1` revision 1 (`logs/decision-D1.md`, sha256
`7c3245e7b53867b47ce70750bf96cdb5503e5b298eb95b0490d091bb7e8f3c92`) against code base
`921079d` (digest `e2714983beb52a28eb0432b58be69d6652ea56a6bc0a07566918118138c2e3fc`, exclusions
`plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`).
All findings originate in the gated decision (no implementation exists yet).

## WF-P07-F001 — Child runs capture a fresh thread-history snapshot (implicit, later context)

- Classification: **actionable defect**, severity **High**, category requirements / security
  (data isolation) / scalability.
- Evidence:
  - Decision D1.2 step 4 (`logs/decision-D1.md:52-58`): the child copies the parent's
    channel/thread association and then runs "thread-history capture … reusing
    `admission_write` helpers".
  - `admission_history::capture` (`src/adapters/persistence/workflow/admission_history.rs:7-38`)
    snapshots *current* `thread_messages` membership (up to 10,000 rows) for any thread-associated
    admission; `history_reader` (`history_reader.rs:1-4,71-80`) serves `context.history` from the
    run's own `workflow_history_members`.
  - Observed: every child run (every `flow.repeat` round) would get a new snapshot taken at child
    creation, possibly hours after the root's admission, including later customer messages that
    started other independent runs and their replies. A child `context.history` step therefore
    sees conversation content the parent never had, and different rounds see different histories.
  - Required: plan 07 §Child calls — "The child receives only its explicit inputs and allowed
    runtime resource bindings, not the parent's entire context"; README — every message has an
    independent run and context; 06-D6 A — history is the frozen admission snapshot.
  - Impact: cross-run context leakage into child prompts, non-deterministic per-round context,
    and up to 10,000 membership inserts per round (×1000 rounds) inside the no-lease park/resume
    transition that runs under a 2 s timeout and the 1000 ms `statement_timeout` set by
    `waits::lock` (`waits.rs:102`), which feeds WF-P07-F002's poison risk.
- Required change: state explicitly that runtime children capture **no** history membership
  (history reaches a child only as explicit input), or that they share/copy the root's frozen
  membership — never a fresh capture — and add a test that a message appended to the thread after
  the parent's admission is invisible to a child `context.history`.
- Instruction lesson: observation — the decision reused an admission helper wholesale without
  re-deriving which admission side effects match child semantics. Candidate (phase-plan scope):
  "When a runtime path reuses an external-admission writer, list each side effect it inherits
  (history, replay commands, readiness, authority) and justify it against the target semantics."
- Resolution history:
  - 2026-10-21 (R001): open.
  - 2026-10-22 (R002, decision rev 2 sha256 `e5ceea98…8c93`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **fixed** (decision level). D1.2 side-effect table: runtime children capture no
    history membership; D1.10 adds the late-message invisibility test. Verified in code that only
    `admission_history::capture` writes `workflow_history_members` (sole non-test writer) and that
    `history_reader::frozen_history` returns `eligible 0` with no members (no "snapshot missing"
    error path), so an empty child history is a valid state; the unbounded ≤10,000-row insert
    leaves the transition. Implementation evidence (test) still to be checked in batch review.

## WF-P07-F002 — No failure classification for deterministic child-creation errors (poison/liveness)

- Classification: **actionable defect**, severity **Medium**, category correctness / liveness.
- Evidence:
  - D1.2 (`decision-D1.md:46,55-56,63`) settles only `InvalidInput` (activation errors and
    `ConfiguredBinding::new`); "any failure rolls back child and wait together" otherwise.
    D1.4 round creation inside `resume` likewise names only `next_input` invalidity and budget
    exhaustion.
  - `park_child` is specified as a pure transition without lease or attempt accounting (like
    `park_wait`, `waits.rs:26-35`). A rollback leaves the parent job `pending`, so
    `CANDIDATES_SQL` (`polling.rs:98-…`) re-offers it as `job` work on every poll; a failed
    round transition leaves the `child_run` wait `waiting` with a wakeup fact, so it is re-offered
    as `wait` work every poll. Nothing increments `retry_count`, so the run is stuck until its
    deadline (up to 24 h) and each candidate occupies a poll page slot.
  - Deterministic failure sources inside the new transaction that are not `InvalidInput`:
    history-membership overflow (`AppError::BadRequest` from `admission_history::capture`, see
    F001), row CHECK violations on the child run/wait (e.g. `workflow_runs_*_check`,
    `workflow_wait_deadline`), statement timeouts on large bundle/history writes under the 1000 ms
    `statement_timeout`, and child-bundle restore/validation errors.
  - Required: `src/application/AGENTS.md` "Keep failure states distinct" — parked states need a
    terminal transition, and pending work must not let a fixed set of poison rows monopolize the
    poll; a poison-batch regression test is required for retryable worker queues. Existing park
    paths classify their deterministic failures (e.g. `PendingFailure::ReviewerUnavailable`).
- Required change: enumerate the park/round-creation failure classes and their settlement
  (Terminal code routed or run failed vs. transient retried with bounded backoff), keep
  storage/timeout errors observable, and add poison tests for both park and round transition
  (two consecutive poll iterations do not re-offer the same settled candidate).
- Instruction lesson: existing AGENTS.md guidance covers this; the decision did not apply it to
  the new transition. No new instruction supported.
- Resolution history:
  - 2026-10-21 (R001): open.
  - 2026-10-22 (R002, decision rev 2 sha256 `e5ceea98…8c93`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **fixed** (decision level). D1.9 classifies every park/round failure; deterministic
    classes settle through existing writers (`pending_recovery::settle`, `wait_commit::fail`,
    `recovery::route_parked_failure`), round-budget exhaustion gets an explicit failing writer, and
    unclassified errors propagate. Verified: `fair_polling::discover` advances a per-company
    `after_job` cursor and resets `horizon` only at the end of a pass, so a failed candidate is not
    returned on the immediately following iteration; `WorkflowWorker::run` logs each failed
    candidate with company/run and always sleeps before the next poll (no hot loop); the run
    deadline is a terminal transition via the `deadline` candidate class. D1.10 adds park and round
    poison tests per `src/application/AGENTS.md`.

## WF-P07-F003 — Rerun-with-link requirement deferred without an agreed disposition

- Classification: **actionable defect**, severity **Medium**, category requirements / scope.
- Evidence:
  - Plan 07 §Retry versus revision (authoritative) row 3: "Rerun with changed definition/input |
    New top-level run, linked to the original for inspection". TASK criterion 4 (B1 scope)
    includes "Retry vs revision vs rerun semantics as tabled".
  - D1.7 (`decision-D1.md:140-143`) defers the link to phase 08/09 as a "recorded disposition",
    but no coordination notice exists (`COORDINATION.md` r1: "Cross-phase notices: None"), the
    phase 08/09 plans do not mention rerun links, and no user/plan agreement is recorded.
  - Nothing forces the deferral: `WorkflowService::admit` (`src/application/workflow/service.rs:45-75`)
    with `TriggerSource::Manual` already admits top-level runs; a validated, same-company,
    access-checked `rerun_of` link persisted with the run and exposed by inspection fits B1.
- Required change: either specify the link (identity shape, FK/tenant constraint, authorization
  of the original, inspection exposure, test) in D1, or obtain and record an explicit agreed
  disposition (user decision and/or plan change plus a coordination notice to the owning phase)
  before treating criterion 4 as satisfied.
- Instruction lesson: candidate (skill scope, `implement-opus-reviewed-2_2`): "Moving a stated
  phase requirement to a later phase needs an agreed disposition recorded in the coordination
  file and the receiving phase plan; a note in the decision alone is not a disposition."
- Resolution history:
  - 2026-10-21 (R001): open.
  - 2026-10-22 (R002, decision rev 2 sha256 `e5ceea98…8c93`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **fixed** (decision level). D1.7/D1.3 build `rerun_of` in B1: Manual-only, same
    company, top-level, same `workflow_id` (FK `(company_id, rerun_of_run_id, workflow_id)` →
    new UNIQUE `(company_id, id, workflow_id)`), original-association authorization with
    non-disclosing `NotFound`, recheck under `FOR SHARE`, immutable, replay-compared, exposed as
    `RunHead.rerun_of`; entry points carried to p08 by WF-C001, display to p09 by WF-C002.
    Verified: `TriggerSource::Manual` exists and is admissible (`admission_source.rs:8`),
    `inspection::HEAD` already selects `workflow_id`, no code deletes individual `workflow_runs`
    rows and all 30 FKs into it cascade from company deletion, so a NO ACTION self-FK is
    compatible. The admission-side `FOR SHARE` follows the established authority → run order of
    the deleted child path. One verification gap split out as WF-P07-F009.

## WF-P07-F004 — Phase-03 binding-based child admission left in place contradicts phase-07 child invariants

- Classification: **actionable defect**, severity **Medium**, category architecture / security.
- Evidence:
  - D1.2 (`decision-D1.md:72-75`) keeps `TriggerSource::Child` + `ChildCause::Execution`
    admission through `WorkflowService::admit` "unchanged and unused by the runtime".
  - That path (`admission.rs:10-46`, `admission_source.rs:87-130`, `admission_write.rs:29-67`)
    admits *any* active binding as a child of any visible parent execution: the child workflow is
    chosen at runtime (bypassing the publication-frozen dependency graph and recursion rejection),
    uses the target binding's params/resources (a "more privileged workflow" binding), receives
    the admission-time `ADMISSION_DEADLINE_SECONDS` deadline instead of the parent's
    (`admission_write.rs:63`), and has no parent wait, so its `parent_wakeup` fact is never
    consumed. It still links into the root budget and would be swept by D1.5's
    `parent_run_id` cascade.
  - Required: plan 07 — children inherit deadlines; "They cannot gain access by selecting a more
    privileged workflow"; "Freeze the dependency graph at publication and reject recursion";
    AGENTS.md — no compatibility paths kept for old semantics. `WorkflowService::admit` is the
    public admission API that phase 08 wires for manual/delegation starts.
  - Mitigating fact: no production caller constructs `TriggerSource::Child` today (only tests),
    so impact is latent, but B1's acceptance claims would be true for one child path only.
- Required change: refuse `TriggerSource::Child` at `WorkflowService::admit`/admission (or remove
  the variant's admission branch) in B1 and move the fixtures that use it
  (`budget_race_tests`, wakeup tests) to runtime children; or define in D1 exactly which
  invariants that path must also satisfy and gate it until phase 08 specifies delegation.
- Instruction lesson: observation — "left unchanged and unused" was treated as neutral. Candidate
  (phase-plan scope): "When a phase defines the semantics of an entity, every creation path for it
  must satisfy them or be refused; unused-but-reachable APIs count as paths."
- Resolution history:
  - 2026-10-21 (R001): open.
  - 2026-10-22 (R002, decision rev 2 sha256 `e5ceea98…8c93`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **fixed** (decision level). D1.2a refuses `TriggerSource::Child` in
    `WorkflowService::admit` and `admission_source::authorize`, deletes the child branches and the
    parent columns of `admission_write::insert_run`, leaving `children::create_on` as the single
    writer; `check_workflow_parent_source` additionally rejects a `child` source key whose run has
    no matching parent. Verified the deleted branches exist as described (`admission.rs:26-39,
    67-82`, `admission_write.rs:30-38`, `admission_source.rs:14-21,87-121`). The fixture-move
    inventory is incomplete (four files reach the removed path through `admit_child`): split out
    as WF-P07-F008; it does not reopen the architectural fix.

## WF-P07-F005 — Explicit phase-03 obligation on control retry of children not addressed

- Classification: **actionable defect**, severity **Low**, category requirements / recovery.
- Evidence: `src/adapters/persistence/workflow/control_retry.rs:8-9` — "A child has already
  published an immutable parent terminal fact. Phase07 must define how a retry reconciles that
  fact before children can reopen." `workflow_control_retry_safe` also excludes any admission
  whose source key starts with `child`. D1.7 (`decision-D1.md:136-143`) discusses technical retry
  only as park/resume replay and never mentions operator retry of a runtime child or of a parent
  whose call/repeat failed through `workflow.child_failed` (whose parked job is closed by
  `route_parked_failure` without a failed attempt, hence also never retry-safe).
- Required change: record the phase-07 disposition (e.g. operator retry stays refused for
  runtime children and for parents failed by a child, with the rationale that the parent fact is
  immutable and a revision/rerun is the recovery path), update the stale comment, and add a test
  that `control retry` on a runtime child returns `unsafe`.
- Instruction lesson: candidate (phase-plan scope): "Search the code base for comments that
  assign work to the current phase (e.g. `Phase07`) during investigation and list each in the
  gate."
- Resolution history:
  - 2026-10-21 (R001): open.
  - 2026-10-22 (R002, decision rev 2 sha256 `e5ceea98…8c93`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **fixed** (decision level). D1.7 keeps operator retry refused for runtime children
    (`retry_on`'s `TriggerSource::Child` arm; `workflow_control_retry_safe` keeps the `child`
    source-key exclusion, and runtime keys keep `"child"` as element 0) and for child-failed
    parents; the stale comment is replaced; D1.10 adds both refusal tests. Verified the rationale:
    `recovery::route_parked_failure` closes the parked job `completed → failed` without an attempt
    row and `pending_recovery::settle` closes a pending job without an attempt, while
    `workflow_control_retry_safe` requires a failed attempt `attempt_ordinal-1` with
    `retry_count > 0` and no `exhausted` budget receipt, so neither parent shape can be retried.

## WF-P07-F006 — Cancellation cascade lookup is unindexed under a 1 s statement timeout

- Classification: **actionable defect**, severity **Low**, category availability / concurrency
  verification.
- Evidence: D1.5 (`decision-D1.md:116-121`) selects children by `parent_run_id = run`. The only
  `workflow_runs` keys lead with `(company_id, id …)` (`init_schema.sql:9490-9563`); there is no
  index on `parent_run_id`, so the cascade scans the tenant's runs per level (≤ 32 levels).
  `control()` sets `lock_timeout` and `statement_timeout` to 1000 ms (`controls.rs:70`), so cancel
  can fail persistently for a large tenant. The live descendant of a parent is exactly the child
  named by its single `waiting` `child_run` wait, reachable through the existing
  `workflow_wait_one_active` index — which is also the precise cascade set once F004 removes
  stray children. The verification plan lists "parent cancel … competes with child completion"
  but not cancel racing a round transition / park that creates a child (a new concurrency
  protocol per AGENTS.md "competing claimants").
- Required change: specify a bounded, index-supported child lookup (via the waiting child wait,
  or a justified index) and add a competing test of cancel versus concurrent child creation
  (park and round transition) proving no live orphan child remains.
- Instruction lesson: no new instruction supported (AGENTS.md already requires competing
  claimant tests and bounded work).
- Resolution history:
  - 2026-10-21 (R001): open.
  - 2026-10-22 (R002, decision rev 2 sha256 `e5ceea98…8c93`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **fixed** (decision level). D1.5 finds the live child through the parent's
    waiting `child_run` wait; `workflow_wait_one_active` is a unique partial index on
    `(company_id, run_id, execution_id) WHERE state='waiting'`, so `company_id, run_id,
    state='waiting'` uses its prefix, and NOT_PLANNED (no parallel branches) makes one live
    execution per run, hence ≤ 1 row. Lock order (authority → parent run → executions/jobs → child
    run) checked against `controls::control`/`cancel_on`, `maintenance::retire_jobs` and
    `action_uncertainty::record` (no root-budget or parent locks) and the wakeup triggers
    (`schedule_workflow_parent_wakeup` takes no parent lock). `control()` runs READ COMMITTED, so
    the post-lock read sees a concurrently committed park/round. Competing cancel-vs-park and
    cancel-vs-round tests (both orders) are planned. Optional B2 robustness note in WF-P07-F010.2.

## WF-P07-F007 — Optional suggestions (non-blocking)

- Classification: **optional suggestion**, severity **Optional**, category design clarity.
- Items:
  1. Typed child errors: pass the child's terminal failure code (from the wakeup fact's terminal
     execution/run) as `StepFailure` detail so successors and inspection can distinguish why a
     child failed, rather than only `workflow.child_failed`.
  2. Child facts: `registry::ChildContract` (`registry/facts.rs:20`, built in
     `publication/dependencies.rs:56-61`) carries only input/output schemas; D1.1's params
     conformance and same-name resource checks need the child `parameter_schema` and resource
     declarations added to the contract (state this in D1.1 so the compiler change is scoped).
  3. Deferred repeat bindings: state that `/steps/<self>/output` in the `repeat` block always
     resolves to the provisional output, never to an earlier activation of the same step in a
     graph loop, and that the block's references are included in the step's input dependencies
     (`activation.rs` `input_dependencies`/`outputs`).
  4. Child runs carry the parent's `binding_id`; phase-09 binding run lists should filter
     `parent_run_id IS NULL` or label children so provenance is not misread as a binding run.
  5. Resources by name are acceptable; an explicit `with.resources` mapping would decouple
     reusable children from parent slot names. Consider recording why by-name was chosen.
  6. `exit_when` runtime evaluation errors should be classified like `next_input` errors
     (Terminal `workflow.invalid_input`, routed).
- Instruction lesson: none.
- Resolution history:
  - 2026-10-21 (R001): open (optional; does not block acceptance).
  - 2026-10-22 (R002, decision rev 2 sha256 `e5ceea98…8c93`, HEAD `2d608f2`, digest `31c52a4a…1b21`): dispositions accepted. Item 1 declined with recorded rationale (05-D2 error routes
    deliver `null`; diagnostic carries child id/state; typed results deferred to B2's tool
    contract) — acceptable for an optional item. Items 2, 3, 5, 6 adopted in D1.1/D1.9 (verified
    `ChildContract` today has only input/output schemas, `registry/facts.rs:20-25`, so the
    extension is needed). Item 4 carried to p09 as WF-C002 r1. Closed.

---

Common context for WF-P07-F008…F010: date 2026-10-22; workflow `implement-opus-reviewed-2_2`;
task `TASK.md`, review `REVIEW.md`; batch B1; stage architecture-gate correction recheck; round
`WF-P07-R002`; reviewer Claude Opus 5.5 (`claude-opus-5-5`), fresh replacement reviewer B (session
2), effort as configured by the spawner (TASK records opus/xhigh); phase `p07`; worktree
`/Users/mac02/dev/openbot` (branch `workflows`); examined decision `P07-D1` revision 2
(`logs/decision-D1-r2.md`, sha256
`e5ceea98409dc8a4e6dc5a48931305c57d99658877d718e87404a6e8d5738c93`) against code base HEAD
`2d608f269761717de7706912bd99ddd325d9d352` (digest
`31c52a4a554512444396a39b77d8b2b5142231232cf0ee3027a5d4855fc91b21`, exclusions
`plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`).
All findings originate in the gated decision / its verification plan (no implementation exists).

## WF-P07-F008 — D1.2a fixture-move inventory and B1 focused checks miss four files built on the removed child path

- Classification: **actionable defect**, severity **Low**, category verification / requirements
  coverage.
- Evidence:
  - D1.2a (`decision-D1-r2.md:126-138`) deletes binding-based child admission and lists the tests
    "that admitted children through bindings" to move to a shared runtime-child fixture. The list
    was evidently built from direct `TriggerSource::Child`/`ChildCause` uses (it matches that grep
    exactly).
  - The helper `admit_child` (`src/adapters/persistence/workflow/wakeup_tests.rs:8-41`) admits a
    child through `TriggerSource::Child { parent: ChildCause::Execution(..) }` and
    `persistence().admit(..)`, and is also called from four files absent from the list:
    `budget_activation_tests.rs` (lines 17-18, 96, 296-297, 354: e.g.
    `workflow_budget_activation_competing_descendants_replay_and_retirement`,
    `..._child_ignores_held_ancestor_locks`, `..._cancelled_owner_never_spends`),
    `budget_schema_tests.rs` (57-58, 104, 179), `wakeup_guard_tests.rs` (75, 100, 167, 180) and
    `wakeup_parent_link_tests.rs` (6).
  - Several of these build grandchildren (`admit_child(&f, child, ..)`), which on the runtime path
    needs a three-level published chain (grandchild → child with `workflow.call` → parent); the
    D1.2a fixture recipe describes only one level ("publish a child, then a parent").
  - TASK "Batch B1 verification plan" filters (`TASK.md:144-148`) include `budget_race` but not
    `budget_activation` or `budget_schema`, so the existing descendant-budget concurrency evidence
    for plan criterion "inherited budget enforcement" (plan 07 §Acceptance; TASK criterion 8) would
    not run during B1 even though its fixture changes semantics.
  - Impact: changed behaviour (child creation path) whose existing coverage could be silently
    rewritten, weakened or left unrun until the final gate; contrary to AGENTS.md "run the smallest
    meaningful set of tests covering each changed behavior and its affected interfaces" and the
    plan requirement that implementation plans specify per-batch focused checks.
- Required change: add the four files to the D1.2a move list (state the nested-publication fixture
  for grandchild cases) and add `budget_activation`, `budget_schema` (and explicitly
  `wakeup_guard`, `wakeup_parent_link`) to the B1 focused filters in TASK.
- Instruction lesson: observation — the inventory searched for the removed type name, not for
  helpers that wrap it. Candidate (phase-plan scope): "When removing a creation path, inventory
  affected tests through the helper call graph (grep each helper that uses the removed API), not
  only direct uses of the removed type."
- Resolution history:
  - 2026-10-22 (R002): open.
  - 2026-10-22 (R003, decision rev 3 sha256 `96d201af…e0b6`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **partially fixed; remains open (Low)**. Rev 3 adds the four `admit_child` callers,
    specifies a chain fixture (leaf published first, each ancestor a `workflow.call` entry step,
    `park_child` per level), keeps the raw-SQL negative inserts, and TASK lists the new filters.
    Still incomplete — the R002 evidence for this finding traced only `admit_child`, so the
    following were missed by both R002 and rev 3:
    (a) Other wrappers of the removed path have callers outside the inventory:
        `action_reconciliation_eligibility_tests.rs:49` `child` (TriggerSource::Child) is reached
        through nested modules in `action_reconciliation_owner_lifecycle_tests.rs:195`,
        `action_reconciliation_claim_race_tests.rs:37`, `action_reconciliation_claim_sql_tests.rs:68`,
        `action_reconciliation_claim_sql_wrong_execution_tests.rs:129`,
        `action_reconciliation_claim_boundary_tests.rs:192` and
        `action_reconciliation_claim_recovery_tests.rs:64,191`; `harness_token_refusal_tests.rs:4`
        `child`/`:51` `child_checkpoint` are reached from `harness_token_scheduling_tests.rs:60,105,
        155,213` and `harness_checkpoint_update_tests.rs:92`. (Complete transitive set checked:
        the other wrappers `unique_child`, `child_command` and `budget_race_tests::child` have no
        callers outside their own files.)
    (b) The TASK filters name files, not test paths. Checked against the 856 adapter test paths
        in `logs/baseline-workflow-tests.log`: every filter written with the
        `adapters::persistence::workflow::` prefix selects 0 tests (all live under
        `…::workflow::tests::…`); as bare substrings `budget_race`, `wakeup_guard`,
        `control_acceptance`, `control_guard` select 0 and `harness_token_refusal` 1 of 6. Module
        segments that do select the full inventory (counts from the log):
        `admission_tests::admission_token_tests` 4, `admission_tests::admission_source_tests` 5,
        `tests::admission_binding_tests` 5, `admission_tests::activation_tests` 17,
        `budget_runtime_tests::race_tests` 5, `batch_tests::wakeup_tests` 30 (includes
        `budget_activation_tests` 9, `budget_schema_tests` 7, `guard_tests` 8,
        `parent_link_tests` 3), `lease_tests::control_tests` 37 (includes `guard_tests` 3,
        `acceptance_tests` 17), `tokens::refusal` 6, `tokens::scheduling` 8, `tokens::updates` 3,
        `sibling_tests::eligibility_tests` 53 (includes claim fairness/race/sql/boundary/recovery/
        wrong-execution and owner lifecycle), plus application `causality_cases` and
        `source_validation`.
    (c) The chain fixture cannot reproduce the precondition of several moved cases, yet D1.10
        says every inventory file "runs on the runtime-child fixture". On the runtime path a
        child exists only under a parked `workflow.call`/`flow.repeat` execution, one live leaf
        per root. Cases that need concurrently live descendants or a child of a non-parked
        execution: `budget_activation` `competing_descendants_replay_and_retirement` (root, child,
        grandchild activate concurrently on a 2-activation root), `budget_schema`
        `competing_descendants_and_exact_duplicate_receipts`, `wakeup_guard`
        `sibling_progress_and_cursors_are_independent` (two live children of one execution),
        the `tokens::*` cases (child of an agent model execution: the B2 topology), and the
        eligibility claim cases (child of an execution holding a live I/O claim). These guard
        root-shared budget/token/reconciliation invariants that B2's concurrent tool-call
        children may make reachable again.
    Required change: (a) add the eight files; (b) state filters as real module-path segments
    (above) and record a non-zero selected-test count per filter in each run; (c) give D1.2a a
    per-case rule for cases whose precondition is unreachable in B1 — e.g. build the state with
    the single writer `children::create_on` plus a `child_run` wait (schema-reachable, satisfies
    D1.3's deferred constraint) labelled as anticipating B2, or move it to B2 with explicit
    tracking, or retire it with a recorded reason — never silently weaken it.
  - 2026-10-22 (R004, decision rev 4 sha256 `5c076606…adc4`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **fixed** (decision level). (a) D1.2a states the search method (constructors →
    every caller, including nested `#[path]` modules) and lists all seven wrappers and the 24
    persistence + 2 application files; it matches my R003 transitive set exactly (the text's
    "18 in persistence" is a miscount of its own complete 24-file list; editorial, F012.6).
    (b) TASK "Batch B1 verification plan" now uses real module-path filters; all 14 counts
    re-derived from the 1111 test paths in `logs/baseline-workflow-tests.log` match
    (53/30/37/25/4/5/17/5/29 and 33/32/31/10/10), and a filter selecting fewer tests than
    expected is defined as a failed check. (c) The A/B/C per-case rule with a per-test record in
    `logs/b1-fixture-moves.md` covering every affected baseline test resolves the reachability
    gap; every test name cited in the rule exists in the baseline log. The author's category-B
    claim was verified (all Rust and schema readers of `workflow_waits` traced, incl.
    `fairness.rs::VALID_HINT`, which reads only `state='waiting'`); precision caveats recorded
    as optional WF-P07-F012. Batch review must check the fixture-move table against the
    baseline list.

## WF-P07-F009 — Rerun link: no planned test for the non-disclosing refusal of an inaccessible original

- Classification: **actionable defect**, severity **Low**, category verification / security
  (authorization).
- Evidence: D1.7 (`decision-D1-r2.md:234-244`) introduces a new authorization decision: the actor
  must be authorized for the *original's* association (`WorkflowOperation::Admit`), "otherwise a
  non-disclosing `NotFound`", rechecked in the admission transaction. D1.10 Rerun tests
  (`decision-D1-r2.md:319-320`) cover unknown, cross-company, child and different-workflow
  originals and replay conflict, but not an existing same-company original in an association the
  actor cannot access (and that the result is indistinguishable from an unknown run). The new run
  may be admitted in a different channel than the original, so this is the only check preventing
  a caller from linking (and probing the existence of) runs in channels it cannot see.
- Required change: add that refusal case (same company, original in an inaccessible
  channel/thread; response equals the unknown-original response; nothing persisted) to D1.10.
- Instruction lesson: no new instruction supported (src/AGENTS.md authorization-path rules and
  persistence AGENTS.md tenant tests already imply it); observation only.
- Resolution history:
  - 2026-10-22 (R002): open.
  - 2026-10-22 (R003, decision rev 3 sha256 `96d201af…e0b6`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **fixed** (decision level). D1.10 now tests that an unknown or cross-company
    original and a same-company original in an inaccessible channel give the identical
    non-disclosing `NotFound` with no run/admission/command/event rows, plus non-`Manual` +
    `rerun_of` → `BadRequest`. Verified feasibility: `association::authorize_association`
    (`association.rs:6-51`) denies with `missing()` = `AppError::NotFound("Workflow resource")`
    (`workflow/mod.rs:68-70`), the same value as a missing row, and propagates database errors
    with `?`, so identical responses need no error collapsing.

## WF-P07-F010 — Optional suggestions on revision 2 (non-blocking)

- Classification: **optional suggestion**, severity **Optional**, category design clarity /
  efficiency / defense in depth.
- Items:
  1. Polling eligibility (D1.4, `decision-D1-r2.md:189-191`): "lookup by the events primary key
     prefix `(company_id, run_id)`" filtered by `event_kind='parent_wakeup'` scans every event of a
     still-running child each time CANDIDATES_SQL evaluates a waiting parent (PK is
     `(company_id, run_id, sequence)`, `init_schema.sql:9515`). The child row's
     `terminal_wakeup_sequence IS NOT NULL` (set in the same transaction as the fact,
     `allocate_workflow_wakeup_sequence`) or the full PK `(company_id, child, terminal_wakeup_sequence)`
     is a point lookup with identical visibility.
  2. Cascade (D1.5): the "at most one row" premise holds for B1 (sequential graphs, NOT_PLANNED;
     `workflow_wait_one_active` is per execution). Writing `cancel_tree_on` to iterate every
     waiting `child_run` wait of the run (ordered, bounded) — or recording in D1.13 that B2's
     concurrent tool-call children must revisit it — keeps the D1.13 "no one-child assumption"
     promise.
  3. Defense in depth for the D1.5 invariant: a deferred constraint trigger requiring every
     inserted run with `parent_run_id` to be named by a `child_run` wait of its parent execution at
     commit would make "every live child is named by its parent's wait" a database invariant
     rather than single-writer discipline (persistence AGENTS.md "Preserve invariants at the
     database boundary").
  4. `ChildPublicationDirectory` (D1.1): reject more than `MAX_DEPENDENCIES` (256) distinct
     declared children before querying, and read sizes in one batched statement (`= ANY`) rather
     than a per-child loop (persistence AGENTS.md "Replace per-parent query loops with bounded
     batch reads").
  5. Park ordering (D1.2 steps 3-4): validate the child configuration (`ConfiguredBinding::new`)
     before inserting the `round:1` repetition receipt, so a deterministic configuration refusal
     does not consume a repetition unit — or state that consuming it is intended.
- Instruction lesson: none.
- Resolution history:
  - 2026-10-22 (R002): open (optional; does not block acceptance).
  - 2026-10-22 (R003, decision rev 3 sha256 `96d201af…e0b6`, HEAD `2d608f2`, digest `31c52a4a…1b21`): all five items **adopted** and checked: (1) D1.4 consumption and polling read the
    child row's `terminal_wakeup_sequence` (PK) and the fact by full PK; both are written in the
    same statement (`allocate_workflow_wakeup_sequence` BEFORE / `schedule_workflow_parent_wakeup`
    AFTER UPDATE), and a runtime child has one terminal transition (never retried); (2) D1.5
    iterates every waiting `child_run` wait ordered by `child_run_id` (deterministic lock order);
    (3) D1.3 `workflow_child_parent_wait` deferred constraint trigger on child-run insert —
    no non-test code deletes `workflow_waits` rows or issues `SET CONSTRAINTS` (only two
    action-reconciliation SQL fixtures do, and they insert no runs), parent links are immutable,
    and the raw-SQL negative inserts fail on their immediate FKs first; (4) D1.1 caps distinct
    pairs at 256 before querying and uses two batched reads; (5) D1.2 validates the child
    configuration before the `round:1` receipt. Closed.

---

Context for WF-P07-F011: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`,
review `REVIEW.md`; batch B1; stage architecture-gate correction recheck; round `WF-P07-R003`;
reviewer Claude Opus 5.5 (`claude-opus-5-5`), reviewer B (session 2, same as R002); phase `p07`;
worktree `/Users/mac02/dev/openbot`; examined decision `P07-D1` revision 3
(`logs/decision-D1-r3.md`, sha256 `96d201af8768e57a05b5796c1e4bd07d1370ddd914d554900eaa127b42cfe0b6`)
against HEAD `2d608f2`, digest `31c52a4a554512444396a39b77d8b2b5142231232cf0ee3027a5d4855fc91b21`
(same exclusions). Originates in the gated decision.

## WF-P07-F011 — Optional: D1.7 overstates what D1.3 enforces for the rerun original

- Classification: **optional suggestion**, severity **Optional**, category design clarity /
  defense in depth.
- Evidence: D1.7 (rev 3) says "D1.3 constraints back the tenant, same-workflow and top-level
  rules". D1.3's CHECK `rerun_of_run_id IS NULL OR (parent_run_id IS NULL AND rerun_of_run_id <> id)`
  makes the *new* run top-level; nothing in the schema requires the *original* to be top-level.
  The application check is race-free (the original is locked `FOR SHARE` and `parent_run_id` is
  immutable via `preserve_workflow_parent`), so this is wording or optional hardening, not a
  defect.
- Suggestion: either reword ("tenant and same-workflow rules; the original's top-level rule is
  the locked admission check") or back it in the schema, e.g. a BEFORE INSERT check that the
  referenced original has `parent_run_id IS NULL`.
- Instruction lesson: none.
- Resolution history:
  - 2026-10-22 (R003): open (optional; does not block acceptance).
  - 2026-10-22 (R004, decision rev 4 sha256 `5c076606…adc4`, HEAD `2d608f2`, digest `31c52a4a…1b21`): **adopted**, closed. D1.3 adds BEFORE INSERT trigger `workflow_rerun_original`
    (original must have `parent_run_id IS NULL`; sufficient at insert because `parent_run_id` is
    immutable via `preserve_workflow_parent` and `rerun_of_run_id` is immutable too); D1.7
    wording now separates FK, CHECK and trigger; D1.10 adds the raw-insert refusal test.

---

Context for WF-P07-F012: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`,
review `REVIEW.md`; batch B1; stage architecture-gate correction recheck; round `WF-P07-R004`;
reviewer Claude Opus 5.5 (`claude-opus-5-5`), reviewer B (session 2); phase `p07`; worktree
`/Users/mac02/dev/openbot`; examined decision `P07-D1` revision 4 (`logs/decision-D1-r4.md`,
sha256 `5c076606580e887de3d12203fa31e3a1d3a73b1112b169840b56938698a4adc4`) against HEAD
`2d608f2`, digest `31c52a4a554512444396a39b77d8b2b5142231232cf0ee3027a5d4855fc91b21` (same
exclusions). Originates in the gated decision (D1.2a category B).

## WF-P07-F012 — Optional: precision caveats for category-B `lineage_child`

- Classification: **optional suggestion**, severity **Optional**, category verification design /
  clarity. The author's claim (a settled `child_run` wait leaves an unparked parent execution's
  behavior unchanged) holds on every normal path; these caveats belong in the helper's
  contract so batch review can check them.
- Evidence (all readers traced): Rust — `fairness.rs:85` (`VALID_HINT`), `polling.rs`
  CANDIDATES_SQL, `waits::select_due`, `controls::cancel_on`, `maintenance::expire_on`,
  `decisions.rs:493`, `message_receipts.rs:309` read or update only `state='waiting'`;
  `checkpoint_updates`, `decision_approvals`, `decision_tokens`, `decisions:594,665`,
  `token_budget:194` join through `decision.wait_id`; `waits::row(Execution)` is called only
  from `waits::park_on` and `wait_commit::resume_on`. Schema — `check_workflow_fenced_completion`,
  the dispatch commit check (`init_schema.sql:3325`), `check_workflow_wait_commit`,
  decision-pair/delete guards, `refuse_expired_workflow_tokens`, model-tool continuation and the
  token checkpoint guard are reason-, decision- or waiting/UPDATE-specific; table CHECKs accept
  a `cancelled` wait with `settled_at` and the parent deadline.
- Items:
  1. `resume_on` reaches `waits::row(Execution)` for any execution with a waiting wait (also
     effect/decision waits of agent/action executions, not only wait/child-kind steps). A settled
     lineage wait sorts after any waiting wait, so it is selected only on a replay or race after
     that wait settled; the outcome there becomes `Refused` instead of the existing
     `saved`/`Err(invalid)`. Not reachable on normal poll/sweep paths; worth a sentence.
  2. `park_on` (wait.event/timer) and `park_child` would return a settled lineage wait as a
     replayed `Parked`: the helper must refuse wait- and child-kind parent steps (assert it).
  3. Inserting the wait fires `workflow_wait_revision` (`advance_workflow_owned_revision`) and
     bumps the parent run's `revision`; tests using `expected_revision` read it after setup.
  4. Several lineage children of one parent execution need distinct `child_ordinal`s (unique
     `(company, run, execution, child_ordinal) WHERE reason='child_run'`).
  5. Lineage children are deliberately outside the D1.5 cascade (it reads only waiting
     `child_run` waits) and their `parent_wakeup` fact is never consumed — they violate the D1.5
     invariant by construction. Category-B tests must not rely on parent cancellation or wait
     consumption reaching them; say so in D1.2a. (No sampled category-B case does.)
  6. Editorial: D1.2a says "18 in persistence" but lists 24 persistence files (the list is
     complete and correct).
- Instruction lesson: none.
- Resolution history:
  - 2026-10-22 (R004): open (optional; does not block acceptance).
  - 2026-10-22 (R005, digest `1e1b0a03…c9e4`): **adopted** — `child_test_support.rs::lineage_child`
    documents the contract and asserts the step-kind precondition and next free ordinal; category-B
    tests exercised (108 passed in `logs/r005-moved-and-races.log`). Closed.

---

Common context for WF-P07-F013…F017: date 2026-10-22; workflow `implement-opus-reviewed-2_2`;
task `TASK.md`, review `REVIEW.md`; batch B1; stage **batch review**; round `WF-P07-R005`;
reviewer Claude Opus 5.5 (`claude-opus-5-5`), reviewer B (session 2); phase `p07`; worktree
`/Users/mac02/dev/openbot` (branch `workflows`); reviewed artifact HEAD
`2d608f269761717de7706912bd99ddd325d9d352` + working tree (B1 is uncommitted; 11 untracked source
files), digest `1e1b0a032b4d93b16d2a09774ab5a13cb925741860d16358f334f2698f5ec9e4` (exclusions
`plan/workflow-foundation/phases/p07/docs plan/workflow-foundation/COORDINATION.md marketing website`);
gate P07-D1 rev 4 (`5c076606…adc4`). These are implementation findings (not gated-decision
defects) unless stated.

## WF-P07-F013 — Shipped template: escalation does not reach a specialist with the artifact and feedback

- Classification: **actionable defect**, severity **Medium**, category requirements.
- Evidence:
  - Plan 07 §Reusable draft-and-review round: "parent routes final choice … escalate:
    specialist workflow with artifact and feedback"; gated D1.8: "`done` → rule routing: accept →
    reply with the accepted artifact, escalate → specialist, cancel → end, timeout → declared
    timeout route. `exhausted` → human escalation."
  - `src/application/workflow/fixtures/repeated-human-revision.yaml`: `outcome` routes
    `escalated: escalate` (and `timed_out: escalate`; `exhausted: escalate`), and `escalate` is a
    `message.send` of the literal body "Human revision needs further attention." to the reviewer.
    No step receives the artifact or the feedback, and no specialist (agent or child workflow)
    runs for an escalation.
  - Not among the author's recorded deviations 1–7 (TASK "B1 implementation notes").
  - The successor fixture `review-feedback-successor.yaml` shows the specialist mapping, but in a
    different workflow; the shipped template — the plan's representative process — lacks it.
- Impact: the template's escalate outcome loses the reviewed artifact and the reviewer's
  feedback; the escalation recipient gets a context-free notice.
- Required change: route `escalated` to a specialist step that maps request, artifact and
  feedback explicitly (e.g. an `agent.run` like the successor fixture, or a pinned specialist
  `workflow.call`), keep `exhausted` → human escalation and the declared timeout route, and
  extend the fixture/publication tests; or record and agree a deviation from D1.8 with its reason.
- Instruction lesson: observation — deviations were listed carefully for code but not for the
  template's behaviour. Candidate (phase-plan scope): "When a gated decision specifies a shipped
  template's routes, check each route against the template in the deviation list."
- Resolution history:
  - 2026-10-22 (R005): open.
  - 2026-10-22 (R006, digest `d82fa028…5c11`): **fixed**. `repeated-human-revision.yaml`: `escalated` → new `specialist` `agent.run`
    whose context maps `untrusted_request` (`/input/message`), `artifact` and `feedback`
    explicitly; `timed_out` and `exhausted` → human escalation; `accepted` → reply; `cancelled` →
    end. Test `fixtures::tests::workflow_fixtures_revision_escalation_reaches_a_specialist_with_artifact_and_feedback`
    checks every route and the specialist's prepared context; the end-to-end publication test
    publishes the changed parent through the production directory chain. Passed in
    `logs/r006-review-checks.log`.

## WF-P07-F014 — Child parameters and resource restriction are untested at compile time and at runtime

- Classification: **actionable defect**, severity **Medium**, category verification / security.
- Evidence:
  - No test in `src/` references `registry.static_params` or `registry.child_resources` (the
    D1.1 diagnostics added in `registry/mod.rs` and `registry/families.rs`).
  - Every child used by the B1 runtime tests declares `resources: []` and no parent passes
    `with.params` (`child_test_support.rs:14-36`, `child_runtime_tests.rs`, `child_repeat_tests.rs`).
    So `children::child_binding` (`children.rs:146-182`) — the restriction of the parent's
    resources to the child's slots and the frozen static params — never runs with content.
  - D1.10 Park requires "invalid params/resources routed"; the only park refusal test
    (`workflow_call_child_limits_reject_input_at_park_and_route_it`) exercises an input larger
    than the child's context limit, not a params/resources rejection by the child bundle.
  - Plan §Child calls: "The child receives only its explicit inputs and allowed runtime resource
    bindings"; "They cannot gain access by selecting a more privileged workflow" (TASK criteria 1–2).
- Impact: the security-relevant half of acceptance criterion 1 has no evidence; a regression that
  passed all parent resources to a child would stay green.
- Required change: compiler/registry tests for a non-static `with.params` reference
  (`registry.static_params`), a child slot the parent lacks or declares with another
  kind/contract (`registry.child_resources`), and params conformance; a runtime test where the
  parent binds more resources than the child declares, asserting the child run row's `resources`
  holds exactly the child's slots and its `params` equal the frozen `with.params`; and a park test
  where the child bundle rejects the params (or resources) → `workflow.invalid_input`, error route,
  no child.
- Instruction lesson: no new instruction supported (AGENTS.md "run the smallest meaningful set of
  tests covering each changed behavior"); observation: the D1.10 list named it, the test plan
  mapping did not check it off.
- Resolution history:
  - 2026-10-22 (R005): open.
  - 2026-10-22 (R006, digest `d82fa028…5c11`): **fixed**. Registry test
    `runtime_tests::workflow_registry_child_params_are_static_and_resources_match_by_slot`
    (`registry.static_params` for run input and run context, literal conformance, omitted params
    → `registry.child_params`, `registry.child_resources` for a missing slot and another kind).
    Runtime `child_configuration_tests`: a parent binding two MCP slots gives the child exactly
    its frozen `with.params` and only the `service` slot's resource (binding id as provenance),
    and the child completes; a parameter the child refuses fails activation with no child
    (deviation 8, accepted — see R006 note). Passed in `logs/r006-review-checks.log`.

## WF-P07-F015 — Revision acceptance is shown with `wait.event` stand-ins; no decision round, no "no replay" check

- Classification: **actionable defect**, severity **Medium**, category verification /
  requirements coverage.
- Evidence:
  - `child_repeat_tests.rs:1-2`: "Each round's child is a `wait.event` whose payload stands in for
    the human result". No B1 test runs a `decision.human` round; the shipped template is only
    published (`child_publication_tests.rs` end-to-end test), never executed.
  - Plan 07 §Acceptance: "Revision feedback reaches the next round, while the previous artifact
    and decision stay intact"; "Accepting an older draft cannot authorize a newer one"; "Only the
    final accepted artifact is sent, once; context loads and earlier effects are not replayed".
    D1.10: "older-round decision submission refused; only the final artifact sent once, with no
    replay of context loads or earlier effects".
  - Covered today: next input carries feedback, round-1 output intact, last-round `done`, late
    *event* for an older round `Unmatched`, `deliver` runs once. Not covered: a decision row of an
    older round unchanged and a late *submission* to it refused; reviewers resolved from the
    child's `/params` (the static-params path, see F014); a context load or effect before the
    repeat step executing exactly once across rounds (`repeat_parent` starts at `revisions`).
- Impact: the plan's core human-revision acceptance criteria rest on a stand-in for the decision
  mechanism and on structural argument.
- Required change: a runtime test whose rounds are `decision.human` children (draft as
  `data.map`, reviewer from child `/params` supplied by the parent's static `with.params`;
  decisions submitted with `submit_decision`, no model needed), with a context step (and ideally
  an effect) before the repeat, asserting the items above.
- Instruction lesson: candidate (phase-plan scope): "A stand-in used for an acceptance criterion
  must be named in the test plan with what it does not prove."
- Resolution history:
  - 2026-10-22 (R005): open.
  - 2026-10-22 (R006, digest `d82fa028…5c11`): **fixed**. `child_decision_round_tests::workflow_revision_rounds_with_human_decisions_keep_old_rounds_and_load_context_once`
    runs through the real worker, fair poll, history handler and decision notices: rounds are
    `decision.human` children whose reviewer comes from the child's `/params` (parent's static
    `with.params`); the revision reaches round two as explicit input; round one's decision row
    stays byte-identical and late submissions (stale and fresh view) are refused while round two
    stays waiting; context and repeat each execute once; history members unchanged; only the
    accepted artifact is delivered, once. No effect precedes the repeat (the shipped template's
    only pre-repeat step is the context load) — accepted. Passed in `logs/r006-review-checks.log`.

## WF-P07-F016 — Invalid child output and real cross-tenant schema rows are untested

- Classification: **actionable defect**, severity **Low**, category verification.
- Evidence:
  - `child_waits.rs:98-101` (`fail(InvalidOutput)` when the child output breaks the pinned schema)
    and `:180-184` (parent completion refused → `InvalidOutput`) have no test (no
    `InvalidOutput`/`invalid_output` in any `child_*_tests.rs`). D1.10 Resume lists "invalid child
    output".
  - `child_schema_tests.rs:133-144` uses a random UUID as "an unknown original (or another
    tenant's)"; no test inserts a `child_run` wait naming another company's child, or a rerun
    naming an existing other-company or other-workflow run, against `workflow_wait_child_link` /
    `workflow_rerun_original_fk`. Persistence AGENTS.md: "add a rollback-only or
    transaction-scoped database test that demonstrates the invalid cross-tenant row is
    rejected"; D1.10: "schema FKs/uniques reject cross-execution and cross-tenant wait and rerun
    links". (The service-level `rerun_tests` do use a real foreign company and sibling workflow,
    but they are refused before the FK.)
- Required change: a resume test with a child whose stored terminal output violates the pinned
  schema (wait `failed`/`invalid_output`, run failed, replay identical); schema tests with real
  foreign-company and other-workflow rows.
- Instruction lesson: none new.
- Resolution history:
  - 2026-10-22 (R005): open.
  - 2026-10-22 (R006, digest `d82fa028…5c11`): **fixed**. `child_output_tests`: the parent's `$end` contract refusing the child
    result and a stored child result corrupted outside the pinned schema both fail the wait
    `invalid_output`, run failed, no successor, replay identical (`session_replication_role` is
    used for the corruption; the phase role and CI's `postgres` user are superusers).
    `child_schema_tests`: real sibling-workflow and foreign-company runs as rerun originals →
    23503; a `child_run` wait naming another company's real child → 23503 at commit. Passed in
    `logs/r006-review-checks.log`.

## WF-P07-F017 — Optional suggestions on B1

- Classification: **optional suggestion**, severity **Optional**, category design / clarity.
- Items:
  1. A parent step that omits `with.params` for a child whose `parameter_schema` requires
     parameters compiles and publishes, then fails every run at park (`{}` rejected). Validating
     the absent default `{}` against the child parameter schema at compile time would report it
     at publication.
  2. The human-escalation `message.send` in the template (exhaustion/timeout) could carry the
     artifact and feedback so the recipient can act (pairs with F013).
- Instruction lesson: none.
- Resolution history:
  - 2026-10-22 (R005): open (optional; does not block acceptance).
  - 2026-10-22 (R006, digest `d82fa028…5c11`): item 1 **adopted** (`Registration::default_params`, `registry.child_params`, tested);
    item 2 **declined** with reason (message body bound equals the artifact bound; the specialist
    path carries artifact and feedback) — accepted. Closed.

---

Common context for WF-P07-F018…F023: date 2026-10-22; workflow `implement-opus-reviewed-2_2`;
task `TASK.md`, review `REVIEW.md`; batch **B2**; stage **architecture gate**; round
`WF-P07-R007`; reviewer Claude Opus 5.5 (`claude-opus-5-5`), reviewer B (session 2); phase
`p07`; worktree `/Users/mac02/dev/openbot`; examined decision `P07-D2` revision 1
(`logs/decision-D2.md`, sha256 `1e11108ac016322ab1be1d293e1d32ca7bc97c29c953936d42918142cd50ff27`)
against the B1 baseline (HEAD `2d608f2` + working tree, digest
`d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11`). All findings originate in
the gated decision (no B2 code exists).

## WF-P07-F018 — Re-pin and archive stop frozen workflows, contrary to P07-Q1's "frozen … picked up on republish"

- Classification: **actionable defect**, severity **Medium**, category requirements / product
  semantics.
- Evidence:
  - P07-Q1 (TASK "Decisions", user): "Workflows using that agent freeze the pinned version at
    publication; upgrading is an explicit admin edit picked up on republish."
  - D2.2 "Publish-time and admission checks": `registered_on` requires the registration to pin
    *the same* workflow, version and params *now* and the version to be selectable; it runs at
    publish, admission and replay ("revoking **or re-pinning** … stops new runs of workflows frozen
    with the old pin until they are republished"). D2.3: `current_on` re-checks the same at every
    agent preparation, so in-flight runs fail their next agent step (terminal "Selected agent
    capability was revoked", `agent_preparation.rs:108-111`).
  - Consequence: an admin upgrade (re-pin v1 → v2) immediately breaks every published workflow
    that froze v1 — new runs refused, in-flight agent steps failed — until each is republished;
    archiving the child definition does the same, whereas a `workflow.call` child keeps running
    from the frozen graph after archive (B1). That is a forced republish, not "frozen … picked up
    on republish". The HTTP analogy ("like an HTTP revocation") fits revocation of a live external
    endpoint; a pinned workflow version is an immutable internal artifact.
- Required change: state the semantics explicitly and get them agreed (the B2 row allows
  product questions to the user): e.g. revocation (and only revocation) stops new runs and
  preparations; a re-pin leaves frozen pins runnable until republish (check the registration
  exists by name and the frozen version is still a version of that company, not equality with
  the current pin); decide whether archiving the pinned definition stops tool use. Add tests for
  the chosen re-pin, revoke and archive behaviour (D2.7 has only "revoked registration →
  preparation and admission refused").
- Instruction lesson: candidate (phase-plan scope): "When a gated decision interprets a user
  product decision, quote the sentence each behaviour implements and flag any behaviour the
  sentence does not state."
- Resolution history:
  - 2026-10-22 (R007): open.
  - 2026-10-22 (R008, decision D2 rev 2 sha256 `46301f5e…3451`): **fixed** (decision level). User decision P07-Q2 (TASK "Decisions": revoke stops,
    re-pin/archive don't) is implemented by two `registered_on` modes: Publish (exact pin,
    selectable, `FOR SHARE`) at publication; Run (a registration of the name exists) at admission,
    replay, binding readiness and dispatch. Preparation checks grants by name only; revocation
    refuses the next call with `workflow.tool_revoked` while a parked call still delivers (step 2
    before step 3). D2.7 tests re-pin, archive, revoke and re-register. One scoping gap in the
    Publish mode for stored child bundles is logged separately as WF-P07-F024.

## WF-P07-F019 — Verification plan lacks competing-claimant and recovery tests for the new park protocol

- Classification: **actionable defect**, severity **Medium**, category verification /
  concurrency.
- Evidence:
  - D2.3/D2.5 add a new concurrency protocol: a fenced agent attempt closes as `child_parked`
    while creating a child and its wait in one transaction, guarded by a new deferred
    fenced-completion park arm (checked at commit against the live lease,
    `workflow_fenced_completion_guard` is `DEFERRABLE INITIALLY DEFERRED`), a reopen arm and a
    continuation guard. AGENTS.md: "When adding or changing a concurrency protocol, include a test
    with competing claimants rather than relying only on sequential mocks."
  - D2.7 lists one race ("competing resumers → one continuation") and sequential cases ("parent
    cancel cascades", "duplicate dispatch and replay", "restart mid-wait").
  - Missing: (a) parent cancel vs tool dispatch, both orders and competing (cancel first → fence
    fails, no child; dispatch first → cascade cancels the child); (b) lease expiry or
    `retire_expired_io` vs dispatch, including a lease that expires inside the dispatch
    transaction (commit refused, no child, no wait); (c) parent cancel vs continuation (resume
    reopening the agent job); (d) recovery after continuation: the reopened attempt is retired
    or fails before it saves the `InvocationResult`, and the retried attempt replays to the same
    result with one child (the "no marker ⇒ no effect" retirement path for marker-owned
    `agent.run`, `actions/failure.rs:26-55`).
- Required change: add (a)–(d) to D2.7 as DB-backed competing/recovery tests.
- Instruction lesson: no new instruction (AGENTS.md already requires it); observation: the B1
  plan named its races explicitly, the B2 plan did not.
- Resolution history:
  - 2026-10-22 (R007): open.
  - 2026-10-22 (R008, decision D2 rev 2 sha256 `46301f5e…3451`): **fixed** (decision level). D2.7 adds cancel vs dispatch (both orders and racing),
    lease expiry / `retire_expired_io` vs dispatch including expiry inside the transaction,
    cancel vs continuation (both orders), and a reopened attempt failing before it saves its
    result (replays to the same result, one child).

## WF-P07-F020 — No registration recheck at dispatch; revocation is weaker than for HTTP tools

- Classification: **actionable defect**, severity **Low**, category security / consistency.
- Evidence: HTTP and MCP agent tools are rechecked at dispatch by `ActionService` (current
  registration and policy, `action_dispatch.rs:130-165`); `current_on` documents "ActionService
  still rechecks each tool's registration and policy" (`agent_preparation.rs:61-66`). D2.3's
  dispatch (steps 1–7) rechecks nothing; revocation is enforced only at preparation and
  admission (D2.6: "revocation stops new runs and new preparations"). A revoke committed during
  an attempt still lets that attempt create children of the revoked tool (the runtime child
  writer itself runs no admission checks).
- Required change: recheck the registration inside the dispatch transaction (under the 06-D5
  lock order: authority-class rows before the run lock, or a non-locking read after it with
  the reason recorded), or record explicitly that workflow-tool revocation takes effect at the
  next preparation and why that is acceptable. Test either way.
- Instruction lesson: none new.
- Resolution history:
  - 2026-10-22 (R007): open.
  - 2026-10-22 (R008, decision D2 rev 2 sha256 `46301f5e…3451`): **fixed** (decision level). Dispatch step 0 reads the registration `FOR SHARE`
    before any run lock; revocation takes the company lock and the registration row and never a
    run, so there is no lock cycle; a revoked name returns `workflow.tool_revoked` with nothing
    created; tested.

## WF-P07-F021 — More readers treat `connection: None` as HTTP than D2 lists (admission readiness would refuse workflow tools)

- Classification: **actionable defect**, severity **Low**, category correctness / affected
  contracts.
- Evidence: D2 keeps `contract.connection: None` for workflow tools and names two places that
  must classify them (`authority::supported_publication`, `agent_preparation::targets`). Other
  readers map `connection: None` to HTTP:
  `resources::check_readiness` (`resources.rs:45-55`, calls `http::registered_on` for every
  non-message `None` snapshot) runs at admission (`admission.rs:28`), admission replay
  (`admission_replay.rs:93`) and binding save/activation (`bindings.rs:43`) — it would refuse
  every binding/admission of a workflow whose agent grants a workflow tool; also
  `application/workflow/agent.rs:232-239` (`prepared_tool` consistency, compiler-enforced only
  once a new target exists) and the `AgentToolDispatch` contract doc ("marker-first dispatch …
  through ActionService", `agent.rs:220-221`).
- Required change: list every such reader in D2 (readiness included) and classify through one
  helper (e.g. `ToolKind::of(snapshot)` returning Message/Http/Mcp/Workflow) used by all of them
  (src/AGENTS.md "One decision, one place"), so a missed site is a compile error, not a runtime
  refusal; add a binding-activation/admission test with a workflow tool.
- Instruction lesson: candidate (phase-plan scope): "When overloading an existing field's
  meaning (here `connection: None`), grep every reader of that field and list them in the gate."
- Resolution history:
  - 2026-10-22 (R007): open.
  - 2026-10-22 (R008, decision D2 rev 2 sha256 `46301f5e…3451`): **fixed** (decision level). D2.2a: `ToolSnapshot::kind` → `ToolKind::{Message, Mcp,
    Http, Workflow}`, matched exhaustively at `supported_publication`, `check_readiness`,
    `targets`, `prepared_tool` and `is_trusted_message_snapshot`; docs updated; D2.7 tests binding
    save/activation, admission and replay with a workflow tool. The remaining name-keyed lookups
    (`http_runtime.rs:41,158,184`, `message.rs:110`) select a snapshot for an already-classified
    HTTP/message call and are safe given unique agent tool names.

## WF-P07-F022 — A same-name MCP step tool silently shadows a workflow-tool registration

- Classification: **actionable defect**, severity **Low**, category correctness / ambiguity.
- Evidence: `granted_tool` (`agent_publication.rs:261-290`) first reuses an already captured
  same-name snapshot when it is unique and is a registration or a selected MCP slot. D2.2 keeps
  that first and only refuses a same-name *HTTP registration*. If an `mcp.call` step of the
  workflow selected an MCP tool named like the company's workflow-tool registration, an agent
  granted that name silently gets the MCP tool.
- Required change: when the company has a workflow-tool registration of the granted name and a
  different captured snapshot of that name exists (MCP slot or HTTP), refuse as ambiguous, like
  the HTTP case; test it.
- Instruction lesson: none.
- Resolution history:
  - 2026-10-22 (R007): open.
  - 2026-10-22 (R008, decision D2 rev 2 sha256 `46301f5e…3451`): **fixed** (decision level). D2.2 step 1 refuses a captured same-name snapshot that is
    not the company's workflow-tool registration as ambiguous; tested (D2.7).

## WF-P07-F023 — Optional suggestions on P07-D2 rev 1

- Classification: **optional suggestion**, severity **Optional**, category clarity.
- Items:
  1. State in D2.3 why marker-free tool calls are safe for marker-owned `agent.run`
     (`marker_owned_effect_kind`, `retry_safe_without_marker`, `pending_recovery::settle`'s
     `unknown_effect`): the call is never uncertain (child and park commit atomically) and replay
     is idempotent by the saved call, so "no marker ⇒ no effect" never re-runs a child. Update the
     `WorkflowHandler` doc (`worker.rs`) and the `AgentToolDispatch` doc accordingly.
  2. Dispatch step 2 "`waiting` → `Parked(wait)` (re-entry before confirmation)" looks
     unreachable: after the park commits, the attempt is closed, so the same fence fails at step
     1, and the job is not claimable while the wait waits. Drop it or say when it occurs.
  3. Workflow-tool registrations have no admin `approval_required` flag, unlike HTTP/MCP
     operations; record this as a deliberate B2 scope choice (children carry their own
     decisions).
  4. WF-C003 targets p09 for "admin UI/API"; p08 owns company-scoped HTTP routes over
     application commands. Consider naming p08 for the API or saying p09 owns both.
- Instruction lesson: none.
- Resolution history:
  - 2026-10-22 (R007): open (optional; does not block acceptance).
  - 2026-10-22 (R008, decision D2 rev 2 sha256 `46301f5e…3451`): all four items **adopted** (D2.3a rationale and doc updates; the unreachable
    `waiting → Parked` branch is now a defect `Err`; no approval flag recorded in D2.1; WF-C003
    names p08 routes and p09 UI/inspection/sample execution). Closed.

---

Context for WF-P07-F024…F025: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task
`TASK.md`, review `REVIEW.md`; batch B2; stage architecture-gate correction recheck; round
`WF-P07-R008`; reviewer Claude Opus 5.5 (`claude-opus-5-5`), reviewer B (session 2); phase `p07`;
worktree `/Users/mac02/dev/openbot`; examined `P07-D2` revision 2 (`logs/decision-D2-r2.md`,
sha256 `46301f5ec1a6f67a8c6a9cd11f1eff98b0225d9a595b330561fecfdba2143451`) against the B1 baseline
(digest `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11`). Both originate in the
gated decision.

## WF-P07-F024 — Publish mode is unscoped for stored child bundles in the closure

- Classification: **actionable defect**, severity **Low**, category requirements consistency /
  specification.
- Evidence: D2.2 puts the `Publish` mode (exact current pin, selectable) "in
  `DefinitionPersistence::publish`", where `authority::supported_publication` runs
  (`definitions.rs:192`). That function walks the whole frozen closure, including every stored
  child bundle (`authority.rs:94-128`, `pending.extend(current.children())`). A new parent that
  pins an existing child version whose own agent froze workflow tool `T → W@v1` would then be
  refused after `T` is re-pinned to `W@v2` (or `W` archived), although under P07-Q2 that child is
  "already published" and keeps running on its frozen pin. The precedent is the opposite:
  `agent_publication::verify_on` re-derives only the root's agents; "Stored child bundles were
  verified at their own publication and are not re-derived" (`agent_publication.rs:149-150`).
  D2.7 does not test this case.
- Required change: state that `Publish` mode applies to the root bundle's own (freshly captured)
  tool snapshots and stored child bundles are checked in `Run` mode (registration exists), or
  state and justify the stricter rule; add a test where a parent pins a child whose tool was
  re-pinned (and one where it was revoked).
- Instruction lesson: none new.
- Resolution history:
  - 2026-10-22 (R008): open.
  - 2026-10-22 (R009, decision D2 rev 3 sha256 `b13f51e9…b335`): **fixed** (decision level). D2.2 scopes `Publish` mode to the workflow-tool
    snapshots the root bundle captured in this publication and checks stored child bundles in
    `Run` mode, matching `agent_publication::verify_on`; `supported_publication` chooses the
    mode per bundle. D2.7 tests a new parent pinning a child whose tool was re-pinned (publishes,
    calls the frozen version) and revoked (does not publish).

## WF-P07-F025 — Optional: re-registering a revoked name re-enables the old frozen pin

- Classification: **optional suggestion**, severity **Optional**, category product clarity.
- Evidence: D2.2 Run mode requires only "a registration of that name exists … Re-registering the
  same name re-enables them". If an admin revokes `T` (pinned to `W1`) as a kill switch and later
  registers `T` for an unrelated `W2`, every workflow frozen with `T → W1` runs `W1` again.
  P07-Q2 calls revocation a kill switch and re-pinning "to another version".
- Suggestion: either key the Run check on the name *and* the frozen `workflow_id` (a re-pin to
  another version of the same workflow keeps old pins; a name reused for another workflow does
  not revive them), or record this behaviour explicitly so p09's UI (WF-C003) can warn when a
  revoked name is reused.
- Instruction lesson: none.
- Resolution history:
  - 2026-10-22 (R008): open (optional; does not block acceptance).
  - 2026-10-22 (R009, decision D2 rev 3 sha256 `b13f51e9…b335`): **adopted** (decision level). D2.2 `Run` mode matches name and `workflow_id`;
    re-pointing or reusing a name for another workflow keeps old pins refused, re-registering for
    the same workflow re-enables them; tested for admissions (D2.7). Closed. The dispatch step
    (D2.3 step 3) was not updated to the same rule; logged as WF-P07-F026.

---

Context for WF-P07-F026: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task `TASK.md`,
review `REVIEW.md`; batch B2; stage architecture-gate correction recheck; round `WF-P07-R009`;
reviewer Claude Opus 5.5 (`claude-opus-5-5`), reviewer B (session 2); phase `p07`; worktree
`/Users/mac02/dev/openbot`; examined `P07-D2` revision 3 (`logs/decision-D2-r3.md`, sha256
`b13f51e92d98a69d4f83dd30c7d321eb91b991386940a04e66350afedd0ab335`) against the B1 baseline
(digest `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11`). Originates in the
gated decision (rev 3 change for F025).

## WF-P07-F026 — Dispatch still refuses only a missing registration, not one re-pointed to another workflow

- Classification: **actionable defect**, severity **Low**, category specification consistency /
  authority.
- Evidence: D2.2 (rev 3, lines 129-135, 141-144) puts dispatch in `Run` mode, which now requires
  a registration of the name "**pinning the same `workflow_id`**", and says that after a name is
  re-pointed to another workflow "old pins of the previous workflow stay refused … for old pins
  it acts as a revocation". The step-by-step dispatch protocol was not updated: D2.3 step 0 reads
  "the company's registration of `name`" and step 3 returns `workflow.tool_revoked` only "If step
  0 found no registration" (lines 179-190). Implemented as written, a running agent frozen with
  `T → W1` keeps starting `W1` children after `T` is re-pointed to `W2`, while admissions of the
  same workflow are refused. The F025 test in D2.7 (lines 295-296) names only admissions
  ("re-enables admissions … keeps old pins refused").
- Required change: make D2.3 step 3 refuse when step 0 found no registration of `name` whose
  `workflow_id` equals `pin.workflow_id` (that is, `registered_on(…, Run)` returning a typed
  refusal rather than `Err`); keep step 2 (a call parked before the re-point still delivers);
  add to D2.7 that a running agent's next call after a re-point gets `workflow.tool_revoked` with
  no child.
- Instruction lesson: none new (covered by the existing "update every place a rule is applied"
  practice).
- Resolution history:
  - 2026-10-22 (R009): open.
  - 2026-10-22 (R010, decision D2 rev 4 sha256 `ebf212db…fab7`): **fixed** (decision level).
    D2.3 step 3 applies the Run-mode rule to the step-0 read: no registration of `name` with
    `workflow_id = pin.workflow_id` (revoked or re-pointed) → `workflow.tool_revoked`, nothing
    created; same workflow at another version/params passes and runs the frozen pin. Step 2 still
    precedes it (parked calls deliver). D2.0/D2.4/D2.6 wording updated; D2.7 tests the re-point
    (refused, no child; parked call delivers) and same-workflow re-pin (frozen version runs).

---

Context for WF-P07-F027…F028: date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task
`TASK.md`, review `REVIEW.md`; batch B2; stage batch review; round `WF-P07-R011`; reviewer Claude
Opus 5.5 (`claude-opus-5-5`), reviewer B (session 2); phase `p07`; worktree
`/Users/mac02/dev/openbot`, branch `workflows`, HEAD `2d608f2` + working tree; examined the B1+B2
tree at digest `201af80cd15074697362d56fe58ac51e6b070ca25bff09ea91b1737846241f98` against
`P07-D2` rev 4 (`logs/decision-D2-r4.md`, `ebf212db…fab7`). Both originate in the B2
implementation.

## WF-P07-F027 — The gated F024 test stops at publication; no tool call runs inside a child run

- Classification: **actionable defect**, severity **Low**, category verification completeness.
- Evidence: D2.7 (rev 4, lines 309-311) requires: "a new parent that pins an existing child whose
  own workflow tool was re-pinned **publishes and its run calls the child's frozen version**".
  `workflow_tool_stored_children_keep_their_frozen_pin_until_the_tool_is_revoked`
  (`workflow_tool_revocation_tests.rs:354-412`) publishes the outer parent and reads the stored
  pin from the bundle, then checks the revoked case; it never binds or admits the outer parent
  and never dispatches the stored child's tool. So two shipped paths have no B2 test:
  (1) admission/binding of a new parent whose *stored* child froze an older tool pin
  (`bindings::verify_selection` → `supported_publication(…, Run)` over children,
  `authority.rs:97-141`); (2) a workflow tool call made from inside a child run — dispatch with a
  child-run scope (`workflow_tool_calls.rs:84-127` reads the child run's own saved binding and
  creates a grandchild through `child_binding`), and the grandchild's terminal fact continuing the
  child's agent job (`call_result` reads the child run's binding, `workflow_tool_calls.rs:217-235`).
  Every B2 runtime test dispatches from a root run.
- Required change: extend that test (or add one) so that after the re-pin the outer parent is
  bound and admitted, its `workflow.call` child (the stored agent workflow) calls the tool, the
  grandchild runs the frozen version (`setup.child`, not v2), and the result reaches the child's
  agent (continuation and replay as in `delivered`).
- Instruction lesson: none new (the existing rule "every gated test item maps to a test" covers
  it).
- Resolution history:
  - 2026-10-22 (R011): open.
  - 2026-10-22 (R012, digest `16e65cb8…faee9`): **fixed**. The test now binds (`bound`) and admits (`admitted_as`, through
    `WorkflowService::admit`, so `verify_selection` runs Run mode over the stored child) the outer
    parent after the re-pin, parks it on its `workflow.call` child, runs the stored agent's
    `agent.run` with a scripted model inside that child run, asserts the grandchild's version is
    `setup.child` (not v2), continues the child's agent job, replays the same call (output
    "nested", saved result = grandchild output, 2 model turns, 1 grandchild). Passed in my run and
    at 2 MiB stack.

## WF-P07-F028 — Optional hardening and coverage

- Classification: **optional suggestion**, severity **Optional**, category defence in depth /
  coverage.
- Items:
  1. `HttpOperation::validate` (`application/workflow/http.rs:75-103`) does not refuse
     `tool.workflow.is_some()`. An HTTP operation stored with a workflow pin would be captured
     by `granted_tool`'s HTTP arm and classified `ToolKind::Workflow`; it fails closed today
     only because `registered_on(…, Publish)` then finds no matching registration. Refusing it at
     registration keeps "HTTP, MCP and message snapshots serialize exactly as today" (D2.2) an
     enforced invariant rather than a consequence.
  2. D2.0 puts "inspection of tool-call children" in B2; the new `inspection.rs` decode arm for a
     `[child, run, exec, step, call, null]` source key (`inspection.rs:118-120`) is not read back
     by any B2 test (`head()` is only called on parent runs). One `head()` assertion on a
     tool-call child would pin it.
  3. Note 9 (judged acceptable): runtime slot passing to a tool-call child goes through the same
     `children::child_binding` and the same saved-binding read as a step child, covered by B1's
     `child_configuration_tests`. If a fixture with a resource slot is added for F027 anyway,
     asserting the child's `resources` would close D2.7's "its slots' resources" literally.
- Resolution history:
  - 2026-10-22 (R011): open (optional; does not block acceptance).
  - 2026-10-22 (R012, digest `16e65cb8…faee9`): items 1 and 2 **adopted** (`HttpOperation::validate` refuses `tool.workflow`,
    `application/workflow/http.rs:81-82`, tested by registering a pinned HTTP operation →
    `BadRequest`; the acceptance test reads the tool-call child's `head()` and asserts
    `ChildCause::ToolCall` with the wait's call id and the parent execution); item 3 not adopted
    (note 9 accepted). Closed.
