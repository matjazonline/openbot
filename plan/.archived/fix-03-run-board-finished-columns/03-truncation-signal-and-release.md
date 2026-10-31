# 03 — Report truncation, leave a tripwire, release

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-run-finished-at.md) and [02](02-bounded-board-read.md).

## Outcome

A capped column always says it is capped, and a slow or oversized board read logs the numbers
needed to decide what to optimize next. Operational views must report truncation rather than
silently returning partial data (`src/adapters/persistence/AGENTS.md`).

## Implementation

1. The read model carries per column the `cards` shown (at most the cap; the extra row read in 02
   only marks the column as cut), its `total` (visible, shown as `max(total, shown)`), and the
   `window` it was computed for. new-board-view renders "Showing 50 of 312" with a link to the list
   filtered by that state and scope. The list reads at most `MAX_LISTED` (200) rows and already
   says when older ones are cut (`src/adapters/http/pages/workflow_runs/mod.rs`,
   `run_list_entries`).
2. Add `warn_if_run_board_is_slow` and `RUN_BOARD_QUERY_WARN_THRESHOLD` (500 ms), timing both reads
   and the visibility step between them. Log `elapsed_ms`, `company_id`, `eligible_runs` (the
   working set: every count from `run_board_counts`, including channels the viewer cannot see),
   `returned_cards`, `channels_checked` and `window`. Unit-test the threshold decision as a pure
   function. Document how to read it in `docs/performance/run-list.md`: `eligible_runs` large and
   rising is a working-set problem; small with a high `elapsed_ms` points at pool contention or the
   visibility checks.
3. If that line shows a working-set problem, write a deferred rollup plan for the run board then.
   The task-board rollup plan it would replace was removed because the board it optimized is gone;
   its reasoning still applies — the live query is the rollup's specification and verification
   oracle, and a baseline is measured before anything is built:
   `git show 2396618:plan/db_improve/kanban_denormalized_rollup_table_optimization.md`.

## Final verification gate

Focused first, against a disposable migrated database: `run_finished_at_`, `run_board_read_`,
`run_top_level_` and `run_agent_filter_`, plus
`cargo test --locked --offline --lib routes::workflows::ui::runs::`. Then run `scripts/check.sh`
once (formatting, offline compilation, clippy, migrations, sqlx metadata, database-backed suite).
Record commands and results, and name any gate that could not run.
