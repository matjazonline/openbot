# 04.7 revision overflow boundary fixture

Architecture proposal, 2026-10-02; corrected for independent Sol finding O1,
pending Sol recheck and root acceptance.
Scope is residual matrix row2a overflow only. Original BRIEF-04.7 section5 and
additional revision checks remain authoritative; no acceptance of row2a is recorded here.

## Decision and evidence

A bounded initial-revision fixture is feasible without disabling any guard or
rewriting a live run's protected revision. This is a numerical-boundary fixture,
not a claim that ordinary admission has executed billions of transitions.

- `migrations/20260928200000_workflow_controls.sql:4–20` defines a positive bigint
  revision with DEFAULT1 and a BEFORE UPDATE guard. Initial INSERT is not required
  to use1. Top-level UPDATE cannot set a different revision; subsequent generated
  changes remain monotonic and overflow remains an error.
- The same migration:22–44 advances revision through execution/job/wait owners;
  `20260930180000_workflow_action_evidence.sql:124–127` attaches that owner to evidence
  and conflict insertion. At bigint MAX the owned `revision+1` expression can raise
  PostgreSQL numeric overflow before the run trigger's explicit exhausted message.
- `src/adapters/persistence/workflow/admission_write.rs:29–67` omits revision from
  the run INSERT, so initial default injection can retain the real admission owner.
- `control_revision_tests.rs:76–103` confirms existing numerical-boundary intent,
  but its trigger-disable/UPDATE setup is not inherited as authorization here.

The original scope explicitly requires this overflow case and permits isolated
database acceptance fixtures. No missing product decision was identified: constructing
a row at an allowed initial numeric value neither grants reconciliation authority nor
changes the production initialization/transition contract. No production migration,
new bypass, bound change, synthetic evidence, or history backfill is needed.

## Constructor

Use a task-owned isolated migrated database. Seed the high revision **at the first
INSERT of the run**, before any execution, job, action, or reconciliation history:

1. Prefer a test-local explicit initial revision if an existing constructor permits
   it without duplicating the admission owner. Otherwise temporarily change only
   `workflow_runs.revision`'s DEFAULT in that isolated database, call real admission,
   and restore DEFAULT1 immediately afterward. This changes omitted INSERT input,
   not a constraint, trigger, function body, or existing row. Assert the restored
   default and unchanged enabled trigger definitions before the tested command.
2. Continue through genuine activation/claim/dispatch/provider/retirement owners.
   Preserve every enabled constraint/trigger and authentic registered-verifier history.
3. Determine bounded setup headroom from the corresponding normal-start fixture's
   generated revision delta (or a documented, asserted setup delta). Initial value
   is `target_revision - setup_delta`; assert the actual pre-command target exactly.
   A changed setup must fail that assertion, never repair the live revision.
4. Retain a matching ordinary-headroom positive control for the same command path.
   Both constructors must reach the same relevant action/job/evidence eligibility.

Explicit INSERT and default injection are equivalent initial-value constructors;
neither permits UPDATE of a protected live revision. Keep this permission local to
the numerical boundary fixture. Do not use it to manufacture grants/witnesses, relax
other SQL attack fixtures, or relabel synthetic state as ordinary admission history.

## Acceptance

Exercise reconciliation through its real service/registered verifier and transaction.
Assert an overflow error from the existing revision path (numeric out of range or
workflow revision exhausted), not a stale-revision/authorization/eligibility refusal.
Compare the complete public-row snapshot before and after: run/generated revision,
job/execution/attempt/accounting, evidence/coverage/conflicts/receipts/command/audit,
and existing provider facts must be unchanged. No command receipt may survive.

The mandatory partial-write discriminator starts an eligible recovered-Applied
command at MAX-1. Use a genuine durable provider effect with lost response and valid
recoverable output. Evidence insertion reaches MAX; evidence coverage and the
recovered receipt are inserted before the existing failed-job reopening mutation
overflows. Those earlier writes must roll back. The matching ordinary-headroom
control must return Scheduled with receipt_only=true and contain the expected
linked evidence, coverage and recovered receipt. Whole-public snapshot equality
after the overflow is mandatory, including the unchanged external provider effect
and exact MAX-1 run revision.

Inspect the actual settlement order to establish the partial writes preceding the
failure (`settlement.rs:57–61`, then `continue_on:193–196`). Require the existing
owned-revision arithmetic's exact underlying `bigint out of range` diagnostic
(SQLSTATE22003 when exposed), not merely any error. If implementation changes move
the failure to the explicit existing exhausted guard, explain that path and retain
an equally specific diagnostic; do not silently accept an earlier unrelated failure.
The command/audit writes occur after reopening and are not expected to execute on
this failing path; assert their absence without claiming they were rolled back
after execution. A MAX-start first-evidence failure may be supplemental only and
cannot replace this later-owner overflow. The separate deferred-commit rollback
matrix remains row3e.

Implementation evidence must record the initial value, setup delta, pre-command
revision, authentic positive outcome, exact overflow diagnostic, snapshot result,
and restored schema. No database/build was run for this read-only proposal.
O1 correction makes the later overflow, Scheduled control and whole-public equality
mandatory, selecting recovered Applied to include actual receipt rollback.
