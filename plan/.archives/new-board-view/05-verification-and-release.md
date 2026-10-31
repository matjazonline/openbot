# 05 — Verify and release

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-board-view-and-layout.md) through [04](04-runs-in-navigation.md).

## Implementation

1. Browser pass on a seeded company with runs in every state, a parent with a waiting child, more
   than 50 runs in one column, a channel the viewer cannot see, and two agents. Check the
   single-agent board from the agent pane, the List/Board toggle, each filter, the window, the
   "open in list" link, a card moving live after cancel or retry from the pane, the rail's other
   links unchanged, a Dashboard job opening its run, and both themes. Record what was checked; if
   the pass cannot run, say so.
2. Confirm the tripwires from the fix plans are in place — `warn_if_run_list_is_slow` and
   `warn_if_run_board_is_slow` — and that `docs/performance/run-list.md` says how to read them
   ([fix-03-run-board-finished-columns 03](../fix-03-run-board-finished-columns/03-truncation-signal-and-release.md)).
3. When this plan and its prerequisites are done, remove their entries from
   `NO_BACKWARD_COMPATIBILITY_NOTES` in `src/main.rs` along with the plan directories, as was done
   for the earlier `fix-*` plans.

## Final verification gate

Focused suites first, against a disposable migrated database: `run_agent_filter_`,
`run_top_level_`, `run_finished_at_`, `run_board_read_`, `run_board_view_`, `icon_rail_`,
`dashboard`, and
`cargo test --locked --offline --lib routes::workflows::ui::runs::`. Then run `scripts/check.sh`
once: generated CSS, the SSE and browser node tests, formatting, patch whitespace, offline
compilation, clippy, migrations, sqlx metadata, the database-backed suite, and the stack and lease
budgets. The query-plan script is not a CI gate: rerun it if any run-list or board SQL constant
changed after its evidence was recorded, and update `docs/performance/run-list.md`. Record
commands and results, and do not claim full verification when a gate could not run.
