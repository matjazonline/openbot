# Bounded V1 claim/reservation races — implementation evidence

Scope is ONLY V1 item 1 at
`/private/tmp/workflow-04.7-claim-budget-foundation-code-review.md`, against original
Execution 6, BRIEF-04.7:194–240,261–326, BRIEF-03.9-BUDGETS:27–58 and the frozen
claim-budget contract (SHA256
`4ea6c4f8652cdd1889ead7ac77abea7702e89b04bb70fa080b393236e45a3938`).
No 04.8, whole-04.7 acceptance, production change, migration change, limit raise,
stage, commit or deploy. Independent actual-code review and root acceptance remain
pending. All preceding WIP was preserved.

Baseline HEAD `c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc`; frozen evidence directory
`/private/tmp/workflow-04.7-claim-races-01a0f6e2/`. `initial-sha256.json`,
`initial.diff.patch`, `initial-status.txt` and `before/` identify inherited state.
Own code delta: one three-line child-module registration in
`action_reconciliation_eligibility_tests.rs`, plus the new 310-line
`action_reconciliation_claim_race_tests.rs`. No production test seam was necessary.

## Discriminatory checks

Each test creates an isolated, genuinely scheduled reconciliation episode and a
live descendant sharing the root budget. An external transaction holds only the
scoped usage row. Actual `reserve_budget` and both actual `claim_io` futures are
polled into identified PostgreSQL Lock waits before release. A dedicated observer
checks current database, exact SQL fragment, active Lock wait and immediate
`pg_blocking_pids` relationship, with a one-second observation deadline and
failure diagnostics. The queued usage waiters form gate → first waiter → second
waiter through PostgreSQL tuple locks; the other claim waits on the first claim's
requesting run. Sleeps only pace the predicate poll. Existing API/statement/lock
timeouts, lease policy and test-stack bounds remain unchanged.

- Debit first: actual granted reservation commits two model calls at the existing
  limit two. Both claims return None, no attempt is added, one exact episode
  refusal/audit is recorded, old attempts/execution/action/proof facts survive,
  and another unchanged-time claim makes no writes.
- Claim first: one claimant receives a fence and one attempt is added; the later
  real reservation is granted. Only that reservation changes usage/receipts.
  Actual `ActionService` dispatch then consumes one proof, creates one new remote
  entry and one provider effect. Saved receipt replay creates no extra I/O, entry
  or consumption, and changes no accounting/attempt/episode facts.
- Debit rollback: a test-owned deferred receipt trigger rejects only the real
  racing reservation at commit, with the intended injection diagnostic. Usage
  and prior receipts stay unchanged; exactly one claim and attempt succeed, no
  refusal/audit is created, and the same actual dispatch/replay checks pass.

Tests snapshot whole isolated-database state before/after and compare exact old
attempts, receipt membership/counts, expected usage counters, frozen execution,
episode, command/evidence/coverage, intent/dispatch/entry/consumption, provider
operation/ledger/effect facts; the debit-first path also reuses the exact scoped
refusal assertions. No manual database counter updates or sequential substitute
for the competing owners is used.

## Verification and retained fixture

All recorded database runs explicitly set DATABASE_URL and TEST_DATABASE_URL to
`postgres://mac03@127.0.0.1:55439/workflow_admission`; tests additionally set
SQLX_OFFLINE=true and RUST_MIN_STACK=2097152 and use locked/offline lib tests.
`commands.jsonl` records literal argument arrays, exits and times; complete logs
are separate files. Focused final `focused-r4.log`: **3 PASS**; full affected
`action-suite.log`: **122 PASS**, including eligibility/lifecycle/reconciliation;
`budget-suite.log`: **24 PASS**, including descendant ancestor-lock regressions.
Format/whitespace, locked offline all-targets, strict Clippy all-targets
`-D warnings`, SQLx prepare and prepare-check all-targets, migration info and final
graft build all PASS. All45 SQLx cache hashes are unchanged. Exact successful
commands/exits are in commands.jsonl; no check remained running at freeze.

Initial three focused runs failed in the harness, and their logs are retained.
Fresh activity snapshots and a separate observer improved diagnostics; the decisive
fix was checking the second queued waiter's immediate blocker instead of the
external gate. Earlier wait predicates let the real owners time out while the
diagnostic still showed the expected eventual usage wait. R4 passed without any
production protocol, deadline or timeout change; the full action suite reran it.

Retained PostgreSQL18 system identifier `7691265791172745923`, data
`/private/tmp/workflow-admission-pg-e3aa`, database workflow_admission, user mac03,
port55439, socket/private/tmp, max_connections200 was verified before startup.
All53 applied SQLx SHA384 checksums match actual immutable migration files, max
20261001120000; no migrations were added/applied/rewritten by this assignment.
`retained-checksums-before.json` and final hashes bind this fact. Test-owned
isolated databases migrated the same53 and are disposed by existing fixture owners.
Final identity query found zero other client sessions. The retained cluster is
clean STOPPED; pg-stop.log and pg-control-final.log verify shutdown and identity.

## Remaining scope

V1 items2–4 (new raw-SQL guard negatives, usage deadline/deferred claim/refusal
faults, consumed-binding ordinary retry/evidence-only revision) remain pending.
Separate-root/foreign-company competing episode isolation was not added in this
bounded assignment. Broader equality/receipt-only/late-result/cancel/recovery/
fairness/compatibility matrices, original04.7 combined integration gates and
independent actual-code review remain pending. These three race checks do not
replace any original criterion or imply foundation/full04.7 acceptance.

Worker verified UUID `01a0f6e2-6e71-7c51-9c4b-168b5527b782`; no child agents.
Root alone records acceptance. Latest measured milestone40.44% (104486/258400)
at2026-10-01T10:08:23.242Z; authoritative sources token_usage_record.usage /
task_started.model_context_window. Final context/quiescence and artifact hashes
are recorded in the frozen HANDOFF.md.
