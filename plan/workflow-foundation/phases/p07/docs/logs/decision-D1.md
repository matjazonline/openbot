# P07-D1 — Batch B1 architecture: runtime child calls and bounded repeat (revision 1)

Author: p07 root (A), 2026-10-21. Status: proposed for B architecture gate.
Scope: batch B1 (see TASK). Agent workflow tools are B2 (P07-D2) and only constrained here
(D1.13). No backward compatibility: the init migration is edited in place; databases are fresh.

## D1.1 Language (compiler/registry)

- `workflow.call`: `with.input` (required, conforms to pinned child `input_schema`), `with.params`
  (optional; absent = `{}`; conforms to child `parameter_schema`). Routes `success` (+ optional
  `error`). Output = the child's terminal output (child `output_schema`).
- **Static child params.** Every reference inside `with.params` must have root `params`
  (literals allowed). Reason: decision reviewers are binding parameters, never run input
  (05-D2 §4.2); a child round reads reviewers from its own `/params`. Compile error
  `reference.static` otherwise. Frozen at activation like every `with` input.
- `flow.repeat`: same `with.input` (round 1) / `with.params` (fixed for all rounds),
  `max_iterations` now `1..=1000` (= `MAX_ROOT_REPETITIONS`; was 10000), and a required
  step-level block (sibling of `rule`, forbidden on other kinds):
  `repeat: {next_input: <binding>, exit_when: <binding>}`. Both are *deferred* bindings: not
  frozen at activation; evaluated after each successful child round against the parent
  context (input, params, available step outputs, run metadata) plus the step's own
  provisional output `{result: <child output>, rounds: n}` at `/steps/<self>/output`.
  Self-reference is permitted only inside this block; availability/type guarantees reuse
  `check_reference` with the current step treated as available. `next_input` must conform to
  child input; `exit_when` must be boolean (`check_binding_shape`).
  Routes: `choices` with exactly `done` and `exhausted` (+ optional `error`); both carry output
  `{result, rounds}`. `done` = exit predicate true; `exhausted` = `rounds == max_iterations`
  without exit (never auto-approves; the author routes it).
- **Child resources by name.** Each resource slot the child declares must be declared by the
  parent with the identical kind and contract (publication/compile error otherwise). The child
  run receives the parent run's bound runtime resource IDs for exactly those slot names; it
  cannot obtain any other resource.
- Publication keeps existing pinning/recursion/depth checks; registry help/examples/notes for
  both kinds lose "Execution unavailable".

## D1.2 Runtime child creation ("park child")

New application port `WorkflowChildren::park_child(scope) -> Option<ParkedWorkflow>` (pure
runtime transition, no worker lease, like `park_wait`). Worker routes `workflow.call` and
`flow.repeat` jobs to it. One transaction, lock order run → execution → job (existing
`waits::lock`), never company authority (runtime transition, as `park_wait`):

1. Existing waiting wait for the execution → replay (return it). Run not queued/running or
   past deadline → no-op.
2. `pending_recovery::activate` (token gate, activation limit, freeze `with` inputs, activation
   debit). Invalid input → `settle(InvalidInput)` (existing routing rules).
3. `flow.repeat`: repetition debit `round:1` (see D1.6).
4. Child row (`children::create_on`): new run id; company, actor, channel/thread association
   and correlation copied from the parent run; new trigger id; `binding_id`/`binding_revision`
   copied from the parent run (provenance only — the child's configuration is its own run row,
   read by the unchanged `read_saved_binding`); workflow/version/bundle = the pinned child
   `PublishedBundle` from `parent_bundle.children()`; input = frozen `input`; params = frozen
   `params` (`{}` when absent); resources = parent resources restricted to the child's slots;
   `ConfiguredBinding::new` validates params/resources against the child bundle (failure →
   `settle(InvalidInput)`); limits from the child definition; **deadline = parent run deadline**
   (never extended); parent link = (parent run, parent execution). Then first execution + job,
   thread-history capture, `workflow_admissions` row (binding = parent's binding id, source key
   below), `admitted` event — reusing `admission_write` helpers.
5. Wait row `reason='child_run'`, `child_run_id`, `child_ordinal=1`, deadline = parent run
   deadline; `completion_job::finish(Pure)`; parent run → `waiting/child_run`; audit
   `wait_parked` with `wait_id`.

Retries: any failure rolls back child and wait together; a replay finds the wait. One logical
call ⇒ one child: unique wait `(company, run, execution, child_ordinal)`, unique
`(company, child_run_id)`, unique admissions source key.

**Source identity.** Runtime child admissions use
`v1:["child", parent_run, parent_execution, step, null, ordinal]`. Domain gains
`ChildCause::Call(ChildCallRef { execution: ExecutionRef, ordinal: ChildOrdinal })`
(`ChildOrdinal` 1..=1000 newtype); inspection restores it. Activation stops parsing the source
key for `/run/parent_id` and reads `workflow_runs.parent_run_id` (one source of truth).
The phase-03 binding-based child admission (`TriggerSource::Child` + `ChildCause::Execution/
Action` through `WorkflowService::admit`) is left unchanged and unused by the runtime;
removal/retargeting belongs to phase 08 entry points (REPLACEMENT-MAP 08.2).

## D1.3 Schema (init migration, edited in place)

`workflow_waits` gains `child_run_id uuid`, `child_ordinal integer`:
- CHECK `(reason='child_run') = (child_run_id IS NOT NULL)` and `= (child_ordinal IS NOT NULL)`,
  `child_ordinal BETWEEN 1 AND 1000`.
- FK `(company_id, child_run_id, run_id, execution_id)` →
  `workflow_run_parents(company_id, child_run_id, parent_run_id, parent_execution_id)` (existing
  unique key): a wait can only name a child of its own execution.
- Unique `(company_id, child_run_id)`; unique `(company_id, run_id, execution_id, child_ordinal)`
  where `reason='child_run'`; `workflow_wait_execution` excludes `child_run` (rounds need one
  wait each); `workflow_wait_one_active` unchanged (one active round).
- Child wait park/completion audits always carry `wait_id` (`audit_wait`).

## D1.4 Consumption ("resume child")

`wait_commit::resume_on` dispatches `reason='child_run'` (boxed) to `child_waits::resume_on`,
parent locks only (run → execution → job → wait). The child's terminal `parent_wakeup` fact
(single per child) and its terminal execution output are read without locking the child.
- Child non-terminal → `Pending`.
- Child `succeeded`: output = child terminal execution `committed_output`, validated against the
  pinned child output schema (mismatch → Terminal `workflow.child_invalid_output`).
  - `workflow.call`: complete the execution (`validate_completion` semantics incl. workflow
    output check at `$end`, activation-limit check as `advance`), wait `completed`, run running,
    `batch_commit::complete(.., Parked)`.
  - `flow.repeat`: provisional `{result, rounds: ordinal}`; evaluate `exit_when` → `done`;
    else `ordinal == max_iterations` → `exhausted`; else evaluate `next_input` (invalid →
    Terminal `workflow.invalid_input` routed), repetition debit `round:<ordinal+1>`, settle the
    current wait `completed`, create child `ordinal+1` + its wait (D1.2 step 4–5) in the same
    transaction; run stays `waiting`; returns `WaitProgress::Continued`.
- Child `failed` → Terminal `workflow.child_failed`; `cancelled` → Terminal
  `workflow.child_cancelled`; wait `completed`; `recovery::route_parked_failure` (declared
  `error` route, else run fails with `terminal_execution_id`). Never fabricated success.
- Late/older children: a wait references exactly one `child_run_id`; another child's fact can
  never settle it. Older rounds' decisions are already settled in their own (terminal) child.

Polling (`CANDIDATES_SQL`): a `child_run` wait is eligible when its child has a `parent_wakeup`
event (or by the existing deadline/terminal-run clauses). `select_due` unchanged.

## D1.5 Cancellation and deadlines

- `controls::cancel_on` cascades: after the run's own cancellation it locks the run's
  non-terminal children (`parent_run_id = run`, `ORDER BY id FOR UPDATE`) and applies the same
  cancellation to each, recursively (boxed; depth ≤ 32 by publication depth). Lock order
  company authority → parent run → child run → grandchild, matching admission's parent→child
  order; children never lock parents. Child audit `cancelled_by_parent`; child terminal
  trigger still appends its wakeup fact (harmless: parent wait is cancelled).
- Deadlines: child deadline = parent deadline; parent deadline expiry uses existing expiry; the
  child expires by its own sweep at the same instant. Child-internal timeouts (decision
  deadlines) follow the child's own routes; a child that fails yields `workflow.child_failed`.

## D1.6 Budgets

Root budgets already cover descendants (activations/model calls/tokens via
`workflow_run_budgets`). Each round (including round 1) inserts one `repetition` receipt on the
parent execution with key `round:<n>` (idempotent on replay through the receipt key, inserted in
the creating transaction). `exhausted` disposition → run fails `workflow.root_budget_exhausted`
(not routed, matching `PendingFailure::RootBudgetExhausted`). Revision never resets usage.

## D1.7 Retry / revision / rerun

- Technical retry: park/resume transactions are idempotent replays of the same execution,
  wait, child identity and frozen inputs; no new child.
- Human revision: a new round = new child run (own executions, decision, artifacts, actions);
  prior rounds stay immutable and linked by `(execution, child_ordinal, child_run_id)`.
- Rerun with changed definition/input = a new top-level run through existing admission. The
  "linked to the original" field belongs to the manual start/rerun command (phase 08 manual
  starts, 09 operations UI); not built in B1 (recorded disposition).

## D1.8 Template/fixtures and successor mapping

`human-revision-round.yaml` (child) and `repeated-human-revision.yaml` (parent) become the
executable shipped support template: child draft (agent) → human review (choices accept /
revise / escalate / cancel, `error` route maps a review timeout to `choice: timeout`) → output
`{choice, artifact, feedback, data}`; parent passes static `params`, `max_iterations: 3`,
`exit_when: choice != revise`, `next_input` maps prior artifact/feedback/data, `done` → rule
routing accept→reply with the accepted artifact / escalate→specialist / cancel→end /
timeout→declared timeout route, `exhausted` → human escalation. A successor-feedback fixture
(plan §Feedback excerpt) proves explicit `/steps/review/output/feedback|data` mapping.
Production catalogue wiring stays phase 08/09.

## D1.9 Failure modes and invariants (verification plan)

Invariants: child + wait atomic; one child per (execution, ordinal); wait settles only from its
own child; parent never holds a worker while waiting; children never lock parents; cancellation
reaches every live descendant; deadlines never extended; repetitions debited once per round;
exhaustion never auto-approves; last-round `done` wins over exhaustion.

Tests (DB-backed, fresh schema): compiler/registry (static params, repeat block, routes,
resources by name, self-reference only in repeat, max 1000); park replay and competing parkers;
injected failure between child and wait rolls back both; duplicate resume/competing resumers
consume once; child success/failure/cancel/invalid output mapping and error routes; repeat:
feedback reaches next round, last-round accept, exhaustion route, repetition budget
exhaustion, next-input invalid; parent cancel cascades (and competes with child completion);
child timeout; restart (fresh persistence handle) mid-wait; older-round decision submission
refused; only final artifact sent once; schema FKs/uniques reject cross-execution wait links;
stack budget for the new boxed seams.

## D1.13 Constraint carried to B2

Agent workflow tools will create children through the same `children::create_on` and child wait
rows (ordinal per pending call or a distinct cause), so B1's schema/ports must not assume one
child kind per step type beyond `child_ordinal`.
