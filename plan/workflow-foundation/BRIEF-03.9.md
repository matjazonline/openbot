# 03.9 — classified recovery, controls and bounded scheduling

Status: root accepted expansion2026-09-28; bounded domain policy fragment implemented.
Runtime integration and full03.9 acceptance remain pending.
Authority: original03 Failure and recovery and complete Acceptance, reconciled
with EXPANSION and accepted BRIEF03.1–03.8. Original nested criteria remain mandatory.

## Contracts and delivery order

1. **Classified recovery.** Reuse domain StepFailure/FailureClass/resolve_outcome;
   handlers report typed failure, never raw errors disguised as output. The engine
   decides retry eligibility from durable attempts and remaining deadline/budgets.
   Retry uses positive capped exponential backoff; expiry, interrupted work and
   abnormal death consume the existing task_attempts/background_tasks allowance.
   Exhausted/terminal outcomes durably fail or take the frozen final_error route.
   Extend the shared batch_commit progression writer for error routing; never add
   a second successor/result ledger. Preserve frozen inputs, original execution
   identity and committed results. Persist bounded classified codes; do not copy
   provider diagnostics into replies/logs. Pure/activation/output failures also
   leave the claimable set, so a poison batch cannot recur at unchanged time.
2. **Maintenance and controls.** Bounded PostgreSQL discovery plus run-first
   transitions sweep overdue runs in every active state, overdue waits and expired
   ownership, including exhausted jobs. Cancellation closes runnable jobs, current
   attempts and open waits atomically with the run/audit; heartbeat refusal drops
   actual work. Retain immutable results, notifications and external receipts.
   Company-scoped authorized commands use expected run revision plus stable command
   identity; current authorization is checked on replay. Safe retry reopens only
   an eligible failed execution, retaining frozen inputs, job/action identities,
   prior attempt evidence, deadline and all consumed budgets. Changed input/bundle
   is a new admission. Exhausted/deadline/unsafe-effect cases refuse retry; operator
   controls cannot silently grant attempts. No route may resume a completed step.
3. **Durable budgets.** Preserve existing step/total-activation/context/output
   bounds and enforce exhaustion as an observable state rather than a hot rollback.
   Add bounded root accounting and an atomic reservation port for model calls and
   repetition/activation costs. Reservations occur before work, survive restart,
   deduplicate equivalent reservation identity and never refund ambiguous work or
   replenish on retry. Shared root accounting must not acquire another run lock:
   run first, then its budget row, consistently across competing descendants.
   Root linkage is tenant-scoped and immutable. Phase07 owns child-call creation
   and inherited selection; phase06 owns actual model integration. Test reservation
   refusal with scripted callers now; do not claim missing production handlers are
   enforced. Oversize values reject at boundaries; existing scoped artifact
   references can be used, but no unscoped blob store or automatic copying is added.
4. **Global capacity and tenant fairness.** PostgreSQL remains sole execution
   ownership. Add bounded global/per-company capacity checks at the real I/O claim
   boundary, serialized against competing claims across worker instances. Durable
   scheduling metadata may track company turns, but is not another work queue or
   attempt owner. Fair candidate selection plus per-company capacity prevents a
   continuously replenished noisy tenant starving a quiet eligible tenant; local
   worker concurrency is also bounded. Expired/terminal work never holds capacity
   indefinitely. Do not hold scheduler coordination while acquiring another run:
   every transition retains run-first ordering. Waiting/unsupported work releases
   capacity and cannot monopolize a scheduling page. Poll/recovery errors back off.
5. **Combined phase03 gate.** Integrate admission→pure→scripted I/O→wait→recovery→
   completion with fresh worker handles and real competing connections. Reuse
   unaffected accepted crash/race/history evidence, rerun the full database suite,
   and have the independent reviewer assess combined runtime effects.

## Effect and subsystem boundaries

A failure class or lease never proves a write safe to replay. The handler contract
must distinguish a retry-safe failure from unknown effect outcome; ambiguous work
stays visibly suspended for the phase04 reconciliation owner. Expired effect-capable
work must not blind-dispatch writes. Preserve the same logical action identity and
require handler-owned receipt/replay checks before any later dispatch. Phase04 owns
the actual action/receipt/reconciliation service,06 model handlers,07 child-call
settlement/cancellation propagation,08 startup replacement,09 public UI. This point
ships reusable commands/ports/worker plus scripted enforcement tests, not invented
production effects or parent-event consumers.

## Source seams

- domain/workflow/outcome.rs86–227 already owns failure class and final-error routing;
  definition.rs38–52 owns step/context limits and final_error.
- application/workflow/supervise.rs25–110 currently discards handler failure detail;
  worker.rs84–146 drives pure/wait/I/O boundaries. Adapt at these ownership seams.
- persistence/workflow/lease_claim.rs5–115 and lease.rs169–226 own claim, retirement
  and attempt debit; spent pending rows currently need terminal recovery.
- batch_commit.rs5–107 is the sole result/successor writer; failure routing extends
  it. activation.rs39–100 freezes input and enforces activation/context limits.
- polling.rs15–97 discovers ID-only work; bounded recovery/fairness adds to this
  reusable worker, not legacy task claims. contracts.rs198–241 and existing service
  cancellation contracts must be reconciled before adding command types.
- Add cohesive application/domain/persistence recovery, control, budget/scheduling
  modules and isolated DB tests as needed. Trace callers before modifying symbols.
  Migrations through20260928150500 are immutable; new schema is additive only.

## Acceptance matrix

1. Retryable/terminal/unknown-effect classification, backoff cap, attempt exhaustion,
   expiry/death debit, final_error branch/end/no-route, invalid pure/output/activation
   failure. Consecutive poison-batch polls at unchanged DB time reclaim nothing.
2. Concurrent failure/completion/expiry/cancel/retry races: one disposition, one
   closed attempt, no stale successor or budget replenishment. Crash before and
   after failure/control commits; duplicate command returns saved result, conflicting
   identity/stale revision/unauthorized tenant refuses without writes.
3. All active run states and wait kinds have bounded overdue transitions; terminal
   replays remain read-only. Cancellation drops live actual handler future and
   preserves effect truth/results. Unsafe ambiguous work cannot operator-retry.
4. Concurrent root reservations hit exact limits, duplicate reservations debit once,
   conflicting payload/scope refuses, restart/retry preserves consumption. Step,
   model, repetition, total activation, deadline and byte ceilings exercised at
   boundary and one over. Existing root/child lock-order invariants remain intact.
5. Sustained replenished mixed-tenant load with multiple workers proves global and
   per-company active maxima and bounded progress for quieter tenants. A hung handler,
   unsupported kind or poison tenant cannot consume all progress. Expiry/restart
   recovers capacity without bypassing attempt/fence checks.
6. Preserve03.1–03.8 admission dedup, independent same-thread messages/history cutoff,
   frozen input/replay, atomic completion/parking/successor, event-before-park,
   competing claimants, parent wakeup lock order and notification-free restart.
   Existing effects use scripted receipt-aware handlers; real delivery remains04.
7. Per fragment meaningful focused stock2MiB DB tests; at point completion full DB
   library suite, fresh isolated migrations/schema invariants, sequential SQLx
   prepare, locked offline all-target check/Clippy, fmt, staged+unstaged diff checks,
   graft build. No raised resource bound without CI early-failure signal. Exact
   commands/results/logs recorded below. Independent actual-code and combined review
   required, with source edits paused during review; no self-review substitution.

## Ownership and baseline

Implementer /root/recovery Astra/medium, verified CODEX_THREAD_ID
01a0e8ef-3c44-7692-a070-e3dc2569a080; startup24040/2584009.30%16:53:18Z.
Reviewer /root/recovery/reviewer Astra/medium, verified session
01a0e8ef-806a-7be2-a510-43b40fc1e0b3; startup23759/2584009.19%16:53:37Z.
Both sources token_usage_record.usage/task_started.model_context_window2026-09-28.
Nested capacity confirmed before edits. Root owns PROGRESS/RESUME and acceptance.
Baseline HEAD83bb29d4759b3dff00d5821864ffe8f6c8e0260b, clean index/worktree at startup;
earlier external staged changes have been committed externally. Preserve all work.
No stage/commit/reset/deploy. Retained taskPG initially STOPPED at
/private/tmp/workflow-admission-pg-e3aa,port55439,DBworkflow_admission,
socket/private/tmp,max_connections200,log/private/tmp/workflow-admission-postgres.log.
Both DATABASE_URL and TEST_DATABASE_URL explicitly
postgres://mac03@127.0.0.1:55439/workflow_admission. Implementer owns restart/cleanup.

## First bounded fragment — pure recovery decisions

Root selected a bounded domain fragment before worker rotation. Added
domain/workflow/recovery.rs and recovery_tests.rs, exported through mod.rs.
RecoveryPolicy uses existing StepFailure/FailureClass/RetryEligibility and delegates
final-error routing to existing resolve_outcome. AttemptBudget includes the failed
attempt already charged by persistence; it cannot manufacture a new allowance.
Positive exponential backoff starts at2s and caps at300s by default; validated custom
bounds are1ms–1hour. Saturated constant-work calculation handles extreme counters.
Deadline/work-budget exhaustion forbids successors; unknown effects require
reconciliation even when the run also expires. Reconcile never extends a deadline.

These are pure decisions, not ownership/effect permission. The types currently
have no production consumers. Subsequent fragments MUST apply them under run-first
locks, atomically charge existing attempts, preserve root budgets and final-error
semantics, and integrate controls/sweepers/fair scheduling. Current lease retirement
still uses its prior behavior until that integration lands. No SQL, schema, worker,
provider, model or PG change is included, and no runtime03.9 claim follows.

Independent actual-code review PASS with no findings2026-09-28 17:00Z, verified
reviewer01a0e8ef-806a-7be2-a510-43b40fc1e0b3,44162/25840017.09%17:00:05.555Z,
token_usage_record.usage/task_started.model_context_window. Reviewer inspected
actual policy/tests/exports and existing resolve_outcome plus references; reviewed
scope is this domain fragment only. Source edits paused during review. Final
verification evidence pending below. No database was started or reset.

### Fragment verification2026-09-28

Commands ran in repository root; build/test/Clippy sequential. Tests explicitly
used RUST_MIN_STACK=2097152 and SQLX_OFFLINE=true, and selected only pure domain
tests (no database-skip flag). PostgreSQL stayed stopped throughout.

- `RUST_MIN_STACK=2097152 SQLX_OFFLINE=true cargo test --locked --offline --lib
  workflow_recovery_`:5PASS/0fail0.00s; compile76s.
  `/private/tmp/workflow-03.9-policy-tests.log`.
- `RUST_MIN_STACK=2097152 SQLX_OFFLINE=true cargo test --locked --offline --lib
  domain::workflow::`:34PASS/0fail0.10s, including existing outcome/routing/state
  and context tests. `/private/tmp/workflow-03.9-policy-domain-tests.log`.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS44.51s;
  `/private/tmp/workflow-03.9-policy-check.log`.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS53.71s; `/private/tmp/workflow-03.9-policy-clippy.log`.
- `cargo fmt --all -- --check`, `git diff --check`, `git diff --cached --check`,
  `graft build`:PASS; `/private/tmp/workflow-03.9-policy-{fmt,diff,diff-staged,graft}.log`.

No SQL or migration changed; SQLx prepare and database/competition gates are due
with actual persistence integration. Full03 acceptance remains unverified, not
waived. No source changes after scoped code review. Final fragment evidence review
requested next, then root acceptance and early worker rotation.

Final independent fragment evidence review PASS2026-09-28 17:03Z; no findings.
Reviewer verified49816/25840019.28%17:03:25.716Z, same session and runtime sources.
Implementer boundary109308/25840042.30%17:03:29.629Z, same session/sources.
Subtree quiescent; root acceptance next. Early rotation before the much larger
persistence fragment is intentional. Do not reuse this retired reviewer subtree.
`pg_ctl -D /private/tmp/workflow-admission-pg-e3aa status` confirmed no server
running (exit3); retained cluster unchanged. No live build/test/prepare processes.
No stage/commit/reset/deploy; root PROGRESS edits preserved. Known graft savings
reported by this implementer's untruncated calls total approximately2,795,931 tokens
(includes the map's whole-repository baseline estimate, not measured usage savings).

## Durable failure fragment — implementation in progress (2026-09-28)

Implementer /root/durable_recovery Astra/medium session
01a0e8f9-c524-73e2-835e-6cac6c5a0d1a; nested reviewer /root/durable_recovery/reviewer
session01a0e8fa-0073-7973-9f5d-c29968ea7dc0, capacity established before edits.
Root accepted this bounded scope: typed handler StepFailure/RetrySafety through
supervision to run-first fenced retirement, existing attempt/job debit and capped
backoff, terminal/reconciliation decisions, and frozen final_error progression via
shared batch_commit. No diagnostics persist. Explicit handler evidence controls
returned-failure safety. Interrupted/expired context.load and memory.load are
engine-owned reads; other kinds suspend pending effect reconciliation. Unknown
outcomes never create a successor or another claimable job.

Implementation currently UNVERIFIED. Migration20260928173500 is now applied to the
retained task database and IMMUTABLE; add a follow-up for corrections. It adds
bounded failure metadata to existing task_attempts and deferred retirement fencing.
Final-error progression records the explicit final_error route and JSON null as
its non-result; classified error detail stays in the attempt ledger. No-route
failure keeps execution inputs available for the later safe-control owner.
Reconciliation retains the original run deadline; all-state overdue sweeping is
still excluded from this fragment and required before03.9 acceptance.

Outstanding scope remains pure/activation/output-validation poison handling,
spent pending legacy rows, overdue run/wait sweepers, authorized idempotent controls,
root reservations/budgets, fair global/company capacity and combined full phase03
gates. Current failure work-budget snapshot has only already-existing step ceiling
and deadline/attempt enforcement; do not infer root/model budget enforcement.

Initial offline all-target check PASS42.81s before final test/decomposition edits;
/private/tmp/workflow-03.9-durable-check.log. Migration PASS38ms;
/private/tmp/workflow-03.9-durable-migrate.log. Focused tests currently running in
/private/tmp/workflow-03.9-durable-tests.log. Both DATABASE_URL and TEST_DATABASE_URL
explicitly postgres://mac03@127.0.0.1:55439/workflow_admission; task PostgreSQL was
restarted, remains running, no reset. Sandbox blocked initial TCP connection;
escalated task-local commands succeeded. No stage/commit/deploy.

### Durable fragment correction and verification checkpoint

Independent actual-code review found a commit-time run-deadline gap for expired
retirement scheduling and invalid error-route fixtures. Added follow-up migration
20260928174000 (applied and immutable): deferred retirement checks original deadline
for pending retries or successor creation while allowing terminal/reconciliation
settlement. Tests now use the actual source key `routes.error`, which compiles to
domain `final_error`. Reviewer independently corrected its initial fixture-key
recommendation after checking compiler/wire.rs71–120. Earlier failed test runs were
fixture-only failures; they are superseded by the final passing runs below.

Post-correction commands (both DB URLs explicitly point to retained task DB,
no skip flag, SQLx/build commands sequential):
- `RUST_MIN_STACK=2097152 SQLX_OFFLINE=true cargo test --locked --offline --lib workflow_failure_`:
  8PASS0fail3.10s; /private/tmp/workflow-03.9-durable-tests-final.log.
  Covers real duplicate retirement, failure/completion race, positive poison backoff,
  preserved frozen input, classified diagnostic exclusion, terminal no-route/end/branch,
  supervisor unknown-effect suspension, effect-capable interrupted/expired suspension,
  exhausted expiry final_error, crash-at-commit rollback, live-lease expiry at commit,
  and run-deadline expiry during deferred commit.
- `RUST_MIN_STACK=2097152 SQLX_OFFLINE=true cargo test --locked --offline --lib lease_tests`:
  63PASS0fail21.64s; /private/tmp/workflow-03.9-durable-integration.log. Includes
  affected completion, wait, polling, restart and competing ownership regressions.
- Earlier `workflow_io_`:13PASS4.59s; superseded for database cases by integration
  suite, still supplies unchanged application supervisor actual-future-drop tests.
- `cargo sqlx prepare -- --all-targets`:PASS18.86s;
  /private/tmp/workflow-03.9-durable-sqlx-final.log. Runtime queries generate no new
  offline metadata; no .sqlx diff. Real DB tests validate their shape.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS1.03s;
  /private/tmp/workflow-03.9-durable-check-final.log.
- Final fmt, staged/unstaged diff checks and graft build PASS;
  /private/tmp/workflow-03.9-durable-{fmt,diff,diff-staged,graft}-final.log.
- Final Clippy pending at this checkpoint; see
  /private/tmp/workflow-03.9-durable-clippy-final.log.

Independent scoped code+test review PASS2026-09-28 17:48Z, no findings remaining;
reviewer session01a0e8fa-0073-7973-9f5d-c29968ea7dc0 measured92467/25840035.78%
17:48:12.129Z, runtime usage/capacity records. Source edits paused during both reviews.
Full03.9 remains incomplete; exclusions listed above are required next fragments,
not waived criteria. Root acceptance of this bounded fragment awaits final Clippy.

Final gate update2026-09-28 17:49Z: locked offline all-target Clippy PASS23.29s,
/private/tmp/workflow-03.9-durable-clippy-final.log. All scoped fragment gates now
pass; root acceptance is next. Task PostgreSQL STOPPED cleanly via escalated pg_ctl
(the sandbox initially refused its signal), cluster retained unchanged at
/private/tmp/workflow-admission-pg-e3aa. No build/test/SQLx process remains.
Implementer measured125915/25840048.73%17:49:00.595Z, session and runtime sources
above. Early handoff now; no further substantive assignment to this subtree.
Reviewer scoped code+tests PASS remains current; final static evidence is listed
above. Root owns PROGRESS/RESUME acceptance. No stage/commit/reset/deploy.
Known graft reported estimate totals303093 tokens for this implementer (file-read
baseline estimate, not measured usage savings). Next worker should begin the
excluded pure/activation/output poison and maintenance/control fragments from
these exact current contracts; do not redo accepted domain or durable recovery.

## Pending poison fragment — unverified implementation 2026-09-28

Root accepted scoped expansion: deterministic pure/input/output failures leave the
runnable set, spent pending work is discovered and retired without resetting/debiting
its spent allowance. Preactivation failures do not fabricate frozen inputs; activated
failures reuse recovery's final_error decision and batch_commit. Earlier pure steps
in the current batch commit with its terminal failure. Infrastructure/SQL failures
still roll back. Effect-capable invalid I/O results and exhausted ambiguous pending
work suspend for reconciliation. Polling/worker support uses existing jobs only.
Wait parking also settles deterministic activation/specification failure; event
admission remains read-only on validation failure. All-state overdue sweeps, controls,
root accounting, fairness/capacity and full03 gates remain excluded and required.

Implementer /root/poison_recovery session01a0e923-f404-7e73-ae59-e9d95e6ddd13,
startup24080/2584009.32%17:50:52Z; milestone91689/25840035.48%17:58:50Z.
Reviewer /root/poison_recovery/reviewer session01a0e924-374b-7582-b0b7-b35ff24c9de2,
startup23617/2584009.14%17:51:08Z. Runtime usage/context capacity sources.
Nested reviewer capacity confirmed before edits. Root owns PROGRESS/RESUME.
Initial offline locked all-target check PASS48.65s before final integration/tests,
/private/tmp/workflow-03.9-poison-check.log. First test compile exposed accidental
signal-path replacement; restored its original non-mutating validation before rerun.
Focused stock2MiB tests running /private/tmp/workflow-03.9-poison-tests.log;
NOT yet verified. Task PostgreSQL RUNNING, retained cluster and explicit URLs above,
no reset or migration added. Independent review is next with source edits paused.

### Pending poison fragment handoff — implemented, NOT verified (2026-09-28 18:09Z)

Scoped changes: new pending_recovery.rs + pending_recovery_tests.rs + pending_io_tests.rs;
activation context-limit classification; pure batch failure disposition/prefix preservation;
I/O claim activation and completion output/budget settlement; pending exhausted polling and
worker dispatch; wait parking kind/input/deadline checks. No migration added. Prior accepted
policy/durable failure work and external HEAD83bb29d preserved; no stage/commit/reset/deploy.

Independent production-code review corrected four findings: wrong-kind wait parking now
refuses before activation, expired event input deadlines settle, I/O step budgets retire
through existing fence/safety policy, and completion Option contract documents settlement.
Second review corrected activated InvalidInput final_error routing and invalid-wait fixture.
Final reviewer reports no further production findings; one legacy wrong-kind expectation
was changed from is_err to unwrap().is_none(), preserving unchanged snapshot. Reviewer
session01a0e924-374b-7582-b0b7-b35ff24c9de2,99355/25840038.45%18:07:20Z, runtime sources.
This is scoped code-review evidence only; test failures below prohibit fragment acceptance.
Reviewer is idle/retired; next implementer creates its own reviewer per skill topology.

Evidence/results:
- First6 focused workflow_poison_ PASS6/0 in1.37s, stock2MiB, explicit task DB URLs,
  /private/tmp/workflow-03.9-poison-tests.log. Earlier sandbox/startup failures superseded.
- Next expanded focused run9PASS1FAIL3.20s: invalid-wait source omitted required value,
  compiler rejected before test; corrected fixture next. /private/tmp/workflow-03.9-poison-tests-final.log.
- Latest `RUST_MIN_STACK=2097152 SQLX_OFFLINE=true DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission cargo test --locked --offline --lib admission_tests`:
  122PASS3FAIL44.16s, /private/tmp/workflow-03.9-poison-integration.log.
  1. pending_io_tests::workflow_poison_wait_deadlines_and_invalid_input_settle line137:
     expected running error successor, actual failed. Investigate whether invalid RFC3339
     is rejected by prepare_inputs BEFORE activated_at is written, in which case terminal
     preactivation failure is correct and the test must separate invalid-input terminal
     from an already-activated expired event deadline with an error successor. Do not
     fabricate frozen inputs to force routing. Production change remains unverified here.
  2. polling_tests::workflow_poll_shutdown_drops_actual_future_and_leaves_recoverable_lease
     line54 Elapsed timeout. Reproduce in focused lease/polling suite, investigate load or
     behavior; do not claim preexisting flake or increase bounds without evidence.
  3. wait_tests::workflow_wait_refuses_scope_kind_and_invalid_events line155 stale is_err
     assertion. Exact source assertion is ALREADY corrected after that binary compiled;
     needs rerun. No production edits since final scoped review.
- Final cargo fmt --all -- --check, git diff --check, git diff --cached --check PASS;
  /private/tmp/workflow-03.9-poison-{fmt,diff,diff-staged}-final.log.
- Initial all-target offline check PASS48.65s before later edits; not final evidence.
- Final SQLx prepare, locked offline all-target check/Clippy, graft build, corrected
  focused+affected DB tests remain REQUIRED. No final migration change; apply/check
  retained schema and preparation sequentially. Full03.9 gates remain future work.

Resource state: task PostgreSQL cleanly STOPPED at handoff, cluster retained unchanged.
Restart MUST specify command-line settings (not persisted in config):
`pg_ctl -D /private/tmp/workflow-admission-pg-e3aa -l /private/tmp/workflow-admission-postgres.log -o '-p 55439 -k /private/tmp -c max_connections=200' -w start`.
Both DATABASE_URL/TEST_DATABASE_URL must explicitly use port55439 and workflow_admission.
An earlier start omitted options and briefly bound5432; restarted correctly before any
successful DB tests; only task cluster was touched. No running cargo/test/SQLx command.
Latest implementer124886/25840048.33%18:08:32Z, runtime usage/context capacity sources.
Early rotation now, before50%; no substantive reassignment to this subtree. Known graft
reported file-read estimate savings ~256029 tokens this worker (not measured usage savings).

### Poison verification correction checkpoint — STOPPED, still UNVERIFIED (2026-09-28 18:31Z)

Implementer `/root/poison_verify`, session01a0e938-ce6e-7723-aa52-0d310c0b6bcc;
reviewer `/root/poison_verify/poison_reviewer`, session01a0e939-10ec-77e2-8711-e95441628857.
Both Astra/medium; nested capacity established before edits. Preserve all existing work,
HEAD83bb29d and applied migrations through20260928174000; no stage/commit/reset/deploy.

Corrected the invalid wait fixture by separating two valid contracts: malformed RFC3339
fails before activation, leaves no frozen input, terminates and cannot be polled again;
a valid but expired event deadline is activated and can take the frozen final_error edge.
The stale wrong-kind assertion from the previous handoff passes unchanged.
Shutdown regression isolated PASS1/0 in2.68s, no timing limit raised:
`/private/tmp/workflow-03.9-poison-shutdown-repro.log`. It also passed in the affected
suite below. No assertion that the prior timeout was a preexisting flake is made.

Fresh independent review found P1: activation treated final_error's JSON-null non-result
as a successful dependency output, so legal default/exists references could hit schema
validation/Internal rollback forever. Corrected activation.rs outputs SQL: select the
latest completed activation in the lateral query, then exclude final_error OUTSIDE it.
This preserves absent failed output without exposing an older successful activation.
Expanded pending_io_tests recovery successor to use both default and exists, verify
`{fallback:"missing",present:false}`, successful completion and two unchanged empty polls.
No schema/migration change. Other code from previous workers preserved.

Verification (all tests stock2MiB `RUST_MIN_STACK=2097152`, `SQLX_OFFLINE=true`,
`cargo test --locked --offline --lib FILTER`, both DB URLs explicitly task55439):
- Before P1 correction, `admission_tests`:127PASS0fail38.76s,
  `/private/tmp/workflow-03.9-poison-verify-integration.log`.
- Before P1 correction, `workflow_poison_`:12PASS0fail3.40s,
  `/private/tmp/workflow-03.9-poison-verify-focused.log`.
- AFTER P1 correction, `workflow_poison_`:12PASS0fail3.47s,
  `/private/tmp/workflow-03.9-poison-verify-focused-corrected.log`.
- Independent actual-code correction review PASS, no remaining scoped findings,
  reviewer92578/25840035.83%18:20:52.208Z runtime usage/capacity sources.
  Source edits paused during both review rounds. Reviewer completed, no edits.
- Pre-P1 fmt/staged+unstaged diff checks passed; final reruns remain pending.

FINAL GATES NOT RUN: corrected affected `admission_tests`, retained migration check,
SQLx prepare, locked offline all-target check/Clippy, final fmt/diff/graft build.
Prepared exact sequential script `/private/tmp/workflow-03.9-poison-final-checks.sh`.
Its escalated invocation waited for approval, then returned `aborted by user after
563.8s`; no corrected integration/SQLx/check/Clippy logs exist. Do not claim those gates
passed or bypass approval by alternative invocation. Root requested checkpoint/stop.
The earlier127PASS is useful unaffected evidence but is BEFORE the SQL correction.

Resources: retained task PG restarted correctly with explicit port55439/socket/private/tmp/
max_connections200; both URLs postgres://mac03@127.0.0.1:55439/workflow_admission.
No stop or reset issued by this worker. Last PostgreSQL log checkpoint18:26:42Z proves
it running then. Sandbox read-only pg_ctl status18:31Z said no server, but postmaster.pid
remains and sandbox ps was denied, so current PG state is UNCONFIRMED/likely RUNNING;
root owns cleanup/approval now. No final-check script start evidence; no worker jobs
intentionally left running. Root observed no build/test/SQLx processes.

Implementer final sample101720/25840039.37%18:31:22.987Z,
`token_usage_record.usage`/`task_started.model_context_window` sources.
Reviewer latest stored sample92982/25840035.98%18:21:00.350Z,
`token_count.info.last_token_usage`/`token_count.info.model_context_window` sources.
No additional scope begun; root acceptance still prohibited by pending gates.

### Poison final verification continuation — 2026-09-28

Implementer `/root/verify_recovery`, session01a0e94e-d453-7740-8ea1-0415804b904a;
sole nested reviewer `/root/verify_recovery/reviewer`,
session01a0e94f-2b7e-72f1-bda4-9626885d42c4, both Astra/medium. Startup verified
implementer23971/2584009.28%18:37:43Z; reviewer32724/25840012.66%18:38:13Z,
runtime usage/context sources. Existing whole-scope expansion reconciled unchanged;
this assignment completes only poison verification before root acceptance.

Explicit retained taskPG restart succeeded with port55439/socket/private/tmp/
max_connections200. Read and executed `/private/tmp/workflow-03.9-poison-final-checks.sh`
with normal escalation. First corrected integration125PASS2FAIL36.51s exposed
NULL-route completed fixtures: SQL `<> 'final_error'` also rejected legitimate NULL
routes permitted by the existing completion schema. Preserved failure log at
`/private/tmp/workflow-03.9-poison-verify-integration-null-route-failure.log`.
Changed only activation.rs188 predicate to `IS DISTINCT FROM 'final_error'`, still
outside latest-completed selection so older successful activations cannot leak.
Independent actual-code correction PASS; no remaining scoped findings. No fixture
or bound was weakened. Full integration and sequential final gates rerunning.
Applied migrations through20260928174000 remain immutable; no stage/commit/reset/deploy.

Final sequential script exited0 after NULL-safe correction. Both DB URLs explicitly
`postgres://mac03@127.0.0.1:55439/workflow_admission`; stock2MiB tests, no skip flag.
Exact commands are preserved in `/private/tmp/workflow-03.9-poison-final-checks.sh`.
Log prefix below is `/private/tmp/workflow-03.9-poison-verify-`:
- `RUST_MIN_STACK=2097152 SQLX_OFFLINE=true cargo test --locked --offline --lib admission_tests`:
  127PASS0fail35.54s, including all12 poison tests and both formerly failing
  activation tests; `integration-corrected.log`. Compile31.86s.
- `cargo sqlx migrate run`:PASS, existing migrations current; `migrate.log`.
- `cargo sqlx prepare -- --all-targets`:PASS19.63s; `sqlx.log`. No `.sqlx` diff;
  runtime query correctness is verified by actual DB tests, not generated metadata.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS1.01s; `check.log`.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`:
  PASS50.92s; `clippy.log`.
- `cargo fmt --all -- --check`, `git diff --check`, `git diff --cached --check`,
  `graft build`:PASS; `fmt.log`, `diff.log`, `diff-staged.log`, `graft.log`.
Final independent evidence review pending. No further source changes after correction
review. Later03.9 maintenance/controls/root budgets/fairness and full03 acceptance
remain outstanding. Root owns acceptance and next assignment.

Final independent actual-code/evidence review PASS, no remaining scoped findings,
reviewer session above,93339/25840036.12%18:43:56.538Z runtime usage/context sources;
owner-verified final transcript sample94802/25840036.69%18:44:11.736Z,
token_count.info.last_token_usage/model_context_window. Implementer boundary
74553/25840028.85%18:44:29.339Z, runtime usage/context sources. Source unchanged
after correction review; bounded poison fragment ready for root acceptance.
TaskPG STOPPED cleanly with escalated pg_ctl, cluster retained. Sequential script
exited0 and no build/test/SQLx job remains. No later fragment started.

### Next maintenance fragment — reconciled scope, no implementation yet

Root accepted bounded maintenance scope after poison acceptance. Controls, budgets,
fairness and full phase gates remain separate. Existing polling excludes overdue
queued/running work entirely; waiting reconciliation without a workflow_waits row
is also undiscoverable. Deadline retirement must cover every active run state and
waiting reason, using existing execution/job identities, without scheduling retries
or successors or erasing effect ambiguity, frozen results, notification intents or
receipts. Close runnable jobs, current attempts and open waits atomically under a
run-first lock; retain consumed attempt counts. Terminal deadline failure needs a
durable audit reason and must remain compatible with later effect reconciliation.

Concrete seams read via graft/callers:
- `polling.rs15–61,83–99`: add deadline candidate classification before wait/job
  classification, including waiting reconciliation and future-run_at jobs. Cursor
  remains existing job UUID; whole poison pages must drain at unchanged time.
  Every active admitted run has existing execution/job identity; confirm candidate
  selection cannot miss an incomplete reconciliation execution with failed job.
- `application/workflow/polling.rs11–16,54–60` and `worker.rs87–126`: narrow required
  expiry transition method plus `PollWork::ExpiredRun` dispatch. Only production
  WorkflowPolling impl found by exhaustive graft grep; no silent default method.
- `lease.rs99–147`: current lock_scope rejects expired deadline and waiting runs,
  so it cannot authorize deadline cleanup. New maintenance transition must lock run
  first and recheck deadline and active state; then owned executions/jobs/attempts/
  waits, with bounded transaction/lock timeout. Existing lease expiry/spent cleanup
  remains applicable for predeadline work, including exhausted allowance.
- `recovery.rs105–157` and immutable migrations20260928173500/174000: current
  attempt retirement discriminator allows only live/expired lease. Administrative
  run-deadline retirement needs additive fence support keyed to deadline and exact
  attempt generation/worker/debit. It must remain valid if lease expiry crosses
  commit, while never granting retries after deadline or letting stale workers
  classify their writes as administrative cleanup. Keep deferred guard protection.
- `waits.rs60–87,288–295` + `wait_commit.rs5–53,113–126`: existing bounded wait
  sweep preserves timer/event semantics; reuse it or route common run expiration
  consistently. Wait expiration currently only fails waiting run; complete expiry
  must cover run/job/attempt facts consistently, retaining immutable notification.
- `lease_tests.rs1–50`: isolated AdmissionFixture/source helpers and full snapshot;
  new sibling maintenance tests can reuse them. Existing polling integration tests
  exercise reusable worker. Tests need all active/waiting states, future due job,
  live/expired/exhausted ownership, competing sweep/completion/heartbeat, commit
  fault rollback, repeated/full-page polls, no replenishment and retained ambiguity.

No maintenance code/schema/fixtures changed or taskPG restarted. Early worker rotation
recommended at97490/25840037.73%18:46:16Z (prior verification context dominates);
reviewer already36.69%. Source is the accepted poison version. New worker should
finish concrete design, implement, test and independently review this scope; do not
mistake this seam note for implementation evidence. Applied migrations remain immutable.

### Maintenance concrete design — 2026-09-28 18:54Z

Implementer `/root/deadline_maintenance`, UUID01a0e95c-9d61-7c10-b402-84357930b8a2;
sole nested reviewer `/root/deadline_maintenance/deadline_review`,
UUID01a0e95c-efb0-7af1-b10d-2d67a68b2134, Astra/medium, capacity confirmed before edits.
Startup9.28%, design28.30% (73131/258400)18:54:24Z; runtime usage/context sources.
Read original03 and reconciled prior seam handoff, root accepted poison remains intact.

Implement narrow required WorkflowPolling::expire_run(scope), ExpiredRun discovery before
job/wait classification, worker dispatch. Transaction locks scoped run first, validates
active state and deadline after lock, validates existing execution/job association, locks
owned executions/jobs in deterministic order, retires processing attempts with exact
current generation/worker/attempt and one debit, fails pending jobs without extra debit,
expires open waits, marks failed terminal run and appends run_deadline_expired audit.
Never modifies execution input/output/routes, prior attempt truth, notification intents,
receipts, or schedules work. Conservative unknown safety on in-flight attempts preserves
effect ambiguity without loading potentially malformed bundles. Existing failed-attempt
reconciliation truth remains untouched. Wait resume delegates run-deadline expiry into
this same transaction so a competing wait sweeper cannot leave incomplete cleanup.
Additive migration extends retirement discriminator with deadline, preserving current
live/expired guard and deferred checks: overdue failed run, failed job, exact debit/fence,
terminal failure code and no successor. Lease crossing during deadline cleanup is valid.
All statements/transaction remain timed and polling page remains1..128 job UUID cursor.
Checks: all active states/wait reasons, live+expired+spent ownership, competing expiry/
completion/heartbeat, wrong scope/predeadline no-op, commit fault rollback, retained truth,
full pages drain without time advance; stock2MiB affected DB, migration/SQLx/static gates
and actual-code independent review. Controls/budgets/fairness/full03 remain excluded.

### Maintenance partial implementation — STOPPED, UNVERIFIED 2026-09-28 19:05Z

Root requested checkpoint/stop after interrupted sandbox escalation wait; no acceptance.
Changes since accepted poison: application polling adds required expire_run and ExpiredRun;
worker dispatch handles it; persistence polling prioritizes overdue queued/running/waiting
runs including failed reconciliation jobs/future pending jobs; new maintenance.rs implements
run-first scoped expiry and owned attempt/job/wait closure with durable audit; wait_commit
calls the shared expiry transaction before ordinary wait handling. No frozen execution
facts or prior failure/effect truth are changed. Mod registration added. New additive
`migrations/20260928190000_workflow_deadline_maintenance.sql` extends attempt discriminator
and deferred retirement guard. NOT APPLIED. Earlier applied migrations untouched.

Production edits currently compile: `SQLX_OFFLINE=true cargo check --locked --offline
--all-targets` PASS19.06s, `/private/tmp/workflow-03.9-maintenance-check.log`, unified
session17932 completed. `cargo fmt --all` completed before that check. No tests authored,
no migration applied, no SQLx regeneration, Clippy, final diff/fmt or independent review.
All planned maintenance acceptance tests remain REQUIRED, followed by affected admission
suite at stock2MiB and migration/SQLx/static/evidence review. The compile pass proves no
runtime SQL or deferred guard correctness. Need check overdue discovery completeness,
all waiting reasons, run-first races with heartbeat/completion/wait sweeper, unchanged
attempt budget/frozen results/effect uncertainty, transaction fault rollback, complete
poll pages and deferred deadline retirement across lease expiry. Schema guards and shared
wait integration require actual-code independent review; reviewer has only startup task.

PG default startup18:56:21Z failed shared-memory shmget Operation not permitted and logged
shutdown. Required normal escalation of the exact retained-cluster explicit port55439
startup waited475.8s then returned `aborted by user`; no rejection reason was returned and
no alternate invocation attempted. Read-only pg_ctl status19:04Z says no server; latest
PostgreSQL log is the failed18:56:21Z startup then shutdown, with no successful restart.
Cluster retained at prior path. No new running DB/test/SQLx resource; build above finished.
Restart must use explicit port/socket/max_connections from RESUME, via required approval;
do not bypass the unresolved escalation. Root owns resource/RESUME updates now.

Implementer fresh79579/25840030.80%19:04:50.655Z, runtime usage/context sources;
UUID and reviewer identities in design section. Nested reviewer idle with no review or
resources started. Stop was root-requested approval checkpoint, not context threshold.
No stage/commit/reset/deploy. Next worker should preserve this partial implementation,
finish tests/design corrections and review; do not treat maintenance as verified.

### Maintenance verification continuation — 2026-09-28 19:38Z

Implementer `/root/maintenance`, verified session01a0e97a-748e-7ba2-b313-a76405ab7a4b;
sole nested reviewer `/root/maintenance/reviewer`, session01a0e97a-b06c-7323-ad53-eaaef5d552d5,
both Astra/medium. Startup implementer21855/2584008.46%19:25:21Z; reviewer21465/
2584008.31%19:25:40Z, runtime usage/context sources. Capacity confirmed before edits.
Original03 and whole03.9 expansion reconciled unchanged; this assignment is ONLY overdue
maintenance. Controls/root budgets/fairness/full03 acceptance remain pending.

Retained taskPG started normally under current Full Access/never with explicit
`-h 127.0.0.1 -p 55439 -k /private/tmp -c max_connections=200`. Actual migration table
confirmed174000 latest before work. Migration190000 applied successfully25.90ms and
is now IMMUTABLE; no applied migration changed. Both DATABASE_URL/TEST_DATABASE_URL
explicitly use postgres://mac03@127.0.0.1:55439/workflow_admission throughout.
No stage/commit/reset/deploy; all preexisting changes preserved.

Added maintenance_tests.rs, maintenance_guard_tests.rs and maintenance_worker_tests.rs,
nested under existing isolated lease/polling test modules.12 real DB tests cover:
- queued/running/all6 waiting reasons, future run_at discovery and scoped/predeadline refusal;
- live/expired/spent exact current attempt retirement, one debit, stale result refusal,
  frozen execution and prior ambiguous-attempt truth retention;
- competing expiry/completion/heartbeat and timer/event wait sweepers; immutable wait facts;
- deferred commit-fault rollback, lease crossing at commit, full persistence/worker pages,
  unsupported-handler work with zero calls/attempts, unchanged-time repeat polling;
- deterministic wait deadline crossing while blocked on execution lock;
- predeadline/wrong generation/worker/attempt/debit/retry/successor forgery rollback,
  explicitly forcing the retirement guard and asserting23514 plus its exact message.

Independent actual-code review found a late wait-path deadline crossing could call the
older wait-only expiry. Fixed wait_commit.rs to invoke shared run expiry after its later
clock check too. Review also required real-worker and adversarial SQL coverage above.
A second review found successor forgery hit the old progression constraint first; fixture
now creates a valid next activation/route, requires all setup statements succeed, and
forces ONLY workflow_fenced_retirement_guard. No production guard/schema change needed.
All findings resolved; actual-code PASS19:38:16Z reviewer87320/25840033.79%, runtime
usage/context sources. Source edits paused during every inspection.

Exact sequential commands: `/private/tmp/workflow-03.9-maintenance-final-checks.sh`;
final test-SQL refresh (same gates excluding unchanged139-test integration):
`/private/tmp/workflow-03.9-maintenance-refresh-checks.sh`.
All tests `SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib FILTER`,
no skip flag. Logs share `/private/tmp/workflow-03.9-maintenance-` prefix:
- `workflow_maintenance_`: initial9PASS5.36s, `focused.log`; corrected12PASS6.41s,
  `focused-corrected.log`; final exact-guard12PASS6.94s, `focused-final.log` (compile38.15s).
- `admission_tests`:139PASS0fail51.69s, `integration.log`, AFTER final production fix
  and before only the final negative-test fixture strengthening. Unaffected evidence reused.
- Migration run/info/schema inspection PASS: `migrate.log`, `migrate-info.log`, `schema.log`.
- Initial SQLx prepare23.69s, offline all-target check0.92s, Clippy31.25s,
  fmt/staged+unstaged diff/graft PASS. Final test-SQL refreshed gates pending below.

Final refresh script exited0: migration current; SQLx prepare PASS16.76s (`sqlx.log`),
locked offline all-target check PASS0.94s (`check-final.log`), Clippy with `-D warnings`
PASS23.03s (`clippy.log`), fmt/bothdiff/graft PASS (`fmt.log`, `diff.log`,
`diff-staged.log`, `graft.log`). No `.sqlx` changes; runtime queries covered by real DB
checks above. No source edits after final actual-code review; evidence review pending.
TaskPG remains RUNNING for root-authorized transfer to fresh controls implementer;
retained data/socket/log identity unchanged, migrations through190000 immutable.
All test/build/SQLx commands completed; no job left running. Later fragments not begun.

Final independent actual-code/evidence review PASS19:40:21Z, all findings resolved,
reviewer92462/25840035.78% runtime usage/context sources; owner-verified final transcript
92871/25840035.94%19:40:29Z token_count.info sources. Reviewer is quiescent, no resources.
Implementer boundary109041/25840042.20%19:40:29Z token_count.info sources. Returning
maintenance-only checkpoint for root acceptance and fresh-worker controls handoff.
Process check confirms no live Cargo/test/rustc/Clippy job. TaskPG retained RUNNING
PID92442 with explicit127.0.0.1:55439/socket/private/tmp/max_connections200. No cleanup
or reset needed. Graft reported savings estimate2852643 tokens this worker (includes
large whole-repo map baseline; not measured model usage savings).

### Controls reconciliation handoff — 2026-09-28 19:46Z, NO source edits

Root accepted next bounded controls scope. Implementer `/root/controls`, verified
session01a0e989-7b83-76a0-b544-71a655c56550, Astra/medium; nested capacity confirmed
before edits with `/root/controls/review_controls`, session01a0e989-bad4-7f83-9b7b-3721fa4f60fd,
Astra/medium. Reviewer read skill only and is idle; no code review performed.
Startup implementer21934/2584008.49%19:41:44Z; latest93533/25840036.20%19:44:57Z
usage/runtime sources. Reviewer startup21418/2584008.29%19:42:01Z, latest owner helper
21964/2584008.50%19:42:12Z token_count.info sources. Root authorized early rotation
after discovery, before source edits, to leave a cohesive implementation window.
Only this handoff appended; no migrations, source, tests, PG or existing changes modified.

**Accepted concrete scope.** Extend the existing WorkflowRunTransitions owner with
company-scoped current-authorized cancel and safe retry. Both commands carry caller's
expected revision, stable bounded command key, authenticated actor and run scope.
Exact replay returns saved result only after current company and stored association
authorization; conflicting actor/run/kind/expected revision under a key refuses.
Terminal cancellation preserves original terminal outcome. Success is one atomic
run/audit/control-receipt/job/current-attempt/open-wait transition. No changes to
frozen inputs/outputs, prior attempt/effect evidence, notifications or external receipts.
Heartbeat refusal must drop the actual handler future (existing supervision mechanism),
verified through a real DB control and running worker/handler test.

Safe retry reopens ONLY the failed terminal execution's existing failed job with
incomplete frozen activation and durable last-attempt retry-safety evidence. Retain
execution/job/action identities, prior attempts, original deadline and all consumed
budgets. Require remaining existing attempt allowance, no completed result/successor,
and no exhausted activation/deadline/structural failure. Never manufacture an attempt
or replenish limits. An explicit retry may recover a safe terminal handler failure
with unused attempts; automatic retry remains governed by existing recovery policy.
Unknown-effect/reconciliation outcomes refuse. Root accepted refusing child retry
until phase07 can reconcile previously published immutable parent terminal wakeups;
test refusal preserves those facts. Cancellation of children remains supported.
No production effects/agent/startup/UI or04+ work; budgets/fairness/full03 remain later.

**Lock hierarchy explicitly accepted by root.** `authority::authorize_company`
locks company FOR UPDATE and membership/principal FOR SHARE before admission may lock
a parent run. Controls MUST reuse that authorization order before run lock, otherwise
run->company versus child admission company->parent run forms a cycle. After the
existing company authority lock, lock the target run first, then owned executions,
jobs/current attempts/waits/audit; current stored association authorization follows
run acquisition. This is existing authority serialization, not a new scheduler lock.
Reviewer must trace company/association/admission/control paths and a real competing
authority/admission-control test must prove no deadlock or stale authorization.

**Exact discovered seams (graph-first/callers traced; avoid whole files).**
- application/workflow/contracts.rs198–241: RunRevision, full RunHead, current
  CancelCommand and CancelWorkflowRequest lack command identity; request also lacks
  expected revision. Existing CancelResult separates applied/terminal/conflict/absence.
- service.rs76–112: cancel checks company, reads scoped head, checks association,
  then auto-uses head.revision. Replace auto revision with caller revision and forward
  actor/key; preserve errors/mismatched-scope checks. Add retry at same service seam.
- ports.rs63–74: existing required WorkflowRunTransitions.cancel; add required retry
  (no default). ports.rs89–95 WorkflowInspection.head exists but has NO production
  implementation. Only MemoryStore and NoRuntime implement either port today.
- application/workflow/tests.rs374–423 holds MemoryStore inspection/cancel;
  tests/state_cases.rs210–271 direct CancelCommand and other request tests in
  tests/{cases,state_cases,authorization_cases/{cancellation,membership}}.
  `graft grep CancelCommand` / `graft grep CancelWorkflowRequest` found exhaustive uses.
  persistence/workflow/admission_tests.rs31–42 NoRuntime must explicitly implement
  new port method. Avoid a second cancellation API or leaving old semantics reachable.
- authorization.rs64–70 operation enum needs retry;146–188 LifecycleAuthorizer uses
  real principal context/company management and channel/thread visibility.
- persistence/workflow/authority.rs6–32 existing atomic company manager authority;
  association.rs6–61 real tenant/channel access mode/grants/thread authority. Reuse.
- workflow/mod.rs1–9 documented runtime lock rules and35–43 shared imports; concrete
  owner is PostgresPersistence (not a new PostgresWorkflowPersistence type).
- admission_binding.rs7ff SourceKey versioned encoding is current trigger owner.
  Production head must reconstruct original RunCausality from durable source_key,
  run.trigger_id/correlation_id and parent execution identity; do not fabricate Manual
  for message/schedule/child. Domain causality.rs98–171 checked constructors.
  SourceKey shape manual/message length2, schedule length3, child length5;
  activation.rs213–228 currently parses only parent run (not full causality).
- migrations20260928070000 workflow_runs schema has NO revision. Add monotonic
  DB-generated revision and overflow guard, covering every material run transition
  (including existing writers), plus tenant-scoped immutable control receipt table
  in NEW migration. Existing run-state update paths need revision semantics audited.
  Original expected-revision CAS behavior remains, not silently upgraded to latest.
- maintenance.rs5–54 closes pending/current jobs/attempts/waits atomically after
  run-first locking; useful closure seam but cancellation needs distinct retirement
  discriminator and guard, preserving deadline behavior. Current-attempt match uses
  task_id/retry_count+1/execution_generation/worker_id and debits exactly once.
- recovery.rs21–68/105–157 durable failure safety/classification, exact attempt
  closure and run/job disposition. Failed run sets terminal_execution_id and leaves
  incomplete frozen execution for future retry. Last attempt's workflow_retry_safety
  is mandatory evidence, status alone never suffices.
- migration20260928190000 is APPLIED IMMUTABLE and latest retirement function; new
  follow-up must preserve exact live/expired/deadline guard behavior while adding
  cancellation. Cancellation must work even if lease crosses expiry during commit.
  Guard no forged generation/worker/attempt/debit/retry/successor.
- migrations20260928130000 and133000 freeze activation/output/route and scoped
  successor identity. Retry cannot erase them.150500 generates child terminal
  wakeup sequences and immutable terminal facts; preserve all prior wakeups.
- lease_tests.rs12–35 fixture()/fixture_source() create isolated AdmissionFixture
  and ActivationRequest;36–50 snapshot/expire/due. Nested tests imported at375ff.
  recovery_tests.rs5–33 failure()/claim()/source() helpers provide real safe terminal
  fixture; maintenance tests/worker tests are examples for deferred guards and
  supervised live handlers. Add controls tests as cohesive sibling modules.

**Required checks.** Real competing cancel/cancel, retry/retry and cancel versus
claim/completion/failure/wait resumption; one result/receipt/closed attempt, no stale
successor. Replay after lost acknowledgement, payload/key conflicts, stale revisions,
foreign scope/current revoked membership and allowlist access, company authority race.
Deferred audit/receipt/retirement fault proves rollback of all writes. Cancel preserves
existing completion/effect/notification/wait-event facts and drops live actual future.
Retry successful with frozen identities/debit unchanged and refuses ambiguous/exhausted/
deadline/completed/child cases. Revision progression/overflow and SQL guards have direct
negative tests. DB tests must run stock2MiB, no skip flag, isolated fixtures as required.
Sequential migration run/info/schema inspection -> SQLx prepare -> focused tests +
affected admission_tests -> offline all-target check/Clippy/fmt/bothdiff/graft, followed
by independent actual-code and evidence review; stop source edits during review.
Exact command forms are in skill references/postgres.md and preceding maintenance
final-check scripts; use NEW controls log prefix. No control checks run yet.

TaskPG still retained RUNNING PID92442, /private/tmp/workflow-admission-pg-e3aa,
log/private/tmp/workflow-admission-postgres.log, localhost55439 socket/private/tmp,
max_connections200. BOTH URLs postgres://mac03@127.0.0.1:55439/workflow_admission.
All migrations through20260928190000 immutable; no stage/commit/reset/deploy.
Root owns PROGRESS/RESUME; next worker owns BRIEF/code/taskPG.

### Controls implementation checkpoint — 2026-09-28 20:04Z, NOT VERIFIED

Root authorized bounded stop after initial controls implementation, focused tests and
narrow review, leaving acceptance completion to a fresh worker before50% context.
Implementer `/root/implement_controls`, session01a0e98f-2cfe-7e02-ad1d-b235237e25f3,
Astra/medium; reviewer `/root/implement_controls/review_controls`,
session01a0e98f-6f90-7500-8fc8-c20f4421c465, Astra/medium. Latest implementer
116676/25840045.15%20:03:30Z usage/runtime sources. Reviewer correction review
37.87%20:03:52Z. Reviewer actual-code review and correction review performed;
full controls acceptance explicitly NOT passed, see remaining gates below.

**Implemented source.** application/workflow/{contracts,ports,service,authorization}.rs
now carry actor, bounded IdempotencyKey and caller expected RunRevision on cancel;
required retry port/service plus RetryResult added, no default. Existing MemoryStore
and NoRuntime explicitly implement retry; application cancellation fixture literals
supply explicit revisions (state race fixtures use observed revised value where needed).
MemoryStore has no durable retry evidence and explicitly errors on retry.
Production adapters added inspection.rs (full original manual/message/schedule/child
causality from durable source key), controls.rs (existing PostgresPersistence owner,
current company+association authority before receipt replay, expected CAS, run/owned
locks, atomic cancel/retry/audit/receipt), control_retry.rs (safe failed execution/job
reopen), control_tests.rs + control_guard_tests.rs. mod.rs and lease_tests.rs wire them.
maintenance.rs shares exact current-attempt/job retirement with typed Deadline/Cancel
cause; earlier deadline semantics otherwise preserved. admission_tests.rs NoRuntime
updated. No commit/stage/reset/deployment performed; all prior changes preserved.

**APPLIED IMMUTABLE new migrations.**
- 20260928200000_workflow_controls.sql: database run revision, material execution/job/
  wait revision triggers (timestamp/heartbeat-only updates excluded), immutable control
  command receipts/audit FK, cancelled wait state, cancellation exact-attempt retirement
  guard preserving existing live/expired/deadline checks, shared safe-retry SQL predicate
  plus deferred failed->pending guard. Safe retry preserves job/execution/frozen facts,
  attempt debit/allowance and deadline, refuses unknown/structural/exhausted/completed/
  child work. Child retry remains refused until phase07 immutable wakeup reconciliation.
- 20260928201000_workflow_control_receipt_deletion.sql: UPDATE and DELETE receipts
  protected while owning run/company exists; actual owner cascade cleanup is allowed.
All migrations through these are immutable. Any correction requires a NEW migration.

**Reviewed corrections and evidence.** Initial reviewer found fresh child creation after
parent cancellation and unguarded receipt DELETE. admission.rs now checks cancelled
parent only AFTER admission_replay::resolve returns no result; source authorization
already holds parent lock, so existing child replay remains allowed. Additive201000
fixes receipt DELETE. Narrow actual-code correction review passed, tests still required.
Initial focused suite6/7 passed, real cancel/completion race consistently timed out.
This exposed a real company/run FK cycle: control held companies FOR UPDATE then waited
run; completion held run then UPDATE workflow_runs triggered companies FOR KEY SHARE.
Server log/private/tmp/workflow-admission-postgres.log54053–54056 and mutual blocker
capture/private/tmp/workflow-controls-locks.log establish it; repeat log
/private/tmp/workflow-controls-race-diagnosis.log. Root accepted reviewer-traced fix:
authority.rs shared company lock FOR NO KEY UPDATE, still company-before-run and unchanged
member/principal FOR SHARE. It conflicts with other authority transactions, company owner
updates/deletes but allows FK KEY SHARE. Reviewer traced admission/lifecycle/configuration
consumers and member mutation/deletion owners. Re-run affected admission/lifecycle tests.
No timeout or stack bound raised. This lock mode refines the previous accepted handoff.

**Evidence so far (logs in /private/tmp).**
- workflow-controls-check.log: initial offline locked all-target check PASS44.47s before tests.
- workflow-controls-migrate.log / migrate-followup.log:200000/201000 applied successfully.
- workflow-controls-migrate-info.log / schema.log: migration/schema inspected; final info
  workflow-controls-migrate-info-final.log after201000.
- workflow-controls-sqlx-prepare.log: SQLx prepare all-target PASS23.14s after correcting
  one test handler-result type; workflow-controls-sqlx-followup.log PASS18.29s final edits.
- workflow-controls-focused.log: stock2MiB6pass1fail (proven cycle above), not passing evidence.
- workflow-controls-focused-followup.log: final10 focused rerun in progress at checkpoint;
  final result appended below. Explicit BOTH URLs taskPG and SQLX_OFFLINE=true,
  RUST_MIN_STACK=2097152, cargo test --locked --offline --lib control_tests.
- fmt run; both diff whitespace checks pass; graph refreshed to workflow-controls-graft.log.
No final Clippy, SQLx prepare --check, final offline all-target, affected broad tests yet.

**Acceptance still REQUIRED, do not infer from focused successes.**
1. Competing cancel versus claim, classified failure and wait resumption; deterministic
   admission-first/existing-child replay AND cancellation-first/fresh-child rejection.
   Current child contender test returns both results correctly but covers only whichever
   scheduling order occurs. Real company owner update/delete contention test added;
   membership/principal and allowlist revocation contract need full evidence.
2. Foreign scope, revoked allowlist access, conflicting actor/run command keys (expected
   revision and kind/key conflict already focused); exact replay after lost ack authorized
   against current authority (focused membership deletion contender already added).
3. Deferred audit/receipt/retirement fault tests proving every write rollback. Current
   deferred receipt fault asserts runs/executions/jobs/attempts+receipts only, snapshot
   excludes audit events and waits despite test name. Extend complete state assertion.
   Direct owner-cascade receipt cleanup test also missing.
4. Revision across execution/job/wait transitions, heartbeat/noop stability, overflow;
   direct adversarial retry/cancel retirement guards (forged generation/worker/attempt/
   debit/retry/successor), cancellation crossing lease expiry. Current expiry cancellation
   positive test and revision-after-claim/stale/direct-decrement test pass initial suite.
5. Preserve existing wait-event/notification/child terminal facts; child retry refusal
   test checks events unchanged. Real message/schedule head reconstruction tests absent
   (manual+child covered). No new production effect claim is authorized.
6. Rerun final focused+affected admission_tests and application workflow tests stock2MiB;
   broader relevant lifecycle/authority tests because shared helper changed. Sequential
   migration info/schema -> SQLx prepare/--check -> offline all-target/Clippy/fmt/bothdiff/
   graft gates and independent actual-code/evidence review of all acceptance remain.
   Root controls budgets/fairness/full03/later phases; do not enter them.

TaskPG retained RUNNING PID92442 /private/tmp/workflow-admission-pg-e3aa;
log/private/tmp/workflow-admission-postgres.log host127.0.0.1 port55439 socket/private/tmp
max_connections200. BOTH URLs postgres://mac03@127.0.0.1:55439/workflow_admission.
Root owns PROGRESS/RESUME. Next implementer owns this BRIEF/code/taskPG; do not reset.

**Checkpoint final focused result20:05Z:** workflow-controls-focused-followup.log
PASS10/10, no skips, stock2MiB; compile42.10s, tests4.51s. New deterministic
company-lock/runtime-FK+owner-mutation test and all three new guard tests passed;
previously failing cancellation/completion competitor now passes. SQLx final
prepare18.29s passed; migration info final and graph build and bothdiff checks passed.
Narrow source correction review PASS; reviewer explicitly retains all broader test
limitations above. Implementer121011/25840046.83%20:05:42Z usage/runtime;
reviewer101333/25840039.22%20:04:09Z token_count.info sources. Both workers quiesced
for root-authorized early rotation; no further scope started. All taskPG resources
retained running. No build/test command remains running after focused test completion.

### Controls remaining acceptance reconciliation — 2026-09-28 20:17Z

Implementer /root/verify_controls UUID01a0e9a8-cfe7-70c3-bb2b-7e6cdbc11b52;
reviewer /root/verify_controls/review_controls UUID01a0e9a8-fe2c-76f3-a78c-2d247b086624.
Both Astra/medium, nested capacity confirmed. Startup19,202/258,4007.43%; reviewer
21,477/258,4008.31%, usage/runtime sources. Original03 and all controls handoffs read.
All six remaining groups reconciled before source edits; prior10 tests are supporting
coverage only. Preserve company-before-run FOR NO KEY UPDATE authority and current
principal/member locks; migrations through201000 immutable; child retry refused until07.

1. Add isolated real DB cancel/claim, cancel/classified-failure, cancel/wait-resume
   competitors. Assert winning disposition, exact attempt debit and absence of duplicate
   successors. Deterministic child admission-before-cancel replay and cancel-before-new
   admission rejection complement existing child race.
2. Exercise foreign company/run, conflicting authorized actor/run command identities,
   revoked allowlist and principal/member authority on saved receipt replay. Reuse actual
   admission association fixtures and live authority locks, never mock authorization.
3. Complete ordered snapshots of runs/executions/jobs/attempts/waits/events/audit/receipts
   at deferred audit, receipt and retirement commit faults; test owner cascade deletes
   receipts while direct receipt mutation/deletion refuses.
4. Verify execution/job/wait material revision advances, noop/heartbeat stability and
   bigint overflow rollback. Adversarial cancel retirement covers generation/worker/
   attempt/debit/retry/successor and cancellation crossing expiry. Direct unsafe retry
   guard is forced independently of unrelated constraints and preserves snapshot.
5. Preserve committed result, notification intent, wait event and child terminal facts;
   real message and schedule admissions must reconstruct exact causal RunHead. No new
   effect implementation or root budget scope. Retry identity/remaining allowance remains
   covered by existing real competing retry test plus affected suites.
6. Sequential migrations/schema -> SQLx prepare/check -> focused controls and affected
   admission/application/lifecycle/authority suites stock2MiB -> locked offline all-target
   check/Clippy/fmt/staged+unstaged whitespace/graft; independent actual-code/callers and
   evidence review against entire controls scope. Final03 full-suite/budgets/fairness
   remain root-owned later gates. If capacity rotates, preserve exact remaining matrix.

Implementation confined to cohesive control test siblings and only production fixes
revealed by those checks/review. No new unresolved contract or approval gate.

### Controls acceptance implementation and review — 2026-09-28 20:29Z

New test-only files control_acceptance_tests.rs/control_revision_tests.rs linked below
control_tests; new workflow_control_ cases appended to admission_authority_tests and
admission_source_tests reuse their real fixtures. No production/migration edits by this
worker. Full scoped current-authority/causality checks, real cancellation competitors,
complete ordered rollback snapshots, immutable receipt cascade and exact deferred guard
negative tests now implemented against task-owned isolated databases.

Independent actual-code/callers review of ENTIRE controls scope returned no production
finding20:27Z. Two concrete findings corrected: missing company_members.id fixture and
foreign-scope case initially used nonexistent UUIDs. It now creates a real second company
and admitted run in the SAME database; tests both mismatched scopes under authorized
actor and correct foreign scope under unauthorized actor, complete snapshot unchanged.
Reviewer correction/evidence review pending; source edits paused during each review.

First22 focused run19PASS3FAIL8.40s; all failures independently classified as test fixture/
assertion defects: cancellation stop_reason was unsupported literal (nowNULL as writer),
child fresh key reused same canonical source (legitimate alias replay; separate fixture
now proves cancellation-first), overflow returns PostgreSQL22003 bigint out-of-range
before explicit23514 guard (both refusal paths require complete rollback). Next26 focused
run25PASS1FAIL13.43s only omitted member id; now corrected. Neither is acceptance PASS.
Logs /private/tmp/workflow-controls-acceptance-focused{,2,3}.log preserve compile/fixture
failures and diagnostics. SQLx prepare initial18.25s, final17.30s, corrected16.51s PASS:
workflow-controls-acceptance-sqlx{,-final,-corrected}.log. Initial compile test had two
extra .unwrap() calls on activate result, corrected before runtime26 tests.

TaskPG restarted normally with retained data at127.0.0.1:55439, socket/private/tmp,
max_connections200, data/private/tmp/workflow-admission-pg-e3aa and existing PG log.
BOTH DATABASE_URL and TEST_DATABASE_URL explicitly
postgres://mac03@127.0.0.1:55439/workflow_admission for all DB commands. No reset.
Migration run/info and psql\d workflow_control_commands/workflow_runs/workflow_waits
PASS, logs workflow-controls-acceptance-{migrate,migrate-info,schema}.log in/private/tmp.
No migrations added; applied200000/201000 remain immutable.

Exact final sequential commands are in
/private/tmp/workflow-controls-acceptance-final-checks.sh (running at this checkpoint):
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_control_;
same all workflow:: filter covers domain/application/admission/lifecycle/authority;
cargo sqlx prepare --check -- --all-targets; cargo check --locked --offline --all-targets;
cargo clippy --locked --offline --all-targets -- -D warnings; cargo fmt --all -- --check;
git diff --check; git diff --cached --check; graft build. Separate logs share
/private/tmp/workflow-controls-acceptance- prefix: focused-final,integration,sqlx-check,
check,clippy,fmt,diff,diff-staged,graft.log. No skip flag or raised stack/timeout/bound.

Latest parent sample108565/25840042.01%20:27:31Z usage/runtime sources; reviewer
99155/25840038.37%20:27:18Z. Root owns acceptance/PROGRESS/RESUME; budgets/fairness
and final03 full-library acceptance remain OUTSIDE controls assignment.

### Controls verification handoff — 2026-09-28 20:32Z, NOT FULLY VERIFIED

Root-authorized context rotation, worker125432/25840048.54%20:32:09Z,
usage/runtime sources. No substantive work after this boundary. Source unchanged after
correction review. Reviewer returned correction PASS20:28:47Z105626/25840040.88%,
owner helper106752/25840041.31%20:28:55Z token_count.info sources, now quiescent.
Entire production controls actual-code/callers review found NO production defect;
fixture/foreign-scope findings both resolved. Overall acceptance remains UNVERIFIED.

Final focused `cargo test --locked --offline --lib workflow_control_` PASS26/26,
no skips, stock2MiB, tests14.36s (compile duration in exact log):
/private/tmp/workflow-controls-acceptance-focused-final.log.
Broader same-env `cargo test --locked --offline --lib workflow::` FAILED407PASS1FAIL,
no skips,152.75s: /private/tmp/workflow-controls-acceptance-integration.log.
Sole failure `workflow_wakeup_completion_has_no_parent_run_or_execution_lock`:
existing wakeup_tests.rs77–120, panic at99 (outer2s tokio timeout .unwrap), Elapsed.
Test holds parent workflow_runs AND workflow_executions FOR UPDATE while advancing
pure child on another connection; original criterion forbids hidden parent locking.
Next worker MUST reproduce isolated and inspect live locks/server diagnostics before
calling this load sensitivity or regression. Do NOT raise2s or dismiss as preexisting.
No isolated rerun or cause classification performed in this worker. Parent lookup test
with12,000 history rows ran>60s but PASSED; server active query had no blockers.

Exact failed script /private/tmp/workflow-controls-acceptance-final-checks.sh exited101;
SQLx --check, offline all-target check,Clippy,fmt check,bothdiff,graft FINAL gates were
NOT REACHED and remain required. SQLx preparation16.51s and migration/schema inspection
remain passing evidence; prepare is NOT --check. Relevant all-workflow tests other than
one wakeup timeout passed; full final integration acceptance cannot be inferred.

New test files control_acceptance_tests.rs (~450lines),control_revision_tests.rs;
control_tests.rs only appends module; admission_authority/source_tests append2 cases.
No production source or migrations edited by this worker. Preserve ALL prior work,
including website. Complete six-group expansion remains above; new tests cover all
six groups, pending this failed integration and final checks/evidence review. No budgets,
fairness or later phases begun. Do not repeat broad discovery; use exact seam above.

All build/test/SQLx commands completed; no job left running. TaskPG retained RUNNING
PID5022, data/private/tmp/workflow-admission-pg-e3aa, log/private/tmp/workflow-admission-postgres.log,
127.0.0.1:55439/socket/private/tmp/max_connections200. BOTH task URLs unchanged.
Do not reset. All migrations through20260928201000 remain immutable. No stage/commit/
reset/deploy. Root owns PROGRESS/RESUME and acceptance; replacement owns BRIEF/code/PG.
Graft estimate this worker2,730,232 tokens (map dominates, not measured model savings);
reviewer initial2,798,562 plus correction30,421, reported separately.

### Controls parent-lock correction expansion — 2026-09-28 20:51Z

Isolated existing wakeup test reproduces2s timeout; live pg_stat_activity records
child execution completion blocked by held parent transaction. PostgreSQL server
CONTEXT identifies nested advance_workflow_owned_revision UPDATE workflow_runs ->
workflow_parent_execution_fk FOR KEY SHARE against parent execution. Repeated run
updates in one transaction recheck the unchanged parent FK. This is a real lock-order
regression exposed by controls revisions, not a reason to raise the bound.
Evidence: /private/tmp/workflow-controls-finish-{wakeup-isolated,wakeup-lockrun,
live-locks}.log and retained postgres log22:47:23/50CEST.

Correction is an additive immutable lineage identity table, not another ledger:
workflow_run_parents stores company,child run,parent run,parent execution. Move the
existing scoped parent execution FK onto this never-updated child-owned row. Keep
run columns and existing consumers unchanged; scoped full-tuple circular FKs prove
run/lineage equality and child ownership. BEFORE INSERT run materializes lineage
atomically; child-owner FK is deferred because the run INSERT is still in progress.
Run->lineage FK references only child-owned row, so revision updates cannot lock
parent trees. Block direct lineage UPDATE/DELETE while owner exists; allow owner
cascade. Parent execution deletion remains constrained by original composite FK.
Backfill retained rows before replacing old FK; retained mismatch and child counts
both0 at20:50Z. No existing applied migration edit or data reset. Existing admission
transaction/replay and parent source immutability remain sole causality owner.

Affected consumers traced with graft grep parent_run_id/parent_execution: admission
insert, run inspection, wakeup lookup, immutable migrations, wakeup schema rollback
fixture. No runtime query API change. SQL functions unindexed; inspected exact
20260928145500 parent FK and20260928200000 revision trigger definitions. Update
migration rollback fixture for new dependent schema. Tests: unchanged2s held-parent
completion regression, repeated child revision/material changes, scoped malformed
parent cases, lineage immutability/deletion integrity, atomic admission rollback,
retained-row backfill. Existing controls26 and whole workflow:: integration plus
sequential migration/SQLx prepare/check/offline all-target/Clippy/fmt/bothdiff/graft
and independent actual-code/evidence review remain required. No new unresolved
contract; budgets/fairness/full03 and parent wait settlement remain later scope.

### Parent-lock correction implementation/review — 2026-09-28 20:55Z

Architecture review PASS; sole reviewer01a0e9c5-3827-7832-b7eb-f1d17f76b1f3
Astra/medium,51,162/258,40019.8%20:50:29Z. New migration20260928210000
APPLIED and IMMUTABLE. Native immediate run->lineage FK prevents direct deletion;
owner FK cascades and is initially deferred for BEFORE INSERT materialization.
Only UPDATE immutability trigger is needed. Admission plain INSERT uses normal
deferred owner checking. Existing run parent columns/causal readers unchanged.

Actual-code correction review found no production defect and one fixture finding:
new parent deletion assertion reached existing background_tasks FK first. Removed
only parent fixture job before exact parent-FK assertion. Original review measured
63,208/258,40024.46%20:53:26Z. Source edits paused during both reviews.
Initial focused run13PASS1FAIL3.61s; its compile snapshot did not include final third
new test. Full15-test rerun started with final source; evidence pending. Existing
held-parent2s completion, wrong-run/foreign-parent constraints, retained-child
backfill all passed initial run. No timeout/stack/resource bound raised.

Changes confined to additive migration, wakeup_guard_tests schema replay fixture
and wakeup_parent_link_tests (3meaningful realDB tests). Current final commands
/private/tmp/workflow-controls-finish-final-checks.sh: migrate info, SQLx prepare,
controls26, all workflow::, SQLx --check, offline alltarget check/Clippy/fmt/bothdiff,
graft. Full logs prefix/private/tmp/workflow-controls-finish-. Root owns acceptance;
no budgets/fairness/full03 claim. Implementer01a0e9c5-018d-7b02-b503-d36c14eb9a01,
89,813/258,40034.76%20:54:48Z; usage/runtime sources throughout this correction.

### Controls final verified handoff — 2026-09-28 21:03Z

Controls acceptance/correction complete for root acceptance. No budgets/fairness,
full03 final-library gate, production handler/startup or later phase work started.
User requests checkpoint/stop after current work because weekly allowance is low.

Root cause was real: revision-trigger repeated child-run UPDATE rechecked parent
FK and blocked on parent's held execution row. Additive20260928210000 moves that
FK onto immutable scoped child-owned lineage while reciprocal full-tuple FKs retain
identity/deletion integrity. All prior applied migrations unchanged;210000 now also
IMMUTABLE. Existing2s regression unchanged and green. Three new realDB tests cover
native parent restriction, child/company cascade, immutable/delete refusal, exact
committed owner and failed-insert rollback; existing migration replay covers2retained
children. Fixture-only corrections removed exact owned jobs that otherwise masked
lineage FKs. Reviewer explicitly verified this does not weaken lifecycle assertions.

Final exact commands /private/tmp/workflow-controls-finish-final-checks.sh EXIT0.
All DB commands set BOTH task URLs; all tests stock RUST_MIN_STACK=2097152,
SQLX_OFFLINE=true, no skips. Logs prefix/private/tmp/workflow-controls-finish-:
- wakeup-final2.log15PASS3.87s; controls.log26PASS9.21s;
  integration.log411PASS139.06s (original failed suite408tests +3new).
- migrate.log/migrate-info.log/lineage-schema.log PASS; sqlx-prepare.log17.42s;
  sqlx-check.log16.50s; check.log1.02s; clippy.log53.07s allPASS.
- fmt.log,diff.log,diff-staged.log,graft.log PASS. Original failed locks and initial
  fixture failures retained in wakeup-isolated/wakeup-lockrun/live-locks/server-lock-
  context/wakeup-corrected/wakeup-final logs; none counted as passing acceptance.

Independent final correction actual-code PASS20:57Z, then evidence PASS
21:02Z. Reviewer session01a0e9c5-3827-7832-b7eb-f1d17f76b1f3 Astra/medium,
73,575/258,40028.47%21:02:08.113Z token_usage_record.usage/runtime window.
Prior entire-controls actual-code review20:28Z reused where unchanged; new reviewer
inspected actual migration/callers/tests/combined effects, not reports alone. No
unresolved findings. Edits paused during review. Reviewer quiescent, no further work.
Implementer01a0e9c5-018d-7b02-b503-d36c14eb9a01 Astra/medium latest
98,240/258,40038.02%21:02:06.818Z same sources; final sample sent root separately.
Graft estimates implementer266,813, reviewer~2.87million (whole-file/map comparison,
not measured usage savings), root excluded.

TaskPG STOPPED CLEANLY21:03Z; pg_ctl status exit3 no server. Data retained at
/private/tmp/workflow-admission-pg-e3aa, log/private/tmp/workflow-admission-postgres.log;
restart explicit127.0.0.1:55439/socket/private/tmp/max_connections200. Both URLs
postgres://mac03@127.0.0.1:55439/workflow_admission. No reset/stage/commit/deploy.
All build/test/SQLx jobs finished. Existing staged/unstaged/untracked and website
work preserved; before/after status logs show only intended wakeup_guard_tests edit
and2new migration/test files added, no removed status entries. Root owns final
PROGRESS/RESUME/README acceptance. Next fresh task resumes remaining03.9 budgets,
fairness and full phase03 combined gate only after rereading root resume.
