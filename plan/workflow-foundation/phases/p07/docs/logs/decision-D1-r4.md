# P07-D1 — Batch B1 architecture: runtime child calls and bounded repeat (revision 4)

Author: p07 root (A), 2026-10-22. Status: proposed for B correction recheck of WF-P07-R003.
Supersedes revisions 3, 2 and 1 (`logs/decision-D1-r3.md`, `logs/decision-D1-r2.md`,
`logs/decision-D1.md`), all immutable. This revision is complete on its own.

## D1.0b Changes from revision 3

| Item | Resolution | Section |
| --- | --- | --- |
| WF-P07-F008 (a) | Inventory rebuilt from every child-constructing helper and all its callers; 18 persistence files + 2 application files. | D1.2a |
| WF-P07-F008 (c) | Per-case rule: A runtime chain / B lineage-only constructed child (settled `child_run` wait) / C retired and replaced; per-test record in `logs/b1-fixture-moves.md`. | D1.2a, D1.10 |
| WF-P07-F008 (b) | TASK focused filters rewritten as real libtest module paths with baseline counts. | TASK |
| F011 | Original's top-level rule backed by trigger `workflow_rerun_original`; D1.7 wording corrected. | D1.3, D1.7 |

## D1.0a Changes from revision 2

| Item | Resolution | Section |
| --- | --- | --- |
| WF-P07-F008 | Move list completed (`budget_activation`, `budget_schema`, `wakeup_guard`, `wakeup_parent_link`, all via `wakeup_tests::admit_child`); nested (grandchild) fixture specified; focused filters extended (TASK). | D1.2a, D1.10 |
| WF-P07-F009 | Test: a same-company original in an inaccessible channel is refused exactly like an unknown original, and nothing is persisted. | D1.10 |
| F010.1 | Child terminal fact found through the child row's `terminal_wakeup_sequence` (primary-key lookup), not an event scan. | D1.4 |
| F010.2 | Cascade handles every waiting `child_run` wait of the run. | D1.5 |
| F010.3 | Deferred constraint: every child run is named by a `child_run` wait of its parent execution. | D1.3 |
| F010.4 | Publication directory caps distinct children at 256 before querying and reads them in batched queries. | D1.1 |
| F010.5 | Child configuration validated before the `round:1` repetition receipt. | D1.2 |
| A refinement | Rerun-original checks live only in the admission transaction (one decision, one place); the service checks only `rerun_of` ⇒ `Manual`. | D1.7 |
Scope: batch B1 (see TASK). Agent workflow tools are B2 (P07-D2), constrained here (D1.13).
No backward compatibility: the init migration is edited in place; databases are fresh.

## D1.0 Changes from revision 1

| Item | Resolution | Section |
| --- | --- | --- |
| WF-P07-F001 | Runtime children capture **no** history membership; history reaches a child only as an explicit input. Inherited admission side effects listed and justified one by one. | D1.2 (table), D1.10 |
| WF-P07-F002 | Every park/round failure class enumerated with its settlement; unclassified errors bounded by fair-poll cadence and run deadline; poison tests for park and round. | D1.9 |
| WF-P07-F003 | Rerun link specified and built in B1 (`rerun_of`), not deferred. | D1.7 |
| WF-P07-F004 | `TriggerSource::Child` is no longer admissible; the runtime child writer is the only creation path; fixtures move to runtime children. | D1.2a |
| WF-P07-F005 | Operator retry stays refused for runtime children and for parents failed by a child; comment updated; refusal tests. | D1.7 |
| WF-P07-F006 | Cascade finds the live child through the parent's waiting `child_run` wait (indexed); competing cancel-vs-park/round tests. | D1.5 |
| F007.1 | Declined for successors (error routes hand `null`, 05-D2 contract); child id/state go in the internal diagnostic. | D1.4 |
| F007.2, .3, .5, .6 | Adopted. | D1.1, D1.4 |
| F007.4 | Adopted as coordination notice WF-C002 to phase 09. | D1.11 |
| A self-finding S1 | Child-capturing publication directory; params/resources checks placed in the registry where parent definition and child contracts are both visible. | D1.1 |

## D1.1 Language and publication

- `workflow.call`: `with.input` (required; conforms to pinned child `input_schema`), `with.params`
  (optional; absent = `{}`; conforms to child `parameter_schema`). Routes `success` (+ optional
  `error`). Output = the child's terminal output (child `output_schema`).
- **Static child params.** Every reference inside `with.params` has root `params` (literals
  allowed): reviewers are binding parameters, never run input (05-D2 §4.2), and a child round
  reads them from its own `/params`. `Registration::static_roots` is generalized to take the
  diagnostic; `with.params` reports `registry.static_params`, reviewers keep
  `registry.static_reviewers`.
- `flow.repeat`: same `with.input` (round 1) and `with.params` (fixed for all rounds);
  `max_iterations` `1..=1000` (= `MAX_ROOT_REPETITIONS`; wire bound lowered from 10000); required
  step-level block (sibling of `rule`, forbidden on other kinds):
  `repeat: {next_input: <binding>, exit_when: <binding>}`. Both are deferred: not frozen at
  activation; evaluated after each successful round against the parent context (input, params,
  available step outputs, run metadata) plus the step's provisional output `{result, rounds}`.
  - Inside the block, `/steps/<self>/output` always resolves to the provisional output of the
    round just completed, never to an earlier activation of the same step in a graph loop.
    Self-reference is permitted only there; availability/type checks reuse `check_reference`
    with the current step treated as available.
  - The block's references to other steps are part of the step's input dependencies
    (`activation::input_dependencies`), so they load under the same
    latest-earlier-activation rule as `with` (F007.3).
  - `next_input` must conform to the child input schema; `exit_when` must be boolean
    (`check_binding_shape`).
  - Routes: `choices` with exactly `done` and `exhausted` (+ optional `error`); both carry
    output `{result, rounds}`. `done` = exit predicate true; `exhausted` = `rounds ==
    max_iterations` without exit. The author routes `exhausted`; nothing auto-approves.
- **Child contract (F007.2).** `registry::ChildContract` gains `parameter_schema` and
  `resources: Vec<ResourceRequirement>`, filled in `publication/dependencies.rs` from the child
  definition. The registry's `child()` uses them for params conformance and resources.
- **Child resources by name.** Each slot the child declares must be declared by the parent with
  identical `kind` and `contract` (`registry.child_resources` otherwise). The child run receives
  the parent run's bound resource IDs for exactly those slot names and nothing else. Rationale
  (F007.5): no new syntax; slot names are already the binding vocabulary; a later explicit
  `with.resources` mapping changes only compile-time selection, not the runtime contract (a
  restricted map either way).
- **Publication capture (S1).** No production `PublicationDirectory` captures children today
  (all return `children: vec![]`), so a parent with a child step cannot publish. New
  persistence wrapper `ChildPublicationDirectory<D>` (same shape as `AgentPublicationDirectory`):
  decode and parse the draft; collect the distinct declared
  `(child_workflow_id, child_version_id)` pairs and reject more than 256 (`MAX_DEPENDENCIES`,
  `publication.limit`) before any query. Then, inside one transaction that first runs
  `authority::authorize_company(actor)`:
  1. one batched query (`unnest` of the pairs) reads `id, octet_length(bundle)` from
     `workflow_versions` joined to an unarchived `workflow_definitions`, scoped by company and
     both IDs (same predicate as `verify_children`); reject when the summed size exceeds the
     4 MiB bundle bound (`publication.limit`) before any bundle is fetched;
  2. one batched query fetches those bundles; `restore_bundle` each; append to
     `PublicationDependencies.children`.
  A missing or archived child is simply not captured, so `dependencies::capture` reports
  `publication.missing` with the step's span. `verify_children` stays as the publish-time
  hash check. No production composition of `DefinitionService` exists yet. Tests compose the
  wrapper, and notice WF-C001 requires every production composition to include it.
- Registry help/examples/notes for both kinds drop "Execution unavailable". `context.history`
  help states that a child run's frozen history is empty (D1.2).

## D1.2 Runtime child creation (park)

New application port `WorkflowChildren::park_child(scope) -> AppResult<Option<ParkedWorkflow>>`
(pure runtime transition, no worker lease, like `park_wait`). The worker routes `workflow.call`
and `flow.repeat` jobs to it, then calls `resume_wait` when it returns `Some`. 2 s client
timeout; `waits::lock` (1000 ms lock/statement timeouts); lock order run → execution → job;
never company authority (runtime transition).

1. An existing wait for the execution → replay it (`waits::row(Execution)`). A run that is not
   queued/running or is past its deadline → `None` (deadline work expires it). Step kind not
   `workflow.call`/`flow.repeat` → `None`.
2. `pending_recovery::activate`: token gate, activation limit, frozen `with` inputs,
   activation debit; existing settlements.
3. Child configuration: bundle = `parent_bundle.children()[control.child_version_id]`; params =
   frozen `params` (`{}` when absent); resources = parent run resources restricted to the child's
   slots; `ConfiguredBinding::new` with the parent's binding id/revision validates params and
   resources against the child bundle. Validated before any repetition receipt (F010.5).
4. `flow.repeat`: repetition receipt `round:1` (D1.6).
5. `children::create_on(db, ChildStart)`, the single runtime child writer (D1.2a).
6. Wait row `reason='child_run'`, `child_run_id`, `child_ordinal`, deadline = parent run
   deadline, no notification intent; `completion_job::finish(Pure)`; parent run →
   `waiting/child_run`; audit `wait_parked` with `wait_id` (`audit_wait`).

Retries: any error rolls back child, receipt and wait together; a replay finds the wait. One
logical call ⇒ one child: unique wait `(company, run, execution, child_ordinal)`, unique
`(company, child_run_id)`, unique admissions source key.

**Side effects of `children::create_on`, compared with external admission (F001):**

| External admission effect | Child | Reason |
| --- | --- | --- |
| Run row | yes | Company, actor, channel/thread association and correlation copied from the parent (inherited authorization ceiling and effect scope); new trigger id; binding id/revision copied as provenance only (the child's configuration is its own row, read by the unchanged `read_saved_binding`); child workflow/version/bundle; frozen input/params/restricted resources; child `max_steps`/`max_context_bytes`; **deadline = the parent's absolute deadline**; parent link. Triggers add the immutable lineage row and the root budget link. |
| First execution + job | yes | Shared helper, refactored to take a struct instead of `PreparedAdmission`. |
| Thread history capture | **no** | The child receives only explicit inputs (plan §Child calls). A child `context.history` reads an empty frozen history, identical for every round and unaffected by later thread messages. Also removes the only unbounded write (≤ 10,000 rows) from the transition. |
| `workflow_admissions` row | yes | Source identity (below), inspection head, retry-safety check. |
| Admission command row | no | No external idempotency key; identity is the source key + wait. |
| `admitted` event | yes | Actor = parent's actor. |
| Company/association authorization, `verify_current`, readiness, token-reviewer authorization | no | Runtime transition. Authority came from root admission and the publication-frozen graph. Effects recheck current access at use time. Token policy is the root's, through the budget link. |

**Source identity.** Runtime child admissions use
`v1:["child", parent_run, parent_execution, step, null, ordinal]`. Domain:
`ChildCause::Execution(ExecutionRef)` is replaced by
`ChildCause::Step(ChildStepRef { execution: ExecutionRef, ordinal: ChildOrdinal })`
(`ChildOrdinal` newtype `1..=1000`); `ChildCause::Action` is kept for B2 and stays refused.
Inspection restores the 6-element key. Activation reads `/run/parent_id` from
`workflow_runs.parent_run_id`, not from the source key.

### D1.2a One creation path (F004)

`WorkflowService::admit` refuses `TriggerSource::Child` (`BadRequest`: children are created only
by their parent's step) before binding lookup; `admission_source::authorize` refuses it too. The
child branches in `admission.rs` (cancelled-parent check, child token reviewers),
`admission_write::insert_run`'s parent columns and `admission_source::child` are deleted;
`bounded_snapshots` no longer derives a parent. Every child therefore satisfies the phase-07
invariants (frozen pinned graph, recursion rejected at publication, inherited deadline, a
parent wait that consumes its wakeup). The deferred constraint in D1.3 enforces the single
path in the schema.

**Inventory (F008, complete).** Method: every function that constructs `TriggerSource::Child`
or `ChildCause::Execution`, then every caller of those functions in any file, including nested
`#[path]` modules. Child-constructing helpers: `wakeup_tests::admit_child`,
`action_reconciliation_eligibility_tests::child`, `action_reconciliation_claim_fairness_tests::
unique_child`, `budget_race_tests::child`, `control_acceptance_tests::child_command`, and
`harness_token_refusal_tests::{child, child_checkpoint}`. Inline constructions:
`admission_token`, `admission_source`, `admission_binding`, `activation`, `control`,
`control_guard`, and application `causality_cases`, `source_validation`. Files affected (18 in
persistence, plus the 2 application files): `admission_token`, `admission_source`,
`admission_binding`, `activation`, `control`, `control_guard`, `control_acceptance`,
`budget_race`, `budget_activation`, `budget_schema`, `wakeup`, `wakeup_guard`,
`wakeup_parent_link`, `harness_token_refusal`, `harness_token_scheduling`,
`harness_checkpoint_update`, and the `action_reconciliation_` files `eligibility`,
`claim_fairness`, `claim_race`, `claim_sql`, `claim_sql_wrong_execution`, `claim_boundary`,
`claim_recovery` and `owner_lifecycle`. No other child-constructing helper has callers outside
its file.

**Per-case rule.** Every affected test takes exactly one category, chosen by the state it needs.
Implementation records each test's name, category and reason in `logs/b1-fixture-moves.md` for
the batch review. No assertion is weakened; only fixture setup and identities change.
- **A — runtime chain (default).** The case needs one live line of runs in which every ancestor
  is parked at its child step. Publish the leaf child workflow the test needs, then each ancestor
  with a `workflow.call` entry step pinning the level below. Bind and admit the root, then run
  `park_child` on each level's entry job in turn: two levels give parent → child, three give
  root → child → grandchild (bounded by publication depth). The fixture returns every level's
  `ActivationRequest`. Expected here: the `wakeup` single-child cases,
  `budget_activation::child_ignores_held_ancestor_locks`, its deferred-commit and
  pure-I/O-refusal cases, `budget_schema::consumption_never_locks_root_run_or_execution` and
  `rollback_scope_mutation_and_owner_cascade`, `wakeup_guard` expiry/forged-fact/composite-FK
  cases, `wakeup_parent_link`, `control` retry refusal, and `activation` parent metadata
  (now read from `parent_run_id`).
- **B — lineage-only child (constructed state).** The case guards a shared SQL protocol for a
  state that the B1 and B2 runtimes never produce, because ancestors are always parked while a
  descendant runs and parallel branches are NOT_PLANNED. That covers a live descendant beside
  an ancestor that is not parked on it: a live I/O claim, reconciliation, or agent/token state.
  It also covers several live descendants of one root at once. The test-support helper
  `lineage_child(tx, parent_scope, child_bundle)` calls the production `children::create_on`
  and inserts the parent's `child_run` wait already settled (`state='cancelled'`, `settled_at`
  set) in the same transaction. That satisfies the deferred constraint while leaving the parent
  execution's own behavior unchanged: a settled wait is read only by `waits::row` for
  wait/child-kind steps, and the poll joins only waiting waits. Each such test carries a
  comment naming the unreachable state and the protocol it guards. Expected here:
  `budget_activation`/`budget_schema` `competing_descendants`, `wakeup_guard` `siblings`,
  `budget_race::descendants_share_without_locking_root_or_parent` when it needs more than one
  live descendant, `tokens::{refusal,scheduling,updates}` (child under an agent execution with
  token state), and the `action_reconciliation_*` eligibility, claim, recovery, boundary,
  owner-lifecycle and fairness cases (descendant beside an ancestor in reconciliation or
  holding a claim).
- **C — retired and replaced.** The case's subject was the removed binding-based child
  admission. `admission_source::parent_source_checks…` is replaced by the `TriggerSource::Child`
  refusal and the 6-element source-trigger checks. `control_guard::child_admission_competes_…`
  is replaced by competing `park_child`. `control_acceptance::child_admission_both_
  deterministic_orders` is replaced by cancel vs `park_child` in both orders (D1.5).
  `admission_token::child_inherits_root_reviewers…` is replaced by a chain child's token gate
  under the root policy. `admission_binding::source_key_has_stable_versioned_identity` is
  updated to the runtime key. The application `causality_cases`/`source_validation` child
  admission cases are replaced by the service refusal.
- Raw-SQL negative inserts (`wakeup_guard_tests.rs:168,183`, `wakeup_parent_link_tests.rs:99`)
  keep asserting their immediate errors; the deferred constraint fires only at commit.

## D1.3 Schema (init migration, edited in place)

`workflow_waits` gains `child_run_id uuid`, `child_ordinal integer`:
- CHECK `(reason='child_run') = (child_run_id IS NOT NULL)`, `(reason='child_run') =
  (child_ordinal IS NOT NULL)`, `child_ordinal BETWEEN 1 AND 1000`.
- FK `(company_id, child_run_id, run_id, execution_id)` →
  `workflow_run_parents(company_id, child_run_id, parent_run_id, parent_execution_id)`: a wait
  can name only a child of its own execution.
- UNIQUE `(company_id, child_run_id)`; unique index `(company_id, run_id, execution_id,
  child_ordinal) WHERE reason='child_run'`; `workflow_wait_execution` also excludes
  `child_run` (rounds need one wait each); `workflow_wait_one_active` unchanged.

`workflow_runs` gains `rerun_of_run_id uuid` (D1.7): UNIQUE `(company_id, id, workflow_id)`;
FK `(company_id, rerun_of_run_id, workflow_id)` → `workflow_runs(company_id, id, workflow_id)`
(same tenant and same workflow); CHECK `rerun_of_run_id IS NULL OR (parent_run_id IS NULL AND
rerun_of_run_id <> id)`; immutable (added to `preserve_workflow_parent`'s column list).

`check_workflow_parent_source`: a child key has 6 elements, `[4]` null, `[5]` an integer
`1..=1000`. Child park/completion audits carry `wait_id` (existing `audit_wait` CHECK).

Rerun original (F011): BEFORE INSERT trigger `workflow_rerun_original` on `workflow_runs`
raises 23514 when `NEW.rerun_of_run_id` names a run whose `parent_run_id IS NOT NULL`.
`parent_run_id` is immutable (`preserve_workflow_parent`), so checking at insert suffices.

Single creation path (F010.3): deferred constraint trigger `workflow_child_parent_wait`,
`AFTER INSERT ON workflow_runs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN
(NEW.parent_run_id IS NOT NULL)`. At commit it raises 23514 unless a `workflow_waits` row
exists with `(company_id, child_run_id, run_id, execution_id, reason) = (NEW.company_id, NEW.id,
NEW.parent_run_id, NEW.parent_execution_id, 'child_run')`, found through the unique
`(company_id, child_run_id)` key. A wait row is never deleted, and the run row keeps the link,
so the condition holds for the rest of the run's life.

## D1.4 Consumption (resume)

`wait_commit::resume_on` dispatches `reason='child_run'` (boxed) to `child_waits::resume_on`
before the generic event lookup. Locks only the parent (run → execution → job → wait). Without
locking the child, it reads the child row's `terminal_wakeup_sequence`, then the
`parent_wakeup` fact by primary key `(company_id, child_run_id, sequence)` (F010.1), and the
terminal execution output. The fact is written by the same statement's trigger that sets the
sequence. A runtime child is never control-retried (D1.7), so it has exactly one terminal fact.
- No fact → `Pending`.
- `succeeded`: output = child terminal execution `committed_output`, validated against the
  pinned child output schema (a mismatch is a defect: existing `fail(InvalidOutput)`).
  - `workflow.call`: `validate_completion(.., Success)` (incl. workflow output check at
    `$end`), activation-limit check as in `advance`, wait `completed`, run running,
    `batch_commit::complete(.., Parked)`.
  - `flow.repeat`: provisional `{result, rounds: ordinal}`; evaluate `exit_when` → true:
    complete with `choice:done`; else `ordinal == max_iterations` → complete with
    `choice:exhausted` (exit is evaluated first, so a last-round accept is `done`); else
    evaluate `next_input` and validate it against the child input; repetition receipt
    `round:<ordinal+1>`; settle the current wait `completed`; create child `ordinal+1` and its
    wait (D1.2 steps 4–6) in the same transaction; run stays `waiting`; return new
    `WaitProgress::NextRound` (counted as changed by `sweep_waits`).
- `failed` → Terminal `workflow.child_failed`; `cancelled` → Terminal `workflow.child_cancelled`;
  wait `completed`, run running, `recovery::route_parked_failure` (declared `error` route,
  successor gets `null`; else run fails with `terminal_execution_id`). The internal diagnostic
  carries child run id and terminal state; successors do not see it (F007.1 declined: error
  routes deliver `null` under the 05-D2 contract; typed child results come with B2's tool
  contract). Never fabricated success.
- Late/older children: a wait names exactly one `child_run_id`, so no other child's fact can
  settle it. Older rounds' decisions were settled inside their own terminal child, so a late
  submission is refused there.

Polling (`CANDIDATES_SQL`): a `child_run` wait is eligible when its child run (primary key
`(company_id, child_run_id)`) has `terminal_wakeup_sequence IS NOT NULL`, or by the existing
deadline/terminal-run clauses. `select_due` unchanged.

## D1.5 Cancellation and deadlines (F006)

`controls::cancel_on` becomes `cancel_tree_on(db, company, run, actor, cause)`. Before
cancelling the run's waits it reads `SELECT child_run_id FROM workflow_waits WHERE
company_id=$1 AND run_id=$2 AND state='waiting' AND reason='child_run' ORDER BY child_run_id`.
The `(company_id, run_id)` prefix of `workflow_wait_one_active` serves it. Today it returns at
most one row, because progression is sequential and an execution has one active wait. After
cancelling the run, it handles every returned child in that order (F010.2, so B2 needs no
change here). For each, it locks the child run `FOR UPDATE` and, when non-terminal, cancels it
the same way (boxed recursion; depth ≤ 32 by publication depth), with audit
`cancelled_by_parent` (actor = the commanding actor).

Invariant: every live runtime child is named by its parent's waiting `child_run` wait. The one
exception is after the parent's deadline, when the child (same absolute deadline) expires
through its own deadline work. D1.2a removes every other creation path, and park and round
create child and wait atomically. Lock order is company authority → parent run →
parent executions/jobs → child run → …, matching park and round, which lock the parent and
then insert the child. No child transition locks a parent: the wakeup fact is written on the
child's own run, and the parent reads child rows without locks. The child's terminal trigger
still appends its wakeup fact, which is harmless because the parent wait is cancelled.

Deadlines: child deadline = parent deadline (absolute, never extended). Child-internal
timeouts (decision deadlines) follow the child's own routes. A failed child yields
`workflow.child_failed`.

## D1.6 Budgets

Root budgets already cover descendants: activations, model calls and tokens debit the root
through each child's `workflow_run_budgets` link. Each round, round 1 included, inserts one
`workflow_budget_receipts` row on the parent execution: `resource='repetition'`,
`reservation_key='round:<n>'`, `quantity=1`. It is written in the creating transaction like
`budget::activate`'s activation receipt, so a replay returns the saved disposition. `exhausted`
→ run fails `workflow.root_budget_exhausted`, not routed (park: `settle(RootBudgetExhausted)`;
round: wait completed, run failed with `terminal_execution_id`, job closed). A revision never
resets usage.

## D1.7 Retry, revision, rerun (F003, F005)

- **Technical retry:** park/resume are idempotent replays of the same execution, wait, child
  identity and frozen inputs; no new child.
- **Human revision:** a new round = a new child run (own executions, decision, artifacts,
  actions); prior rounds stay immutable, linked by `(execution, child_ordinal, child_run_id)`.
- **Rerun with changed definition/input (F003):** `AdmitWorkflowRequest.rerun_of:
  Option<RunId>`. `WorkflowService::admit` checks only that `rerun_of` comes with
  `TriggerSource::Manual` (`BadRequest` otherwise). All original checks happen once, in the
  admission transaction's source authorization (`admission_source::authorize`, which also runs on
  replay), after company and association authorization of the new run:
  1. Lock the original `FOR SHARE` (company-scoped). Missing → non-disclosing `NotFound`.
  2. Authorize the actor for the original's association (`association::authorize_association`).
     Failure gives the same non-disclosing `NotFound` as a missing original.
  3. The original must be top-level (`BadRequest`) and have the same `workflow_id` as the
     selected binding's bundle (`BadRequest`; the version may differ, which is the changed
     definition).
  The run persists `workflow_runs.rerun_of_run_id`. D1.3 backs the tenant and same-workflow
  rules (FK), the new run's top-level shape (CHECK) and the original's top-level rule
  (`workflow_rerun_original` trigger). Replay equivalence includes `rerun_of` (a mismatch is
  `Conflict`). Inspection: `RunHead.rerun_of: Option<RunId>`. The original's state is not
  constrained; the link is for inspection only. Entry points are phase 08 (notice WF-C001).
- **Operator retry (F005):** `control retry` stays refused (`Unsafe`) for runtime children
  (`retry_on` refuses `TriggerSource::Child`; `workflow_control_retry_safe` excludes `child`
  source keys). A child's terminal `parent_wakeup` fact is immutable and may already be
  consumed. A parent failed by a child outcome (`workflow.child_failed`/`child_cancelled`,
  repetition exhaustion, invalid next input) closed its parked job without a failed attempt,
  so it is never retry-safe either. Recovery is a human revision round or a rerun. The stale
  `control_retry.rs:8-9` comment is replaced by this rule.

## D1.8 Template, fixtures, successor mapping

`human-revision-round.yaml` (child) and `repeated-human-revision.yaml` (parent) become the
executable shipped support template. The child runs draft (`agent.run`) → human review
(`decision.human`, choices accept / revise / escalate / cancel; `error` route → a `data.map`
yielding `choice: timeout`) → output `{choice, artifact, feedback, data}`, where `artifact` is
the exact reviewed/edited draft. The parent passes static `params`, sets `max_iterations: 3`,
`exit_when: choice != revise`, and maps prior artifact, feedback and data into `next_input`.
`done` → rule routing: accept → reply with the accepted artifact, escalate → specialist,
cancel → end, timeout → declared timeout route. `exhausted` → human escalation. A
successor-feedback fixture (plan §Feedback excerpt) proves explicit
`/steps/review/output/feedback|data` mapping. Production catalogue wiring stays phase 08/09.

## D1.9 Failure classification (F002)

| Transition | Failure | Class | Settlement |
| --- | --- | --- | --- |
| park | run terminal / past deadline | not runnable | no-op; existing deadline work expires run (and child) |
| park | token allowance pending | deferred | job stays pending (existing gate) |
| park | token refusal, activation limit | deterministic | existing `settle` (run fails) |
| park | invalid `with` input; params/resources rejected by the child bundle | deterministic | `settle(InvalidInput)` → declared `error` route, else run fails |
| park | `round:1` exhausted | deterministic | `settle(RootBudgetExhausted)` (run fails, not routed) |
| round | child output fails its schema | defect | existing `fail(InvalidOutput)` (run fails) |
| round | `exit_when`/`next_input` resolve error, non-boolean exit, next input not matching child schema | deterministic | Terminal `workflow.invalid_input` via `route_parked_failure` (F007.6) |
| round | `round:<n+1>` exhausted | deterministic | run fails `workflow.root_budget_exhausted` (not routed) |
| round | activation limit at `done`/`exhausted` | deterministic | existing `fail(ActivationLimit)` |
| both | lock/statement timeout, connection, serialization, unexpected constraint violation, child bundle absent from the parent closure (codec invariant) | transient or defect | propagate: full rollback, nothing partial; the worker logs company/run/error per candidate; re-offered at most once per fair-poll epoch of its company (cursor + horizon; never hot); terminal at the run deadline through the existing `deadline` work |

The transition's writes are bounded, so timeouts are not systematic: no history; one run row
(the child bundle is a subset of the ≤ 4 MiB parent bundle); one execution, one job, one
admission row, one event, one wait, ≤ 1 receipt.

## D1.10 Invariants and verification plan

Invariants: child + wait atomic; one child per (execution, ordinal); a wait settles only from
its own child; the parent holds no worker while waiting; children never lock parents;
cancellation reaches every live descendant; deadlines never extended; one repetition receipt
per round; exhaustion never auto-approves; last-round `done` wins over exhaustion; children see
no thread history; `TriggerSource::Child` is never admitted.

Focused tests (DB-backed, fresh schema; `own_database` for poll/sweep/whole-table assertions):
- Compiler/registry: static params, repeat block, routes, resources by name, self-reference
  only in the block, provisional-output resolution in a loop, max 1000, `ChildContract` fields.
- Publication: `ChildPublicationDirectory` captures pinned children; a missing or archived
  child gives `publication.missing`; the size bound; end-to-end publish of the template parent.
- Park: replay and competing parkers (one child); an injected failure between child and wait
  rolls back both; invalid params/resources routed; `round:1` exhaustion.
- History (F001): a message appended to the thread after the parent's admission is invisible
  to a child `context.history` (eligible 0); the parent's snapshot is unchanged.
- Resume: duplicate/competing resumers consume once; success/failure/cancel mapping and error
  routes; invalid child output.
- Repeat: feedback reaches the next round with the prior artifact/decision intact; last-round
  accept → `done`; exhaustion route; repetition budget exhaustion; invalid `next_input`/
  `exit_when` routed.
- Poison (F002), `own_database`: settled park and round candidates are not re-offered across
  two consecutive `poll_fair` iterations without advancing time. An injected unclassified park
  failure (test trigger) leaves no child or wait, is not re-offered on the immediately
  following iteration, and is retired at the run deadline.
- Cancel (F006): cascade through grandchildren; competing cancel vs `park_child` and cancel vs
  round transition (both orders) leave no live child under a cancelled parent; cancel vs child
  completion.
- Child timeout; restart (fresh persistence handle) mid-wait; older-round decision submission
  refused; only the final artifact sent once, with no replay of context loads or earlier
  effects; schema FKs/uniques reject cross-execution and cross-tenant wait and rerun links.
- Refusals (F004/F005): `TriggerSource::Child` admission refused; control retry on a runtime
  child and on a child-failed parent → `Unsafe`.
- Single path (F010.3): a raw child-run insert without its parent wait fails at commit (23514);
  `children::create_on` commits.
- Moved fixtures (F008): every test in the D1.2a inventory runs under its recorded category.
  Category A uses the runtime chain, including the three-level descendant cases. Category B uses
  `lineage_child` with its stated reason, and category C has its named replacement. The
  `logs/b1-fixture-moves.md` table accounts for every affected test in the baseline log.
- Rerun original (F011): a raw insert naming a child run as `rerun_of` fails (23514).
- Rerun (F003, F009): link persisted and inspected. An unknown or cross-company original and a
  same-company original in a channel the actor cannot access give the identical non-disclosing
  `NotFound` (same error value), and nothing is persisted (no run, admission, command or event
  rows). A child or different-workflow original → `BadRequest`. Replay with a different
  `rerun_of` → `Conflict`. A non-`Manual` trigger with `rerun_of` → `BadRequest`.
- Stack budget for the new boxed seams (`child_waits::resume_on`, cancel recursion, worker arm).

## D1.11 Coordination (proposed; activated after B passes this revision)

- **WF-C001 → p08:** `TriggerSource::Child` is not admissible. Delegation or other runtime
  children must use `children::create_on` semantics or a reviewed path satisfying D1.2a. Manual
  start/rerun entry points expose `rerun_of`. Every production `DefinitionService` composition
  includes `ChildPublicationDirectory`.
- **WF-C002 → p09:** child runs carry the parent's binding id for provenance only. Binding run
  lists filter `parent_run_id IS NULL` or label children. Run inspection shows lineage (parent,
  round ordinal) and `rerun_of`.

## D1.13 Constraint carried to B2

Agent workflow tools create children through the same `children::create_on` and `child_run`
waits (ordinal per pending call, or `ChildCause::Action`). B1's schema and ports must not assume
one child kind per step type beyond `child_ordinal`.
