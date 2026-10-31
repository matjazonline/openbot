# 03 — Keep the board current

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-board-view-and-layout.md) and [02](02-filters-and-entry-points.md).

## Outcome

The board follows run changes through the existing run event stream and re-reads its own state on
every wake-up (`src/adapters/http/AGENTS.md`, "Make partial-page updates race-safe").

## Implementation

1. Reuse `/ui/workflow-runs/events` (`src/adapters/http/routes/workflows/ui/runs/events.rs`): it
   already sends `resync` first and on lag, `runs` and `run-<id>` for runs the caller may see, and
   `tick` every 30 s. No new stream.
2. An empty leaf inside `#run-board` re-reads `part=columns` with the current filters on
   `sse:runs`, `sse:resync` and `sse:tick`, with `hx-sync="#run-board:replace"` — the list's
   reload leaf in `run_list` (`src/adapters/http/pages/workflow_runs/mod.rs`) is the model.
   `tick` matters here: it moves finished runs out of the window and refreshes ages, because the
   cutoff is recomputed per read.
3. **Bound the refresh rate.** A burst of run changes must not cost one board read per event per
   viewer. Coalesce wake-ups to at most one read per viewer per 2 s, *and* always read after the
   last event of a burst. Plain htmx `throttle` drops that trailing event and `delay` can starve
   under constant change. Follow the throttled open-run snapshot
   (`src/adapters/http/routes/ui_run_counts.rs`) or an equivalent leaf script, and state the
   guarantee in its doc comment.
4. Extend the detached-response guard in `WORKFLOW_RUNS_SCRIPT`
   (`src/adapters/http/pages/workflow_runs/mod.rs:53-59`) to `#run-board`, so a late wake-up
   response cannot swap into a board that a filter change already replaced.
5. The pane keeps its own `run-<id>` wake-ups; a card moving column while its pane is open needs
   nothing new.

## Focused checks and acceptance

- Rust, `run_board_view_live_*`: a run changing between page render and subscription is
  reconciled by the first `resync`; a run on a channel the caller cannot see sends no wake-up; a
  finished run leaves the window on a read with a later `as_of`.
- Browser protocol, extending `scripts/tests/workflow-runs.mjs` (shipped htmx and run script):
  two board reads completing out of order, the later request winning; a filter change while a
  wake-up read is in flight; a burst of `runs` events producing no more than the bounded number of
  reads and still showing the final state.

```sh
node --test scripts/tests/workflow-runs.mjs
```

That file already runs in CI's "SSE navigation lifecycle" step and in `scripts/check.sh`. Record
the results.
