# 04.6 — action uncertainty parking and durable audit

Status: affected expansion independent PASS; root ACCEPTED 2026-09-30 17:49Z. Implementation, original criteria, integration and required gates independently PASS; root ACCEPTED 2026-09-30 18:29Z.

## Original scope and preserved boundaries

Execution item 5 in 04-actions-http-and-delivery.md remains authoritative: ambiguous
post-dispatch effects without supported replay/reconciliation must mark the invocation
as needing reconciliation and park its execution; never repeat a potentially applied
non-idempotent write. Preserve accepted 03 and 04.1–04.5 contracts. Protected actions
stay undispatched through 05. Evidence settlement is 04.7, the full cancellation matrix
04.8, production adapters 04.9/10, messaging 04.11, Rig 06, bypass removal 08.
No fabricated not-applied evidence, new job/attempt/lease owner, ancestor lock,
compatibility/backfill, or applied migration edits. Root owns PROGRESS/RESUME.

## Concrete affected contracts

1. Existing recovery::retire_on is the sole live/expired attempt retirement owner.
   Its shared workflow_action_retry_safe predicate overrides handler-supplied safety.
   Unsafe/malformed/missing/expired replay evidence or unsafe sibling forces
   reconciliation irrespective of failure class, output rejection, root budgets,
   retry allowance, operation timeout, or historical read-handler defaults. Supported
   replay retains accepted 04.5 scheduling/re-entry checks. Preserve source failure code
   and class on task_attempts; do not rewrite a provider error to claim not-applied.
2. Add an immutable, tenant/scope/digest/dispatch-linked first reconciliation fact per
   invocation, workflow_action_reconciliations, using an additive migration. It stores
   the classified first failure code and observation time, references the original
   remote marker and frozen intent, and has no ownership or scheduling columns. All
   unresolved remote markers of the parked execution are recorded in the retirement
   transaction, with no receipt fabrication. Duplicate observation is idempotent.
   Preserve original marker/entries/receipts and existing attempt/run audits. A SQL
   state projection workflow_action_effect_states derives prepared, possible_dispatch,
   needs_reconciliation, or committed from those facts, with receipt truth taking
   precedence after a late receipt without deleting the reconciliation audit. The
   projection is internal persistence infrastructure; no new unauthenticated public
   read/command API. 04.7 will consume the existing scoped facts under its authority.
3. Database constraints reject foreign tenant/scope/digest/marker provenance and
   non-remote linkage; reconciliation facts are append-only. An insertion guard
   requires unresolved effect truth and the owning run parked for reconciliation or
   terminal (failed/cancelled), rather than accepting a fabricated reconciled grant.
   No evidence from this table authorizes replay or changes workflow_action_retry_safe.
4. Pending deterministic failures must consult the same action predicate before any
   error edge/failure. Any unresolved unsafe prior marker parks even for invalid
   input/output, activation/root-budget/attempt exhaustion. No-marker behavior keeps
   accepted 03 semantics; supported markers keep accepted 04.5 behavior. This prevents
   a reclaimed handler's read-kind default or failed activation from hiding uncertainty.
5. Cancellation/deadline retirement records all unresolved remote markers for the run
   after its terminal transition, including previously parked/failed jobs (there may
   be no live attempt). It retains the existing cancellation/deadline attempt codes,
   consumes no extra allowance, and cannot manufacture an unsent/not-applied state.
   Run deadline is the reconciliation wait deadline; existing poll_work/expire_run
   discovers parked executions even though their job is failed. Deadline fails the
   run once while preserving invocation/marker/reconciliation truth and later receipts.
   Do not create a workflow_waits row or another sweeper/lease queue.
6. Remote provider/output/storage errors continue returning failure or interruption;
   the existing supervised handler must release its exact fence, and crash recovery
   uses the existing expiring lease. Neither action service errors nor raw AppError
   text become effect-safety evidence. Integration tests exercise actual lost effects
   and retirement rather than claiming production HTTP/MCP/Rig wiring is present.

## Files, callers, and checks

Affected seams: persistence workflow/recovery.rs:21–177, pending_recovery.rs:47–126,
maintenance.rs:5–68, controls.rs:163–180 and their existing callers (completion,
budget, lease); add action_uncertainty module and action test sibling. Additive
migration after immutable 20260930160000; no application-layer adapter imports.
Before edits trace graft callers/grep for all changed symbols. No bound increase.

Meaningful acceptance:

- Actual remote write followed by lost response/output-schema rejection/provider
  classified error, exact-fence retirement with deliberately safe caller report:
  invocation needs reconciliation, run waits, one failed attempt and debit, original
  code/class preserved, no error edge/committed output, repeated competing claims
  cannot call again. Include provider timeout/local cancellation/crash classification.
- Root-budget exhaustion after remote marker on historical read handler forces park;
  pending activation/attempt/root-budget failures with unsafe marker cannot route
  around it. No-marker cases and supported replay/receipts remain green.
- Competing retirement/claims commit only one retirement/reconciliation fact; injected
  deferred commit failure rolls back run/job/attempt/audit together.
- Parked deadline appears in actual poll result, expire_run fails once, repeated poll
  excludes it, original unresolved facts persist. Cancellation while parked preserves
  uncertainty; late accepted receipt resolves state projection while audit remains and
  never advances cancelled/failed execution (full 04.8 matrix remains separate).
- Database tests reject wrong tenant/digest/marker/local effect and audit update/delete;
  insertion in live runnable run or with existing receipt fails.
- Stock 2048 KiB accepted workflow_action plus affected recovery/budget/completion/
  lease/control/maintenance/pending integration suites; formatting/whitespace, locked
  offline all-target compilation, strict Clippy, fresh migrations, SQLx prepare/check,
  graft build. No full-suite or combined-phase-04 completion claim unless run.

## Resources and context

Implementer UUID 01a0f366-72db-7680-9997-3c7188484707, verified Sol 6.1/high.
Startup 20024/258400 = 7.75% at 2026-09-30T17:39:45.759Z,
token_usage_record.usage / task_started.model_context_window, depth 0.
Own task PG/build/SQLx sequentially; retained database and immutable migrations preserved.
Graft map/ask/skeleton/callers/grep used before source exploration; per-call savings are
being totaled for final evidence. No project implementation edits before review.

## Independent expansion PASS and focused handoff — 2026-09-30

Independent report: [/private/tmp/workflow-04.6-expansion-review.md](/private/tmp/workflow-04.6-expansion-review.md).
Astra/medium reviewer /root/uncertainty_046/review_046, verified UUID
01a0f36b-09be-7260-be5b-293904d50f03, passed with no blocking findings after actual
recovery/pending/supervision/cancel/deadline seam inspection. This is expansion-only,
not code/test verification. Reviewer quiescent; no further delegation or resources.
Owner-confirmed latest depth-0 reviewer sample87767/258400=33.97% at17:47:06.921Z,
token_count.info.last_token_usage / token_count.info.model_context_window.

Next fresh implementer executes this reviewed04.6 scope only, creates its own fresh
Astra reviewer for actual-code review (do not reuse this subtree), then reports root
acceptance evidence before04.7. Reuse accepted dependency/remaining light expansion;
do not reopen history. Concrete local choices remain open inside this reviewed scope:
new additive migration timestamp/name, helper names, SQL provenance guard shape, and
test module fixture reuse. Material contract deviations require renewed expansion
review before implementation.

Recommended implementation seams: add action_uncertainty.rs with narrow helpers for
exact execution/whole run append of unresolved remote markers and the internal SQL
projection; call from recovery::retire_on after reconcile settlement, pending_recovery::
settle after parked settlement, maintenance::expire_on/controls::cancel_on after
terminal transition. Helpers use existing run-first transaction/frozen failure codes,
not another provider call or owner. Add sibling action_uncertainty_tests.rs under
accepted action test fixtures; prefer their actual-effect provider and own-database
setup. Preserve classifications and assert original code/class/debit/facts/zero repeat.
Shared predicate must precede all pending error-routing decisions; retain existing
no-marker behavior. Polling needs tests, not speculative replacement; current query
already includes waiting runs whose job is failed at run deadline.

Preimplementation evidence: /private/tmp/workflow-04.6-preimplementation-status.txt,
/private/tmp/workflow-04.6-preimplementation.diff,
/private/tmp/workflow-04.6-preimplementation-hashes.json. Baseline includes all earlier
modified/untracked work plus this brief/expansion. Root-owned records may change under
root ownership; preserve source work. No code/migration/build/test/SQLx edits or runs
occurred in this expansion subtree. Task PG remains stopped/retained as inherited;
no task-created operational resources to clean up. Next worker follows skill PG
reference and exact supplied URLs/start options. No applied migration modification.

Graft estimated savings in this expansion worker:2,863,953 tokens across map/ask/
skeleton/callers/grep outputs (includes2,698,119 orientation-map estimate). Reviewer's
separate savings are not included. Targeted spans were opened only after graph context.

## Implementation checkpoint — 2026-09-30 18:02Z

Implementer UUID01a0f370-39f3-7c80-92a5-60ddd7570778, Sol6.1/high.
Startup depth0 20098/258400=7.78%@17:50:26Z; intermediate106683/258400=41.29%
at18:02:20Z, token_usage_record.usage/task_started.model_context_window.
Implemented narrow action_uncertainty::record helper, receipt-first SQL state view,
append-only scoped reconciliation facts, calls in live/pending/deadline/cancel retirement.
All pending failures consult shared action-history predicate before error routing;
no-marker exhaustion still uses accepted handler classification. No replay grant added.
Root-owned PROGRESS/RESUME unchanged by this worker.

Both new migrations are APPLIED and IMMUTABLE:
-20260930170000_workflow_action_uncertainty.sql SHA256
`a400112030c474ae8ad0ff02fe207d745f45805ddab37be56e8b1247d5d528f3`.
-20260930171000_workflow_action_failure_identity.sql SHA256
`3a53fae6968478e761b83fd05af920fb2694b477b8b889c86e9c98496b29d283`.
The additive follow-up preserves the full existing FailureCode grammar; no applied
file was edited. PG restarted at exact inherited endpoint/options; retained data intact.

Initial offlinealltargets compilation passed18.29s:
`env SQLX_OFFLINE=true cargo check --locked --offline --all-targets`,
/private/tmp/workflow-04.6-check-initial.log. Migration logs:
/private/tmp/workflow-04.6-migrate.log and /private/tmp/workflow-04.6-migrate-r1.log.
Six focused uncertainty tests authored. Initial compile import errors fixed;
first run4passed/2failed because test expected obsolete fence to return possible-dispatch
instead of conflict, and requested exactly the budget allowance instead of exceeding it.
Fixture corrections in place; r2 active /private/tmp/workflow-04.6-uncertainty-r2.log.
Exact command uses both retained URLs,SQLX_OFFLINE=true,RUST_MIN_STACK=2097152,
`cargo test --locked --offline --lib workflow_action_uncertainty`.
No full suite or final gates claimed. Independent actual-code review and final checks remain.

## Focused implementation rotation handoff — 2026-09-30 18:08Z

Actual-code R1 report [/private/tmp/workflow-04.6-actual-review-r1.md](/private/tmp/workflow-04.6-actual-review-r1.md):
**CHANGES REQUESTED**, two P2 acceptance-test gaps; no concrete production defect found.
Fresh independent reviewer /root/uncertainty_implementation/review_046, verified UUID
01a0f37d-04ce-7270-96bd-de6b57b3fadf, gpt-6-astra/medium; startup24070/258400=9.32%
at18:04:21Z, boundary82356/258400=31.87%@18:07:11.323Z. Reviewer stopped/retired,
no further work; next implementer creates its own fresh reviewer. Root has not accepted04.6.

Correct tests together before expensive reruns:
1. SQL corrupt execution/digest/dispatch cases currently reuse an already-inserted
   reconciliation PK and merely assert any error. Use fresh parked/terminal unresolved
   invocation with no observation per corrupt insert (or rollback-only transaction and
   assert specific SQLSTATE/constraint). Prove wrong tenant/scope/digest/marker FK
   rejection independently; assert remote-only linkage without an existing receipt
   masking it. A rollback-only transaction can insert a local marker, change run to
   terminal, then attempt remote reconciliation (FK failure) before deferred local
   receipt guard runs; roll back all fixtures. Separately test existing *remote* receipt
   insertion rejection. Keep append-only/idempotent first observation coverage.
2. Real cancel/expire_run must start with unresolved remote markers and zero audit,
   so deleting either terminal record call fails a test. Include >=2 unresolved markers
   and preferably a receipted sibling, assert every unresolved linkage is recorded with
   workflow.cancelled/workflow.run_deadline, receipt excluded, original attempt
   code/class/debit preserved, no fabricated receipt or committed step/output/edge.
   Retain existing already-parked/late-receipt/deadline-poll tests. Reuse accepted action
   fixtures with changed arguments/new invocation identities under the same live fence;
   action service supports multiple intents in one logical execution. Test file currently
   ~475 lines: split SQL/terminal tests into a sibling to keep modules under500 lines.
These are accepted04.6 criteria; do not reopen or weaken scope or start04.7.

Completed verification on current code (all logs full, no skipped DB checks):
- `cargo fmt --all -- --check`: PASS, /private/tmp/workflow-04.6-fmt.log.
- `git diff --check`: PASS, /private/tmp/workflow-04.6-whitespace.log.
- Both additive migrations applied using exact inherited DATABASE_URL/TEST_DATABASE_URL,
  `cargo sqlx migrate run`: PASS, migrate.log/migrate-r1.log above.
- `env SQLX_OFFLINE=true cargo check --locked --offline --all-targets`: initial PASS,
  /private/tmp/workflow-04.6-check-initial.log (before new test modules; final check remains).
- Both exact retained URLs,SQLX_OFFLINE=true,RUST_MIN_STACK=2097152,
  `cargo test --locked --offline --lib workflow_action_uncertainty`:6passed/0failed/0ignored,
  4.60s, /private/tmp/workflow-04.6-uncertainty-r2.log.
- Same environment, `cargo test --locked --offline --lib workflow_action`:58passed/0/0,
  19.44s, /private/tmp/workflow-04.6-action.log.
- Same environment, `cargo test --locked --offline --lib adapters::persistence::workflow::tests::admission_tests`:
  264passed/0/0,103.67s, /private/tmp/workflow-04.6-affected.log. This includes affected
  recovery/budget/completion/lease/control/maintenance/pending tests, plus other admission
  tests; it is not the full library suite.
- `env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS46.26s, /private/tmp/workflow-04.6-clippy.log. This was started while the broad
  test binary was running (compilation was complete); both operations now completed.

Required outstanding gates after grouped test corrections: focused action tests and
affected stock2048KiB tests, final format/whitespace, final lockedofflinealltargets,
strictClippy, fresh task-created schema migration verification, SQLxprepare+check,
graftbuild, independent actual-code correction/overall PASS and root acceptance.
SQLx preparation was not run during frozen-code review; no cache modifications claimed.
No full-phase/full-suite pass, no settlement/productiontransport/protecteddispatch claimed.

Implementation delta files: new migrations170000/171000; new
src/adapters/persistence/workflow/action_uncertainty.rs and action_uncertainty_tests.rs;
retirement seam additions in recovery.rs,pending_recovery.rs,maintenance.rs,controls.rs;
mod.rs and action_receipt_tests.rs hooks. Exact pre-existing baseline retained above;
/private/tmp/workflow-04.6-retirement-current.diff includes prior04.5 changes, so compare
against preimplementation.diff. No stage/unstage/commit/reset/deploy, no applied migration edits.
Retained PG/data identity/options unchanged. No freshschemaDB was created in this worker.
All task tests/builds complete; PG clean stop follows this checkpoint; preserve its data.


## Grouped acceptance corrections and final gates — 2026-09-30

Correction implementer `/root/uncertainty_corrections`, verified session UUID
`01a0f384-96aa-72c3-a711-395fb987afb2`, Sol 6.1/high. Startup depth-0 sample
20164/258400 = 7.8% at 18:12:38.260Z; correction boundary 81841/258400 = 31.67%
at 18:22:11.616Z; final-gate milestone 86182/258400 = 33.35% at 18:26:08.466Z.
Sources: token_usage_record.usage / task_started.model_context_window.

Both R1 P2 gaps corrected together, with no production code or migration edits:
- Kept the five existing behavioral uncertainty tests. Replaced the masked SQL test
  with three focused tests in `action_uncertainty_acceptance_tests.rs`, nested beneath
  the original sibling. The original module is 364 lines and the new module 309 lines.
  SQL provenance negatives begin without any existing reconciliation PK and assert
  SQLSTATE 23503 for each company/run/execution/invocation/digest/marker corruption.
  Each rollback-only transaction temporarily disables only the insertion guard to
  isolate the composite FK; rollback restores it. Separate active-guard tests reject
  a live run and an actual remote receipt with 23514. An unreceipted local marker in
  a rollback-only terminal transaction fails the remote composite FK with 23503,
  before the deferred local-receipt constraint could mask it. Immutable updates and
  deletes fail with the actual append-only trigger SQLSTATE P0001; duplicate record
  insertion preserves the entire first observation.
- Real cancel and expire_run now begin with zero observations, two actual applied
  remote effects whose responses were lost, and one actual remote receipted sibling.
  Both tests assert exactly two complete marker/intent provenance links, exact terminal
  failure codes, observation time, original single attempt/debit/class/safety/retirement,
  no fabricated receipt, no completed output or successor, and unchanged state after
  stale release. The receipted sibling is excluded. The existing parked/late-receipt
  and deadline-poll regression remains. A pure synchronous assertion helper keeps
  the new async test helper below the source guide's function-size trigger.

Initial correction compile found two fixture API mistakes, corrected before testing;
`/private/tmp/workflow-04.6-corrections-tests-r1.log` records them. Focused r2 was
9 passed / 1 failed because the immutable trigger emits P0001 rather than 23514;
`/private/tmp/workflow-04.6-corrections-tests-r2.log`. Corrected fixture expectation,
then the checks below passed. No production failure was concealed or migration changed.

Final verification commands and complete logs:

| Command / environment | Result | Complete log |
| --- | --- | --- |
| Both retained URLs, SQLX_OFFLINE=true, RUST_MIN_STACK=2097152; `cargo test --locked --offline --lib workflow_action` | 62 passed, 0 failed, 0 ignored; 22.42s | `/private/tmp/workflow-04.6-corrections-actions-r3.log` |
| Same environment; `cargo test --locked --offline --lib adapters::persistence::workflow::tests::admission_tests` | 268 passed, 0 failed, 0 ignored; 100.20s, including all 10 uncertainty tests and final synchronous assertion extraction | `/private/tmp/workflow-04.6-corrections-affected-final.log` |
| `cargo fmt --all -- --check` | PASS | `/private/tmp/workflow-04.6-final-fmt.log` |
| `git diff --check` | PASS | `/private/tmp/workflow-04.6-final-whitespace.log` |
| `env SQLX_OFFLINE=true cargo check --locked --offline --all-targets` | PASS, 14.65s | `/private/tmp/workflow-04.6-final-check.log` |
| `env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings` | PASS, 20.64s | `/private/tmp/workflow-04.6-final-clippy.log` |
| `createdb -h 127.0.0.1 -p 55439 workflow_046_schema_01a0f384`, then both URLs pointing to that fresh DB; `cargo sqlx migrate run` | PASS, all migrations through 20260930171000 | `/private/tmp/workflow-04.6-fresh-migrations.log` |
| `psql postgres://mac03@127.0.0.1:55439/workflow_046_schema_01a0f384 -X -c '\d workflow_action_reconciliations' -c '\d workflow_action_effect_states' -c 'SELECT version,success FROM _sqlx_migrations ORDER BY version'` | PASS; composite FK, remote-only check, failure-code grammar, immutable/guard triggers, view, all migration success flags inspected | `/private/tmp/workflow-04.6-fresh-schema.log` |
| Both retained URLs; `cargo sqlx prepare -- --all-targets` | PASS, 11.44s; no .sqlx changes | `/private/tmp/workflow-04.6-final-sqlx-prepare.log` |
| Both retained URLs; `cargo sqlx prepare --check -- --all-targets` | PASS, 13.24s | `/private/tmp/workflow-04.6-final-sqlx-check.log` |
| `graft build` | PASS, 721 files / 14366 nodes / 21453 edges | `/private/tmp/workflow-04.6-final-graft.log` |

Both retained URLs mean `DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission`
and `TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission`, set explicitly.
The fresh DB run used `DATABASE_URL` and `TEST_DATABASE_URL` both set to
`postgres://mac03@127.0.0.1:55439/workflow_046_schema_01a0f384`.
Cargo checks/builds and SQLx operations were sequential; no resource contention queue.
Production source and both immutable migration hashes remain as recorded above.

Fresh independent Astra/medium reviewer `/root/uncertainty_corrections/review_corrections`,
verified UUID `01a0f38e-5a74-75b1-97c8-1c2d1d669b41`, startup depth-0
27269/258400 = 10.55% at 18:23:18.757Z; pre-final boundary 93767/258400 = 36.29%
at 18:26:15.376Z. Actual source/callers, original criteria and both correction gaps
independently reviewed; no further blocking findings. Final report/gate decision:
[/private/tmp/workflow-04.6-actual-review-final.md](/private/tmp/workflow-04.6-actual-review-final.md).
Code remained frozen during review. Root queue acceptance remains separate.

Only task-created `workflow_046_schema_01a0f384` was dropped with
`dropdb -h 127.0.0.1 -p 55439 workflow_046_schema_01a0f384`. The retained cluster was
stopped cleanly with `pg_ctl -D /private/tmp/workflow-admission-pg-e3aa -m fast -w stop`
(`/private/tmp/workflow-04.6-final-pg-stop.log`). Retained data was never reset;
no operations/checks remain active. Root-owned PROGRESS/RESUME were not edited.
Graft estimated savings in this correction worker: 293945 tokens; reviewer separate.
No full library/phase04 completion claim; 04.7 onward and protected/production/Rig/
bypass obligations remain as recorded above.


Final independent overall **PASS**, no unresolved findings, in the linked final
report. Both R1 P2 acceptance gaps resolved. Reviewer final depth-0 sample
97883/258400 = 37.88% at 18:28:32.838Z, runtime usage/window sources;
owner-confirmed fresh 99843/258400 = 38.64% at 18:29:06.781Z,
token_count.info.last_token_usage / token_count.info.model_context_window.
Root accepted 04.6 at 18:29Z after confirming code/criteria/integration and all gates.
Implementer final checkpoint 95062/258400 = 36.79% at 18:30:50.830Z,
token_usage_record.usage / task_started.model_context_window. Both workers remain
below their 50% thresholds and quiescent, with no active resource operations.
Next work requires a separate focused 04.7 expansion/review gate; no 04.7 code
or contract expansion was started in this assignment.
