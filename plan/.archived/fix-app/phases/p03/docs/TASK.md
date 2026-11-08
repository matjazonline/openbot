# FIXAPP-P03 — Workflow database guards (phase record)

## mac02 implementation resume — coordination r13

- User explicitly resumed implementation; prior pause is superseded by this entry. Root/A /root,
  Sol 6.1/xhigh confirmed in conversation; B pending fresh Astra/high assignment, no inherited
  history. Installed skill /Users/mac02/.codex/worktrees/b109/openbot/.agents/skills/implement-astra-reviewed-2-2.
- Worktree /Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03, branch fix-app/p03; start/review base/current HEAD
  48626a1240f032ca3b99372bd3f5a05eb08c0c66. All restored 76 changes preserved; source digest
  at pause 05defcdaaf223bb332f5123dcddda09b740a1d64b96501f9108892e25a1220e9.
  Exclusion remains only plan/fix-app/phases/p03/docs; wrapper logs/snapshot-mac02.sh.
- Authority /Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md, current root sole writer, r13 pinned logs/COORDINATION-r13-resumed.md.
  I001/I003/I004 r1, I002 r2 acknowledged; I005 r1 not applicable to p03.
  C001/C002/C005/C007/C009 r1 acknowledged; C006 proposed output pending acceptance.
- Prerequisites p01/p02/p05 integrated at 48626a1. Original p03 G1 r4 decision SHA256
  5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25 retained;
  fresh B will validate continuity before consequential dependent changes. Source batch never accepted.
- Own PG 127.0.0.1:55403, role mac02, mail_agents/mail_agents_test, migrations verified.
  CARGO_INCREMENTAL=0, own target directory. All checks require explicit own URLs.
- B1 next: inspect/fix all 31 inherited affected failures, retaining native constraint and rollback
  coverage. Focused regression filters first; final affected union workflow persistence/UI runs
  and p02 continuation/delivery/notification/schedule interfaces, fmt/offline compilation/clippy/
  sqlx prepare; no full suite/check.sh per I004. Record commands/results and unavailable checks.
- Current root/A owns all TASK/HANDOFF updates and implementation; B owns REVIEW/notes/findings.
  No sibling work started, no new commits, no source edits yet.


## Local recovery environment — 2026-11-04, coordination revision 12

- Current authority: `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md`; sole writer and local setup owner: current Codex `/root`
  in `/Users/mac02/dev/openbot-recovery` (Sol 6.1/xhigh confirmed by user). Historical managers/workers are stopped;
  no author or reviewer was started for this environment-only request.
- Current record root: `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03/plan/fix-app/phases/p03/docs`. Historical absolute paths below are preserved as evidence;
  map `/Users/mac03/dev/openbot` to `/Users/mac02/dev/openbot-recovery` for current operations.
- Setup owner acknowledges I002/r2 and C009/r1 for this phase; technical instructions/notices are
  unchanged. Fresh implementation workers must read/reacknowledge the current authority on resume.
- Owned worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03`; cluster `/Users/mac02/dev/openbot-recovery/.worktrees/pg-p03` at
  `127.0.0.1:55403`, role `mac02`, PostgreSQL 18.6, lock limit 256.
- DATABASE_URL=`postgres://mac02@127.0.0.1:55403/mail_agents`;
  TEST_DATABASE_URL=`postgres://mac02@127.0.0.1:55403/mail_agents_test`. Both are in the ignored mode-0600 `.env`.
  Export these explicitly for cargo tests; Cargo does not read .env. Keep CARGO_INCREMENTAL=0
  and this worktree's own target directory.
- Both databases are freshly migrated using this checkout's migrations; migration ledger success,
  exact SQL checksums and server identity verified. Evidence: `/Users/mac02/dev/openbot-recovery/plan/fix-app/integration/resumed-all/docs/logs/local-setup-20261104/p03-database-setup.log`;
  common manifest `/Users/mac02/dev/openbot-recovery/plan/fix-app/integration/resumed-all/docs/logs/local-setup-20261104/resources.json`; pinned coordination
  `/Users/mac02/dev/openbot-recovery/plan/fix-app/integration/resumed-all/docs/logs/local-setup-20261104/COORDINATION-r12-local-ready.md`.
- Source remains unchanged and historical reviews/results remain historical. No new application
  tests, implementation, reviews, commits, merges or phase acceptance. The user authorized local
  setup only; implementation remains paused and its unfinished work below is preserved.
- Next action: await explicit implementation resume, then reconcile historical gaps and rerun
  affected checks against these new fixtures. No future phase has started.

## Current status: PAUSED BY USER, 2026-11-04

Final checkpoint: [logs/PAUSE-checkpoint.md](logs/PAUSE-checkpoint.md), authoritative over active
notes below. Source/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`, branch `fix-app/p03`;
paused digest `05defcdaaf223bb332f5123dcddda09b740a1d64b96501f9108892e25a1220e9`, exclusion only
`plan/fix-app/phases/p03/docs` (`logs/PAUSE-source-snapshot.log`). Coordinationr8 read/pinned;
only pause pointers changed. Fresh G1r4 and core24/24 PASS retained. In-flight affected union
finished1071 passed/31 failed/1 ignored,exit101; all31 unresolved, with full log and diagnostic
extract in logs. No B1 review/acceptance, no commit/integration. Every own command stopped;
no new work until explicit resume. Preserve all source/evidence/resources.

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

## Resume adoption — 2026-11-04

- Current workflow: `implement-astra-reviewed-2-2`; installed skill
  `/Users/mac03/dev/openbot/.agents/skills/implement-astra-reviewed-2-2`. Historical sections above
  retain original Opus identities/settings/evidence. Exact paused TASK/HANDOFF copies:
  `logs/TASK-paused-20261101.md`, `logs/HANDOFF-paused-20261101.md`.
- Manager `/root`, BB `thr_gwccjxwzjt`, thin coordinator, actual model/effort unavailable.
  Author `/root/p03_author`, requested Sol6.1/xhigh, actual runtime metadata unavailable.
  Fresh B is manager's direct child (Astra/high requested), pending assignment. Do not reuse stale handles.
  Context target75%; effective threshold unknown; durable checkpoints required.
- Preserved all original uncommitted changes. Start/review-base/HEAD remains `48626a1`; adopted
  source digest `7b5299ca08b6dec63a7c84ef37d266e2a1c98103a838681ee0a1e6c721c9c979` equals pause.
  Exact exclusion only `plan/fix-app/phases/p03/docs`; snapshot `logs/resume-adopt-snapshot.log`.
  Installed snapshot initially failed on macOS realpath GNU options; `logs/snapshot-macos.sh`
  adapts only realpath and calls installed script unchanged. Reuse this exact wrapper/exclusions.
- Coordination authority unchanged; writer now manager thr_gwccjxwzjt, checked revision6.
  Immutable `logs/coordination-r6.md` SHA256 `d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371`.
  I001-I004 r1 and C001/C002/C005/C007 r1 acknowledged; original C001/C002/C005 dispositions
  retained. C007: preserve WorkflowGuard and p04 Forbidden during second integration merge; both
  phases' affected checks and fresh combined review needed. C006 r1 proposed only; output contract
  will be supplied after accepted p03. No changes to shared coordination file by A.
- G1 historical r3 PASS remains historical. Fresh gate required for unfinished SQL/schema/
  concurrency work. Immutable consolidated G1 **r4**: `logs/G1-r4-decision.md`, SHA256
  `5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25`. No technical change from effective r3; fresh reviewer validates D1-D4 before
  dependent edits. Implementation remains B1 in progress, no batch review/freeze/acceptance.
- Private PG55403 manager-owned and healthy (manager probe); old baseline DB names mail_agents/
  mail_agents_test retained. Own target3.3GiB, incremental0; machine free8.7GiB. p03 owns initial
  heavy-build slot; mandatory CARGO_INCREMENTAL=0. Release slot to manager after checks.
- Intervening main e4273e8 changes include mail_agents→open_bots, test migration serialization,
  agent protocol/base/compiler/provisioning changes, Cargo dependency updates, and app_error/
  harness overlap. Finish on existing baseline; do not merge/rebase. Integration must reconcile
  these with p03 and C007, reset the correctly named combined fixtures, and independently review.
- Next: request fresh G1 r4 validation via manager; during gate continue source/caller inventory
  and independent tests. After pass implement all remaining D1-D4 as cohesive B1, reset, run
  focused checks only, audit/freeze, request B1 review. No commit before manager accepts digest.

### Integration evidence, read-only audit (2026-11-04)

`git diff 48626a1 e4273e8` shows exact shared-contract overlap: main adds exclusions for
`agent.budget`, `agent.invalid_output`, `agent.repeated_tool_call` to
`workflow_control_retry_safe`; retain them alongside p03 guard-event exclusion. Main adds
`ExecutionFailure::RepeatedToolCall` and a terminal `repeated_tool_call` harness mapping adjacent
to p03 guard mapping; preserve both. Main renames DBs to open_bots and serializes fixture
migrations (`MIGRATION_LOCK`/`migrate_test_database`); p03 checks use old mail_agents at its
unchanged baseline, integration checks must use combined names/helper. No merge/rebase/import.

### G1 r4 accepted for implementation (2026-11-04)

Manager accepted B's explicit FIXAPP-P03-G1-R4 PASS for immutable decision SHA256
5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25. Source digest preserved
at 7b5299ca08b6dec63a7c84ef37d266e2a1c98103a838681ee0a1e6c721c9c979. Fresh reviewer
/root/p03_reviewer, Astra/high requested; actual metadata not exposed. B owns REVIEW/notes/
findings. Author may resume all D1-D4 implementation; B1 remains unreviewed and unaccepted.

### B1 resumed evidence (in progress)

- D2 transition trigger/CHECK and both wait terminal-id writers implemented; child follow-up
  removed. D3 Rust root-budget insertion and canonical TokenAllowance::policy shared with
  compiler implemented; SQL bundle decode/provenance removed. D4 renew_owned carries full fence.
- 2026-11-04 reset --all --yes passed (logs/resume-schema-reset.log). Affected offline lib/tests
  check passed (53.68s; logs/resume-check-lib-tests.log).
- First workflow_guard_error_ run:12 tests,9 passed/3 failed (logs/workflow-guard-error-first.log).
  Corrections preserve required behavior: a parked decision/child job is already completed;
  retire_jobs changes only pending/processing jobs, while run failure closes waits and prevents
  polling. The assertion now expects completed for those two cases and failed for pure/IO/pending.
  Explicit replay of a stale pure candidate returns Conflict(Workflow batch is not runnable);
  assert that refusal and retain named event count1, terminal id, metric1 and two empty polls.
- F6 state-witness disposition: terminal failed/cancelled→waiting is now rejected by the new
  earlier transition guard. Their original service-owner attacks remain and assert the exact
  typed transition guard/message plus EVERY-public-relation rollback and unchanged native
  catalog. The Human waiting/decision→waiting/reconciliation→running attack remains reachable
  and retains the full original selected check_workflow_control_retry witness assertions,
  including native SQLSTATE/message/context and the now explicit prefixed constraint. The
  first-OLD capture unit covers running and waiting; final terminal no-op updates cannot create
  material first-OLD witnesses anymore. Wakeup tests use separate failed/cancelled children and
  prove each emits exactly one immutable fact; a second terminal transition is expressly refused.
- Read/timestamp retry fixtures now use genuine failed IO claims and actual control retry; no
  failed→queued fabrication remains. Raw terminal fixtures supply same-run execution ids.
  Raw root inserts use seed_root_budget; intended child/FK failures keep their existing targets.
  run-list-query-plan temporary clones inherit the new CHECK, so terminal seed rows name ids.
- Heavy build slot released to manager/p04 after first focused run; no cargo until regranted.
  Free disk8.2GiB after first link. Local discovery graph is records/logs/graft with temporary
  worktree symlink graft; remove that symlink before snapshot. Fallback command `graft skeleton
  src/adapters/persistence/workflow/child_test_support.rs` silently refreshed primary generated
  graph once; no primary tracked paths changed/reverted by A. All later graph calls are local.

### Coordination boundary — revision7 (2026-11-04)

Re-read authoritative coordination; I001-I004/C001/C002/C005/C007 r1 unchanged and
acknowledged. New I005/r1 targets p06/p08/integration only: not applicable to p03; no G1/B1
requirement changes. G1 immutable r6 evidence remains historical; B1 review pins
logs/coordination-r7.md SHA256 `3c5c648e15210556c8a8e27bcb5959df759ab1458bd8dcbcf26fe14582652b73`. C006 proposed outputs unchanged.

### Focused verification checkpoint (2026-11-04)

- Core union command: `CARGO_INCREMENTAL=0 SQLX_OFFLINE=true
  DATABASE_URL=postgres://mac03@127.0.0.1:55403/mail_agents cargo test --locked --offline --lib --
  workflow_guard_error_ run_transition_ root_budget_admission_ workflow_lease_renew_update_`
  PASS24/24 (guard12; run transition7 including domain test and36 SQL pairs; root budget4;
  direct renewal1), build1m41s and tests7.99s; `logs/b1-core-focused.log`.
- Subsequent audit strengthened remaining native constraint pins and added a transient IO
  confirmation regression (one handler/attempt, success, no guard event/counter). No core
  runtime protocol changed; source is now undergoing the combined affected check.
- Final schema reset passed after the checksum-only comment edit:
  `logs/b1-final-schema-reset.log`; live constraint definitions `logs/b1-schema-shape.log`.
- Active affected union (session64449, source frozen while compiling/testing):
  `CARGO_INCREMENTAL=0 SQLX_OFFLINE=true DATABASE_URL=postgres://mac03@127.0.0.1:55403/mail_agents
  cargo test --locked --offline --lib -- adapters::persistence::workflow::
  routes::workflows::ui::runs:: message_park_continuation_ delivery_late_completion_
  notification_stale_ schedule_claim_ workflow_guard_error_ domain::workflow::state::tests::
  persistence::tests::`, log `logs/b1-affected-union.log`. Result pending; no full-suite claim.
- Next: poll current check; fix all affected failures without weakening assertions, rerun
  affected filters; final fmt/offline/sqlx metadata, bounded performance-fixture SQL probe,
  source/contract audit and local graph refresh. Remove temporary graft symlink/cache before
  source freeze. Snapshot with installed script through macOS adapter, request B1 review via
  manager; no commit before manager accepts reviewed digest. Heavy slot currentlyp03, release
  after compilation/link per manager so p04 can build while isolated runtime tests finish.

### Active union checkpoint (not final)

Session64449 continues with source frozen; heavy slot transferred to p04 after build1m55s.
Latest completed tests: 844 /1103. Failures so far:

- `workflow_wakeup_actual_wait_expiry_produces_failed_fact`
- `workflow_action_history_direct_sql_reopen_rechecks_current_retention`
- `workflow_action_reconciliation_append_only_mutation_delete_upsert_and_owner_lifecycle`
- `workflow_action_reconciliation_conflict_row_rejects_update_and_delete`
- `workflow_action_reconciliation_siblings_existing_job_reopen_gates_preserve_history`
- `workflow_action_reconciliation_claim_sql_multiple_applicable_candidates`
- `workflow_action_reconciliation_claim_sql_episode_wrong_retired_ordinal`
- `workflow_action_reconciliation_native_coverage_marker_nonempty`
- `workflow_action_reconciliation_native_coverage_unknown_missing`
- `workflow_action_reconciliation_native_coverage_applied_membership`
- `workflow_action_reconciliation_native_attribution_scope_and_before`
- `workflow_action_reconciliation_native_chronology_marker_rejected`
- `provisioning_lists_an_oversized_directory_within_its_bound`
- `provisioning_refusals_commit_a_receipt_and_no_effect`
- `provisioning_transient_fault_leaves_no_marker_receipt_or_effect`
- `workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses`
- `workflow_action_approval_continuation_ceiling_refuses_without_resetting_failures`

Full failure stdout/causality still pending harness summary. Source triage suggests typed/native
constraint pins and terminal-state fixture adaptations for the first group, but do not treat
that as a result until actual diagnostics are inspected. Three provisioning failures (oversized
directory, refusal receipt, transient fault rollback) remain explicitly uncertain. Main's
b13caaf serializes fixture migrations; do not import it or label failures unrelated without
evidence. Correct all affected failures together, preserving every original rollback/native
guard target where still reachable; record earlier transition refusals explicitly where not.
Core24/24 remains prior evidence. Final reset/live schema checks and bounded TEMP query-plan
fixture(120 rows/0 invalid terminal shapes/ROLLBACK) pass. Recheck coordinationr7 before review;
I005/r1 future-only not applicable, gate unchanged. No commit/freeze/B1 acceptance yet.

### mac02 B1 correction checkpoint

- Independent B /root/p03_reviewer, requested gpt-6-astra/high, no inherited conversation.
  FIXAPP-P03-G1-R5 PASS accepts unchanged G1 decision r4 continuity under coordination r13;
  decision SHA256 5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25.
  Notes/REVIEW persisted; no source acceptance.
- CARGO_INCREMENTAL=0 SQLX_OFFLINE=true DATABASE_URL=postgres://mac02@127.0.0.1:55403/mail_agents
  TEST_DATABASE_URL=postgres://mac02@127.0.0.1:55403/mail_agents_test
  cargo check --locked --offline --lib --tests PASS; logs/mac02-check-lib-tests-initial.log.
- Corrected raw-run fixture atomicity (budget+run inside transaction/savepoint), native guard
  constraint assertions, expected terminal execution on wait expiry, sibling terminal-shape
  fixtures, and new immediate-vs-deferred retry guard coverage. Retained all whole-transaction
  rollback and native target assertions; added after-reopen retention mutation to preserve
  deferred guard coverage rather than replacing it with an earlier refusal.
- cargo fmt --all applied. Focused regression command saved in
  logs/mac02-correction-focused-command.json; compile/tests running, source frozen meanwhile.
  No new passing runtime claim; all historical failures remain open until results.

### Author audit while cold test build runs

- Inspected guard compensation lock/ownership checks, one-confirmation worker path (IO handler
  never reexecuted), typed guard decoder, Rust run table, root admission ordering/token policy,
  and removal of the unused delivery terminal predicate against the retained G1 decision.
  No additional architectural change identified. Exact changed areas recorded in
  logs/mac02-author-audit-progress.json; this is author inspection, not independent acceptance.
- Post-correction candidate snapshot 7e46dae5e58b3d22587aa915be12efdc09bda0b5044caf2e74693037b378a2fb
  at unchanged HEAD, sole phase-doc exclusion, logs/mac02-post-correction-candidate-snapshot.txt.
  Not a final review artifact: focused runtime verification still running/cold compiling.

### mac02 focused correction results

- First focused command logs/mac02-correction-focused-command.json selected 107 tests:
  106 PASS, 1 FAIL, 0 ignored, 2496 filtered; 119.59s runtime after 2m38s cold build.
  Retained failure log logs/mac02-correction-focused.log. All earlier fixture/native-constraint
  corrections passed, including provisioning, continuation ceiling, tool cancellation and
  immediate/deferred retention guards.
- Remaining prior-bounds failure reached scheduled_128 (rather than earlier expiry failure):
  excluded entry chosen as snapshot[0] is UUID/row-order arbitrary and can be the latest entry,
  which legitimately consumed an older proof. Production availability function is unchanged.
  Replaced fixture selection with scoped attempt_number=1 query, preserving the original
  excluded-old/foreign-entry refusal plus 128/129 bound and competing-consumption assertions.
- Extracted the touched retention scenario loop into a focused helper to keep the test readable;
  no behavior change. Next run covers these deltas and the full required affected union once.

### B1 batch review freeze — mac02

- Current source frozen for independent whole-B1 review; identity in
  logs/mac02-b1-review-snapshot.txt, exact sole exclusion plan/fix-app/phases/p03/docs.
  Reviewer /root/p03_reviewer is reused with original requirements and pinned coordinationr13.
- Required affected union remains active (session3528), over315 passes and no failures observed
  at this checkpoint. Final verification pending; reviewer must not infer acceptance from this.
- Final clippy/offline all-target/sqlx prepare checks will run after runtime union to avoid
  heavy compilation competing with time-sensitive concurrency fixtures. Source and metadata
  must retain this digest for review; any unexpected metadata/source delta requires new identity.
- No source corrections while review is active. B records all actionable findings together; root
  will correct the complete set after the round, rerun affected checks and request one recheck.

### Required affected union PASS — current mac02 artifact

- logs/mac02-b1-affected-union.log: 1102 passed, 0 failed, 1 ignored, 1500 filtered;
  283.88s runtime, exit0. Exact command JSON and per-filter nonzero counts retained.
- Counts: workflow persistence1051 (includes1 ignored), run UI26, continuation5, late delivery8,
  stale notification3, schedule claim2, guard errors13, domain run states5, persistence guards7.
  Overlapping filters are not additive. Historical31 failures are resolved by current affected
  evidence; prior failed logs are preserved. The deterministic128/129 race case passed.
- Final cargo sqlx prepare -- --all-targets --locked --offline followed by offline all-target
  clippy -Dwarnings now running, with explicit own URLs. Source digest must remain16ab2bf5.
- No B1 independent code verdict yet; no phase acceptance/integration/commit.

### Independent B1 round1 changes required

- FIXAPP-P03-B1-R1-MAC02 reviewed entire batch at16ab2bf5; one Medium finding
  FIXAPP-P03-B1-F1-MAC02: compensation WorkflowGuard after an IO handler reenters candidate
  confirmation and becomes Ok(Deferred). G1 r4 remains valid; implementation propagation defect.
- SQLx prepare -- --all-targets --locked --offline PASS, no metadata/source delta;
  offline all-target clippy -Dwarnings PASS, unchanged frozen digest. Logs retained.
- Added real-DB regression forcing completion guard then a distinct compensation guard; asserts
  typed original compensation failure, one handler/attempt, rollback and error metric. Running
  it against unfixed production code first, then correcting internal error-origin routing.

### F1 compensation regression/correction

- Red regression reproduced the actual reported failure on uncorrected worker code:1 selected,
  failed with `expect_err: Deferred`, exit101; logs/mac02-compensation-regression-red.log.
- Author correction keeps error origin private to worker orchestration: candidate errors are
  eligible for the existing once-only confirmation; post-handler errors (including failed
  compensation) propagate directly with their original AppError. No port/schema/gated-decision change.
- New sibling regression module keeps the existing guard-test file navigable. Two real-DB cases
  force guarded completion then guarded or ordinary08006 compensation failure; assert original
  cause, one candidate capability visit/handler/attempt, transaction rollback and error metric.
  Existing permanent/transient completion and competing-owner coverage remains.
- Focused guard green check now running; final affected union/fmt/offline compilation/lint/metadata
  checks will follow for this shared worker delta, then a single correction recheck by finding ID.

- Initial F1 green attempt hit compile-only fixture mismatch: polling helper accepted Scripted
  concretely while the regression wraps it to count candidate entries. Retained compiler output
  logs/mac02-compensation-and-guard-green.log. Generalized only that test helper over its existing
  WorkflowHandler bound; traced callers across seven files, all retained in the workflow union.
  No production port change. Green rerun log mac02-compensation-and-guard-green-r2.log.

## Root acceptance — FIXAPP-P03-B1 (2026-11-04)

- ACCEPTED IN PHASE for reviewed digestba772625d27ac6dfa497e3798067e0665d09ebfff4081d9b0dd02ef724cf1440,
  base/HEAD48626a1240f032ca3b99372bd3f5a05eb08c0c66; exact exclusion only this phase docs directory.
- Independent PASS FIXAPP-P03-B1-R2-MAC02 saved after whole-B1 R1 and F1 correction recheck.
  FIXAPP-P03-B1-F1-MAC02 fixed/closed by B; no unresolved actionable defects. Root recomputed
  digest and coordinationr13 pin before acceptance: identical. G1 continuity R5 remains valid.
- Final affected union1104 passed/0failed/1ignored; all filters nonzero. Offline all-target
  compilation/SQLx prepare, clippy-Dwarnings, formatting, whitespace PASS; current summary
  logs/mac02-f1-final-check-summary.txt. Earlier failures and red regression retained.
  Repository-wide full suite intentionally not run per I004.
- Source content fingerprint5454182be91ee687b3fcabcd8d472536c669f98624edbae0ccb03db0346ccf28
  over1465 files, manifest logs/mac02-accepted-content-manifest.json, will verify routine staging/
  commit changes only Git representation, not reviewed source. Review-base/start remains48626a1.
- Authorized next action: commit accepted p03 source, own records and evidence on fix-app/p03;
  no push. NOT INTEGRATED. p04 remains queued; downstream p06/p08 require reviewed p03 integration.

- Commit preparation: forcing ignored *.log evidence into the phase records exposed console
  progress carriage returns/SQL table padding/blank final lines in git diff--cached--check.
  These are captured log data, not source formatting. Preserved their exact bytes and added
  phase-doc-only .gitattributes whitespace classification for logs/*.log (including subdirs).
  Implementation checks remain unchanged; source fingerprint excludes this workflow record.
  Diagnostics retained in logs/mac02-staged-log-whitespace-diagnostics.txt.
