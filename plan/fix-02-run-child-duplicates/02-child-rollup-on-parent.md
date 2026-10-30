# 02 — Show a parent's children on its card

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-top-level-filter.md).

## Outcome

Leaving children off the board does not hide their work: a parent card carries its direct
children counted by state ("2 child runs · 1 waiting"), read in one batch for every card shown.

## Implementation

1. Add `child_rollup(company, parents: &[RunId]) -> HashMap<RunId, ChildCounts>` to the workflow
   reads port: one grouped statement over `workflow_run_parents` (indexed by
   `workflow_run_parents_by_parent`, init schema line 7328) joined to `workflow_runs` on
   `(company_id, child_run_id)`, grouped by parent and state. No per-card query
   (`src/adapters/persistence/AGENTS.md`, "Replace per-parent query loops with bounded batch
   reads"). The input is bounded by the cards one board read shows — six columns of
   `BOARD_COLUMN_CAP`
   ([fix-03-run-board-finished-columns 02](../fix-03-run-board-finished-columns/02-bounded-board-read.md))
   — so refuse a larger input rather than cutting it silently.
2. Direct children only; a grandchild is reachable from its parent's pane. Say so on
   `ChildCounts`.
3. Visibility: establish whether a child can carry a different channel association than its
   parent (`insert_run` writes the association per run,
   `src/adapters/persistence/workflow/admission_write.rs:94`). If a child always inherits it, the
   parent's visibility covers the counts — pin that with a test. If it can differ, count only
   children on channels the viewer may see, with the same per-channel `Visibility` cache the list
   uses (`src/adapters/http/routes/workflows/bindings.rs:103`).
4. Rendering contract for new-board-view: total children, open (queued, running, waiting) and
   failed counts. Opening the card shows the pane's existing children section; the pane does not
   change.

## Focused checks and acceptance

DB-backed tests named `run_top_level_rollup_*`: no children gives no entry; children in mixed
states are counted per state; another company's run ids in the input return nothing; a grandchild
is not counted on the root; an input above the cap is refused. Run as in 01 with the
`run_top_level_` filter, which includes these, and record the count.
