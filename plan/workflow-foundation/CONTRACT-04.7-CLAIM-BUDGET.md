# 04.7 affected contract — budget eligibility at reconciliation claim

Status: proposed expansion for independent Sol check, **not an implementation gate**.
The fresh-path contract below is resolved. Populated-upgrade provenance is a bounded
compatibility blocker unless the preflight below proves no ambiguous pending episode.
Original Execution 6, BRIEF-04.7:194–240,261–326, accepted refinements/AUDIT/BOUNDS
and RESUME remain authoritative. No 04.8. All 50 existing migrations are immutable.

## Evidence and semantic boundary

Frozen reproducer: `/private/tmp/workflow-04.7-eligibility-01a0f680/HANDOFF.md`.
Its schedule control passes; after scheduling, actual child claim/reservation uses
2 of 2 shared model calls and two parent claimants incorrectly return one claim.
This is evidence of the missing next-claim check, not whole-04.7 acceptance.

Source paths below are under `src/adapters/persistence/workflow/`:

- `lease.rs:103–137` locks only requesting run, execution, job. `lease_claim.rs:5–72`
  then activates, checks fairness and installs the next attempt. This is the owner
  of the new check, before fairness/attempt installation.
- `activation.rs:44–111` returns frozen inputs without `budget::activate`. Do not
  put a global budget gate there: callers also include completion, failure routing
  and wait advancement (`completion.rs:110`, `recovery.rs:205`, `wait_commit.rs:71`,
  `waits.rs:247`). An already accepted effect/result must remain recordable.
- `budget.rs:24–68,76–93,113–143` and migration
  `20260929070000_workflow_root_budgets.sql:166–198` own accounting. A new positive
  charge is granted when consumed + quantity <= limit; exact replay returns its
  saved disposition without another charge. Exact-limit usage is not itself an
  exhausted reservation. Activation is charged once; model/repetition reservations
  name logical operations, not attempts (BRIEF-03.9-BUDGETS:27–58).
- `action_reconciliation/settlement.rs:154–202` delegates scheduling eligibility to
  `20260930180000_workflow_action_evidence.sql:319–346`. That deliberately stronger
  reconciliation predicate requires frozen provenance, activation <= limit,
  model calls < limit, repetitions <= limit and no owning-run exhausted receipt.
  It applies before deciding receipt_only. Preserve that existing policy for the
  exact next reconciliation claim; do not turn it into a universal resource debit.
- Ordinary retry's current predicate is in
  `20260930150000_workflow_action_receipts.sql:215–250`: it excludes exhausted
  receipts but does **not** impose the strict model headroom predicate. Therefore
  checking every frozen activation, or every job ever reconciled, changes ordinary
  retry semantics. The frozen failing test is a reconciliation claim, not evidence
  authorizing that broader change.

Receipt-only **new reconciliation claims** retain the same conservative eligibility
as their schedule. Receipt recording, truth attachment, saved accounting replay,
and already-live completion are not new claims and must not acquire this gate.
A claim is not a ModelCall/Repetition reservation and creates no such granted or
exhausted receipt. At activation/repetition equality, a frozen continuation remains
eligible; a genuinely new charge still goes through its existing accounting owner.
No refund, reset, replenishment, artificial operation key or invented attempt.

## Exact episode provenance, not historical command existence

Existing commands persist job/scope/result_revision but no scheduling attempt ordinal
(`20260930180000_workflow_action_evidence.sql:47–76`). Existence of any scheduled
command is insufficient: the job survives later attempts. Current-revision equality
is insufficient: later evidence-only commands can legitimately advance revision
before the first claim. Timestamp ordering and xmin are not durable substitutes.

Add narrow provenance linked to the existing scheduled command and existing job;
it is not another runnable queue, lease, retry counter or scheduling authority.
Persist company/run/execution/job, command key and **the retired attempt number at
schedule**, atomically with the existing transition and command receipt. Enforce
scoped FKs to job/execution/command and the exact retired task attempt, command
outcome=scheduled, and equality to the scheduling job's retry_count. Validate the
new binding at deferred commit using the existing current-transaction initial
waiting/reconciliation witness and final command result_revision, not client labels.
A new scheduled command must have exactly one binding; refusal/audit-only/replay
commands create none. Historical command and attempt facts remain immutable.

The exact claim discriminator is: pending lease-free matching workflow job, frozen
uncompleted execution, scoped scheduled-command binding, binding.retired_attempt
= current job.retry_count, and no task_attempt for that job with a greater number.
More than one matching episode is integrity failure, never arbitrary selection.
A successfully installed next attempt makes this episode inapplicable to all later
claims, even after safe retry or interruption. A rolled-back install does not.
Evidence-only revision changes do not erase it. Separate scheduled episodes can
reuse the existing job with different retired attempts and commands.

Store a durable, append-once **budget refusal fact for this episode**, linked to the
existing bounded root-budget-exhausted audit, without changing its original binding.
Sol may use a companion immutable refusal row or a guarded single-assignment field
on the binding; it must not become another job state machine. Replays cannot clear
it. Ordinary and reconciliation explicit retry predicates must reject that exact
refused execution/episode, so ordinary retry cannot bypass the refusal merely
because no new accounting request was made. All other ordinary retries retain their
current behavior. Preserve original attempt safety, failure, retirement and counts.

The refusal insert/single-assignment **database guard**, not the caller's label or
audit FK alone, must establish all of the following in the same transaction:

- Exact company/run/execution/job/command/retired-ordinal equality with the applicable
  episode; no already-installed greater task attempt, both when refusal is proposed
  and at deferred commit. A historical schedule witness alone proves no refusal.
- The episode was genuinely pending and lease-free immediately before its current
  pending-to-retired transition, and that transition belongs to this transaction.
  Reuse the existing run/job/attempt state and database-owned transition witnesses;
  extend narrowly where they lack the exact job/episode OLD-state fact. Merely finding
  an already failed job or accepting supplied previous-state fields is insufficient.
- Actual current budget ineligibility from the shared predicate under the documented
  requesting-run → execution → job → usage serialization. The database guard acquires
  or verifies that same serialization and reads current usage; a fabricated refusal
  for an eligible episode cannot commit even with otherwise valid scoped references.
  Active state/deadline before retirement and the actual pending retirement outcome
  must agree with the existing owner, without fabricating a budget reservation.
- The audit is the event emitted by this exact current retirement: equal company,
  run, nonnull execution and sequence, with literal `workflow.root_budget_exhausted`
  kind. Establish current-transaction retirement linkage through database-owned
  transition evidence; a historical same-kind event, another execution's event or
  an independently inserted caller-labelled event cannot stand in for it. Event
  presence alone is not proof of the transition or budget condition.

At deferred commit, refusal, exact retirement and linked audit must agree atomically;
absence or mismatch rolls back all three. Protect the linked audit's company/run/
sequence/execution/kind and deletion after binding, including changes later in the
same transaction; its identity and reason cannot be substituted or erased. This is
a narrow guard for refusal-linked audits, preserving unrelated historical audits
and the accepted `action_reconciled` actor-audit contract. Exact SQL representation
remains Sol's local choice; these invariants add no queue, lease or fake charge.

## Enforcement, serialization and retirement

1. Reuse claim's requesting run → execution → job lock order. Resolve the immutable
   scoped root linkage and lock only its shared usage row, then read current counters
   in a fresh statement after acquiring the lock. No parent/root **run** lock is
   added; for a root claimant its own existing run lock is naturally retained.
2. On a matching episode, re-evaluate the exact budget portion above under the usage
   lock. Hold it through claim/refusal commit. Do not call the entire reopen predicate
   in a state where its failed-job/waiting-run assumptions no longer apply. Prefer
   one shared SQL budget predicate reused by schedule and claim; if duplicated,
   require a DB equivalence matrix. Any SQL change is an additive migration.
3. Usage-lock acquisition orders a competing sibling reservation against this claim.
   Debit commits first: claim sees its counters and refuses when ineligible. Claim
   commits first: claim may succeed; the later debit is permitted. The claim does
   not reserve speculative model allowance or promise headroom until dispatch.
   Subsequent real model/repetition work must use its own reservation owner.
4. Recheck requesting-run active state and DB deadline after the usage wait and
   after later waits before commit. Keep existing persistence/statement/lock timeouts
   and final live_window. Timeout/cancellation/transaction abort rolls everything
   back; it must not commit a refusal based on an expired eligibility snapshot.
5. Budget refusal creates no lease/attempt, consumes no proof, creates no remote
   entry and changes no usage or prior receipt. Use existing
   `pending_recovery::settle(...RootBudgetExhausted)` (`pending_recovery.rs:47–96`)
   for the pending job/run/audit, atomically with the refusal fact. That owner selects
   Failed versus waiting/reconciliation from actual action truth; do not force a
   success/error route. Subsequent unchanged-time claims/polls cannot hot-loop.
6. Check before `fairness::available` (`fairness.rs:16–56`) to retain existing usage
   → capacity ordering used by first activation. Preserve tenant capacity/tickets;
   budget refusal grants no slot. Do not hold shared usage while acquiring another
   run or introducing a capacity → usage path.

## Upgrade boundary and delivery order

Sol independently checks this artifact, including exact predicate semantics and
schema feasibility, before code changes. Then implement binding/refusal persistence,
shared budget predicate and retry poison guard additively; integrate existing
settlement/claim/pending retirement; add actual-owner tests. No public budget API,
production model handler or new dispatch framework is needed.

For retained history, do not fill a historical command's missing attempt ordinal
by timestamps, MAX(remote entry attempt), current retry_count, or revision guesses.
A failed attempt may have produced no entry; a later ordinary retry may reuse the
same job. Preflight must identify pre-existing pending jobs with scheduled historical
commands but no provable episode binding. **Such ambiguous valid history blocks this
upgrade; do not silently backfill, waive the gate, or retire it as if budget-refused.**
Fresh databases and histories with no such active ambiguity can proceed, preserving
all old command/evidence/audit/attempt/accounting bytes. Resolving an actual ambiguous
retained episode requires a separate, explicit compatibility design before deployment.
This is an identified migration-feasibility gap, not a request to widen budget policy.

## Discriminatory acceptance

- Preserve the frozen control and make its two-competing-parent-claim test pass:
  committed child Granted(2), exact shared limit=2, zero returned parent claims,
  zero new attempts/entries/consumptions, unchanged usage and old receipts; one
  durable refusal/audit/retirement. Both repeated claim and ordinary RetryCommand
  cannot reopen the refused episode. Never satisfy this by mutating test limits.
- Race actual sibling reserve_budget against two actual claim_io calls, deterministically
  exercising both usage-lock orders. Debit-first gives zero claims; claim-first gives
  exactly one claim, at most one new attempt, unchanged claim-side budget usage and
  at most one subsequent proof consumption/entry through the actual dispatcher.
  A sibling rollback leaves allowance and eligibility unchanged. Separate roots and
  foreign companies cannot affect one another's predicate.
- Exact-limit activation/repetition do not block frozen continuation or double charge.
  Model equality blocks this reconciliation episode, including receipt-only new claim;
  below-limit permits it. Saved Granted replay at equality remains Granted without
  debit. Ordinary automatic/explicit retry with a legitimate saved logical reservation
  remains unchanged when it has no matching current reconciliation episode.
- Claim, then real safe retirement/retry: the old binding no longer applies. Later
  genuine reconciliation scheduling creates a distinct binding. Evidence-only revision
  advance before first claim retains the binding. Historical/wrong job, execution,
  company, command, ordinal and duplicate candidate negatives fail the exact guard.
- Current receipt-only, mixed and supported-replay matrices remain intact. Exhaustion
  after a valid live claim must not prevent actual late receipt storage or completion
  of already accepted work; pending new reconciliation claim remains separately gated.
- Usage wait crossing deadline/persistence timeout; cancel-first and claim-first;
  cancellation during claim wait; lost claim response and normal expired-lease recovery;
  injected deferred failure after candidate claim and after refusal. Compare full
  scoped job/run/revision/attempt/episode/audit/receipt/usage/proof state. No partial
  binding, refusal, debit, lease, fairness mutation or fabricated attempt survives abort.
- Held parent/root run+execution locks do not block a descendant claimant's usage
  check; retain existing budget descendant no-parent-lock and tenant fairness tests.
  Concurrent same-root debits and claims must not deadlock under the documented order.
- Fresh migration and unambiguous populated upgrade preserve all history. Ambiguous
  retained pending provenance produces the named preflight failure with zero mutation.
  New raw-SQL scheduled-command/binding/refusal negatives isolate scoped FK, immutable,
  deferred current-transaction and attempt-ordinal guards rather than earlier failures.
- Isolated raw-SQL refusal negatives: otherwise valid eligible episode plus fabricated
  refusal/retirement audit; valid ineligible episode plus historical same-execution
  exhaustion audit, wrong-execution audit, wrong-kind audit, or caller-labelled event
  lacking the current retirement; refusal after the first next attempt was installed;
  and replacement/mutation/deletion of a refusal-linked audit's protected identity or
  reason. Include both same-transaction substitution and later mutation attempts.
  Arrange all unrelated FKs/checks to pass and assert the intended refusal/audit guard
  diagnostic or named constraint. Compare full scoped episode/refusal/audit/run/job/
  revision/attempt/accounting state: every failing transaction rolls back completely.
  Keep genuine owner-path refusal as the positive control; unrelated historical
  audits and accepted reconciliation actor-audit behavior retain their contracts.
- Required broader stock-2MiB action/affected suites, format/whitespace, offline locked
  all-targets, strict Clippy, migrations/schema, SQLx prepare/check and graft remain.
  Full original-criteria actual-code/integration review and root acceptance stay pending.
