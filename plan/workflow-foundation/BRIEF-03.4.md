# 03.4 — fenced I/O ownership and actual-future supervision

Status: implemented and independently code-reviewed; all verification gates passed.
Final combined independent review passed; awaiting root acceptance. Expansion coverage
root-accepted2026-09-28 13:37Z; implementation evidence appended below.
Original authority:03-durable-runtime-and-persistence.md Execution and commits item3,
plus its applicable concurrency, bounded work, crash/restart and stale-write criteria.
03.1–03.3 accepted. EXPANSION.md remains the wider queue; later points retain owners.

## Reconciled contract

- Keep `background_tasks` and `task_attempts` as sole job/attempt ledgers. Add narrow
  required workflow lease port(s), typed scope/fence and validated lease policy in
  application/workflow. Do not widen legacy `TaskPersistence` or reuse legacy owner
  semantics (`TaskLeaseRef` carries agent ownership). Workflow execution identity and
  fresh per-claim generation are distinct types/facts. Every fence carries exact
  company/run/execution/job/worker/generation/attempt; job payload remains ID-only.
- Exact-job claim is the03.4 primitive; global selection/polling belongs03.8. Lock
  run→execution→job, consistently with03.3. Bound lock/statement/transaction waits;
  recheck scope, queue kind, payload identity, due time, active run, run deadline,
  execution incompletion and attempt allowance after locks. Reject pure steps and
  unsupported wait/control boundaries; the caller must use the pure batch or later
  owning subsystem. Scripted external handler is enough here; no provider dispatch,
  current-secret resolution or production cutover is implied.
- Claim freezes inputs via existing activation within the same transaction, sets
  run running if queued, mints fresh generation, installs bounded live lease and
  opens exactly one `task_attempts` processing row atomically. A failure at any write
  rolls back all three. Never use the legacy attempt UPSERT that can overwrite a
  prior generation. Duplicate claim while live returns unavailable; lost claim
  acknowledgement does not authorize an unfenced rerun.
- Retry/crash ownership needs a minimal exact-job transition here, not a second
  recovery engine: expired ownership closes its original attempt as lost; a new
  grant uses a new monotonically numbered attempt and generation, reuses frozen
  activation, consumes the existing job attempt budget, and cannot be granted until
  positive engine-owned retry delay has elapsed. No retries replenish counters or
  activation/root budgets. A bounded exact release after local interruption likewise
  consumes the attempt and schedules later eligibility, fenced on the current live
  owner. Expired/exhausted/deadline/terminal runs fail closed.03.9 owns global sweep,
  classified retry policy/final failure routing and operator retry/cancel commands;
  exhausted/deadline records need not receive invented terminal routing in03.4.
- Renew and live-fence validation use the same run-first lock order and all scope,
  generation, worker, attempt, active-run and deadline predicates. Extend only a
  live lease, never resurrect expiry or exceed run deadline. Persistence time is
  authoritative; check at final write/commit as needed, not only a stale pre-lock
  timestamp. Return explicit refusal separately from storage failure. Preserve all
  legacy query discriminator exclusions and workflow queue constraints.
- Application supervision directly owns/pins the actual handler future; no detached
  child work or outer-JoinHandle-only cancellation. External work starts only after
  claim/activation commit and runs without a DB transaction/lock. Supervise heartbeat,
  local cancellation/shutdown, lease-expiry safety deadline and operation/run deadline.
  Bound renewal latency so a hung database call cannot keep work alive past known
  expiry. Fail closed on refusal, renewal error or timeout; drop the work before
  returning/attempt cleanup. Cancellation remains observable even during renewal.
  Durable run cancellation/wait/terminal state is observed on heartbeat; local signal
  may accelerate it. No new public cancellation command in03.4.
- Success returns a bounded handler result with the unchanged fence for03.5, after
  a final live-owner check; this check is not commit authorization.03.5 must recheck
  fence and run state in the atomic result/run/audit/successor transaction and close
  the same attempt/job there.03.4 must not mark successful work completed independently,
  create a successor, or add a parallel result ledger. A stale result can never be
  represented as an unfenced completion request. External-effect receipts/replay
  authorization remain04; a lease by itself is not effect permission.

## Source seams and boundaries

New application/workflow lease/supervision module(s); persistence/workflow lease SQL,
focused isolated DB tests, application future-drop/heartbeat tests; module wiring.
Use existing columns/constraints unless concrete missing invariants require an additive
migration newer than133100. Applied migrations through20260928133100 are immutable.

Discovery: `activation.rs:39–100` freezes under run/execution locks and `102–117`
checks exact ID-only job association; caller graph includes public activate, batch
run_steps and focused activation tests. `batch.rs:49–98` gives run-first/bounded
transaction convention; pure requests already reject leased jobs. Legacy
`task/operations.rs:2127–2180` and `task/queue.rs:58–69` explicitly exclude workflow
jobs; keep those contracts unchanged. `task_queue.rs:113–142` has legacy-only lease
supervision callers in task_worker. `inbound_event_worker.rs:521–566` and
`memory_job_lease.rs:30–83` illustrate direct future ownership, but do not copy their
unbounded renewal gap. New workflow supervision must enforce known expiry during
renewal. `application/transport/lease.rs:25–35` is a transport-specific generic fence;
do not couple workflow semantics to that subsystem just for structural similarity.

## Acceptance and review gates

1. Two synchronized independent DB claimants get exactly one live generation and
   attempt; failed/lost claim acknowledgement preserves a recoverable single owner.
   Exact foreign/mismatched/legacy/not-due/pure/wait/terminal/deadline requests have
   no writes. Claimed inputs survive retries and changed available context.
2. Force crash/rollback after activation, lease update and attempt insertion; prove
   atomic rollback and clean retry. After committed claim then simulated process
   loss, expiry plus positive backoff yields one new generation/attempt; old ledger
   fact is retained and counters increase. Immediate repeated claim does not spin.
   Exhaustion refuses a new grant without replenishment.
3. Old generation (including same worker ID reused), wrong worker/scope/attempt,
   expired owner and cancelled/waiting/terminal/deadline run cannot renew, validate
   a result or release replacement work. Real renew/reclaim/cancel contenders
   serialize run-first; contention cannot cross the checked deadline. DB constraints
   reject any new incoherent lease/attempt shape if added.
4. Instrumented actual handler Drop proves stop on cancellation/shutdown, renewal
   false/error, hung renewal timeout, known lease expiry and operation/run deadline.
   Repeated successful heartbeats keep a long operation alive; a ready result racing
   cancellation/expiry does not win incorrectly. Future is gone before cleanup/return;
   no detached effect runs later. Bounded handler result and final fence handed to03.5.
5. A handler can acquire the run/execution/job from another DB connection while it
   runs, proving external work holds no transaction locks. Integration test runs
   pure→I/O boundary→claim/freeze→supervised scripted result; completion/successor
   assertions are deferred to03.5, not simulated with an alternate commit path.
6. Preserve03.2 activation and03.3 batch tests, shared queue/attempt isolation, scoped
   association, no legacy claims/controls stealing workflow jobs. Focused tests and
   full DB library suite at stock2MiB; migration checks if schema changes, SQLx prepare
   sequential with builds, locked offline all-target check/Clippy, fmt/diff/graft;
   independent actual-code and combined integration review with edits paused.

No unresolved user decision. The exact retry-counter mapping and bounded policy
values must be reconciled with existing queue schema before edits; they cannot weaken
attempt charging/backoff or advertise completed03.9 recovery. Root coverage acceptance
is the next action, not a user permission gate.

## Ownership

Root owns PROGRESS/RESUME and point acceptance. Implementer `/root/batch_verify`
UUID01a0e832-e897-7952-b436-ce88d876a887 owns this brief and taskPG RUNNING at
`/private/tmp/workflow-admission-pg-e3aa`, port55439, DBworkflow_admission. Explicit
DATABASE_URL/TEST_DATABASE_URL both `postgres://mac03@127.0.0.1:55439/workflow_admission`.
Nested reviewer `/root/batch_verify/review` UUID01a0e833-51cf-78f1-a208-4907154ee36c
is completed/available below threshold; reuse for independent review when ready.
No03.4 source edit/build has run. Preserve all existing work including deleted index
and marketing images/untracked website; no staging, commit, reset or deployment.

## Fresh implementer transfer

Root accepted coverage and chose natural early rotation before substantive03.4
implementation because prior03.3 context dominates. No implementation failure or
context-threshold breach. This subtree is retired; replacement must create its OWN
nested Astra/medium reviewer before edits and must not reuse the retired child.
Previous reviewer is completed/quiescent and has been told to remain stopped.

Exact next work: reconcile `background_tasks.retry_count/max_retries` semantics,
`task_attempts` uniqueness/status constraints and frozen workflow retry allowance
with the03.4 claim policy. Preserve a single attempt ledger, monotonic attempt
numbers and fresh generations; decide counter increment timing once, ensure expired
or explicitly released ownership charges an attempt and a positive engine-owned
retry delay, bound lease/renewal/transaction timing, and refuse exhausted/deadline
grants without inventing03.9 final routing. Then implement the accepted contract
and six acceptance groups above. No03.4 source or migration edit exists to resume.

Reuse source discovery above and accepted03.3 evidence.03.5 owns atomic result/run/
audit/successor completion and rechecks live fence;03.8 owns global polling;03.9
owns classified recovery/sweepers/fairness. TaskPG remains RUNNING and transfers
unchanged; no build/test/prepare processes. Applied migrations through133100 immutable.
Root owns PROGRESS/RESUME; replacement owns this brief and task database.

Retiring implementer startup8.27%, last expansion87664/25840033.93%
2026-09-28T13:36:51.852Z, usage/runtime sources; root boundary measurement35.14%.
Final cleanup sample is reported directly to root. Reviewer last parent-measured
55908/25840021.64%13:33:22.677Z token_count.info sources. Both below50%; rotation
is proactive at a clean boundary. No further assignments to this retired subtree.

## Implemented 2026-09-28 — awaiting root acceptance

Owner `/root/io_leases`, verified UUID01a0e83d-1ebb-7be2-bfaf-048abf7ec08f;
independent nested reviewer `/root/io_leases/review`, UUID01a0e83d-635d-7820-8474-0e664b942bed.
Both Astra/medium. Startup samples8.28%/7.23%; latest implementation sample44.88%
(115962/258400,13:56:51.537Z), reviewer29.47% (76143/258400,13:56:10.653Z).
Measurement sources token_usage_record.usage or token_count.info.last_token_usage;
capacity task_started.model_context_window or token_count.info.model_context_window.
Final samples reported directly to root.

Concrete counter reconciliation: existing workflow job `max_retries=3` is the
attempt allowance; `retry_count` counts consumed failures/losses, and the live
attempt is `retry_count+1`. No frozen definition currently supplies another retry
allowance. Claim does not increment; exact release or expired retirement increments
once and retains the original failed ledger row. A grant inserts a fresh generation
and new numbered attempt without UPSERT. Exhaustion refuses another grant without
routing a final failure (03.9). Retirement sets a fixed2second engine-owned delay;
even an ancient expired lease must pass through that delayed state before a grant.
LeasePolicy validates3–60second leases, heartbeat interval lease/3,1second persistence
operation bound. Existing job lease/queue constraints already cover the shape; no
migration or new owner/table was required.

New application `lease.rs` supplies typed exact scope/worker/generation/attempt,
required ports, validated policy and a fenced result. `supervise.rs` owns and drops
the actual handler future before bounded cleanup, observes cancellation even while
renewal hangs, and races both known lease expiry and operation/run deadline. Lease
windows use database time conservatively anchored before the request. Successful
work undergoes final live validation and returns a bounded result plus unchanged
fence; no I/O completion, successor or result ledger is created.03.5 must still
recheck the fence and atomically commit result/run/audit/successor/same attempt.

New persistence `lease.rs`/`lease_claim.rs` lock run→execution→job, check exact
scope/payload/queue/active state/due time/attempt allowance/I/O kind, and atomically
freeze activation, install ownership, open the attempt and mark the run running.
Renew/validate/release reject stale generation, reused worker, wrong attempt and
expired/terminal/waiting/deadline ownership. Bounded lock/statement/transaction waits
and final persistence-time predicates prevent resurrection after contention.

Independent actual-code review found two P2 issues and both are resolved:
1. Release now carries typed LeaseReleaseCause. Deadline/lost ownership map to
   timed_out/lease_lost; unclassified local/provider/storage interruptions preserve
   NULL rather than falsely recording shutdown or inventing03.9 classification.
2. Tests now mutate available run input across retry, mix existing run/execution/job
   identities, check waiting/terminal/deadline lease operations, and contend renewal
   past expiry. Reviewer rereview passed; final fixture-only deadline correction
   also passed rereview. Initial test failures were invalid fixtures (waiting_reason,
   deadline>created_at), corrected without changing production behavior.

### Verification evidence

All commands run in repository root; explicit DATABASE_URL and TEST_DATABASE_URL
were `postgres://mac03@127.0.0.1:55439/workflow_admission` for DB test commands.
No missing-database skip flag. RUST_MIN_STACK=2097152 for both test runs.

- `cargo test --locked --offline --lib workflow_io_`:13PASS,0fail,4.45s;
  `/private/tmp/workflow-03.4-focused.log`. Real synchronized claimants; lost ACK;
  crash/backoff/exhaustion/frozen mutation; write-boundary rollback; exact scope and
  fencing; renew/reclaim/cancel contenders; lock/deadline/expiry races; actual-future
  Drop during cancellation/refusal/error/hung renewal/expiry/deadline; repeated
  heartbeats; bounded result and final owner check; ready-result races; handler
  obtains independent DB locks; pure→I/O→supervised result integration.
- `cargo test --locked --offline --lib`:2020PASS,22existing ignored,0fail,80.65s;
  `/private/tmp/workflow-03.4-full.log`. Includes accepted activation/batch and legacy
  isolation tests plus isolated freshly migrated databases.
- `DATABASE_URL=... cargo sqlx prepare -- --all-targets`:PASS42.14s,
  `/private/tmp/workflow-03.4-sqlx.log`; no `.sqlx` diff (new queries are runtime SQL).
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS0.99s,
  `/private/tmp/workflow-03.4-check.log`.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS48.41s, `/private/tmp/workflow-03.4-clippy.log`.
- `cargo fmt --all -- --check`, `git diff --check`, `graft build`:PASS;
  `/private/tmp/workflow-03.4-{fmt,diff,graft}.log`.
- Applied migrations through20260928133100 unchanged. Reuse03.3 migration validation;
  new focused/full isolated database fixtures also exercised fresh migrations.
- Build/test/prepare commands ran sequentially; no live command remains. SQLx source
  metadata unchanged. Root PROGRESS/RESUME untouched by this implementer.

TaskPG remains RUNNING at/private/tmp/workflow-admission-pg-e3aa,port55439 and transfers
unchanged. No commit/stage/reset/deploy; no03.5 work. Final combined independent evidence review PASS2026-09-28 14:01Z: no unresolved
findings or missing03.4 gates. Reviewer now quiescent at78598/25840030.42%
(14:01:00.833Z). Implementer final-boundary sample121331/25840046.95%
(14:01:00.487Z). Root acceptance follows; this section does not self-accept the point.
