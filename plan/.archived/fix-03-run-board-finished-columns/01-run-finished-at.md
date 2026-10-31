# 01 — Record when a run finished

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Third of the run board plans, after
[fix-02-run-child-duplicates](../fix-02-run-child-duplicates/01-top-level-filter.md); see
[new-board-view](../new-board-view/README.md) for the order.

## Outcome

Every run in a terminal state carries `finished_at`, the moment it got there, enforced by the
database. The board's finished columns can then be windowed by finishing time.

## Why not `created_at`

`workflow_runs` has only `created_at`, and the Work page's failed-run rule windows on it
(`src/adapters/persistence/attention.rs:196`). That misplaces runs on a board:

- Retry takes a failed run back to `running` (`src/adapters/persistence/workflow/control_retry.rs:40`):
  a run created ten days ago that failed again today falls outside a seven-day window.
- A run that waited long on a decision and then succeeded leaves the window the moment it finishes.

## Implementation

1. Add `finished_at timestamptz` to `workflow_runs` in the init schema, with CHECK
   `workflow_run_finished_shape`:
   `(state IN ('succeeded','failed','cancelled')) = (finished_at IS NOT NULL)`.
2. Add a `BEFORE INSERT OR UPDATE OF state` trigger, `stamp_workflow_run_finished`: entering a
   terminal state from a non-terminal one sets `clock_timestamp()`; staying terminal keeps the old
   value; leaving one (retry) clears it. Writers never set the column, so no `UPDATE … SET state`
   call site changes, and the CHECK catches a path the trigger misses. Confirm it does not depend
   on ordering against the existing `BEFORE UPDATE` triggers on `workflow_runs` (revision,
   action-capture witnesses).
3. Carry it through the read side together (`src/adapters/persistence/AGENTS.md` §3):
   `RUN_COLUMNS` (`src/adapters/persistence/workflow/reads.rs:116`), `RunRow`, `run_summary`, and
   `RunSummary.finished_at: Option<DateTime<Utc>>` (`src/application/workflow/reads.rs:87`). Fix
   the struct literals this breaks, driven by `cargo check --all-targets`.
4. Reset: `./scripts/reset-db.sh --all`, then `psql "$DATABASE_URL" -c '\d workflow_runs'`.

## Focused checks and acceptance

DB-backed tests named `run_finished_at_*`: success, failure and cancellation each stamp it;
queued, running and waiting rows have none; retry clears it and a second failure stamps a later
time; an `UPDATE` to a terminal state that tries `finished_at = NULL` is still stamped; a row
violating the CHECK is rejected. Then rerun the run control suites that change state
(`cargo test --locked --offline --lib routes::workflows::ui::runs::`) and
`cargo sqlx prepare --check -- --all-targets`.

```sh
createdb mail_agents_run_finished
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_run_finished" \
  cargo test --locked --offline --lib run_finished_at_
dropdb mail_agents_run_finished
```

Confirm a nonzero test count and record the result.
