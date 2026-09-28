# 03.6 — atomic durable parking and resumption

Status: expansion reconciled with current code; root coverage acceptance pending.
Authority:03 Execution item5, Failure/recovery and relevant Acceptance; phase05
Other waits defines event-before-park without moving human semantics into03.
03.1–03.5 are root-accepted. Preserve all staged/unstaged/untracked work.

## Contract and boundaries

- Implement a required application WorkflowWaits port and Postgres adapter. Concrete
  supported steps are existing registry wait.event and wait.timer; inputs/deadline,
  event/correlation and payload schema come from the frozen activation/bundle, never
  a caller-selected successor. Human assignment, decision submission/comments,
  action approvals, credentials and provider delivery remain04/05; no generic event
  API may settle decision.human or protected actions.
- Reuse workflow_waits, workflow_executions, background_tasks and the single
  batch_commit::complete writer. Parking locks run→execution→job→wait, freezes inputs
  once, atomically saves one wait plus its precise wait-owned notification intent,
  sets run waiting and makes the original job non-runnable without a lease. These
  local transitions require no I/O attempt; existing I/O claimant excludes waits.
  Notification intent is durable registration metadata keyed by the exact wait;
  dispatch, recipients, authorization and delivery receipts remain their existing
  phase04/05 owners, not a new notification queue. No external notification is sent
  by this point. Replay returns the same wait and intent without duplication.
- Add scoped durable incoming-event storage, not a work queue. Event identity,
  company/run/execution, name/correlation and bounded payload are immutable facts.
  Caller is a trusted internal event producer, not a public authorization bypass.
  Events are addressed to an exact admitted execution plus declared correlation;
  no newest-wait or thread heuristic, and unmatched ordinary ingress is unaffected.
  Unique producer event ID deduplicates equivalent submissions and rejects conflicts.
  Scoped foreign keys reject unrelated/cross-company execution/wait relationships.
- Event submission persists before any worker notification. Event-before-park is
  recoverable: parking/resumption can read previously committed events. A matching
  event, validated against the frozen output schema, is consumed once in the same
  transaction that records output/route, run state, audit and successor through the
  existing progression writer. Duplicate events/resume requests and lost ACK replay
  the same committed result. Wrong scope/name/correlation, malformed/oversize output,
  terminal/cancelled/deadline state cannot consume or advance. Invalid events must
  not create an unrecoverable poison head. Events are not directly human decisions.
- Every wait has a future deadline capped by run deadline. Timer due time completes
  using existing timer output contract; event expiry fails closed (no invented
  timeout route). Provide bounded deadline sweep/transition now so no new immortal
  parked state is introduced; global scheduling/polling integration belongs03.8–03.9.
  Cancellation versus resume/expiry serializes on the run. Expiry and event settlement
  are mutually exclusive; resumed/completed waits retain history and do not reopen.
- Bound transaction/lock/statement time, payload bytes and sweep batch. Preserve
  activation/root limits and frozen input. Reuse deferred deadline guards where
  applicable and close any park/resume delayed-commit window with additive SQL.
  Applied migrations through20260928141500 are immutable.

## Source seams

- Registry families.rs:121–139 and examples.rs:104–108 already define event/timer
  inputs and output schemas. Domain outcome.rs:49–82 has typed DurableWaitRequest;
  ids.rs:130 supplies WaitId. Reuse domain state/route decisions and existing limits.
- workflow_runs migration20260928070000:51–65 owns waits, presently only scoped
  identity/reason/deadline. Extend it via an additive migration, never replacement.
- activation.rs:39–100 freezes inputs; batch.rs:49–98 demonstrates run-first load;
  completion.rs:39–101 supplies completion transaction pattern. All are persistence.
- batch_commit.rs:5–107 is shared progression writer; completion_job.rs:5–60 owns
  job closure. Extend a typed parked owner narrowly, retaining pure/I/O behavior.
  Trace actual indexed calls with graft before changes (ambiguous complete has no
  exact edges; use exhaustive batch_commit::complete search).
- New application/persistence wait modules, ID exports, module wiring and isolated
  DB tests are expected. No legacy queue or human approval owner replacement.

## Acceptance gates

1. Pure→wait→event/timer→pure/end integration: frozen inputs, exact wait/intent,
   no runnable parked job or lease, output/route/audit and one successor atomically.
   Another admitted message in the same thread progresses while first is parked.
2. Real synchronized park/park, event/event, resume/resume and park/event competitors
   on independent DB connections: one wait/intent, one consumption and successor;
   event-before-park and restart with fresh adapter work without listeners. Duplicate
   IDs with changed payload refuse. Conflicting scope/name/correlation/schema refuse
   without consuming valid work. Late old-execution events do not affect newer waits.
3. Crash injection at wait/intent/job/run/audit and event/result/successor write
   boundaries proves rollback; commit/lost-ACK replay proves no duplicate work.
4. Deadline cap, overdue timer, event expiry and run cancellation versus resumption
   with real competitors; expired waits transition durably and repeated sweep/poll
   cannot reclaim the same settled rows. Delayed commit cannot exceed authorization
   deadline. Terminal replay is read-only and never schedules after cancellation.
5. Scoped FK/unique constraints, bounds and existing activation limits; no human
   decision or I/O lease bypass. Independent reviewer inspects actual code/callers.
6. Focused wait tests plus full DB library suite at stock2MiB, isolated fresh
   migrations, sequential SQLx prepare/builds, locked offline all-target check and
   Clippy, fmt/staged+unstaged diff checks and graft build. Logs under
   /private/tmp/workflow-03.6-*.log. No acceptance claimed with missing gates.

Root owns PROGRESS/RESUME and acceptance. Implementer /root/durable_waits,
Astra/medium UUID01a0e86a-1f1d-7f71-9e0d-c615bb0600d2; startup21358/2584008.27%
14:27:51.027Z. Independent reviewer /root/durable_waits/reviewer,
UUID01a0e86a-4fbf-7a92-9ae9-c6f6c90dc2ec; startup21113/2584008.17%14:28:06.232Z.
Sources token_usage_record.usage/task_started.model_context_window; rotate50%.
TaskPG RUNNING /private/tmp/workflow-admission-pg-e3aa,port55439,DBworkflow_admission;
DATABASE_URL/TEST_DATABASE_URL postgres://mac03@127.0.0.1:55439/workflow_admission.
No stage/commit/reset/deploy; no03.7 until03.6 root acceptance.

## Implementation checkpoint — 2026-09-28 14:37Z

Root accepted coverage before source edits. Core implementation is UNVERIFIED pending
independent review and remaining checks. New application waits.rs and persistence
waits.rs/wait_commit.rs reuse the shared progression writer via CompletionOwner::Parked.
The original background job completes its local registration at park; no lease/attempt
is held while waiting, and resumption never reopens that job. Atomic wait row contains
version1 wait_registered intent with exact wait/run/execution IDs; it is registration
metadata for later delivery ownership, not an external notification or new queue.

Event intake targets one admitted execution, freezes activation on its first valid
signal if needed, validates exact name/correlation and schema before storage. Refused
signals roll back activation. One wait consumes one event; incoming immutable facts
are retained and bounded to128 per execution. No public authorization endpoint added.

Routine timer reconciliation: already-due timers may register their actual past due
instant and are immediately eligible for resume/sweep. Event deadlines must be future.
Both cap to run deadline; reaching the run deadline fails rather than completes.
Migration20260928144000 applied97.73ms and is now IMMUTABLE, including a timer-specific
exception to the original future-wait constraint. It extends the existing wait owner,
adds scoped event facts, and guards event/run deadlines at commit.

Current focused run PASS9/0fail6.92s at stock2MiB, explicit task DB URLs, command
`cargo test --locked --offline --lib workflow_wait_ -- --nocapture`; log
/private/tmp/workflow-03.6-focused.log. First4-test run had3invalid-schema fixture
failures: the registry example declares a string whereas test supplied an object;
fixture now explicitly declares its bounded object schema, with production unchanged.
Initial all-target compile PASS46.59s; its diagnostic log was shared accidentally by
an earlier failing check still releasing the build lock, so rerun a clean final gate.
No parallel compilation/SQLx prepare is active now. fmt and both diff checks PASS.
Full suite, SQLx prepare and final check/Clippy/graft are PENDING.

Independent reviewer is actively inspecting source with edits paused. Self-audit
candidates sent for review: count only new transitions in competing sweep results,
bound initial sweep scan duration, SQL immutable-event guard, and additional event
fault/FK/expiry-race tests. These are not yet resolved. Latest implementer context
102918/25840039.83%14:36:10.435Z, reviewer starting22073/2584008.54%14:36:09.036Z;
same UUID/sources above. TaskPG RUNNING unchanged. No03.7 or stage/commit/reset/deploy.

## Corrected implementation and final gates — 2026-09-28 14:48Z

Independent actual-code review required five corrections, all implemented and
rereviewed PASS: a due timer exhausting its activation budget now durably fails
its wait/run with typed reason instead of poisoning repeated sweeps; initial sweep
selection has1second SQL lock/statement limits and2second caller timeout; event
facts reject SQL UPDATE mutation; sweep counts only newly committed transitions;
and adversarial acceptance coverage was expanded. No source edits occurred while
review was active. The timer failure reasons are activation_limit/invalid_output;
storage errors remain distinct. Full classified recovery/operator commands stay03.9.

Additive migration20260928144500 applied8.37ms and is IMMUTABLE. It adds coherent
failed-wait reasons and event immutability;144000 and all earlier migrations remain
unchanged. Schema was inspected after migration (/private/tmp/workflow-03.6-schema.log).
Correction code review PASS at reviewer79136/25840030.63%14:42:04.904Z. One16-test
run had15pass/1fixtureFail because the newly inserted foreign company lacked required
user_id. Corrected fixture selects the existing owner into a distinct foreign company
ID, preserving the cross-company test; narrow rereview PASS81109/25840031.39%
14:42:50.588Z. No production change to clear that fixture failure.

Final current-tree verification, all commands in repo root; DATABASE_URL and
TEST_DATABASE_URL explicitly postgres://mac03@127.0.0.1:55439/workflow_admission for
DB tests, RUST_MIN_STACK=2097152 and SQLX_OFFLINE=true; no database skip option:

- `cargo test --locked --offline --lib workflow_wait_ -- --nocapture`:16PASS,
  0fail,8.62s; /private/tmp/workflow-03.6-focused.log. Real park/signal/resume and
  sweep competitors; event-before-park/restart; same-thread independent messages;
  pure→wait→event/timer progression; invalid scope/kind/schema/bounds; write faults;
  poison/valid timer batch; expiration/cancellation races; event/run delayed-commit
  deadline guards; event immutability/scoped FKs; old execution events and saved replay.
- `cargo test --locked --offline --lib`:2050PASS,0fail,22existing ignored,100.31s;
  /private/tmp/workflow-03.6-full.log. Includes all previous phase03 regressions and
  isolated databases migrated from fresh schema.
- `cargo sqlx migrate run`:PASS for both new migrations; logs
  /private/tmp/workflow-03.6-migrate.log and workflow-03.6-migrate-guards.log.
- `cargo sqlx prepare -- --all-targets`:PASS46.44s;
  /private/tmp/workflow-03.6-sqlx.log; no staged/unstaged.sqlx metadata diff.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS0.94s;
  /private/tmp/workflow-03.6-check.log.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS51.16s; /private/tmp/workflow-03.6-clippy.log.
- `cargo fmt --all -- --check`, staged+unstaged `git diff --check`, `graft build`:
  PASS; /private/tmp/workflow-03.6-{fmt,diff,graft}.log.

Final checks/prepare/tests ran sequentially with no live process remaining. Reviewer
combined final evidence review requested next; root acceptance remains pending.
Latest implementer122108/25840047.26%14:46:53.096Z, same UUID/sources above.
TaskPG RUNNING unchanged; no stage/commit/reset/deploy; no03.7. Root owns acceptance
and PROGRESS/RESUME. This checkpoint supersedes earlier pending gate statements.

Final independent combined review PASS2026-09-28 14:48Z: all findings resolved,
no remaining acceptance gaps. Reviewer /root/durable_waits/reviewer is completed and
quiescent, reused actual-code/fixture review and inspected all final logs. Reported
sample82247/25840031.83%14:48:27.272Z; owner will verify final returned sample.
Implementer final pre-review boundary124679/25840048.25%14:48:28.396Z.
All03.6 gates complete; root acceptance is next. No commands or source edits remain
active. TaskPG stays RUNNING and ownership transfers unchanged to root. No03.7.

Owner-verified returned samples14:48:39Z: implementer125476/25840048.56%;
reviewer88304/25840034.17%, token_count.info.last_token_usage /
token_count.info.model_context_window. Same verified UUIDs. Both quiescent.
