# 04.7 eligibility poison subset

Implementation evidence only; root owns acceptance. Scope is row1 of the preserved
remaining matrix, BRIEF-04.7:194–240 and archive/RESUME-through-20261002-0619:352–384.
Full row1 and full04.7 remain incomplete. No04.8 or dependent runtime work.

Direct validation traced settlement::continue_on (settlement.rs:154–202), its sole
shared reopen predicate (20261001100000_workflow_action_claim_budget.sql:281–305),
and real retirement owner recovery::retire_on (:21–96), called by lease retirement,
budget reservation and completion. No production code or migration was changed.

New `workflow_action_reconciliation_eligibility_poison_codes_preserve_history`
isolates each of activation-limit, invalid-result, deadline and root-budget-exhausted
classified retirement codes. Each case starts with a genuinely admitted and activated
execution, actual pending provider entry, real classified release, and authoritative
provider barrier. SQL-derived action safety overrides the caller's safe label to
unknown and parks the run. A final proof goes through the actual command service.
Every non-poison reopen conjunct is asserted true after the blocked settlement,
including future DB deadline, spare attempts, correct lease-free failed attempt,
available budget, complete action evidence and no runnable sibling. The unchanged
production predicate is false; result is exactly Blocked/IneligibleJob.

Each case has its own equivalent normal `provider.rejected` retirement control that
commits Scheduled on the existing job. Negative cases preserve exact job bytes and
old execution/attempt/intent/marker/entry/admission/lineage/budget/debit/consumption
rows; record bounded evidence and command; create no episode or schedule witness.
Real claim returns None with full-public-table equality. Controls preserve old
accounting and attempts, job identity/counters, and zero new entry/effect/consumption.
There are four independently checked negatives and four paired positives inside one
test, plus the two existing model-budget/recheck tests in the affected filter.

These are classified-code discriminators, not a claim that an actual budget-owner
exhaustion receipt is independently isolated. The genuine reserve owner necessarily
records that receipt together with a budget-poison retirement. Unactivated/completed/
output/route/successor, failed lease shape, runnable sibling, activation/max_steps,
missing retired attempt and other root-budget constructors require the separate
remaining-eligibility contract and independent check before implementation. No
immutable facts, applied guards, counters or allowances were rewritten to construct
these tests. Accepted child and other prior evidence remains preserved.

## Frozen artifact and checks

HEAD9cff5164fa0694a4e7664f6ff8c69e473d4c2a97; initial WIP preserved.
Artifacts and full logs: `/private/tmp/workflow-eligibility-20261002/`.
`initial-hashes.json`, `initial.diff`, `initial.status`, `initial.head` record startup;
`freeze-hashes.json`, `review.diff`, `poison-final.rs` identify review bytes.

Changed source only:

- action_reconciliation_eligibility_tests.rs: add one nested module;
  SHA256ff5f100b3b366e8bc77ef7345ea2bb7a5113bc07698005fa213b97fda3d2618a.
- action_reconciliation_eligibility_poison_tests.rs: new isolated matrix;
  SHA2568efa92770e94b4e2d2a7ab028f88c9dc9199ab10c489fe6824848cebf186dc52.

Both DATABASE_URL and TEST_DATABASE_URL explicitly select retained
`postgres://mac03@127.0.0.1:55439/workflow_admission`. Test/check commands use
SQLX_OFFLINE=true and RUST_MIN_STACK=2097152; no test-thread override.
PG verified system7691265791172745923, data/private/tmp/workflow-admission-pg-e3aa,
port55439, mac03/workflow_admission, max_connections200. Retained data untouched;
fixtures use existing isolated disposable databases.

- `cargo test --locked --offline --lib workflow_action_reconciliation_eligibility`:
  final frozen source3PASS/0FAIL/0ignored,15.89s; `eligibility-final.log`.
- Original `poison.log` compiled but sandbox denied PostgreSQL maintenance connection;
  explicitly escalated authorized rerun `poison-escalated.log`1PASS,32.46s.
  `eligibility.log`3PASS preceded the named-fixture style correction; final rerun
  above is the current evidence.
- `cargo fmt --all -- --check` and `git diff --check`: PASS;
  `fmt-final.log`, `whitespace-final.log`.
- `cargo check --locked --offline --all-targets`: PASS29.68s; `offline-check.log`.
- `cargo clippy --locked --offline --all-targets -- -D warnings`: PASS45.61s;
  `clippy.log`.
- `cargo sqlx prepare -- --all-targets`: PASS24.08s; `sqlx-prepare.log`.
- `cargo sqlx prepare --check -- --all-targets`: PASS22.95s; `sqlx-check.log`.
  Both SQLx commands used the explicit two URLs with authorized local network access.

`preservation.json` confirms only the two assigned source files differ from startup;
all pre-existing A/C source, SQLx cache and migration bytes are unchanged. Root-owned
PROGRESS and Astra-owned contract changes are separate. `postgres-final.log` confirms
the exact retained identity, only postgres/workflow_admission remain, and no disposable
test databases survive. All commands completed. Graph refresh and the broader original
04.7 verification matrix remain final-group work; this subset creates no concurrency
protocol or production change. Graft discovery estimate totals3103205 saved tokens,
not measured spend/savings.

No commit, staging, deployment, reset, migration edit, bound increase or provider
implementation. Independent Astra code review and root acceptance remain pending.

Worker verified CODEX_THREAD_ID01a0fc03-d948-7b03-992e-55974bc26a5b;
startup26169/25840010.13%@09:48:42Z, preparation82926/25840032.09%@09:50:47Z,
final source milestone116652/25840045.14%@10:03:10Z. Sources are
token_usage_record.usage/task_started.model_context_window; no threshold waiver.
