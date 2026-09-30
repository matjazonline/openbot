# Remaining claim SQL tests — bounded Architecture preparation

## Authorized A/C amendment — 2026-10-02

The implementation request supersedes the historical A/C acceptance and fixture
restrictions below only as follows. A accepts layered coverage: the unchanged
production guard accepts genuine current witnesses and rejects the genuine historical
pair on a reference relation; catalog attachment, genuine public scheduling and public
wrong-scope/ordinal controls remain required. This does **not** exercise the historical
witness diagnostic through an otherwise-valid public episode INSERT, nor independently
isolate the two transaction-ID predicates.

C accepts the real public run-update duplicate-candidate diagnostic after committed
adversarial P1/P2 preparation in a disposable database, with both protected scheduling
histories created by full application owners. P1, P2 and the two schedules commit
separately. The final attack rolls back every public table; database disposal removes
preparation. No fabricated protected history, disabled guards or production changes.

The original criteria and obstruction analysis below remain historical context.
The mandatory [Phase 10 integration reassessment](10-verification-and-cutover.md#reconciliation-stale-evidence-and-duplicate-candidate-integration)
retains the exact-public-boundary obligation. A/C do not complete all 04.7.
Implementation proceeds directly at the user's request; no Sol/Astra workflow or
independent-review PASS is implied. Evidence: [A/C record](EVIDENCE-04.7-SQL-AC.md).

## Historical preparation record

Status: amended proposal awaiting independent Sol correction recheck. Sol's check
`/private/tmp/workflow-04.7-remaining-sql-expansion-check.md` reports scoped preparation
PASS for B; A's original public-binding criterion is unresolved/blocked and C remains
blocked. No implementation or root acceptance PASS. Foundation, full V1 item 2,
original 04.7 and every other open matrix remain pending. No 04.8. Root owns
acceptance and PROGRESS/RESUME. Only B is proposed for next implementation after
the independent correction gate and root assignment.

Authority: Execution 6 in `04-actions-http-and-delivery.md:24–38`;
`BRIEF-04.7.md:194–240,261–326`;
`CONTRACT-04.7-CLAIM-BUDGET.md:52–118,178–220`; original V1 finding
`/private/tmp/workflow-04.7-claim-budget-foundation-code-review.md:47–65`.
Read-only validations `/private/tmp/workflow-04.7-remaining-sql-validation-01a0f772.md`
and `/private/tmp/workflow-04.7-episode-validation-01a0f757.md` supply proposals,
not replacement requirements. Preserve root-accepted original eight and additional
three cases, frozen in `/private/tmp/workflow-04.7-episode-alias-correction-01a0f772/`
with scoped review `/private/tmp/workflow-04.7-episode-three-code-review-final.md`.

## Shared decisions and boundaries

- Production SQL, all 53 migrations, all 45 SQLx entries and existing history remain
  unchanged. No API seam, disabled/replaced production trigger, shadowed production
  relation/function, copied protected public history, history deletion or rewritten
  attempt/retry counter is a fixture strategy. Raw invalid-state attacks are rolled
  back and explicitly distinguished from valid owner behavior.
- Reuse isolated real fixtures and complete owners. Scheduling owner
  `src/adapters/persistence/workflow/action_reconciliation/settlement.rs:5–79`
  owns its transaction and records the command last; `continue_on:154–202` uses the
  real reopen predicate. Existing `action_reconciliation_claim_sql_tests.rs:27–62,
  85–120` supplies genuine schedule, child debit, refusal insert and complete current
  retirement. `pending_recovery.rs:47–96` owns run/job/audit retirement; production
  `lease_claim.rs:127–174`, called by `claim_on:5–75`, invokes it before refusal.
- New test helpers belong in a bounded sibling SQL test module under
  `src/adapters/persistence/workflow/`, registered from the existing SQL test parent.
  Keep the accepted cases unchanged. Use descriptive SQL aliases with AS, explicit
  columns and bound values. Box existing deep fixture seams to retain 2 MiB stacks.
- Assert every unrelated prerequisite before the attack. An unexpected successful
  attack and a fixture failure must produce different failures from the target
  SQLSTATE/message. Force only the named target deferred constraint, never ALL when
  another queued guard could mask it. Compare every public table after complete
  rollback, including revisions, attempts, episodes, refusals, audits, witnesses,
  receipts, proof facts and accounting. Test-owned DDL has separate absence checks.
- Keep genuine committed scheduling/refusal positive controls. Guard-function
  coverage and production table boundary coverage must be reported separately.
  Independent Sol must check this coverage decomposition against the originals;
  neither a fixture relation nor a named unique key silently substitutes for a
  required public-boundary/duplicate-capture assertion.

## A. Historical episode: current-xid state/schedule pair

Original-criterion status: UNRESOLVED/blocked. The following paired current-transaction
success/historical-transaction rejection test invokes the unchanged production
`workflow_action_claim_episode_guard()` and is an optional partial contribution,
not a replacement criterion or required next implementation.
Affected production references, not edits:
`migrations/20261001100000_workflow_action_claim_budget.sql:40–54,69–108` and
`20260930181000_workflow_action_evidence_binding.sql:55–81`.

Create a uniquely named test-owned schema and an explicit reference-shaped relation
containing the six episode identity fields. This is a disposable reference fixture,
not a copy of the protected episode table: no LIKE, copied keys/history or production
table shadow. Attach a named deferred constraint trigger whose function OID is the
unchanged public production episode guard. Verify both this OID and the actual public
episode trigger's enabled/AFTER INSERT/deferred attachment in the catalog.

Install a narrowly scoped BEFORE real episode INSERT checkpoint before invoking the
unchanged full scheduling owner. At the checkpoint, insert NEW's six genuine identity
values into the reference relation and force only its constraint; require success.
Return the original NEW unchanged so the public episode and real scheduling commit
normally. The checkpoint must not fabricate any state/schedule witness, mutate the
command, suppress binding, change search_path or catch unrelated owner errors. Test
DDL must be visible on the owner connection; a connection-local temporary table on
the caller connection is insufficient. Remove the scheduling checkpoint afterwards.

In a later transaction insert the same six reference values. Assert the genuine
schedule and initial waiting/reconciliation witnesses are historical, their xids
differ from current, current matching witness counts are zero, and every non-xid
predicate in the guard remains true: exact scope/command/final revision, pending
lease-free job, matching retired ordinal, failed finished attempt and no greater
attempt. Force the reference constraint and require exact 23514
`invalid current workflow reconciliation claim episode`. Roll back and require
whole-public-table equality, including original protected witnesses. Remove all
test DDL and assert catalog absence.

Discrimination: removing both current-xid predicates would accept the historical
reference; the negative must fail distinctly on unexpected acceptance. This is the
witness-pair check, not independent mutation coverage of each conjunct. The positive
check in the real scheduling transaction rules out a fixture that always rejects.

Coverage limit confirmed by independent Sol: this exercises the production function
with genuine current/historical facts, but does **not** prove an otherwise-valid
public historical episode INSERT reaches the xid diagnostic. The original public
binding criterion (`CONTRACT-04.7-CLAIM-BUDGET.md:206–207`, V1 item 2) remains
unresolved/blocked. Existing wrong-ordinal boundary coverage, catalog attachment and
successful scheduling cannot establish the missing historical-xid public negative.
A committed scheduled command already has its append-only binding, so reinserting
its tuple hits a key before the deferred guard; replacing its command key fails
the scoped command provenance. Do not infer original-criterion satisfaction from
this partial option or alter the requirement to fit it.

Dependencies for any separately assigned partial test: accepted three-case schedule
probe patterns and genuine scheduling. Resolving A requires a separately prepared
and independently checked public-boundary design; no A implementation is proposed
in the next B-only assignment.

## B. Refusal revalidated after a greater attempt appears

Deliverable: actual public refusal INSERT succeeds, then its named deferred guard
rejects a greater-attempt raw-SQL attack in the same transaction.
Production references: claim-budget migration `:180–206,219–266`; fixture/owner spans
above. No copied transition, retirement witness or audit is permitted.

Start with genuine scheduled episode plus actual child budget debit. Begin transaction,
call existing `current_retirement` (real scope lock and complete pending owner), and
assert exact confirmed current-xid witness/audit, false budget, live deadlines, correct
retired owner/job and uncompleted activated execution, unchanged ordinal, no greater
attempt and unused refusal key. Insert the actual refusal with the existing helper;
require one row. Its real BEFORE guard, keys and FKs have now passed.

Insert one fresh `task_attempts` row for that real job with ordinal retired_attempt+1,
new id/generation/worker, nonempty machine id, status processing, and NULL classified
failure/retirement fields. Do not install a lease, reopen the failed job or change its
counter. This is a rollback-only attack on the shared ledger, not a successful next
claim. Require its actual insertion and reassert all refusal predicates other than
the no-greater-attempt condition. Force only
`workflow_action_claim_budget_refusal_commit_guard IMMEDIATE`; require exact 23514
`invalid current workflow reconciliation budget refusal`. Roll back the entire
transaction and compare all public tables; no new attempt or owner/refusal fact survives.

Feasibility: `20260817000000_init_schema.sql:3216–3235,4726–4735,7494–7495` supplies
row shape, free ordinal/id keys and task FK;
`20260928173500_workflow_classified_retirement.sql:2–14` and
`20260928200000_workflow_controls.sql:75–83` permit all classified fields NULL;
`20260930120000_workflow_action_dispatch.sql:4–5` adds fence uniqueness only.
Read-only exhaustive migration search found no task_attempts INSERT trigger. The
retirement guards attach to background-task transitions, which this attack does not
perform. Removing the deferred no-greater-attempt check would make the targeted SET
succeed; do not count failure of a different deferred trigger as coverage.

Positive control: on a fresh genuine fixture, actual `claim_io` commits the ordinary
ineligible refusal with no new attempt, and all existing refusal assertions pass.
No new production concurrency protocol is added; retain accepted competing-claimant
coverage. No unresolved design contract identified for B; runtime feasibility remains
to be established by Sol's database-backed implementation and independent review.

## C. Run-wide duplicate applicable candidates — blocked fixture contract

Required boundary: `workflow_action_capture_claim_retirement`, claim-budget migration
`:147–178`, must reject two applicable episodes across the same company/run with exact
23514 `multiple current workflow reconciliation claim episodes`, before arbitrary
selection. Its call to `workflow_action_pending_claim_episode:110–120` evaluates each
binding's own job/execution; it does not narrow the count to the retiring job.

Verified constraints do **not** prove this branch unreachable:

- Episode UNIQUE(company_id,job_id,retired_attempt) (`:44`) is per job. One workflow
  job per execution (`20260928133000_workflow_pure_batches.sql:27–38`) still allows
  distinct jobs/executions in a run. Execution ordinal uniqueness
  (`20260928130000_workflow_activation.sql:1–18`) does not bound active executions.
- Reopen's no-runnable-sibling predicate (`claim_budget.sql:281–306`) is checked by
  real settlement and deferred failed-to-pending guard
  (`20260930184000_workflow_action_audit_commands.sql:109–145`). It is not a declarative
  run-wide uniqueness constraint over all INSERTs and later transitions.
- The checked genuine sequential successor fixture cannot supply two candidates:
  `batch_commit.rs:5–67,69–90` creates successor while completing predecessor, and
  `20260928130000_workflow_activation.sql:25–45` prevents clearing completed history.
  A next claim also creates a greater attempt, permanently disqualifying that old
  episode. These are proofs about those construction paths, not every SQL history.
- Graph callers identify production successor through complete and the pending
  episode helper through claim-side eligibility plus SQL capture/confirmation.
  SQL functions are unindexed: exact migration search, not absent graph edges, is
  the SQL caller evidence. No complete global reachable-topology proof is asserted.

Exact blocker: no allowed construction has yet produced two different genuine
scheduled episodes in one run whose executions both remain activated/uncompleted,
whose jobs are pending at their original retired ordinals with no greater attempt,
while preserving provenance and reaching the public run UPDATE capture trigger.
One actual pending episode blocks ordinary scheduling of the second; progressing
the first through the checked real successor path invalidates its applicability.
This is an unresolved fixture/coverage contract, not established impossibility.

The missing deliverable is a bounded construction and predicate/constraint inventory
showing those two candidates exist **before** the attack. It may expose a transient
invalid state under raw SQL before deferred guards, but must retain genuine protected
episode/command/witness history, state exactly which new raw mutation is the attack,
and show earlier triggers/FKs cannot mask capture. It must use distinct jobs, assert
the production pending helper returns each exact command, force the public run-state
transition, capture the exact duplicate diagnostic, roll back all facts and keep a
genuine single-candidate owner positive control. An alternative unreachable argument
must exhaust insertion/update topology and caller paths; it cannot discharge the
original test criterion without a separately checked coverage decision.

A named per-job unique-key negative may be useful additional coverage, but cannot
complete C. Do not implement a fake second binding, shadow helper, copied trigger
body, guard removal or production schema/API extension to manufacture this branch.
Do not authorize dependent C implementation or accept full V1 item 2 from this draft.

## Gates and ownership

Independent Sol rechecks this status correction and any cross-effect on B against
the originals and actual source. Its prior scoped preparation PASS for B does not
accept this amendment or implementation. A and C stay blocked until their missing
designs are separately prepared and independently checked. Root may then accept
B's preparation and assign B-only implementation, followed by independent Astra
actual-code review. No self-certification in this document.

Implementation must record actual stock-2MiB focused tests, required scoped format/
whitespace/locked-offline-alltargets/strict-Clippy, migration/schema and SQLx
prepare/check evidence, then graph refresh as appropriate. Broader original phase
checks remain due at their existing boundary; prior evidence is reused only while
its code/service/fixture assumptions hold. No checks have run for this expansion.

Preparation owner: Astra UUID `01a0f788-1665-76b0-9aa8-dedcd96daf1e`.
Base HEAD `c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc`, all inherited WIP preserved.
Only this contract is written. No source/test/DB/build/SQLx/graph-build operation,
child, background process, permission request, staging or commit. Retained PG
system `7691265791172745923` is inherited STOPPED; no connection or state query.
Independent hash and fresh context sample are supplied to root outside this file.
