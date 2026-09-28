# 03.7 — run-first transitions and durable parent wakeups

Status: root coverage accepted2026-09-28; implementation started. Authority: phase03 Execution
item6 and concurrency/cancellation/restart acceptance.03.1–03.6 are accepted.

## Reconciled contract and boundaries

- Runtime mutation transactions lock one existing run first, followed by its
  executions, jobs/attempts, waits/events and later actions. Discovery reads may
  precede the lock but cannot authorize mutation. Reentrant acquisition of the
  same already-owned run is safe. Never hold a child run and acquire its parent.
  New child admission may lock the parent before inserting its previously
  nonexistent child; it must never acquire an already-existing child tree.
- Audit all current activation, pure progression, fenced claim/renew/release/
  completion and wait park/signal/resume/expiry paths, including SQL triggers.
  Preserve existing bounds and live lease/deadline guards. Document the common
  order at the persistence module boundary; repair any actual inversion found.
- Normalize the existing admitted execution-child cause into immutable scoped
  parent run/execution columns on workflow_runs. Validate/backfill existing
  versioned admission source keys in an additive migration; reject mismatched
  scope rather than fabricating associations. Parent association is established
  while admitting the child, never during its terminal transition. Action-child
  admission remains unsupported pending its owning action/child contracts.
- A child terminal transition atomically appends exactly one durable parent-wakeup
  fact per actual terminal transition in the existing workflow_run_events ledger. It references child-owned rows
  and resolves its destination through the immutable admitted association. Do not
  insert a parent foreign key during child completion: even implicit KEY SHARE
  could block against parent cancellation. Wakeups contain identity and typed
  terminal state; saved child execution owns output, no duplicated result ledger.
  Success, failure and cancellation use the same terminal-event mechanism; no
  wakeup for top-level/nonterminal updates, duplicate completion or no-op state.
- Add a narrow application-owned read port for bounded exact-parent wakeup rereads,
  exposing child/run/event/terminal execution identities and typed outcome. No
  notification delivery is needed for correctness of these durable facts. Parent
  cancellation never erases a child result or causes child completion to advance
  the parent. The child terminal transaction must commit while parent run and
  execution locks are held elsewhere.
- Phase07 remains the sole owner of workflow.call creation together with its
  parent wait, child schema/output mapping, inherited budgets/authorization,
  cancellation propagation and exact agent checkpoint resume. It will consume
  these existing wakeup facts transactionally under the parent lock and its wait
  identity;03.7 does not invent a competing call ledger, result owner, queue or
  generic event-to-human/child resume bypass.03.8 owns global polling integration;
  03.9 owns public cancellation/retry commands and classified recovery. Tests use
  already-supported child admission and run-first cancellation fixtures.

## Source seams and delivery

- admission_source.rs:87–121 already validates execution-child scope under parent
  run→execution SHARE locks; admission_write.rs:29–56 inserts run snapshots.
- batch_commit.rs:5–67 is the one result/progression writer, invoked by pure,
  fenced and parked completion. wait_commit.rs:113–126/161–184 owns wait failure.
  A terminal-state SQL event hook can cover these and future cancellation without
  acquiring parent locks or introducing a second progression writer.
- activation.rs:39–100, batch.rs:49–98/171–198, lease.rs:99–147,
  completion.rs:39–122 and waits.rs:90–130 currently use run-first ordering.
  Trace callers plus all SQL FOR UPDATE/SHARE paths before edits.
- Expected files: additive migration newer than20260928144500; parent cause
  insertion; narrow application/persistence wakeup modules, module wiring and
  isolated DB concurrency/guard tests. Applied migrations remain immutable.

## Acceptance gates

1. Real admitted child completes through pure, fenced or resumed wait progression;
   terminal state/result and one parent-wakeup event commit atomically. Top-level
   and nonterminal transitions create no wakeup. Replays/reconnect return the same
   identities and saved result; parent is unchanged by child completion.
2. Independent competing child completions and parent cancellation, plus parent
   run/execution held while a child completes, finish under bounded timeout with
   no deadlocks. Parent cancel may inspect/cancel a child only in another
   transaction. Verify sibling children and unrelated runs remain independent.
3. Inject wakeup write/commit failure: result, run, jobs and audit all roll back;
   clean retry emits one event. Success/failure/cancel and repeated terminal writes
   obey the typed transition event contract and cannot overwrite terminal history.
4. Database rejects cross-company/wrong-parent-execution and mutation of the
   admitted cause/wakeup fact. Existing admissions backfill correctly. Bounded
   reads refuse invalid limits and wrong scope; no human/event resume bypass.
5. Audit actual current callers/SQL lock order, including foreign-key triggers;
   independent reviewer verifies final implementation and phase07 boundary.
6. Focused real DB tests and full library DB suite at stock2MiB, fresh migrations,
   schema inspection, sequential SQLx prepare, offline locked all-target check/
   Clippy, fmt/staged+unstaged diff checks and graft build. Logs under
   /private/tmp/workflow-03.7-*.log. No03.8 before root acceptance.

Implementer /root/run_lock_order Astra/medium, verified UUID
01a0e87e-5f3c-7e02-a274-1a35263c8699, startup21321/2584008.25%
2026-09-28T14:50:00.220Z. Nested independent reviewer
/root/run_lock_order/review Astra/medium, UUID01a0e87e-9cdb-7c10-92ff-226002b2bf81,
startup21095/2584008.16%14:50:17.584Z; capacity established before source edits.
Sources token_usage_record.usage/task_started.model_context_window; rotate50%.
TaskPG RUNNING /private/tmp/workflow-admission-pg-e3aa port55439 DBworkflow_admission;
DATABASE_URL and TEST_DATABASE_URL explicitly
postgres://mac03@127.0.0.1:55439/workflow_admission. Root owns PROGRESS/RESUME.
Preserve all external staged/unstaged/untracked work. No stage/commit/reset/deploy.

Root accepted refinement: no-op/duplicate completions emit no wakeup, but a future
03.9 authorized retry may produce a new terminal transition and distinct event
sequence. Never suppress new facts using permanent per-child uniqueness.

## Implementation and correction checkpoint — 2026-09-28 15:08Z

Actual-code independent review PASS; all reported findings resolved. Admission
normalizes its existing execution-child cause into immutable scoped run columns.
Terminal child transitions append one typed immutable event in the existing
workflow_run_events owner. A generated child transition sequence plus INSERT
validation/PK rejects forged, duplicate and overridden event identities. Future
safe retry may produce another transition; history stays immutable. No parent
locks or parent writes occur during child completion, including implicit FKs.

The application read port takes an exact parent ExecutionRef AND child RunId,
with a child-scoped after_sequence cursor and limit1..128. This is sufficient for
phase07's known logical-call consumer and avoids skipping late sibling events.
No global polling/parent settlement/cancellation propagation was implemented;
those retain03.8/07 ownership. Admission replay may take parent then existing
child SHARE locks; runtime child transitions never reverse that order.

Migration20260928145500 applied20.49ms, correction20260928150500 applied4.36ms;
both IMMUTABLE. The correction reconciles current terminal children admitted
before03.7 and guards generated event identity. Only existing supported canonical
v1 admission causes are reconciled; obsolete ad-hoc test strings were corrected,
not made into production compatibility formats. Existing event facts survive.

Reviewer findings corrected: missing historical terminal wakeups; forged INSERT
facts; noncanonical older schema-test source literals; missing sibling, real wait
expiry, migration and scoped-FK coverage; unused duplicate parent extraction.
The first6-test build also exposed a test helper shadowing error, fixed locally.
Neither fix changed production behavior to clear a fixture failure. Review was
paused during corrections and edits paused during every active review.

Final focused gate PASS:12tests/0fail,3.14s, explicit task DB URLs as above,
RUST_MIN_STACK=2097152 SQLX_OFFLINE=true `cargo test --locked --offline --lib
workflow_wakeup_ -- --nocapture`; /private/tmp/workflow-03.7-focused.log. Covers
held parent run+execution locks, real competing completion/cancel, terminal pure/
fenced/timer paths, actual wait expiry, sibling independent progress/cursors,
write+deferred-commit rollback, replay/restart, no-op/terminal history, forged
facts/bounds, wrong-run and valid foreign-company FK refusal, and actual migration
replay over existing terminal+queued admitted children in an isolated database.

Full DB library suite, SQLx prepare, offline check/Clippy and final fmt/graft are
running sequentially under one orchestration process; all remain PENDING until
final evidence below. Initial fmt and staged+unstaged diff checks PASS; schema
inspected in /private/tmp/workflow-03.7-schema.log. No SQLx cache changes expected
for runtime SQL, but prepare and DB tests remain required.

Final actual-code reviewer UUID01a0e87e-9cdb-7c10-92ff-226002b2bf81 is idle at
87300/25840033.78%15:07:15.399Z; implementer119789/25840046.36%15:07:13.869Z,
sources token_usage_record.usage/task_started.model_context_window. Root owns
acceptance and PROGRESS/RESUME. TaskPG RUNNING unchanged; no03.8, no stage/commit/
reset/deploy, all external work preserved.

## Final verification gates — 2026-09-28 15:11Z

All commands in repository root; DB commands used DATABASE_URL and
TEST_DATABASE_URL explicitly postgres://mac03@127.0.0.1:55439/workflow_admission.
Tests used RUST_MIN_STACK=2097152 SQLX_OFFLINE=true; no DB-skip option.

- `cargo test --locked --offline --lib workflow_wakeup_ -- --nocapture`:
  12PASS/0fail3.14s; /private/tmp/workflow-03.7-focused.log.
- `cargo test --locked --offline --lib`:2062PASS/0fail/22existing ignored93.34s;
  /private/tmp/workflow-03.7-full.log. Includes schema source fixtures and all
  previous03 regressions, with fresh isolated DB migrations.
- `cargo sqlx migrate run`:145500PASS20.49ms and150500PASS4.36ms;
  /private/tmp/workflow-03.7-migrate.log and workflow-03.7-migrate-guard.log.
- `cargo sqlx prepare -- --all-targets`:PASS42.52s;
  /private/tmp/workflow-03.7-sqlx.log. No staged or unstaged.sqlx diff.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS1.09s;
  /private/tmp/workflow-03.7-check.log.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS47.51s; /private/tmp/workflow-03.7-clippy.log.
- `cargo fmt --all -- --check`, both staged/unstaged diff checks, `graft build`:
  PASS; /private/tmp/workflow-03.7-{fmt,diff,graft}.log.

All builds/tests/prepare ran sequentially; no live process remains. Final combined
review requested next; root acceptance remains pending. Latest implementer
123742/25840047.89%15:10:50.775Z; reviewer idle88075/25840034.08%15:07:23.329Z,
same UUIDs and measurement sources above. TaskPG remains RUNNING, no03.8 and no
stage/commit/reset/deploy. This supersedes prior pending check statements.

Final independent combined acceptance review PASS2026-09-28 15:12Z; no remaining
findings or verification gaps. Reviewer reused actual-code/correction reviews,
inspected all final logs, and is quiescent at88458/25840034.23%15:12:13.965Z,
UUID01a0e87e-9cdb-7c10-92ff-226002b2bf81. Root acceptance is next. No work remains
active in this subtree; taskPG ownership transfers unchanged to root. No03.8.
