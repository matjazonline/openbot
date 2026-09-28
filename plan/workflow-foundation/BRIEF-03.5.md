# 03.5 — atomic fenced step completion

Status: implemented; all verification gates and independent combined review PASS.
Root coverage accepted2026-09-28 14:04Z; final root acceptance pending.
Original authority:03-durable-runtime-and-persistence.md Execution and commits
item4, with applicable Failure and recovery and Acceptance criteria.03.1–03.4
are root-accepted; their current implementations and evidence remain authoritative.

## Reconciled contract

- Application owns a narrow required completion port taking the03.4 typed
  `FencedWorkflowResult` (exact company/run/execution/job/worker/generation/attempt),
  returning saved progression and a typed committed/replayed/refused outcome.
  A successful03.4 final validation is never commit authorization. Persistence
  independently verifies live ownership and active run/deadline under locks.
- Generalize03.3 `batch_commit::complete` as the single progression writer used by
  pure batches and fenced completion. Share bounded output/schema validation and
  domain route/state selection rather than duplicating policy in SQL or adapters.
  Fenced success uses the frozen step contract; handler output cannot supply an
  arbitrary target/run state. Choice-producing supported handlers must derive and
  validate declared choices from their typed output contract. Pure-only and later
  wait/control kinds remain ineligible for this I/O path.
- One bounded run→execution→job→owned-attempt transaction writes the execution
  result/route/target, run state, compact audit event, at most one successor
  execution and ID-only job, and completes the SAME claimed job and attempt.
  Terminal success records the terminal execution and validates workflow output.
  Clear job lease fields in agreement with existing queue constraints; retain
  generation/worker/attempt evidence on the original attempt, never UPSERT it.
  Preserve the single background_tasks/task_attempts/execution/audit owners.
- Use exact scope/payload/discriminator and current generation/worker/attempt
  checks from03.4. Check persistence time at the final guarded completion write,
  after staging other changes; an expiry/deadline refusal rolls the entire
  transaction back. Bound lock/statement and whole operation duration. Never
  resurrect expiry, complete replacement work or append a stale successor.
- Logical execution identity and existing unique activation/job constraints own
  deduplication. A duplicate completion after lost acknowledgement returns the
  committed output/route/successor without advancing it or rewriting any ledger.
  Replay must prove the exact original completed attempt fence and equivalent
  submitted result; wrong generation/worker/attempt/scope or conflicting output
  cannot masquerade as the accepted result. Valid committed replay remains a
  read after run completion/deadline/cancellation, with no fresh scheduling.
  No extra result ledger or independent command identity is needed.
- Preserve frozen inputs across retry, enforce existing context/output and total
  activation bounds, and never replenish attempt/root budgets. Normal success
  closes the processing attempt without incrementing consumed-failure count.
  Reclaimed stale results refuse while a replacement can commit once. Handler
  failures and invalid output cannot be mistaken for success: rollback/refusal
  leaves existing ownership recoverable through03.4; classified retries, final
  failure/error routing and explicit operator commands belong03.9.03.6 owns
  parking/resumption;03.7 parent wakeups;03.8 polling;04 effect receipts and unknown
  outcome reconciliation. This point does not invent those contracts.

## Source seams

`src/application/workflow/batch.rs:82–145` already validates pure output and
selects route/state; factor reusable completion validation there or a cohesive
new completion module. `application/workflow/lease.rs:99–102` supplies the result.
`adapters/persistence/workflow/batch_commit.rs:6–108` owns progression and successor
creation; only indexed caller is `batch.rs:107–161` (`graft grep` verified after
callers returned no edges). Generalize this seam, retaining pure behavior/tests.
`lease.rs:99–167` owns bounded run-first scope locking and exact live-window checks;
reuse/refactor narrowly to support completed replay and final guarded completion.
New completion adapter/application modules and isolated DB tests are expected;
module wiring and small batch/lease helper adjustments are in scope.

Reuse schema invariants unless missing concrete evidence requires an additive
migration newer than20260928133100. Already-applied migrations are immutable.
No legacy task owner/control path or provider implementation is replaced here.

## Acceptance checks

1. Pure→I/O→supervised handler→atomic fenced commit→pure/end integration records
   frozen input, validated result/route, run state and one audit/successor; original
   job and exact attempt close together and lease fields clear. Terminal I/O
   success works; declared choice/output/schema/byte/activation limits fail closed.
2. Two synchronized independent DB completions of the same fence produce one
   commit and an equivalent replay, one successor/job/event, one closed attempt.
   A conflicting duplicate output refuses without changing the accepted result.
   Reconnect after simulated lost commit ACK returns saved progression, including
   after successor completion or terminal run state, never reruns handler/context.
3. Wrong worker/generation/attempt/scope, mixed valid IDs, legacy/pure/unclaimed
   job, expired lease, waiting/cancelled/terminal/deadline run refuse fresh writes.
   Old generation after real expiry/backoff/reclaim cannot commit or alter the new
   attempt; replacement commits once. Contention crossing expiry/deadline rolls
   back even if initial validation passed.
4. Real completion/completion, completion/run-cancellation and completion/reclaim
   competitors synchronize on independent DB connections. Either completion wins
   first with one valid successor, or cancellation/reclaim prevents old completion;
   no post-cancellation scheduling by a losing commit and no replacement closure.
   Run cancellation fixture uses run-first SQL transition, not a fabricated public
   command. Parent-tree concurrency remains03.7 scope.
5. Inject failures at result, successor execution/job, run, audit, job and attempt
   write boundaries; snapshots prove all-or-nothing rollback and clean retry.
   Commit then simulate process/response loss; recovered saved result is identical
   with no duplicate successor. Retain03.2/03.3 activation/crash and03.4 claim tests;
   database rejects duplicate activation/job and cross-scope successor relationships.
6. Focused completion plus affected batch/lease/activation tests and full DB library
   suite at stock2MiB; isolated fresh migrations, SQLx prepare sequential with builds,
   locked offline all-target check/Clippy, fmt/diff checks, graft refresh. Independent
   nested reviewer inspects actual diff/callers and combined integration with source
   edits paused. No claim of03.5 completion until all required gates pass.

No unresolved user decision. Root coverage acceptance precedes source edits;
this is an internal plan gate, not a user permission request.

## Ownership and startup

Root owns PROGRESS/RESUME and acceptance. Implementer `/root/fenced_commit`,
Astra/medium UUID01a0e852-8eba-73a0-8c76-f823842ceaa1 owns this brief and taskPG
RUNNING at `/private/tmp/workflow-admission-pg-e3aa`,port55439,DBworkflow_admission.
DATABASE_URL and TEST_DATABASE_URL both
`postgres://mac03@127.0.0.1:55439/workflow_admission`.
Nested independent Astra/medium reviewer `/root/fenced_commit/reviewer`, UUID
01a0e852-d6c6-7571-be85-546b6cb218a6 was spawned before edits and is quiescent/ready.
Startup samples: implementer21375/2584008.27%14:02:10.686Z; reviewer21111/258400
8.17%14:02:27.207Z (2026-09-28; token_usage_record.usage /
task_started.model_context_window). Workers rotate at50%; next sample after expansion.
Preserve all actual staged/unstaged/untracked work including website assets and
historical external deletions; no stage/commit/reset/deploy. No03.6 advance.

## Implementation and independent review — 2026-09-28

Implemented single shared progression writer: application `completion.rs` owns the
required WorkflowCompletion port and success/choice preparation; pure validation
is shared through `batch::validate_completion`. Persistence completion locks
run→execution→job→attempt, reuses03.4 live-window validation, loads frozen activation,
and calls existing batch_commit::complete. CompletionOwner selects exact job/attempt
closure and the audit kind; no second result/successor writer or ledger was added.
The final job write checks generation/worker/attempt/expiry/deadline and clears
lease fields; the same attempt retains its generation and completed stop reason.
The retry counter remains consumed failures, unchanged by success.

Replay proves the original completed attempt fence and equal output, then returns
saved route/target/successor. Wrong and conflicting replays refuse without writes.
Replay still works after successor completion, cancellation or elapsed run deadline.
External results cannot choose arbitrary targets; decision.agent derives its choice
from the declared output contract. Failure classification and parking retain later
owners as specified above.

A concrete missing invariant required additive migration20260928141500:
`workflow_fenced_completion_guard` is deferred to COMMIT and validates the prior
job lease expiry and exact completed attempt/execution. This closes delayed-commit
expiry beyond the final application check, matching03.3's existing deadline guard.
Applied successfully59ms; now IMMUTABLE. Old migrations remain unchanged. Isolated
focused fixtures migrate fresh databases and exercise delayed commit rollback for
both lease and run deadline.

Independent Astra/medium reviewer found no production correctness defect. Two
acceptance gaps were corrected: exact completed-fence negative/replay-after-terminal
coverage, and completion-specific mixed-ID/pure/unclaimed/legacy refusal coverage.
Long test cases were split at cancellation/reclaim and fresh/reclaimed seams.
Correction code review PASS; a later legacy fixture needed a real same-company
channel to satisfy the existing legacy constraint, and that narrow rereview PASSed.
No source edits occurred during active review. Initial compile-only test import
and local signature-refactor mistakes were fixed; no production behavior was
changed to clear fixture failures.

Current focused gate PASS: explicit DATABASE_URL/TEST_DATABASE_URL as above,
RUST_MIN_STACK=2097152 SQLX_OFFLINE=true `cargo test --locked --offline --lib
workflow_completion -- --nocapture`:14passed,0failed,6.62s;
`/private/tmp/workflow-03.5-focused.log`. Full/prepare/static gates still pending at
this checkpoint; final evidence below will supersede pending status.

TaskPG was unexpectedly stopped by an external actor during work. Root confirmed
no intentional lifecycle change; implementer restarted the retained task-only
cluster and verified `workflow_admission|55439`. No reset/data removal occurred.
External staging also occurred; baseline commit remaineda3995f6 (`workflow3.3`).
All staged and unstaged edits were preserved; reviewer inspected actual scoped
files rather than treating an unstaged diff as complete. Implementer did not stage.

Implementer last sample114182/25840044.19%14:21:59.597Z; reviewer final narrow sample
88222/25840034.14%14:21:42Z (latest parent row8860634.29%14:21:45.235Z). UUIDs and
measurement/capacity sources remain as recorded above. Root owns acceptance and
PROGRESS/RESUME; no03.6 work started. TaskPG RUNNING unchanged.

### Final verification gates

Current-tree evidence, all2026-09-28, commands in repository root. Database commands
used explicit DATABASE_URL and TEST_DATABASE_URL both
`postgres://mac03@127.0.0.1:55439/workflow_admission`; tests used
RUST_MIN_STACK=2097152 and SQLX_OFFLINE=true. No DB-skip flag.

- `cargo test --locked --offline --lib workflow_completion -- --nocapture`:
  14PASS/0fail,6.62s, `/private/tmp/workflow-03.5-focused.log`.
- `cargo test --locked --offline --lib`:2034PASS/0fail/22existing ignored,84.18s,
  `/private/tmp/workflow-03.5-full.log`. Includes affected activation/pure batch/
  lease tests and freshly migrated isolated DB fixtures.
- `cargo sqlx migrate run`:PASS, new migration20260928141500 applied59ms;
  `/private/tmp/workflow-03.5-migrate.log`.
- `cargo sqlx prepare -- --all-targets`:PASS25.25s,
  `/private/tmp/workflow-03.5-sqlx.log`; no staged or unstaged `.sqlx` diff.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS0.91s,
  `/private/tmp/workflow-03.5-check.log`.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS52.58s, `/private/tmp/workflow-03.5-clippy.log`.
- `cargo fmt --all -- --check`, both `git diff --check` and `git diff --cached
  --check`, `graft build`:PASS, `/private/tmp/workflow-03.5-{fmt,diff,graft}.log`.

Build/test/prepare ran sequentially. No live command remains. Independent combined
final evidence review requested next; root acceptance remains pending. No03.6 work.

Final independent combined review PASS2026-09-28 14:26Z: no unresolved findings;
reviewer inspected all final logs and reused actual-code/correction review above.
Reviewer now completed/quiescent at95289/25840036.88%14:26:35.338Z; implementer
boundary120159/25840046.50%14:26:26.003Z. Same verified UUIDs/sources above.
All required03.5 gates complete; root acceptance is next. TaskPG remains RUNNING,
no build/test/prepare processes remain, no03.6 work, and all external work preserved.
No staging, commit, reset or deployment by this subtree.
