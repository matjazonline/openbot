# 02 — A bounded read for the whole board

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-run-finished-at.md),
[fix-01-run-agent-query](../fix-01-run-agent-query/01-agent-run-filter.md) and
[fix-02-run-child-duplicates](../fix-02-run-child-duplicates/01-top-level-filter.md).

## Outcome

The board's data comes from two port reads joined in the route, the way `run_page`
(`src/adapters/http/routes/workflows/ui/runs/mod.rs`) already joins `list_runs` with `Visibility`:
six columns — queued, running, waiting, succeeded, failed, cancelled — each with at most
`BOARD_COLUMN_CAP` cards and the number of matching runs the viewer may see. Finished columns hold
only runs finished inside the chosen window. The work done is bounded by open runs plus runs
finished in the window, never by retained history.

## Contract

- **Scope:** the list's scope filters (binding, agent, and the workflow filter new-board-view
  adds), always `RunLineage::TopLevel`, no state filter — the states are the columns.
- **Window:** `BoardWindow::{Day, Week, Month}`, default `Week` (the same seven days as Work's
  failed-run rule). The cutoff is computed from an `as_of` passed with every read and never kept
  between reads (`src/adapters/http/AGENTS.md`, relative-time filters).
- **Order:** open columns oldest first (`created_at, id`), so stuck runs surface; finished columns
  most recently finished first (`finished_at DESC, id DESC`).
- **Cap:** `BOARD_COLUMN_CAP = 50`. Cards are read one past the cap, so a column knows it was cut
  from the card read itself. Whoever raises the cap must leave 03's truncation report and tripwire
  working.

## Implementation

1. **`run_board_counts`** on the workflow reads port: matching runs grouped by
   `(state, channel_id)`, as open-run counts already do (`src/adapters/persistence/run_counts.rs:11`).
   Open states take no window; finished states take `finished_at >= cutoff`.
2. **Visibility in the route.** The handler resolves which of those channels the viewer may see
   with the existing per-channel `Visibility` (`src/adapters/http/routes/workflows/bindings.rs:103`;
   a run without a channel is the company association) and sums only visible channels. Totals are
   exact and say nothing about channels the viewer cannot see. Authorization stays out of
   persistence, as it does for the list.
3. **`run_board_cards`:** one statement with a `LATERAL` per state, restricted to the visible
   channels from step 2 (an array bounded by the company's channels, plus whether runs without a
   channel are visible), ordered per the contract, `LIMIT BOARD_COLUMN_CAP + 1`. Columns stay full
   instead of showing gaps for hidden runs.
4. **No transaction spans the two reads**, because the authorization step sits between them and a
   database transaction must not stay open across it. A run that changes between the reads can
   leave a column with more cards than its count, so the shown total is `max(total, shown)`. Every
   state change notifies `workflow_run_changed`, and the next wake-up reconciles.
5. **Index by evidence.** Add the count and card shapes (all, agent, binding; sparse and dense
   tenant) to `scripts/tests/run-list-query-plan.py`. Without an index on `finished_at` the window
   cannot prune history; expect a candidate such as
   `(company_id, state, finished_at DESC, id DESC) WHERE finished_at IS NOT NULL`, added only if
   the plans show the history scan. Open states can use `workflow_runs_open_by_channel` (init
   schema line 7322). Record the result in `docs/performance/run-list.md`.
6. Child counts for the shown cards come from
   [fix-02-run-child-duplicates 02](../fix-02-run-child-duplicates/02-child-rollup-on-parent.md)
   in one batch after the card read.

## Focused checks and acceptance

DB-backed tests named `run_board_read_*`:

- cap + 1 runs in one state: 50 cards shown, the column marked cut, a total of 51;
- runs finished just inside and just outside the window, relative to `as_of`;
- a retried run moves from the failed column to running;
- a run on a channel the viewer cannot see is in neither the total nor the cards, and the column
  is filled from visible runs;
- another company's runs never appear; agent scope and top-level apply;
- a run inserted between the count and card reads shows as `max(total, shown)` and is counted on
  the next read.

Run as in 01 with the `run_board_read_` filter; confirm a nonzero count and record the result.
