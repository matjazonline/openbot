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
