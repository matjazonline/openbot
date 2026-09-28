# 03.2 — durable activation inputs

Status: implemented and all verification gates plus combined independent review
PASS; awaiting root acceptance. Root accepted expansion coverage
2026-09-28 before implementation. Original criterion is
03-durable-runtime-and-persistence.md, Execution and commits item 1. Whole03.1
is verified; RESUME.md and ADMISSION-03.1.md supersede stale pending-fragment
paragraphs in EXPANSION.md. No accepted discovery or gates need repeating.

## Reconciled contract

- Reuse `workflow_executions` for logical identity and frozen activation inputs;
  `background_tasks` / `task_attempts` remain the only scheduling/attempt owners.
  Existing identity is `(company, run, step, activation)` plus immutable execution
  UUID. Retry/reclaim changes attempts/fences, never execution identity or inputs.
- Existing admission creates activation 1 and a version-1 execution-ID-only job
  (`admission_write.rs:59–73`). The SQL shape constraint already prohibits payload
  contexts. Reuse it, adding a strict typed decoder and scoped job-to-execution
  validation at the activation boundary; unknown versions/extra keys/mismatches
  fail without writes. No alternative queue or generic context payload.
- Introduce a cohesive application activation port and typed result under
  `src/application/workflow/activation`; adapter under persistence/workflow.
  Request contains scoped durable identifiers, never caller-provided contexts.
  It is a trusted internal storage operation, not a user authorization endpoint,
  claim, permission grant, or authorization to perform an effect.
- First activation resolves through the existing frozen bundle
  `prepare_step_inputs` (publication/mod.rs:59–94), from saved run input/params,
  run/parent metadata, and committed outputs only. Reuse bounded compiler/domain
  validation. No current binding/config/history reread or provider I/O.
- One transaction locks the run first, then execution, validates the job linkage,
  reads dependencies, and writes the frozen inputs and activation marker together.
  A replay returns stored inputs without resolving again. Failures before commit
  leave no activation; lost acknowledgement after commit returns the saved value.
  Database guards make the frozen tuple immutable and reject malformed/unbounded
  records; JSON null is a value, SQL NULL means not activated.
- Define activation as a positive run-wide sequence and add scoped uniqueness
  `(company, run, activation)` so sequential/repeated steps have deterministic
  predecessor order.03.3/03.5 allocate successors under the run lock. For each
  referenced step use its latest committed output with lower activation sequence;
  never choose by wall-clock timestamps, UUIDs, or worker attempts. Persist only
  execution-owned output facts needed by this reader, with an explicit completion
  marker and immutability/bounds; production result/route commits belong to03.3/03.5.
  Tests may seed those facts to verify non-entry/repeated-step resolution. No
  public partial-result writer is introduced at03.2.
- Read and resolve bounded data: admitted max-context bytes, compiler structure
  limits, bounded dependency count and output sizes. Reject absent/incomplete/
  foreign dependencies and invalid frozen bundles instead of substituting null
  or uncommitted results. Preserve existing run deadline semantics; expired runs
  cannot start a new activation, while retrieval of an already-frozen snapshot
  alone is not a claim or execution grant.

## Cross-point boundaries

03.3 owns pure mapping/routing execution and bounded batches; it reuses activation
inside the same transaction and writes committed output/route facts.03.4 adds real
worker claims, attempt fences/heartbeat and cancellation of actual I/O; activation
does not claim ownership, and concurrent callers may both read the same committed
snapshot.03.5 owns result/run/audit/successor atomic progression.03.6–03.9 own waits,
parent wakeups, polling and recovery/budgets. Activation introduces no live worker
loop or scheduling path and makes no lease-exclusivity/completion claim. Future
effects still recheck current authorization via phase04. Immutable run causality
supplies parent metadata; it is never copied from a task payload.

## Deliverables and checks

1. Application types/port/pure preparation plus strict identifier payload codec;
   reuse domain newtypes and compiler validation, no adapter imports.
2. Additive migration newer than20260928105000 for activation persistence,
   ordered identity, bounded completion-read seam and immutable facts; preserve
   all existing migration files and accepted admission/association constraints.
3. PostgreSQL adapter with run-first transaction owner and composable internal
   connection helper for03.3. Module wiring and concise contract documentation.
4. Real DB checks: two synchronized connections race first activation and observe
   one frozen identity/value; aborted transaction before activation/commit permits
   clean retry; committed-but-unacknowledged activation survives pool reconnect;
   distinct attempt rows reuse one execution/input snapshot; invalid/missing/
   foreign job or dependency, malformed payload, invalid inputs and size overflow
   leave no writes; committed predecessor output resolves correctly, later/repeated
   activation cannot alter an already-frozen value; direct SQL mutation is rejected.
   Existing ID-only DB constraint and immutable identity evidence is reused with
   focused regressions for the new reader/decoder.
5. Formatting/diff, locked offline all-target check and Clippy, task-only isolated
   migrations and SQLx prepare (sequential with builds), focused DB tests at stock
   2MiB and model-change full library suite. Independent actual-code review after
   checks; refresh graft after substantial edits. No tests skipped silently.

## Resources and evidence

Preserve startup index/tree exactly: `MM index.html`, added marketing/images/
BB-billing.png and features.png. No stage/commit/reset/deploy. Task PG retained
stopped at `/private/tmp/workflow-admission-pg-e3aa`, port55439; explicit
DATABASE_URL and TEST_DATABASE_URL both
`postgres://mac03@127.0.0.1:55439/workflow_admission`. No other DB touched.

Implementer session01a0e800-552f-7193-9219-ec7210fdded0 startup21713/258400=8.4%
2026-09-28T12:32:20.904Z (token_usage_record.usage / task_started.model_context_window).
Nested review handle `/root/execution_activation/activation_review`, session
01a0e800-bac8-7802-be31-04c81f1f6214 startup21253/258400=8.22%12:32:44.333Z,
same sources; ready and idle, no discovery or edits. Both Astra/medium.

Graph discovery used admission writer/ports/contracts/context skeletons and
`prepare_step_inputs` / `insert_execution_job` callers. Reported savings are
tool estimates versus whole-file reads, not measured token spending. Exact
implementation/check/review evidence will be appended here; root owns queue
acceptance in PROGRESS/RESUME.

## Implementation and correction checkpoint — 2026-09-28 12:48Z

Implemented application `activation.rs` contract/strict ID codec/input-dependency
collector, PostgreSQL `activation.rs` freeze-once transaction and internal helper,
`activation_tests.rs`, and additive migration20260928130000. Admission writer now
uses the same ID-only encoder; test fixture supports custom source; module wiring
and domain execution documentation clarify run-wide ordinal. Existing per-step
uniqueness is retained. No completion writer, queue claimant or worker loop added.
Optional/defaulted missing references retain compiler semantics, rather than
blanket rejection. Only dependencies referenced by current inputs are loaded.

Migration130000 was applied to retained task PG and is now IMMUTABLE. No earlier
migration was modified. PG remains RUNNING for successor worker at the same path,
port and explicit URLs above; all activation test databases were scoped fixtures.
No ongoing build/test/prepare command remains at this checkpoint.

Independent reviewer `/root/execution_activation/activation_review` inspected
actual changed/new files and returned two P2 findings: unrelated historical
outputs were charged to input context, and deadline observation could grow stale
while locks/preparation waited. Corrected together: traverse current input
bindings only; final UPDATE additionally requires live database deadline and
checks affected row count. Two real DB regressions cover these failures. Reviewer
reinspection PASS, no remaining concrete findings, with verification gates below
explicitly pending. Reviewer session01a0e800-bac8-7802-be31-04c81f1f6214 last sample
74771/258400=28.94%2026-09-28T12:46:57.618Z usage/task_started sources. Reviewer
was told to quiesce; no edits or further delegates. Replacement implementer must
create its own reviewer, not reuse this retired subtree.

Evidence:

- `DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission cargo sqlx migrate run`
  PASS, `/private/tmp/workflow-activation-migrate.log` (130000 applied22ms).
- `SQLX_OFFLINE=true DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission RUST_MIN_STACK=2097152 cargo test --locked --lib workflow_activation`
  PASS11/0failed/0ignored,2.15s after41.57s build,
  `/private/tmp/workflow-activation-tests-corrected.log`. Covers competing DB
  connections, rollback/lost-ack/reconnect/distinct attempts, prior committed
  outputs/repeats/JSON null, scoped rejection, invalid/oversized inputs, direct
  SQL immutability/bounds/ordinal uniqueness, final-statement rollback, execution
  deadline/max-steps/output bounds, parent causality, unrelated output accumulation,
  and lock-wait through expiry. Strict codec unit test is included.
- `cargo fmt --all -- --check` PASS, `/private/tmp/workflow-activation-fmt.log` empty.
  `git diff --check` PASS, `/private/tmp/workflow-activation-diff.log` empty.
- Earlier `/private/tmp/workflow-activation-check.log` failed Box<str> to String
  conversion (corrected). `/private/tmp/workflow-activation-tests.log` built but
  DB connections denied by sandbox; rerun escalated. First approved run
  `/private/tmp/workflow-activation-tests-approved.log` reached DB but8 fixtures
  failed compiler validation; required input schema and parent_id pointer fixed.
  These failures are superseded by the corrected11-test PASS, not waived.

### Next worker: required gates, then final acceptance

Reuse accepted discovery,11-test evidence and unchanged-code review above. Remaining
commands (SQLx prepare sequential with builds; task-only DB connection approval may
be required):

```sh
DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission cargo sqlx prepare -- --all-targets
SQLX_OFFLINE=true cargo check --locked --all-targets
SQLX_OFFLINE=true cargo clippy --locked --all-targets -- -D warnings
SQLX_OFFLINE=true DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission RUST_MIN_STACK=2097152 cargo test --locked --lib
graft build
```

Use `/private/tmp/workflow-activation-prepare.log`, `-check-final.log`, `-clippy.log`,
`-full.log`, `-graft.log`. The full library gate is mandatory for persisted model
changes; no prior full-suite result verifies130000. Apply migrations in isolated
test fixture databases as usual; never amend130000 if a schema correction is
needed. Offline metadata changes may be empty for runtime SQL; still run prepare.
Independent successor review checks any corrections and remaining combined effects
without repeating unchanged review. Repeat affected checks only when changed.
Verify index preservation and add final evidence here; root owns acceptance and
PROGRESS/RESUME. Do not start03.3 without root assignment.

Final implementer context117368/258400=45.42%12:48:22.192Z,
token_usage_record.usage/task_started.model_context_window. Stopping early for
rotation rather than starting remaining long gates near50%.

External index observation at checkpoint: startup had `MM index.html` and staged
marketing images; latest status shows only unstaged `M index.html`, images absent
from status. Implementer issued NO stage/commit/reset commands. Root notified to
reconcile the external change; do not restore the old index or undo external work.

## Final verification — 2026-09-28 12:55Z

Successor implementer `/root/activation_gates`, verified session
01a0e810-4390-7870-8bae-ce6d69530aac, Astra/medium, completed the exact remaining
commands above without source corrections. Prior actual-code review, focused-test,
migration and formatting evidence remains applicable. All commands exited 0:

- SQLx prepare PASS43.25s, `/private/tmp/workflow-activation-prepare.log`.
  Initial sandbox attempt was denied database access; approved rerun passed.
  `.sqlx` diff remains empty, expected for the runtime SQL added here.
- Locked offline all-target check PASS1.03s,
  `/private/tmp/workflow-activation-check-final.log`.
- Locked offline all-target Clippy with `-D warnings` PASS51.01s,
  `/private/tmp/workflow-activation-clippy.log`.
- Full locked library suite against the explicit task-only DATABASE_URL and
  TEST_DATABASE_URL at stock `RUST_MIN_STACK=2097152`: **1992 passed, 0 failed,
  22 existing ignored, 0 filtered**,60.57s after21.76s build,
  `/private/tmp/workflow-activation-full.log`. Includes all11 activation tests.
- `graft build` PASS, `/private/tmp/workflow-activation-graft.log`; local ignored
  graph refreshed. Current `git diff --check` PASS; unchanged-code fmt PASS reused.

Fresh nested reviewer `/root/activation_gates/combined_review`, Astra/medium,
session01a0e810-a6dd-7eb0-b913-6db615622a85, independently inspected final logs and
preservation evidence, reusing the unchanged actual-code review. Combined
integration/evidence **PASS**, no concrete findings. No reviewer edits/delegates.
Reviewer final49385/258400=19.11%2026-09-28T12:54:38.862Z, sources
token_usage_record.usage/task_started.model_context_window; completed/quiescent.

Preservation verified across this successor turn: no staged diff; index.html SHA1
e3d81f91360908bb872723233193c6ab98f8adf4 and immutable migration130000 SHA1
9944a5702ce7db6c240cbe2a271871426a6b8714 unchanged. No stage/commit/reset/deploy.
Task PostgreSQL remains RUNNING at the same task-only directory/port/database
for the next point. No build/test/prepare commands remain running. Root owns
acceptance and next-point authorization;03.3 was not started.

Implementer startup21703/258400=8.40%12:49:44.162Z; intermediate48767=18.87%
12:51:38.211Z; post-gates55114=21.33%12:54:26.670Z, all2026-09-28 with
token_usage_record.usage/task_started.model_context_window sources.
