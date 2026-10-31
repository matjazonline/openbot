# New board view — runs by state, company-wide

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Goal

Answer "what is queued, running, waiting or failed right now, per workflow or agent" in one place,
and make it reachable from the rail. Today nothing does:

- **Work** (`/ui/work`) lists only what needs a person.
- **Dashboard** (`/ui/dashboard`) counts step jobs in `background_tasks`, not runs.
- **The run list** (`/ui/workflow-runs`) is a feed filtered by binding and state, one run at a
  time. It is not in the rail: it is reached from the Workflows page "Runs" button
  (`src/adapters/http/pages/workflow_authoring/mod.rs:57`), a binding's "runs" link
  (`src/adapters/http/pages/workflow_authoring/bindings.rs:215`), or run links in conversations.

The tasks Kanban (`/ui/tasks?view=board`) went away with the task system in `5d4aab1`.

## Decisions

- The board is a view of the runs workspace: `/ui/workflow-runs?view=board`. Without `view` the
  workspace shows the list, so every existing link keeps working. Nothing gets a separate path;
  instead the rail learns to build links that carry a query ([04](04-runs-in-navigation.md)).
- Six columns, one per run state. Top-level runs only; a parent card summarizes its children.
- Scope filters: binding, workflow, agent — an agent filter is the single-agent board. The list
  gets the same scope filters; the board adds a window for finished columns.
- Finished columns are windowed by `finished_at` (default seven days); every column is capped at
  50 cards and says "Showing N of M" when capped.
- A "Runs" rail entry opens the board, for the members who can open Workflows.

## Order

The prerequisites are backend plans, each verified on its own before this one starts:

1. [fix-01-run-agent-query](../.archives/fix-01-run-agent-query/01-agent-run-filter.md) — agent filter,
   query-plan evidence, slow-list tripwire.
2. [fix-02-run-child-duplicates](../.archives/fix-02-run-child-duplicates/01-top-level-filter.md) — top-level
   filter, child rollup.
3. [fix-03-run-board-finished-columns](../.archives/fix-03-run-board-finished-columns/01-run-finished-at.md) —
   `finished_at`, the bounded board read, truncation report and tripwire.
4. This plan:
   [01](01-board-view-and-layout.md) view and layout ·
   [02](02-filters-and-entry-points.md) filters and entry points ·
   [03](03-live-updates.md) live updates ·
   [04](04-runs-in-navigation.md) navigation ·
   [05](05-verification-and-release.md) verification and release.

## Out of scope

Dragging cards between columns — run state belongs to the engine, and cancel, retry and rerun stay
in the pane. Swimlanes, saved views, and an operator board across companies.
