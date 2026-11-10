# 02 — Silent stalls

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Part of [fix-app](README.md). No prerequisites. [03](03-workflow-database-guards.md) starts after
this merges, and [08](08-schema-cleanup.md) constrains the queues changed here.

## Outcome

No single row, and no guard mismatch, can keep work spinning until a deadline. Each case below
either refuses up front or reaches an observable terminal state. The four batches are independent,
so take them in any order.

## 1 — The 64th email send stalls the run

### Today

- **Every other park checks the limit.** Approvals, token pauses and workflow tool calls refuse to
  park a job that has used its 63 continuations:
  - `decision_approvals.rs:56`, `:126`;
  - `decision_tokens.rs:35`;
  - `workflow_tool_calls.rs:118-123`, which returns a tool-limit error to the agent.
- **The email send doesn't.** `park_on` (`src/adapters/persistence/workflow/message_publish.rs:268-293`)
  parks with no check, and the SQL completion guard's `effect_parked` branch
  (`init_schema.sql:180-187`) has no limit either. The approval and child branches do (`:189`,
  `:199`).
- **The resume can never succeed.** `continue_delivery` (`wait_commit.rs:121-133`) calls
  `reopen_job` (`decision_approvals.rs:257-262`), which requires `continuation_count < 63`. It
  fails, and the whole resume rolls back.
- **The failure repeats.** Polling re-selects the wait on every pass, with no backoff, until the
  24-hour run deadline fails the run.
- **It is easy to reach.** The default `tool_calls` budget is 64, so an agent whose last tool call
  is an email hits it.
- **The limit is written in eight places:** the four SQL strings and one Rust comparison above,
  plus `init_schema.sql:189`, `:199`, `:211` and the CHECK at `:3338`.

### Change

- **Define the limit once.**
  - Add an `IMMUTABLE` SQL function `workflow_job_continuation_limit()` that returns 63.
  - Use it in the CHECK, the three guard sites and the four SQL strings.
  - The Rust comparison uses a constant. A DB test asserts that the constant equals the function.
- **Refuse before sending.** At the top of `publish_on`, read the job's `continuation_count`
  alongside the other facts it loads. When none remain, return the same tool-limit observation
  `workflow_tool_calls.rs:118-123` returns. Nothing is written and no email goes out.
- **Guard the park in SQL too.** Add `OLD.continuation_count < workflow_job_continuation_limit()` to
  the `effect_parked` branch.

### Tests, `message_park_continuation_*` (DB-backed)

- At the limit, the send returns the tool-limit error and creates no intent, delivery or wait.
- One below the limit, the send parks, the receipt resumes the job, and the step continues.
- A hand-written effect park at the limit is rejected by the guard.

## 2 — One bad schedule snapshot fails the whole claim page

### Today

- **The column holds a whole entity.** `schedule_runs.schedule_snapshot` (`init_schema.sql:4067`,
  CHECK `:4093`) holds the entire `ChannelSchedule`, written by `serde_json::to_value(&schedule)`
  (`src/adapters/persistence/workflow/schedules.rs:640`). The payload rule in
  `src/adapters/persistence/AGENTS.md` (§2) forbids storing an entity in a durable payload.
- **The decode happens after the commit.** `claim_due` commits the claim (`schedules.rs:709`) and
  only then decodes every row strictly, dropping the error context (`:113-121`, `:710`).
- **Effect:**
  - One row that no longer decodes fails the whole page.
  - Up to 100 healthy occurrences stay leased, and each loses an attempt.
  - Because the bad row sorts first, the same page is reclaimed together until every occurrence on
    it fails.
- **Only three ids are used.** Admission reads only `company_id`, `channel_id` and `id` from the
  snapshot (`src/application/workflow/schedule_admission.rs:194`, `:343-360`). The row already
  stores all three as columns.

### Change

- Drop the column and its CHECK.
- `ClaimedScheduleRun` carries `schedule_id`, `company_id` and `channel_id` from the row.
- Remove the snapshot write at `:640`.
- Grep `schedule_snapshot` for any other reader.

### Tests, `schedule_claim_*`

- A claim page of N occurrences returns N.
- A poison-batch test, as `src/application/AGENTS.md` requires: fill a batch with occurrences whose
  admission fails, run two consecutive iterations without advancing time, and show that the second
  does not reclaim the same rows.

## 3 — A slow provider turns successful sends into `outcome_unknown`

### Today

- **The completion check requires a live lease.** `complete_part`
  (`src/adapters/persistence/delivery/queue.rs:249-280`) fences on `live_fence_sql` (`:808-815`),
  which requires `lock_expires_at > now`.
- **This contradicts the documented intent.** `own_row_fence_sql` (`:817-829`) says a late provider
  outcome must still be recordable, and that the reaper cannot take a row while this execution's id
  is still on it.
- **Leases do expire under load.** The worker claims 20 deliveries (`queue.rs:55`) under one
  120-second lease (`:51`), sends them in sequence (`src/application/services/delivery_worker.rs:121-136`),
  and never renews. `renew_delivery_lease` (`queue.rs:147`) has no production caller.
- **Effect:** under a backlog or a slow provider, an accepted send returns `LeaseLost` and rolls
  back. The reaper then marks it `outcome_unknown`, which is never retried. A human has to clear it,
  and the provider's message key is lost.

### Change

- **Record the outcome.** Fence `complete_part` on this execution and status `sending`, without the
  expiry check, the way `lock_held_delivery_on` already does. Before relying on the reaper claim,
  confirm it by reading `reap_expired_deliveries`: does it clear `execution_id` or change the status
  when it takes a row?
- **Stop leases expiring.** Renew the lease before each delivery in the batch, and stop that
  delivery when renewal reports it lost.

### Tests, `delivery_late_completion_*`

- A delivery whose lease expired but was not reaped records `delivered` and the provider key.
- A delivery the reaper took first returns `LeaseLost` and stays `outcome_unknown`.
- With a short lease, the worker renews before each delivery and the last delivery in the batch
  completes.

## 4 — A notification can stay `pending` forever

### Today

- **Each claim charges an attempt.** The claim requires `attempt_count < max_attempts` and
  increments it (`src/adapters/persistence/notification.rs:174-180`).
- **The stale path skips the dead-letter rule.** `release_stale_projection` (`:529-548`) sets
  `pending` with `available_at = now`, without the dead-letter rule that the fail path (`:268`) and
  the reaper (`:292`) both apply.
- **Effect:** if the 10th attempt comes back stale, the row stays `pending` at 10/10. It is never
  claimed again and never dead-lettered, the census counts it forever, and the user is never
  notified.

### Change

- In `release_stale_projection`, apply the same `attempt_count >= max_attempts → dead_letter` rule
  as the fail path.
- Set `available_at` to the fail path's backoff instead of now, so a source that keeps changing
  cannot hot-loop.
- A stale projection is legitimate work, but it still has to reach a terminal state
  (`src/application/AGENTS.md`).

### Tests, `notification_stale_*`

- An event at the last attempt that comes back stale ends `dead_letter`.
- Below the limit, it returns to `pending` with a future `available_at`.
- The poison-batch two-iteration test.

## Focused checks

Run each batch on its own database (run `./scripts/reset-db.sh --all` after an init-schema edit):

```sh
createdb mail_agents_fix_app_02
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_fix_app_02" \
  cargo test --locked --offline --lib message_park_continuation_
# repeat with schedule_claim_, delivery_late_completion_, notification_stale_
dropdb mail_agents_fix_app_02
```

Also rerun each batch's existing neighbours:
- the workflow decision and tool-call suites after batch 1 (`decision_approvals`,
  `workflow_tool_calls`);
- `schedules` after batch 2;
- `delivery` after batch 3;
- `notification` after batch 4.

Confirm a nonzero test count for each filter.

## Final gate

Run `./scripts/reset-db.sh --all`, then `scripts/check.sh` once. Record the commands and results,
and name any gate that could not run.
