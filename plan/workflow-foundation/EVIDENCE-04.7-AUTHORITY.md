# 04.7 bounded authority / replay / verifier-race evidence

Implementation evidence only. Root owns acceptance; independent focused review and
full04.7 original-criteria/integration/gate review are pending. No04.8 work.
Preserve baseline655aaaba3132c561809f97fd25c89f600bca1fa9 plus all existing work;
applied migrations through20260930184000 remain immutable. No production/schema edits.

New test files:

- `src/adapters/persistence/workflow/action_reconciliation_authority_tests.rs`:
  seven isolated real-DB tests, nested under existing provider_tests to reuse its
  private authoritative request/effect/closure fixture.
- `src/adapters/persistence/workflow/action_reconciliation_authority_support.rs`:
  matrix-local database-backed actor grants implementing existing resource and
  dispatch ports; counting/pausable wrapper delegates actual authoritative ledger.
- Existing provider file gains only child-module wiring. Receipt file was restored
  byte-identical to its pre-assignment snapshot. No shared production port changed.

Meaningful assertions now passing:

1. Current owner and distinct admin may record Unknown; member/outsider cannot,
   despite valid resource grants. Denial preserves complete durable snapshot and
   calls verifier zero times; allowed audit-only commands preserve exact existing
   job, execution, attempt, budget receipts/usage and remote entries.
2. Forged company/run/execution/invocation/digest/marker reject before verification
   with zero durable writes. These are nonexistent forged identities; authentic
   valid foreign tenant/association cases remain required separately.
3. Every caller-claimed UnknownNote disposition remains Unknown. Fresh service
   reconstruction/exact duplicate retains outcome/revision/evidence without facts,
   audit, revision or verification changes; changed expected revision conflicts.
4. Authoritative active-provider Unknown also persists/replays after fresh service
   reconstruction, verifies once, refuses ordinary retry and claim, preserves
   original attempts/debits/entries. Current resource revocation rejects exact replay
   and a new command without re-verifying.
5. Distinct admin reconciles a real applied effect with lost response into a saved
   receipt and schedules receipt-only continuation. Actor audit names admin while
   run actor remains original. Successful duplicate returns exact outcome/revision/
   evidence with no reverify or mutation. Changed authorized actor, revision, input,
   and reference conflict with same key. Admin resource revocation or role demotion
   rejects exact replay. Revoked original dispatch actor cannot return that saved
   receipt through actual dispatch; zero new provider calls, existing receipt kept.
6. Paused genuine ledger verification permits competing company/run/resource locks
   and writes within bounded time. Resource revocation and principal demotion deny
   settlement; revision change returns StaleSnapshot. All leave zero evidence and
   no claimable job. No detached competitor task: tokio::join owns both futures.
7. Actual CancelCommand and existing expire_run progress while verifier is paused;
   stale settlement refuses without evidence. Generated terminal run, events, jobs,
   executions, attempts, budget and entries remain unchanged by the refusal; no claim.

Exact retained PostgreSQL identity inspected before work:
workflow_admission / mac03 / data/private/tmp/workflow-admission-pg-e3aa /
port55439 / max_connections200. Both URLs set explicitly below. Tests use existing
isolated AdmissionFixture databases, never reset/drop retained data. Sole PG/Cargo/
SQLx ownership came from root after scoped audit code-review PASS.

Commands and complete logs:

```
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests::provider_tests::authority_tests
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests
cargo fmt --all -- --check
git diff --check
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=false RUST_MIN_STACK=2097152 cargo sqlx prepare -- --all-targets
```

Focused initial6PASS/0FAIL in `/private/tmp/workflow-04.7-authority-r3.log`.
Final combined17PASS/0FAIL/0ignored (new7 + retained10),2290filtered,6.73s,
compile28.97s: `/private/tmp/workflow-04.7-authority-combined-r2.log`.
Intermediate compile-only corrections: r1 private fixture items (fixed by nested
module); r2 wrong enum spellings (actual UnknownRecorded/Unsafe) and private helper;
combined-r1 missing WorkflowPolling trait import. Full logs retained with those names.
Formatting and whitespace logs `/private/tmp/workflow-04.7-authority-fmt.log` and
`-whitespace.log`; final status recorded in frozen HANDOFF. SQLxprepare PASS21.68s,
`/private/tmp/workflow-04.7-authority-sqlx.log`; no .sqlx changes. Runtime SQL fixture
queries remain verified by DB tests, not by prepare's macro cache.

Remaining exact authority/verification obligations: authentic valid foreign tenant,
channel/thread association denial and association revocation; operational directory/
transactional resource errors; owner membership/principal revocation beyond tested
admin demotion; actual dispatch-call branch with distinct actor (current saved-receipt
branch is tested); paused changed entry coverage/operation/marker/late receipt; proof
expiry/future/bounds and expired-successful replay; timeout/caller cancellation/drop
ownership; cancellation/deadline ordering with applied/final proof rather than paused
Unknown. Other full04.7 matrix, audit upgrade/malformed history/deferred rollback,
proof consumption/restart/siblings/shared-conflict checks and broader stock2MiB,
offline all-targets, strictClippy, fresh migrations, SQLxprepare/check, graft and FULL
independent integration review remain required. No full04.7 acceptance is inferred.

Preparation/criterion mapping: `/private/tmp/workflow-04.7-authority-preparation.md`.
Freeze before focused Astra review: `/private/tmp/workflow-04.7-authority-frozen/`.
Worker /root/reconciliation_authority_matrix, Sol6.1/high,
verified UUID01a0f45d-00b9-7173-a0da-47258e5d1f8b. Startup24976/2584009.67%;
post-SQLx112180/25840043.41%@2026-09-30T22:23:15.557Z depth0 estimate;
usage/window sources. Final fresh sample in frozen HANDOFF. Worker50% enforced.
