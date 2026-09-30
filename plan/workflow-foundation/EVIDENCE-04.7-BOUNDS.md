# 04.7 bounded late sibling-overflow evidence

Implementation evidence only; root owns acceptance. This assignment does not complete
RESUME group1 or04.7. No04.8. Route validation used the accepted original Execution6,
BRIEF-04.7:103–108,159–191,307–308 and CONTRACT-04.7-AUDIT; no re-expansion or
production/schema correction. Preserve initial HEADc9cb4b434258b6d33ca26def4ff6b5728dd5a6bc
and all WIP. All49 applied migrations through20260930184000 remain immutable.

The bounded assignment added three isolated database tests in
`src/adapters/persistence/workflow/action_reconciliation_bounds_tests.rs`, three-line
parent module wiring in `action_reconciliation_sibling_tests.rs`, and pub(super)
visibility for the existing `accepted_limit` fixture in
`action_reconciliation_sibling_boundary_tests.rs`. Existing callers were traced before
that visibility-only edit. No runtime SQL query, production code, migration, bound,
port or SQLx cache was changed. SQLx prepare is not independently claimed here.

## Criterion mapping and validation decision

- `workflow_action_reconciliation_bounds_paused_128_to_129_settlement_refuses`:
  starts with128 genuine ordinary-dispatched receipts and an authorized snapshot.
  Genuine ledger verification pauses after its provider transaction releases locks.
  A joined competitor creates the129th genuine ordinary receipt using the current
  renewed fence; shared SQL safety becomes false. Settlement returns BoundExceeded,
  zero evidence, unchanged revision and complete all-public-table equality except
  its one immutable refusal command. PASS proves no truncated settlement/schedule.
- `workflow_action_reconciliation_bounds_sibling_overflow_before_proof_reserve_refuses`:
  one real lost/pending invocation plus127 genuine receipt siblings, genuine provider
  quiescence barrier, final evidence schedules exactly the existing job at128 markers;
  two real claimants produce one current owner. Ordinary dispatch then produces129th
  sibling and shared retry safety is false. Expected proof reservation refusal fails:
  reserve actually returns Reserved, creates a130th entry and consumes the proof.
  This is a production gap, not a synthetic-history or trigger-bypass fixture.
- `workflow_action_reconciliation_bounds_sibling_overflow_before_proof_enter_refuses`:
  same valid128-marker schedule; reserve commits proof consumption at128 markers.
  Joined ordinary work then adds129th genuine sibling. Expected enter refusal fails:
  enter_remote returns a usable RemoteEntry despite execution-wide overflow. No
  provider is polled through that returned entry by the failing test. Existing
  successful consumed audit remains durable and must never be refunded.

The cap applies to reconciliation/recovery evidence, preserving the unchanged
ordinary supported/initial-dispatch branches. Earlier accepted ordinary129 fixture
remains intact. Root's Astra interpretation resolved this from originals; it does
not certify a production fix. Current `workflow_action_not_applied_available` checks
only invocation entry count at migration180000:146–148 and lacks execution sibling
count. `reserve_entry_on` uses that predicate and `enter_remote` uses its exact
excluded-entry form. Reopen affected architecture before correcting immutable SQL
additively. No affected production edit was attempted.

Astra also identified a pending distinct entry-bound defect: the180000 predicate
counts ALL entries before excluding the exact consuming NEW entry. Genuine128prior
entries followed by one proof reservation may therefore fail its deferred guard at
129 total, although BRIEF159–181 excludes exactly consuming NEW from covered prior
requests. This worker did not construct that history; fresh Sol must independently
validate the reopened expansion and add the128/129 history discriminator.

## Verification and preservation

All tests use isolated AdmissionFixture databases, real durable provider operations,
requests/effects/barriers, current authority, actual prepare/dispatch/reconcile/claim
owners and all-public-table repeatable-read snapshots. No copied marker/receipt,
manual fake attempt, disabled trigger, reset/backfill or raised limit was used.

Exact command prefix for test logs:
`env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152`
followed by `cargo test --locked --offline --lib FILTER`.

- `/private/tmp/workflow-04.7-bounds-r1.log`: FILTERaction_reconciliation_bounds_tests,
  compile failure due sibling-private fixture visibility; corrected to pub(super).
- `-r2.log`: same obsolete filename filter; compile PASS but0tests, no runtime evidence.
- `-r3.log`: FILTERworkflow_action_reconciliation_bounds_,1PASS/2FAIL/0ignored,
  2324filtered,4.38s at stock2MiB. Both failure assertions isolate the actual boundary.
- `-r4.log`: final same prefix/filter after matching original host registration and
  improving reservation diagnostic. Final1PASS/2FAIL/0ignored,2324filtered,4.54s; reservation diagnostic proves
  received=reserved with130entries/1consumption instead of129entries/0consumptions.
  Final result and hashes in frozenHANDOFF.

`cargo fmt --all -- --check` and `git diff --check` pass; final logs -fmt-final.log
and -whitespace-final.log. No expensive unrelated suites or final all-action/affected/
Clippy/offlinealltargets/freshschema/SQLxcheck gates were run or claimed. Independent
actual-code review and root acceptance remain required; both failing tests remain
enabled to retain the signal until the authorized additive correction.

## Exact remaining scope and resource ownership

Group1 still needs genuine128/129 invocation remote-entry history at snapshot,
settlement,schedule,reserve,enter,shared recovery incl paused128→129; correction of
reconciliation-dependent sibling reserve/enter overflow; remaining child/runnable/
unactivated/completed/output/route/successor/max_steps/failed-lease/missing-retired-
attempt/poison/root-budget eligibility and claim budget recheck. Existing accepted
snapshot/shared ordinary recovery sibling tests remain reusable. RESUMEgroups2–4 and
FULL04.7 criteria/gates/review remain open. Root alone edits PROGRESS/RESUME/queue.

Retained cluster verified shut down before startup, system identifier7691265791172745923,
PG18 data/private/tmp/workflow-admission-pg-e3aa; explicit pg_ctl host127.0.0.1 port55439,
socket/private/tmp max_connections200. Runtime current DBworkflow_admission usermac03,
49 migrations maximum20260930184000; logs -pg-start.log and -pg-identity.log. Never
reset/drop retained data. PG remains running for root's next explicit owner; all
Cargo/test sessions finish before freeze, isolated fixtures dispose on failure too.
Own-before source copies/delta/all initial WIP hashes and frozen own/dependency/
migration manifests live in `/private/tmp/workflow-04.7-bounds-frozen/`.

Worker /root/bounds_eligibility Sol6.1/high UUID01a0f638-0975-7e21-880b-43d7775c9053.
Depth0 startup22560/2584008.73%; preparation96996/25840037.54%; milestone105865/258400
40.97%; post-r3sample110392/25840042.72%@2026-10-01T06:56:01.836Z. Sources
usage/window estimate; fresh final context.json stored with HANDOFF. No spawning.
Graft estimates saved163506 tokens plus map2763715; approximate discovery savings,
not token spend or an optimization claim.
