# 03 — Workflow database guards

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Part of [fix-app](README.md). Starts after [02](02-silent-stalls.md) merges, because it classifies
the completion guard that 02 edits and uses 02's continuation limit.
[06](06-workflow-storage-growth.md) starts after this one.

## Outcome

- Run state changes are guarded in the database, as execution and decision changes already are.
- A database guard violation becomes a typed, counted, terminal outcome. Today it is a generic
  database error that is retried until the deadline.
- Root budgets no longer depend on SQL parsing the compiled bundle.

## Batches

Take batch 1 first: it makes failures in the later batches visible.

### 1 — Typed guard errors

**Today:**
- `From<sqlx::Error>` (`src/adapters/persistence/mod.rs:103-115`) maps every SQLSTATE except 55P03
  and 57014 to `AppError::Database`.
- Errors raised at COMMIT by the deferred workflow guards are therefore indistinguishable from an
  outage. Examples: "completion lost ownership", "token approval required", "park lost its fence",
  and the reopen refusal at `init_schema.sql:265`.
- The worker logs a warning, the lease expires, and the job retries. Only
  `workflow/decisions.rs:76` inspects 23514.
- Effect: any disagreement between Rust and SQL, like the one 02 fixes, spins silently until the
  run deadline.

**Change:**
- Give the workflow guard `RAISE`s an identifiable SQLSTATE or a stable constraint name.
- Map them to a distinct error variant that carries the guard's name.
- The workflow worker treats that variant as terminal for the job: it fails the run with a reason
  naming the guard, and records a failure metric. To find where existing workflow failure metrics
  go, use `graft ask "workflow failure metric"`.

**Tests:**
- `workflow_guard_error_*`: a forced guard violation fails the run once, with the guard's name, and
  is not reclaimed on the next poll.
- An ordinary connection error is still retried.

### 2 — Run state transitions

**Today:**
- **Writers are unguarded.** 26 `UPDATE workflow_runs SET state=` statements across 17 files
  change run state, most of them with no state predicate.
- **No trigger restricts the state.** The triggers on `workflow_runs` are
  `workflow_action_capture_claim_retirement`, `workflow_action_capture_state_witness`,
  `workflow_allocate_wakeup`, `workflow_parent_immutable` and `stamp_workflow_run_finished`.
  Executions and decisions, by contrast, are guarded (`preserve_workflow_progression`,
  `workflow_decision_immutable`).
- **Rust and SQL disagree about terminal states.**
  - The domain `RunState` (`src/domain/workflow/state.rs:43`) treats terminal states as final, yet
    retry moves failed → running (`src/adapters/persistence/workflow/control_retry.rs:40`).
  - SQL would also accept cancelled → running.
- **`terminal_execution_id` has no shape constraint.**
  - It is set by `maintenance.rs:44`, `recovery.rs:195` and `decision_tokens.rs:160`, but not by
    `wait_commit.rs:306` or `:394`.
  - Retry eligibility (`control_retry.rs:19`) depends on it.

**Change:**
- **State the transition table once, in Rust.** Add `RunState::can_transition_to`, covering the
  retry edge.
- **Mirror it in SQL.** Add a `BEFORE UPDATE OF state` trigger, `guard_workflow_run_transition`,
  with the same table. Allow the retry edge only where `check_workflow_control_retry` already
  permits it.
- **Make the trigger fire first.** Name it so it runs before `stamp_workflow_run_finished`, since
  same-event triggers fire in name order.
- **Equivalence test.** Add a DB-backed matrix: for every (from, to) pair, SQL accepts the update
  exactly when `can_transition_to` says so (`src/adapters/persistence/AGENTS.md`, "Encode queue state
  machines as constraints").
- **`terminal_execution_id`:**
  - First inventory its writers and decide when it is required, for example on every failed run
    that has a failing execution.
  - Add a CHECK for that rule, and fix the writers that miss it.

**Tests:**
- `run_transition_*`: the matrix, plus a cancelled → running update that is rejected.
- Rerun the run control suites (`cargo test --locked --offline --lib routes::workflows::ui::runs::`).

### 3 — Root budgets come from Rust

**Today:**
- **SQL parses the bundle.** The `insert_workflow_budget_link` trigger (`init_schema.sql:615-645`)
  decodes `workflow_runs.bundle` from bytea to jsonb and reads root budget limits only when
  `compiler_revision = '6'` (`:629`), mirroring `SEMANTIC_REVISION = 6`
  (`src/application/workflow/compiler/mod.rs:9`).
- **Any other bundle gets `sealed_legacy`.** It then has no limits, and the token check reports
  model calls as exhausted (`src/adapters/persistence/workflow/token_budget.rs:94`).
- **Effect:** after the next compiler bump, every new run is refused at its first model call.
- **It is also a legacy path.** `sealed_legacy` exists for "old raw schema fixtures", a
  compatibility path the no-backward-compatibility rule forbids.

**Change:**
- Admission (`src/adapters/persistence/workflow/admission_write.rs`) inserts the root run's
  `workflow_root_budgets` and `workflow_root_budget_usage` rows from the compiled limits it already
  holds, in the same transaction.
- The trigger keeps only the run-to-root link for child runs.
- Drop `sealed_legacy`, and the provenance column if nothing else needs it (`:475`, `:2546`,
  `:5458-5460`).
- Delete or rewrite the legacy test (`budget_schema_tests.rs:219`).

**Tests:**
- `root_budget_admission_*`: a root run's budget equals its compiled limits.
- A child run links to its root.
- `grep -n compiler_revision migrations/` finds nothing.

### 4 — Smaller mismatches

- **Lease renewal.** The renewal statement (`src/adapters/persistence/workflow/lease.rs:85`)
  updates by id and expiry only. It is safe today only because `lock_fence` runs first. Add the
  execution generation to its `WHERE`.
- **Delivery terminal states.**
  - `DeliveryStatus::is_terminal` (`src/domain/entities/transport/delivery.rs:111`) counts
    `outcome_unknown` as terminal.
  - Yet `workflow_message_delivery_transition_guard` (around `init_schema.sql:3092-3125`) lets a
    workflow delivery leave `outcome_unknown` once reconciliation evidence arrives.
  - Rename the method to what its callers mean (`graft callers is_terminal`), or take
    `OutcomeUnknown` out of it.
- **Investigate before fixing.**
  - `refuse_expired_workflow_tokens` (`init_schema.sql:11031`) sets the root's `token_refusal`
    whenever a token-allowance wait is expired or cancelled.
  - `cancel_run_on` (`controls.rs:204`) cancels a child run's waits in bulk.
  - First write a test: cancel a child parked on a token decision, then try a token-consuming step
    in its parent.
  - Fix it only if that test fails: refuse on expiry alone, or only within the cancelled run.

## Focused checks

```sh
createdb mail_agents_fix_app_03
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_fix_app_03" \
  cargo test --locked --offline --lib workflow_guard_error_
# repeat with run_transition_, root_budget_admission_, budget_, decision_tokens, control_retry
dropdb mail_agents_fix_app_03
```

This plan changes shared workflow infrastructure, so run the whole workflow persistence suite after
batch 2 (`cargo test --locked --offline --lib adapters::persistence::workflow::`). Confirm a nonzero
test count for each filter.

## Final gate

Run `./scripts/reset-db.sh --all`, then `scripts/check.sh` once. Record the commands and results,
and name any gate that could not run.
