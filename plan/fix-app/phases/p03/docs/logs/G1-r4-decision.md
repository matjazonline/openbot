# FIXAPP-P03 G1 — immutable resumed decision revision r4 (2026-11-04)

> No backward compatibility. Reset databases; no backfills, dual reads/writes, legacy decoders,
> compatibility shims, or staged rollouts.

## Identity and precedence

- Scope: all G1 decisions D1–D4 for whole phase B1; independent fresh validation before dependent
  edits. r4 consolidates the effective historical r3 decision without a technical change.
- Original requirements: `plan/fix-app/03-workflow-database-guards.md` and its README.
- Exact historical decision/evidence: `logs/TASK-paused-20261101.md`, G1 r1 with r2 then r3
  overriding only the named portions; REVIEW and agent-review-notes/findings remain authoritative
  historical evidence. The consolidated decision below is normative for resumed implementation.
- Phase start, review base and HEAD: `48626a1240f032ca3b99372bd3f5a05eb08c0c66`.
- Adopted implementation digest: `7b5299ca08b6dec63a7c84ef37d266e2a1c98103a838681ee0a1e6c721c9c979`;
  exclusions: **only** `plan/fix-app/phases/p03/docs`; `logs/resume-adopt-snapshot.log`.
  This matches the paused digest. Existing D1/D5/D6 and partial D2 changes are preserved.
- Workflow: implement-astra-reviewed-2-2, installed at
  `/Users/mac03/dev/openbot/.agents/skills/implement-astra-reviewed-2-2`.
- Manager: current root `/root`, BB thread `thr_gwccjxwzjt`, actual model/effort unavailable.
  Author: `/root/p03_author`, requested `gpt-6.1-sol`/`xhigh`; actual runtime metadata unavailable.
  Fresh independent reviewer: supplied as manager's direct child, Astra/high requested.
  Historical Opus author/reviewer identities and passes are preserved and are not fresh passes.
- Context compaction target 75%; effective threshold not exposed, durable checkpoints required.
- Authoritative coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md`, writer
  manager `thr_gwccjxwzjt`, revision 6. Immutable copy `logs/coordination-r6.md` (SHA below).
  I001–I004 r1 and C001/C002/C005/C007 r1 acknowledged; C006 r1 proposed outputs only.

## D1 — typed workflow guard errors and terminal guard failure

### Identification, mappings, and scope

- Every init-schema `RAISE EXCEPTION` in a function whose name contains `workflow` gains stable
  `CONSTRAINT='workflow_guard:<guard>'`, preserving SQLSTATE/message. Use function name unless an
  existing specific guard name exists. `validate_workflow_job_association` is excluded, retaining
  FK-shaped names. Deliberate exclusions: table CHECKs, already-named schedule guards, and guards
  shared with legacy tasks or reached only inside handlers (`guard_harness_*`, `require_harness_owner`,
  `guard_model_usage_*`, `guard_jev_invocation_update`, `preserve_background_task_queue_identity`).
- `From<sqlx::Error>` maps that exact prefix and a validated `[a-z0-9_]{1,63}` guard name to
  `AppError::WorkflowGuard(WorkflowGuardViolation { guard, message })`; invalid names stay Database.
  Timeout SQLSTATE 55P03/57014 retains precedence. HTTP returns the Database-equivalent 500 and
  logs `workflow_guard`; inbound commit classification stays Internal, not routed.
- Terminal policy applies to errors propagated from the worker's persistence calls: claim, pure
  advancement, park/resume, expiry/retirement, and IO completion. Handler-internal transaction
  guards stay Retryable, bounded by the attempt budget, because fence-loss races are expected there.
  Shared action and agent mappings use `<family>.guard_violation`/`agent.guard_violation` with
  diagnostic `guard <name>`, same safety/class as the former unresolved mapping.
- C001: running→waiting/effect is permitted. `check_workflow_fenced_completion`'s effect-park
  continuation refusal occurs at handler publication COMMIT; classify it as bounded retryable
  `message.guard_violation`. p02's Rust continuation refusal remains first; SQL is the backstop.
- Receipt consumption catches per-action errors with backoff and stalls after five failures;
  its `workflow_message_reopen_guard`/`workflow_message_delivery_transition_guard` do not propagate
  terminally. A reopen guard propagated through `resume_wait` does follow the worker policy.

### Confirmation and compensating transaction

- A guard returned by `process_once` triggers exactly one immediate rerun of that candidate. If a
  guard repeats (including a different guard), act on the second name. Re-execution sees moved
  clocks/current ownership and lets the established owner settle time/lease races.
- At `complete_io`, confirmation stays inside `execute_io`: retry only completion once with the
  same cloned `FencedWorkflowResult`; a repeated guard calls fail_run_on_guard with the claim fence
  and returns its disposition. Never rerun the IO candidate or its handler after handler execution.
  Guard errors during claim propagate to the candidate wrapper; the failed claim rolled back.
- `GuardFailure { scope, guard, site, fence }`, boxed-future port `WorkflowGuardFailure`.
  Sites: Step, RunExpiry, LeaseExpiry; fence Some only at IO completion.
- One READ COMMITTED transaction, leading lock order run→executions→jobs, matching existing owners:
  1. Lock run FOR UPDATE; non-live state → Inactive, no write.
  2. Scope execution completed or has successor → Deferred, no write.
  3. Run deadline passed unless site RunExpiry → Deferred; deadline polling owns settlement.
  4. Scope-job ownership: fence Some requires processing under exact worker/generation/attempt
     and live lease, else Deferred. Fence None: RunExpiry accepts any status; LeaseExpiry accepts
     pending or processing with expired lease; Step accepts pending/completed; else Deferred.
     There is no open-wait-deadline deferral: Wait is itself the timeout owner.
  5. Otherwise collect live children from waiting child_run waits and cancel each subtree before
     closing parent waits; existing parent-before-child traversal and cancelled_by_parent events.
     Retire parent's jobs with Guard (processing attempt failed/terminal,
     `workflow.guard_violation`, safety unknown, retirement guard; pending jobs failed); cancel
     waits and pending message deliveries with WorkflowExpired; run failed with no waiting_reason,
     terminal_execution_id=scope.execution; append `guard_violation:<guard>` and record action
     uncertainty. Return Failed. Error propagates; warn/error logs and metric outcome expose it.
- `workflow_guard_violations_total{guard,site,outcome}` increments for every handled outcome,
  including failure-transaction errors. Production worker gets engine monitoring; tests may omit.
- Schema admits `guard` retirement and the exact failed/terminal/code/unknown shape, with no
  successor, failed owning run and guard_violation event. `workflow_control_retry_safe` excludes
  every run with guard_violation event, including a pending-job failure without a newly closed attempt.
- Invariants: repeated disagreements terminate; a stale worker does not undo a competing owner's
  outcome; live children stop on early failure; terminal runs stay untouched; guard-failed runs are
  never control-retry safe. A failure transaction that itself fails remains observable/retryable.
- Accepted residual: an event-wait INSERT may cross its own deadline in both confirmation windows;
  guard failure and invalid-input settle both fail the step.

## D2 — one run transition table and terminal execution shape

- Same-state updates allowed. Other transitions:
  queued→running/waiting/succeeded/failed/cancelled;
  running→waiting/succeeded/failed/cancelled;
  waiting→running/failed/cancelled;
  failed→running (explicit eligible retry only);
  succeeded/cancelled→none. Never return to queued.
- Domain `RunState::stored_name` and `can_transition_to` express this table; Failed→Running is
  documented as subject to stronger persistence eligibility.
- `guard_workflow_run_transition` BEFORE UPDATE OF state, lexicographically before all current
  workflow_* BEFORE triggers including finished stamps. Mirror the table; failed→running requires
  OLD terminal_execution_id, NEW NULL, terminal job already pending (reopened earlier in retry
  transaction), and workflow_control_retry_safe. Deferred check_workflow_control_retry still
  requires the applied retry receipt at COMMIT. Refusal uses SQLSTATE23514 and constraint
  workflow_guard:guard_workflow_run_transition.
- Immediate CHECK `workflow_run_terminal_execution_shape`:
  terminal_execution_id non-NULL iff state succeeded/failed; NULL for queued/running/waiting/cancelled.
  This CHECK remains an untyped ordinary 23514 invariant on INSERT and UPDATE.
- Set terminal_execution_id in wait_commit::expire/fail in the state UPDATE; remove child_waits'
  redundant follow-up. Existing maintenance/recovery/decision_tokens/pending_recovery/batch_commit
  writers and retry clearing are inventoried and retained. Repair test fixtures to use allowed
  transitions and supply terminal id; do not bypass the new trigger.
- Every accepted transition-matrix update satisfies other shape constraints; refusal must name
  the transition guard, not a confounding CHECK. Include accepted eligible failed→running and
  refusal without eligibility. Preserve state-witness adversarial coverage using a reachable
  attack and document disposition rather than weakening assertions.

## D3 — root budgets owned by Rust admission

- Shared admission_write::insert_run inserts root budget and zero usage from compiled definition
  limits before inserting a root run, in the same transaction. Child runs do not get new root rows.
  Existing root_budget→run FK is deferred; run_budget→root_budget is immediate, proving presence.
- Token-policy encoding shares one helper with compilation so stored policy equals compiled policy.
- insert_workflow_budget_link drops all bytea/bundle decoding and root-row inserts; retain parent
  root lookup and run-to-root link insertion. A raw root run without a budget fails 23503.
- Remove provenance, sealed_legacy/frozen_v6 branches. Budgets require activations/model_calls/
  repetitions within existing compiled bounds; token policy is NULL or valid. Drop provenance
  conditions in debit_workflow_budget_receipt, workflow_action_reconciliation_budget_eligible,
  token_budget.rs and state-witness validate fixture.
- Inventory all raw-SQL root-run fixtures; insert budget/usage first using one test helper.
  No compiler_revision reference remains in migrations.

## D4 — lease renewal has a complete execution fence

- Renewal UPDATE predicates: company_id, workflow_execution_id, queue_kind=workflow,
  status=processing, generation, worker, attempt_ordinal, live expiry. Retain lock_fence and
  deadline cap. Factor the UPDATE so its own predicate is tested with stale fences, not merely
  the preceding lock_fence refusal.

## Verification, coordination, and implementation boundary

- Whole B1 includes preserved ungated D5 predicate deletion (no callers) and D6 test-first child
  token-cancellation fix (cancel no longer refuses shared root; expiry still does). Historical
  failing test and 30 token passes retained. Do not rerun the applied guard-naming script.
- D1: forced guard at pure, IO completion, run expiry with processing/expired lease, decision
  timeout; fail once with named event, terminal id, expected jobs/attempts and two empty polls.
  IO case proves one claim/handler call. Ordinary 08006 error remains retryable. Tests cover
  retirement-first versus stale fenced failure, concurrent cancel versus failure, pending job
  exclusion from retry, child subtree cascade, handler mappings, and stable constraint pins.
- D2: exhaustive Rust/SQL six-state matrix, explicit retry eligibility, cancellation finality,
  terminal shape and trigger order; control suites and repaired raw fixtures/state-witness attack.
- D3: compiled root limits/token policy equality, child root link without own budget, absent-root
  refusal; budget/token/control retry interfaces. D4: lease and capacity-renew tests plus direct
  stale-fence UPDATE test. C001 filters: message_park_continuation_, delivery_late_completion_,
  notification_stale_, schedule_claim_. Run workflow persistence suite for shared infrastructure.
- Reset owned dev/test databases after schema edits. cargo fmt --check, affected-target offline
  compilation, cargo sqlx prepare -- --all-targets (empty cache diff expected for runtime queries).
  Every filter must have nonzero tests. I004 supersedes full suite/scripts/check.sh; do not run them.
- C002 fixtures preserve company/owner atomicity; C005 run→binding FK and deferred history FKs
  remain intact. C007 preserve WorkflowGuard and p04 Forbidden semantics at integration.
- C006 r1 remains proposed output for p06/p08 until p03 acceptance/integration. Final records must
  identify trigger order/terminal shape and guard retirement/codes/events/token-refusal removal.
- Main e4273e8 has intervening database rename, migration-test serialization, agent runtime/base
  workflow/compiler and dependency changes. Finish on 48626a1 without merge/rebase; manager owns
  integration and independently reviewed combined checks. No phase pass implies integration.

Coordination SHA256: `d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371`.
