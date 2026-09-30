# 04.7 affected contract — descendant claim without ancestor locks

Status: architectural reconciliation proposed for independent Sol check. No code
acceptance or change to child eligibility. Scope is queue 2e's descendant criterion
only; accepted claim-budget/race/recovery contracts remain intact. Execution 6 and
BRIEF-04.7:194–240 remain authoritative. No 04.8 or upgrade/SQL matrix expansion.

## Reachability and decision

BRIEF-04.7:196 permits only a **non-child** run to reopen through reconciliation.
This means the admission source, not merely a run waiting on a child:
`20260930180000_workflow_action_evidence.sql:319–346` excludes source kind `child`;
the current replacement in `20261001100000_workflow_action_claim_budget.sql:281–305`
preserves that exclusion. `action_reconciliation/settlement.rs:154–202` consults
this predicate before changing the failed job to pending. Genuine child admission
therefore cannot schedule a descendant reconciliation episode, even with accepted
final-not-applied evidence and available shared budget. Truth may still be recorded.
The frozen two-test failure in
`/private/tmp/workflow-04.7-claim-fairness-01a0f92c/HANDOFF.md` is expected policy,
not evidence of a production claim or locking defect.

Interpret CLAIM-BUDGET:203–205 as preservation of the shared claim/accounting
architecture for ordinary descendants, combined with real reconciliation races.
It does not grant child reconciliation eligibility. That contract itself preserves
ordinary claims, forbids adding ancestor run locks, and distinguishes the exact
episode check from universal accounting. Its no-ancestor-lock guarantee can be
tested through a reachable descendant's first ordinary I/O claim and activation.
Do not report that test as executing a descendant reconciliation episode or the
episode-specific strict model-headroom predicate.

Actual owners (paths relative to `src/adapters/persistence/workflow/`):

- `lease.rs:26–42` → `lease_claim.rs:5–75` →
  `reconciliation_budget_eligible:127–174`: all pending I/O claims pass this seam;
  no episode returns true before taking the reconciliation usage lock.
- `pending_recovery.rs:27–45` → `activation.rs:44–111` → `budget.rs:113–143`:
  a **not-yet-activated** descendant then obtains its real activation allowance.
  Frozen activation skips this accounting call and cannot prove the usage path.
- `20260929070000_workflow_root_budgets.sql:166–198`: the receipt owner locks the
  requesting run and shared usage, never ancestor run/execution rows. Real episode
  claims use the same usage row before fairness, without charging model allowance.

This is an explicit correction of the failed preparation assumption. No bypass
episode inserts, disabled triggers, edited admission sources/lineage, SQL policy
changes, or root fixture relabelled as a descendant are authorized. No unresolved
product decision is needed for this decomposition. Requiring an actual descendant
reconciliation episode instead would conflict with the original non-child policy
and must reopen product scope rather than be inferred from this test criterion.

## Deliverables and discriminatory checks

1. Replace the impossible new tests in
   `action_reconciliation_claim_fairness_tests.rs` (and adjust only necessary module
   wiring/names) with reachable coverage. Admit root → parent → descendant through
   actual admission with unique logical keys; assert child source and immutable
   shared-root links. Keep the descendant's first I/O execution unactivated with no
   budget receipt or reconciliation episode before claiming.
2. Hold **both** root and immediate-parent run and execution rows `FOR UPDATE` in
   another transaction. Start two actual `claim_io` competitors for that descendant
   with distinct workers. Both calls must finish successfully within a bounded
   timeout **before releasing ancestor locks**; exactly one returns a live attempt
   1. Assert exactly one new granted activation receipt and shared activation debit,
   no model/repetition debit, and unchanged ancestor rows and prior accounting.
   No reconciliation episode/refusal/proof consumption/remote entry is created.
   This detects unintended ancestor locks anywhere on the shared claim seam and
   proves a real usage check; a preactivated fixture or zero successful claims fails.
3. Retain the failed genuine child action setup as a policy regression test with
   the correct expected result: authorized final-not-applied evidence is recorded,
   but outcome is `Blocked { IneligibleJob }`, the failed job remains failed and the
   run remains waiting/reconciliation. Establish that budget and other ordinary
   reopen preconditions are satisfied; use a corresponding non-child scheduled
   control to discriminate lineage policy from a generally broken fixture. Assert
   unchanged execution/job/attempt/accounting/entry/consumption state, no episode or
   refusal, and no subsequent claim. Allow only expected evidence/command/audit and
   owner revision changes; preserve historical facts. No artificial exhausted
   descendant reconciliation branch remains.
4. Preserve and run the existing descendant accounting no-parent-lock tests
   (`budget_race_tests.rs:212–282`, `budget_schema_tests.rs:177–207`), descendant
   activation control (`budget_activation_tests.rs:258–290`), and tenant fairness
   suite. Existing accepted actual root reconciliation versus descendant reservation
   races continue to prove both same-root usage-lock orders, rollback and no deadlock;
   reuse their evidence only after verifying unchanged source/fixture assumptions.
   They separately own strict episode eligibility/refusal and actual dispatch checks.

Sol must independently check this decomposition against originals and current
owners before test edits. No production change is anticipated. Preserve inherited
WIP and accepted evidence. Run focused DB tests at stock 2 MiB plus formatting and
required repository checks; record exact filters/counts and distinguish this fresh
coverage from reused race evidence. Root alone records acceptance after independent
Astra code review. This document itself proves neither tests nor implementation.
