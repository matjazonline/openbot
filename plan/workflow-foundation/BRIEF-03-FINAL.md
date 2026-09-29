# Phase03 combined acceptance

Status: phase03 independently verified and root ACCEPTED2026-09-29 08:55Z. No phase04 work.
Original authority:03-durable-runtime-and-persistence.md, EXPANSION.md phase03,
BRIEF-03.9.md contracts1–5 and matrix1–7. Prior accepted fragments unchanged.

## Expansion and coverage

The existing complete-chain polling test lacks classified retry in the same run.
Add one isolated DB test joining admission→pure→scripted I/O→event wait→classified
retry→completion, fresh persistence/worker handles, actual competing wait/retry
workers, duplicate signal, positive retry delay, frozen input reuse and exact
execution/attempt counts. Shared existing fixture migrates an own_database.
No production handler, migration, limit, queue owner or successor writer changes.

| Criterion | Current actual test coverage (rerun in full suite) |
| --- | --- |
| Admission atomic dedup, independent message/history snapshots | admission_tests/admission_history_tests; wait_deadline_tests same-thread independent runs |
| ID-only activation, freeze, pure bounded batch, crash boundaries | activation_tests; batch_tests/batch_recovery_tests; lease_tests write rollback |
| Competing ownership, stale fences, actual cancellation | lease_tests/adversarial_tests; application supervise tests; control race tests |
| Atomic result/successor and parking/events | completion_tests/completion_refusal_tests; wait_tests/wait_guard_tests |
| Run-first parent wakeups, no ancestor locks | wakeup tests and unchanged two-second parent-link regression |
| Poll/restart and combined seams | polling_tests; new phase03_tests combined gate |
| Classified recovery, poison, deadlines and controls | recovery/pending/maintenance/control tests, BRIEF-03.9 accepted evidence |
| Frozen root budgets, reservation/activation no-refill | budget schema/runtime/race/activation tests, BRIEF-03.9-BUDGETS |
| Global/company capacity and sustained mixed demand | capacity/renew/fairness worker/mixed/acceptance tests, BRIEF-03.9-FAIRNESS |

Full DB library via scripts/stack-budget.sh --offline supplies stock2MiB gate;
CI already invokes this script in addition to migration/offline/database checks.
New test SQL requires sequential SQLxprepare/check despite runtime SQL yielding no
metadata. Exact remaining verification commands: /private/tmp/workflow-phase03-gates.sh.
No environment DB skip and both URLs explicitly retained taskPG. Fresh isolated
migration/schema tests run inside full library; prior applied migrations immutable.

## Ownership and limitations

Implementer /root/phase03_gate, Astra/medium, verified session
01a0ec52-90c8-7632-9408-e4d08d8ae8ec; sole independent reviewer
/root/phase03_gate/combined_reviewer, Astra/medium, session
01a0ec52-d514-73c3-96d4-124faffe4af2. Startup9.70%/9.39%, capacity258400,
token_usage_record.usage/task_started.model_context_window. Root owns PROGRESS/RESUME.
Preserve all staged/unstaged/untracked work, including external website assets.
No staging/commit/reset/deploy. User requests handoff and stop after03.

Phase04 effect receipts/reconciliation,06 real model/context handlers,07 child
execution and08 startup replacement remain later scope. Current runtime uses
scripted handlers; unknown effect outcomes stay suspended. Fairness assumes a
responsive supported worker/DB within retry bound; EXPLAIN is representative,
not proof of history-independent database work. No bounds raised.

## Verification checkpoint — 2026-09-29 08:51Z

Exact sequential commands in /private/tmp/workflow-phase03-gates.sh EXIT0.
Both URLs explicitly postgres://mac03@127.0.0.1:55439/workflow_admission;
SQLX_OFFLINE=true for tests/check/Clippy; RUST_MIN_STACK=2097152, script default
STACK_BUDGET_KIB2048 unchanged. No ALLOW_MISSING_DATABASE_URL or test skip added.
Logs prefix /private/tmp/workflow-phase03-:
- library.log: scripts/stack-budget.sh --offline PASS2191/0failed/22existingignored,
  180.95s. Includes all461workflow tests, isolated fresh migration/schema tests,
  immutable lineage/budget replay, competition and same-run combined test.
- Existing22ignored are19optional HydraDB tests (including1credentialed live smoke),
  2optional liveDNS smokes,1developer JavaScript dump. No phase03 or DB gate ignored.
- focused-verified.log: combined test1PASS0ignored1.11s. focused.log sandbox EPERM
  failure is not correctness evidence; initial helper visibility compile failure
  corrected before this successful run.
- migrations.log PASS through20260929100000; SQL catalog confirms all success.
- prepare.log PASS17.14s; prepare-check.log PASS14.20s. No .sqlx staged/unstaged diff.
- check.log PASS0.84s; clippy.log strict-Dwarnings PASS28.46s; fmt.log,
  diff.log,diff-staged.log,graft.log PASS.

Independent combined behavioral/architecture/coverage review PASS08:46Z. Sole
finding: new combined test exceeded ~80line rule. Extracted synchronous final
snapshot assertions and shared empty-poll assertion, main74lines. Larger128
empty-poll page strengthens same assertion. Correction actual-code review PASS
08:47:36Z, reviewer87,253/258,40033.77% (usage/runtime sources). No production change.
Full library binary predates assertion extraction; its unaffected behavior evidence
is retained, static/SQLx checks used corrected tree. Corrected focused rerun
focused-corrected.log is currently compiling; final evidence review still pending.

Corrected focused test PASS1/0failed/0ignored1.11s, focused-corrected.log;
script/session46727 EXIT0. Source unchanged since reviewer correction PASS.
README now reports phase03 evidence with final acceptance pending and user stop.
Post-documentation staged/unstaged whitespace checks PASS. Final evidence review
requested; PG will be cleanly stopped and retained after reviewer completion.

## Final independent acceptance and stopped handoff — 2026-09-29 08:53Z

Independent combined actual-code/correction/final-evidence acceptance PASS, no
unresolved findings. Reviewer verified all461workflow tests within2191library
passes, corrected focused test and full required static/schema gates. Reviewer
session01a0ec52-d514-73c3-96d4-124faffe4af2 measured90,444/258,40035.00%
08:53:08Z, usage/runtime capacity sources; quiescent with no further work.

Task PostgreSQL STOPPED CLEANLY; pg_ctl status exit3/no server. Retain data
/private/tmp/workflow-admission-pg-e3aa and log/private/tmp/workflow-admission-postgres.log.
Next authorized session may restart with explicit127.0.0.1:55439/socket/private/tmp/
max_connections200 and both DB URLs above; never reset the retained database.
No builds/tests/SQLx commands or approvals remain active. All prior applied
migrations through20260929100000 unchanged. No stage/unstage/commit/reset/deploy.
External website/index.html and website/assets/BB-logo1-transparent.png preserved.

Root owns final queue acceptance and RESUME/PROGRESS. Requested stopping boundary
is fullphase03. Next work is04-actions-http-and-delivery.md only after a future
resume; no04 expansion or implementation performed. Reuse this criterion matrix,
original plan,03.9 budgets/fairness briefs and exact logs rather than repeating
accepted reviews. Production handler/startup limitations above still apply.
