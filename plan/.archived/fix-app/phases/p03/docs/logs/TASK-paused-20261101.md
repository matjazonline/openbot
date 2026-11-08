# FIXAPP-P03 — Workflow database guards (phase record)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. No backfills, dual reads/writes, legacy decoders or compatibility shims.

## Identity

- Phase ID: `p03` (IDs `FIXAPP-P03-…`). Workflow: `implement-opus-reviewed-2_2`
  (skill dir `/Users/mac03/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Record root: `/Users/mac03/dev/openbot/.worktrees/fix-app-p03/plan/fix-app/phases/p03/docs/`.
- Worktree / branch: `/Users/mac03/dev/openbot/.worktrees/fix-app-p03`, `fix-app/p03`.
  Starting commit `48626a1` (main with p01, p02, p05 merged). Plan spans cite `4e92edd`; every
  span re-checked against `48626a1` before editing. No preserved pre-existing changes (clean
  worktree at start).
- Root = Author (A): phase root subagent of the manager session, Opus 5.5 / `high` (set by the
  manager at spawn). Reviewer (B): Agent `general-purpose`, model `opus`, effort `xhigh`; ID below.
- `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` observed via `printenv` in this session.
- Resources (fix-app-I002): private Postgres 18 at `127.0.0.1:55403` (trust), `.env`
  `DATABASE_URL=postgres://mac03@127.0.0.1:55403/mail_agents`; tests on `mail_agents_test` and
  `own_database` siblings; `CARGO_TARGET_DIR` default `target/` in this worktree; cargo runs with
  `CARGO_INCREMENTAL=0`.

## Requirements and sources

- Plan: `plan/fix-app/03-workflow-database-guards.md` (four batches), `plan/fix-app/README.md`.
- Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md` (state machines as constraints + equivalence matrix,
  preserve correctness guards, DB test isolation), `src/adapters/http/AGENTS.md` (error mapping).
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` (writer: manager), checked
  at revision 4 (2026-11-01).
  - fix-app-I001 r1 (repo/plan rules; edit only named init-schema statements in place) —
    acknowledged. The plan names the workflow guard RAISEs (batch 1), so editing those function
    bodies is in scope; recorded as a merge-risk note below.
  - fix-app-I002 r1 (isolation/resources) — acknowledged.
  - fix-app-I003 r1 (commit accepted phase on `fix-app/p03`, no push, no COORDINATION.md) —
    acknowledged.
  - fix-app-I004 r1 (targeted tests only, no full suite / `scripts/check.sh`; reset DB when the
    schema changed; nonzero counts) — acknowledged; supersedes the plan's "Final gate".
  - fix-app-C001 r1 (p02 continuation limit; effect_parked branch bounded) — acknowledged; impact
    and response in D1/D2 below. Verification: p02 filters `message_park_continuation_`,
    `delivery_late_completion_`, `notification_stale_`, `schedule_claim_` rerun after p03.
  - fix-app-C002 r1 (company/owner one fact; fixtures use `insert_company_with_owner` etc.) —
    acknowledged; new fixtures go through existing workflow fixtures / `test_support` helpers.
  - fix-app-C005 r1 (run→binding revision FK; deferrable FKs in workflow message/thread tables) —
    acknowledged; p03 adds no FK and deletes no runs/bindings; the guard-failure path only updates.

## Acceptance criteria (from the plan)

1. Workflow guard RAISEs carry a stable identifier; a distinct `AppError` variant carries the
   guard's name; the workflow worker treats it as terminal for the job: fails the run with a reason
   naming the guard and records a failure metric. Tests `workflow_guard_error_*` (forced violation
   fails the run once with the guard's name, not reclaimed on the next poll; an ordinary
   connection error is still retried).
2. `RunState::can_transition_to` (incl. retry edge) mirrored by SQL trigger
   `guard_workflow_run_transition`, firing before `stamp_workflow_run_finished`; retry edge only
   where `check_workflow_control_retry` permits; DB-backed equivalence matrix; `terminal_execution_id`
   writers inventoried, rule decided, CHECK added, writers fixed. Tests `run_transition_*` (matrix,
   cancelled → running rejected); run control suites.
3. Admission inserts root `workflow_root_budgets` + `workflow_root_budget_usage` from compiled
   limits in the same transaction; trigger keeps only the run→root link; `sealed_legacy` and
   provenance dropped; legacy test removed/rewritten. Tests `root_budget_admission_*`, child links
   to root; `grep -n compiler_revision migrations/` empty.
4. Lease renewal `WHERE` carries the execution generation; `DeliveryStatus::is_terminal` resolved;
   token-refusal investigation test first, fix only if it fails.

## Batching

One architecture gate **G1** (decisions D1–D4 below: error classification + terminal failure
transaction, run state machine trigger + CHECK, admission budget ownership/schema). Then the whole
plan as one review batch **B1** (plan batches 1–4), implemented in plan order (batch 1 first).
D5–D7 (batch 4) are routine and not gated, recorded for context.

## Architecture gate G1 — decision revision r1 (2026-11-01)

### D1 — Typed workflow guard errors and terminal guard failure

**Identification (SQL).** Every `RAISE EXCEPTION` in an init-schema function whose name contains
`workflow` gets `CONSTRAINT = 'workflow_guard:<name>'`, keeping its existing SQLSTATE (most are
`23514`; a few are `P0001` — e.g. 'workflow message park lost its fence' — and one `54000`).
`<name>` is the function name, or the RAISE's existing constraint name where it already has one
(`workflow_activation_immutable`, `workflow_output_immutable`, `workflow_execution_identity_immutable`,
`workflow_run_association_immutable`; tests referring to those names are updated to the prefixed
form). Exclusion: `validate_workflow_job_association` keeps its FK-shaped names
(`background_tasks_workflow_execution_fk`, `background_tasks_workflow_association`) — it is an
association/FK check at insert, not a progression guard. ~127 RAISEs, edited by script in place;
no message text or SQLSTATE changes, so existing tests asserting codes/messages keep their meaning.
- Why a constraint-name prefix, not a new SQLSTATE: ~30 test files assert `23514` on these guards
  and `decisions.rs::closed_at_commit` maps `23514`; a prefix is unambiguous (table CHECKs never
  carry it) and leaves SQLSTATE semantics alone.

**Mapping (Rust).** `From<sqlx::Error>` (`adapters/persistence/mod.rs`): a database error whose
`constraint()` starts with `workflow_guard:` becomes `AppError::WorkflowGuard(WorkflowGuardViolation
{ guard: WorkflowGuardName, message: String })` (message = the former `Database` text). The name is
validated (`[a-z0-9_]{1,63}`) — an unexpected value stays `AppError::Database`. Timeouts (55P03/57014)
keep precedence. HTTP maps the variant like `Database` (500 "Database error", log kind
`workflow_guard`). No other non-test code matches `AppError::Database`.

**Worker policy (application).** `WorkflowWorker::process` wraps its current body: on
`Err(AppError::WorkflowGuard(v))` it calls a new port `WorkflowGuardFailure::fail_run_on_guard(
GuardFailure { scope, guard, site, fence })` (boxed future, stock-stack safety) and returns its
disposition; every outcome increments counter `workflow_guard_violations_total{guard, outcome}`
via a new optional `monitoring: Option<&dyn MonitoringService>` worker field (production wiring
passes the engine's monitoring; tests `None`) and logs a warning (error when the failure itself
fails). `site` = which candidate work hit the guard (`Step` for jobs/waits/receipts, `RunExpiry`
for `ExpiredRun`, `LeaseExpiry` for `ExpiredLease`/`ExhaustedPending`); `fence` = the IO claim's
fence when the guard came from `complete_io` (handled inside `execute_io`), else `None`.

**Guard-failure transaction (persistence), one transaction, existing lock order (run, executions,
jobs):**
1. Lock run `FOR UPDATE`; not `queued/running/waiting` → `Inactive` (no write).
2. Time races stay with their existing owners (no write → `Deferred`): run deadline passed and site
   ≠ `RunExpiry` (expiry fails it as `workflow.run_deadline`); scope job `processing` with an
   expired lease and site ≠ `LeaseExpiry` (expired-lease retirement owns it); scope job
   `processing` with a live lease not matching `fence` (another worker's claim, or `fence` None).
   Rationale: guards compare `clock_timestamp()` at COMMIT, so a slow commit can trip "lost
   ownership" or a deadline legitimately; those must not become guard failures.
3. Otherwise fail the run like deadline expiry/cancel do: lock executions and jobs,
   `maintenance::retire_jobs(…, RetirementCause::Guard)` (processing attempts → failed, class
   `terminal`, code `workflow.guard_violation`, safety `unknown`, retirement `guard`; jobs →
   `failed`; pending jobs → `failed`), open waits → `cancelled`, message deliveries cancelled with
   `DeliveryCancellationReason::WorkflowExpired` ("ended before it went out"), run →
   `failed`/`waiting_reason NULL`/`terminal_execution_id = scope.execution`, audit event
   `guard_violation:<guard>` (precedent `attempt_failed:<n>`), `action_uncertainty::record(…,
   workflow.guard_violation)`. Children are not cascaded (same as deadline expiry; a live child
   ends on its own deadline and its terminal fact wakes a terminal parent harmlessly).
4. Return `Failed`. Any error (incl. another guard) → returned; worker logs at error level and the
   job stays as it was (next poll may retry the whole thing once more).

**Schema for D1.** `task_attempts.workflow_attempt_failure_shape` admits retirement `guard`;
`check_workflow_fenced_retirement` gains a `guard` branch mirroring `cancel`: `NEW.status='failed'`,
class `terminal`, code `workflow.guard_violation`, safety `unknown`, and the owning run is `failed`
with a `guard_violation:%` audit event and `successor_execution_id IS NULL`. Retry eligibility
(`workflow_control_retry_safe`, `…_reopen_safe`) already admits only `live/expired`, so a guard
failure is never retry-safe (a guard failure is not retried by control retry: it would re-trip).

- Invariants: a guard violation never silently loops to the deadline; time-based refusals keep
  their existing owners; a stale worker never fails a run another worker owns; terminal runs are
  untouched.
- Failure modes: failure txn itself trips a guard → error log + counter `outcome=error`, job left
  for the next poll (bounded by the deadline as today).
- C001 response: the effect park (running → waiting/effect) is an allowed transition; a refusal by
  the effect_parked branch at the limit (`check_workflow_fenced_completion`) is classified as guard
  `check_workflow_fenced_completion` → terminal, after p02's Rust refusal (the guard is a backstop).
- Verification: `workflow_guard_error_*` DB tests on `own_database`: (a) an injected test trigger
  raising with `CONSTRAINT='workflow_guard:test_forced_guard'` on a pure step's commit → run
  `failed`, `terminal_execution_id` set, event `guard_violation:test_forced_guard`, job `failed`,
  second `process` of the same candidate does nothing; (b) a real guard (`guard_workflow_run_transition`)
  maps to `WorkflowGuard` with that name; (c) an injected non-guard error (SQLSTATE `08006`) leaves
  the run/job untouched and the step succeeds on the next poll after the injection is dropped;
  (d) deferral: an IO claim whose lease expired → `Deferred`, no write; (e) unit test of the
  mapping in `persistence/mod.rs`.

### D2 — One run state machine (Rust table + SQL trigger) and `terminal_execution_id` shape

**Table (states as stored).** Same-state updates are allowed (no transition; e.g. waiting→waiting
re-park, terminal stamps preserved by `stamp_workflow_run_finished`). Otherwise:
- `queued` → `running`, `waiting`, `succeeded`, `failed`, `cancelled` (claim; park/pending
  recovery from queued; a pure first step that ends; expiry; cancel);
- `running` → `waiting`, `succeeded`, `failed`, `cancelled`;
- `waiting` → `running`, `failed`, `cancelled` (resume; wait expire/fail; cancel);
- `failed` → `running` — the explicit retry edge only;
- `succeeded`, `cancelled` → nothing. Nothing ever returns to `queued`.

**Rust.** `RunState::can_transition_to(self, next) -> bool` in `domain/workflow/state.rs`
(comparing stored state, ignoring waiting reason), documenting `Failed → Running` as the control
retry edge the database further restricts; plus `RunState::stored_name()` so the matrix test and
persistence share one name mapping.

**SQL.** `guard_workflow_run_transition()` + `CREATE TRIGGER guard_workflow_run_transition BEFORE
UPDATE OF state ON workflow_runs FOR EACH ROW` (name sorts before every `workflow_*` BEFORE trigger,
so it fires before `stamp_workflow_run_finished`). Same table; the `failed → running` edge is
admitted only when `OLD.terminal_execution_id IS NOT NULL`, `NEW.terminal_execution_id IS NULL`, and
the terminal execution's workflow job is already `pending` (reopened earlier in the same
transaction, as `control_retry::retry_on` does) and `workflow_control_retry_safe(company, run, job)`
holds — the same predicate `check_workflow_control_retry` requires at commit (which additionally
checks the applied retry receipt). Refusal: `RAISE … 'workflow run cannot move from % to %' USING
ERRCODE='23514', CONSTRAINT='workflow_guard:guard_workflow_run_transition'`.

**`terminal_execution_id`.** Writers today: `maintenance.rs:44` (deadline, failed), `recovery.rs:195`
(failed only), `recovery.rs:355` (parked failure), `decision_tokens.rs:160`, `pending_recovery.rs:103`
(failed only), `batch_commit.rs:50` (set when there is no successor: `succeeded`/`failed`),
`child_waits.rs:264` (follow-up UPDATE after `wait_commit::fail`), cleared by `control_retry.rs:40`.
Missing: `wait_commit.rs:306` (`expire`, waiting→failed) and `:394` (`fail`). Rule:
`terminal_execution_id IS NOT NULL` exactly when `state IN ('succeeded','failed')` — every
succeeded/failed run names the execution that ended it; queued/running/waiting/cancelled never do.
CHECK `workflow_run_terminal_execution_shape` on `workflow_runs`. Fixes: `wait_commit::expire` and
`::fail` set `terminal_execution_id = scope.execution` in the same UPDATE; the separate
`child_waits.rs:264` UPDATE is removed (now redundant). Retry eligibility is unchanged for those
paths (their job is `completed`, never retry-safe).

- Invariants: SQL accepts a state update exactly when `can_transition_to` does (retry edge: plus
  retry eligibility); no transition out of succeeded/cancelled; failed only back to running by an
  eligible control retry.
- Failure modes: an unknown writer making a disallowed move now fails with a typed guard error
  (terminal under D1) instead of corrupting state.
- Verification: `run_transition_matrix_matches_domain` (every (from,to) of the 6 stored states on
  real rows, incl. failed→running refused without retry evidence), `run_transition_cancelled_to_running_is_rejected`,
  `run_transition_terminal_execution_shape_*`; then `routes::workflows::ui::runs::`, `control_retry`,
  and the whole `adapters::persistence::workflow::` suite (plan-mandated broad check).

### D3 — Root budgets come from Rust admission; provenance dropped

- `admission_write::insert_run` (shared by admission and the runtime child writer): for a root run
  (`parent: None`) insert `workflow_root_budgets(company_id, root_run_id, activations, model_calls,
  repetitions, token_policy)` from `definition.limits.root_budget` and `workflow_root_budget_usage`
  (zeros), **before** the run INSERT, same transaction. `workflow_root_budgets → workflow_runs` FK
  is already `DEFERRABLE INITIALLY DEFERRED`; `workflow_run_budgets.root_run_id → workflow_root_budgets`
  is immediate, so the trigger's link insert proves the root budget exists (a root run without one
  fails the INSERT with 23503 instead of becoming budgetless).
- Token policy JSON comes from one helper shared with the compiler (`compile.rs` builds
  `root_budget.tokens` from the same `TokenAllowance`), so stored policy equals the compiled one.
- `insert_workflow_budget_link`: drops the bundle decode and the root-budget inserts; keeps the
  child lookup and the `workflow_run_budgets` link insert for every run.
- `workflow_root_budgets.provenance` dropped with `sealed_legacy`; CHECK becomes the former
  `frozen_v6` arm (activations/model_calls/repetitions NOT NULL within ranges);
  `workflow_root_token_policy_check` = `token_policy IS NULL OR valid_workflow_token_policy(…)`;
  `debit_workflow_budget_receipt` and `workflow_action_reconciliation_budget_eligible` drop the
  provenance predicate; `token_budget.rs` drops `budget.provenance<>'frozen_v6' OR`;
  test SQL `action_reconciliation_state_witness_validate.sql` follows.
- Raw-SQL test fixtures inserting root runs (7 test files) insert the budget rows first through a
  shared test helper.
- Verification: `root_budget_admission_*` (root budget = compiled limits incl. token policy; child
  links to its root and gets no budget row; raw root run without budget refused),
  `budget_`, `decision_tokens`, `control_retry` filters; `grep -n compiler_revision migrations/` empty.

### D4 — Lease renewal fenced to its execution (protocol hardening)

`lease.rs` renewal UPDATE adds `AND status='processing' AND execution_generation=$5 AND worker_id=$6`
(the fence's generation/worker), so it is correct on its own, not only because `lock_fence` ran
first. Verification: existing `lease_` / `capacity_renew_` tests + a stale-generation renewal test.

### D5–D7 (not gated, batch 4)

- D5 `DeliveryStatus::is_terminal` has no callers (`graft callers`, grep): delete it rather than
  keep a misleading predicate (workflow deliveries leave `outcome_unknown` on reconciliation).
- D6 Token refusal: write `token_refusal_child_cancel_*` first (child parked on a token decision is
  cancelled; parent then tries a token-consuming step). Fix only if it fails; candidate fix: refuse
  on expiry alone (cancellation is abandonment, not refusal) and drop `cancelled` from the
  `token_refusal` CHECK. D1's guard failure cancels waits (like cancel), so whatever D6 decides
  for cancellation applies to a guard-failed child too; the D6 test covers the cancel path.
- D7 records: merge-risk notes (below).

## G1 decision revision r2 (2026-11-01) — responses to FIXAPP-P03-G1-R1

r1 text above is preserved; r2 replaces the named parts. D2–D4 unchanged except where stated.

### D1 r2 — scope, confirmation, deferral, cascade, retry safety

**Scope (F2, option b).** The terminal policy covers guard violations returned by the worker's own
persistence calls: every port call in `WorkflowWorker::process` (poll candidates of every kind,
`claim_io`, `advance_pure`, parks, `resume_wait`, `expire_run`, `retire_expired_io`,
`retire_exhausted_work`, `consume_message_receipts`) and `complete_io`. Guard violations inside a
handler's own transactions (message publication/effect park, action dispatch and remote entry,
harness checkpoint/token admission, model-tool publication) stay **retryable step failures bounded
by the attempt budget**, because most of them are fence-loss races the handler must treat as
retryable (`workflow message park lost its fence`, `… dispatch lost live fence`, `… requires a live
fence`); turning those terminal would convert lease races into run failures. They become typed and
visible instead: `action_step_failure` (message/HTTP/MCP/tool families) and the agent harness
`failure` map `AppError::WorkflowGuard` to Retryable `<family>.guard_violation` /
`agent.guard_violation` with diagnostic `guard <name>` (was `*.unresolved` with no diagnostic;
same class and safety). When the budget is spent the existing exhaustion path ends the step with
that code. Supervisor-internal persistence (release/retire inside `supervise_io`) surfaces as
`Stopped(Persistence)`; the lease then expires and the `ExpiredLease` candidate reaches D1 at the
worker level, so a deterministic retirement disagreement is still failed and counted there.
- C001 response (corrected): the effect-park backstop in `check_workflow_fenced_completion` fires
  at the message handler's publication COMMIT, so it is a handler-level violation: Retryable
  `message.guard_violation` naming `check_workflow_fenced_completion`, bounded by the attempt
  budget. p02's Rust refusal (`MessagePublication::ContinuationLimit`) answers first; the SQL branch
  is the backstop. The effect park itself (running → waiting/effect) is an allowed transition (D2).
- Guard lists. Fire at worker level (terminal policy): `check_workflow_fenced_completion` /
  `_retirement` (complete_io, retirements), `check_workflow_progression_deadline`,
  `preserve_workflow_progression`, `check_workflow_wait_commit` (park/resume), wait/decision
  guards (`workflow_decision_*`, `workflow_continuation_counter_guard`, `immutable_workflow_wait_event`,
  `allocate_workflow_wakeup_sequence`, child/parent guards), `workflow_message_reopen_guard` and
  `workflow_message_delivery_transition_guard` (receipt consumption), `guard_workflow_run_transition`,
  budget guards on `advance_pure`/activation. Fire inside handlers (typed retryable):
  `workflow_action_*` dispatch/remote-entry/receipt guards, `workflow_message_action_*`,
  `workflow_message_admission_guard`, `workflow_message_effect_wait_guard`, the harness
  token/progression guards, `guard_workflow_token_proposal`, `require_workflow_token_progression`.
  Several (e.g. `check_workflow_fenced_completion`) fire in both; the site decides the policy.

**Confirmation (F1, F5 — time races uniformly).** The worker acts on a violation only if an
immediate re-execution of the same operation trips a guard again: `process` re-runs the candidate
once; `execute_io` re-runs `complete_io` once with the same result (`FencedWorkflowResult` gains
`Clone`). Guards compare `clock_timestamp()` at COMMIT; on re-execution the Rust statement-time
checks already see the moved clock and take the owned path (deadline expiry, lease-lost/expired
retirement, wait timeout / invalid-input settle, expired proof), or the fence check finds the claim
settled by its competing owner (F1's retirement-first interleaving: `complete_on` → `lock_fence`
refuses, no guard). Only a violation that repeats is a disagreement. A different guard on the
re-run is acted on under its own name. Cost: one extra transaction per violation.

**Failure transaction deferral (F1, F5), after locking the run (READ COMMITTED, so every later read
sees state committed by whoever held the lock before):**
1. Run not `queued/running/waiting` → `Inactive`.
2. Scope execution complete (`completed_at` or `successor_execution_id` not NULL) → `Deferred`.
3. Run deadline passed and site ≠ `RunExpiry` → `Deferred` (expiry owns it).
4. An open wait of the scope execution with `deadline <= clock_timestamp()` and site ≠ `RunExpiry`
   → `Deferred` (wait timeout owns it).
5. Scope job: with `fence = Some(f)` require `status='processing'` under exactly `f` (worker,
   generation, attempt ordinal) with `lock_expires_at > clock_timestamp()`, else `Deferred` (the
   claim was settled or reclaimed by another owner). With `fence = None`: site `LeaseExpiry`
   requires `processing` with an expired lease or `pending`; other sites require `pending` or
   `completed`; anything else (live foreign claim, `failed` by a competing retirement) → `Deferred`.
6. Otherwise fail (r1 step 3) and **cascade** (F3): before closing the run's waits, collect live
   children from its waiting `child_run` waits and cancel each subtree with the existing
   `cancel_tree_on` traversal (parent locked before children, events `cancelled_by_parent`, actor =
   run actor). Rationale: a guard failure ends the run early like cancellation; children share the
   parent's deadline only at expiry.
- Residual (documented, not deferred): a park that INSERTs an event wait whose *own* deadline lands
  inside that same COMMIT, and re-runs into the same window, is failed as a guard violation instead
  of the invalid-input settle; both end the step failed. Bounded by confirmation (two consecutive
  windows).

**Retry safety (F4).** `workflow_control_retry_safe` gains `AND NOT EXISTS (run event
'guard_violation:%' for the run)` (PK-indexed lookup), so a guard-failed run is never control-retry
safe whether its job was processing or pending; D2's retry edge inherits it. Recovery stays a
rerun.

**Competing-claimant tests (AGENTS.md).** (i) Retirement-first: a claim whose lease expired is
retired by `retire_expired_io`, then `fail_run_on_guard` with the stale fence → `Deferred`, retired
state intact. (ii) Concurrent guard failure vs `cancel` on the same run: exactly one terminal
outcome, every job/wait coherent, the loser sees a terminal run. (iii) Pending-job guard failure is
not control-retry safe.

**F6 verification additions.** (1) Tests pinning an empty constraint field
(`action_reconciliation_high_bounds_native.rs`, `*_validate.sql`/`*_fixture.sql`
`native_constraint=''`, ~10 files) are updated to the new `workflow_guard:<name>` (stronger), not
deleted. (2) `action_reconciliation_state_witness_fixture.sql`'s `failed/cancelled → waiting` attack:
disposition decided at implementation so that the witness guard is still exercised (preferred: an
attack edge the transition table allows); recorded with evidence; assertions not weakened. (3) Raw
fixtures reaching `failed`/`succeeded` without `terminal_execution_id` or through refused edges are
inventoried and fixed to reach states through allowed edges with the shape satisfied. (4) The D2
matrix builds each UPDATE so only the transition guard can refuse, asserts the refusal's constraint
is `workflow_guard:guard_workflow_run_transition`, and includes an accepted eligible `failed →
running`. (5) D4's predicate is tested on the UPDATE itself (factored helper called with a stale
fence). (6) `workflow_guard_error_*` asserts `poll_work` yields no candidate for the run on two
consecutive polls without advancing time, plus handler-level mapping unit tests
(`action_step_failure`, harness `failure`).

### D2 r2 note (F7)
A violation of the CHECK `workflow_run_terminal_execution_shape` stays an untyped 23514 table-CHECK
error (declined to fold into the trigger: the CHECK is the invariant on every write incl. INSERT;
writers set the column in the same UPDATE and the matrix/shape tests catch a writer bug).
Deliberate exclusions from the `workflow_guard:` prefix (F7): `validate_workflow_job_association`
(FK-shaped), schedule guards (already named, schedule materializer), and guards shared with legacy
tasks or only reachable inside handlers: `guard_harness_*`, `require_harness_owner`,
`guard_model_usage_*`, `guard_jev_invocation_update`, `preserve_background_task_queue_identity`.

### D4 r2 (F8)
Renewal UPDATE fences `company_id`, `workflow_execution_id`, `queue_kind='workflow'`,
`status='processing'`, `execution_generation`, `worker_id`, `attempt_ordinal`, plus the live expiry.

### D1 r2 consumers (F8)
Exhaustive `AppError` matches reached by the new variant: `adapters/http/app_error_impl.rs` (500,
kind `workflow_guard`), `application/services/inbound_event_worker.rs::classify_commit_error`
(`Internal`, not routed — same as `Database`). The run view shows the guard through the
`guard_violation:<guard>` audit event (the only carrier when no attempt was closed).

## G1 decision revision r3 (2026-11-01) — responses to FIXAPP-P03-G1-R2

r1/r2 text preserved; r3 replaces only the named parts of D1 r2.

- **F9 — deferral never hands back to the failing owner.**
  - Rule 4 (open wait past its deadline) is **dropped**. The `Wait` candidate *is* the wait-timeout
    owner, so deferring to it loops; clock races on resume are covered by confirmation (the re-run
    sees the moved clock and takes the timeout path; a timeout path that itself trips a guard
    repeatedly is a disagreement and is failed).
  - Rule 5 at site `RunExpiry` admits **any** job state (`retire_jobs` already settles `processing`
    jobs with an expired or live lease, and `pending` ones). Rule 5 at `LeaseExpiry`: `processing`
    (expired lease) or `pending`, as r2. Other sites without a fence: `pending` or `completed`, as
    r2.
  - Rule 3 (run deadline passed and site ≠ `RunExpiry` → `Deferred`) stays: it defers to a
    different operation (`expire_run`), whose own violation is then failed at site `RunExpiry`.
  - Final deferral list: (1) run not live → `Inactive`; (2) scope execution complete → `Deferred`;
    (3) deadline passed and site ≠ `RunExpiry` → `Deferred`; (5) scope-job ownership rule as above.
  - Tests added: forced guard on the expiry path (`expire_run` at site `RunExpiry`, job
    `processing` with a deadline-capped expired lease) and on the decision-timeout path (`Wait`
    candidate whose timeout handling trips a forced guard twice): each ends in one guard failure and
    `poll_work` yields no candidate for the run on two consecutive polls.
- **F10 — confirmation contract at `complete_io`.** A guard violation from `complete_io` is
  confirmed and acted on **inside `execute_io`**: re-run `complete_io` once with the same
  `FencedWorkflowResult`; if a guard trips again, `fail_run_on_guard` with `site Step` and
  `fence = Some(claim.fence)`, and `execute_io` returns its disposition (`Ok(Done)`); it is never
  re-raised to the `process` wrapper, which therefore never re-runs an IO candidate after its
  handler ran. Guard errors from `claim_io` (before any handler) propagate to the wrapper, whose
  re-run claims afresh (the failed claim rolled back, so no claim is held). Test: forced guard on
  the IO completion commit → one claim, one handler execution, run `failed` with
  `terminal_execution_id`, the attempt retired `guard` (`workflow.guard_violation`), no candidate on
  two consecutive polls.
- **F11 (optional) — adopted as a list correction.** Receipt consumption (`consume_message_receipts`
  → `consume_page`) catches errors per action with backoff and marks the action `stalled` after 5
  failures; guard errors there (`workflow_message_reopen_guard`,
  `workflow_message_delivery_transition_guard`) are therefore bounded and observable (stalled
  action) but not under the terminal policy. The r2 "fire at worker level" list is corrected: those
  two move to "bounded by receipt-consumption backoff/stall". Not propagated (the per-action
  isolation keeps one bad action from blocking a page of others).

## Batch B1 status (PAUSED 2026-11-01 by manager/user; not frozen, not reviewed)

- G1 r3 PASS (FIXAPP-P03-G1-R3). Implementation started 2026-11-01.
- **Done (uncommitted, compiles: `cargo check --lib --tests` clean):**
  - D5: `DeliveryStatus::is_terminal` removed (no callers).
  - D6: test `token_refusal_child_cancel_leaves_parent_progression_open` written first and
    **failed** (root `token_refusal='cancelled'` after cancelling only the child) → fix applied:
    `refuse_expired_workflow_tokens` refuses on `expired` only; `token_refusal` CHECK drops
    `cancelled`; `TokenRefusal::Cancelled` removed;
    `workflow_harness_token_cancel_and_approval_contend_without_late_resurrection` asserts no
    refusal after a root cancel. 30 token tests pass (see Checks).
  - D1 (batch 1) code: 126 workflow RAISEs carry `CONSTRAINT='workflow_guard:<name>'` (script
    `$TMPDIR/p03scripts/name_guards.py`, already applied; do not rerun); `AppError::WorkflowGuard(
    WorkflowGuardViolation)` + `WORKFLOW_GUARD_PREFIX` (`application/app_error.rs`); mapping in
    `adapters/persistence/mod.rs` + test `workflow_guard_error_is_typed_by_its_constraint_name`;
    consumers `app_error_impl.rs`, `inbound_event_worker.rs`; port `application/workflow/guard.rs`
    (`GuardSite`, `GuardFailure`, `GuardFailureOutcome`, `WorkflowGuardFailure`); worker
    `process` = confirm-by-re-run wrapper over `process_once`, `complete_confirmed` in
    `execute_io` (fence `Some`), `fail_on_guard` (counter `workflow_guard_violations_total{guard,
    site,outcome}`, warn/error logs); `monitoring` field on `WorkflowWorker` (prod wired in
    `infra/workflow.rs`, tests `None`); `FencedWorkflowResult: Clone`; persistence
    `workflow/guard_failure.rs` (deferral rules 1/2/3/5, retire `Guard`, waits cancelled,
    deliveries `WorkflowExpired`, run failed + tid, event `guard_violation:<guard>`, uncertainty
    record, child cascade); `maintenance::RetirementCause::Guard`; `controls::cancel_children_on`
    extracted from `cancel_tree_on`; schema: `workflow_attempt_failure_shape` admits `guard`,
    `check_workflow_fenced_retirement` guard branch, `workflow_control_retry_safe` excludes runs
    with a `guard_violation:%` event; handler typing `guard_step_failure` (`<family>.guard_violation`,
    `agent.guard_violation`, diagnostic `guard <name>`) in `actions/failure.rs` and
    `harness/rig/workflow.rs`.
  - D2 (batch 2) started: `RunState::stored_name` and `RunState::can_transition_to` added in
    `domain/workflow/state.rs` (no unit test yet).
- **Not done yet (resume here, in order):**
  1. D2 SQL: function + trigger `guard_workflow_run_transition` (BEFORE UPDATE OF state; retry edge
     per D2), CHECK `workflow_run_terminal_execution_shape`; `wait_commit::expire`/`::fail` set
     `terminal_execution_id`; delete the `child_waits.rs` follow-up tid UPDATE; domain unit test for
     `can_transition_to`; `run_transition_*` DB matrix (assert constraint name; accepted eligible
     failed→running) + cancelled→running test + shape tests; F6.2 state-witness fixture
     disposition; F6.3 raw-fixture inventory (list in G1-F6 and the grep in this session:
     `lease_adversarial_tests.rs`, `child_runtime_tests.rs:161`, `run_finished_at_tests.rs`,
     `wakeup_guard_tests.rs:121`, `maintenance_guard_tests.rs:88`, `control_revision_tests.rs`,
     `wakeup_tests.rs:217`, `run_lineage_tests.rs:48`, `completion_race_tests.rs:123`, ...).
  2. D1 tests: `workflow_guard_error_*` at four sites (pure step, IO completion, expiry,
     decision timeout) with two `poll_work` checks; non-guard error retried (SQLSTATE 08006);
     competing-claimant tests (i) retirement-first, (ii) guard failure vs cancel, (iii) pending
     job not retry-safe; handler mapping unit tests; F6.1 tests pinning empty constraint names
     (`action_reconciliation_high_bounds_native.rs:103`, `*_validate.sql`/`*_fixture.sql`
     `native_constraint=''`, job_association_tests constraint names → prefixed).
  3. D3: admission root budgets in `admission_write::insert_run` (before the run INSERT, root
     only), shared token-policy helper with `compile.rs`, trigger keeps only the link, drop
     `provenance`/`sealed_legacy`, `debit_workflow_budget_receipt`,
     `workflow_action_reconciliation_budget_eligible`, `token_budget.rs`,
     `action_reconciliation_state_witness_validate.sql`, rewrite
     `budget_schema_tests.rs` sealed test → `root_budget_admission_*`, raw root-run fixtures
     (7 files) insert budgets first.
  4. D4: renewal UPDATE full fence (`lease.rs` renew_io) + test on the UPDATE itself.
  5. `./scripts/reset-db.sh --all`; targeted filters (below); `cargo fmt --check`; sqlx prepare
     check; freeze digest; B1 review request to B.

## Checks (commands and results)

- 2026-11-01 `createdb mail_agents` on 55403 (dev DB was missing), `./scripts/reset-db.sh --all
  --yes` → reset complete.
- 2026-11-01 D6 investigation (before fix): `SQLX_OFFLINE=true cargo test --locked --offline --lib
  token_refusal_child_cancel` → 1 failed (`left: Some("cancelled")`), as expected for the bug.
- 2026-11-01 after D6 fix + reset: `cargo test --locked --offline --lib -- token_refusal tokens`
  → 30 passed, 0 failed.
- 2026-11-01 after D1 code: `CARGO_INCREMENTAL=0 SQLX_OFFLINE=true cargo check --locked --offline
  --lib --tests` → clean (no errors/warnings). No D1 tests run yet.
- Planned targeted filters for B1 (I004: no full suite): `workflow_guard_error_`,
  `run_transition_`, `root_budget_admission_`, `budget_`, `decision_tokens`, `control_retry`,
  `routes::workflows::ui::runs::`, `adapters::persistence::workflow::` (plan-mandated broad
  workflow persistence suite), p02 C001 filters `message_park_continuation_`,
  `delivery_late_completion_`, `notification_stale_`, `schedule_claim_`, `lease_`, `capacity_renew_`,
  `token`, `persistence::tests::`.

## Cross-phase notes / merge risk

- init_schema statements edited by p03 (planned): ~90 workflow function bodies (RAISE constraint
  names only), `check_workflow_fenced_retirement` (guard branch), `task_attempts` CHECK
  `workflow_attempt_failure_shape`, `workflow_runs` (new CHECK), new function+trigger
  `guard_workflow_run_transition`, `insert_workflow_budget_link`, `workflow_root_budgets` (column,
  CHECKs), `debit_workflow_budget_receipt`, `workflow_action_reconciliation_budget_eligible`,
  possibly `refuse_expired_workflow_tokens` / `workflow_root_budget_usage` CHECK (D6).
- For p06 (adds triggers on `workflow_runs`): BEFORE UPDATE triggers fire in name order:
  `guard_workflow_run_transition` (new, must stay first among those touching state),
  `workflow_action_capture_claim_retirement`, `workflow_action_capture_state_witness`,
  `workflow_allocate_wakeup`, `workflow_parent_immutable`, `workflow_run_finished`,
  `workflow_run_revision`, `workflow_run_snapshot_immutable`, `workflow_runs_preserve_association`.
  New CHECK `workflow_run_terminal_execution_shape`; retention must not rely on cancelled runs
  carrying `terminal_execution_id`.
- For p08 (constrains `task_attempts` stop reasons/codes): new retirement value `guard`, failure
  code `workflow.guard_violation`, its `stop_reason` NULL; handler codes `<family>.guard_violation`
  / `agent.guard_violation`; run event kind `guard_violation:<guard>`.

## Agents

- B: agent `a01d5ce0a5e64d461` (general-purpose, opus, xhigh). G1-R1 changes required (F1–F6
  actionable, F7–F8 optional); G1-R2 changes required (F9, F10; optional F11); G1-R3 **PASS** for
  decision revision r3 (TASK.md sha256 `bb5e48e4…d864`).

## Next action

PAUSED. On resume: re-check COORDINATION.md (was rev 4; it changed on disk at pause time — read
and record the new revision first), then continue "Not done yet" step 1 (D2 SQL trigger + CHECK).
