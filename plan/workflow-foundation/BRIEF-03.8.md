# 03.8 — durable PostgreSQL polling

Status: root coverage accepted2026-09-28; implementation in progress.
Authority: phase03 Execution item7 and applicable restart/concurrency acceptance.
03.1–03.7 accepted. Later03.9 and phases04–07 retain their owning contracts.

## Contract and boundaries

- Add an application-owned bounded polling/read port and reusable worker loop.
  PostgreSQL background_tasks/executions/waits/events remain the durable owners.
  Candidate discovery returns scoped IDs, never serialized contexts, and grants
  no ownership. Existing run-first pure/wait transactions and exact fenced I/O
  claims independently recheck eligibility. Competing polls may read the same
  candidate; only the existing transition/claim winner can advance or invoke I/O.
- Poll immediately on startup and periodically thereafter, including after empty
  pages, database errors, closed wakeup channels and reconnects. An optional
  in-process wakeup only shortens the next reread; no listener, notification ACK,
  SSE delivery or process-memory queue is required to recover durable work.
  Coalesce/rate-limit hints so notification storms cannot hot-spin the database.
- Discover due pending workflow jobs, expired workflow I/O ownership eligible
  for03.4 retirement, and exact parked waits with committed matching events or
  due deadlines. Preserve discriminator exclusions, active-run/deadline checks,
  attempt allowance and positive retry delay. Exclude live leases and completed
  jobs. Never gate event resumption on receiving its ephemeral notification.
- Bound page size, SQL/connection/operation time and loop concurrency. Use a
  deterministic cyclic scan cursor with unique tie-breaker, advancing across
  unsuccessful/unsupported candidates so a persistent head cannot hide later
  runnable work. Reset/wrap and startup full rereads ensure commits behind the
  cursor are revisited. Cursor is an optimization, never durable acknowledgement.
  Cursor fairness does not claim03.9 tenant budgets or classified poison recovery.
- Connect discovery to existing pure batching, wait park/resume, and fenced
  claim→supervise→complete through reusable application orchestration. A required
  injected handler executes supported external steps; tests use scripted handlers.
  No provider effect permission/replay semantics are inferred from a lease. Handler
  availability is explicit; unavailable later subsystem boundaries stay durable
  and do not prevent other candidates from progressing. Do not wire an incomplete
  provider dispatcher into main or legacy workers. Phase04/06 supplies production
  effect/agent handlers; phase08 owns application replacement and startup cutover.
- Phase07 owns parent-call wait settlement and consuming03.7 child wakeup facts.
  Do not forge generic wait events or claim that a child terminal fact can resume
  a parent before that handler exists. Its required consumer must reread the
  durable exact-child wakeup port; global parent scheduling integration is
  deferred with the absent workflow.call owner, explicitly rather than dropped.
- Shutdown owns/drops actual work, with no detached correctness task. Polling
  failures remain observable and retry after a positive interval.03.9 still owns
  classified failure routing, global recovery sweeps, operator commands and
  sustained tenant fairness; this point must not introduce a new failure ledger.

## Source seams

New application/workflow polling/worker modules and persistence/workflow polling
adapter plus isolated DB tests; module wiring. Existing batch.rs31–98/107–169
advances pure work to a durable boundary; lease_claim.rs5–91 owns exact claim and
positive-delay expired retirement; supervise.rs25–94 owns the actual future;
completion adapter owns atomic fenced result. waits.rs26–87 owns park/resume and
deadline sweep; select_due275–282 currently discovers deadlines only, so polling
must additionally discover stored event availability. No existing schema change
is assumed; any necessary migration must be newer than20260928150500, immutable.
Trace callers/exhaustive uses before adapting existing symbols.

## Acceptance gates

1. Real isolated PostgreSQL admission→pure→scripted I/O→wait→event/timer→successor
   progresses solely by periodic polling with no listener/wakeup sender. Verify
   committed results and successors, no repeated completed invocation.
2. Commit work while no worker/listener exists; reconstruct fresh worker/persistence
   handles and recover it. Stop after candidate discovery, claim, and committed
   completion/lost acknowledgement; restart rereads correct durable ownership,
   expiry/backoff and saved successor without duplicating logical work.
3. Independent concurrent pollers contend on identical durable work: one active
   I/O generation and actual handler invocation, one result/successor; pure and
   parked event competitors likewise progress once. Existing stale fences refuse.
4. Lost/spurious/closed wakeups and transient poll failure cannot disable periodic
   rereads. Timer due after an empty poll is found; event-before-park and event
   committed during worker downtime resume. No notification write is required.
5. More than one page, changing eligibility behind cursor, unsupported/failed
   candidates ahead of healthy work, live leases, future jobs, terminal/legacy
   records, payload bounds, cancellation and shutdown. No zero-delay retry based
   only on unchanged full pages; elapsed-time retry behavior is asserted.
6. Focused real DB tests and full DB library suite at stock2MiB; migrations/schema
   checks if added, sequential SQLx prepare, locked offline all-target check and
   Clippy, fmt/staged+unstaged diff checks, graft build, independent actual-code
   and final integration review. Logs /private/tmp/workflow-03.8-*.log.

## Ownership

Implementer /root/workflow_polling Astra/medium, verified session
01a0e893-c15e-73e0-bba7-ecab22bfa506; startup21334/2584008.26%
2026-09-28T15:13:22.399Z. Nested reviewer /root/workflow_polling/polling_reviewer
Astra/medium, session01a0e893-ffed-76f3-9e26-c8d4f11109dc; startup21218/2584008.21%
15:13:40.929Z. Sources token_usage_record.usage/task_started.model_context_window.
Capacity established before edits; stop/rotate50%. Root owns PROGRESS/RESUME.
TaskPG RUNNING /private/tmp/workflow-admission-pg-e3aa port55439 DBworkflow_admission;
DATABASE_URL/TEST_DATABASE_URL postgres://mac03@127.0.0.1:55439/workflow_admission.
Preserve all external work. No stage/commit/reset/deploy or03.9 advance.

## Implementation and correction checkpoint — 2026-09-28 15:27Z

Actual-code independent review PASS after two corrections. New application polling
port/validated policy and reusable single-slot worker poll immediately and repeatedly.
Postgres discovery reads bounded ID pages in UUID order, wraps after an empty final
page, and includes committed matching events, due timers and expired ownership.
Every operation uses existing run-first transitions/fences. Notifications use an
optional coalescing Notify; there is no listener/channel whose disconnect can disable
polling. Every page, including full unchanged pages, waits positively; hints cap
rereads at one per50ms. Worker shutdown drops the actual in-flight future and leaves
any committed interrupted claim recoverable by expiry with attempt accounting.

Explicit handler support prevents unsupported kinds from consuming new attempts.
A review correction adds a separate required retirement-only WorkflowLeaseRecovery
port: spent expired ownership retires even when its handler is absent, while stale
expired discovery never becomes a fresh unsupported claim. It reuses existing
lease lock/retirement code and does not add another attempt ledger. Classified
terminal routing/fairness remain03.9; production effects/agents and parent-call
settlement retain04/06/07 ownership. No main/legacy-worker integration is claimed.

Reviewer also required deterministic unsupported-first ordering and elapsed-time
assertions. Tests now place the unsupported unclaimed job first, assert positive
progress delay, and record handler-availability checks under a notification storm
across full unchanged pages and cursor wraps. New real competing expiry consumers
verify one charged lost attempt, and stale discovery cannot grant another.

Initial6test run:2PASS/4fixtureFAIL due invalid scripted context output/event schema;
corrected fixtures use actual contract, no production weakening. Next9PASS3.81s;
then11PASS3.94s. Pre-correction full suite2073PASS/22existing ignored95.01s.
Corrected13focused and final broader checks remain PENDING until evidence below.
No new migrations; applied migrations through20260928150500 remain immutable.
Reviewer /root/workflow_polling/polling_reviewer actual-code correction PASS,
73214/25840028.33%15:26:56.256Z. Implementer last106930/25840041.38%
15:26:25.356Z; same verified UUIDs and measurement sources as above.
TaskPG remains RUNNING unchanged; no03.9, stage/commit/reset/deploy.

The first corrected full run exposed two new fixture failures (2073PASS/2FAIL):
2second event/timer deadlines were computed before isolated database setup, which
can exceed that under suite load. Fixtures now use30minute deadlines; timer test
explicitly makes its parked timer due after empty discovery. No production bound
changed. Independent fixture-only rereview PASS15:30:59Z. Corrected focused13PASS
3.99s; final full2075PASS/0fail/22existing ignored91.94s. SQLx/static/final checks
remain in progress and are recorded below when complete.

## Final verification gates — 2026-09-28 15:35Z

All commands in repository root. DATABASE_URL and TEST_DATABASE_URL explicitly
postgres://mac03@127.0.0.1:55439/workflow_admission. DB tests use
RUST_MIN_STACK=2097152 SQLX_OFFLINE=true; no missing-database skip option.

- `cargo test --locked --offline --lib workflow_poll_ -- --nocapture`:
  13PASS/0fail3.99s; /private/tmp/workflow-03.8-focused-corrected.log.
- `cargo test --locked --offline --lib`:2075PASS/0fail/22existing ignored91.94s;
  /private/tmp/workflow-03.8-full-final.log. Fresh isolated migrations and all
  prior03 activation/lease/wait/wakeup regressions included.
- No new schema/migration. `cargo sqlx prepare -- --all-targets`:PASS42.10s;
  /private/tmp/workflow-03.8-sqlx.log. No staged/unstaged.sqlx metadata diff.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS1.07s;
  /private/tmp/workflow-03.8-check.log.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS46.99s; /private/tmp/workflow-03.8-clippy.log.
- `cargo fmt --all -- --check`, staged+unstaged diff checks, `graft build`:PASS;
  /private/tmp/workflow-03.8-{fmt,diff,diff-staged,graft}.log.

All build/test/prepare commands ran sequentially and are now complete. Final
combined evidence review requested next; root acceptance remains pending.
Latest implementer117157/25840045.34%15:35:18.531Z; reviewer idle80747/258400
31.25%15:31:12.829Z, verified UUIDs and runtime sources unchanged.
TaskPG RUNNING, no source/build process remains, no03.9/stage/commit/reset/deploy.

Final independent combined review PASS2026-09-28 15:36Z; no remaining findings
or acceptance gaps in03.8 scope. Reviewer inspected all final logs and reused
actual-code/correction/fixture reviews. Reviewer reported82189/25840031.81%
15:36:08.952Z, same verified UUID/sources. Subtree is quiescent; root acceptance
is next. TaskPG ownership transfers unchanged to root. No03.9 work started.
