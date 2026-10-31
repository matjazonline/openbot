# 02 — Filters and the ways in

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-board-view-and-layout.md).

## Outcome

Board and list share the scope filters — binding, workflow, agent — and the board adds a window.
The places that name an agent or a workflow link to its board.

## Implementation

1. **Workflow filter.** Add `workflow: Option<WorkflowId>` to `RunFilter`
   (`AND workflow_id = $n`), resolved company-scoped through a definition read; another tenant's
   id is not found. Add its list and board shapes to `scripts/tests/run-list-query-plan.py` and
   decide on an index as [fix-01-run-agent-query 02](../.archives/fix-01-run-agent-query/02-query-plan-evidence-and-index.md)
   does, recording the result in `docs/performance/run-list.md`.
2. **Query parameters** on both views: `binding_id`, `workflow_id`, `agent_id`; the board also
   takes `window` (`day`, `week`, `month`). Extend `WorkspaceQuery` and `ListQuery`
   (`src/adapters/http/routes/workflows/ui/runs/mod.rs:44-60`). An unknown window or state falls
   back as `state_filter` does; an unknown or foreign *id* is not found, never ignored.
3. **One query builder** for scope (extend `RunFilterView::query` in
   `src/adapters/http/pages/workflow_runs/mod.rs`), so the List/Board toggle — which only changes
   `view` — "open in list" and "Load more" keep the same scope.
4. **Controls** in the board toolbar and the list sidebar: selects for workflow and agent, bounded
   like the binding form's options (`MAX_BINDING_OPTIONS`); when cut, say so rather than dropping
   options silently. Each control uses `hx-sync="#run-board:replace"` (or `#run-list`) so the
   last choice wins.
5. **Entry points:**
   - the agent pane's "Used by" card (`agent_usage_card`,
     `src/adapters/http/pages/workflow_authoring/tools.rs:301`) links to
     `/ui/workflow-runs?company_id=…&view=board&agent_id=…` — the single-agent board;
   - a workflow in the library links to its board by `workflow_id`;
   - the Workflows page "Runs" button (`src/adapters/http/pages/workflow_authoring/mod.rs:57`)
     opens the board (`view=board`);
   - a binding's "runs" link keeps opening the list filtered by binding.

## Focused checks and acceptance

Route tests named `run_board_view_filter_*`: each filter alone and combined, on both views;
another tenant's workflow, agent or binding id is not found; the toggle preserves scope; the
agent pane link opens a board scoped to that agent; an option list past its bound reports the
cut. JSON route parity for `workflow_id`. Run as in 01 with the `run_board_view_` filter, plus
`run_agent_filter_`; record the results.
