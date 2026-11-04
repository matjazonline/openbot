# FIXAPP-P03 agent review findings

Common context for the findings below, unless a section says otherwise:

- Workflow `implement-opus-reviewed-2_2`; task record `TASK.md`, review record `REVIEW.md` (same
  directory); batch: phase p03 (plan 03, batches 1–4); stage: architecture gate G1.
- Review round: `FIXAPP-P03-G1-R1` in `agent-review-notes.md`. Reviewer: Opus 5.5 / xhigh.
- Phase p03, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p03`, branch `fix-app/p03`.
- Reviewed: decision revision r1 of TASK.md "Architecture gate G1" (TASK.md sha256
  `61469d8e708e39f1d467aac611f99570f3ce867f94ef56a2cdd8119ddfc6e0a7`) against code at HEAD
  `48626a1240f032ca3b99372bd3f5a05eb08c0c66`, digest
  `c325eea2f3e71d55bbb4f3c5acdfb0df01515171e063db16d2366218d4aa3462` (exclusion
  `plan/fix-app/phases/p03/docs`).
- All findings originate in the gated decision text (no implementation exists yet). Evidence is
  code inspection unless a log is cited.

---

## FIXAPP-P03-G1-F1 — Guard-failure transaction misclassifies a lost-lease race that a competing retirement settles first

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R1. Decision: D1 (guard-failure transaction, steps 2–3).
- Classification: **actionable defect**, severity **High**, category correctness / concurrency.
- Evidence:
  - D1 step 2 defers only while the scope job is still `processing` (expired lease, or a live lease
    with another fence). It has no rule for a claim that has *already been settled by someone
    else* between the failed COMMIT and the failure transaction.
  - The interleaving is the expected one, not a corner case. `complete_on` takes the run lock first
    (`src/adapters/persistence/workflow/completion.rs:78-80`), and so does expired-lease
    retirement (`lease.rs:273-281` `lock_scope`, "Run-first ordering serializes claims, renewals,
    cancellation and progression"; called from `retire_expired_io`, `lease.rs:391-418`). When the
    completion's COMMIT is slow enough to trip "workflow completion lost ownership before commit"
    (`check_workflow_fenced_completion`, the exact case D1's rationale names), a retirement that
    discovered the expired lease is already queued on the run lock. It acquires the lock the moment
    the COMMIT fails, before the worker can open a new connection and begin the failure
    transaction.
  - Retirement then leaves one of these states (`recovery.rs:185-200` `settle_run`, `:203-215`
    `finish_job`):
    - job `pending` and run `running` (RetryAt);
    - job `failed` and run `waiting/reconciliation` (Reconcile);
    - job `failed` and run `failed` (final).
    In the first two states D1's step 1 sees a live run, step 2 sees a job that is not
    `processing`, and step 3 fails the run as `guard_violation:check_workflow_fenced_completion`.
    The result:
    - A retry that recovery had scheduled is dropped.
    - A run parked for reconciliation of an uncertain effect is failed instead.
    - The run becomes not retry-safe (retirement `guard`).
  - The same gap applies to `check_workflow_fenced_retirement` "lost ownership" trips on the
    failure path of `complete_io`.
  - D1 also never checks that the scope execution is still current. Unlike `expire_on`'s
    `associated` check (`maintenance.rs:22-27`, which requires `execution.completed_at IS NULL`),
    the transaction can write `terminal_execution_id = scope.execution` for an execution another
    owner has already completed.
  - Impact: this is the very time race D1's invariants say must keep its existing owner ("time-based
    refusals keep their existing owners; a stale worker never fails a run another worker owns").
- Required change (decision level):
  - When `fence` is `Some`, defer unless the job is still `processing` under exactly that fence
    (worker, generation, attempt) with a live lease. A claim settled by another owner means that
    owner decided it.
  - In every case, also require the scope execution to be incomplete (`completed_at IS NULL`,
    `successor_execution_id IS NULL`). Optionally require the job's `attempt_ordinal` and
    `retry_count` to be unchanged from what the failing operation used.
  - Add a competing-claimant test, as root `AGENTS.md` requires for a new concurrency protocol:
    1. Hold a completion past its lease so it trips lost-ownership at COMMIT.
    2. Let a retirement queued on the run lock commit first.
    3. Prove the failure transaction returns `Deferred` and leaves the retired state intact.
    Also add a cancel-versus-guard-failure race.
- Instruction lesson: the gate stated an invariant, but its verification plan did not enumerate
  which actors compete for the run lock. Candidate (scope: architecture gates that add a
  compensating transaction after a failed one): "list every actor that can acquire the same leading
  lock between the failed commit and the compensating transaction, and state the outcome for each."
  This is a hypothesis drawn from this single case.
- Resolution history:
  - 2026-11-01: open.
  - 2026-11-01, recheck FIXAPP-P03-G1-R2 (TASK.md r2 sha256 `bb318b3d…a7`): **fixed (decision level).** r2 adds confirmation by re-execution (the retirement-first interleaving makes `complete_on`'s fence check refuse on re-run, so no guard), exact-fence deferral (rule 5, `fence = Some`: processing under exactly that fence with a live lease, else `Deferred`), scope-execution-incomplete deferral (rule 2), and competing-claimant tests (i) retirement-first and (ii) guard failure vs cancel. Implementation and tests are to be verified in batch review B1. A related circular-deferral gap in the fence-None rule is logged separately as FIXAPP-P03-G1-F9.

---

## FIXAPP-P03-G1-F2 — Guard violations raised inside handler-owned transactions never reach the worker wrapper; the C001 response is incorrect

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R1. Decision: D1 (worker policy, "C001 response").
- Classification: **actionable defect**, severity **High**, category requirements / architecture
  (cross-subsystem contract).
- Evidence:
  - D1 wraps only `WorkflowWorker::process` (`src/application/workflow/worker.rs:104-170`). Many
    workflow guard RAISEs fire at COMMIT of transactions that run *inside*
    `self.handler.execute(...)`. Their errors are turned into ordinary step failures before
    `process` sees them:
    - **Message publication and effect park.** `message_publish.rs:319-342` `park_on` runs inside
      the `PostgresActionDispatch` publication transaction.
      `check_workflow_fenced_completion`'s `effect_parked` branch, with the C001 limit, is checked
      at that COMMIT. So are `workflow_action_dispatch_commit_guard` ("workflow message park lost
      its fence", P0001, `init_schema.sql:2273`) and "workflow message action limit" (54000,
      `:3066`). Errors become `StepError::Dispatch`, then
      `action_step_failure(family, Outcome::Dispatch)`, which is **Retryable `message.unresolved`**
      (`src/application/workflow/message_runtime.rs:574-579`,
      `src/application/workflow/actions/failure.rs:99-112`).
    - **HTTP/MCP action dispatch and remote-entry guards.** Same mapping (`http_runtime.rs:153`,
      `mcp_runtime.rs:229`).
    - **Agent harness checkpoints and model-tool publication.** `guard_workflow_token_proposal`,
      `guard_harness_token_admission` → `require_workflow_token_progression` ("workflow token
      approval required"), and `message_runtime.rs:60-75` for model tools all fall into
      `src/adapters/harness/rig/workflow.rs:402-416` `failure`, whose `_ => (Retryable,
      "unresolved")` arm catches them.
  - So D1's C001 response is not what the code will do. D1 says "a refusal by the effect_parked
    branch at the limit ... is classified as guard `check_workflow_fenced_completion` → terminal".
    In fact that refusal surfaces as a retryable step failure, is retried within the attempt budget,
    and is neither typed nor counted. C001 requires p03's "guard-error classification must admit
    this park and the refusals it produces". Two of plan 03's named examples are not covered at all
    ("park lost its fence"), or only partly ("token approval required" when raised at dispatch,
    remote entry or harness admission).
  - Impact: the plan outcome, "a database guard violation becomes a typed, counted, terminal
    outcome", holds only for guards that trip in worker-level persistence calls. The rest stay
    disguised as provider-style `*.unresolved` retries. The `workflow_guard_violations_total`
    metric under-reports.
  - Choosing where this boundary sits changes the `WorkflowHandler` / `WorkflowHandlerOutcome`
    contract (`src/application/workflow/lease.rs:104-107`) and every handler's failure mapping. It
    is exactly the kind of cross-subsystem decision the gate exists to settle before dependent code
    is written.
- Required change (decision level): choose and record one of the following.
  - (a) **Handler-level guards are terminal too.** The shared mappings (`action_step_failure`, the
    harness `failure`, JEV, MCP) recognise `AppError::WorkflowGuard`, and the handler returns a
    distinct outcome. Examples: a `WorkflowHandlerOutcome::GuardViolation(v)`, or a
    terminal-without-routing failure. `execute_io` turns that outcome into `fail_run_on_guard`
    with the claim fence. It must not take the step's declared `error` route, because plan 03 says
    the run fails. Effect safety stays with `action_uncertainty::record`, as in D1 step 3.
  - (b) **Scope D1 explicitly to worker-level persistence calls.** Record that handler-internal
    guard errors stay retryable step failures bounded by the attempt budget. Correct the C001
    response accordingly, and add a test proving the effect-park backstop is bounded and observable.
  - Either way, list which guard functions can fire inside handlers and which outside.
- Instruction lesson:
  - Observation: the gate traced the guard error from SQL to `From<sqlx::Error>`, but not from
    `From<sqlx::Error>` to the code that consumes the `AppError`.
  - Candidate (scope: decisions that reclassify an error type): "trace every consumer of the
    reclassified error, including handler failure mappings with wildcard arms, and state the
    resulting behaviour per consumer".
- Resolution history:
  - 2026-11-01: open.
  - 2026-11-01, recheck FIXAPP-P03-G1-R2: **fixed (decision level), option (b).** Scope is limited to the worker's own persistence calls plus `complete_io`. Handler-internal guards become typed Retryable `<family>.guard_violation` / `agent.guard_violation` with diagnostic `guard <name>`, bounded by the attempt budget; the rationale given is fence-loss races. The C001 response is corrected. Worker-level and handler-level guard lists are recorded. A minor accuracy point about receipt consumption is logged as optional FIXAPP-P03-G1-F11.

---

## FIXAPP-P03-G1-F3 — Guard failure leaves live child runs running; "same as deadline expiry" is not equivalent

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R1. Decision: D1 step 3 ("Children are not cascaded").
- Classification: **actionable defect**, severity **Medium**, category correctness / architecture.
- Evidence:
  - A runtime child inherits its parent's absolute deadline:
    - `admission_write.rs:61-66` `RunDeadline::Inherited`;
    - `children.rs:202-203`: "a child can never outlive its parent";
    - `children.rs:273`.
    At deadline expiry, every live descendant is therefore expired at the same instant.
  - A guard failure ends the parent early. Under D1 a live child keeps executing until that
    inherited deadline, possibly hours or days later. Meanwhile it keeps doing several things:
    - sending messages;
    - calling tools and models;
    - debiting the shared root budget;
    - creating its own children.
    All of that happens on behalf of a run that has already failed.
  - D1 also cancels the parent's open `child_run` waits. Cancellation finds the cascade through
    exactly those waiting `child_run` waits (`controls.rs:173-197` `cancel_tree_on`), so after a
    guard failure the orphaned children can no longer be cancelled through the parent.
  - Cancellation does cascade to live descendants. A guard failure, which is an abnormal early end
    like cancellation, is closer to cancel than to deadline expiry.
- Required change (decision level):
  - Either cascade to live descendants in the same transaction (reuse the `cancel_tree_on`
    traversal, so the children end `cancelled_by_parent`; parent before child, as today),
  - or record a correct justification for leaving them running and the operator path to stop them.
  - Note the D6 interaction: cancelling a child's token-allowance wait sets the root's
    `token_refusal`.
- Instruction lesson: an equivalence argument ("same as X") was accepted without checking the
  precondition that makes X safe (here, the inherited deadline). No instruction change proposed
  beyond the F1 candidate.
- Resolution history:
  - 2026-11-01: open.
  - 2026-11-01, recheck FIXAPP-P03-G1-R2: **fixed (decision level).** r2 rule 6 cascades to live descendants through the waiting `child_run` waits using the `cancel_tree_on` traversal (parent before child, `cancelled_by_parent`, run actor), before closing the run's waits. The lock order matches cancellation. Verify in B1 that only the children's subtrees are cancelled (not the parent via `cancel_run_on`), and that the D6 interaction is covered.

---

## FIXAPP-P03-G1-F4 — "A guard failure is never retry-safe" is false when the scope job was pending

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R1. Decisions: D1 ("Schema for D1") and D2 (retry edge).
- Classification: **actionable defect**, severity **Low**, category correctness.
- Evidence:
  - `maintenance::retire_jobs` closes an attempt and increments `retry_count` only for
    `processing` jobs. A `pending` job is set to `failed` without closing an attempt or touching
    `retry_count` (`maintenance.rs:58-79`).
  - `attempt_ordinal` is generated as `retry_count + continuation_count + 1`
    (`init_schema.sql:3372`). For a pending job, `workflow_control_retry_safe`'s
    `attempt_number = attempt_ordinal - 1` (`init_schema.sql:2818-2847`) therefore points at the
    previous attempt.
  - Example: an IO step whose attempt 1 failed retryable and safe, now pending with
    `retry_count = 1`, trips a guard at `claim_io` (`fence` None, Step site). Its last attempt has
    retirement `live`, safety `safe` and an allowed code, so `workflow_control_retry_safe` is
    TRUE. `control_retry::retry_on` (`control_retry.rs:18-36`) then accepts the retry. D2's
    trigger accepts the `failed → running` edge on the same predicate.
  - This contradicts D1's stated invariant, which D2's retry-edge rule relies on. Each retry can
    re-trip the guard and fail again without consuming `retry_count`.
- Required change:
  - Either make the invariant true. For example, exclude runs with a `guard_violation:%` audit
    event from `workflow_control_retry_safe`, or have the guard failure of a pending job record an
    attempt-level fact the predicate already rejects.
  - Or drop the claim and state when a guard-failed run is retryable.
  - Add a test for the pending-job case.
- Instruction lesson: cause is a missed code path in a stated invariant. No instruction change
  supported by one case.
- Resolution history:
  - 2026-11-01: open.
  - 2026-11-01, recheck FIXAPP-P03-G1-R2: **fixed (decision level).** `workflow_control_retry_safe` gains `NOT EXISTS` for a `guard_violation:%` run event, inherited by the D2 retry edge and by `check_workflow_control_retry`; test (iii) covers the pending-job case. Verify in B1.

---

## FIXAPP-P03-G1-F5 — Wait-deadline time races are not deferred

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R1. Decision: D1 step 2.
- Classification: **actionable defect**, severity **Low**, category correctness.
- Evidence:
  - `check_workflow_wait_commit` (`init_schema.sql:406-432`; deferred,
    `workflow_wait_commit_deadline`, `:9061`) compares the **wait's own** deadline with
    `clock_timestamp()` at COMMIT. It applies to an INSERT of a decision wait, or of an event wait
    carrying `notification_intent` (every `park_on`, `waits.rs:179-245`).
  - `park_on` checks `deadline <= now` at statement time only. A park whose wait deadline (not the
    run's) passes during COMMIT trips "workflow wait deadline reached before commit".
  - D1 defers only run-deadline and lease races, so this trip becomes a terminal guard failure.
    Today the retried park sees the deadline and takes its owned path: the `InvalidInput` settle
    for event waits (`waits.rs:220-223`), or the decision timeout route (`wait_commit.rs:145-191`
    `decision_timed_out`, which can take a declared `error` route).
  - D1 lists only two of the clock-consulting guard families. These guards also consult the clock:
    - `check_workflow_wait_commit`;
    - `workflow_message_action_capture`;
    - `workflow_message_reopen_guard`;
    - `workflow_action_evidence_commit_guard`;
    - `workflow_action_claim_budget_refusal_guard`.
    D1 should classify each one as covered by the lease or run-deadline rule, deferred on its own
    clock, or accepted with a stated reason.
- Required change:
  - Add the wait deadline to the time-race deferrals: an open wait of the scope execution whose
    deadline has passed.
  - For the rolled-back INSERT case, either defer when the step is a wait or decision step and the
    run lock shows no open wait, or document the residual risk with a bound.
  - Record the classification of the other clock-consulting guards.
- Instruction lesson: no change proposed; this is one instance of F1's "enumerate competing owners"
  lesson.
- Resolution history:
  - 2026-11-01: open.
  - 2026-11-01, recheck FIXAPP-P03-G1-R2: **fixed for the race (decision level).** Confirmation by re-execution handles wait-deadline races, because the re-run takes the timeout or invalid-input path. A documented residual remains: an event-wait INSERT that crosses its own deadline in two consecutive COMMIT windows; it is accepted, since both outcomes fail the step. The new deferral rule 4 that came with this fix is circular when the failing operation is the wait-timeout owner; see FIXAPP-P03-G1-F9.

---

## FIXAPP-P03-G1-F6 — Verification plan gaps for D1, D2 and D4

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R1. Decisions: D1, D2, D4 (verification sections).
- Classification: **actionable defect** (verification), severity **Low**.
- Evidence and required changes:
  1. **Tests pin empty constraint names.** D1 says "no message text or SQLSTATE changes, so
     existing tests asserting codes/messages keep their meaning". But 10 test files pin the guard
     errors' constraint field as empty or None, so they break on the new constraint names. Examples:
     - `action_reconciliation_high_bounds_native.rs:103` `assert!(native.constraint().is_none())`;
     - `action_reconciliation_*_validate.sql`, `*_fixture.sql` `native_constraint=''`.
     Plan to update them to assert the new `workflow_guard:<name>`, a stronger assertion, rather
     than deleting them.
  2. **State-witness attack fixture.** `action_reconciliation_state_witness_fixture.sql:67-69`
     deliberately moves a run `failed/cancelled → waiting`. The D2 trigger now refuses that first.
     Record a disposition that keeps the witness guard exercised: a still-reachable attack, or a
     trigger-disabled fixture with justification. Do not weaken the assertions.
  3. **Raw fixtures violate the new CHECK.** At least 9 raw fixtures set `failed` or `succeeded`
     without `terminal_execution_id`, for example:
     - `lease_adversarial_tests.rs:22`, `:244`;
     - `child_runtime_tests.rs:161`;
     - `run_finished_at_tests.rs:98-164`;
     - `wakeup_guard_tests.rs:121`;
     - `maintenance_guard_tests.rs:88`.
     Add an inventory step. Fixtures must reach states through allowed edges, without bypassing
     the trigger.
  4. **Matrix confounding.** The D2 matrix must assert that a refusal carries constraint
     `workflow_guard:guard_workflow_run_transition`. Otherwise a refusal by
     `workflow_run_terminal_execution_shape`, `workflow_run_waiting_shape` or another trigger
     passes as "SQL refuses". Build each UPDATE so that only the transition guard could refuse.
     Include an *accepted* `failed → running` case with retry eligibility, not only the refusal.
  5. **D4 test is vacuous.** `renew_io` runs `lock_fence` first (`lease.rs:79`), so a
     stale-generation renewal through `renew_io` passes with or without the new predicate. Exercise
     the UPDATE statement itself, for example factored into a helper and called with a stale
     fence.
  6. **Poll check.** Plan 03's test says the failed run "is not reclaimed on the next poll". D1(a)
     checks only a second `process`. Also assert `poll_work` returns no candidate for the run on
     two consecutive polls without advancing time (`src/application/AGENTS.md` poison-batch rule).
- Instruction lesson: the gate claimed tests "keep their meaning" without grepping for assertions
  on the changed field. Candidate (scope: decisions that change error fields): "grep tests for
  assertions on every error field the change touches (code, message, constraint, schema, table)".
- Resolution history:
  - 2026-11-01: open.
  - 2026-11-01, recheck FIXAPP-P03-G1-R2: **fixed (plan level).** All six items are adopted in r2's "F6 verification additions". Item 2's disposition is deferred to implementation, with its constraint stated (the witness guard must still be exercised; assertions not weakened). Verify in B1. One site test is still missing (`complete_io`); see FIXAPP-P03-G1-F10.

---

## FIXAPP-P03-G1-F7 — Optional: decide typing for non-RAISE and non-"workflow"-named guards

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R1. Decisions: D1 (identification), D2 (failure modes).
- Classification: **optional suggestion**, severity Low, category architecture.
- Evidence:
  - D2 says "an unknown writer making a disallowed move now fails with a typed guard error". That
    holds for the trigger, but a writer that violates the new CHECK
    `workflow_run_terminal_execution_shape` raises 23514 with an unprefixed table constraint name.
    The error stays `AppError::Database` and is retried until the deadline. Options: make the shape
    rule part of `guard_workflow_run_transition`, raised as a RAISE with the guard name, or map
    named workflow-table CHECKs.
  - Guards that fire on workflow-owned rows inside worker transactions, but whose function names
    lack "workflow", stay untyped. Examples: `preserve_background_task_queue_identity`,
    `guard_jev_invocation_update`, `guard_model_usage_task`, `guard_model_usage_update`,
    `guard_harness_*`, `require_harness_owner`. List these as deliberate exclusions or inclusions.
- Resolution history:
  - 2026-11-01: open (optional).
  - 2026-11-01, recheck FIXAPP-P03-G1-R2: **declined with reason, accepted.** The CHECK stays untyped because it is the every-write invariant, writers set the column in the same UPDATE, and the matrix and shape tests catch writer bugs. The exclusion list is recorded (D2 r2 note). The "shared with legacy tasks" reason covers `preserve_background_task_queue_identity`. Closed.

---

## FIXAPP-P03-G1-F8 — Optional: smaller D1/D4 completeness items

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R1. Decisions: D1, D4, D7.
- Classification: **optional suggestion**, severity Low, category architecture / records.
- Items:
  - **D4.** Fence the renewal completely: also bind `company_id` and `attempt_ordinal`, matching
    `finish_job` (`recovery.rs:213`) and persistence `AGENTS.md` "require it in every renewal".
  - **D1.** Name the exhaustive `AppError` matches the new variant reaches:
    - `inbound_event_worker.rs:658-673` `classify_commit_error` (retry versus routing);
    - `app_error_impl.rs`.
    Also say how the run view shows the guard reason when no attempt was closed (pending or parked
    job): only the `guard_violation:<guard>` audit event carries it.
  - **D7.** Add cross-phase notes:
    - For p08, which constrains `task_attempts` stop reasons: the new retirement value `guard`,
      failure code `workflow.guard_violation` and its `stop_reason` (NULL).
    - For p06: the full list of BEFORE UPDATE triggers on `workflow_runs`. It includes
      `workflow_run_snapshot_immutable`, which plan 03 does not list.
- Resolution history:
  - 2026-11-01: open (optional).
  - 2026-11-01, recheck FIXAPP-P03-G1-R2: **fixed.** D4 r2 fences `company_id`, `workflow_execution_id`, `queue_kind`, `status`, generation, worker, `attempt_ordinal` and live expiry. The consumer list names `app_error_impl` and `inbound_event_worker::classify_commit_error` (`Internal`). The run view's guard carrier is named. p06 notes list the BEFORE UPDATE triggers in correct byte order, the new CHECK and the retention caveat; p08 notes are complete. Closed.

---

## FIXAPP-P03-G1-F9 — Deferral sends the failure back to the owner that just failed (run-expiry and wait-timeout paths)

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R2 (correction recheck). Decision: D1 r2, failure-transaction
  deferral rules 4 and 5 (TASK.md sha256 `bb318b3df60479a1d38f44abd9dd05d69a49918c2f585c1859c790ada448f9a7`).
  The rule-5 part was already in r1 step 2 and was missed in round FIXAPP-P03-G1-R1.
- Classification: **actionable defect**, severity **Medium**, category correctness / liveness.
- Evidence:
  1. **Run expiry (rule 5, fence None).** The text reads: "other sites require `pending` or
     `completed`; anything else … → `Deferred`". Site `RunExpiry` is one of those "other sites".
     - A claim in flight when the deadline passes is `processing`, and its lease is capped at the
       run deadline (claim `lease_claim.rs:168` and renewal `lease.rs:85`, both `LEAST(…, run.deadline)`). So at expiry it is normally
       `processing` with an expired lease.
     - After the deadline nothing else can settle it:
       - polling classifies every incomplete execution of a live, past-deadline run as `deadline`
         before `expired` (`polling.rs` `CANDIDATES_SQL` CASE order), so no `ExpiredLease`
         candidate is ever produced;
       - a live holder's `finish_job` (`recovery.rs:213`) needs a live lease unless the cause is
         expiry.
     - So if `expire_on` trips a guard deterministically (confirmed by the re-run), the failure
       transaction returns `Deferred` on every poll, past the deadline, indefinitely. The only
       visible sign is a "deferred" counter.
     - r1 had the same gap: its step 2 deferred "processing with an expired lease and site ≠
       `LeaseExpiry`".
     - The guard-failure writes differ from `expire_on`'s (retirement `guard`; waits `cancelled`
       rather than `expired`), so failing the run would often succeed exactly where expiry tripped.
       The deferral blocks that fallback.
  2. **Wait timeout (rule 4).** The rule defers when "an open wait of the scope execution with
     `deadline <= clock_timestamp()` and site ≠ `RunExpiry`". For a `Wait` candidate (site `Step`)
     the failing operation *is* the wait-timeout owner:
     - `resume_on` → `decision_timed_out` / `expire`, then `decisions::tokens::fail`,
       `approvals::fail` or `route_parked_failure` (`wait_commit.rs:44-110`, `:145-191`).
     - A deterministic guard in that path is confirmed, then deferred to "wait timeout owns it"
       (itself). This repeats every poll until the run deadline: the "spins until the deadline"
       behaviour plan 03 removes.
     - Confirmation already handles the wait-deadline *race* (the re-run sees the deadline and
       takes the timeout path). So rule 4 adds nothing for `Wait` sites. For park sites the
       rolled-back INSERT leaves no open wait, so rule 4 does not apply there either.
- Required change (decision level):
  - Treat the owner's own site as the owner. With `RunExpiry`, admit any job state: expiry's
    `retire_jobs` already settles `processing` jobs with or without a live lease.
  - Restrict rule 4 to sites that are not the wait's own resume, either by introducing a
    `WaitExpiry` site for `Wait` candidates analogous to `RunExpiry`, or by dropping rule 4 and
    relying on confirmation.
  - Add tests in which the expiry path, and separately the decision-timeout path, trip a forced
    guard deterministically. Each must end in one guard failure, with no candidate on the next two
    polls.
- Instruction lesson: same root as F1's candidate (enumerate the owners for every deferral). The
  extra point here: a deferral rule must name an owner that is *different* from the operation that
  failed, or it becomes a loop. Candidate wording, for compensating-transaction gates: "for each
  deferral, name the other actor that will settle the state and the candidate that reaches it".
  This review missed the rule-5 instance in R1; recorded in R1's follow-up.
- Resolution history:
  - 2026-11-01: open.
  - 2026-11-01, recheck FIXAPP-P03-G1-R3 (TASK.md r3 sha256 `bb5e48e4…d864`): **fixed (decision level).** Rule 4 is dropped. Clock races on resume are covered by confirmation, and a timeout path that repeatedly trips is failed. Rule 5 at `RunExpiry` admits any job state, consistent with `retire_jobs` settling `processing` (live or expired) and `pending` jobs. Rule 3 is kept and is not circular: `resume_on` calls `maintenance::expire_on` itself (`wait_commit.rs:8`, `:48`), but after a rule-3 deferral the next poll classifies the work as `deadline` (`CANDIDATES_SQL` CASE order) and reaches `expire_run` at site `RunExpiry`. Rechecked every remaining deferral for a distinct settling owner, using the r3 final list (1), (2), (3), (5):
    - rule 2 has no live candidate left to loop on;
    - a foreign live claim is settled by its holder;
    - an expired foreign claim is settled by the `ExpiredLease` candidate, or by expiry after the deadline;
    - a job `failed` by a competing retirement means the run is failed (`Inactive`) or in `waiting/reconciliation`, which has no job candidate.
  Forced-guard tests are added on the expiry and decision-timeout paths. Verify in B1.

---

## FIXAPP-P03-G1-F10 — `complete_io` site: confirmation contract and test are not explicit

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R2. Decision: D1 r2 "Confirmation" and r1 "fence".
- Classification: **actionable defect** (verification and contract precision), severity **Low**.
- Evidence:
  - r2 says "`process` re-runs the candidate once; `execute_io` re-runs `complete_io` once". r1
    says the fence is set when the guard came from `complete_io` ("handled inside `execute_io`").
  - The decision does not state that a *confirmed* `complete_io` violation bypasses the
    candidate-level re-run. If the wrapper re-ran the `Job` candidate after `execute_io` returned
    the error, the re-run would reach `claim_io` (`worker.rs:183-185`). Our own claim is still
    `processing` with a live lease, so `claim_io` returns `None` and the result is
    `WorkDisposition::Deferred`.
  - The violation would then be dropped. It degrades into a lease-expiry retirement and a fresh
    handler execution on every attempt, never typed or counted.
  - This is the most important worker-level site (fenced completion, token completion guard,
    progression guards). Yet D1's tests (a)–(e) and the F6 additions inject only at a pure step's
    commit.
- Required change:
  - State that `execute_io` acts on a confirmed `complete_io` violation itself, calling
    `fail_run_on_guard` with the claim fence, or returns a disposition the wrapper does not re-run.
  - Add a `workflow_guard_error_*` case at the `complete_io` site. It must assert exactly one claim
    and one handler execution, run `failed` with `terminal_execution_id`, the attempt retired as
    `guard`, and no candidate on two polls.
- Instruction lesson: cause unknown (wording precision); no change proposed.
- Resolution history:
  - 2026-11-01: open.
  - 2026-11-01, recheck FIXAPP-P03-G1-R3: **fixed (decision level).** `execute_io` re-runs `complete_io` once with the same result. If a guard trips again, it calls `fail_run_on_guard` (site `Step`, `fence = Some(claim.fence)`) and returns the disposition; the error is never re-raised, so no IO candidate is re-run after its handler ran. `claim_io` guard errors propagate, and the re-run claims afresh, since the failed claim rolled back. The IO-site test is added. Verify in B1, including that this path increments the same counter and logs.

---

## FIXAPP-P03-G1-F11 — Optional: receipt-consumption guards are absorbed by per-action backoff, not the terminal policy

- Date: 2026-11-01. Round: FIXAPP-P03-G1-R2. Decision: D1 r2 guard lists.
- Classification: **optional suggestion**, severity Low, category records accuracy.
- Evidence:
  - r2 lists `workflow_message_reopen_guard` and `workflow_message_delivery_transition_guard`
    "(receipt consumption)" under the worker-level terminal policy, and includes
    `consume_message_receipts` in scope.
  - `consume_page` (`message_receipts.rs:27-62`) catches every per-action error. It logs a warning
    and calls `record_consume_failure`, which backs off and marks the action `stalled` after 5
    failures (`:213-233`). Only errors from the initial query or the backoff write propagate.
  - So receipt-consumption guard violations are bounded and observable (the action is `stalled`),
    but they do not fail the run. `workflow_message_reopen_guard` does reach the worker level
    through `continue_delivery` in `resume_wait`.
- Suggestion: correct the list. Either note that receipt consumption stalls the action, or decide
  to propagate `WorkflowGuard` from `consume_one`.
- Resolution history:
  - 2026-11-01: open (optional).
  - 2026-11-01, recheck FIXAPP-P03-G1-R3: **adopted.** The r3 guard list moves `workflow_message_reopen_guard` and `workflow_message_delivery_transition_guard` to "bounded by receipt-consumption backoff/stall", with no propagation. Note that `workflow_message_reopen_guard` also fires at worker level through `continue_delivery` in `resume_wait`; there the site decides, per r2's "the site decides the policy". Closed.


## FIXAPP-P03-G1-R5 — continuity disposition (2026-11-04)

No new concrete findings for unchanged decision r4 (SHA256 `5753375da4d526406487a78080c9deecdbd090b8dc16afa947ab7e398bbf2e25`) under coordination r13. Historical F1–F11 dispositions are preserved; decision-level closure does not close B1 implementation/verification obligations. See [G1-R5 notes](agent-review-notes.md#fixapp-p03-g1-r5--mac02-architecture-continuity-validation-pass) and [identity evidence](logs/G1-r5-review-verification.log). Reviewer `/root/p03_reviewer`, Astra/high requested, independent context; no source or DB mutations.


## FIXAPP-P03-B1-F1-MAC02 — IO compensation guard error re-enters the candidate and is swallowed

- Date: 2026-11-04; workflow `implement-astra-reviewed-2-2`; phase p03 whole B1; round [FIXAPP-P03-B1-R1-MAC02](agent-review-notes.md#fixapp-p03-b1-r1-mac02--whole-b1-review-changes-required). Reviewer `/root/p03_reviewer`, Astra/high requested, independent context; actual runtime metadata unavailable. [TASK](TASK.md), [REVIEW](REVIEW.md).
- Artifact: worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03`, branch `fix-app/p03`, review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`, digest `16ab2bf541275753b6b0c2dfdef77dee8359af0e65ba6723e471f646beddf31b`, sole exclusion `plan/fix-app/phases/p03/docs`; normative decision G1 r4, current coordination r13.
- Classification: **actionable defect, Medium**, correctness/error propagation; implementation defect against the accepted gate, not a newly required architectural decision.
- Evidence: `src/application/workflow/worker.rs:250–261` returns the result of `fail_on_guard` directly after two refused IO completions. At `:290–293`, that helper returns the compensation error unchanged. If compensation itself returns `AppError::WorkflowGuard`, `process` at `:124–128` interprets it as a first candidate-operation guard and calls `process_once` again. The handler has already executed. `execute_io` at `:220–223` then calls `claim_io` again. With the original live claim, `lease_claim.rs:40–42` sees `processing`, `recover_expired` returns false (`:144–148`), and the claim returns `None`. Consequently the outer call returns **Ok(WorkDisposition::Deferred)** rather than the compensation error. The counter/log records outcome=error, but the caller loses the failure and the completion path incorrectly re-enters candidate confirmation. This contradicts r4 D1's completion-only confirmation/no IO-candidate rerun and its requirement that a failed compensating transaction propagate.
- Reproduction recipe (inspection-derived, not executed by B): extend the existing isolated IO forced-guard fixture so the trigger raises on both `NEW.status='completed'` and `NEW.status='failed'`. Both completion transactions and then the guard-compensation transaction roll back. Assert `process` returns the compensation error, one handler call, one claim invocation (instrument the port or test claim path), one compensation invocation/error metric, no committed guard-failure event, and unchanged processing claim. Current code returns `Ok(Deferred)` after a second claim invocation. Also exercise an ordinary non-guard compensation error so the original cause remains visible.
- Required correction: preserve the distinction between guard errors eligible for candidate confirmation and errors after IO handling/compensation has already begun. Propagate the latter without passing them through candidate confirmation, while retaining completion-only confirmation, exact claim fencing, and the error metric/log. Add the focused regression above; retain existing permanent/transient completion and competing-owner tests.
- Instruction lesson: historical G1-F10 and G1-R4 explicitly required no post-handler candidate rerun, including compensation errors. Candidate for future reviews: when a wrapper retries by error type, trace errors from its recovery/compensation path through that same wrapper. No skill/source instruction edits authorized.
- Resolution history: 2026-11-04 **open**; no implementation correction reviewed yet. The passing 1102-test affected union does not cover this error-of-compensation path.


### FIXAPP-P03-B1-F1-MAC02 — correction recheck, 2026-11-04

- Round FIXAPP-P03-B1-R2-MAC02; Reviewer `/root/p03_reviewer` (independent Astra/high requested; actual runtime metadata unavailable). Source digest `ba772625d27ac6dfa497e3798067e0665d09ebfff4081d9b0dd02ef724cf1440`, same base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`, sole exclusion `plan/fix-app/phases/p03/docs`; coordination r13 unchanged.
- **Fixed.** `worker.rs` now carries private `ProcessFailure::Candidate` versus `AfterHandler`. Claim/pre-handler errors convert to Candidate; complete-confirmed errors (including compensation) convert to AfterHandler at the IO boundary. Both outer confirmation matches retry only Candidate guard errors and unwrap AfterHandler errors unchanged. Non-guard errors also preserve their original value. No IO-candidate re-entry is possible through the corrected compensation path.
- [Actual red reproduction](logs/mac02-compensation-regression-red.log) fails because old code returns Deferred. [Green guard union](logs/mac02-compensation-and-guard-green-r2.log) passes 15/15, including new guard and ordinary 08006 compensation failures. Inspected tests prove original cause, one capability visit (before every claim path), one handler invocation/attempt, processing attempt retained by rollback, no committed failure event, and exactly one error metric. Existing permanent/transient completion and competing-owner cases pass alongside them.
- Reviewed helper genericization and module registration; caller trace confirms existing polling helper users remain under the same WorkflowHandler bound. The initial green compile failure from helper type mismatch remains in author evidence; it was corrected without changing runtime semantics.
- This closes the concrete defect. Artifact acceptance still waits for the already-running final affected union and final prepare/clippy/fmt evidence; final round note/REVIEW will record those results. No new findings so far and no G1 redesign.


### FIXAPP-P03-B1-F1-MAC02 — final verification confirmation, 2026-11-04

F1 remains **fixed/closed** at digest `ba772625d27ac6dfa497e3798067e0665d09ebfff4081d9b0dd02ef724cf1440`. Final affected union 1104 passed/0 failed/1 ignored, prepare/all-target compilation, offline clippy -D warnings, fmt and diff-check all PASS; source and coordination identities match on return. [R2 note](agent-review-notes.md#fixapp-p03-b1-r2-mac02--correction-recheck-and-whole-b1-artifact-pass) and [final summary](logs/mac02-f1-final-check-summary.txt). No new findings.
